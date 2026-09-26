//! `create-test-user`: argument checks, legacy-field detection and the guarded CREATE.

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{Context, Result};
use database::schema::{RecordId, Store, SurrealValue, normalize_email};
use serde::Deserialize;
use surrealdb::types::Value;
use surrealdb::{Connection, Surreal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
    User,
    Manager,
    Warehouse,
}

impl Authorization {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Manager => "Manager",
            Self::Warehouse => "Warehouse",
        }
    }
}

/// A validated test account request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestUser {
    pub email: String,
    pub name: String,
    pub store: Store,
    pub authorization: Authorization,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestUserError {
    RootRefused,
    UnknownAuthorization(String),
    WarRefused,
    UnknownStore(String),
    BareUsername(String),
    InvalidEmail(String),
    MissingName,
    EmptyPassword,
    PasswordMismatch,
    EmailExists(String),
}

impl fmt::Display for TestUserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootRefused => write!(f, "test users cannot be Root"),
            Self::UnknownAuthorization(value) => write!(
                f,
                "unknown authorization {value:?}; use User, Manager or Warehouse"
            ),
            Self::WarRefused => write!(
                f,
                "store WAR is refused: builds without the WAR variant fail every login once a WAR row exists"
            ),
            Self::UnknownStore(value) => write!(
                f,
                "store {value:?} is not a retail store; use RIV, LTN, MUR, ORE or SAN"
            ),
            Self::BareUsername(value) => {
                write!(f, "{value:?} is a bare username; pass a full email address")
            }
            Self::InvalidEmail(value) => write!(f, "{value:?} is not a valid email address"),
            Self::MissingName => write!(f, "--name must not be empty"),
            Self::EmptyPassword => write!(f, "the password must not be empty"),
            Self::PasswordMismatch => write!(f, "the passwords do not match"),
            Self::EmailExists(email) => write!(f, "a user with email {email} already exists"),
        }
    }
}

impl std::error::Error for TestUserError {}

/// Checks the command-line values; performs no I/O.
pub fn validate(
    email: &str,
    name: &str,
    store: &str,
    authorization: &str,
) -> Result<TestUser, TestUserError> {
    let authorization = match authorization.trim().to_ascii_lowercase().as_str() {
        "user" => Authorization::User,
        "manager" => Authorization::Manager,
        "warehouse" => Authorization::Warehouse,
        "root" => return Err(TestUserError::RootRefused),
        _ => {
            return Err(TestUserError::UnknownAuthorization(
                authorization.trim().to_string(),
            ));
        }
    };
    let store = match Store::from_code(store) {
        Some(Store::WAR) => return Err(TestUserError::WarRefused),
        Some(code) if Store::RETAIL.contains(&code) => code,
        _ => return Err(TestUserError::UnknownStore(store.trim().to_string())),
    };
    if !email.contains('@') {
        return Err(TestUserError::BareUsername(email.trim().to_string()));
    }
    let email = normalize_email(email)
        .ok_or_else(|| TestUserError::InvalidEmail(email.trim().to_string()))?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(TestUserError::MissingName);
    }
    Ok(TestUser {
        email,
        name,
        store,
        authorization,
    })
}

/// Both prompts must match and be non-empty; passwords are never trimmed.
pub fn check_passwords(first: &str, second: &str) -> Result<(), TestUserError> {
    if first.is_empty() {
        return Err(TestUserError::EmptyPassword);
    }
    if first != second {
        return Err(TestUserError::PasswordMismatch);
    }
    Ok(())
}

/// Required legacy string fields a new row must still carry.
const LEGACY_FIELDS: [&str; 2] = ["everest_initials", "version"];

/// `''` for each legacy field the table still defines.
pub fn legacy_fields(defined: &[String]) -> BTreeMap<String, String> {
    LEGACY_FIELDS
        .iter()
        .filter(|field| defined.iter().any(|name| name == *field))
        .map(|field| (field.to_string(), String::new()))
        .collect()
}

const FIELDS_SQL: &str = "RETURN object::keys((INFO FOR TABLE user).fields);";
const EXISTS_SQL: &str =
    "SELECT VALUE id FROM user WHERE string::lowercase(string::trim(email)) = $email;";
const CREATE_SQL: &str = "IF array::len((SELECT VALUE id FROM user WHERE string::lowercase(string::trim(email)) = $email)) != 0 {
    THROW 'A user with this email already exists.'
} ELSE {
    CREATE user CONTENT object::extend({
        active: true, authorization: $authorization, email: $email, name: $name,
        password: crypto::argon2::generate($password), store: $store,
        id_prestashop: NONE, id_store: '', mcp_settings: {}, user_statuses: [],
        user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } }
    }, $legacy) RETURN id, email, name, store, authorization, active
};";

/// Field names `INFO FOR TABLE user` lists.
pub async fn user_fields<C: Connection>(db: &Surreal<C>) -> Result<Vec<String>> {
    let mut response = db
        .query(FIELDS_SQL)
        .await
        .context("INFO FOR TABLE user")?
        .check()
        .context("INFO FOR TABLE user")?;
    response.take(0).context("decode user field names")
}

