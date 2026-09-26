//! The assistant schema, event, permissions, job SQL and rollout against in-memory SurrealDB.

use chrono::{Duration, Utc};
use database::schema::assistant::{
    COMPLETE_ASSISTANT_TASK_SQL, CREATE_ASSIGNED_TASK_SQL, OVERDUE_ASSIGNED_SQL, OverdueTask, POST_BRIEF_SQL,
    UNSNOOZE_SQL,
};
use database::schema::task_schedule::{CLAIM_RUN_SQL, DUE_SCHEDULES_SQL, SCHEMA_APPLIED_SQL, TaskSchedule};
use database::schema::{AiProfile, Datetime, RecordId};
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;

const USER: &str = include_str!("../schema/user.surql");
const TASK: &str = include_str!("../schema/task.surql");
const NOTIFICATION: &str = include_str!("../schema/notification.surql");
const TASK_NOTE: &str = include_str!("../schema/task_note.surql");
const TASK_SCHEDULE: &str = include_str!("../schema/task_schedule.surql");
const ROLLOUT: &str = include_str!("../rollouts/20260926170000__ai_assistant_tools.toml");

const SEED: &str = "
    DEFINE ACCESS user ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM user WHERE email = $email);
    CREATE user:logan CONTENT { name: 'Logan Lees', email: 'logan@x.com', store: 'RIV', authorization: 'Root',
        everest_initials: 'LL', password: 'x', version: '4.8.5',
        user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } } };
    CREATE user:ana CONTENT { name: 'Ana Ortiz', email: 'ana@x.com', store: 'RIV', authorization: 'Manager',
        everest_initials: 'AO', password: 'x', version: '4.8.5',
        user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } } };
    CREATE user:sam CONTENT { name: 'Sam Jones', email: 'sam@x.com', store: 'RIV', authorization: 'User',
        everest_initials: 'SJ', password: 'x', version: '4.8.5',
        user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } } };
    CREATE user:kim CONTENT { name: 'Kim Park', email: 'kim@x.com', store: 'LTN', authorization: 'User',
        everest_initials: 'KP', password: 'x', version: '4.8.5',
        user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } } };
";

/// The SQL of the rollout step named `id`.
fn rollout_step(id: &str) -> &'static str {
    let step = &ROLLOUT[ROLLOUT.find(&format!("id = \"{id}\"")).expect("the step")..];
    let body = &step[step.find("sql = \"\"\"").expect("its sql") + 9..];
    &body[..body.find("\"\"\"").expect("the sql end")]
}

/// `schema` with OVERWRITE on every DEFINE.
fn overwrite(schema: &str) -> String {
    ["TABLE", "FIELD", "INDEX", "EVENT"].iter().fold(schema.to_string(), |s, kind| {
        s.replace(&format!("DEFINE {kind} "), &format!("DEFINE {kind} OVERWRITE "))
            .replace(&format!("DEFINE {kind} OVERWRITE OVERWRITE "), &format!("DEFINE {kind} OVERWRITE "))
    })
}

async fn mem_db(schemas: &[&str]) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for schema in schemas {
        db.query(overwrite(schema)).await.expect("apply schema").check().expect("schema statements");
    }
    db.query(SEED).await.expect("seed").check().expect("seed statements");
    db
}

