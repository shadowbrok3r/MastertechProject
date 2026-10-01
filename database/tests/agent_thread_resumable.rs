//! Which open thread a general chat resumes, against in-memory SurrealDB.

use database::schema::agent_thread::{AGENT_THREAD_WORKING_STATUSES, RESUMABLE_SQL};
use serde_json::Value;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

const CS: &str = "general:owner@x.com";

async fn mem_db(seed: &str) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    db.query("DEFINE TABLE agent_turn SCHEMALESS;").await.expect("define").check().expect("agent_turn table");
    db.query(seed).await.expect("seed").check().expect("seed statements");
    db
}

/// The key of the thread `RESUMABLE_SQL` picks for `CS`.
async fn resumed(db: &Surreal<Db>) -> Option<String> {
    let mut res = db
        .query(RESUMABLE_SQL)
        .bind(("cs", CS))
        .bind(("working", AGENT_THREAD_WORKING_STATUSES.map(String::from).to_vec()))
        .await
        .expect("query");
    let rows: Vec<Value> = res.take(0).expect("rows");
    rows.first().and_then(|r| r["key"].as_str()).map(str::to_string)
}

#[tokio::test]
async fn a_general_session_idle_for_hours_is_not_resumed() {
    let db = mem_db(
        "CREATE agent_thread:old CONTENT { key: 'old', connection_string: 'general:owner@x.com', status: 'idle',
             created_at: time::now() - 7d, updated_at: time::now() - 2h };
         CREATE agent_thread:done CONTENT { key: 'done', connection_string: 'general:owner@x.com', status: 'closed',
             created_at: time::now() - 1m, updated_at: time::now() };
         CREATE agent_thread:other CONTENT { key: 'other', connection_string: 'general:mate@x.com', status: 'idle',
             created_at: time::now(), updated_at: time::now() };",
    )
    .await;
    assert_eq!(resumed(&db).await, None);
}

#[tokio::test]
async fn a_working_or_recent_general_session_is_resumed_newest_first() {
    let db = mem_db(
        "CREATE agent_thread:busy CONTENT { key: 'busy', connection_string: 'general:owner@x.com', status: 'running',
             created_at: time::now() - 3h, updated_at: time::now() - 2h };",
    )
    .await;
    assert_eq!(resumed(&db).await.as_deref(), Some("busy"), "a working session, however quiet");
    db.query(
        "CREATE agent_thread:recent CONTENT { key: 'recent', connection_string: 'general:owner@x.com', status: 'idle',
             created_at: time::now() - 20m, updated_at: time::now() - 5m };",
    )
    .await
    .expect("insert")
    .check()
    .expect("statement");
    assert_eq!(resumed(&db).await.as_deref(), Some("recent"), "the newest resumable session");
}

#[tokio::test]
async fn a_recent_row_write_does_not_resume_a_session_nobody_asked_for_30_minutes() {
    let db = mem_db(
        "CREATE agent_thread:voice CONTENT { key: 'voice', connection_string: 'general:owner@x.com', status: 'idle',
             created_at: time::now() - 18h, updated_at: time::now() - 1m };
         CREATE agent_turn CONTENT { thread: agent_thread:voice, kind: 'start', text: 'q', status: 'sent',
             created_at: time::now() - 40m };
         CREATE agent_turn CONTENT { thread: agent_thread:voice, kind: 'rename', text: 'Voice', status: 'sent',
             created_at: time::now() - 2m };",
    )
    .await;
    assert_eq!(resumed(&db).await, None, "a runner release or rename is not a question");
}

#[tokio::test]
async fn a_question_in_the_last_30_minutes_keeps_an_old_session_resumable() {
    let db = mem_db(
        "CREATE agent_thread:voice CONTENT { key: 'voice', connection_string: 'general:owner@x.com', status: 'idle',
             created_at: time::now() - 18h, updated_at: time::now() - 2h };
         CREATE agent_turn CONTENT { thread: agent_thread:voice, kind: 'queue', text: 'q', status: 'sent',
             created_at: time::now() - 10m };",
    )
    .await;
    assert_eq!(resumed(&db).await.as_deref(), Some("voice"));
}