/// True when a user's trimmed, lowercased email equals `email`.
pub async fn email_exists<C: Connection>(db: &Surreal<C>, email: &str) -> Result<bool> {
    let mut response = db
        .query(EXISTS_SQL)
        .bind(("email", email.to_string()))
        .await
        .context("look up the email")?
        .check()
        .context("look up the email")?;
    let ids: Vec<RecordId> = response.take(0).context("decode user ids")?;
    Ok(!ids.is_empty())
}

/// The created row as the CREATE returns it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, SurrealValue)]
pub struct CreatedUser {
    pub id: RecordId,
    pub email: String,
    pub name: String,
    pub store: String,
    pub authorization: String,
    pub active: bool,
}

fn create_vars(
    user: &TestUser,
    password: String,
    legacy: BTreeMap<String, String>,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "authorization".to_string(),
            user.authorization.as_str().to_string().into_value(),
        ),
        ("email".to_string(), user.email.clone().into_value()),
        ("name".to_string(), user.name.clone().into_value()),
        ("password".to_string(), password.into_value()),
        (
            "store".to_string(),
            user.store.as_str().to_string().into_value(),
        ),
        ("legacy".to_string(), legacy.into_value()),
    ])
}

/// Creates the row; the password is bound once and hashed server-side.
pub async fn create<C: Connection>(
    db: &Surreal<C>,
    user: &TestUser,
    password: String,
    legacy: BTreeMap<String, String>,
) -> Result<CreatedUser> {
    let mut response = db
        .query(CREATE_SQL)
        .bind(create_vars(user, password, legacy))
        .await
        .context("create the test user")?
        .check()
        .map_err(|e| anyhow::anyhow!(e.message().to_string()))?;
    let rows: Vec<CreatedUser> = response.take(0).context("decode the created row")?;
    rows.into_iter()
        .next()
        .context("the CREATE returned no row")
}

#[cfg(test)]
mod tests {
    use database::schema::User;
    use serde_json::json;
    use surrealdb::Surreal;
    use surrealdb::engine::local::Db;
    use surrealdb::opt::auth::Record;

    use super::*;
    use crate::test_db;

    const PASSWORD: &str = "test user password 1";

    fn ok() -> TestUser {
        validate(" QA.Tester@PCLaptops.com ", "  QA   Tester ", "riv", "user").expect("valid")
    }

    #[test]
    fn valid_input_is_normalized() {
        assert_eq!(
            ok(),
            TestUser {
                email: "qa.tester@pclaptops.com".into(),
                name: "QA Tester".into(),
                store: Store::RIV,
                authorization: Authorization::User,
            }
        );
        let warehouse = validate("a@internal.test", "A", "SAN", "Warehouse").expect("valid");
        assert_eq!(warehouse.authorization, Authorization::Warehouse);
        assert_eq!(warehouse.store, Store::SAN);
    }

    #[test]
    fn root_and_unknown_roles_are_refused() {
        assert_eq!(
            validate("a@pclaptops.com", "A", "RIV", "Root"),
            Err(TestUserError::RootRefused)
        );
        assert_eq!(
            validate("a@pclaptops.com", "A", "RIV", " root "),
            Err(TestUserError::RootRefused)
        );
        assert_eq!(
            validate("a@pclaptops.com", "A", "RIV", "Admin"),
            Err(TestUserError::UnknownAuthorization("Admin".into()))
        );
    }

    #[test]
    fn war_and_non_retail_stores_are_refused() {
        assert_eq!(
            validate("a@pclaptops.com", "A", "WAR", "User"),
            Err(TestUserError::WarRefused)
        );
        assert_eq!(
            validate("a@pclaptops.com", "A", " war ", "User"),
            Err(TestUserError::WarRefused)
        );
        for store in ["Unknown", "ZZTest", "", "7"] {
            assert_eq!(
                validate("a@pclaptops.com", "A", store, "User"),
                Err(TestUserError::UnknownStore(store.trim().into())),
                "{store:?}"
            );
        }
    }

    #[test]
    fn bare_usernames_and_bad_emails_are_refused() {
        assert_eq!(
            validate(" qa.tester ", "A", "RIV", "User"),
            Err(TestUserError::BareUsername("qa.tester".into()))
        );
        for email in ["@pclaptops.com", "a b@pclaptops.com", "a@b@c", "a@"] {
            assert_eq!(
                validate(email, "A", "RIV", "User"),
                Err(TestUserError::InvalidEmail(email.trim().into())),
                "{email:?}"
            );
        }
    }

    #[test]
    fn an_empty_name_is_refused() {
        assert_eq!(
            validate("a@pclaptops.com", "   ", "RIV", "User"),
            Err(TestUserError::MissingName)
        );
    }

