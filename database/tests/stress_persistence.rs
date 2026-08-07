//! Integration tests for stress-test SurrealQL persistence queries.
//! Runs against in-memory SurrealDB so syntax/schema issues surface without
//! deploying to a remote client.

use database::schema::{
    stress_test_sql, CoreSampleRow, HardwareComponent, HardwareKind, RecordId, StressTestMetric,
    StressTestRun, TargetKind, TestTool, COMPUTER_TABLE,
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
        .bind(("embedding", Some(vec![0.1f32; 768])))
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
            stressor: "cpu".to_string(),
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

    let mut response = db
        .query(stress_test_sql::STRESS_RUN_CREATE)
        .bind(("id", run.id.clone()))
        .bind(("content", content))
        .bind(("embedding", Some(vec![0.1f32; 768])))
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

/// CREATE a run row the way `StressTestRun::create` does, minus the embedding
/// call. Returns once the row is confirmed readable.
async fn seed_run(db: &Surreal<Db>, computer: &RecordId) -> RecordId {
    let run = StressTestRun::new_for(
        computer.clone(),
        TestTool::StressKit {
            stressor: "gpu_display".to_string(),
        },
        TargetKind::Gpu,
    );
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
    db.query(stress_test_sql::STRESS_RUN_CREATE)
        .bind(("id", run.id.clone()))
        .bind(("content", content))
        .bind(("embedding", None::<Vec<f32>>))
        .await
        .expect("create run");
    run.id
}

fn sample_metric(run_ref: &RecordId, tick: i64) -> StressTestMetric {
    let captured = chrono::Utc::now() + chrono::Duration::milliseconds(tick);
    let mut metric = StressTestMetric::new(run_ref.clone(), captured.into());
    metric.cores = vec![CoreSampleRow {
        index: 0,
        brand: "AMD".into(),
        usage_pct: 99.0,
        freq_mhz: 4200,
        temp_c: Some(78.0),
    }];
    metric.memory_used_mb = Some(8192);
    metric.memory_used_pct = Some(51.2);
    metric
}

/// The v22 abort signature: metric rows issued on the first tick, right after
/// the run row was created, were rejected as orphans mid-run.
#[tokio::test]
async fn metrics_written_immediately_after_run_creation_land() {
    let db = mem_db().await;
    let computer = RecordId::new(COMPUTER_TABLE, "AK23-desk:6a71cbe65");
    db.query("CREATE $id CONTENT { hostname: $host }")
        .bind(("id", computer.clone()))
        .bind(("host", "AK23-desk"))
        .await
        .expect("seed computer");

    let run_id = seed_run(&db, &computer).await;

    // No settle delay: the parent must be visible to the very next write.
    let mut exists_resp = db
        .query(stress_test_sql::RECORD_EXISTS)
        .bind(("id", run_id.clone()))
        .await
        .expect("record::exists");
    let exists: Option<bool> = exists_resp.take(0).expect("exists");
    assert_eq!(exists, Some(true), "run row not visible to the next write");

    for tick in 0..20 {
        let metric = sample_metric(&run_id, tick);
        metric.validate_shape().expect("sample passes shape checks");
        let mut content = metric.clone().into_value();
        if let surrealdb::types::Value::Object(obj) = &mut content {
            obj.remove("id");
        }
        db.query("CREATE $id CONTENT $content")
            .bind(("id", metric.id.clone()))
            .bind(("content", content))
            .await
            .unwrap_or_else(|e| panic!("metric {tick} rejected: {e}"));
    }

    let mut count_resp = db
        .query("SELECT VALUE count() FROM stress_test_metric WHERE run_ref = $r GROUP ALL")
        .bind(("r", run_id.clone()))
        .await
        .expect("count metrics");
    let counts: Vec<i64> = count_resp.take(0).expect("count rows");
    assert_eq!(counts.first().copied(), Some(20));
}

/// `record::exists` must answer `Some(false)` for an absent row. If it answered
/// `None`, an orphan link would classify as `Unknown` and slip past the guard.
#[tokio::test]
async fn record_exists_answers_false_for_a_missing_row() {
    let db = mem_db().await;
    let mut resp = db
        .query(stress_test_sql::RECORD_EXISTS)
        .bind(("id", RecordId::new("stress_test_run", "nope-not-here")))
        .await
        .expect("record::exists");
    let exists: Option<bool> = resp.take(0).expect("exists");
    assert_eq!(exists, Some(false));
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

#[tokio::test]
async fn run_summary_missing_test_errors_defaults_to_zero() {
    // Rows older than `summary.test_errors` must still deserialize, with the
    // field defaulting instead of failing the whole row.
    use database::schema::RunSummary;
    let db = mem_db().await;
    let mut resp = db
        .query(
            "RETURN { thermal_throttle_detected: false, vrm_throttle_detected: false, \
             whea_delta_count: 2, tdr_count: 0, bsod_detected: false, \
             disk_io_errors: 1, memory_errors: 0, max_temp_c: 91.5f }",
        )
        .await
        .expect("old-shape summary object");
    let summary: Option<RunSummary> = resp.take(0).expect("deserialize old-shape summary");
    let summary = summary.expect("summary present");
    assert_eq!(summary.test_errors, 0);
    assert_eq!(summary.whea_delta_count, 2);
    assert_eq!(summary.disk_io_errors, 1);
    assert_eq!(summary.max_temp_c, Some(91.5));
}

#[tokio::test]
async fn benchmark_result_round_trips_through_surreal() {
    use database::schema::{BenchmarkKind, BenchmarkResult};
    let db = mem_db().await;
    let computer = RecordId::new(COMPUTER_TABLE, "DESKTOP-TEST:bench");
    db.query("CREATE $id CONTENT { hostname: $host }")
        .bind(("id", computer.clone()))
        .bind(("host", "DESKTOP-TEST"))
        .await
        .expect("seed computer");

    let mut row = BenchmarkResult::new(computer, BenchmarkKind::CpuMulti, 4321.5, "Mflop/s");
    row.samples = 12;
    row.threads = 16;
    row.duration_secs = 15.2;
    row.peak = Some(4500.0);
    row.errors = 0;
    row.detail = Some(serde_json::json!([{ "size_kb": 64, "latency_ns": 1.2 }]));

    let mut content = row.clone().into_value();
    if let surrealdb::types::Value::Object(obj) = &mut content {
        obj.remove("id");
    }
    db.query("CREATE $id CONTENT $content")
        .bind(("id", row.id.clone()))
        .bind(("content", content))
        .await
        .expect("create benchmark_result");

    let mut resp = db
        .query("SELECT * FROM $id")
        .bind(("id", row.id.clone()))
        .await
        .expect("select benchmark_result");
    let rows: Vec<BenchmarkResult> = resp.take(0).expect("round-trip decode");
    assert_eq!(rows.len(), 1);
    let got = &rows[0];
    assert_eq!(got.kind, BenchmarkKind::CpuMulti);
    assert_eq!(got.kind_label, "cpu_multi");
    assert_eq!(got.score, 4321.5);
    assert_eq!(got.unit, "Mflop/s");
    assert_eq!(got.peak, Some(4500.0));
    assert_eq!(got.threads, 16);
    assert!(got.detail.is_some());
}
