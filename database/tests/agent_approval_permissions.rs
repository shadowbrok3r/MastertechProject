//! Approval, thread, turn, request and SurrealQL-approval rules against in-memory SurrealDB, with record users signed in.

use database::schema::agent_thread::ADOPT_THREAD_LINKS_SQL;
use database::schema::assist::CREATE_CONFIRMED_SQL;
use database::schema::{
    AgentApproval, AgentThread, AssistRequest, Datetime, RecordId, SqlApproval, CREATE_THREAD_SQL, DECIDER_ALLOWED_SQL,
    DECIDE_SQL, LIST_PENDING_MINE_SQL, MAY_STEER_SQL, REOPEN_SQL, SQL_CLAIM_SQL, SQL_DECIDE_SQL,
};
use serde_json::json;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;
use surrealdb::Surreal;

const SCHEMA: &[&str] = &[
    include_str!("../schema/agent_approval.surql"),
    include_str!("../schema/agent_thread.surql"),
    include_str!("../schema/agent_turn.surql"),
    include_str!("../schema/assist_request.surql"),
    include_str!("../schema/sql_approval.surql"),
    "DEFINE TABLE user TYPE ANY SCHEMALESS PERMISSIONS FOR select WHERE $access = 'user';
     DEFINE ACCESS user ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM user WHERE email = $email);
     DEFINE ACCESS guest ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM guest_user WHERE username = $username);",
];

const ROLLOUT: &str = include_str!("../rollouts/20260926000000__agent_table_permissions.toml");
const APPROVE_ALL_ROLLOUT: &str = include_str!("../rollouts/20260926030000__agent_approve_all.toml");

/// The SQL of the rollout step named `id`.
fn rollout_step(id: &str) -> &'static str {
    step_in(ROLLOUT, id)
}

/// The SQL of the step named `id` in `manifest`.
fn step_in(manifest: &'static str, id: &str) -> &'static str {
    let step = &manifest[manifest.find(&format!("id = \"{id}\"")).expect("the step")..];
    let body = &step[step.find("sql = \"\"\"").expect("its sql") + 9..];
    &body[..body.find("\"\"\"").expect("the sql end")]
}

const SEED: &str = "
    CREATE user:owner CONTENT { email: 'owner@x.com', authorization: 'User', active: true, store: 'MUR' };
    CREATE user:mate CONTENT { email: 'mate@x.com', authorization: 'User', active: true, store: 'MUR' };
    CREATE user:root CONTENT { email: 'root@x.com', authorization: 'Root', active: true, store: 'RIV' };
    CREATE user:gone CONTENT { email: 'gone@x.com', authorization: 'Root', active: false, store: 'RIV' };
    CREATE user:idle CONTENT { email: 'idle@x.com', authorization: 'User', active: false, store: 'MUR' };
    CREATE guest_user:guest CONTENT { username: 'guest' };
    CREATE agent_thread:t1 CONTENT { status: 'idle', connection_string: 'PC-1:abc', assignee: user:owner,
        requested_by: 'owner@x.com', store: 'MUR' };
    CREATE agent_thread:t0 CONTENT { status: 'idle', connection_string: 'PC-2:def' };
    CREATE agent_thread:t2 CONTENT { status: 'idle', connection_string: 'PC-4:jkl', assignee: user:idle,
        requested_by: 'idle@x.com', store: 'MUR' };
";

/// In-memory database with the real schema; this handle is unauthenticated and bypasses permissions.
async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for ddl in SCHEMA {
        db.query(*ddl).await.expect("apply schema").check().expect("schema statements");
    }
    db.query(SEED).await.expect("seed").check().expect("seed statements");
    db
}

/// A second session on the same database, signed in as the record user with `email`.
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

/// A second session signed in through the guest record access, the way pre-login binaries connect.
async fn guest(db: &Surreal<Db>) -> Surreal<Db> {
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

fn user(key: &str) -> RecordId {
    RecordId::new("user", key)
}

/// A pending tool-call approval on `thread`, expiring `ttl_secs` from now (negative for the past).
async fn approval(db: &Surreal<Db>, key: &str, thread: &str, assignee: Option<&str>, ttl_secs: i64) -> RecordId {
    let id = RecordId::new("agent_approval", key);
    db.query(
        "CREATE $id CONTENT { thread: type::record('agent_thread', $thread), kind: 'tool_call', \
         method: 'item/tool/call', codex_request_id: '1', summary: 'run desktop_click on PC-1', \
         assignee: $assignee, status: 'pending', expires_at: $expires }",
    )
    .bind(("id", id.clone()))
    .bind(("thread", thread.to_string()))
    .bind(("assignee", assignee.map(user)))
    .bind(("expires", Datetime::from_timestamp(Datetime::now().timestamp() + ttl_secs, 0)))
    .await
    .expect("create approval")
    .check()
    .expect("create approval statement");
    id
}

async fn read(db: &Surreal<Db>, id: &RecordId) -> AgentApproval {
    let rows: Vec<AgentApproval> =
        db.query("SELECT * FROM $id").bind(("id", id.clone())).await.expect("select").take(0).expect("rows");
    rows.into_iter().next().expect("the approval row")
}

async fn decide(session: &Surreal<Db>, id: &RecordId, status: &str) -> Vec<AgentApproval> {
    session
        .query(DECIDE_SQL)
        .bind(("id", id.clone()))
        .bind(("status", status.to_string()))
        .bind(("note", None::<String>))
        .bind(("answers", None::<serde_json::Value>))
        .await
        .expect("decide")
        .check()
        .expect("decide statement")
        .take(0)
        .expect("decided rows")
}

async fn visible_approvals(session: &Surreal<Db>) -> Vec<RecordId> {
    session
        .query("SELECT VALUE id FROM agent_approval ORDER BY id")
        .await
        .expect("select")
        .take(0)
        .expect("ids")
}

#[tokio::test]
async fn record_sessions_are_permission_checked() {
    let db = mem_db().await;
    let mine = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let unowned = approval(&db, "a0", "t0", None, 600).await;

    assert_eq!(visible_approvals(&signed_in(&db, "owner@x.com").await).await, vec![mine.clone()]);
    assert!(visible_approvals(&signed_in(&db, "mate@x.com").await).await.is_empty());
    assert!(visible_approvals(&signed_in(&db, "gone@x.com").await).await.is_empty());
    assert_eq!(visible_approvals(&signed_in(&db, "root@x.com").await).await, vec![unowned, mine]);
}

#[tokio::test]
async fn the_owner_records_a_decision_stamped_with_their_id() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let won = decide(&signed_in(&db, "owner@x.com").await, &id, "accepted").await;
    assert_eq!(won.len(), 1);
    let row = read(&db, &id).await;
    assert_eq!(row.status, "accepted");
    assert_eq!(row.decided_by, Some(user("owner")));
    assert!(row.decided_at.is_some());
}

