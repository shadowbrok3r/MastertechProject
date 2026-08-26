//! SurrealDB loaders for the Stress Lab tab.

use database::db;
use database::schema::{
    Datetime, HardwareKind, RecordId, RunResult, StressTestEvent, TargetKind,
};
use database::SurrealValue;
use serde::{Deserialize, Serialize};

/// Catalog row for one part. Projected so the 768-float `embedding` never
/// crosses the wire.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct ComponentInfo {
    pub id: RecordId,
    pub kind: HardwareKind,
    pub vendor: String,
    pub model: String,
    pub display_name: String,
    pub occurrence_count: u64,
    pub last_seen: Datetime,
}

/// The slice of `stress_test_run` the lab charts and lists actually read.
/// `SELECT *` would drag every run's embedding along with it.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct RunRecord {
    pub id: RecordId,
    pub target_component: Option<RecordId>,
    pub touched_components: Vec<RecordId>,
    pub target_kind: TargetKind,
    pub tool_label: String,
    pub preset_label: Option<String>,
    pub result: RunResult,
    pub failure_kind: String,
    pub hostname: Option<String>,
    pub started_at: Datetime,
    pub ended_at: Option<Datetime>,
    pub duration_actual_secs: Option<f64>,
    /// Read untyped on purpose. `summary` is `TYPE object` in the schema, so
    /// SurrealDB coerces nothing on write and the stored number kind follows
    /// whatever the writer happened to send. A typed read rejects an integer
    /// where the field says float, and one bad row fails the entire batch — the
    /// same trap that emptied `hardware_test_baseline`.
    pub summary: Option<serde_json::Value>,
}

impl RunRecord {
    /// Whether this run exercised `component`, as its primary target or as one
    /// of the parts a mixed-stage run touched.
    pub fn involves(&self, component: &RecordId) -> bool {
        self.target_component.as_ref() == Some(component)
            || self.touched_components.iter().any(|c| c == component)
    }

    /// How long the run covered, from its recorded duration or, when that is
    /// missing, from the start/end stamps. 30 of the runs in the corpus record
    /// no duration.
    pub fn span_secs(&self) -> Option<f64> {
        self.duration_actual_secs.or_else(|| {
            let end = self.ended_at?;
            let secs = end.timestamp() - self.started_at.timestamp();
            (secs > 0).then_some(secs as f64)
        })
    }

    /// A numeric `summary` field, accepting either number kind.
    pub fn summary_num(&self, key: &str) -> Option<f64> {
        self.summary.as_ref()?.get(key)?.as_f64()
    }

    pub fn summary_str(&self, key: &str) -> Option<&str> {
        self.summary.as_ref()?.get(key)?.as_str()
    }

    pub fn throughput_unit(&self) -> Option<&str> {
        self.summary_str("throughput_unit")
    }

    pub fn peak_throughput(&self) -> Option<f64> {
        self.summary_num("peak_throughput")
    }

    /// The run's peak temperature, whichever field carries it. `max_temp_c` is
    /// written by only a handful of runs; the CPU and GPU peaks cover the rest.
    pub fn peak_temp_c(&self) -> Option<f64> {
        self.summary_num("max_temp_c")
            .or_else(|| self.summary_num("max_cpu_temp_c"))
            .or_else(|| self.summary_num("max_gpu_temp_c"))
    }
}

pub async fn fetch_components() -> anyhow::Result<Vec<ComponentInfo>> {
    let rows: Vec<ComponentInfo> = db()
        .query(
            "SELECT id, kind, vendor, model, display_name, occurrence_count, last_seen \
             FROM hardware_component ORDER BY last_seen DESC LIMIT 1000",
        )
        .await?
        .take(0)?;
    Ok(rows)
}

/// Every run, newest first. The whole table is ~500 rows, so the lab loads it
/// once and answers filtering, sorting and cross-component comparison locally
/// instead of re-querying per selection.
pub async fn fetch_runs(limit: u64) -> anyhow::Result<Vec<RunRecord>> {
    // `duration_actual_secs` is cast at read time: rows written before the
    // write-side `<float>` cast stored `duration::secs` as an integer, which
    // fails to deserialize into `Option<f64>`.
    let rows: Vec<RunRecord> = db()
        .query(
            "SELECT id, target_component, touched_components, target_kind, tool_label, \
                    preset_label, result, failure_kind, hostname, started_at, ended_at, summary, \
                    (IF duration_actual_secs != NONE THEN <float> duration_actual_secs END) \
                        AS duration_actual_secs \
             FROM stress_test_run ORDER BY started_at DESC LIMIT $limit",
        )
        .bind(("limit", limit))
        .await?
        .take(0)?;
    Ok(rows)
}

pub async fn fetch_events(run_id: &RecordId) -> anyhow::Result<Vec<StressTestEvent>> {
    StressTestEvent::list_for_run(run_id).await
}

