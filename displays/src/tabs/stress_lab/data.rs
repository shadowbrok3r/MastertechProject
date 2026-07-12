//! SurrealDB loaders for the Stress Lab tab.

use database::schema::{
    HardwareComponent, HardwareKind, HardwareTestBaseline, RecordId, RecordIdExt,
    StressTestEvent, StressTestMetric, StressTestRun,
};
use database::db;

use super::{ComponentRow, RunRow};

pub async fn fetch_components(kind_filter: Option<HardwareKind>) -> anyhow::Result<Vec<ComponentRow>> {
    let rows: Vec<HardwareComponent> = if let Some(kind) = kind_filter {
        HardwareComponent::list_by_kind(kind).await?
    } else {
        let mut response = db()
            .query(
                "SELECT * FROM hardware_component ORDER BY last_seen DESC LIMIT 300",
            )
            .await?;
        response.take(0)?
    };

    let mut out = Vec::with_capacity(rows.len());
    for c in rows {
        let baselines = HardwareTestBaseline::for_component(&c.id).await.unwrap_or_default();
        let run_count: u64 = baselines.iter().map(|b| b.run_count).sum();
        let fail_count: u64 = baselines.iter().map(|b| b.fail_count).sum();
        out.push(ComponentRow {
            id: c.id.key_string(),
            display_name: c.display_name,
            kind: c.kind.as_str().to_string(),
            run_count,
            fail_count,
        });
    }
    out.sort_by(|a, b| b.run_count.cmp(&a.run_count));
    Ok(out)
}

pub async fn fetch_runs_for_component(component_id: &str) -> anyhow::Result<Vec<RunRow>> {
    let cid = parse_record_id(component_id, database::schema::HARDWARE_COMPONENT_TABLE);
    let runs = StressTestRun::list_for_component(&cid).await?;
    Ok(runs.into_iter().map(run_to_row).collect())
}

pub async fn fetch_recent_runs(limit: u64) -> anyhow::Result<Vec<RunRow>> {
    // Cast `duration_actual_secs` to float at read time so rows written
    // before the write-side `<float>` cast (where `duration::secs` could
    // land as an integer) deserialize into the Rust `Option<f64>` field
    // without "Expected float, got number" errors.
    let mut response = db()
        .query(
            "SELECT *, <float> duration_actual_secs AS duration_actual_secs \
             FROM stress_test_run ORDER BY started_at DESC LIMIT $limit",
        )
        .bind(("limit", limit))
        .await?;
    let runs: Vec<StressTestRun> = response.take(0)?;
    Ok(runs.into_iter().map(run_to_row).collect())
}

pub async fn fetch_metrics(run_id: &str) -> anyhow::Result<Vec<StressTestMetric>> {
    let rid = parse_record_id(run_id, database::schema::STRESS_TEST_RUN_TABLE);
    StressTestMetric::list_for_run(&rid, None, None).await
}

pub async fn fetch_events(run_id: &str) -> anyhow::Result<Vec<StressTestEvent>> {
    let rid = parse_record_id(run_id, database::schema::STRESS_TEST_RUN_TABLE);
    StressTestEvent::list_for_run(&rid).await
}

fn run_to_row(run: StressTestRun) -> RunRow {
    RunRow {
        id: run.id.key_string(),
        tool_label: run.tool_label,
        result: run.result.as_str().to_string(),
        failure_kind: run.failure_kind,
        hostname: run.hostname,
        started_at: format_datetime(&run.started_at),
        duration_secs: run.duration_actual_secs,
        peak_throughput: run.summary.peak_throughput,
        throughput_unit: run.summary.throughput_unit.clone(),
        max_temp_c: run.summary.max_temp_c,
        target_component: run
            .target_component
            .as_ref()
            .map(|r| r.key_string()),
        preset_label: run.preset_label,
    }
}

fn format_datetime(dt: &database::schema::Datetime) -> String {
    chrono::DateTime::<chrono::Utc>::from(*dt)
        .format("%Y-%m-%d %H:%M UTC")
        .to_string()
}

fn parse_record_id(key: &str, table: &'static str) -> RecordId {
    database::schema::entity_link::parse_record_id(key, table)
}