#[tokio::test]
async fn a_store_mate_cannot_decide_and_sees_no_error() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    assert!(decide(&signed_in(&db, "mate@x.com").await, &id, "accepted").await.is_empty());
    let row = read(&db, &id).await;
    assert_eq!(row.status, "pending");
    assert_eq!(row.decided_by, None);
}

#[tokio::test]
async fn an_active_root_decides_any_row_and_an_inactive_root_none() {
    let db = mem_db().await;
    let owned = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let unowned = approval(&db, "a0", "t0", None, 600).await;

    let gone = signed_in(&db, "gone@x.com").await;
    assert!(decide(&gone, &owned, "accepted").await.is_empty());
    assert!(decide(&gone, &unowned, "accepted").await.is_empty());

    let root = signed_in(&db, "root@x.com").await;
    assert_eq!(decide(&root, &owned, "declined").await.len(), 1);
    assert_eq!(decide(&root, &unowned, "accepted").await.len(), 1);
    assert_eq!(read(&db, &owned).await.decided_by, Some(user("root")));
    assert_eq!(read(&db, &unowned).await.status, "accepted");
}

#[tokio::test]
async fn an_owner_cannot_decide_an_unowned_row() {
    let db = mem_db().await;
    let unowned = approval(&db, "a0", "t0", None, 600).await;
    assert!(decide(&signed_in(&db, "owner@x.com").await, &unowned, "accepted").await.is_empty());
}

#[tokio::test]
async fn an_expired_row_takes_no_decision() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), -60).await;
    assert!(decide(&signed_in(&db, "owner@x.com").await, &id, "accepted").await.is_empty());
    assert_eq!(read(&db, &id).await.status, "pending");
}

#[tokio::test]
async fn a_session_without_auth_cannot_decide_an_unowned_row() {
    let db = mem_db().await;
    let unowned = approval(&db, "a0", "t0", None, 600).await;
    assert!(decide(&db, &unowned, "accepted").await.is_empty(), "NONE must never match an unowned row");
    assert_eq!(read(&db, &unowned).await.status, "pending");
}

#[tokio::test]
async fn pending_mine_lists_only_the_viewers_rows() {
    let db = mem_db().await;
    let mine = approval(&db, "a1", "t1", Some("owner"), 600).await;
    approval(&db, "a0", "t0", None, 600).await;
    let list = |session: Surreal<Db>| async move {
        let rows: Vec<AgentApproval> =
            session.query(LIST_PENDING_MINE_SQL).await.expect("list").take(0).expect("rows");
        rows.into_iter().map(|r| r.id).collect::<Vec<_>>()
    };
    assert_eq!(list(signed_in(&db, "owner@x.com").await).await, vec![mine]);
    assert!(list(signed_in(&db, "mate@x.com").await).await.is_empty());
    assert!(list(signed_in(&db, "root@x.com").await).await.is_empty());
    assert!(list(db.clone()).await.is_empty());
}

#[tokio::test]
async fn the_decider_check_follows_the_owner_rule() {
    let db = mem_db().await;
    let allowed = |by: Option<&str>, owner: Option<&str>| {
        let db = db.clone();
        let (by, owner) = (by.map(user), owner.map(user));
        async move {
            let ok: Option<bool> = db
                .query(DECIDER_ALLOWED_SQL)
                .bind(("by", by))
                .bind(("owner", owner))
                .await
                .expect("check")
                .take(0)
                .expect("bool");
            ok.unwrap_or(false)
        }
    };
    assert!(allowed(Some("owner"), Some("owner")).await);
    assert!(allowed(Some("root"), Some("owner")).await);
    assert!(allowed(Some("root"), None).await);
    assert!(!allowed(Some("gone"), Some("owner")).await);
    assert!(!allowed(Some("mate"), Some("owner")).await);
    assert!(!allowed(Some("mate"), None).await);
    assert!(!allowed(Some("idle"), Some("idle")).await, "an inactive owner's decision does not count");
    assert!(!allowed(None, Some("owner")).await);
    assert!(!allowed(None, None).await);
}

#[tokio::test]
async fn reopen_restores_only_the_decision_it_was_asked_about() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    db.query("UPDATE $id SET status = 'accepted', decided_by = user:mate, decided_at = time::now(), answers = { a: ['x'] }")
        .bind(("id", id.clone()))
        .await
        .expect("forge")
        .check()
        .expect("forge statement");
    let reopen = |status: &str, by: &str| {
        let (db, id) = (db.clone(), id.clone());
        let (status, by) = (status.to_string(), user(by));
        async move {
            let ids: Vec<RecordId> = db
                .query(REOPEN_SQL)
                .bind(("id", id))
                .bind(("status", status))
                .bind(("by", Some(by)))
                .await
                .expect("reopen")
                .check()
                .expect("reopen statement")
                .take(0)
                .expect("ids");
            ids.len()
        }
    };
    assert_eq!(reopen("accepted", "owner").await, 0, "a different decider is left alone");
    assert_eq!(reopen("declined", "mate").await, 0, "a different status is left alone");
    assert_eq!(reopen("accepted", "mate").await, 1);
    let row = read(&db, &id).await;
    assert_eq!(row.status, "pending");
    assert_eq!((row.decided_by, row.decided_at, row.answers), (None, None, None));
    assert_eq!(reopen("accepted", "mate").await, 0);
}

