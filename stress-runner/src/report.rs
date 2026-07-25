//! Run report assembly: one DB fetch + a UI-agnostic view model that egui,
//! terminal mode, and the MCP wire can all render without re-deriving stats.

use database::schema::{
    Datetime, RecordId, RecordIdExt, ScenarioStageSummary, StressTestEvent, StressTestMetric,
    StressTestRun,
};
use database::shape_walk::{self, Row};
use facet::Facet;
use serde::Serialize;

/// Raw rows backing one report.
#[derive(Debug, Clone)]
pub struct RunReportData {
    pub run: StressTestRun,
    pub metrics: Vec<StressTestMetric>,
    pub events: Vec<StressTestEvent>,
}

/// Fetch the run row plus its metric/event series.
pub async fn fetch_report_data(run_id: &RecordId) -> anyhow::Result<RunReportData> {
    let run = StressTestRun::get(run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("stress_test_run {run_id:?} not found"))?;
    let metrics = StressTestMetric::list_for_run(run_id, None, None).await?;
    let events = StressTestEvent::list_for_run(run_id).await?;
    Ok(RunReportData { run, metrics, events })
}

/// One chart series: `(seconds since run start, value)` pairs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReportSeries {
    pub label: String,
    pub unit: String,
    pub points: Vec<(f64, f64)>,
}

/// Stage transition marker on the shared x-axis.
#[derive(Debug, Clone, Serialize)]
pub struct StageBoundary {
    pub at_secs: f64,
    pub index: u32,
    pub label: String,
}

/// Discrete event marker on the shared x-axis.
#[derive(Debug, Clone, Serialize)]
pub struct EventMarker {
    pub at_secs: f64,
    pub kind: String,
    pub detail: String,
}

/// Event timeline row.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineRow {
    pub at_secs: f64,
    pub kind: String,
    pub source: String,
    pub code: Option<String>,
    pub detail: String,
}

/// Everything a renderer needs, pre-digested.
#[derive(Debug, Clone, Serialize, Facet)]
pub struct RunReportModel {
    pub run_id: String,
    pub hostname: Option<String>,
    pub machine_id: Option<String>,
    pub tool_label: String,
    pub preset_label: Option<String>,
    pub tech: Option<String>,
    pub service_order: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_planned_secs: Option<u64>,
    pub duration_actual_secs: Option<f64>,

    /// "pass" / "fail" / "aborted" / "inconclusive" / "in_progress".
    pub result: String,
    pub failure_kind: Option<String>,
    pub failure_detail: Option<String>,

    /// Peak average-core temperature observed during the run.
    #[facet(rename = "CPU max (°C)")]
    pub max_temp_c: Option<f32>,
    /// Mean average-core temperature across the run.
    #[facet(rename = "CPU avg (°C)")]
    pub avg_temp_c: Option<f32>,
    /// Hottest GPU temperature across all cards.
    #[facet(rename = "GPU max (°C)")]
    pub max_gpu_temp_c: Option<f32>,
    pub max_cpu_temp_c: Option<f32>,
    /// Lowest +12V rail reading of the run. Uncalibrated SuperIO value — read as
    /// droop trend, not an absolute voltage.
    #[facet(rename = "12V min (V)")]
    pub min_v12_v: Option<f32>,
    /// Highest average core clock reached.
    #[facet(rename = "Max clock (MHz)")]
    pub max_clock_mhz: Option<u32>,
    pub avg_clock_mhz: Option<u32>,
    pub max_power_w: Option<u32>,
    pub peak_throughput: Option<f64>,
    pub avg_throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    /// WHEA hardware-error count accrued since the run started.
    #[facet(rename = "WHEA")]
    pub whea_delta_count: u32,
    /// GPU TDR (driver timeout/reset) events during the run.
    #[facet(rename = "TDR")]
    pub tdr_count: u32,
    /// Verifying-stressor detected errors (memtest/cpu_verify/linpack/VRAM).
    #[facet(rename = "Test errors")]
    pub test_errors: u32,
    /// Disk I/O errors observed during the run.
    #[facet(rename = "Disk errors")]
    pub disk_io_errors: u32,
    pub thermal_throttle_detected: bool,
    pub vrm_throttle_detected: bool,

    #[facet(opaque)]
    pub stages: Vec<ScenarioStageSummary>,

    #[facet(opaque)]
    pub cpu_temp: ReportSeries,
    #[facet(opaque)]
    pub gpu_temp: ReportSeries,
    #[facet(opaque)]
    pub avg_clock: ReportSeries,
    #[facet(opaque)]
    pub throughput: ReportSeries,
    #[facet(opaque)]
    pub stage_boundaries: Vec<StageBoundary>,
    #[facet(opaque)]
    pub event_markers: Vec<EventMarker>,
    #[facet(opaque)]
    pub timeline: Vec<TimelineRow>,
}