/// One time bucket of one run's telemetry.
///
/// Averaged fields arrive as SUMS paired with the `*_n` count of ticks that
/// carried a value; the division happens on the client. `math::mean(x ?? 0)`
/// divides by every tick in the bucket, so a sensor read on half of them would
/// report half its true average. `*_n` also separates "never sampled" from
/// "measured zero", which is what keeps an absent series off the chart instead
/// of drawing it as a flat line at zero.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct SeriesBucket {
    pub run: RecordId,
    /// `unix_seconds / bucket_secs`. Absolute, so the query needs no per-row
    /// lookup of the run's `started_at`; elapsed time is worked out on the
    /// client, where every run's start is already known.
    pub bucket: i64,
    pub throughput: f64,
    pub throughput_n: u32,
    pub max_cpu_temp_c: f64,
    pub cpu_temp_n: u32,
    pub max_gpu_temp_c: f64,
    pub gpu_temp_n: u32,
    pub clock_mhz: f64,
    pub clock_n: u32,
    pub gpu_clock_mhz: f64,
    pub gpu_clock_n: u32,
    pub power_w: f64,
    pub power_n: u32,
    pub gpu_power_w: f64,
    pub gpu_power_n: u32,
    pub cpu_usage_pct: f64,
    pub cpu_usage_n: u32,
    pub gpu_usage_pct: f64,
    pub gpu_usage_n: u32,
    pub memory_used_pct: f64,
    pub memory_n: u32,
    pub whea_delta: i64,
    pub whea_n: u32,
    pub ticks: u32,
}

/// Projection for [`fetch_series`]. Every aggregate is cast: `math::sum` and
/// `math::max` return an Int whenever the result is integral, and a typed read
/// accepts only the exact number kind the field declares.
const SERIES_PROJECTION: &str = "SELECT run_ref AS run, \
    <int> math::floor(time::unix(captured_at) / $bucket) AS bucket, \
    <float> math::sum(throughput ?? 0) AS throughput, \
    <int> math::sum(IF throughput != NONE THEN 1 ELSE 0 END) AS throughput_n, \
    <float> math::max(cpu_temp_c ?? 0) AS max_cpu_temp_c, \
    <int> math::sum(IF cpu_temp_c != NONE THEN 1 ELSE 0 END) AS cpu_temp_n, \
    <float> math::max(gpu_temp_c ?? 0) AS max_gpu_temp_c, \
    <int> math::sum(IF gpu_temp_c != NONE THEN 1 ELSE 0 END) AS gpu_temp_n, \
    <float> math::sum(clock_mhz ?? 0) AS clock_mhz, \
    <int> math::sum(IF clock_mhz != NONE THEN 1 ELSE 0 END) AS clock_n, \
    <float> math::sum(gpu_clock_mhz ?? 0) AS gpu_clock_mhz, \
    <int> math::sum(IF gpu_clock_mhz != NONE THEN 1 ELSE 0 END) AS gpu_clock_n, \
    <float> math::sum(power_w ?? 0) AS power_w, \
    <int> math::sum(IF power_w != NONE THEN 1 ELSE 0 END) AS power_n, \
    <float> math::sum(gpu_power_w ?? 0) AS gpu_power_w, \
    <int> math::sum(IF gpu_power_w != NONE THEN 1 ELSE 0 END) AS gpu_power_n, \
    <float> math::sum(cpu_usage_pct ?? 0) AS cpu_usage_pct, \
    <int> math::sum(IF cpu_usage_pct != NONE THEN 1 ELSE 0 END) AS cpu_usage_n, \
    <float> math::sum(gpu_usage_pct ?? 0) AS gpu_usage_pct, \
    <int> math::sum(IF gpu_usage_pct != NONE THEN 1 ELSE 0 END) AS gpu_usage_n, \
    <float> math::sum(memory_used_pct ?? 0) AS memory_used_pct, \
    <int> math::sum(IF memory_used_pct != NONE THEN 1 ELSE 0 END) AS memory_n, \
    <int> math::max(whea_delta_count ?? 0) AS whea_delta, \
    <int> math::sum(IF whea_delta_count != NONE THEN 1 ELSE 0 END) AS whea_n, \
    <int> count() AS ticks \
    FROM stress_test_metric WHERE run_ref IN $runs \
    GROUP BY run, bucket ORDER BY bucket ASC";

/// Bucketed telemetry for several runs in one query.
///
/// Raw ticks are not an option: the table holds ~900k rows and a single run can
/// carry 38k of them. Bucketing server-side bounds the result to
/// `runs x duration / bucket_secs`. The id list must be a bound array —
/// `WHERE run_ref IN (SELECT ...)` bypasses the `(run_ref, captured_at)` index
/// and hits the 45 s query cap.
pub async fn fetch_series(
    run_ids: &[RecordId],
    bucket_secs: u32,
) -> anyhow::Result<Vec<SeriesBucket>> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<SeriesBucket> = db()
        .query(SERIES_PROJECTION)
        .bind(("runs", run_ids.to_vec()))
        .bind(("bucket", bucket_secs.max(1)))
        .await?
        .take(0)?;
    Ok(rows)
}