#[tokio::test]
async fn a_non_owner_update_is_filtered_without_an_error() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let mate = signed_in(&db, "mate@x.com").await;
    let rows: Vec<serde_json::Value> = mate
        .query("UPDATE $id SET status = 'accepted' RETURN AFTER")
        .bind(("id", id.clone()))
        .await
        .expect("update")
        .check()
        .expect("a denied update is not an error")
        .take(0)
        .expect("rows");
    assert!(rows.is_empty());
    assert_eq!(read(&db, &id).await.status, "pending");
}

#[tokio::test]
async fn identity_fields_are_frozen_for_the_owner() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let before = read(&db, &id).await;
    signed_in(&db, "owner@x.com")
        .await
        .query(
            "UPDATE $id SET assignee = user:mate, thread = agent_thread:t0, expires_at = time::now() + 1y, \
             summary = 'something else', tool = 'remote_exec_start', deny_note = 'mine'",
        )
        .bind(("id", id.clone()))
        .await
        .expect("update")
        .check()
        .expect("update statement");
    let after = read(&db, &id).await;
    assert_eq!(after.assignee, before.assignee);
    assert_eq!(after.thread, before.thread);
    assert_eq!(after.expires_at, before.expires_at);
    assert_eq!(after.summary, before.summary);
    assert_eq!(after.tool, before.tool);
    assert_eq!(after.deny_note.as_deref(), Some("mine"), "unfrozen fields still take the write");
}

#[tokio::test]
async fn a_forged_decider_fails_the_assert() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let forged = signed_in(&db, "owner@x.com")
        .await
        .query("UPDATE $id SET status = 'accepted', decided_by = user:root")
        .bind(("id", id.clone()))
        .await
        .expect("query")
        .check();
    assert!(forged.is_err(), "decided_by must be the signed-in user");
    assert_eq!(read(&db, &id).await.status, "pending");
}

#[tokio::test]
async fn record_users_cannot_create_or_delete_approvals() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    for email in ["owner@x.com", "root@x.com"] {
        let session = signed_in(&db, email).await;
        let created: Vec<RecordId> = session
            .query(
                "CREATE agent_approval CONTENT { thread: agent_thread:t1, kind: 'tool_call', method: 'm', \
                 codex_request_id: '2', summary: 's', assignee: $auth.id, status: 'accepted' } RETURN VALUE id",
            )
            .await
            .expect("create")
            .check()
            .expect("a denied create is not an error")
            .take(0)
            .expect("ids");
        assert!(created.is_empty(), "{email}");
        session.query("DELETE $id").bind(("id", id.clone())).await.expect("delete").check().expect("delete statement");
    }
    assert_eq!(visible_approvals(&db).await, vec![id]);
}

#[tokio::test]
async fn the_system_user_still_creates_resolves_and_reopens() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    db.query(
        "UPDATE $id SET status = IF status = 'pending' THEN 'expired' ELSE status END, \
         response_sent = { ok: false }, sent_to_codex_at = time::now(), decided_at = decided_at ?? time::now()",
    )
    .bind(("id", id.clone()))
    .await
    .expect("resolve")
    .check()
    .expect("resolve statement");
    assert_eq!(read(&db, &id).await.status, "expired");
    db.query("UPDATE $id SET status = 'pending', decided_at = NONE").bind(("id", id.clone())).await.expect("reset");
    assert_eq!(read(&db, &id).await.status, "pending");
}

#[tokio::test]
async fn record_users_cannot_create_threads() {
    let db = mem_db().await;
    for email in ["mate@x.com", "root@x.com"] {
        let created: Vec<RecordId> = signed_in(&db, email)
            .await
            .query(
                "CREATE agent_thread CONTENT { status: 'idle', connection_string: 'PC-1:abc', assignee: $auth.id } \
                 RETURN VALUE id",
            )
            .await
            .expect("create")
            .check()
            .expect("a denied create is not an error")
            .take(0)
            .expect("ids");
        assert!(created.is_empty(), "{email}");
    }
    let count: Option<i64> =
        db.query("RETURN count(SELECT * FROM agent_thread)").await.expect("count").take(0).expect("n");
    assert_eq!(count, Some(3));
}

#[tokio::test]
async fn a_thread_ownership_rewrite_is_reverted() {
    let db = mem_db().await;
    signed_in(&db, "mate@x.com")
        .await
        .query(
            "UPDATE agent_thread:t1 SET assignee = user:mate, requested_by = 'mate@x.com', \
             connection_string = 'PC-9:zzz', store = 'RIV', assist_request = assist_request:x, service_number = '2155113'",
        )
        .await
        .expect("update")
        .check()
        .expect("update statement");
    let t1 = thread_row(&db, "t1").await;
    assert_eq!(t1.assignee, Some(user("owner")));
    assert_eq!(t1.requested_by.as_deref(), Some("owner@x.com"));
    assert_eq!(t1.connection_string, "PC-1:abc");
    assert_eq!(t1.store.as_deref(), Some("MUR"));
    assert_eq!(t1.assist_request, None);
    assert_eq!(t1.service_number.as_deref(), Some("2155113"), "service links still take the write");
}

async fn thread_row(db: &Surreal<Db>, key: &str) -> AgentThread {
    let rows: Vec<AgentThread> = db
        .query("SELECT * FROM type::record('agent_thread', $key)")
        .bind(("key", key.to_string()))
        .await
        .expect("select")
        .take(0)
        .expect("rows");
    rows.into_iter().next().expect("the thread")
}