/// Max points kept per chart series after decimation.
const MAX_SERIES_POINTS: usize = 1024;

impl RunReportModel {
    pub fn from_data(data: &RunReportData) -> Self {
        let run = &data.run;
        let start_ms = run.started_at.timestamp_millis();
        let secs_since = |at: &Datetime| (at.timestamp_millis() - start_ms) as f64 / 1000.0;

        let mut cpu_temp = ReportSeries {
            label: "CPU max core".into(),
            unit: "°C".into(),
            points: Vec::new(),
        };
        let mut gpu_temp = ReportSeries {
            label: "GPU".into(),
            unit: "°C".into(),
            points: Vec::new(),
        };
        let mut avg_clock = ReportSeries {
            label: "Avg core clock".into(),
            unit: "MHz".into(),
            points: Vec::new(),
        };
        let mut throughput = ReportSeries {
            label: "Throughput".into(),
            unit: run.summary.throughput_unit.clone().unwrap_or_default(),
            points: Vec::new(),
        };

        let mut stage_boundaries: Vec<StageBoundary> = Vec::new();
        let mut last_stage: Option<u32> = None;

        for m in &data.metrics {
            let x = secs_since(&m.captured_at);

            if m.stage_index != last_stage {
                if let Some(idx) = m.stage_index {
                    stage_boundaries.push(StageBoundary {
                        at_secs: x,
                        index: idx,
                        label: m.stage_label.clone().unwrap_or_default(),
                    });
                }
                last_stage = m.stage_index;
            }

            let tick_max_temp = m.cpu_temp_c.or_else(|| {
                m.cores
                    .iter()
                    .filter_map(|c| c.temp_c)
                    .fold(None::<f32>, |acc, t| Some(acc.map_or(t, |p| p.max(t))))
            });
            if let Some(t) = tick_max_temp {
                cpu_temp.points.push((x, t as f64));
            }
            if let Some(t) = m.gpu_temp_c {
                gpu_temp.points.push((x, t as f64));
            }
            let clocks: Vec<u64> = m.cores.iter().map(|c| c.freq_mhz).filter(|&f| f > 0).collect();
            if !clocks.is_empty() {
                let avg = clocks.iter().sum::<u64>() as f64 / clocks.len() as f64;
                avg_clock.points.push((x, avg));
            }
            if let Some(tp) = m.throughput {
                if tp > 0.0 {
                    throughput.points.push((x, tp));
                }
            }
        }

        for series in [&mut cpu_temp, &mut gpu_temp, &mut avg_clock, &mut throughput] {
            decimate(&mut series.points, MAX_SERIES_POINTS);
        }

        let mut event_markers = Vec::new();
        let mut timeline = Vec::new();
        for e in &data.events {
            let x = secs_since(&e.at);
            let kind = e.kind.as_str().to_string();
            timeline.push(TimelineRow {
                at_secs: x,
                kind: kind.clone(),
                source: e.source.clone(),
                code: e.code.clone(),
                detail: e.detail.clone(),
            });
            let marks = matches!(
                kind.as_str(),
                "whea_hit"
                    | "tdr"
                    | "memory_error"
                    | "disk_io_error"
                    | "thermal_throttle"
                    | "vrm_throttle"
                    | "bsod"
            ) || e.code.as_deref() == Some("stage_verdict")
                || e.code.as_deref() == Some("data_mismatch");
            if marks {
                event_markers.push(EventMarker {
                    at_secs: x,
                    kind: kind.clone(),
                    detail: e.detail.clone(),
                });
            }
        }

        let failure_kind = (run.failure_kind != "none").then(|| run.failure_kind.clone());

        Self {
            run_id: format!("stress_test_run:{}", run.id.key_string()),
            hostname: run.hostname.clone(),
            machine_id: run.machine_id.clone(),
            tool_label: run.tool_label.clone(),
            preset_label: run.preset_label.clone(),
            tech: run.tech.clone(),
            service_order: run
                .service_order
                .as_ref()
                .map(|r| format!("service_order:{}", r.key_string())),
            started_at: datetime_rfc3339(&run.started_at),
            ended_at: run.ended_at.as_ref().map(datetime_rfc3339),
            duration_planned_secs: run.duration_planned_secs,
            duration_actual_secs: run.duration_actual_secs,
            result: run.result.as_str().to_string(),
            failure_kind,
            failure_detail: failure_detail(run),
            max_temp_c: run.summary.max_temp_c,
            avg_temp_c: run.summary.avg_temp_c,
            max_gpu_temp_c: run.summary.max_gpu_temp_c,
            max_cpu_temp_c: run.summary.max_cpu_temp_c,
            min_v12_v: run.summary.min_v12_v,
            max_clock_mhz: run.summary.max_clock_mhz,
            avg_clock_mhz: run.summary.avg_clock_mhz,
            max_power_w: run.summary.max_power_w,
            peak_throughput: run.summary.peak_throughput,
            avg_throughput: run.summary.avg_throughput,
            throughput_unit: run.summary.throughput_unit.clone(),
            whea_delta_count: run.summary.whea_delta_count,
            tdr_count: run.summary.tdr_count,
            test_errors: run.summary.test_errors,
            disk_io_errors: run.summary.disk_io_errors,
            thermal_throttle_detected: run.summary.thermal_throttle_detected,
            vrm_throttle_detected: run.summary.vrm_throttle_detected,
            stages: run.scenario_stages.clone(),
            cpu_temp,
            gpu_temp,
            avg_clock,
            throughput,
            stage_boundaries,
            event_markers,
            timeline,
        }
    }

