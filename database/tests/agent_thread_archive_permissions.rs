//! Per-user session archive rules against in-memory SurrealDB, with record users signed in.

use database::schema::agent_thread_archive::{ARCHIVED_SQL, ARCHIVE_SQL, UNARCHIVE_SQL};
use database::schema::RecordId;
use serde_json::json;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;
use surrealdb::Surreal;

const USERS: &str = "DEFINE TABLE user TYPE ANY SCHEMALESS PERMISSIONS FOR select WHERE $access = 'user';
     DEFINE ACCESS user ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM user WHERE email = $email);";

const ROLLOUT: &str = include_str!("../rollouts/20260929210000__agent_thread_archive.toml");

const SEED: &str = "
    CREATE user:owner CONTENT { email: 'owner@x.com', authorization: 'User', active: true };
    CREATE user:mate CONTENT { email: 'mate@x.com', authorization: 'User', active: true };
    CREATE agent_thread:t1 CONTENT { status: 'idle', connection_string: 'PC-1:abc' };
    CREATE agent_thread:t2 CONTENT { status: 'closed', connection_string: 'general:owner@x.com' };
";

/// The SQL of the step named `id` in the archive rollout.
fn rollout_step(id: &str) -> &'static str {
    let step = &ROLLOUT[ROLLOUT.find(&format!("id = \"{id}\"")).expect("the step")..];
    let body = &step[step.find("sql = \"\"\"").expect("its sql") + 9..];
    &body[..body.find("\"\"\"").expect("the sql end")]
}

/// In-memory database with `archive_ddl` applied; this handle is unauthenticated and bypasses permissions.
async fn mem_db(archive_ddl: &str) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for ddl in [USERS, include_str!("../schema/agent_thread.surql"), archive_ddl, SEED] {
        db.query(ddl).await.expect("apply").check().expect("statements");
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

fn thread(key: &str) -> RecordId {
    RecordId::new("agent_thread", key)
}

async fn archive(db: &Surreal<Db>, threads: &[&str]) -> surrealdb::Result<()> {
    let threads: Vec<RecordId> = threads.iter().map(|t| thread(t)).collect();
    db.query(ARCHIVE_SQL).bind(("threads", threads)).await?.check()?;
    Ok(())
}

async fn archived(db: &Surreal<Db>) -> Vec<RecordId> {
    let mut rows: Vec<RecordId> = db.query(ARCHIVED_SQL).await.expect("select").take(0).expect("rows");
    rows.sort_by_key(|id| format!("{id:?}"));
    rows
}

#[tokio::test]
async fn an_archive_hides_the_session_only_for_the_user_who_archived_it() {
    let db = mem_db(include_str!("../schema/agent_thread_archive.surql")).await;
    let owner = signed_in(&db, "owner@x.com").await;
    let mate = signed_in(&db, "mate@x.com").await;

    archive(&owner, &["t1", "t2"]).await.expect("archive");
    archive(&owner, &["t1"]).await.expect("archiving again is harmless");

    assert_eq!(archived(&owner).await, vec![thread("t1"), thread("t2")]);
    assert!(archived(&mate).await.is_empty(), "another user still lists both sessions");
    let rows: Vec<serde_json::Value> =
        db.query("SELECT * FROM agent_thread_archive").await.expect("select").take(0).expect("rows");
    assert_eq!(rows.len(), 2, "one row per user and thread: {rows:?}");
}

#[tokio::test]
async fn unarchiving_removes_only_the_callers_row() {
    let db = mem_db(include_str!("../schema/agent_thread_archive.surql")).await;
    let owner = signed_in(&db, "owner@x.com").await;
    let mate = signed_in(&db, "mate@x.com").await;
    archive(&owner, &["t1"]).await.expect("owner archives");
    archive(&mate, &["t1"]).await.expect("mate archives");

    owner
        .query(UNARCHIVE_SQL)
        .bind(("threads", vec![thread("t1")]))
        .await
        .expect("unarchive")
        .check()
        .expect("unarchive statement");

    assert!(archived(&owner).await.is_empty());
    assert_eq!(archived(&mate).await, vec![thread("t1")], "mate's archive survives");
}

#[tokio::test]
async fn a_user_cannot_archive_for_someone_else_or_read_their_archive() {
    let db = mem_db(include_str!("../schema/agent_thread_archive.surql")).await;
    let owner = signed_in(&db, "owner@x.com").await;
    let mate = signed_in(&db, "mate@x.com").await;
    archive(&owner, &["t1"]).await.expect("owner archives");

    let forged = mate
        .query("CREATE agent_thread_archive CONTENT { thread: agent_thread:t2, user: user:owner }")
        .await
        .expect("query")
        .check();
    assert!(forged.is_err() || archived(&owner).await == vec![thread("t1")], "no row lands in owner's archive");
    let peeked: Vec<serde_json::Value> =
        mate.query("SELECT * FROM agent_thread_archive").await.expect("select").take(0).expect("rows");
    assert!(peeked.iter().all(|r| r["user"] != json!("user:owner")), "{peeked:?}");
    mate.query("DELETE agent_thread_archive").await.expect("delete").check().expect("delete statement");
    assert_eq!(archived(&owner).await, vec![thread("t1")], "mate cannot clear owner's archive");
}

#[tokio::test]
async fn the_rollout_creates_the_same_table_the_schema_defines() {
    let db = mem_db(rollout_step("add_agent_thread_archive")).await;
    let owner = signed_in(&db, "owner@x.com").await;
    archive(&owner, &["t1"]).await.expect("archive through the rollout's table");
    assert_eq!(archived(&owner).await, vec![thread("t1")]);
    db.query(rollout_step("drop_agent_thread_archive")).await.expect("rollback").check().expect("rollback statements");
}
