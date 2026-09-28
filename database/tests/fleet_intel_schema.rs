//! The fleet_intel SQL against in-memory SurrealDB: the case projection, the latest complaint, the open ticket,
//! the fleet note and the internal-machine flag.

use database::schema::fleet_intel::{
    computer_key_from_value, model_from_value, open_ticket_from_value, str_field, FLEET_NOTE_AUTHOR, FLEET_NOTE_SQL,
    IS_INTERNAL_SQL, LATEST_COMPLAINT_SQL, OPEN_TICKET_SQL, POST_FLEET_NOTE_SQL, SIMILAR_CASES_SQL,
};
use database::schema::RecordId;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

const COMPUTER: &str = include_str!("../schema/computer.surql");
const SERVICE_ORDER: &str = include_str!("../schema/service_order.surql");
const DIAGNOSTIC_SESSION: &str = include_str!("../schema/diagnostic_session.surql");
const TASK: &str = include_str!("../schema/task.surql");
const TASK_NOTE: &str = include_str!("../schema/task_note.surql");

fn overwrite(schema: &str) -> String {
    ["TABLE", "FIELD", "INDEX", "EVENT"].iter().fold(schema.to_string(), |s, kind| {
        s.replace(&format!("DEFINE {kind} "), &format!("DEFINE {kind} OVERWRITE "))
            .replace(&format!("DEFINE {kind} OVERWRITE OVERWRITE "), &format!("DEFINE {kind} OVERWRITE "))
    })
}

/// A service order with its required fields filled.
fn order(key: &str, service_number: &str, notes: &str, computer: &str) -> String {
    format!(
        "CREATE service_order:{key} CONTENT {{ service_number: '{service_number}', checkin_notes: '{notes}', \
         computer: computer:{computer}, checkin_rep: '', sales_rep: '', tech: '', terms: '', ticket_total: '', \
         doc_alias: '', hardware_test_results: {{}} }};"
    )
}

async fn mem_db(schemas: &[&str], seed: &str) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for schema in schemas {
        db.query(overwrite(schema)).await.expect("apply schema").check().expect("schema statements");
    }
    db.query(seed).await.expect("seed").check().expect("seed statements");
    db
}

#[tokio::test]
async fn similar_cases_projection_traverses_record_links() {
    let seed = [
        "CREATE computer:pc1 CONTENT { hostname: 'PC1', product_vendor: 'HP', product_name: 'Victus 15' };",
        "CREATE computer:pc2 CONTENT { hostname: 'PC2', product_vendor: 'hp', product_name: 'victus 15' };",
        "CREATE computer:dell CONTENT { hostname: 'DELL', product_vendor: 'Dell Inc.', product_name: 'XPS 15' };",
        &order("so1", "2155001", "random blue screen crashing while gaming", "pc1"),
        &order("so2", "2155002", "no power", "dell"),
        "CREATE diagnostic_session:d1 CONTENT { connection_string: 'PC1:aaa', hostname: 'PC1', status: 'resolved', \
         summary: 'Reseated RAM, memtest clean', computer_id: computer:pc1, service_order: service_order:so1, \
         diagnosed_at: time::now(), started_at: time::now(), tags: [] };",
        "CREATE diagnostic_session:d2 CONTENT { connection_string: 'DELL:bbb', hostname: 'DELL', status: 'resolved', \
         summary: 'Replaced charger', computer_id: computer:dell, service_order: service_order:so2, \
         diagnosed_at: time::now(), started_at: time::now(), tags: [] };",
        "CREATE diagnostic_session:d3 CONTENT { connection_string: 'PC2:ccc', hostname: 'PC2', status: 'open', \
         summary: 'still looking', computer_id: computer:pc2, started_at: time::now(), tags: [] };",
    ]
    .join("\n");
    let db = mem_db(&[COMPUTER, SERVICE_ORDER, DIAGNOSTIC_SESSION], &seed).await;
    let rows: Vec<serde_json::Value> =
        db.query(SIMILAR_CASES_SQL).bind(("limit", 100i64)).await.expect("query").take(0).expect("rows");

    // Only resolved/escalated sessions with a summary and diagnosed_at (d1, d2), not the open d3.
    assert_eq!(rows.len(), 2, "resolved sessions only");

    let pc1 = rows.iter().find(|r| computer_key_from_value(r, "computer") == "pc1").expect("pc1 case present");
    assert_eq!(model_from_value(pc1), ("hp victus 15".to_string(), "HP Victus 15".to_string()));
    assert_eq!(str_field(pc1, "service_number"), "2155001");
    assert_eq!(str_field(pc1, "hostname"), "PC1");
    assert!(str_field(pc1, "checkin_notes").contains("gaming"));
    let diagnosed = pc1.get("diagnosed_at").and_then(|v| v.as_str()).expect("diagnosed_at string");
    assert!(chrono::DateTime::parse_from_rfc3339(diagnosed).is_ok(), "diagnosed_at is RFC 3339: {diagnosed}");

    let dell = rows.iter().find(|r| computer_key_from_value(r, "computer") == "dell").expect("dell case present");
    assert_eq!(model_from_value(dell).0, "dell xps 15");
}