    /// Curated telemetry-rollup rows for the summary block, label/value/hover
    /// sourced from the SHAPE walk so egui and terminal mode share one flatten.
    pub fn summary_rows(&self) -> Vec<Row> {
        const KEEP: &[&str] = &[
            "CPU max (°C)",
            "CPU avg (°C)",
            "GPU max (°C)",
            "Max clock (MHz)",
            "WHEA",
            "TDR",
            "Test errors",
            "Disk errors",
        ];
        let mut all = shape_walk::rows(self);
        KEEP.iter()
            .filter_map(|k| all.iter().position(|r| r.label == *k).map(|i| all.remove(i)))
            .collect()
    }
}

fn datetime_rfc3339(dt: &Datetime) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(dt.timestamp_millis())
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn failure_detail(run: &StressTestRun) -> Option<String> {
    use database::schema::FailureMode as F;
    Some(match &run.failure_mode {
        F::None => return None,
        F::AppError { message, .. } => format!("app error: {message}"),
        F::Bsod { code, .. } => format!("BSOD {}", code.as_deref().unwrap_or("(no code)")),
        F::Tdr { count } => format!("{count} GPU driver reset(s)"),
        F::GpuDeviceLost { message } => format!("GPU device lost: {message}"),
        F::WheaError { count } => format!("{count} WHEA hardware error(s)"),
        F::ThermalThrottle { peak_temp_c } => format!("thermal limit breached (peak {peak_temp_c:.1}°C)"),
        F::DiskIoError { message } => format!("disk I/O: {message}"),
        F::DataMismatch { .. } => "data mismatch detected".to_string(),
        F::ClockCollapse { stage_label, below_pct } => format!(
            "clock collapse in '{stage_label}' (below {:.0}% of stage max)",
            below_pct * 100.0
        ),
        F::ThroughputUnstable { stage_label, cv } => {
            format!("unstable throughput in '{stage_label}' (CV {cv:.3})")
        }
        F::Reboot => "unexpected reboot".to_string(),
        F::Timeout => "timed out".to_string(),
        F::OperatorOverride { reason } => format!("operator override: {reason}"),
        F::RailDroop { rail, min_v } => {
            format!("{rail} droop under load (min {min_v:.2}V, uncalibrated)")
        }
    })
}

