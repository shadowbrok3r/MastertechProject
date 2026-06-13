//! Scored benchmarks on top of the run pipeline. Each stressor-backed
//! benchmark drives a normal `RunController` run (so `stress_test_run` /
//! `stress_test_metric` / `stress_test_event` rows persist exactly like any
//! stress run), then condenses the tick stream into a steady-state score and
//! persists a `benchmark_result` row linked to the run. The memory-latency
//! ladder is a one-shot measurement persisted standalone.

use std::sync::Arc;
use std::time::Instant;

use database::schema::{BenchmarkKind, BenchmarkResult, RecordId, RecordIdExt, RunResult};
use stress_kit::{bench, telemetry::TelemetryAgent, Stressor};

use crate::controller::{RunPlan, RunSpec, RunUpdate};
use crate::drive::drive_blocking;
use crate::runtime;

/// Fraction of leading tick samples discarded as warmup.
const WARMUP_FRACTION: f64 = 0.25;
pub const DEFAULT_BENCH_SECS: u64 = 15;

/// `benchmark_result.notes` marker for zero-sample executions.
pub const NO_SAMPLES_NOTE: &str = "no throughput samples collected";

/// How a benchmark execution resolved: scored normally, ran clean but
/// produced zero throughput samples (score meaningless, non-fatal), or
/// failed outright.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkStatus {
    #[default]
    Scored,
    NoSamples,
    Error,
}

/// Condensed outcome returned to callers (MCP, UI).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkOutcome {
    pub kind: String,
    pub label: String,
    /// `scored`, `no_samples`, or `error`.
    #[serde(default)]
    pub status: BenchmarkStatus,
    pub score: f64,
    pub unit: String,
    pub peak: Option<f64>,
    pub low: Option<f64>,
    pub samples: u32,
    pub threads: u32,
    pub duration_secs: f64,
    pub errors: u32,
    pub max_temp_c: Option<f32>,
    pub avg_temp_c: Option<f32>,
    /// Non-fatal caveat invalidating score interpretation (e.g. discrete GPU
    /// driver stack broken — GPU kinds ran on the iGPU and score iGPU-class).
    #[serde(default)]
    pub warning: Option<String>,
    /// `benchmark_result` record id when persistence succeeded.
    pub result_id: Option<String>,
    /// Backing `stress_test_run` id for stressor-backed benchmarks.
    pub run_id: Option<String>,
    /// Ladder points for `memory_latency`.
    pub detail: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Stressor + thread shape behind each benchmark kind.
fn kind_plan(kind: BenchmarkKind) -> Option<(Stressor, usize)> {
    match kind {
        BenchmarkKind::CpuSingle => Some((Stressor::Fp, 1)),
        BenchmarkKind::CpuMulti => Some((Stressor::Fp, 0)),
        BenchmarkKind::MatrixSingle => Some((Stressor::Matrix, 1)),
        BenchmarkKind::MatrixMulti => Some((Stressor::Matrix, 0)),
        BenchmarkKind::Linpack => Some((Stressor::Linpack, 0)),
        BenchmarkKind::MemoryBandwidth => Some((Stressor::Stream, 0)),
        BenchmarkKind::Memcpy => Some((Stressor::Memcpy, 0)),
        BenchmarkKind::MemoryLatency => None,
        BenchmarkKind::Disk => Some((Stressor::Disk, 1)),
        BenchmarkKind::GpuCompute => Some((Stressor::Gpu, 0)),
        BenchmarkKind::GpuMatmul => Some((Stressor::GpuMatmul, 0)),
        BenchmarkKind::GpuVram => Some((Stressor::GpuVram, 0)),
        BenchmarkKind::GpuPcie => Some((Stressor::GpuPcie, 0)),
    }
}

pub fn parse_benchmark_kind(s: &str) -> Option<BenchmarkKind> {
    BenchmarkKind::all()
        .iter()
        .copied()
        .find(|k| k.as_str() == s.trim().to_ascii_lowercase())
}

/// The standard quick suite, cheapest-signal-first. GPU kinds are appended
/// by callers that confirmed a discrete GPU is present.
pub fn default_suite() -> Vec<BenchmarkKind> {
    vec![
        BenchmarkKind::CpuSingle,
        BenchmarkKind::CpuMulti,
        BenchmarkKind::MatrixMulti,
        BenchmarkKind::Linpack,
        BenchmarkKind::MemoryBandwidth,
        BenchmarkKind::Memcpy,
        BenchmarkKind::MemoryLatency,
        BenchmarkKind::Disk,
    ]
}

