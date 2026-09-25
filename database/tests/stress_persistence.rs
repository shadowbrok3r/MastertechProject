//! Integration tests for stress-test SurrealQL persistence queries.
//! Runs against in-memory SurrealDB so syntax/schema issues surface without
//! deploying to a remote client.

use database::schema::{
    COMPUTER_TABLE, CoreSampleRow, Datetime, EventKind, HardwareComponent, HardwareKind,
    ORPHAN_GRACE, OrphanReason, ReapScope, RecordId, RecordIdExt, STRESS_TEST_RUN_TABLE,
    StressTestEvent, StressTestMetric, StressTestRun, TargetKind, TestTool, reap_orphaned_on,
    stress_test_sql,
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
    let backfill = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/queries/stress_orphan_backfill.surql"
    );

    for path in [schema, queries, backfill] {
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

const ORPHAN_BACKFILL: &str = include_str!("../queries/stress_orphan_backfill.surql");

fn hours(n: i64) -> chrono::Duration {
    chrono::Duration::hours(n)
}

fn minutes(n: i64) -> chrono::Duration {
    chrono::Duration::minutes(n)
}

async fn seed_computer(db: &Surreal<Db>, key: &str) -> RecordId {
    let computer = RecordId::new(COMPUTER_TABLE, key);
    db.query("CREATE $id CONTENT { hostname: $host }")
        .bind(("id", computer.clone()))
        .bind(("host", key.split(':').next().unwrap_or(key).to_string()))
        .await
        .and_then(|r| r.check())
        .expect("seed computer");
    computer
}

/// CREATE an `in_progress` run with a fixed key, start and plan.
async fn seed_timed_run(
    db: &Surreal<Db>,
    key: &str,
    computer: &RecordId,
    started_at: chrono::DateTime<chrono::Utc>,
    planned_secs: Option<u64>,
) -> RecordId {
    let mut run = StressTestRun::new_for(
        computer.clone(),
        TestTool::StressKit {
            stressor: "cpu".to_string(),
        },
        TargetKind::Cpu,
    );
    run.id = RecordId::new(STRESS_TEST_RUN_TABLE, key);
    run.started_at = started_at.into();
    run.duration_planned_secs = planned_secs;
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
        .and_then(|r| r.check())
        .expect("create run");
    run.id
}

async fn seed_metric_at(db: &Surreal<Db>, run: &RecordId, at: chrono::DateTime<chrono::Utc>) {
    let metric = StressTestMetric::new(run.clone(), at.into());
    let mut content = metric.clone().into_value();
    if let surrealdb::types::Value::Object(obj) = &mut content {
        obj.remove("id");
    }
    db.query("CREATE $id CONTENT $content")
        .bind(("id", metric.id.clone()))
        .bind(("content", content))
        .await
        .and_then(|r| r.check())
        .expect("create metric");
}

async fn seed_event_at(db: &Surreal<Db>, run: &RecordId, at: chrono::DateTime<chrono::Utc>) {
    let mut event = StressTestEvent::new(run.clone(), EventKind::StageStarted, "stress-kit");
    event.at = at.into();
    let mut content = event.clone().into_value();
    if let surrealdb::types::Value::Object(obj) = &mut content {
        obj.remove("id");
    }
    db.query("CREATE $id CONTENT $content")
        .bind(("id", event.id.clone()))
        .bind(("content", content))
        .await
        .and_then(|r| r.check())
        .expect("create event");
}

async fn set_fields(db: &Surreal<Db>, run: &RecordId, set: &str) {
    db.query(format!("UPDATE $id SET {set}"))
        .bind(("id", run.clone()))
        .await
        .and_then(|r| r.check())
        .expect("update run");
}

#[derive(Debug, PartialEq, SurrealValue)]
struct RunState {
    id: String,
    result: String,
    finish_reason: Option<String>,
    ended_at: Option<Datetime>,
    duration_actual_secs: Option<f64>,
    notes: Option<String>,
}

const RUN_STATE_FIELDS: &str =
    "<string> id AS id, result, finish_reason, ended_at, duration_actual_secs, notes";

async fn run_state(db: &Surreal<Db>, run: &RecordId) -> RunState {
    db.query(format!("SELECT {RUN_STATE_FIELDS} FROM ONLY $id"))
        .bind(("id", run.clone()))
        .await
        .expect("select run")
        .take::<Option<RunState>>(0)
        .expect("decode run")
        .expect("run present")
}

async fn all_run_states(db: &Surreal<Db>) -> Vec<RunState> {
    db.query(format!(
        "SELECT {RUN_STATE_FIELDS} FROM stress_test_run ORDER BY id"
    ))
    .await
    .expect("select runs")
    .take(0)
    .expect("decode runs")
}

/// One run per window-rule case, all on one computer.
struct FleetCases {
    orphan: RecordId,
    open_ended: RecordId,
    long_live: RecordId,
    overrun: RecordId,
    within_grace: RecordId,
    finished: RecordId,
}

async fn seed_fleet_cases(db: &Surreal<Db>, now: chrono::DateTime<chrono::Utc>) -> FleetCases {
    let computer = seed_computer(db, "DESKTOP-REAP:0a1b2c3d4").await;

    let orphan = seed_timed_run(db, "orphan", &computer, now - hours(10), Some(3600)).await;
    seed_event_at(db, &orphan, now - hours(10) + minutes(1)).await;
    seed_metric_at(db, &orphan, now - hours(9) - minutes(30)).await;
    set_fields(db, &orphan, "notes = 'tech note'").await;

    let open_ended = seed_timed_run(db, "open_ended", &computer, now - hours(5), None).await;
    seed_metric_at(db, &open_ended, now - hours(4)).await;

    let long_live =
        seed_timed_run(db, "long_live", &computer, now - hours(3), Some(12 * 3600)).await;
    seed_metric_at(db, &long_live, now - minutes(1)).await;

    let overrun = seed_timed_run(db, "overrun", &computer, now - hours(5), Some(3600)).await;
    seed_metric_at(db, &overrun, now - minutes(1)).await;

    // Planned end 90 minutes ago: overdue at a 1h grace, not at 2h.
    let within_grace = seed_timed_run(
        db,
        "within_grace",
        &computer,
        now - hours(3) - minutes(30),
        Some(7200),
    )
    .await;

    let finished = seed_timed_run(db, "finished", &computer, now - hours(10), Some(3600)).await;
    set_fields(
        db,
        &finished,
        "result = 'pass', finish_reason = 'completed'",
    )
    .await;

    FleetCases {
        orphan,
        open_ended,
        long_live,
        overrun,
        within_grace,
        finished,
    }
}

#[tokio::test]
async fn fleet_reap_closes_silent_overdue_runs_and_spares_live_ones() {
    let db = mem_db().await;
    let now = chrono::Utc::now();
    let cases = seed_fleet_cases(&db, now).await;

    let closed = reap_orphaned_on(&db, ReapScope::Fleet, ORPHAN_GRACE, now)
        .await
        .expect("reap");
    let mut keys: Vec<String> = closed.iter().map(|(id, _)| id.key_string()).collect();
    keys.sort();
    assert_eq!(keys, ["open_ended", "orphan"]);
    assert!(
        closed
            .iter()
            .all(|(_, reason)| *reason == OrphanReason::Overdue)
    );

    let note = OrphanReason::Overdue.note(ORPHAN_GRACE);
    let orphan = run_state(&db, &cases.orphan).await;
    assert_eq!(orphan.result, "aborted");
    assert_eq!(orphan.finish_reason.as_deref(), Some("crashed"));
    assert_eq!(orphan.ended_at, Some((now - hours(9) - minutes(30)).into()));
    assert_eq!(orphan.duration_actual_secs, Some(1800.0));
    assert_eq!(orphan.notes, Some(format!("tech note {note}")));

    let open_ended = run_state(&db, &cases.open_ended).await;
    assert_eq!(open_ended.result, "aborted");
    assert_eq!(open_ended.ended_at, Some((now - hours(4)).into()));
    assert_eq!(open_ended.duration_actual_secs, Some(3600.0));
    assert_eq!(open_ended.notes, Some(note));

    for live in [&cases.long_live, &cases.overrun, &cases.within_grace] {
        let state = run_state(&db, live).await;
        assert_eq!(state.result, "in_progress", "{} was closed", state.id);
        assert_eq!(state.finish_reason, None);
        assert_eq!(state.ended_at, None);
    }
    let finished = run_state(&db, &cases.finished).await;
    assert_eq!(finished.result, "pass");
    assert_eq!(finished.finish_reason.as_deref(), Some("completed"));
}

#[tokio::test]
async fn machine_reap_closes_this_computers_runs_silent_since_before_boot() {
    let db = mem_db().await;
    let now = chrono::Utc::now();
    let this = seed_computer(&db, "DESKTOP-JFAT75B:4198373a9").await;
    let other = seed_computer(&db, "DESKTOP-EOA4FR0:3a1e473a3").await;

    let before_boot =
        seed_timed_run(&db, "before_boot", &this, now - minutes(30), Some(28_800)).await;
    seed_metric_at(&db, &before_boot, now - minutes(20)).await;
    let after_boot = seed_timed_run(&db, "after_boot", &this, now - minutes(5), Some(5_400)).await;
    seed_metric_at(&db, &after_boot, now - minutes(1)).await;
    let elsewhere = seed_timed_run(&db, "elsewhere", &other, now - minutes(30), Some(28_800)).await;
    seed_metric_at(&db, &elsewhere, now - minutes(20)).await;

    let scope = ReapScope::Machine {
        computer: &this,
        booted_at: Some(now - minutes(10)),
    };
    let closed = reap_orphaned_on(&db, scope, ORPHAN_GRACE, now)
        .await
        .expect("reap");
    assert_eq!(closed, vec![(before_boot.clone(), OrphanReason::Rebooted)]);

    let state = run_state(&db, &before_boot).await;
    assert_eq!(state.result, "aborted");
    assert_eq!(state.finish_reason.as_deref(), Some("crashed"));
    assert_eq!(state.ended_at, Some((now - minutes(20)).into()));
    assert_eq!(state.duration_actual_secs, Some(600.0));
    assert_eq!(state.notes, Some(OrphanReason::Rebooted.note(ORPHAN_GRACE)));
    assert_eq!(run_state(&db, &after_boot).await.result, "in_progress");
    assert_eq!(run_state(&db, &elsewhere).await.result, "in_progress");

    let fleet = reap_orphaned_on(&db, ReapScope::Fleet, ORPHAN_GRACE, now)
        .await
        .expect("fleet reap");
    assert!(fleet.is_empty(), "window rule closed {fleet:?}");
}

#[tokio::test]
async fn orphan_close_leaves_a_run_that_finished_meanwhile() {
    let db = mem_db().await;
    let now = chrono::Utc::now();
    let computer = seed_computer(&db, "DESKTOP-REAP:5e6f7a8b9").await;
    let run = seed_timed_run(&db, "raced", &computer, now - hours(10), Some(3600)).await;
    set_fields(&db, &run, "result = 'pass', finish_reason = 'completed'").await;

    let closed: Vec<RecordId> = db
        .query(stress_test_sql::ORPHAN_CLOSE)
        .bind(("id", run.clone()))
        .bind(("ended_at", Datetime::from(now - hours(9))))
        .bind(("note", OrphanReason::Overdue.note(ORPHAN_GRACE)))
        .await
        .expect("close query")
        .take(0)
        .expect("closed ids");
    assert!(closed.is_empty());

    let state = run_state(&db, &run).await;
    assert_eq!(state.result, "pass");
    assert_eq!(state.finish_reason.as_deref(), Some("completed"));
    assert_eq!(state.ended_at, None);
    assert_eq!(state.notes, None);
}

#[derive(Debug, SurrealValue)]
struct BackfillPreviewRow {
    id: RecordId,
    last_seen: Datetime,
}

#[tokio::test]
async fn orphan_backfill_script_matches_the_fleet_reaper() {
    let now = chrono::Utc::now();
    let scripted = mem_db().await;
    let reaped = mem_db().await;
    let cases = seed_fleet_cases(&scripted, now).await;
    seed_fleet_cases(&reaped, now).await;

    let mut response = scripted
        .query(ORPHAN_BACKFILL)
        .await
        .and_then(|r| r.check())
        .expect("backfill script");
    let mut preview: Vec<BackfillPreviewRow> = response.take(2).expect("preview rows");
    preview.sort_by_key(|row| row.id.key_string());
    let preview_ids: Vec<&RecordId> = preview.iter().map(|row| &row.id).collect();
    assert_eq!(preview_ids, [&cases.open_ended, &cases.orphan]);
    assert_eq!(preview[1].last_seen, (now - hours(9) - minutes(30)).into());

    reap_orphaned_on(&reaped, ReapScope::Fleet, ORPHAN_GRACE, now)
        .await
        .expect("reap");
    assert_eq!(
        all_run_states(&scripted).await,
        all_run_states(&reaped).await
    );

    let rerun: Vec<BackfillPreviewRow> = scripted
        .query(ORPHAN_BACKFILL)
        .await
        .and_then(|r| r.check())
        .expect("second backfill run")
        .take(2)
        .expect("second preview");
    assert!(rerun.is_empty(), "a second run found {rerun:?}");
}
