//! One-time password reset codes: issued by Root or a store Manager, redeemed from the sign-in page.

use serde::{Deserialize, Serialize};
use surrealdb::opt::auth::Record;

use super::{User, UserAuthorization, normalize_email, signup::strip_thrown_prefix};
use crate::schema::{Datetime, RecordId, SurrealValue};
use crate::{DB, NS, db};

/// Digits in a reset code.
pub const RESET_CODE_DIGITS: usize = 6;
/// Shortest password a reset accepts; the access method enforces the same.
pub const MIN_PASSWORD_CHARS: usize = 8;
/// Wrong guesses a code survives; the access method enforces the same.
pub const RESET_MAX_ATTEMPTS: i64 = 5;
/// Record access whose SIGNIN redeems a code.
const RESET_ACCESS: &str = "password_reset";

/// Why issuing or redeeming a code failed; `Display` is user-facing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResetError {
    #[error("You can't issue a reset code for this account.")]
    NotAllowed,
    #[error("That code is wrong, expired or already used. Ask for a new one.")]
    InvalidCode,
    #[error("{0}")]
    Rejected(String),
    #[error("Could not reach the database; try again.")]
    Unreachable,
    #[error("{0}")]
    Failed(String),
}

impl From<surrealdb::Error> for ResetError {
    fn from(err: surrealdb::Error) -> Self {
        if err.is_connection() {
            Self::Unreachable
        } else {
            Self::Failed(err.to_string())
        }
    }
}

/// Uniform six-digit code from the OS random source.
pub fn generate_reset_code() -> String {
    const SPAN: u32 = 1_000_000;
    // Largest multiple of SPAN that fits in a u32; draws at or above it are redrawn.
    const LIMIT: u32 = u32::MAX - u32::MAX % SPAN;
    loop {
        // The low 32 bits of a v4 UUID carry no version or variant bits.
        let draw = (uuid::Uuid::new_v4().as_u128() & u128::from(u32::MAX)) as u32;
        if draw < LIMIT {
            return format!("{:0width$}", draw % SPAN, width = RESET_CODE_DIGITS);
        }
    }
}

/// The code's digits with spaces and dashes removed, or `None` when it is not six digits.
pub fn normalize_reset_code(input: &str) -> Option<String> {
    let digits: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    (digits.len() == RESET_CODE_DIGITS && digits.chars().all(|c| c.is_ascii_digit()))
        .then_some(digits)
}

/// Whether `issuer` may give `target` a code; mirrors the `password_reset` create rule.
pub fn may_issue_reset(issuer: &User, target: &User) -> bool {
    if !issuer.is_active() || issuer.get_id() == target.get_id() {
        return false;
    }
    match issuer.get_authorization() {
        UserAuthorization::Root => true,
        UserAuthorization::Manager => {
            issuer.get_store() == target.get_store()
                && !matches!(
                    target.get_authorization(),
                    UserAuthorization::Root | UserAuthorization::Manager
                )
        }
        UserAuthorization::User | UserAuthorization::Warehouse => false,
    }
}

/// Active accounts `issuer` may give a code, by name.
pub fn reset_targets<'a>(issuer: &User, users: &'a [User]) -> Vec<&'a User> {
    let mut targets: Vec<&User> = users
        .iter()
        .filter(|u| u.is_active() && may_issue_reset(issuer, u))
        .collect();
    targets.sort_by_key(|u| u.get_name().to_lowercase());
    targets
}

/// A code just issued; the plaintext exists only here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCode {
    pub code: String,
    pub expires_at: Datetime,
}

#[derive(Debug, Deserialize, SurrealValue)]
struct IssuedRow {
    expires_at: Datetime,
}

/// Creates a code for `target`; the table's create rule decides who may.
pub async fn issue_reset_code(target: &RecordId) -> Result<IssuedCode, ResetError> {
    let code = generate_reset_code();
    let rows: Vec<IssuedRow> = db()
        .query(
            "CREATE password_reset SET user = $user, code_hash = crypto::argon2::generate($code) \
             RETURN expires_at",
        )
        .bind(("user", target.clone()))
        .bind(("code", code.clone()))
        .await?
        .take(0)?;
    let row = rows.into_iter().next().ok_or(ResetError::NotAllowed)?;
    Ok(IssuedCode {
        code,
        expires_at: row.expires_at,
    })
}