/// Run one benchmark to completion (blocking). Persists the score row and
/// returns the condensed outcome.
pub fn run_benchmark(
    kind: BenchmarkKind,
    computer: RecordId,
    telemetry: Arc<TelemetryAgent>,
    duration_secs: u64,
) -> BenchmarkOutcome {
    match kind_plan(kind) {
        Some((stressor, threads)) => {
            run_stressor_benchmark(kind, stressor, threads, computer, telemetry, duration_secs)
        }
        None => run_latency_ladder(computer),
    }
}

/// Execute a benchmark script by catalog name (see
/// [`crate::BENCHMARK_SCRIPT_NAMES`]): one outcome for a single kind, the
/// full standard set for "Benchmark Suite" (GPU kinds appended when
/// `include_gpu`). `None` for names not in the catalog.
pub fn run_benchmark_script(
    name: &str,
    computer: RecordId,
    telemetry: Arc<TelemetryAgent>,
    duration_secs: u64,
    include_gpu: bool,
) -> Option<Vec<BenchmarkOutcome>> {
    if let Some(kind) = crate::script_catalog::benchmark_kind_for_script(name) {
        return Some(vec![run_benchmark(kind, computer, telemetry, duration_secs)]);
    }
    if name != "Benchmark Suite" {
        return None;
    }
    if !include_gpu {
        if let Some(fault) = stress_kit::gpu_stack::check_gpu_stack().summary() {
            log::warn!(
                "stress-runner: GPU benchmark kinds skipped (telemetry reports no GPU) \
                 while a discrete controller is WMI-active — {fault}"
            );
        }
    }
    let mut kinds = default_suite();
    if include_gpu {
        kinds.extend([
            BenchmarkKind::GpuCompute,
            BenchmarkKind::GpuMatmul,
            BenchmarkKind::GpuVram,
            BenchmarkKind::GpuPcie,
        ]);
    }
    Some(
        kinds
            .into_iter()
            .map(|k| run_benchmark(k, computer.clone(), telemetry.clone(), duration_secs))
            .collect(),
    )
}

fn run_stressor_benchmark(
    kind: BenchmarkKind,
    stressor: Stressor,
    threads: usize,
    computer: RecordId,
    telemetry: Arc<TelemetryAgent>,
    duration_secs: u64,
) -> BenchmarkOutcome {
    let duration_secs = duration_secs.clamp(5, 600);
    // GPU scores are meaningless when the discrete card is WMI-active but
    // invisible to wgpu/NVML — the stressor silently runs on the iGPU.
    let warning = if matches!(
        stressor,
        Stressor::Gpu | Stressor::GpuMatmul | Stressor::GpuVram | Stressor::GpuPcie
    ) {
        stress_kit::gpu_stack::check_gpu_stack().summary()
    } else {
        None
    };
    let mut spec = RunSpec::single_stresskit(computer.clone(), stressor, Some(duration_secs));
    if let RunPlan::Single { threads: t, memory_cap_mb, .. } = &mut spec.plan {
        *t = threads;
        if matches!(stressor, Stressor::Linpack) {
            *memory_cap_mb = 1024;
        }
    }
    spec.preset_label = Some(format!("benchmark:{}", kind.as_str()));
    spec.tags = vec!["benchmark".into(), format!("benchmark:{}", kind.as_str())];

    let unit = stressor.throughput_unit().to_string();
    let mut throughputs: Vec<f64> = Vec::new();
    let mut run_id: Option<RecordId> = None;
    let mut error: Option<String> = None;
    let started = Instant::now();

    let verdict = drive_blocking(spec, telemetry, |update| match update {
        RunUpdate::Started { run_id: id } => run_id = Some(id),
        RunUpdate::Tick { metrics, .. } => {
            if metrics.throughput > 0.0 {
                throughputs.push(metrics.throughput);
            }
        }
        RunUpdate::Error { message } => error = Some(message),
        _ => {}
    });

    let wall_secs = started.elapsed().as_secs_f64();
    let warmup = ((throughputs.len() as f64) * WARMUP_FRACTION).floor() as usize;
    let steady = &throughputs[warmup.min(throughputs.len().saturating_sub(1))..];
    let samples = steady.len() as u32;
    let score = if steady.is_empty() {
        0.0
    } else {
        steady.iter().sum::<f64>() / steady.len() as f64
    };
    let peak = steady.iter().copied().fold(None::<f64>, |m, v| Some(m.map_or(v, |m| m.max(v))));
    let low = steady.iter().copied().fold(None::<f64>, |m, v| Some(m.map_or(v, |m| m.min(v))));

    let (errors, max_temp_c, avg_temp_c) = verdict
        .as_ref()
        .map(|v| {
            let whea = v.summary.whea_delta_count;
            (
                v.summary.test_errors.saturating_add(whea),
                v.summary.max_temp_c,
                v.summary.avg_temp_c,
            )
        })
        .unwrap_or((0, None, None));

    if error.is_none() {
        if let Some(v) = &verdict {
            if v.result == RunResult::Fail {
                error = Some(format!("run verdict: fail ({})", v.failure_mode.kind()));
            }
        } else {
            error = Some("benchmark run produced no verdict".to_string());
        }
    }
    let mut status = if error.is_some() {
        BenchmarkStatus::Error
    } else if samples == 0 {
        BenchmarkStatus::NoSamples
    } else {
        BenchmarkStatus::Scored
    };

    let used_threads = if threads == 0 {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as u32
    } else {
        threads as u32
    };

    let mut row = BenchmarkResult::new(computer, kind, score, unit.clone());
    row.run_ref = run_id.clone();
    row.peak = peak;
    row.low = low;
    row.samples = samples;
    row.threads = used_threads;
    row.duration_secs = wall_secs;
    row.errors = errors;
    row.max_temp_c = max_temp_c;
    row.avg_temp_c = avg_temp_c;
    row.hostname = sysinfo::System::host_name();
    if status == BenchmarkStatus::NoSamples {
        row.notes = Some(NO_SAMPLES_NOTE.to_string());
    }
    let result_id = match persist_result(&row) {
        Ok(id) => Some(id),
        Err(e) => {
            error.get_or_insert(format!("benchmark_result persist failed: {e}"));
            status = BenchmarkStatus::Error;
            None
        }
    };

    BenchmarkOutcome {
        kind: kind.as_str().to_string(),
        label: stressor.label().to_string(),
        status,
        score,
        unit,
        peak,
        low,
        samples,
        threads: used_threads,
        duration_secs: wall_secs,
        errors,
        max_temp_c,
        avg_temp_c,
        warning,
        result_id,
        run_id: run_id.map(|r| format!("{}:{}", r.table, r.key_string())),
        detail: None,
        error,
    }
}

