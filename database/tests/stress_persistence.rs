//! Integration tests for stress-test SurrealQL persistence queries.
//! Runs against in-memory SurrealDB so syntax/schema issues surface without
//! deploying to a remote client.

use database::schema::{
    stress_test_sql, HardwareComponent, HardwareKind, RecordId, StressTestRun, TargetKind,
    TestTool, StressKitStressor, COMPUTER_TABLE,
};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;
use surrealdb::types::SurrealValue;

async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    db.query(include_str!("fixtures/stress_schema.surql"))
        .await
        .expect("apply stress schema");
    db
}

#[tokio::test]
async fn hardware_component_upsert_sets_embedding_and_exists() {
    let db = mem_db().await;
    let component = HardwareComponent::new(
        HardwareKind::Cpu,
        "AMD",
        "AMD Ryzen 5 5600G with Radeon Graphics",
    );

    let mut response = db
        .query(stress_test_sql::HW_COMPONENT_UPSERT)
        .bind(("id", component.id.clone()))
        .bind(("kind", component.kind.as_str().to_string()))
        .bind(("vendor", component.vendor.clone()))
        .bind(("model", component.model.clone()))
        .bind(("sku", component.sku.clone()))
        .bind(("display", component.display_name.clone()))
        .bind(("specs", component.specs.clone()))
        .bind(("embed_src", component.embed_source()))
        .await
        .expect("upsert query");

    let ids: Vec<RecordId> = response.take(0).expect("upsert ids");
    assert_eq!(ids.len(), 1);

    let mut exists_resp = db
        .query(stress_test_sql::RECORD_EXISTS)
        .bind(("id", component.id.clone()))
        .await
        .expect("record::exists");
    let exists: Option<bool> = exists_resp.take(0).expect("exists bool");
    assert_eq!(exists, Some(true));

    let mut embed_resp = db
        .query("SELECT VALUE array::len(embedding) FROM $id")
        .bind(("id", component.id.clone()))
        .await
        .expect("embedding len");
    let len: Option<i64> = embed_resp.take(0).expect("embedding length");
    assert_eq!(len, Some(768));
}

#[tokio::test]
async fn stress_test_run_create_merges_content_and_embedding() {
    let db = mem_db().await;
    let computer = RecordId::new(COMPUTER_TABLE, "DESKTOP-TEST:abc123");
    db.query("CREATE $id CONTENT { hostname: $host }")
        .bind(("id", computer.clone()))
        .bind(("host", "DESKTOP-TEST"))
        .await
        .expect("seed computer");

    let mut run = StressTestRun::new_for(
        computer.clone(),
        TestTool::StressKit {
            stressor: StressKitStressor::Cpu,
        },
        TargetKind::Cpu,
    );
    run.preset_label = Some("scripts:single:cpu".into());
    run.hostname = Some("DESKTOP-TEST".into());

    let mut content = run.clone().into_value();
    if let surrealdb::types::Value::Object(obj) = &mut content {
        obj.remove("embedding");
        obj.remove("id");
        obj.insert(
            "failure_mode".to_string(),
            surrealdb::types::Value::Object(
                [(
                    "None".to_string(),
                    surrealdb::types::Value::Object(Default::default()),
                )]
                .into_iter()
                .collect(),
            ),
        );
    }

    let embed_src = run.embed_source();
    let mut response = db
        .query(stress_test_sql::STRESS_RUN_CREATE)
        .bind(("id", run.id.clone()))
        .bind(("content", content))
        .bind(("embed_src", embed_src))
        .await
        .expect("create run");

    let created: Vec<RecordId> = response.take(0).expect("created id");
    assert_eq!(created.len(), 1);

    let mut exists_resp = db
        .query(stress_test_sql::RECORD_EXISTS)
        .bind(("id", run.id.clone()))
        .await
        .expect("record::exists");
    let exists: Option<bool> = exists_resp.take(0).expect("exists");
    assert_eq!(exists, Some(true));

    let mut row_resp = db
        .query("SELECT tool_label, result, array::len(embedding) AS embed_len FROM $id")
        .bind(("id", run.id.clone()))
        .await
        .expect("select run");
    #[derive(serde::Deserialize, SurrealValue)]
    struct RunRow {
        tool_label: String,
        result: String,
        embed_len: i64,
    }
    let rows: Vec<RunRow> = row_resp.take(0).expect("run row");
    assert_eq!(rows[0].tool_label, "stresskit:cpu");
    assert_eq!(rows[0].result, "in_progress");
    assert_eq!(rows[0].embed_len, 768);
}

#[tokio::test]
async fn object_merge_is_not_valid_surrealql() {
    let db = mem_db().await;

    let err = db
        .query("CREATE stress_test_run:bad CONTENT object::merge({ a: 1 }, { b: 2 })")
        .await
        .expect_err("object::merge should not parse");

    let msg = err.to_string();
    assert!(
        msg.contains("object::merge") || msg.contains("Invalid function"),
        "unexpected error: {msg}"
    );
}

#[test]
fn stress_query_fixtures_validate_with_surreal_cli() {
    let schema = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stress_schema.surql"
    );
    let queries = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stress_queries.surql"
    );

    for path in [schema, queries] {
        let output = std::process::Command::new("surreal")
            .args(["validate", path])
            .output()
            .unwrap_or_else(|e| panic!("failed to run `surreal validate` for {path}: {e}"));
        assert!(
            output.status.success(),
            "surreal validate failed for {path}:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