async fn as_user(db: &Surreal<Db>, email: &str) -> Surreal<Db> {
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

async fn json_rows(db: &Surreal<Db>, sql: &str) -> Vec<Value> {
    let value: surrealdb::types::Value =
        db.query(sql).await.expect("query").check().expect("ok").take(0).expect("rows");
    match value.into_json_value() {
        Value::Array(rows) => rows,
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

fn user(key: &str) -> RecordId {
    RecordId::new("user", key)
}

async fn create_assigned(db: &Surreal<Db>, by: Option<RecordId>, part: Option<Value>) {
    db.query(CREATE_ASSIGNED_TASK_SQL)
        .bind(("name", "Count paste"))
        .bind(("description", ""))
        .bind(("assignee", user("sam")))
        .bind(("assigned_by", by))
        .bind(("due", Datetime::from(Utc::now())))
        .bind(("priority", "Normal"))
        .bind(("origin", "ai"))
        .bind(("schedule", None::<RecordId>))
        .bind(("about", None::<String>))
        .bind(("part", part))
        .await
        .expect("create")
        .check()
        .expect("assistant task");
}

#[tokio::test]
async fn schedules_follow_the_store_rule() {
    let db = mem_db(&[USER, TASK_SCHEDULE]).await;
    let sam = as_user(&db, "sam@x.com").await;
    let create = "CREATE task_schedule CONTENT { title: 'Count paste', assignee: $who, every: 'day', at: '10:00' }";
    for who in ["sam", "ana", "kim"] {
        sam.query(create).bind(("who", user(who))).await.expect("create").check().ok();
    }
    let logan = as_user(&db, "logan@x.com").await;
    logan.query(create).bind(("who", user("kim"))).await.expect("create").check().expect("root may assign anyone");

    let all =
        json_rows(&db, "SELECT VALUE string::concat(created_by.email, '>', assignee.email) FROM task_schedule").await;
    let mut pairs: Vec<String> = all.iter().filter_map(|p| p.as_str().map(str::to_string)).collect();
    pairs.sort();
    assert_eq!(pairs, ["logan@x.com>kim@x.com", "sam@x.com>ana@x.com", "sam@x.com>sam@x.com"]);

    let kim = as_user(&db, "kim@x.com").await;
    let seen = json_rows(&kim, "SELECT VALUE created_by FROM task_schedule").await;
    assert_eq!(seen.len(), 1, "kim sees only the schedule assigned to her");

    kim.query("UPDATE task_schedule SET title = 'hijacked'").await.expect("update").check().ok();
    let titles = json_rows(&db, "SELECT VALUE title FROM task_schedule WHERE created_by = user:sam").await;
    assert!(titles.iter().all(|t| t == "Count paste"), "{titles:?}");

    sam.query("UPDATE task_schedule SET assignee = user:kim WHERE created_by = user:sam")
        .await
        .expect("update")
        .check()
        .ok();
    let moved = json_rows(&db, "SELECT VALUE assignee.email FROM task_schedule WHERE created_by = user:sam").await;
    assert!(moved.iter().all(|e| e != "kim@x.com"), "assignee is read-only: {moved:?}");
}

#[tokio::test]
async fn the_task_event_names_the_sender_and_links_the_task() {
    let db = mem_db(&[USER, TASK_SCHEDULE, NOTIFICATION, TASK]).await;
    create_assigned(&db, Some(user("logan")), None).await;
    let part = json!({ "part": "1TB NVMe", "quantity": 1, "from_store": "SAN", "to_store": "RIV" });
    create_assigned(&db, Some(user("ana")), Some(part)).await;
    db.query("CREATE task CONTENT { task_name: 'Kayleen Reese - 2154905', assignee: user:sam }")
        .await
        .expect("create")
        .check()
        .expect("ok");

    let notes = json_rows(&db, "SELECT notification_type, notification_description, task != NONE AS linked, from_user FROM notification ORDER BY notification_description").await;
    let got: Vec<(String, String, bool)> = notes
        .iter()
        .map(|n| {
            (
                n["notification_type"].as_str().unwrap_or_default().to_string(),
                n["notification_description"].as_str().unwrap_or_default().to_string(),
                n["linked"].as_bool().unwrap_or(false),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("Part Request".to_string(), "Ana Ortiz: Count paste".to_string(), true),
            ("Reminder".to_string(), "Logan Lees: Count paste".to_string(), true),
            ("Task Created".to_string(), "New Task for Sam Jones".to_string(), true),
        ]
    );
}

#[tokio::test]
async fn an_ai_profile_is_owner_edited_and_hidden_from_coworkers() {
    let db = mem_db(&[USER]).await;
    let sam = as_user(&db, "sam@x.com").await;
    let profile =
        AiProfile { assistant_name: Some("Jarvis".into()), detail: Some("brief".into()), ..Default::default() };
    sam.query("UPDATE $auth.id SET ai_profile = $p")
        .bind(("p", profile.clone()))
        .await
        .expect("save")
        .check()
        .expect("owner saves");
    let own: Option<Option<AiProfile>> =
        sam.query("SELECT VALUE ai_profile FROM user:sam").await.expect("read").take(0).expect("row");
    assert_eq!(own.flatten(), Some(profile.clone()));

    let kim = as_user(&db, "kim@x.com").await;
    let peek = json_rows(&kim, "SELECT VALUE ai_profile FROM user:sam").await;
    assert!(peek.iter().all(Value::is_null), "a coworker must not read it: {peek:?}");
    kim.query("UPDATE user:sam SET ai_profile.assistant_name = 'Hacked'").await.expect("update").check().ok();

    let logan = as_user(&db, "logan@x.com").await;
    let seen: Option<Option<AiProfile>> =
        logan.query("SELECT VALUE ai_profile FROM user:sam").await.expect("read").take(0).expect("row");
    assert_eq!(seen.flatten(), Some(profile));
    let too_long = AiProfile { about_me: Some("x".repeat(501)), ..Default::default() };
    assert!(
        sam.query("UPDATE $auth.id SET ai_profile = $p").bind(("p", too_long)).await.expect("save").check().is_err()
    );
}

#[tokio::test]
async fn job_sql_claims_once_and_rows_deserialize() {
    let db = mem_db(&[USER, TASK_SCHEDULE, NOTIFICATION, TASK, TASK_NOTE]).await;
    let applied: Option<bool> = db.query(SCHEMA_APPLIED_SQL).await.expect("info").take(0).expect("value");
    assert_eq!(applied, Some(true));

    let due_at = Datetime::from(Utc::now() - Duration::minutes(5));
    db.query("CREATE task_schedule:s CONTENT { title: 'Count paste', assignee: user:sam, created_by: user:logan, every: 'week', weekdays: [1], at: '10:00', next_run: $at }")
        .bind(("at", due_at.clone()))
        .await
        .expect("create")
        .check()
        .expect("ok");
    let due: Vec<TaskSchedule> =
        db.query(DUE_SCHEDULES_SQL).bind(("limit", 10)).await.expect("due").take(0).expect("rows");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].describe(), "Mondays at 10:00");
    let claim = |next: Datetime| {
        db.query(CLAIM_RUN_SQL)
            .bind(("id", RecordId::new("task_schedule", "s")))
            .bind(("expected", due_at.clone()))
            .bind(("next", Some(next)))
            .bind(("fired", Datetime::from(Utc::now())))
            .bind(("active", true))
    };
    let first: Vec<RecordId> =
        claim(Datetime::from(Utc::now() + Duration::days(7))).await.expect("claim").take(0).expect("ids");
    let second: Vec<RecordId> =
        claim(Datetime::from(Utc::now() + Duration::days(7))).await.expect("claim").take(0).expect("ids");
    assert_eq!((first.len(), second.len()), (1, 0), "a run is claimed exactly once");

    db.query("CREATE notification:n CONTENT { user: user:sam, notification_type: 'Reminder', notification_description: 'x', status: 'Snoozed', snooze_until: time::now() - 1m }")
        .await
        .expect("create")
        .check()
        .expect("ok");
    let back: Vec<RecordId> = db.query(UNSNOOZE_SQL).await.expect("unsnooze").take(0).expect("ids");
    assert_eq!(back.len(), 1);

    db.query("CREATE task:ticket CONTENT { task_name: 'Kayleen Reese - 2154905', assignee: user:sam, service_number: '2154905' }; \
               CREATE task:chore CONTENT { task_name: 'Count paste', assignee: user:sam, assigned_by: user:logan, due_date: time::now() - 3h }")
        .await
        .expect("create")
        .check()
        .expect("ok");
    for _ in 0..2 {
        db.query(POST_BRIEF_SQL)
            .bind(("task", RecordId::new("task", "ticket")))
            .bind(("note", "Now: x\nFound: y\nTell the customer: z"))
            .bind(("author_name", "AI brief"))
            .bind(("author", user("sam")))
            .bind(("sn", "2154905"))
            .await
            .expect("brief")
            .check()
            .expect("ok");
    }
    let briefs = json_rows(&db, "SELECT VALUE private FROM task_note WHERE kind = 'ai_brief'").await;
    assert_eq!(briefs, vec![json!(true)], "one private brief per ticket");

    let overdue: Vec<OverdueTask> = db
        .query(OVERDUE_ASSIGNED_SQL)
        .bind(("cutoff", Datetime::from(Utc::now() - Duration::hours(1))))
        .await
        .expect("overdue")
        .take(0)
        .expect("rows");
    assert_eq!(overdue.len(), 1);
    assert_eq!(overdue[0].store.as_deref(), Some("RIV"));

    let ticket: Vec<RecordId> = db
        .query(COMPLETE_ASSISTANT_TASK_SQL)
        .bind(("id", RecordId::new("task", "ticket")))
        .await
        .expect("complete")
        .take(0)
        .expect("ids");
    let chore: Vec<RecordId> = db
        .query(COMPLETE_ASSISTANT_TASK_SQL)
        .bind(("id", RecordId::new("task", "chore")))
        .await
        .expect("complete")
        .take(0)
        .expect("ids");
    assert_eq!((ticket.len(), chore.len()), (0, 1), "only assistant tasks complete from a notification");
}

#[tokio::test]
async fn the_rollout_applies_over_existing_tables_and_rolls_back() {
    let db = mem_db(&[USER, NOTIFICATION, TASK, TASK_NOTE]).await;
    db.query(rollout_step("define_ai_assistant_tools")).await.expect("rollout").check().expect("rollout statements");
    let tables = json_rows(&db, "RETURN (INFO FOR DB).tables.task_schedule != NONE").await;
    assert_eq!(tables, vec![json!(true)]);

    db.query(rollout_step("remove_ai_assistant_tools")).await.expect("rollback").check().expect("rollback statements");
    let gone = json_rows(&db, "RETURN (INFO FOR DB).tables.task_schedule = NONE").await;
    assert_eq!(gone, vec![json!(true)]);
    db.query("CREATE task CONTENT { task_name: 'Kayleen Reese - 2154905', assignee: user:sam }")
        .await
        .expect("create")
        .check()
        .expect("ok");
    let kinds = json_rows(&db, "SELECT VALUE notification_type FROM notification").await;
    assert_eq!(kinds, vec![json!("Task Created")]);
}
