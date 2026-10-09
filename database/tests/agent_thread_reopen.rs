//! Which session MasterTech reopens when it starts on a machine, against in-memory SurrealDB with record users signed in.

use database::schema::RecordId;
use database::schema::agent_thread::REOPEN_SQL;
use database::schema::agent_thread_archive::ARCHIVE_SQL;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;

const CS: &str = "PC-1:abc";

const USERS: &str =
    "DEFINE TABLE user TYPE ANY SCHEMALESS PERMISSIONS FOR select WHERE $access = 'user';
     DEFINE ACCESS user ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM user WHERE email = $email);
     CREATE user:owner CONTENT { email: 'owner@x.com', authorization: 'User', active: true };
     CREATE user:mate CONTENT { email: 'mate@x.com', authorization: 'User', active: true };";

/// In-memory database with the session tables and `seed`; this handle bypasses permissions.
async fn mem_db(seed: &str) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for ddl in [
        USERS,
        include_str!("../schema/agent_thread.surql"),
        include_str!("../schema/agent_thread_archive.surql"),
        seed,
    ] {
        db.query(ddl)
            .await
            .expect("apply")
            .check()
            .expect("statements");
    }
    db
}

async fn signed_in(db: &Surreal<Db>, email: &str) -> Surreal<Db> {
    let session = db.clone();
    session
        .signin(Record {
            namespace: "test".into(),
            database: "test".into(),
            access: "user".into(),
            params: json!({ "email": email }),
        })
        .await
        .expect("record signin");
    session
}

/// The key of the session `REOPEN_SQL` picks for `CS`.
async fn reopened(db: &Surreal<Db>) -> Option<String> {
    let mut res = db.query(REOPEN_SQL).bind(("cs", CS)).await.expect("query");
    let rows: Vec<Value> = res.take(0).expect("rows");
    rows.first()
        .and_then(|r| r["key"].as_str())
        .map(str::to_string)
}

#[tokio::test]
async fn the_newest_open_session_of_this_machine_reopens() {
    let db = mem_db(
        "CREATE agent_thread:older CONTENT { key: 'older', connection_string: 'PC-1:abc', status: 'idle',
             created_at: time::now() - 2d, updated_at: time::now() - 1d };
         CREATE agent_thread:newer CONTENT { key: 'newer', connection_string: 'PC-1:abc', status: 'running',
             created_at: time::now() - 3h, updated_at: time::now() - 1m };
         CREATE agent_thread:elsewhere CONTENT { key: 'elsewhere', connection_string: 'PC-2:def', status: 'running',
             created_at: time::now(), updated_at: time::now() };",
    )
    .await;
    let owner = signed_in(&db, "owner@x.com").await;
    assert_eq!(reopened(&owner).await.as_deref(), Some("newer"));
}

#[tokio::test]
async fn closed_failed_and_stale_sessions_stay_shut() {
    let db = mem_db(
        "CREATE agent_thread:closed CONTENT { key: 'closed', connection_string: 'PC-1:abc', status: 'closed',
             created_at: time::now() - 1h, updated_at: time::now() };
         CREATE agent_thread:failed CONTENT { key: 'failed', connection_string: 'PC-1:abc', status: 'failed',
             created_at: time::now() - 1h, updated_at: time::now() };
         CREATE agent_thread:stale CONTENT { key: 'stale', connection_string: 'PC-1:abc', status: 'idle',
             created_at: time::now() - 9d, updated_at: time::now() - 4d };",
    )
    .await;
    let owner = signed_in(&db, "owner@x.com").await;
    assert_eq!(reopened(&owner).await, None);
}

#[tokio::test]
async fn a_session_archived_by_the_signed_in_user_stays_shut_for_them_only() {
    let db = mem_db(
        "CREATE agent_thread:qc CONTENT { key: 'qc', connection_string: 'PC-1:abc', status: 'idle',
             created_at: time::now() - 5h, updated_at: time::now() - 1h };",
    )
    .await;
    let owner = signed_in(&db, "owner@x.com").await;
    let mate = signed_in(&db, "mate@x.com").await;
    owner
        .query(ARCHIVE_SQL)
        .bind(("threads", vec![RecordId::new("agent_thread", "qc")]))
        .await
        .expect("archive")
        .check()
        .expect("archive statement");
    assert_eq!(reopened(&owner).await, None, "the owner archived it");
    assert_eq!(
        reopened(&mate).await.as_deref(),
        Some("qc"),
        "another technician still gets it"
    );
}