#[tokio::test]
async fn broker_state_is_frozen_even_for_the_owner() {
    let db = mem_db().await;
    db.query("UPDATE agent_thread:t1 SET codex_thread_id = 'codex-a', tool_path = 'dynamic', title = 'Owner session'")
        .await
        .expect("seed state")
        .check()
        .expect("seed statement");
    for session in [signed_in(&db, "owner@x.com").await, signed_in(&db, "root@x.com").await] {
        session
            .query(
                "UPDATE agent_thread:t1 SET codex_thread_id = 'codex-b', status = 'closed', closed_at = time::now(), \
                 hostname = 'elsewhere', broker_node = 'n2', tool_path = 'mcp', model = 'm', provider = 'p', \
                 last_seq = 99, computer = computer:x, diagnostic_session = diagnostic_session:x, \
                 driven_by = 'codex/x', error = 'e', activity = 'idle', title = 'renamed', tokens_used = 1, \
                 tokens_window = 2",
            )
            .await
            .expect("update")
            .check()
            .expect("update statement");
    }
    let t1 = thread_row(&db, "t1").await;
    assert_eq!(t1.codex_thread_id.as_deref(), Some("codex-a"));
    assert_eq!(t1.status, "idle");
    assert_eq!(t1.closed_at, None);
    assert_eq!(t1.tool_path.as_deref(), Some("dynamic"));
    assert_eq!(t1.title.as_deref(), Some("Owner session"));
    assert_eq!((t1.hostname, t1.model, t1.last_seq, t1.computer), (None, None, None, None));
}

#[tokio::test]
async fn only_an_active_root_toggles_box_shell() {
    let db = mem_db().await;
    let toggle = |email: &'static str| {
        let db = db.clone();
        async move {
            signed_in(&db, email)
                .await
                .query("UPDATE agent_thread:t1 SET allow_box_shell = true")
                .await
                .expect("update")
                .check()
                .expect("update statement");
            thread_row(&db, "t1").await.allow_box_shell
        }
    };
    assert!(!toggle("owner@x.com").await);
    assert!(!toggle("gone@x.com").await);
    assert!(toggle("root@x.com").await);
}

#[tokio::test]
async fn a_guest_writes_no_thread_field() {
    let db = mem_db().await;
    guest(&db)
        .await
        .query("UPDATE agent_thread:t1 SET service_number = '1', codex_thread_id = 'codex-b'")
        .await
        .expect("update")
        .check()
        .expect("a denied update is not an error");
    let t1 = thread_row(&db, "t1").await;
    assert_eq!((t1.service_number, t1.codex_thread_id), (None, None));
}

#[tokio::test]
async fn service_links_still_adopt_for_a_record_user() {
    let db = mem_db().await;
    let ids: Vec<RecordId> = signed_in(&db, "mate@x.com")
        .await
        .query(ADOPT_THREAD_LINKS_SQL)
        .bind(("cs", "PC-1:abc"))
        .bind(("sn", "2155113"))
        .bind(("so", RecordId::new("service_order", "so1")))
        .bind(("cust", None::<RecordId>))
        .await
        .expect("adopt")
        .check()
        .expect("adopt statement")
        .take(0)
        .expect("ids");
    assert_eq!(ids, vec![RecordId::new("agent_thread", "t1")]);
}

async fn queue_turn(session: &Surreal<Db>, thread: &str) -> Vec<RecordId> {
    session
        .query(
            "CREATE agent_turn CONTENT { thread: type::record('agent_thread', $thread), kind: 'start', \
             text: 'hello', status: 'pending' } RETURN VALUE id",
        )
        .bind(("thread", thread.to_string()))
        .await
        .expect("create turn")
        .check()
        .expect("a denied create is not an error")
        .take(0)
        .expect("ids")
}

#[tokio::test]
async fn only_the_owner_or_an_active_root_queues_a_turn() {
    let db = mem_db().await;
    let owner = signed_in(&db, "owner@x.com").await;
    let root = signed_in(&db, "root@x.com").await;
    assert_eq!(queue_turn(&owner, "t1").await.len(), 1);
    assert_eq!(queue_turn(&root, "t1").await.len(), 1);
    assert!(queue_turn(&signed_in(&db, "mate@x.com").await, "t1").await.is_empty());
    assert!(queue_turn(&signed_in(&db, "gone@x.com").await, "t1").await.is_empty());
    assert!(queue_turn(&owner, "t0").await.is_empty(), "an unowned thread takes turns from Root only");
    assert_eq!(queue_turn(&root, "t0").await.len(), 1);
    assert!(queue_turn(&guest(&db).await, "t1").await.is_empty(), "the guest session never steers");
    assert!(queue_turn(&signed_in(&db, "idle@x.com").await, "t2").await.is_empty(), "an inactive owner no longer steers");
    assert_eq!(queue_turn(&db, "t1").await.len(), 1, "the broker bypasses the rule");
}

#[tokio::test]
async fn only_the_broker_deletes_turns() {
    let db = mem_db().await;
    let turn = queue_turn(&signed_in(&db, "owner@x.com").await, "t1").await.into_iter().next().expect("a turn");
    for session in [signed_in(&db, "owner@x.com").await, signed_in(&db, "root@x.com").await, guest(&db).await] {
        session.query("DELETE $id").bind(("id", turn.clone())).await.expect("delete").check().expect("delete statement");
    }
    let left: Vec<RecordId> = db.query("SELECT VALUE id FROM agent_turn").await.expect("select").take(0).expect("ids");
    assert_eq!(left, vec![turn.clone()]);
    db.query("DELETE $id").bind(("id", turn)).await.expect("delete").check().expect("broker delete");
    let left: Vec<RecordId> = db.query("SELECT VALUE id FROM agent_turn").await.expect("select").take(0).expect("ids");
    assert!(left.is_empty());
}

