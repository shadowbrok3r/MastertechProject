//! Sign-up params built from an employee directory record.

use serde::Serialize;

use super::{EmployeeDirectory, EmployeeRecord, normalize_email_with, short_name};
use crate::schema::{Store, SurrealValue};

/// Prefix the server puts in front of a `THROW` message.
const THROWN_PREFIX: &str = "An error occurred:";

/// Shown for a signup failure with no more specific text.
const GENERIC_SIGNUP_ERROR: &str = "Could not create the account; try again.";

/// What the signup form collects.
#[derive(Clone, PartialEq, Eq)]
pub struct SignupInput {
    pub email: String,
    pub password: String,
    /// Domain appended to a bare username.
    pub domain: String,
    /// Store used when the employee's store is not a retail store.
    pub fallback_store: Store,
}

impl std::fmt::Debug for SignupInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignupInput")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .field("domain", &self.domain)
            .field("fallback_store", &self.fallback_store)
            .finish()
    }
}

/// Params the `user` access SIGNUP clause reads.
#[derive(Clone, PartialEq, Eq, Serialize, SurrealValue)]
pub struct SignupParams {
    pub email: String,
    pub password: String,
    pub name: String,
    pub store: Store,
    pub id_prestashop: u64,
    pub id_store: String,
    /// Always empty; the current SIGNUP clause writes it into a required string field.
    pub everest_initials: String,
}

impl std::fmt::Debug for SignupParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignupParams")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .field("name", &self.name)
            .field("store", &self.store)
            .field("id_prestashop", &self.id_prestashop)
            .field("id_store", &self.id_store)
            .field("everest_initials", &self.everest_initials)
            .finish()
    }
}

/// Why a signup was refused before reaching the server.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignupError {
    #[error("Enter a valid email address.")]
    InvalidEmail,
    #[error("No employee record matches this email.")]
    EmployeeNotFound,
    #[error("This employee is marked inactive in the employee directory.")]
    EmployeeInactive,
    #[error("Could not reach the employee directory; try again.")]
    DirectoryUnavailable(String),
    #[error("Enter a password.")]
    MissingPassword,
}

/// Normalizes the email, requires an active directory employee and builds the signup params.
pub async fn prepare_signup<D: EmployeeDirectory>(
    dir: &D,
    input: SignupInput,
) -> Result<(SignupParams, EmployeeRecord), SignupError> {
    let email =
        normalize_email_with(&input.email, &input.domain).ok_or(SignupError::InvalidEmail)?;
    if input.password.is_empty() {
        return Err(SignupError::MissingPassword);
    }
    let employee = match dir.by_email(&email).await {
        Ok(Some(employee)) => employee,
        Ok(None) => return Err(SignupError::EmployeeNotFound),
        Err(e) => return Err(SignupError::DirectoryUnavailable(format!("{e:#}"))),
    };
    if !employee.active {
        return Err(SignupError::EmployeeInactive);
    }

    let fallback = if Store::RETAIL.contains(&input.fallback_store) {
        input.fallback_store
    } else {
        Store::default()
    };
    let store = employee
        .store()
        .filter(|store| Store::RETAIL.contains(store))
        .unwrap_or(fallback);
    let name = match employee.name() {
        name if name.is_empty() => short_name(&email).to_string(),
        name => name,
    };

    let params = SignupParams {
        email,
        password: input.password,
        name,
        store,
        id_prestashop: employee.id,
        id_store: employee.id_store.clone(),
        everest_initials: String::new(),
    };
    Ok((params, employee))
}

/// User-facing text for a failed signup.
pub fn signup_error_text(err: &surrealdb::Error) -> String {
    if err.is_thrown() {
        let text = strip_thrown_prefix(err.message());
        return if text.is_empty() {
            GENERIC_SIGNUP_ERROR.to_string()
        } else {
            text.to_string()
        };
    }
    if err.is_connection() {
        return "Could not reach the database; try again.".to_string();
    }
    signup_message_text(err.message())
}

/// User-facing text for a signup error message string.
pub fn signup_message_text(message: &str) -> String {
    let message = message.trim();
    if let Some(thrown) = message.strip_prefix(THROWN_PREFIX) {
        let thrown = thrown.trim();
        if !thrown.is_empty() {
            return thrown.to_string();
        }
    }
    let lower = message.to_lowercase();
    if lower.contains("signup query failed") || lower.contains("problem with signing up") {
        return "The server refused the signup; this email or employee may already have an account.".to_string();
    }
    if lower.contains("connection")
        || lower.contains("websocket")
        || lower.contains("uninitialised")
    {
        return "Could not reach the database; try again.".to_string();
    }
    GENERIC_SIGNUP_ERROR.to_string()
}

