//! Service-task, link-adoption and stress-run reconcile statements against in-memory SurrealDB with the real table definitions.

use chrono::{Duration, Utc};
use database::schema::agent_thread::ADOPT_THREAD_LINKS_SQL;
use database::schema::crash_intel::{CLAIM_STRESS_RUNS_SQL, LINK_STRESS_RUN_ORDERS_SQL};
use database::schema::diagnostic::ADOPT_SESSION_LINKS_SQL;
use database::schema::service_task::{
    NewServiceTask, Requester, ServiceOrderRow, TaskCandidate, FILL_ORDER_LINKS_SQL,
    REQUESTER_SQL, SERVICE_ORDER_BY_NUMBER_SQL, TASKS_FOR_SERVICE_SQL,
};
use database::schema::{Datetime, Record, RecordId};
use serde_json::Value;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

const SCHEMA: &[&str] = &[
    include_str!("../schema/task.surql"),
    include_str!("../schema/notification.surql"),
    include_str!("../schema/service_order.surql"),
    include_str!("../schema/diagnostic_session.surql"),
    include_str!("../schema/agent_thread.surql"),
    include_str!("../schema/stress_test_run.surql"),
    "DEFINE TABLE user TYPE ANY SCHEMALESS; DEFINE TABLE customer TYPE ANY SCHEMALESS; \
     DEFINE TABLE computer TYPE ANY SCHEMALESS;",
];

async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for ddl in SCHEMA {
        let ddl = ddl.replace("DEFINE FIELD ", "DEFINE FIELD OVERWRITE ");
        db.query(ddl).await.expect("apply schema").check().expect("schema statements");
    }
    db
}

fn rid(table: &str, key: &str) -> RecordId {
    RecordId::new(table, key)
}

fn ago(minutes: i64) -> Datetime {
    Datetime::from(Utc::now() - Duration::minutes(minutes))
}

async fn row(db: &Surreal<Db>, id: &RecordId) -> Value {
    let rows: Vec<Value> = db
        .query("SELECT * FROM $id")
        .bind(("id", id.clone()))
        .await
        .expect("select")
        .take(0)
        .expect("rows");
    rows.into_iter().next().unwrap_or(Value::Null)
}

/// Key of the record a field links to.
fn link(v: &Value, field: &str) -> Option<String> {
    let text = v.get(field)?.as_str()?;
    let key = text.split_once(':').map_or(text, |(_, key)| key);
    Some(key.trim_matches(|c| matches!(c, '`' | '\u{27e8}' | '\u{27e9}')).to_string())
}

fn text(v: &Value, field: &str) -> Option<String> {
    v.get(field).and_then(Value::as_str).map(str::to_string)
}

async fn seed_user(db: &Surreal<Db>) {
    db.query(
        "CREATE user:derek CONTENT { name: 'Derek Anderson', email: 'derek.anderson@pclaptops.com', \
         store: 'MUR', active: true }; \
         CREATE user:gone CONTENT { name: 'Old Tech', email: 'old.tech@pclaptops.com', store: 'RIV', \
         active: false };",
    )
    .await
    .expect("seed users")
    .check()
    .expect("user rows");
}

async fn seed_order(
    db: &Surreal<Db>,
    key: &str,
    customer: Option<&RecordId>,
    computer: Option<&RecordId>,
) -> RecordId {
    let id = rid("service_order", key);
    db.query(
        "CREATE $id CONTENT { service_number: $sn, customer: $cust, computer: $comp, \
         checkin_notes: 'CPS and annual tuneup', checkin_rep: '', doc_alias: 'service', \
         sales_rep: '', tech: 'caelan.grippa', terms: '', ticket_total: '0', \
         hardware_test_results: {} }",
    )
    .bind(("id", id.clone()))
    .bind(("sn", key.to_string()))
    .bind(("cust", customer.cloned()))
    .bind(("comp", computer.cloned()))
    .await
    .expect("seed order")
    .check()
    .expect("order row");
    id
}

async fn seed_task(
    db: &Surreal<Db>,
    key: &str,
    sn: Option<&str>,
    ticket: Option<&RecordId>,
    completed: bool,
) -> RecordId {
    let id = rid("task", key);
    db.query(
        "CREATE $id CONTENT { task_name: $name, service_number: $sn, service_ticket: $ticket, \
         assignee: user:derek, completed: $done, status: 'Todo', priority: 'Normal' }",
    )
    .bind(("id", id.clone()))
    .bind(("name", format!("{key} task")))
    .bind(("sn", sn.map(str::to_string)))
    .bind(("ticket", ticket.cloned()))
    .bind(("done", completed))
    .await
    .expect("seed task")
    .check()
    .expect("task row");
    id
}

