//! A DB-level sign-in on a session that first signed in through guest record access, against in-memory SurrealDB.

use serde_json::json;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;
use surrealdb::Surreal;

const SETUP: &str = "
    DEFINE TABLE guest_user TYPE ANY SCHEMALESS PERMISSIONS NONE;
    DEFINE ACCESS guest ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM guest_user WHERE username = $username);
    DEFINE USER agent ON DATABASE PASSWORD 'agent-pass' ROLES EDITOR;
    DEFINE TABLE agent_thread TYPE ANY SCHEMALESS PERMISSIONS FOR select FULL, FOR create, update, delete NONE;
    CREATE guest_user:guest CONTENT { username: 'guest' };
";

/// A second session on an in-memory database, signed in through the guest record access.
async fn guest_session() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    db.query(SETUP).await.expect("setup").check().expect("setup statements");
    let session = db.clone();
    session
        .signin(Record {
            namespace: "test".into(),
            database: "test".into(),
            access: "guest".into(),
            params: json!({ "username": "guest" }),
        })
        .await
        .expect("guest signin");
    session
}

async fn access_of(session: &Surreal<Db>) -> Option<String> {
    session.query("RETURN $access").await.expect("probe").take(0).expect("access")
}

#[tokio::test]
async fn a_database_user_signin_leaves_no_guest_access_behind() {
    let session = guest_session().await;
    assert_eq!(access_of(&session).await.as_deref(), Some("guest"));
    database::signin_database_user_on(&session, "test", "test", "agent", "agent-pass").await.expect("database user signin");
    assert_eq!(access_of(&session).await, None);
    let no_auth: Option<bool> = session.query("RETURN $auth = NONE").await.expect("probe").take(0).expect("auth");
    assert_eq!(no_auth, Some(true), "no guest record stays in $auth");
}

#[tokio::test]
async fn the_database_user_writes_where_record_access_cannot() {
    let session = guest_session().await;
    let denied: Vec<serde_json::Value> =
        session.query("CREATE agent_thread:t1 CONTENT { status: 'idle' }").await.expect("create").take(0).expect("rows");
    assert!(denied.is_empty(), "the guest may not create threads");
    database::signin_database_user_on(&session, "test", "test", "agent", "agent-pass").await.expect("database user signin");
    let created: Vec<serde_json::Value> =
        session.query("CREATE agent_thread:t2 CONTENT { status: 'idle' }").await.expect("create").take(0).expect("rows");
    assert_eq!(created.len(), 1, "the database user bypasses record permissions");
}

#[tokio::test]
async fn a_failed_signin_leaves_the_session_signed_out() {
    let session = guest_session().await;
    assert!(database::signin_database_user_on(&session, "test", "test", "agent", "nope").await.is_err());
    assert_eq!(access_of(&session).await, None, "the guest session does not survive a failed signin");
}
