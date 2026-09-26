//! Signup and sign-in through the live legacy `user` access, against in-memory SurrealDB.

use std::collections::HashMap;

use database::schema::{
    EmployeeDirectory, EmployeeRecord, SignupInput, Store, SurrealValue, User, normalize_email,
    prepare_signup,
};
use database::{AccountCheckError, require_active_account};
use serde_json::json;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;

const USER_SCHEMA: &str = include_str!("../schema/user.surql");
const AUDIT_LOG_SCHEMA: &str = include_str!("../schema/audit_log.surql");
const NOTIFICATION_SCHEMA: &str = include_str!("../schema/notification.surql");
const USER_ACCESS_LEGACY: &str = include_str!("../access/user_legacy.surql");
const JWT_KEY: &str = "user-signup-test-key-0123456789abcdef0123456789abcdef";
const PASSWORD: &str = "correct horse battery";
const BOB: &str = "bob.smith@pclaptops.com";

struct FakeDirectory(HashMap<String, EmployeeRecord>);

impl EmployeeDirectory for FakeDirectory {
    async fn by_email(&self, email: &str) -> anyhow::Result<Option<EmployeeRecord>> {
        Ok(self.0.get(email).cloned())
    }

    async fn by_id(&self, id: u64) -> anyhow::Result<Option<EmployeeRecord>> {
        Ok(self.0.values().find(|r| r.id == id).cloned())
    }
}

fn directory() -> FakeDirectory {
    let bob = EmployeeRecord {
        id: 1501,
        email: BOB.into(),
        first_name: "Bob".into(),
        last_name: "Smith".into(),
        id_store: "7".into(),
        active: true,
    };
    FakeDirectory(HashMap::from([(BOB.to_string(), bob)]))
}

/// Root session on a fresh database with the user schema and the legacy access applied.
async fn legacy_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    // OVERWRITE redefines the fields that `user_statuses.*` auto-defines.
    let user_schema = USER_SCHEMA.replace("DEFINE FIELD ", "DEFINE FIELD OVERWRITE ");
    for (name, sql) in [
        ("user", user_schema.as_str()),
        ("audit_log", AUDIT_LOG_SCHEMA),
        ("notification", NOTIFICATION_SCHEMA),
    ] {
        db.query(sql)
            .await
            .unwrap_or_else(|e| panic!("{name} schema: {e}"))
            .check()
            .unwrap_or_else(|e| panic!("{name} schema statements: {e}"));
    }
    db.query(USER_ACCESS_LEGACY)
        .bind(("jwt_key", JWT_KEY))
        .await
        .expect("legacy access")
        .check()
        .expect("legacy access statement");
    db
}

fn user_access<P: SurrealValue>(params: P) -> Record<P> {
    Record {
        namespace: "test".into(),
        database: "test".into(),
        access: "user".into(),
        params,
    }
}

/// Signs Bob up on a new session cloned from `root` and returns that session.
async fn sign_up_bob(root: &Surreal<Db>) -> Surreal<Db> {
    let input = SignupInput {
        email: " Bob.Smith@PCLaptops.com ".into(),
        password: PASSWORD.into(),
        domain: "pclaptops.com".into(),
        fallback_store: Store::RIV,
    };
    let (params, _) = prepare_signup(&directory(), input)
        .await
        .expect("prepare signup");
    let session = root.clone();
    session
        .signup(user_access(params))
        .await
        .expect("signup through the legacy access");
    session
}

async fn user_emails(root: &Surreal<Db>) -> Vec<String> {
    root.query("SELECT VALUE email FROM user ORDER BY email")
        .await
        .expect("query")
        .take(0)
        .expect("emails")
}

async fn auth_id(session: &Surreal<Db>) -> Option<surrealdb::types::RecordId> {
    session
        .query("RETURN $auth.id")
        .await
        .expect("query")
        .take(0)
        .expect("auth id")
}

#[tokio::test]
async fn prepared_params_sign_up_and_store_every_required_field() {
    let root = legacy_db().await;
    let session = sign_up_bob(&root).await;

    let row: Option<serde_json::Value> = root
        .query(
            "SELECT active, authorization, email, id_prestashop, id_store, name, store, mcp_settings, \
             user_statuses, everest_initials, version, type::is_object(user_settings) AS settings_is_object, \
             user_settings.ui_layout AS ui_layout, user_settings.color_scheme != NONE AS has_theme \
             FROM user WHERE email = $email",
        )
        .bind(("email", BOB))
        .await
        .expect("query")
        .take(0)
        .expect("row");
    let row = row.expect("the signup stored a row");
    assert_eq!(row["active"], json!(true));
    assert_eq!(row["authorization"], json!("User"));
    assert_eq!(row["email"], json!(BOB));
    assert_eq!(row["id_prestashop"], json!(1501));
    assert_eq!(row["id_store"], json!("7"));
    assert_eq!(row["name"], json!("Bob Smith"));
    assert_eq!(row["store"], json!("RIV"));
    assert!(
        row["mcp_settings"].is_object(),
        "mcp_settings: {}",
        row["mcp_settings"]
    );
    assert!(
        row["user_statuses"].is_array(),
        "user_statuses: {}",
        row["user_statuses"]
    );
    assert_eq!(row["everest_initials"], json!(""));
    assert_eq!(row["version"], json!(""));
    assert_eq!(row["settings_is_object"], json!(true));
    assert_eq!(
        row["ui_layout"],
        json!({ "mastertech": {}, "mtechserver": {} })
    );
    assert_eq!(row["has_theme"], json!(true));

    let me: Option<User> = session
        .query("SELECT * FROM $auth.id")
        .await
        .expect("query")
        .take(0)
        .expect("the signed-up row decodes into User");
    let me = me.expect("the session sees its own row");
    assert_eq!(me.get_email(), BOB);
    assert_eq!(me.get_store(), Store::RIV);
    assert_eq!(me.get_employee_id(), Some(1501));
    assert!(me.is_active());
}