#[tokio::test]
async fn tasks_are_found_by_service_number_or_ticket_but_never_by_a_missing_ticket() {
    let db = mem_db().await;
    seed_user(&db).await;
    let order = seed_order(&db, "2155467", None, None).await;
    let by_number = seed_task(&db, "by-number", Some("2155467"), None, false).await;
    let by_ticket = seed_task(&db, "by-ticket", None, Some(&order), true).await;
    seed_task(&db, "other", Some("2155113"), None, false).await;
    seed_task(&db, "unlinked", None, None, false).await;

    let mut found: Vec<TaskCandidate> = db
        .query(TASKS_FOR_SERVICE_SQL)
        .bind(("sn", "2155467"))
        .bind(("so", Some(order.clone())))
        .await
        .expect("query")
        .take(0)
        .expect("candidates");
    found.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: Vec<RecordId> = found.iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids, vec![by_number.clone(), by_ticket]);
    assert_eq!(found[1].completed, Some(true));
    assert!(found[0].created_at.is_some());

    let found: Vec<TaskCandidate> = db
        .query(TASKS_FOR_SERVICE_SQL)
        .bind(("sn", "2155467"))
        .bind(("so", None::<RecordId>))
        .await
        .expect("query")
        .take(0)
        .expect("candidates");
    let ids: Vec<RecordId> = found.iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids, vec![by_number]);
}

#[tokio::test]
async fn the_order_row_carries_its_customer_name() {
    let db = mem_db().await;
    let customer = rid("customer", "4215");
    db.query("CREATE customer:`4215` CONTENT { name: 'Barbara Baker', cust_code: '4215' }")
        .await
        .expect("seed customer")
        .check()
        .expect("customer row");
    seed_order(&db, "2155467", Some(&customer), None).await;
    let rows: Vec<ServiceOrderRow> = db
        .query(SERVICE_ORDER_BY_NUMBER_SQL)
        .bind(("sn", "2155467"))
        .await
        .expect("query")
        .take(0)
        .expect("order rows");
    let order = rows.into_iter().next().expect("order found");
    assert_eq!(order.customer, Some(customer));
    assert_eq!(order.customer_name.as_deref(), Some("Barbara Baker"));
    assert_eq!(order.checkin_notes.as_deref(), Some("CPS and annual tuneup"));
    assert_eq!(order.computer, None);
}

#[tokio::test]
async fn a_new_service_task_satisfies_the_task_schema_and_notifies_the_assignee() {
    let db = mem_db().await;
    seed_user(&db).await;
    let order = seed_order(&db, "2155467", None, None).await;
    let order_row = ServiceOrderRow {
        id: order.clone(),
        service_number: Some("2155467".into()),
        customer: None,
        customer_name: Some("Barbara Baker".into()),
        computer: None,
        checkin_notes: Some("CPS and annual tuneup".into()),
    };
    let derek = Requester {
        id: rid("user", "derek"),
        name: Some("Derek Anderson".into()),
        email: Some("derek.anderson@pclaptops.com".into()),
        store: Some("MUR".into()),
    };
    let task = NewServiceTask::for_order(&order_row, "2155467", None, &derek, Utc::now().into());
    let id = rid("task", "agent-created");
    let created: Option<Record> =
        db.create(id.clone()).content(task).await.expect("task passes the schema");
    assert!(created.is_some());

    let stored = row(&db, &id).await;
    assert_eq!(text(&stored, "task_name").as_deref(), Some("Barbara Baker - 2155467"));
    assert_eq!(text(&stored, "service_number").as_deref(), Some("2155467"));
    assert_eq!(text(&stored, "priority").as_deref(), Some("Normal"));
    assert_eq!(text(&stored, "status").as_deref(), Some("Todo"));
    assert_eq!(text(&stored, "origin").as_deref(), Some("ai"));
    assert_eq!(stored.get("completed"), Some(&Value::Bool(false)));
    assert_eq!(link(&stored, "service_ticket").as_deref(), Some("2155467"));
    assert_eq!(link(&stored, "assignee").as_deref(), Some("derek"));

    let notified: Vec<Value> = db
        .query("SELECT notification_description FROM notification WHERE user = user:derek")
        .await
        .expect("notifications")
        .take(0)
        .expect("rows");
    assert_eq!(notified.len(), 1, "the task CREATE event notifies the assignee");
}