#[tokio::test]
async fn latest_complaint_is_the_newest_order_inside_the_window() {
    let seed = [
        "CREATE computer:pc1 CONTENT { hostname: 'PC1' };",
        &order("older", "1", "older complaint", "pc1"),
        "UPDATE service_order:older SET created_at = time::now() - 5d;",
        &order("ancient", "2", "ancient complaint", "pc1"),
        "UPDATE service_order:ancient SET created_at = time::now() - 90d;",
        &order("newest", "3", "newest complaint", "pc1"),
    ]
    .join("\n");
    let db = mem_db(&[COMPUTER, SERVICE_ORDER], &seed).await;
    let comp = RecordId::new("computer", "pc1");

    let rows: Vec<serde_json::Value> =
        db.query(LATEST_COMPLAINT_SQL).bind(("comp", comp.clone())).await.expect("query").take(0).expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(str_field(&rows[0], "checkin_notes"), "newest complaint");

    let other: Vec<serde_json::Value> = db
        .query(LATEST_COMPLAINT_SQL)
        .bind(("comp", RecordId::new("computer", "nobody")))
        .await
        .expect("query")
        .take(0)
        .expect("rows");
    assert!(other.is_empty());
}

#[tokio::test]
async fn open_ticket_is_the_open_task_on_the_newest_order() {
    let seed = [
        "CREATE user:sam SET name = 'Sam Jones';",
        "CREATE user:kim SET name = 'Kim Park';",
        "CREATE computer:pc1 CONTENT { hostname: 'PC1' };",
        &order("so_old", "2150000", "old visit", "pc1"),
        "UPDATE service_order:so_old SET created_at = time::now() - 50d;",
        &order("so_new", "2155144", "blue screens", "pc1"),
        "CREATE task:t_old CONTENT { task_name: 'Jane Doe - 2150000', assignee: user:kim, \
         service_ticket: service_order:so_old, service_number: '2150000' };",
        "CREATE task:t_done CONTENT { task_name: 'Jane Doe - 2155144 (done)', assignee: user:kim, \
         service_ticket: service_order:so_new, service_number: '2155144', completed: true };",
        "CREATE task:t_open CONTENT { task_name: 'Jane Doe - 2155144', assignee: user:sam, \
         service_ticket: service_order:so_new, service_number: '2155144' };",
        "CREATE task:t_loose CONTENT { task_name: 'Count thermal paste', assignee: user:kim };",
    ]
    .join("\n");
    let db = mem_db(&[COMPUTER, SERVICE_ORDER, TASK], &seed).await;

    let rows: Vec<serde_json::Value> = db
        .query(OPEN_TICKET_SQL)
        .bind(("computer", RecordId::new("computer", "pc1")))
        .await
        .expect("query")
        .take(1)
        .expect("rows");
    assert_eq!(rows.len(), 1);
    let ticket = open_ticket_from_value(&rows[0]).expect("ticket parses");
    assert_eq!(ticket.task, RecordId::new("task", "t_open"));
    assert_eq!(ticket.assignee, RecordId::new("user", "sam"));
    assert_eq!(ticket.task_name, "Jane Doe - 2155144");
    assert_eq!(ticket.service_number, "2155144");

    // No recent order: the loose task without a service_ticket must not match.
    let none: Vec<serde_json::Value> = db
        .query(OPEN_TICKET_SQL)
        .bind(("computer", RecordId::new("computer", "nobody")))
        .await
        .expect("query")
        .take(1)
        .expect("rows");
    assert!(none.is_empty(), "no order means no ticket: {none:?}");
}

#[tokio::test]
async fn fleet_note_replaces_only_its_own_note() {
    let seed = "CREATE task_note:n1 CONTENT { task_id: task:t1, note: 'customer called', username: 'Sam Jones', \
                user: user:sam };";
    let db = mem_db(&[TASK_NOTE], seed).await;
    let task = RecordId::new("task", "t1");
    for text in ["Seen before:\n• first", "Seen before:\n• second"] {
        let mut res = db
            .query(POST_FLEET_NOTE_SQL)
            .bind(("task", task.clone()))
            .bind(("author", FLEET_NOTE_AUTHOR))
            .bind(("note", text.to_string()))
            .bind(("user", RecordId::new("user", "sam")))
            .bind(("sn", "2155144".to_string()))
            .await
            .expect("post")
            .check()
            .expect("post statements");
        let ids: Vec<RecordId> = res.take(1).expect("id");
        assert_eq!(ids.len(), 1);
    }

    let notes: Vec<String> = db
        .query(FLEET_NOTE_SQL)
        .bind(("task", task.clone()))
        .bind(("author", FLEET_NOTE_AUTHOR))
        .await
        .expect("read")
        .take(0)
        .expect("notes");
    assert_eq!(notes, vec!["Seen before:\n• second".to_string()]);

    let all: Vec<serde_json::Value> = db
        .query("SELECT note, username, private FROM task_note WHERE task_id = $task ORDER BY note")
        .bind(("task", task))
        .await
        .expect("all notes")
        .take(0)
        .expect("rows");
    assert_eq!(all.len(), 2, "the ordinary note survives: {all:?}");
    let fleet = all.iter().find(|n| str_field(n, "username") == FLEET_NOTE_AUTHOR).expect("fleet note");
    assert_eq!(fleet.get("private").and_then(|v| v.as_bool()), Some(true));
    assert!(all.iter().any(|n| str_field(n, "note") == "customer called"));
}

#[tokio::test]
async fn internal_flag_reads_none_as_false() {
    let seed = "CREATE computer:pc1 CONTENT { hostname: 'PC1' }; \
                CREATE computer:staff CONTENT { hostname: 'STAFF', is_internal: true };";
    let db = mem_db(&[COMPUTER], seed).await;
    let flag = |key: &'static str| {
        let db = db.clone();
        async move {
            let flags: Vec<bool> = db
                .query(IS_INTERNAL_SQL)
                .bind(("comp", RecordId::new("computer", key)))
                .await
                .expect("query")
                .take(0)
                .expect("flags");
            flags
        }
    };
    assert_eq!(flag("pc1").await, vec![false]);
    assert_eq!(flag("staff").await, vec![true]);
    assert!(flag("nobody").await.is_empty());
}