fn run_latency_ladder(computer: RecordId) -> BenchmarkOutcome {
    let started = Instant::now();
    let sizes = bench::default_ladder_sizes_kb();
    let points = bench::measure_ladder(&sizes);
    let wall_secs = started.elapsed().as_secs_f64();

    // Score = ns/access at the largest (RAM-resident) footprint; lower is better.
    let score = points.last().map(|p| p.latency_ns).unwrap_or(0.0);
    let detail = serde_json::to_value(&points).ok();

    let mut status = if points.is_empty() {
        BenchmarkStatus::NoSamples
    } else {
        BenchmarkStatus::Scored
    };

    let mut row = BenchmarkResult::new(computer, BenchmarkKind::MemoryLatency, score, "ns");
    row.samples = points.len() as u32;
    row.threads = 1;
    row.duration_secs = wall_secs;
    row.detail = detail.clone();
    row.hostname = sysinfo::System::host_name();
    if status == BenchmarkStatus::NoSamples {
        row.notes = Some(NO_SAMPLES_NOTE.to_string());
    }
    let mut persist_error: Option<String> = None;
    let result_id = match persist_result(&row) {
        Ok(id) => Some(id),
        Err(e) => {
            persist_error = Some(format!("benchmark_result persist failed: {e}"));
            status = BenchmarkStatus::Error;
            None
        }
    };

    BenchmarkOutcome {
        kind: BenchmarkKind::MemoryLatency.as_str().to_string(),
        label: "Memory Latency Ladder".to_string(),
        status,
        score,
        unit: "ns".to_string(),
        peak: None,
        low: None,
        samples: points.len() as u32,
        threads: 1,
        duration_secs: wall_secs,
        errors: 0,
        max_temp_c: None,
        avg_temp_c: None,
        warning: None,
        result_id,
        run_id: None,
        detail,
        error: persist_error,
    }
}

fn persist_result(row: &BenchmarkResult) -> Result<String, String> {
    let row = row.clone();
    match runtime::block_on(async move { BenchmarkResult::create(&row).await }) {
        Ok(id) => Ok(format!("{}:{}", id.table, id.key_string())),
        Err(err) => {
            log::error!("stress-runner: benchmark_result persist failed: {err}");
            Err(err.to_string())
        }
    }
}