async fn requester(db: &Surreal<Db>, emails: &[&str], name: &str) -> Option<RecordId> {
    let rows: Vec<Requester> = db
        .query(REQUESTER_SQL)
        .bind(("emails", emails.iter().map(|e| e.to_string()).collect::<Vec<_>>()))
        .bind(("name", name.to_string()))
        .await
        .expect("query")
        .take(0)
        .expect("rows");
    rows.into_iter().next().map(|r| r.id)
}

#[tokio::test]
async fn requesters_resolve_by_email_username_or_name_and_skip_inactive_users() {
    let db = mem_db().await;
    seed_user(&db).await;
    let derek = Some(rid("user", "derek"));
    let email = "derek.anderson@pclaptops.com";
    assert_eq!(requester(&db, &[email], email).await, derek);
    assert_eq!(requester(&db, &["derek.anderson", email], "derek.anderson").await, derek);
    assert_eq!(
        requester(&db, &["derek anderson", "derek anderson@pclaptops.com"], "derek anderson").await,
        derek
    );
    assert_eq!(requester(&db, &["old.tech@pclaptops.com"], "old.tech@pclaptops.com").await, None);
}

#[tokio::test]
async fn order_links_fill_only_what_is_unset() {
    let db = mem_db().await;
    let bench = rid("computer", "BENCH:1");
    let order = seed_order(&db, "2155467", None, Some(&bench)).await;
    db.query(FILL_ORDER_LINKS_SQL)
        .bind(("so", order.clone()))
        .bind(("computer", Some(rid("computer", "DESKTOP-787KAB8:8d3db801f"))))
        .bind(("customer", Some(rid("customer", "4215"))))
        .await
        .expect("update")
        .check()
        .expect("fill links");
    let stored = row(&db, &order).await;
    assert_eq!(link(&stored, "computer").as_deref(), Some("BENCH:1"));
    assert_eq!(link(&stored, "customer").as_deref(), Some("4215"));
}

async fn seed_session(
    db: &Surreal<Db>,
    key: &str,
    links: Option<(&RecordId, &RecordId, &RecordId)>,
) -> RecordId {
    let id = rid("diagnostic_session", key);
    let (task, so, cust) = match links {
        Some((t, s, c)) => (Some(t.clone()), Some(s.clone()), Some(c.clone())),
        None => (None, None, None),
    };
    db.query(
        "CREATE $id CONTENT { connection_string: 'DESKTOP-787KAB8:8d3db801f', \
         hostname: 'DESKTOP-787KAB8', status: 'open', tags: [], task_ref: $task, \
         service_order: $so, customer_id: $cust, \
         customer_name: IF $cust != NONE THEN 'Old Customer' ELSE NONE END, started_at: $started }",
    )
    .bind(("id", id.clone()))
    .bind(("task", task))
    .bind(("so", so))
    .bind(("cust", cust))
    .bind(("started", ago(60)))
    .await
    .expect("seed session")
    .check()
    .expect("session row");
    id
}

async fn adopt_session(db: &Surreal<Db>, sid: &RecordId, task: &RecordId, so: &RecordId, cust: &RecordId) {
    db.query(ADOPT_SESSION_LINKS_SQL)
        .bind(("sid", sid.clone()))
        .bind(("cust", Some(cust.clone())))
        .bind(("cust_name", Some("Barbara Baker".to_string())))
        .bind(("task", task.clone()))
        .bind(("svc", so.clone()))
        .await
        .expect("adopt")
        .check()
        .expect("adopt statements");
}

#[tokio::test]
async fn a_session_adopts_links_it_lacks_and_keeps_the_ones_it_has() {
    let db = mem_db().await;
    let (task, so, cust) = (rid("task", "new"), rid("service_order", "2155467"), rid("customer", "4215"));
    let bare = seed_session(&db, "bare", None).await;
    adopt_session(&db, &bare, &task, &so, &cust).await;
    let stored = row(&db, &bare).await;
    assert_eq!(link(&stored, "task_ref").as_deref(), Some("new"));
    assert_eq!(link(&stored, "service_order").as_deref(), Some("2155467"));
    assert_eq!(link(&stored, "customer_id").as_deref(), Some("4215"));
    assert_eq!(text(&stored, "customer_name").as_deref(), Some("Barbara Baker"));
    assert!(stored.get("last_activity_at").is_some_and(|v| !v.is_null()));

    let old = (rid("task", "old"), rid("service_order", "2100000"), rid("customer", "1"));
    let linked = seed_session(&db, "linked", Some((&old.0, &old.1, &old.2))).await;
    adopt_session(&db, &linked, &task, &so, &cust).await;
    let stored = row(&db, &linked).await;
    assert_eq!(link(&stored, "task_ref").as_deref(), Some("old"));
    assert_eq!(link(&stored, "service_order").as_deref(), Some("2100000"));
    assert_eq!(link(&stored, "customer_id").as_deref(), Some("1"));
    assert_eq!(text(&stored, "customer_name").as_deref(), Some("Old Customer"));
}

