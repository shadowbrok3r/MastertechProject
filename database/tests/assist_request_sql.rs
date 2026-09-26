//! assist_request statements against in-memory SurrealDB with the real table definition.

use database::schema::assist::{CREATE_CONFIRMED_SQL, WITHDRAW_SQL};
use database::schema::{random_record_id, AssistRequest, RecordId, ASSIST_REQUEST_TABLE};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

const SCHEMA: &[&str] = &[
    include_str!("../schema/assist_request.surql"),
    "DEFINE TABLE computer TYPE ANY SCHEMALESS;",
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

async fn read(db: &Surreal<Db>, id: &RecordId) -> AssistRequest {
    let rows: Vec<AssistRequest> = db
        .query("SELECT * FROM $id")
        .bind(("id", id.clone()))
        .await
        .expect("select")
        .take(0)
        .expect("rows");
    rows.into_iter().next().expect("the request row")
}

#[tokio::test]
async fn a_confirmed_request_lands_under_its_client_id_and_asks_for_a_fresh_session() {
    let db = mem_db().await;
    let id = random_record_id(ASSIST_REQUEST_TABLE);
    db.query(CREATE_CONFIRMED_SQL)
        .bind(("id", id.clone()))
        .bind(("cs", "DESKTOP-787KAB8:8d3db801f"))
        .bind(("host", "DESKTOP-787KAB8"))
        .bind(("sn", "2155467"))
        .bind(("computer", RecordId::new("computer", "DESKTOP-787KAB8:8d3db801f")))
        .bind(("by", "derek.anderson@pclaptops.com"))
        .bind(("store", "MUR"))
        .await
        .expect("create")
        .check()
        .expect("create statement");

    let req = read(&db, &id).await;
    assert_eq!(req.id, id);
    assert!(req.fresh);
    assert!(req.machine_confirmed);
    assert_eq!(req.trigger_source, "tur_sheet");
    assert_eq!(req.status, "pending");
    assert_eq!(req.service_number.as_deref(), Some("2155467"));
    assert_eq!(req.agent_thread, None);
}

#[tokio::test]
async fn a_request_written_without_the_flag_reads_back_as_joining() {
    let db = mem_db().await;
    let id = RecordId::new(ASSIST_REQUEST_TABLE, "older-client");
    db.query("CREATE $id CONTENT { connection_string: 'PC-1:abc', trigger_source: 'chat', status: 'pending' }")
        .bind(("id", id.clone()))
        .await
        .expect("create")
        .check()
        .expect("create statement");

    assert!(!read(&db, &id).await.fresh);
}

async fn withdraw(db: &Surreal<Db>, id: &RecordId) -> Vec<RecordId> {
    db.query(WITHDRAW_SQL)
        .bind(("id", id.clone()))
        .await
        .expect("withdraw")
        .take(0)
        .expect("withdrawn ids")
}

#[tokio::test]
async fn withdrawing_declines_a_pending_request() {
    let db = mem_db().await;
    let id = RecordId::new(ASSIST_REQUEST_TABLE, "waited-out");
    db.query("CREATE $id CONTENT { connection_string: 'PC-1:abc', trigger_source: 'chat', status: 'pending', fresh: true }")
        .bind(("id", id.clone()))
        .await
        .expect("create")
        .check()
        .expect("create statement");

    assert_eq!(withdraw(&db, &id).await, vec![id.clone()]);
    let req = read(&db, &id).await;
    assert_eq!(req.status, "declined");
    assert_eq!(req.dispatch_error.as_deref(), Some("the client stopped waiting"));
}

#[tokio::test]
async fn withdrawing_leaves_a_claimed_request_alone() {
    let db = mem_db().await;
    let id = RecordId::new(ASSIST_REQUEST_TABLE, "claimed");
    db.query("CREATE $id CONTENT { connection_string: 'PC-1:abc', trigger_source: 'chat', status: 'dispatched' }")
        .bind(("id", id.clone()))
        .await
        .expect("create")
        .check()
        .expect("create statement");

    assert!(withdraw(&db, &id).await.is_empty());
    let req = read(&db, &id).await;
    assert_eq!(req.status, "dispatched");
    assert_eq!(req.dispatch_error, None);
}