/// Deletes `target`'s unused codes; returns how many went.
pub async fn revoke_reset_codes(target: &RecordId) -> Result<usize, ResetError> {
    let rows: Vec<surrealdb::types::Value> = db()
        .query("DELETE password_reset WHERE user = $user AND used_at = NONE RETURN BEFORE")
        .bind(("user", target.clone()))
        .await?
        .take(0)?;
    Ok(rows.len())
}

/// The newest code issued for an account, as its issuer or Root sees it.
#[derive(Debug, Clone, PartialEq, Deserialize, SurrealValue)]
pub struct ResetCodeStatus {
    pub created_at: Datetime,
    pub expires_at: Datetime,
    pub used_at: Option<Datetime>,
    pub attempts: i64,
    pub issued_by_name: Option<String>,
}

impl ResetCodeStatus {
    /// Still redeemable at `now`.
    pub fn is_live(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.used_at.is_none() && self.attempts < RESET_MAX_ATTEMPTS && *self.expires_at > now
    }
}

/// Query reading the newest code for `$id`.
pub(crate) const LATEST_CODE_SQL: &str = "SELECT created_at, expires_at, used_at, attempts, \
     issued_by.name AS issued_by_name FROM password_reset WHERE user = $id \
     ORDER BY created_at DESC LIMIT 1;";

#[derive(Serialize, SurrealValue)]
struct RedeemParams {
    email: String,
    code: String,
    new_password: String,
}

/// Sets `new_password` for `email` if `code` is its live reset code; the session it opens is dropped.
pub async fn redeem_reset_code(
    email: &str,
    code: &str,
    new_password: &str,
) -> Result<(), ResetError> {
    let email = normalize_email(email)
        .ok_or_else(|| ResetError::Rejected("Enter a valid email or username.".to_string()))?;
    let code = normalize_reset_code(code).ok_or(ResetError::InvalidCode)?;
    if new_password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ResetError::Rejected(format!(
            "The new password must be at least {MIN_PASSWORD_CHARS} characters."
        )));
    }
    crate::connect_and_use().await;
    let dbh = db();
    let signed = dbh
        .signin(Record {
            namespace: NS.to_string(),
            database: DB.to_string(),
            access: RESET_ACCESS.to_string(),
            params: RedeemParams {
                email: email.clone(),
                code,
                new_password: new_password.to_string(),
            },
        })
        .await;
    if let Err(e) = dbh.invalidate().await {
        log::warn!("Dropping the reset session failed: {e}");
    }
    signed.map(|_| ()).map_err(|e| {
        log::warn!("Password reset for {email} refused: {e}");
        redeem_error(&e)
    })
}