async fn adopt_thread(db: &Surreal<Db>, cs: &str) -> Vec<RecordId> {
    db.query(ADOPT_THREAD_LINKS_SQL)
        .bind(("cs", cs.to_string()))
        .bind(("sn", "2155467".to_string()))
        .bind(("so", rid("service_order", "2155467")))
        .bind(("cust", Some(rid("customer", "4215"))))
        .await
        .expect("adopt")
        .take(0)
        .expect("ids")
}

#[tokio::test]
async fn only_the_machines_open_thread_on_that_service_number_adopts_the_order() {
    let db = mem_db().await;
    let cs = "DESKTOP-787KAB8:8d3db801f";
    for (key, conn, status, sn) in [
        ("live", cs, "running", None),
        ("other-order", cs, "idle", Some("2100000")),
        ("closed", cs, "closed", None),
        ("other-machine", "DESKTOP-OTHER:1", "running", None),
    ] {
        db.query("CREATE $id CONTENT { connection_string: $cs, status: $status, service_number: $sn }")
            .bind(("id", rid("agent_thread", key)))
            .bind(("cs", conn.to_string()))
            .bind(("status", status.to_string()))
            .bind(("sn", sn.map(str::to_string)))
            .await
            .expect("seed thread")
            .check()
            .expect("thread row");
    }
    assert_eq!(adopt_thread(&db, cs).await, vec![rid("agent_thread", "live")]);
    let live = row(&db, &rid("agent_thread", "live")).await;
    assert_eq!(text(&live, "service_number").as_deref(), Some("2155467"));
    assert_eq!(link(&live, "service_order").as_deref(), Some("2155467"));
    assert_eq!(link(&live, "customer").as_deref(), Some("4215"));
    assert!(adopt_thread(&db, cs).await.is_empty(), "a thread with both links is not rewritten");
    for key in ["other-order", "closed", "other-machine"] {
        assert_eq!(link(&row(&db, &rid("agent_thread", key)).await, "service_order"), None, "{key}");
    }
}

#[derive(Default)]
struct RunLinks {
    session: Option<RecordId>,
    order: Option<RecordId>,
    task: Option<RecordId>,
}

async fn seed_run(
    db: &Surreal<Db>,
    key: &str,
    computer: &RecordId,
    started: Datetime,
    links: RunLinks,
) -> RecordId {
    let id = rid("stress_test_run", key);
    db.query(
        "CREATE $id CONTENT { computer: $comp, target_kind: 'cpu', tool: {}, \
         tool_label: 'stresskit:cpu', failure_mode: {}, summary: {}, started_at: $started, \
         session_ref: $sess, service_order: $so, task_ref: $task }",
    )
    .bind(("id", id.clone()))
    .bind(("comp", computer.clone()))
    .bind(("started", started))
    .bind(("sess", links.session))
    .bind(("so", links.order))
    .bind(("task", links.task))
    .await
    .expect("seed run")
    .check()
    .expect("run row");
    id
}