    #[test]
    fn passwords_must_match_and_be_non_empty() {
        assert_eq!(check_passwords("", ""), Err(TestUserError::EmptyPassword));
        assert_eq!(
            check_passwords("secret", "secret "),
            Err(TestUserError::PasswordMismatch)
        );
        assert_eq!(check_passwords(" secret ", " secret "), Ok(()));
    }

    #[test]
    fn legacy_fields_follow_the_table_definition() {
        let with = vec![
            "email".to_string(),
            "version".into(),
            "everest_initials".into(),
        ];
        assert_eq!(
            legacy_fields(&with),
            BTreeMap::from([
                ("everest_initials".to_string(), String::new()),
                ("version".to_string(), String::new()),
            ])
        );
        assert!(legacy_fields(&["email".to_string()]).is_empty());
    }

    #[test]
    fn the_create_sql_never_sets_root_or_war_literals() {
        assert!(!CREATE_SQL.contains("'Root'") && !CREATE_SQL.contains("'WAR'"));
        assert!(CREATE_SQL.contains("crypto::argon2::generate($password)"));
    }

    async fn stored(db: &Surreal<Db>, email: &str) -> serde_json::Value {
        let row: Option<serde_json::Value> = db
            .query(
                "SELECT active, authorization, email, name, store, id_prestashop, id_store, mcp_settings, \
                 user_statuses, user_settings, everest_initials, version, \
                 crypto::argon2::compare(password, $password) AS password_ok FROM user WHERE email = $email",
            )
            .bind(("email", email.to_string()))
            .bind(("password", PASSWORD))
            .await
            .expect("query")
            .take(0)
            .expect("row");
        row.expect("a stored row")
    }

    #[tokio::test]
    async fn creates_a_complete_row_that_decodes_and_signs_in() {
        let db = test_db::schema_db().await;
        let fields = user_fields(&db).await.expect("fields");
        let legacy = legacy_fields(&fields);
        assert_eq!(
            legacy.len(),
            2,
            "user.surql still defines both legacy fields"
        );

        let user = ok();
        assert!(!email_exists(&db, &user.email).await.expect("exists"));
        let created = create(&db, &user, PASSWORD.into(), legacy)
            .await
            .expect("create");
        assert_eq!(created.email, "qa.tester@pclaptops.com");
        assert_eq!(created.store, "RIV");
        assert_eq!(created.authorization, "User");
        assert!(created.active);

        let row = stored(&db, &user.email).await;
        assert_eq!(row["name"], json!("QA Tester"));
        assert_eq!(row["id_store"], json!(""));
        assert_eq!(row["id_prestashop"], serde_json::Value::Null);
        assert_eq!(row["mcp_settings"], json!({}));
        assert_eq!(row["user_statuses"], json!([]));
        assert_eq!(
            row["user_settings"],
            json!({ "ui_layout": { "mastertech": {}, "mtechserver": {} } })
        );
        assert_eq!(row["everest_initials"], json!(""));
        assert_eq!(row["version"], json!(""));
        assert_eq!(row["password_ok"], json!(true));

        let decoded: Option<User> = db
            .query("SELECT * FROM $id")
            .bind(("id", created.id.clone()))
            .await
            .expect("select")
            .take(0)
            .expect("decodes into database::schema::User");
        assert!(decoded.is_some());

        test_db::apply_legacy_access(&db).await;
        let session = db.clone();
        session
            .signin(Record {
                namespace: "test".into(),
                database: "test".into(),
                access: "user".into(),
                params: json!({ "email": user.email, "password": PASSWORD }),
            })
            .await
            .expect("the test user signs in through the live access");
    }

    #[tokio::test]
    async fn an_existing_email_in_any_case_is_refused() {
        let db = test_db::schema_db().await;
        test_db::insert_user(&db, "qa.tester", "RIV", Some("7"), true).await;
        let user = ok();
        assert!(email_exists(&db, &user.email).await.expect("exists"));

        let legacy = legacy_fields(&user_fields(&db).await.expect("fields"));
        let err = create(&db, &user, PASSWORD.into(), legacy)
            .await
            .expect_err("duplicate");
        assert!(err.to_string().contains("already exists"), "{err:#}");
        let count: Vec<RecordId> = db
            .query("SELECT VALUE id FROM user")
            .await
            .expect("query")
            .take(0)
            .expect("ids");
        assert_eq!(count.len(), 1);
    }

    #[tokio::test]
    async fn rows_omit_legacy_fields_once_the_table_drops_them() {
        let db = test_db::schema_db().await;
        db.query("REMOVE FIELD everest_initials ON user; REMOVE FIELD version ON user;")
            .await
            .expect("remove")
            .check()
            .expect("remove statements");
        let legacy = legacy_fields(&user_fields(&db).await.expect("fields"));
        assert!(legacy.is_empty());
        let user = ok();
        create(&db, &user, PASSWORD.into(), legacy)
            .await
            .expect("create without legacy fields");
        let row = stored(&db, &user.email).await;
        assert_eq!(row["everest_initials"], serde_json::Value::Null);
        assert_eq!(row["version"], serde_json::Value::Null);
        assert_eq!(row["password_ok"], json!(true));
    }
}