/// User-facing reason a redeem failed.
fn redeem_error(err: &surrealdb::Error) -> ResetError {
    if err.is_thrown() {
        return ResetError::Rejected(strip_thrown_prefix(err.message()).to_string());
    }
    if err.is_connection() {
        return ResetError::Unreachable;
    }
    ResetError::InvalidCode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Store;

    fn user(key: &str, role: UserAuthorization, store: Store, active: bool) -> User {
        User {
            id: RecordId::new("user", key),
            authorization: role,
            store,
            active,
            name: key.to_string(),
            ..User::default()
        }
    }

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..200 {
            let code = generate_reset_code();
            assert_eq!(code.len(), RESET_CODE_DIGITS, "{code}");
            assert!(code.chars().all(|c| c.is_ascii_digit()), "{code}");
        }
    }

    #[test]
    fn codes_vary() {
        let codes: std::collections::HashSet<String> =
            (0..50).map(|_| generate_reset_code()).collect();
        assert!(
            codes.len() > 40,
            "only {} distinct codes in 50",
            codes.len()
        );
    }

    #[test]
    fn typed_codes_normalize() {
        assert_eq!(normalize_reset_code(" 123 456 ").as_deref(), Some("123456"));
        assert_eq!(normalize_reset_code("123-456").as_deref(), Some("123456"));
        assert_eq!(normalize_reset_code("12345"), None);
        assert_eq!(normalize_reset_code("1234567"), None);
        assert_eq!(normalize_reset_code("12345a"), None);
        assert_eq!(normalize_reset_code(""), None);
    }

    #[test]
    fn root_may_reset_anyone_but_themselves() {
        let root = user("root", UserAuthorization::Root, Store::RIV, true);
        for (role, store) in [
            (UserAuthorization::User, Store::LTN),
            (UserAuthorization::Manager, Store::MUR),
            (UserAuthorization::Root, Store::RIV),
            (UserAuthorization::Warehouse, Store::WAR),
        ] {
            assert!(
                may_issue_reset(&root, &user("t", role.clone(), store, true)),
                "{role:?}"
            );
        }
        assert!(!may_issue_reset(&root, &root));
    }

    #[test]
    fn a_manager_resets_only_regular_staff_in_their_store() {
        let mgr = user("mgr", UserAuthorization::Manager, Store::RIV, true);
        assert!(may_issue_reset(
            &mgr,
            &user("a", UserAuthorization::User, Store::RIV, true)
        ));
        assert!(may_issue_reset(
            &mgr,
            &user("b", UserAuthorization::Warehouse, Store::RIV, true)
        ));
        assert!(!may_issue_reset(
            &mgr,
            &user("c", UserAuthorization::User, Store::LTN, true)
        ));
        assert!(!may_issue_reset(
            &mgr,
            &user("d", UserAuthorization::Manager, Store::RIV, true)
        ));
        assert!(!may_issue_reset(
            &mgr,
            &user("e", UserAuthorization::Root, Store::RIV, true)
        ));
        assert!(!may_issue_reset(&mgr, &mgr));
    }

    #[test]
    fn inactive_issuers_and_regular_staff_cannot_reset() {
        let target = user("t", UserAuthorization::User, Store::RIV, true);
        let off = user("off", UserAuthorization::Root, Store::RIV, false);
        assert!(!may_issue_reset(&off, &target));
        for role in [UserAuthorization::User, UserAuthorization::Warehouse] {
            assert!(!may_issue_reset(
                &user("x", role, Store::RIV, true),
                &target
            ));
        }
    }

    #[test]
    fn targets_skip_inactive_accounts_and_sort_by_name() {
        let mgr = user("mgr", UserAuthorization::Manager, Store::RIV, true);
        let users = vec![
            user("zed", UserAuthorization::User, Store::RIV, true),
            user("amy", UserAuthorization::User, Store::RIV, true),
            user("gone", UserAuthorization::User, Store::RIV, false),
            user("far", UserAuthorization::User, Store::SAN, true),
            mgr.clone(),
        ];
        let names: Vec<&str> = reset_targets(&mgr, &users)
            .iter()
            .map(|u| u.get_name())
            .collect();
        assert_eq!(names, ["amy", "zed"]);
    }

    #[test]
    fn a_status_is_live_until_used_expired_or_exhausted() {
        let now = chrono::Utc::now();
        let later: Datetime = (now + chrono::Duration::minutes(30)).into();
        let earlier: Datetime = (now - chrono::Duration::minutes(1)).into();
        let base = ResetCodeStatus {
            created_at: earlier,
            expires_at: later,
            used_at: None,
            attempts: 0,
            issued_by_name: None,
        };
        assert!(base.is_live(now));
        assert!(
            !ResetCodeStatus {
                used_at: Some(earlier),
                ..base.clone()
            }
            .is_live(now)
        );
        assert!(
            !ResetCodeStatus {
                expires_at: earlier,
                ..base.clone()
            }
            .is_live(now)
        );
        assert!(
            !ResetCodeStatus {
                attempts: RESET_MAX_ATTEMPTS,
                ..base
            }
            .is_live(now)
        );
    }
}