#[tokio::test]
async fn stress_run_claims_stay_inside_the_engagement() {
    let db = mem_db().await;
    let computer = rid("computer", "DESKTOP-787KAB8:8d3db801f");
    let other_pc = rid("computer", "DESKTOP-OTHER:1");
    let (sid, task, so) = (rid("diagnostic_session", "s1"), rid("task", "t1"), rid("service_order", "2155467"));

    let inside = seed_run(&db, "inside", &computer, ago(30), RunLinks::default()).await;
    let slack = seed_run(&db, "slack", &computer, ago(70), RunLinks::default()).await;
    let same_order =
        seed_run(&db, "same-order", &computer, ago(20), RunLinks { order: Some(so.clone()), ..Default::default() }).await;
    let early = seed_run(&db, "early", &computer, ago(80), RunLinks::default()).await;
    let elsewhere = seed_run(&db, "elsewhere", &other_pc, ago(30), RunLinks::default()).await;
    let foreign_order = seed_run(
        &db,
        "foreign-order",
        &computer,
        ago(30),
        RunLinks { order: Some(rid("service_order", "2100000")), ..Default::default() },
    )
    .await;
    let foreign_task =
        seed_run(&db, "foreign-task", &computer, ago(30), RunLinks { task: Some(rid("task", "t0")), ..Default::default() }).await;
    let other_session = seed_run(
        &db,
        "other-session",
        &computer,
        ago(30),
        RunLinks { session: Some(rid("diagnostic_session", "s0")), ..Default::default() },
    )
    .await;

    let mut claimed: Vec<RecordId> = db
        .query(CLAIM_STRESS_RUNS_SQL)
        .bind(("sid", sid.clone()))
        .bind(("task", Some(task.clone())))
        .bind(("so", Some(so.clone())))
        .bind(("comp", computer.clone()))
        .bind(("started", ago(60)))
        .bind(("ended", None::<Datetime>))
        .await
        .expect("claim")
        .take(0)
        .expect("claimed ids");
    claimed.sort();
    let mut expected = vec![inside.clone(), same_order.clone(), slack.clone()];
    expected.sort();
    assert_eq!(claimed, expected);
    for id in [&inside, &slack, &same_order] {
        let stored = row(&db, id).await;
        assert_eq!(link(&stored, "session_ref").as_deref(), Some("s1"));
        assert_eq!(link(&stored, "task_ref").as_deref(), Some("t1"));
        assert_eq!(link(&stored, "service_order").as_deref(), Some("2155467"));
    }
    for id in [&early, &elsewhere, &foreign_order, &foreign_task] {
        assert_eq!(link(&row(&db, id).await, "session_ref"), None, "{id:?} was claimed");
    }
    assert_eq!(link(&row(&db, &other_session).await, "session_ref").as_deref(), Some("s0"));
}

#[tokio::test]
async fn a_closed_engagement_claims_nothing_after_it_ended() {
    let db = mem_db().await;
    let computer = rid("computer", "DESKTOP-787KAB8:8d3db801f");
    let late = seed_run(&db, "late", &computer, ago(5), RunLinks::default()).await;
    let claimed: Vec<RecordId> = db
        .query(CLAIM_STRESS_RUNS_SQL)
        .bind(("sid", rid("diagnostic_session", "s1")))
        .bind(("task", None::<RecordId>))
        .bind(("so", None::<RecordId>))
        .bind(("comp", computer))
        .bind(("started", ago(60)))
        .bind(("ended", Some(ago(30))))
        .await
        .expect("claim")
        .take(0)
        .expect("claimed ids");
    assert!(claimed.is_empty());
    assert_eq!(link(&row(&db, &late).await, "session_ref"), None);
}

#[tokio::test]
async fn session_runs_gain_the_order_unless_they_belong_to_another_task() {
    let db = mem_db().await;
    let computer = rid("computer", "DESKTOP-787KAB8:8d3db801f");
    let (sid, task, so) = (rid("diagnostic_session", "s1"), rid("task", "t1"), rid("service_order", "2155467"));
    let on_session = |task: Option<RecordId>, order: Option<RecordId>| RunLinks {
        session: Some(sid.clone()),
        order,
        task,
    };
    let bare = seed_run(&db, "bare", &computer, ago(30), on_session(None, None)).await;
    let same_task = seed_run(&db, "same-task", &computer, ago(30), on_session(Some(task.clone()), None)).await;
    let other_task =
        seed_run(&db, "other-task", &computer, ago(30), on_session(Some(rid("task", "t0")), None)).await;
    let other_order = seed_run(
        &db,
        "other-order",
        &computer,
        ago(30),
        on_session(None, Some(rid("service_order", "2100000"))),
    )
    .await;

    let mut linked: Vec<RecordId> = db
        .query(LINK_STRESS_RUN_ORDERS_SQL)
        .bind(("sid", sid.clone()))
        .bind(("so", so.clone()))
        .bind(("task", Some(task.clone())))
        .await
        .expect("link")
        .take(0)
        .expect("linked ids");
    linked.sort();
    let mut expected = vec![bare, same_task];
    expected.sort();
    assert_eq!(linked, expected);
    assert_eq!(link(&row(&db, &other_task).await, "service_order"), None);
    assert_eq!(link(&row(&db, &other_order).await, "service_order").as_deref(), Some("2100000"));
}
