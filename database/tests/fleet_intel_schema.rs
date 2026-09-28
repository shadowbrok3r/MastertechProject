//! The fleet_intel same-model case query and its record-link projection against
//! in-memory SurrealDB: proves `computer_id.<field>` / `service_order.<field>`
//! traversal returns what the value parsers expect.

use database::schema::fleet_intel::{
    computer_key_from_value, model_from_value, str_field, SIMILAR_CASES_SQL,
};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

const COMPUTER: &str = include_str!("../schema/computer.surql");
const SERVICE_ORDER: &str = include_str!("../schema/service_order.surql");
const DIAGNOSTIC_SESSION: &str = include_str!("../schema/diagnostic_session.surql");

const SEED: &str = "
    CREATE computer:pc1 CONTENT { hostname: 'PC1', product_vendor: 'HP', product_name: 'Victus 15' };
    CREATE computer:pc2 CONTENT { hostname: 'PC2', product_vendor: 'hp', product_name: 'victus 15' };
    CREATE computer:dell CONTENT { hostname: 'DELL', product_vendor: 'Dell Inc.', product_name: 'XPS 15' };
    CREATE service_order:so1 CONTENT { service_number: '2155001', checkin_notes: 'random blue screen crashing while gaming', computer: computer:pc1, checkin_rep: '', sales_rep: '', tech: '', terms: '', ticket_total: '', doc_alias: '', hardware_test_results: {} };
    CREATE service_order:so2 CONTENT { service_number: '2155002', checkin_notes: 'no power', computer: computer:dell, checkin_rep: '', sales_rep: '', tech: '', terms: '', ticket_total: '', doc_alias: '', hardware_test_results: {} };
    CREATE diagnostic_session:d1 CONTENT { connection_string: 'PC1:aaa', hostname: 'PC1', status: 'resolved', summary: 'Reseated RAM, memtest clean', computer_id: computer:pc1, service_order: service_order:so1, diagnosed_at: time::now(), started_at: time::now(), tags: [] };
    CREATE diagnostic_session:d2 CONTENT { connection_string: 'DELL:bbb', hostname: 'DELL', status: 'resolved', summary: 'Replaced charger', computer_id: computer:dell, service_order: service_order:so2, diagnosed_at: time::now(), started_at: time::now(), tags: [] };
    CREATE diagnostic_session:d3 CONTENT { connection_string: 'PC2:ccc', hostname: 'PC2', status: 'open', summary: 'still looking', computer_id: computer:pc2, started_at: time::now(), tags: [] };
";

fn overwrite(schema: &str) -> String {
    ["TABLE", "FIELD", "INDEX", "EVENT"].iter().fold(schema.to_string(), |s, kind| {
        s.replace(&format!("DEFINE {kind} "), &format!("DEFINE {kind} OVERWRITE "))
            .replace(&format!("DEFINE {kind} OVERWRITE OVERWRITE "), &format!("DEFINE {kind} OVERWRITE "))
    })
}

async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for schema in [COMPUTER, SERVICE_ORDER, DIAGNOSTIC_SESSION] {
        db.query(overwrite(schema)).await.expect("apply schema").check().expect("schema statements");
    }
    db.query(SEED).await.expect("seed").check().expect("seed statements");
    db
}

#[tokio::test]
async fn similar_cases_projection_traverses_record_links() {
    let db = mem_db().await;
    let rows: Vec<serde_json::Value> = db
        .query(SIMILAR_CASES_SQL)
        .bind(("limit", 100i64))
        .await
        .expect("query")
        .take(0)
        .expect("rows");

    // Only resolved/escalated sessions with a summary and diagnosed_at (d1, d2), not the open d3.
    assert_eq!(rows.len(), 2, "resolved sessions only");

    let pc1 = rows
        .iter()
        .find(|r| computer_key_from_value(r, "computer") == "pc1")
        .expect("pc1 case present");

    // Fields pulled through the computer_id link.
    assert_eq!(model_from_value(pc1), ("hp victus 15".to_string(), "HP Victus 15".to_string()));
    // Field pulled through the service_order link.
    assert_eq!(str_field(pc1, "service_number"), "2155001");
    assert_eq!(str_field(pc1, "hostname"), "PC1");
    assert!(str_field(pc1, "checkin_notes").contains("gaming"));
    assert!(pc1.get("diagnosed_at").and_then(|v| v.as_str()).is_some());

    // pc1 and pc2 share a normalized model key despite vendor/name casing.
    let dell = rows
        .iter()
        .find(|r| computer_key_from_value(r, "computer") == "dell")
        .expect("dell case present");
    assert_ne!(model_from_value(pc1).0, model_from_value(dell).0);
    assert_eq!(model_from_value(dell).0, "dell xps 15");
}