#[tokio::test]
async fn sign_in_matches_only_the_normalized_email() {
    let root = legacy_db().await;
    sign_up_bob(&root).await;

    let email = normalize_email(" BOB.SMITH@pclaptops.com ").expect("normalizes");
    let normalized = root.clone();
    normalized
        .signin(user_access(json!({ "email": email, "password": PASSWORD })))
        .await
        .expect("the normalized email signs in");

    let raw = root.clone();
    let refused = raw
        .signin(user_access(
            json!({ "email": " BOB.SMITH@pclaptops.com ", "password": PASSWORD }),
        ))
        .await;
    assert!(
        refused.is_err(),
        "the legacy SIGNIN compares the email as sent"
    );
}

#[tokio::test]
async fn params_without_everest_initials_fail_the_legacy_signup() {
    let root = legacy_db().await;
    let mut params = json!({
        "email": BOB,
        "password": PASSWORD,
        "name": "Bob Smith",
        "store": "RIV",
        "id_prestashop": 1501,
        "id_store": "7",
    });

    let session = root.clone();
    assert!(session.signup(user_access(params.clone())).await.is_err());
    assert!(
        user_emails(&root).await.is_empty(),
        "a refused signup leaves no row"
    );

    params["everest_initials"] = json!("");
    let session = root.clone();
    session
        .signup(user_access(params))
        .await
        .expect("the same params with everest_initials sign up");
    assert_eq!(user_emails(&root).await, [BOB]);
}

#[tokio::test]
async fn an_unknown_store_code_decodes_inside_the_user_list() {
    let root = legacy_db().await;
    root.query(
        "CREATE user:zztest CONTENT { active: false, authorization: 'User', email: 'zz_livetest@pclaptops.com', \
         everest_initials: '', name: 'ZZ Test', password: 'x', store: 'ZZTest', version: '', \
         user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } } }",
    )
    .await
    .expect("create")
    .check()
    .expect("zz row");
    let session = sign_up_bob(&root).await;

    let users: Vec<User> = session
        .query("SELECT * FROM user ORDER BY email")
        .await
        .expect("query")
        .take(0)
        .expect("every row decodes");
    assert_eq!(users.len(), 2);
    let zz = users
        .iter()
        .find(|u| u.get_email() == "zz_livetest@pclaptops.com")
        .expect("zz row");
    assert_eq!(zz.get_store(), Store::Unknown);
    let bob = users
        .iter()
        .find(|u| u.get_email() == BOB)
        .expect("bob row");
    assert_eq!(bob.get_store(), Store::RIV);
}

#[tokio::test]
async fn a_deactivated_account_is_refused_and_signed_out() {
    let root = legacy_db().await;
    let signed_up = sign_up_bob(&root).await;
    let me = require_active_account(&signed_up)
        .await
        .expect("an active account passes");
    assert_eq!(me.get_email(), BOB);

    let token = root
        .clone()
        .signin(user_access(json!({ "email": BOB, "password": PASSWORD })))
        .await
        .expect("sign in")
        .access
        .as_insecure_token()
        .to_string();
    root.query("UPDATE user SET active = false WHERE email = $email")
        .bind(("email", BOB))
        .await
        .expect("deactivate")
        .check()
        .expect("deactivate statement");

    let password_session = root.clone();
    password_session
        .signin(user_access(json!({ "email": BOB, "password": PASSWORD })))
        .await
        .expect("the legacy SIGNIN ignores active");
    let token_session = root.clone();
    token_session
        .authenticate(token)
        .await
        .expect("the earlier token still authenticates");

    for (path, session) in [("password", password_session), ("token", token_session)] {
        assert!(auth_id(&session).await.is_some(), "{path}: signed in");
        let err = require_active_account(&session)
            .await
            .expect_err("a deactivated account is refused");
        assert!(
            matches!(err, AccountCheckError::Deactivated),
            "{path}: {err:?}"
        );
        assert_eq!(err.to_string(), "This account is deactivated.");
        assert_eq!(
            auth_id(&session).await,
            None,
            "{path}: the refused session is invalidated"
        );
    }
}