fn strip_thrown_prefix(message: &str) -> &str {
    let message = message.trim();
    message
        .strip_prefix(THROWN_PREFIX)
        .unwrap_or(message)
        .trim()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use surrealdb::types::Value;

    use super::*;

    #[derive(Default)]
    struct FakeDirectory {
        rows: HashMap<String, EmployeeRecord>,
        unreachable: bool,
    }

    impl FakeDirectory {
        fn with(records: impl IntoIterator<Item = EmployeeRecord>) -> Self {
            Self {
                rows: records.into_iter().map(|r| (r.email.clone(), r)).collect(),
                unreachable: false,
            }
        }
    }

    impl EmployeeDirectory for FakeDirectory {
        async fn by_email(&self, email: &str) -> anyhow::Result<Option<EmployeeRecord>> {
            if self.unreachable {
                anyhow::bail!("HTTP 502");
            }
            Ok(self.rows.get(email).cloned())
        }

        async fn by_id(&self, id: u64) -> anyhow::Result<Option<EmployeeRecord>> {
            Ok(self.rows.values().find(|r| r.id == id).cloned())
        }
    }

    fn record(email: &str, id_store: &str, active: bool) -> EmployeeRecord {
        EmployeeRecord {
            id: 1501,
            email: email.into(),
            first_name: " Bob ".into(),
            last_name: " Smith ".into(),
            id_store: id_store.into(),
            active,
        }
    }

    fn input(email: &str, domain: &str, fallback_store: Store) -> SignupInput {
        SignupInput {
            email: email.into(),
            password: " secret ".into(),
            domain: domain.into(),
            fallback_store,
        }
    }

    #[tokio::test]
    async fn a_bare_username_completes_with_the_chosen_domain() {
        let dir = FakeDirectory::with([
            record("bob@pclaptops.com", "7", true),
            record("bob@xidax.com", "7", true),
        ]);
        let (params, _) = prepare_signup(&dir, input(" Bob ", "pclaptops.com", Store::RIV))
            .await
            .expect("signup");
        assert_eq!(params.email, "bob@pclaptops.com");
        let (params, _) = prepare_signup(&dir, input("bob", "xidax.com", Store::RIV))
            .await
            .expect("signup");
        assert_eq!(params.email, "bob@xidax.com");
    }

    #[tokio::test]
    async fn a_typed_xidax_address_is_kept() {
        let dir = FakeDirectory::with([record("chris@xidax.com", "7", true)]);
        let (params, _) =
            prepare_signup(&dir, input("Chris@XIDAX.com", "pclaptops.com", Store::RIV))
                .await
                .expect("signup");
        assert_eq!(params.email, "chris@xidax.com");
    }

    #[tokio::test]
    async fn params_carry_the_employee_identity() {
        let dir = FakeDirectory::with([record("bob.smith@pclaptops.com", "12", true)]);
        let (params, employee) = prepare_signup(
            &dir,
            input(" Bob.Smith@PCLaptops.com ", "pclaptops.com", Store::RIV),
        )
        .await
        .expect("signup");
        assert_eq!(params.name, "Bob Smith");
        assert_eq!(params.store, Store::SAN);
        assert_eq!(params.id_prestashop, 1501);
        assert_eq!(params.id_store, "12");
        assert_eq!(params.password, " secret ");
        assert_eq!(params.everest_initials, "");
        assert_eq!(employee.id, 1501);
    }

    #[tokio::test]
    async fn warehouse_and_unmapped_stores_use_the_fallback() {
        for id_store in ["1", "11", ""] {
            let dir = FakeDirectory::with([record("bob@pclaptops.com", id_store, true)]);
            let (params, _) = prepare_signup(&dir, input("bob", "pclaptops.com", Store::MUR))
                .await
                .expect("signup");
            assert_eq!(params.store, Store::MUR, "id_store {id_store:?}");
            assert_eq!(params.id_store, id_store);
        }
    }

    #[tokio::test]
    async fn a_non_retail_fallback_becomes_the_default_store() {
        for fallback in [Store::WAR, Store::Unknown] {
            let dir = FakeDirectory::with([record("bob@pclaptops.com", "1", true)]);
            let (params, _) = prepare_signup(&dir, input("bob", "pclaptops.com", fallback))
                .await
                .expect("signup");
            assert_eq!(params.store, Store::RIV);
        }
    }

    #[tokio::test]
    async fn an_employee_without_a_name_is_named_by_the_email() {
        let mut nameless = record("bob@pclaptops.com", "7", true);
        nameless.first_name = " ".into();
        nameless.last_name = String::new();
        let dir = FakeDirectory::with([nameless]);
        let (params, _) = prepare_signup(&dir, input("bob", "pclaptops.com", Store::RIV))
            .await
            .expect("signup");
        assert_eq!(params.name, "bob");
    }

    #[tokio::test]
    async fn refusals_name_the_reason() {
        let dir = FakeDirectory::with([record("gone@pclaptops.com", "7", false)]);
        let refused = |email: &str| input(email, "pclaptops.com", Store::RIV);

        assert_eq!(
            prepare_signup(&dir, refused("a b")).await.unwrap_err(),
            SignupError::InvalidEmail
        );
        assert_eq!(
            prepare_signup(&dir, refused("")).await.unwrap_err(),
            SignupError::InvalidEmail
        );
        assert_eq!(
            prepare_signup(&dir, refused("nobody")).await.unwrap_err(),
            SignupError::EmployeeNotFound
        );
        assert_eq!(
            prepare_signup(&dir, refused("gone")).await.unwrap_err(),
            SignupError::EmployeeInactive
        );

        let mut no_password = refused("gone");
        no_password.password = String::new();
        assert_eq!(
            prepare_signup(&dir, no_password).await.unwrap_err(),
            SignupError::MissingPassword
        );

        let down = FakeDirectory {
            unreachable: true,
            ..Default::default()
        };
        let err = prepare_signup(&down, refused("bob")).await.unwrap_err();
        assert!(
            matches!(err, SignupError::DirectoryUnavailable(ref detail) if detail.contains("502"))
        );
        assert_eq!(
            err.to_string(),
            "Could not reach the employee directory; try again."
        );
    }

    #[test]
    fn signup_params_carry_exactly_the_signup_clause_keys() {
        let params = SignupParams {
            email: "bob@pclaptops.com".into(),
            password: "pw".into(),
            name: "Bob".into(),
            store: Store::RIV,
            id_prestashop: 1501,
            id_store: "7".into(),
            everest_initials: String::new(),
        };
        let Value::Object(map) = params.into_value() else {
            panic!("SignupParams did not encode as an object");
        };
        let keys: BTreeSet<String> = map.keys().cloned().collect();
        let expected: BTreeSet<String> = [
            "email",
            "password",
            "name",
            "store",
            "id_prestashop",
            "id_store",
            "everest_initials",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(keys, expected);
        assert_eq!(map.get("store"), Some(&Value::String("RIV".into())));
        assert_eq!(
            map.get("everest_initials"),
            Some(&Value::String(String::new()))
        );
    }

    #[test]
    fn debug_output_hides_the_password() {
        let shown = format!("{:?}", input("bob", "pclaptops.com", Store::RIV));
        assert!(!shown.contains("secret"));
    }

    #[test]
    fn thrown_errors_show_their_message() {
        let wrapped = surrealdb::Error::thrown("An error occurred: Enter a password.".into());
        assert_eq!(signup_error_text(&wrapped), "Enter a password.");
        let bare = surrealdb::Error::thrown("This employee already has an account.".into());
        assert_eq!(
            signup_error_text(&bare),
            "This employee already has an account."
        );
    }

    #[test]
    fn other_errors_fall_back_to_readable_text() {
        let refused = surrealdb::Error::internal("The record access signup query failed".into());
        assert!(signup_error_text(&refused).contains("may already have an account"));
        let odd = surrealdb::Error::internal("something else".into());
        assert_eq!(signup_error_text(&odd), GENERIC_SIGNUP_ERROR);
        assert_eq!(
            signup_message_text("An error occurred: Unknown store."),
            "Unknown store."
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_prestashop_signup_futures_are_send() {
        fn assert_send<T: Send>(_: &T) {}
        let dir = super::super::employee_directory();
        let prepare = prepare_signup(&dir, input("bob", "pclaptops.com", Store::RIV));
        assert_send(&prepare);
        let params = SignupParams {
            email: "bob@pclaptops.com".into(),
            password: "pw".into(),
            name: "Bob".into(),
            store: Store::RIV,
            id_prestashop: 1,
            id_store: "7".into(),
            everest_initials: String::new(),
        };
        let signup = crate::Database::signup(params);
        assert_send(&signup);
    }
}