/// Per-bucket max decimation, preserving first/last points.
fn decimate(points: &mut Vec<(f64, f64)>, max_points: usize) {
    if points.len() <= max_points || max_points < 2 {
        return;
    }
    let bucket = (points.len() as f64 / max_points as f64).ceil() as usize;
    let mut out = Vec::with_capacity(max_points);
    for chunk in points.chunks(bucket) {
        let max = chunk
            .iter()
            .copied()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("chunks are non-empty");
        out.push(max);
    }
    if let (Some(first), Some(out_first)) = (points.first(), out.first_mut()) {
        *out_first = *first;
    }
    if let (Some(last), Some(out_last)) = (points.last(), out.last_mut()) {
        *out_last = *last;
    }
    *points = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::{
        CoreSampleRow, EventKind, FailureMode, FinishReason, RunResult, RunSummary,
        StressTestEvent, StressTestMetric, StressTestRun, TargetKind, TestTool,
    };

    fn ms(base: i64, offset_secs: i64) -> Datetime {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(base + offset_secs * 1000)
            .unwrap()
            .into()
    }

    fn synthetic() -> RunReportData {
        let base = 1_700_000_000_000_i64;
        let run_id = RecordId::new("stress_test_run", "r1");
        let mut run = StressTestRun::new_for(
            RecordId::new("computer", "c1"),
            TestTool::StressKitScenario { name: Some("cert:gold-v1".into()) },
            TargetKind::System,
        );
        run.id = run_id.clone();
        run.started_at = ms(base, 0);
        run.ended_at = Some(ms(base, 120));
        run.result = RunResult::Fail;
        run.finish_reason = Some(FinishReason::Completed);
        run.failure_mode = FailureMode::Tdr { count: 1 };
        run.failure_kind = "tdr".into();
        run.preset_label = Some("cert:gold-v1".into());
        run.summary = RunSummary {
            max_temp_c: Some(91.0),
            tdr_count: 1,
            ..Default::default()
        };
        run.scenario_stages = vec![ScenarioStageSummary {
            index: 0,
            label: "cpu_fma".into(),
            result: Some("pass".into()),
            ..Default::default()
        }];

        let metrics = (0..120)
            .map(|i| {
                let mut m = StressTestMetric::new(run_id.clone(), ms(base, i));
                m.stage_index = Some(if i < 60 { 0 } else { 1 });
                m.stage_label = Some(if i < 60 { "cpu_fma" } else { "gpu" }.into());
                m.cores = vec![CoreSampleRow {
                    index: 0,
                    brand: "test".into(),
                    usage_pct: 99.0,
                    freq_mhz: 4500,
                    temp_c: Some(70.0 + (i % 10) as f32),
                }];
                m.throughput = Some(100.0 + i as f64);
                m.gpu_temp_c = if i >= 60 { Some(80.0) } else { None };
                m
            })
            .collect();

        let mut tdr_event = StressTestEvent::new(run_id.clone(), EventKind::Tdr, "telemetry");
        tdr_event.at = ms(base, 90);
        tdr_event.detail = "TDR counter moved 0 -> 1".into();

        RunReportData {
            run,
            metrics,
            events: vec![tdr_event],
        }
    }

    #[test]
    fn model_builds_with_boundaries_and_markers() {
        let model = RunReportModel::from_data(&synthetic());
        assert_eq!(model.result, "fail");
        assert_eq!(model.failure_kind.as_deref(), Some("tdr"));
        assert!(model.failure_detail.unwrap().contains("driver reset"));
        assert_eq!(model.stage_boundaries.len(), 2);
        assert_eq!(model.stage_boundaries[1].at_secs, 60.0);
        assert_eq!(model.event_markers.len(), 1);
        assert_eq!(model.event_markers[0].kind, "tdr");
        assert_eq!(model.timeline.len(), 1);
        assert_eq!(model.cpu_temp.points.len(), 120);
        // GPU temp only exists for the second stage's ticks.
        assert_eq!(model.gpu_temp.points.len(), 60);
        assert_eq!(model.stages.len(), 1);
        assert_eq!(model.preset_label.as_deref(), Some("cert:gold-v1"));
    }

    #[test]
    fn summary_rows_parity_with_hand_flatten() {
        let model = RunReportModel::from_data(&synthetic());
        let rows = model.summary_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "CPU max (°C)",
                "CPU avg (°C)",
                "GPU max (°C)",
                "Max clock (MHz)",
                "WHEA",
                "TDR",
                "Test errors",
                "Disk errors",
            ]
        );
        let by = |l: &str| rows.iter().find(|r| r.label == l).map(|r| r.value.as_str());
        // Old hand-flatten emitted these values (unit now folded into the label,
        // None renders "" instead of "—" — documented cosmetic shifts).
        assert_eq!(by("CPU max (°C)"), Some("91.0"));
        assert_eq!(by("CPU avg (°C)"), Some(""));
        assert_eq!(by("GPU max (°C)"), Some(""));
        assert_eq!(by("Max clock (MHz)"), Some(""));
        assert_eq!(by("WHEA"), Some("0"));
        assert_eq!(by("TDR"), Some("1"));
        assert_eq!(by("Test errors"), Some("0"));
        assert_eq!(by("Disk errors"), Some("0"));
        // Doc comments surface as hover help.
        let cpu_max = rows.iter().find(|r| r.label == "CPU max (°C)").unwrap();
        assert_eq!(
            cpu_max.hover.as_deref(),
            Some("Peak average-core temperature observed during the run.")
        );
    }

    #[test]
    fn decimation_caps_points_and_keeps_peaks() {
        let mut points: Vec<(f64, f64)> = (0..5000)
            .map(|i| (i as f64, if i == 2500 { 999.0 } else { 1.0 }))
            .collect();
        decimate(&mut points, 1024);
        assert!(points.len() <= 1024);
        assert!(points.iter().any(|p| p.1 == 999.0), "peak survives decimation");
        assert_eq!(points.first().unwrap().0, 0.0);
        assert_eq!(points.last().unwrap().0, 4999.0);
    }
}