#[tokio::test]
async fn an_inactive_owner_neither_decides_nor_lists() {
    let db = mem_db().await;
    let id = approval(&db, "a2", "t2", Some("idle"), 600).await;
    let idle = signed_in(&db, "idle@x.com").await;
    assert!(decide(&idle, &id, "accepted").await.is_empty());
    assert_eq!(read(&db, &id).await.status, "pending");
    assert!(visible_approvals(&idle).await.is_empty());
    let rows: Vec<AgentApproval> = idle.query(LIST_PENDING_MINE_SQL).await.expect("list").take(0).expect("rows");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn a_queued_turn_cannot_be_moved_or_changed_by_others() {
    let db = mem_db().await;
    let owner = signed_in(&db, "owner@x.com").await;
    let turn = queue_turn(&owner, "t1").await.into_iter().next().expect("a turn");
    let turn_row = |db: Surreal<Db>, turn: RecordId| async move {
        let rows: Vec<serde_json::Value> =
            db.query("SELECT status, text, <string> thread AS thread FROM $id").bind(("id", turn)).await.expect("select").take(0).expect("rows");
        rows.into_iter().next().expect("the turn")
    };

    owner.query("UPDATE $id SET thread = agent_thread:t0").bind(("id", turn.clone())).await.expect("move");
    assert_eq!(turn_row(db.clone(), turn.clone()).await["thread"], "agent_thread:t1");

    signed_in(&db, "mate@x.com")
        .await
        .query("UPDATE $id SET text = 'injected', status = 'cancelled'")
        .bind(("id", turn.clone()))
        .await
        .expect("update")
        .check()
        .expect("a denied update is not an error");
    let row = turn_row(db.clone(), turn.clone()).await;
    assert_eq!((row["text"].as_str(), row["status"].as_str()), (Some("hello"), Some("pending")));

    let taken: Vec<serde_json::Value> = owner
        .query("UPDATE $id SET status = 'cancelled' WHERE status IN ['pending', 'queued', 'held'] RETURN BEFORE")
        .bind(("id", turn.clone()))
        .await
        .expect("take back")
        .take(0)
        .expect("rows");
    assert_eq!(taken.len(), 1, "the owner can still take a turn back");
}

#[tokio::test]
async fn a_record_user_files_requests_only_in_their_own_name() {
    let db = mem_db().await;
    let mate = signed_in(&db, "mate@x.com").await;
    let create = |by: Option<&str>| {
        let (mate, by) = (mate.clone(), by.map(str::to_string));
        async move {
            mate.query("CREATE assist_request CONTENT { connection_string: 'PC-1:abc', requested_by: $by } RETURN VALUE requested_by")
                .bind(("by", by))
                .await
                .expect("create")
                .check()
                .map(|mut r| r.take::<Vec<Option<String>>>(0).expect("rows"))
        }
    };
    assert!(create(Some("owner@x.com")).await.is_err(), "another tech's email is refused");
    assert_eq!(create(Some("mate@x.com")).await.expect("own email"), vec![Some("mate@x.com".to_string())]);
    assert_eq!(create(None).await.expect("no email"), vec![Some("mate@x.com".to_string())], "the default names the signed-in tech");

    let id = RecordId::new("assist_request", "tur1");
    mate.query(CREATE_CONFIRMED_SQL)
        .bind(("id", id))
        .bind(("cs", "PC-1:abc"))
        .bind(("host", "PC-1"))
        .bind(("sn", "2155113"))
        .bind(("computer", RecordId::new("computer", "PC-1:abc")))
        .bind(("by", "mate@x.com"))
        .bind(("store", "MUR"))
        .await
        .expect("confirmed request")
        .check()
        .expect("the TUR sheet's request still files");
}

#[tokio::test]
async fn a_new_thread_finds_its_assignee_ignoring_case() {
    let db = mem_db().await;
    let create = |by: Option<&str>| {
        let (db, by) = (db.clone(), by.map(str::to_string));
        async move {
            let ids: Vec<RecordId> = db
                .query(CREATE_THREAD_SQL)
                .bind(("cs", "PC-3:ghi"))
                .bind(("requested_by", by))
                .await
                .expect("create thread")
                .check()
                .expect("create thread statements")
                .take(1)
                .expect("ids");
            let id = ids.into_iter().next().expect("a thread");
            let rows: Vec<AgentThread> =
                db.query("SELECT * FROM $id").bind(("id", id)).await.expect("select").take(0).expect("rows");
            rows.into_iter().next().expect("the thread").assignee
        }
    };
    assert_eq!(create(Some("OWNER@X.com")).await, Some(user("owner")));
    assert_eq!(create(Some("owner@x.com")).await, Some(user("owner")));
    assert_eq!(create(Some("nobody@x.com")).await, None);
    assert_eq!(create(None).await, None);
}

#[tokio::test]
async fn only_the_owner_or_an_active_root_may_steer_through_the_broker() {
    let db = mem_db().await;
    let may = |thread: &str, by: Option<&str>| {
        let (db, thread, by) = (db.clone(), RecordId::new("agent_thread", thread), by.map(str::to_string));
        async move {
            let ok: Option<bool> = db
                .query(MAY_STEER_SQL)
                .bind(("thread", thread))
                .bind(("by", by))
                .await
                .expect("check")
                .take(1)
                .expect("bool");
            ok.unwrap_or(false)
        }
    };
    assert!(may("t1", Some("Owner@X.com")).await);
    assert!(may("t1", Some("root@x.com")).await);
    assert!(!may("t1", Some("mate@x.com")).await);
    assert!(!may("t1", Some("gone@x.com")).await);
    assert!(!may("t1", None).await);
    assert!(!may("t0", Some("owner@x.com")).await);
    assert!(may("t0", Some("root@x.com")).await);
    assert!(!may("t2", Some("idle@x.com")).await, "an inactive owner may not steer");
}

async fn file_request(session: &Surreal<Db>, requested_by: Option<&str>, filed_access: Option<&str>) -> RecordId {
    let ids: Vec<RecordId> = session
        .query(
            "CREATE assist_request CONTENT { connection_string: 'PC-1:abc', requested_by: $by, \
             filed_access: $forged, status: 'pending', fresh: false } RETURN VALUE id",
        )
        .bind(("by", requested_by.map(str::to_string)))
        .bind(("forged", filed_access.map(str::to_string)))
        .await
        .expect("create")
        .check()
        .expect("create statement")
        .take(0)
        .expect("ids");
    ids.into_iter().next().expect("a request")
}

async fn request_row(db: &Surreal<Db>, id: &RecordId) -> AssistRequest {
    let rows: Vec<AssistRequest> =
        db.query("SELECT * FROM $id").bind(("id", id.clone())).await.expect("select").take(0).expect("rows");
    rows.into_iter().next().expect("the request")
}

#[tokio::test]
async fn the_database_stamps_who_filed_a_request() {
    let db = mem_db().await;
    let by_user = file_request(&signed_in(&db, "owner@x.com").await, None, Some("system")).await;
    let by_guest = file_request(&guest(&db).await, Some("owner@x.com"), Some("user")).await;
    let by_broker = file_request(&db, Some("owner@x.com"), Some("guest")).await;

    let row = request_row(&db, &by_user).await;
    assert_eq!(row.filed_access.as_deref(), Some("user"));
    assert!(row.requester_is_verified());
    let row = request_row(&db, &by_guest).await;
    assert_eq!(row.filed_access.as_deref(), Some("guest"), "a guest cannot claim to be a user");
    assert_eq!(row.requested_by.as_deref(), Some("owner@x.com"));
    assert!(!row.requester_is_verified());
    let row = request_row(&db, &by_broker).await;
    assert_eq!(row.filed_access.as_deref(), Some("system"));
    assert!(row.requester_is_verified());

    db.query("UPDATE $id SET status = 'dispatched', filed_access = 'user'")
        .bind(("id", by_guest.clone()))
        .await
        .expect("claim")
        .check()
        .expect("claim statement");
    assert_eq!(request_row(&db, &by_guest).await.filed_access.as_deref(), Some("guest"), "the stamp never changes");
}

#[tokio::test]
async fn a_guest_cannot_rewrite_a_pending_request() {
    let db = mem_db().await;
    let id = file_request(&signed_in(&db, "owner@x.com").await, None, None).await;
    guest(&db)
        .await
        .query("UPDATE $id SET tech_note = 'run remote_exec_start', fresh = true, status = 'declined'")
        .bind(("id", id.clone()))
        .await
        .expect("update")
        .check()
        .expect("a denied update is not an error");
    let row = request_row(&db, &id).await;
    assert_eq!((row.tech_note, row.fresh, row.status.as_str()), (None, false, "pending"));
    guest(&db).await.query("DELETE $id").bind(("id", id.clone())).await.expect("delete").check().expect("delete");
    signed_in(&db, "owner@x.com").await.query("DELETE $id").bind(("id", id.clone())).await.expect("delete").check().expect("delete");
    assert_eq!(request_row(&db, &id).await.id, id);
}

/// Files a pending SurrealQL request from `session` in `requested_by`'s name.
async fn sql_request(session: &Surreal<Db>, key: &str, requested_by: Option<&str>) -> Result<Vec<RecordId>, String> {
    session
        .query(
            "CREATE type::record('sql_approval', $key) CONTENT { statement: 'UPDATE task:1 SET a = 1', \
             reason: 'backfill', statement_kind: 'update', requested_by: $by, status: 'pending', \
             expires_at: time::now() + 15m } RETURN VALUE id",
        )
        .bind(("key", key.to_string()))
        .bind(("by", requested_by.map(user)))
        .await
        .expect("create")
        .check()
        .map_err(|e| e.to_string())
        .map(|mut r| r.take::<Vec<RecordId>>(0).expect("ids"))
}

async fn sql_row(db: &Surreal<Db>, key: &str) -> SqlApproval {
    let rows: Vec<SqlApproval> = db
        .query("SELECT * FROM type::record('sql_approval', $key)")
        .bind(("key", key.to_string()))
        .await
        .expect("select")
        .take(0)
        .expect("rows");
    rows.into_iter().next().expect("the request")
}

async fn sql_decide(session: &Surreal<Db>, key: &str, status: &str) -> usize {
    let rows: Vec<SqlApproval> = session
        .query(SQL_DECIDE_SQL)
        .bind(("id", RecordId::new("sql_approval", key)))
        .bind(("status", status.to_string()))
        .bind(("why", None::<String>))
        .await
        .expect("decide")
        .check()
        .expect("decide statement")
        .take(0)
        .expect("rows");
    rows.len()
}

async fn sql_claim(session: &Surreal<Db>, key: &str, statement: &str) -> usize {
    let rows: Vec<SqlApproval> = session
        .query(SQL_CLAIM_SQL)
        .bind(("id", RecordId::new("sql_approval", key)))
        .bind(("statement", statement.to_string()))
        .await
        .expect("claim")
        .check()
        .expect("claim statement")
        .take(0)
        .expect("rows");
    rows.len()
}

#[tokio::test]
async fn a_tech_files_sql_requests_only_in_their_own_name() {
    let db = mem_db().await;
    let mate = signed_in(&db, "mate@x.com").await;
    assert_eq!(sql_request(&mate, "own", Some("mate")).await.expect("own request").len(), 1);
    let forged = sql_request(&mate, "forged", Some("owner")).await;
    assert!(forged.map_or(true, |ids| ids.is_empty()), "another user's name is refused");
    let unnamed = sql_request(&mate, "unnamed", None).await;
    assert!(unnamed.map_or(true, |ids| ids.is_empty()), "an unnamed request is refused");
    assert!(sql_request(&guest(&db).await, "guest", None).await.expect("a denied create").is_empty());
    let ids: Vec<RecordId> = db.query("SELECT VALUE id FROM sql_approval").await.expect("select").take(0).expect("ids");
    assert_eq!(ids, vec![RecordId::new("sql_approval", "own")]);
}

#[tokio::test]
async fn only_an_active_root_approves_a_sql_request() {
    let db = mem_db().await;
    let mate = signed_in(&db, "mate@x.com").await;
    sql_request(&mate, "q1", Some("mate")).await.expect("request");

    let self_approve = mate.query("UPDATE sql_approval:q1 SET status = 'approved', decided_by = user:mate").await.expect("update").check();
    assert!(self_approve.is_err(), "the requester cannot approve their own request");
    let forged = mate.query("UPDATE sql_approval:q1 SET decided_by = user:root").await.expect("update").check();
    assert!(forged.is_err(), "decided_by must be the signed-in user");
    guest(&db)
        .await
        .query("UPDATE sql_approval:q1 SET status = 'approved', decided_by = user:root")
        .await
        .expect("update")
        .check()
        .expect("a denied update is not an error");
    assert_eq!(sql_decide(&signed_in(&db, "owner@x.com").await, "q1", "approved").await, 0);
    assert_eq!(sql_decide(&signed_in(&db, "gone@x.com").await, "q1", "approved").await, 0);
    assert_eq!(sql_row(&db, "q1").await.status, "pending");

    assert_eq!(sql_decide(&signed_in(&db, "root@x.com").await, "q1", "approved").await, 1);
    let row = sql_row(&db, "q1").await;
    assert_eq!((row.status.as_str(), row.decided_by), ("approved", Some(user("root"))));
}

#[tokio::test]
async fn the_requester_runs_only_the_statement_a_root_approved() {
    let db = mem_db().await;
    let mate = signed_in(&db, "mate@x.com").await;
    sql_request(&mate, "q1", Some("mate")).await.expect("request");
    mate.query("UPDATE sql_approval:q1 SET statement = 'UPDATE user:mate SET authorization = \"Root\"', impact_rows = 0")
        .await
        .expect("rewrite")
        .check()
        .expect("a frozen field reverts quietly");
    let row = sql_row(&db, "q1").await;
    assert_eq!((row.statement.as_str(), row.impact_rows), ("UPDATE task:1 SET a = 1", None));

    assert_eq!(sql_decide(&signed_in(&db, "root@x.com").await, "q1", "approved").await, 1);
    assert_eq!(sql_claim(&mate, "q1", "UPDATE user:mate SET authorization = 'Root'").await, 0, "another statement never claims");
    assert_eq!(sql_claim(&mate, "q1", "UPDATE task:1 SET a = 1").await, 1);
    assert_eq!(sql_row(&db, "q1").await.status, "executing");
    mate.query("UPDATE sql_approval:q1 SET status = 'executed', result_summary = '1 row(s) returned'")
        .await
        .expect("record")
        .check()
        .expect("the requester records the outcome");
    assert_eq!(sql_row(&db, "q1").await.status, "executed");
}

#[tokio::test]
async fn an_approval_not_recorded_by_an_active_root_never_claims() {
    let db = mem_db().await;
    sql_request(&signed_in(&db, "mate@x.com").await, "q1", Some("mate")).await.expect("request");
    db.query("UPDATE sql_approval:q1 SET status = 'approved', decided_by = user:mate").await.expect("forge").check().expect("forge");
    assert_eq!(sql_claim(&db, "q1", "UPDATE task:1 SET a = 1").await, 0);
    db.query("UPDATE sql_approval:q1 SET decided_by = user:gone").await.expect("forge").check().expect("forge");
    assert_eq!(sql_claim(&db, "q1", "UPDATE task:1 SET a = 1").await, 0);
    db.query("UPDATE sql_approval:q1 SET decided_by = NONE").await.expect("forge").check().expect("forge");
    assert_eq!(sql_claim(&db, "q1", "UPDATE task:1 SET a = 1").await, 0);
    assert_eq!(sql_row(&db, "q1").await.status, "approved");
}

#[tokio::test]
async fn sql_requests_are_private_to_the_requester_and_root() {
    let db = mem_db().await;
    sql_request(&signed_in(&db, "mate@x.com").await, "q1", Some("mate")).await.expect("request");
    sql_request(&db, "broker", None).await.expect("system request");
    let visible = |session: Surreal<Db>| async move {
        let ids: Vec<RecordId> =
            session.query("SELECT VALUE id FROM sql_approval ORDER BY id").await.expect("select").take(0).expect("ids");
        ids
    };
    let q1 = RecordId::new("sql_approval", "q1");
    let broker = RecordId::new("sql_approval", "broker");
    assert_eq!(visible(signed_in(&db, "mate@x.com").await).await, vec![q1.clone()]);
    assert!(visible(signed_in(&db, "owner@x.com").await).await.is_empty());
    assert!(visible(guest(&db).await).await.is_empty());
    assert_eq!(visible(signed_in(&db, "root@x.com").await).await, vec![broker, q1.clone()]);
    assert_eq!(sql_decide(&signed_in(&db, "root@x.com").await, "broker", "denied").await, 1, "Root decides a system request");
    signed_in(&db, "mate@x.com").await.query("DELETE $id").bind(("id", q1.clone())).await.expect("delete").check().expect("delete");
    assert_eq!(visible(db.clone()).await.len(), 2);
}

#[tokio::test]
async fn the_rollout_steps_apply_and_roll_back_cleanly() {
    let db = mem_db().await;
    let id = approval(&db, "a1", "t1", Some("owner"), 600).await;
    let mate_update = |db: Surreal<Db>, id: RecordId| async move {
        let rows: Vec<serde_json::Value> = signed_in(&db, "mate@x.com")
            .await
            .query("UPDATE $id SET deny_note = 'mate' RETURN AFTER")
            .bind(("id", id))
            .await
            .expect("update")
            .check()
            .expect("update statement")
            .take(0)
            .expect("rows");
        rows.len()
    };

    db.query(rollout_step("restore_full_agent_permissions")).await.expect("rollback").check().expect("rollback statements");
    assert_eq!(mate_update(db.clone(), id.clone()).await, 1, "the rollback reopens the table");

    db.query(rollout_step("tighten_agent_permissions")).await.expect("start").check().expect("start statements");
    assert_eq!(mate_update(db.clone(), id.clone()).await, 0, "the start step closes it again");
    assert!(queue_turn(&signed_in(&db, "mate@x.com").await, "t1").await.is_empty());
    assert!(queue_turn(&signed_in(&db, "idle@x.com").await, "t2").await.is_empty());
    signed_in(&db, "owner@x.com")
        .await
        .query("UPDATE agent_thread:t1 SET codex_thread_id = 'codex-b', status = 'closed'")
        .await
        .expect("update")
        .check()
        .expect("update statement");
    let t1 = thread_row(&db, "t1").await;
    assert_eq!((t1.codex_thread_id, t1.status.as_str()), (None, "idle"));
    let by_guest = file_request(&guest(&db).await, Some("owner@x.com"), Some("user")).await;
    assert_eq!(request_row(&db, &by_guest).await.filed_access.as_deref(), Some("guest"));
    sql_request(&signed_in(&db, "mate@x.com").await, "q1", Some("mate")).await.expect("request");
    assert!(signed_in(&db, "mate@x.com").await.query("UPDATE sql_approval:q1 SET status = 'approved'").await.expect("update").check().is_err());
    assert_eq!(sql_decide(&signed_in(&db, "root@x.com").await, "q1", "approved").await, 1);
    assert_eq!(sql_claim(&signed_in(&db, "mate@x.com").await, "q1", "UPDATE task:1 SET a = 1").await, 1);
}

#[tokio::test]
async fn the_start_step_defines_what_the_schema_files_define() {
    let files = mem_db().await;
    let rolled = mem_db().await;
    rolled.query(rollout_step("restore_full_agent_permissions")).await.expect("rollback").check().expect("rollback statements");
    rolled.query(rollout_step("tighten_agent_permissions")).await.expect("start").check().expect("start statements");
    for table in ["agent_approval", "agent_thread", "agent_turn", "assist_request", "sql_approval"] {
        let info = |db: Surreal<Db>| async move {
            let info: Option<serde_json::Value> = db
                .query(format!("RETURN {{ table: (INFO FOR DB).tables.{table}, fields: (INFO FOR TABLE {table}).fields }}"))
                .await
                .expect("info")
                .take(0)
                .expect("info value");
            info.expect("the definitions")
        };
        let (rolled_info, files_info) = (info(rolled.clone()).await, info(files.clone()).await);
        assert!(files_info["table"].as_str().is_some_and(|t| t.contains("PERMISSIONS")), "{table}: {files_info}");
        assert!(files_info["fields"].as_object().is_some_and(|f| !f.is_empty()), "{table}");
        assert_eq!(rolled_info, files_info, "{table}");
    }
}

#[tokio::test]
async fn the_rollback_restores_every_table_it_tightened() {
    let db = mem_db().await;
    db.query(rollout_step("restore_full_agent_permissions")).await.expect("rollback").check().expect("rollback statements");
    let mate = signed_in(&db, "mate@x.com").await;
    assert_eq!(queue_turn(&mate, "t1").await.len(), 1);
    mate.query("UPDATE agent_thread:t1 SET codex_thread_id = 'codex-b'").await.expect("update").check().expect("update");
    assert_eq!(thread_row(&db, "t1").await.codex_thread_id.as_deref(), Some("codex-b"));
    assert_eq!(sql_request(&guest(&db).await, "g", None).await.expect("guest request").len(), 1);
    let by_guest = file_request(&guest(&db).await, Some("owner@x.com"), Some("user")).await;
    assert_eq!(request_row(&db, &by_guest).await.filed_access.as_deref(), Some("user"), "the stamp is gone after rollback");
}

#[tokio::test]
async fn approve_all_is_written_by_the_broker_only() {
    let db = mem_db().await;
    for email in ["owner@x.com", "root@x.com"] {
        signed_in(&db, email)
            .await
            .query("UPDATE agent_thread:t1 SET approve_all = true")
            .await
            .expect("update")
            .check()
            .expect("a denied update is not an error");
    }
    assert_eq!(thread_row(&db, "t1").await.approve_all, None);
    db.query("UPDATE agent_thread:t1 SET approve_all = true").await.expect("update").check().expect("broker update");
    assert_eq!(thread_row(&db, "t1").await.approve_all, Some(true));
}

#[tokio::test]
async fn an_approvals_turn_follows_the_steering_rule() {
    let db = mem_db().await;
    let ask_again = |session: Surreal<Db>| async move {
        let ids: Vec<RecordId> = session
            .query("CREATE agent_turn CONTENT { thread: agent_thread:t1, kind: 'approvals', text: 'prompt' } RETURN VALUE id")
            .await
            .expect("create turn")
            .check()
            .expect("a denied create is not an error")
            .take(0)
            .expect("ids");
        ids.len()
    };
    assert_eq!(ask_again(signed_in(&db, "owner@x.com").await).await, 1);
    assert_eq!(ask_again(signed_in(&db, "root@x.com").await).await, 1);
    assert_eq!(ask_again(signed_in(&db, "mate@x.com").await).await, 0);
}

#[tokio::test]
async fn the_owner_records_approve_all_as_a_decision() {
    let db = mem_db().await;
    let id = approval(&db, "a9", "t1", Some("owner"), 600).await;
    let owner = signed_in(&db, "owner@x.com").await;
    let rows = decide(&owner, &id, database::schema::agent_approval::ACCEPTED_ALL_FOR_SESSION).await;
    assert_eq!(rows.len(), 1);
    let row = read(&db, &id).await;
    assert_eq!(row.status, "accepted_all_for_session");
    assert!(row.is_human_decision());
    assert_eq!(row.decided_by, Some(user("owner")));
}

#[tokio::test]
async fn the_approve_all_rollout_applies_and_rolls_back_cleanly() {
    let db = mem_db().await;
    let rollback = step_in(APPROVE_ALL_ROLLOUT, "narrow_agent_approvals");
    db.query(rollback).await.expect("rollback").check().expect("rollback statements");
    let refused = db.query("CREATE agent_turn CONTENT { thread: agent_thread:t1, kind: 'approvals' }").await.expect("create");
    assert!(refused.check().is_err(), "the narrowed kind list refuses an approvals turn");
    db.query(step_in(APPROVE_ALL_ROLLOUT, "widen_agent_approvals")).await.expect("start").check().expect("start statements");
    db.query("CREATE agent_turn CONTENT { thread: agent_thread:t1, kind: 'approvals', text: 'prompt' }")
        .await
        .expect("create")
        .check()
        .expect("the widened kind list admits an approvals turn");
    signed_in(&db, "owner@x.com")
        .await
        .query("UPDATE agent_thread:t1 SET approve_all = true")
        .await
        .expect("update")
        .check()
        .expect("a denied update is not an error");
    assert_eq!(thread_row(&db, "t1").await.approve_all, None);
}
