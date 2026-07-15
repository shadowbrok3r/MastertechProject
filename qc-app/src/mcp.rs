//! Localhost MCP: JSON-RPC TCP `127.0.0.1:9100`, streamable HTTP `http://127.0.0.1:9101/mcp`.
//! No auth on loopback (same idea as `displays` MCP bridge).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::{
    handler::server::{wrapper::Parameters, tool::ToolRouter, ServerHandler},
    model::{
        CallToolResult, ContentBlock, ErrorData, Implementation, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use stress_kit::telemetry::TelemetryAgent;
use stress_kit::{Metrics, Stressor};
use stress_runner::{
    stressor_to_db, RecordId, RunController, RunPlan, RunSpec, RunStage, RunUpdate, TestTool,
};

use crate::hw_sampler::CoreRow;
use crate::reporting::ReportSink;
use crate::telemetry::{HwSnapshot, QcReport};

/// Snapshot of an in-flight stress run, observable via `get_run_status`.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RunSnapshot {
    /// Server-side run id. Stable for the lifetime of the run.
    pub run_id: String,
    /// "stressor" or "scenario".
    pub mode: String,
    /// User-visible label (stressor name or scenario stage label).
    pub label: String,
    pub elapsed_secs: f64,
    pub throughput: f64,
    pub throughput_unit: String,
    pub last_error: Option<String>,
    pub finished: bool,
    pub finished_reason: Option<String>,
}

/// Shared per-run handle so `stop_stress_run` / `get_run_status` can see
/// the same run that `run_stressor` / `run_stress_scenario` started.
#[derive(Default)]
pub struct RunSlot {
    pub cancel: Option<Arc<AtomicBool>>,
    pub latest: RunSnapshot,
}

pub struct QcMcpState {
    pub latest_cores: Arc<Mutex<Vec<CoreRow>>>,
    pub last_report: Arc<Mutex<Option<QcReport>>>,
    pub report_sink: Arc<Mutex<Option<ReportSink>>>,
    /// Shared telemetry agent. Held in `Option` so the state can be constructed
    /// before the sampler boots on the first frame.
    pub telemetry: Arc<Mutex<Option<Arc<TelemetryAgent>>>>,
    /// Stable `computer:<machine_id>` record this agent reports runs against.
    /// Populated once at state construction (mirrors `app::local_computer_record`).
    pub computer: RecordId,
    /// The active stress run (single or scenario), if any.
    pub run_slot: Arc<Mutex<RunSlot>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct NoArgs {}

/// JsonSchema-friendly DTO for `stress_kit::Stressor`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StressorKind {
    Cpu,
    Memory,
    Disk,
    Matrix,
    Memcpy,
    Bitops,
    Cache,
    Vm,
    Stream,
    Branch,
    Atomic,
    Mutex,
    Switch,
    Prime,
    Fp,
    Hash,
    Prefetch,
    Icache,
    Tsc,
    MemTest,
    CpuVerify,
    Linpack,
    Psu,
    Gpu,
    GpuMatmul,
    GpuVram,
    GpuPcie,
    Combined,
}

impl From<StressorKind> for Stressor {
    fn from(k: StressorKind) -> Self {
        match k {
            StressorKind::Cpu => Stressor::Cpu,
            StressorKind::Memory => Stressor::Memory,
            StressorKind::Disk => Stressor::Disk,
            StressorKind::Matrix => Stressor::Matrix,
            StressorKind::Memcpy => Stressor::Memcpy,
            StressorKind::Bitops => Stressor::Bitops,
            StressorKind::Cache => Stressor::Cache,
            StressorKind::Vm => Stressor::Vm,
            StressorKind::Stream => Stressor::Stream,
            StressorKind::Branch => Stressor::Branch,
            StressorKind::Atomic => Stressor::Atomic,
            StressorKind::Mutex => Stressor::Mutex,
            StressorKind::Switch => Stressor::Switch,
            StressorKind::Prime => Stressor::Prime,
            StressorKind::Fp => Stressor::Fp,
            StressorKind::Hash => Stressor::Hash,
            StressorKind::Prefetch => Stressor::Prefetch,
            StressorKind::Icache => Stressor::Icache,
            StressorKind::Tsc => Stressor::Tsc,
            StressorKind::MemTest => Stressor::MemTest,
            StressorKind::CpuVerify => Stressor::CpuVerify,
            StressorKind::Linpack => Stressor::Linpack,
            StressorKind::Psu => Stressor::Psu,
            StressorKind::Gpu => Stressor::Gpu,
            StressorKind::GpuMatmul => Stressor::GpuMatmul,
            StressorKind::GpuVram => Stressor::GpuVram,
            StressorKind::GpuPcie => Stressor::GpuPcie,
            StressorKind::Combined => Stressor::Combined,
        }
    }
}

/// One stage of a scenario submitted via MCP.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct StressStageArgs {
    pub label: String,
    pub stressor: StressorKind,
    /// `0` means "use one worker per logical CPU".
    #[serde(default)]
    pub threads: usize,
    /// Stage runtime in seconds (≥1).
    pub duration_secs: u64,
    /// Memory cap in MB for `memory` / `vm` stressors. Default 256.
    #[serde(default = "default_memory_cap_mb")]
    pub memory_cap_mb: u64,
    /// File size in MB for the `disk` stressor. Default 16.
    #[serde(default = "default_disk_file_mb")]
    pub disk_file_mb: u64,
}

fn default_memory_cap_mb() -> u64 {
    256
}

fn default_disk_file_mb() -> u64 {
    16
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunScenarioArgs {
    pub stages: Vec<StressStageArgs>,
    /// Cap the whole run in seconds. `None` means run each stage exactly once.
    #[serde(default)]
    pub total_wall_secs: Option<u64>,
    /// With `total_wall_secs`, loop the stage list until the cap is reached.
    #[serde(default)]
    pub repeat_until_total: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunConcurrentArgs {
    /// Lanes that run AT THE SAME TIME (e.g. cpu + memory + gpu). Each lane's
    /// `threads` defaults to an auto-budget across the core pool; per-lane
    /// `duration_secs` is ignored in favor of the shared `duration_secs` below.
    pub lanes: Vec<StressStageArgs>,
    /// How long to run all lanes together, in seconds. `None` = run until stopped.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScenarioReport {
    /// Server-side run id (`stress_test_run:<uuid>` formatted).
    pub run_id: String,
    pub finished_reason: String,
    pub total_elapsed_secs: f64,
    pub stages: Vec<StageReport>,
    /// Controller verdict + telemetry rollup, when the run finished normally.
    pub verdict: Option<RunVerdictDto>,
    /// First fatal `Error` emitted by the underlying RunController, if any.
    /// `None` on clean runs.
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StageReport {
    pub index: usize,
    pub label: String,
    pub last_metrics: Option<MetricsDto>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MetricsDto {
    pub elapsed_secs: f64,
    pub throughput: f64,
    pub last_error: Option<String>,
    /// Cumulative detected-error count (data mismatches, residual breaches).
    pub errors: u64,
}

impl From<&Metrics> for MetricsDto {
    fn from(m: &Metrics) -> Self {
        Self {
            elapsed_secs: m.elapsed_secs,
            throughput: m.throughput,
            last_error: m.last_error.clone(),
            errors: m.errors,
        }
    }
}

/// Condensed `RunVerdict` for the MCP wire: result, failure rubric, and the
/// telemetry rollup (temps, clocks, errors) the controller accumulated.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RunVerdictDto {
    /// "pass" | "fail" | "aborted" | "inconclusive".
    pub result: String,
    /// Lowercase `FailureMode` tag ("none", "whea_error", "data_mismatch", ...).
    pub failure_kind: String,
    /// Cumulative test-detected errors (memtest mismatches, cpu_verify
    /// divergences, linpack residual breaches, VRAM mismatches).
    pub test_errors: u32,
    pub whea_delta_count: u32,
    pub disk_io_errors: u32,
    pub max_temp_c: Option<f32>,
    pub avg_temp_c: Option<f32>,
    pub max_clock_mhz: Option<u32>,
    pub max_power_w: Option<u32>,
    pub peak_throughput: Option<f64>,
    pub avg_throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    pub tdr_count: u32,
    pub max_gpu_temp_c: Option<f32>,
    /// Per-stage rules verdicts; empty when the run carried no rules.
    pub stage_verdicts: Vec<StageVerdictDto>,
}

/// One stage's rules verdict on the MCP wire.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StageVerdictDto {
    pub index: usize,
    pub label: String,
    pub pass: bool,
    pub violations: Vec<String>,
    pub peak_throughput: Option<f64>,
}

impl From<&stress_runner::RunVerdict> for RunVerdictDto {
    fn from(v: &stress_runner::RunVerdict) -> Self {
        Self {
            result: format!("{:?}", v.result).to_lowercase(),
            failure_kind: v.failure_mode.kind().to_string(),
            test_errors: v.summary.test_errors,
            whea_delta_count: v.summary.whea_delta_count,
            disk_io_errors: v.summary.disk_io_errors,
            max_temp_c: v.summary.max_temp_c,
            avg_temp_c: v.summary.avg_temp_c,
            max_clock_mhz: v.summary.max_clock_mhz,
            max_power_w: v.summary.max_power_w,
            peak_throughput: v.summary.peak_throughput,
            avg_throughput: v.summary.avg_throughput,
            throughput_unit: v.summary.throughput_unit.clone(),
            tdr_count: v.summary.tdr_count,
            max_gpu_temp_c: v.summary.max_gpu_temp_c,
            stage_verdicts: v
                .stage_outcomes
                .iter()
                .filter_map(|o| {
                    o.verdict.as_ref().map(|sv| StageVerdictDto {
                        index: sv.index as usize,
                        label: sv.label.clone(),
                        pass: sv.pass,
                        violations: sv.violation_lines(),
                        peak_throughput: o.summary.peak_throughput,
                    })
                })
                .collect(),
        }
    }
}

#[derive(Clone)]
pub struct QcToolProvider {
    router: ToolRouter<QcToolProvider>,
    state: Arc<QcMcpState>,
}

#[tool_router]
impl QcToolProvider {
    #[tool(
        name = "get_hw_snapshot",
        description = "Per-core CPU %, MHz, °C when available."
    )]
    async fn get_hw_snapshot(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = self
            .state
            .latest_cores
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let snapshot = HwSnapshot::from_cores(&cores);
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_last_report",
        description = "Last `QcReport` this session, or null."
    )]
    async fn get_last_report(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .state
            .last_report
            .lock()
            .map(|g| g.clone())
            .unwrap_or(None);
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "send_report",
        description = "Enqueue a `QcReport` to the orchestrator sink (no await)."
    )]
    async fn send_report(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = self
            .state
            .latest_cores
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let snapshot = HwSnapshot::from_cores(&cores);

        let machine_id = self
            .state
            .report_sink
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.machine_id.as_ref().clone()))
            .unwrap_or_else(|| "unknown".to_string());

        let report = QcReport::new(&machine_id, snapshot);

        if let Ok(mut g) = self.state.last_report.lock() {
            *g = Some(report.clone());
        }

        if let Ok(g) = self.state.report_sink.lock() {
            if let Some(sink) = g.as_ref() {
                sink.send_report(report);
                return Ok(CallToolResult::success(vec![ContentBlock::text(
                    "Report queued for upload.".to_string(),
                )]));
            }
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(
            "Report generated but no orchestrator URL is configured — not uploaded.".to_string(),
        )]))
    }

    #[tool(
        name = "get_extended_telemetry",
        description = "Full TelemetrySnapshot: per-core CPU, memory (incl. vmmem and page file), per-disk r/w MB/s, per-adapter Mbps, WHEA delta + absolute (Windows)."
    )]
    async fn get_extended_telemetry(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let snapshot = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|a| a.snapshot()));
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_stress_scenario",
        description = "Run a multi-stage stress scenario via stress_runner::RunController. Blocks until done. MANDATORY persistence: hardware_component upsert, stress_test_run, stress_test_metric (~1 Hz), stress_test_event. Returns run_id in the report JSON."
    )]
    async fn run_stress_scenario(
        &self,
        Parameters(args): Parameters<RunScenarioArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.stages.is_empty() {
            return Err(to_internal("stages cannot be empty"));
        }

        let stages: Vec<RunStage> = args
            .stages
            .iter()
            .map(|s| RunStage {
                label: s.label.clone(),
                stressor: s.stressor.into(),
                threads: s.threads,
                duration_secs: s.duration_secs.max(1),
                memory_cap_mb: s.memory_cap_mb,
                disk_file_mb: s.disk_file_mb,
            })
            .collect();
        let labels: Vec<String> = stages.iter().map(|s| s.label.clone()).collect();

        let telemetry = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready (first frame pending)"))?;
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();

        let mut spec =
            RunSpec::single_stresskit(computer, stages.first().map(|s| s.stressor).unwrap_or(Stressor::Cpu), None);
        spec.plan = RunPlan::Scenario {
            stages,
            total_wall_secs: args.total_wall_secs,
            repeat_until_total: args.repeat_until_total,
        };
        spec.tool = TestTool::StressKitScenario {
            name: Some("qc-mcp:scenario".to_string()),
        };
        spec.preset_label = Some("qc-mcp:scenario".to_string());
        spec.tags = vec!["origin:mcp".into()];

        // RunController driving is blocking. Hop to a blocking thread so we
        // don't stall the tokio runtime.
        let report = tokio::task::spawn_blocking(move || {
            drive_scenario_via_controller(spec, telemetry, labels, slot)
        })
        .await
        .map_err(|e| to_internal(format!("scenario task join: {e}")))?;

        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_stressor",
        description = "Run a single stressor via stress_runner::RunController for `duration_secs` seconds. MANDATORY persistence: hardware_component, stress_test_run, stress_test_metric, stress_test_event. Returns run_id in the report JSON."
    )]
    async fn run_stressor(
        &self,
        Parameters(args): Parameters<RunStressorArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind = args.stressor;
        let stressor: Stressor = kind.into();
        let duration_secs = args.duration_secs.max(1);
        let label = stressor.label().to_string();
        let unit = stressor.throughput_unit().to_string();

        let telemetry = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready (first frame pending)"))?;
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();

        let mut spec = RunSpec::single_stresskit(computer, stressor, Some(duration_secs));
        spec.plan = RunPlan::Single {
            stressor,
            threads: args.threads,
            duration_secs: Some(duration_secs),
            memory_cap_mb: args.memory_cap_mb,
            disk_file_mb: args.disk_file_mb,
        };
        spec.tool = TestTool::StressKit {
            stressor: stressor_to_db(stressor),
        };
        spec.preset_label = Some(format!("qc-mcp:single:{}", stressor.label()));
        spec.tags = vec!["origin:mcp".into()];

        let report = tokio::task::spawn_blocking(move || {
            drive_single_via_controller(spec, telemetry, label, unit, slot)
        })
        .await
        .map_err(|e| to_internal(format!("stressor task join: {e}")))?;

        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "stop_stress_run",
        description = "Cancel any in-flight stress run (single or scenario). Returns the snapshot captured at cancel time."
    )]
    async fn stop_stress_run(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let snapshot = if let Ok(slot) = self.state.run_slot.lock() {
            if let Some(c) = slot.cancel.as_ref() {
                c.store(true, Ordering::SeqCst);
                Some(slot.latest.clone())
            } else {
                None
            }
        } else {
            None
        };
        let body = match snapshot {
            Some(snap) => serde_json::to_string_pretty(&snap)
                .map_err(|e| to_internal(e.to_string()))?,
            None => "{\"status\":\"idle\",\"detail\":\"no run in flight\"}".to_string(),
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(body)]))
    }

    #[tool(
        name = "get_run_status",
        description = "Non-blocking snapshot of the currently-active stress run (single or scenario), or `null` when idle."
    )]
    async fn get_run_status(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let snapshot = self
            .state
            .run_slot
            .lock()
            .ok()
            .filter(|s| s.cancel.is_some() || !s.latest.run_id.is_empty())
            .map(|s| s.latest.clone());
        let body = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(body)]))
    }

    #[tool(
        name = "run_qc_benchmark",
        description = "Run the curated 8-stage QC burn-in (cpu, matrix, fp, stream, cache, branch, memory, vm) and return a pass/warn/fail verdict. Routes through stress_runner so the full run + telemetry persists to SurrealDB. Watches WHEA deltas and per-stage throughput floors."
    )]
    async fn run_qc_benchmark(
        &self,
        Parameters(args): Parameters<QcBenchmarkArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // Capture WHEA baseline so we can detect any new MCEs during the run.
        let whea_before = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .and_then(|a| a.snapshot().whea.map(|w| w.total_retained));

        let mult = args.duration_multiplier.unwrap_or(1.0).max(0.1).min(10.0);
        let stages = qc_benchmark_stages(mult);
        let labels: Vec<String> = stages.iter().map(|s| s.label.clone()).collect();
        // Snapshot the (stressor, floor) pairs alongside each stage so the
        // verdict can map per-stage results back to their pass/fail floor.
        let floors: Vec<(Stressor, f64)> = stages
            .iter()
            .map(|s| (s.stressor, qc_floor_for(s.stressor)))
            .collect();

        let telemetry = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready"))?;
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();

        let mut spec =
            RunSpec::single_stresskit(computer, stages.first().map(|s| s.stressor).unwrap_or(Stressor::Cpu), None);
        spec.plan = RunPlan::Scenario {
            stages,
            total_wall_secs: None,
            repeat_until_total: false,
        };
        spec.tool = TestTool::StressKitScenario {
            name: Some(QC_BENCHMARK_PRESET.into()),
        };
        spec.preset_label = Some(QC_BENCHMARK_PRESET.into());
        spec.tags = vec!["origin:mcp".into(), "preset:qc-benchmark".into()];

        let scenario = tokio::task::spawn_blocking(move || {
            drive_scenario_via_controller(spec, telemetry, labels.clone(), slot)
        })
        .await
        .map_err(|e| to_internal(format!("benchmark task join: {e}")))?;

        // Post-run WHEA delta + final per-stage scoring.
        let whea_after = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .and_then(|a| a.snapshot().whea.map(|w| w.total_retained));
        let whea_delta = match (whea_before, whea_after) {
            (Some(b), Some(a)) => Some(a.saturating_sub(b)),
            _ => None,
        };

        let mut stage_results: Vec<QcStageResult> = Vec::with_capacity(scenario.stages.len());
        let mut failed_count = 0usize;
        let mut warn_count = 0usize;
        for (idx, stage) in scenario.stages.iter().enumerate() {
            let (stressor, floor) = floors.get(idx).copied().unwrap_or((Stressor::Cpu, 0.0));
            let throughput = stage.last_metrics.as_ref().map(|m| m.throughput).unwrap_or(0.0);
            let last_error = stage.last_metrics.as_ref().and_then(|m| m.last_error.clone());
            let mut status = if throughput >= floor {
                "pass"
            } else if throughput >= floor * 0.9 {
                warn_count += 1;
                "warn"
            } else {
                failed_count += 1;
                "fail"
            };
            // Any explicit per-tick error forces fail regardless of throughput.
            if last_error.is_some() {
                failed_count += 1;
                status = "fail";
            }
            stage_results.push(QcStageResult {
                index: idx,
                label: stage.label.clone(),
                stressor: stressor.label().to_string(),
                throughput,
                throughput_unit: stressor.throughput_unit().to_string(),
                floor,
                ratio: if floor > 0.0 { throughput / floor } else { 0.0 },
                last_error,
                status: status.into(),
            });
        }

        // Overall verdict precedence:
        //   `errored`       — RunController emitted a fatal Error (likely DB).
        //                     Distinct from `fail` because it's an *infra*
        //                     problem, not a hardware verdict.
        //   `fail`          — WHEA delta > 0, or any stage below 0.9× floor.
        //   `warn`          — any stage between 0.9× and 1.0× floor.
        //   `inconclusive`  — finished_reason != "completed" (cancelled/aborted).
        //   `pass`          — everything else.
        let verdict = if scenario.error.is_some() {
            "errored"
        } else if whea_delta.unwrap_or(0) > 0 {
            "fail"
        } else if failed_count > 0 {
            "fail"
        } else if warn_count > 0 {
            "warn"
        } else if scenario.finished_reason != "completed" {
            "inconclusive"
        } else {
            "pass"
        };

        let body = QcBenchmarkReport {
            preset: "qc-benchmark-v1".into(),
            verdict: verdict.into(),
            finished_reason: scenario.finished_reason,
            total_elapsed_secs: scenario.total_elapsed_secs,
            duration_multiplier: mult,
            whea_delta,
            stages: stage_results,
            reasoning: build_reasoning(
                verdict,
                whea_delta,
                failed_count,
                warn_count,
                scenario.error.as_deref(),
            ),
            error: scenario.error,
        };
        let json = serde_json::to_string_pretty(&body)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_certification",
        description = "Run a certification preset (bronze ~1.5h, silver ~3.5h, gold ~8h, platinum ~12h, power-virus ~30m) with per-stage verdict rules (WHEA/TDR/errors/temp limits/clock collapse/throughput stability). duration_multiplier scales stage durations (e.g. 0.005 for a smoke run). Full persistence via stress-runner; returns run_id, per-stage verdicts, and the run verdict."
    )]
    async fn run_certification(
        &self,
        Parameters(args): Parameters<RunCertificationArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let preset = stress_runner::load_cert_preset(&args.preset)
            .map_err(|e| to_internal(format!("{e:#}")))?;
        let mult = args.duration_multiplier.unwrap_or(1.0).clamp(0.001, 1.0);

        let telemetry = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready"))?;
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();

        let snapshot = telemetry.snapshot();
        let mut spec = if snapshot.memory.total_mb > 0 {
            let gpu_vram_mb = snapshot.gpus.iter().filter_map(|g| g.memory_total_mb).max();
            stress_runner::cert_spec(&preset, computer, snapshot.memory.total_mb, gpu_vram_mb, mult)
        } else {
            stress_runner::cert_spec_detected(&preset, computer, mult)
        };
        spec.tags.push("origin:mcp".into());

        let labels: Vec<String> = preset.stages.iter().map(|s| s.label.clone()).collect();
        let scenario = tokio::task::spawn_blocking(move || {
            drive_scenario_via_controller(spec, telemetry, labels, slot)
        })
        .await
        .map_err(|e| to_internal(format!("certification task join: {e}")))?;

        let body = serde_json::json!({
            "preset": preset.label,
            "duration_multiplier": mult,
            "run_id": scenario.run_id,
            "verdict": scenario.verdict.as_ref().map(|v| v.result.clone()).unwrap_or_else(|| "errored".into()),
            "failure_kind": scenario.verdict.as_ref().map(|v| v.failure_kind.clone()),
            "whea_delta": scenario.verdict.as_ref().map(|v| v.whea_delta_count),
            "tdr_delta": scenario.verdict.as_ref().map(|v| v.tdr_count),
            "test_errors": scenario.verdict.as_ref().map(|v| v.test_errors),
            "max_temp_c": scenario.verdict.as_ref().and_then(|v| v.max_temp_c),
            "max_gpu_temp_c": scenario.verdict.as_ref().and_then(|v| v.max_gpu_temp_c),
            "stage_verdicts": scenario.verdict.as_ref().map(|v| v.stage_verdicts.clone()),
            "finished_reason": scenario.finished_reason,
            "total_elapsed_secs": scenario.total_elapsed_secs,
            "error": scenario.error,
        });
        let json = serde_json::to_string_pretty(&body)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_run_report",
        description = "Fetch the full report model for a stress run: header, verdict, per-stage results with rule violations, decimated temp/clock/throughput chart series, stage boundaries, and the event timeline. run_id accepts 'stress_test_run:<key>' or a bare key; omit it for the most recent run in this session."
    )]
    async fn get_run_report(
        &self,
        Parameters(args): Parameters<GetRunReportArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let raw = match args.run_id {
            Some(id) => id,
            None => {
                let slot_id = self
                    .state
                    .run_slot
                    .lock()
                    .ok()
                    .map(|s| s.latest.run_id.clone())
                    .unwrap_or_default();
                if slot_id.is_empty() || slot_id.starts_with("pending-") {
                    return Err(to_internal(
                        "no run in this session yet — pass run_id explicitly",
                    ));
                }
                slot_id
            }
        };
        let key = raw.strip_prefix("stress_test_run:").unwrap_or(&raw);
        let run_id = stress_runner::RecordId::new("stress_test_run", key);
        let data = stress_runner::fetch_report_data(&run_id)
            .await
            .map_err(|e| to_internal(format!("{e:#}")))?;
        let model = stress_runner::RunReportModel::from_data(&data);
        let json = serde_json::to_string_pretty(&model)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_gpu_probe",
        description = "Run the curated 4-stage GPU probe (gpu_compute, gpu_matmul, gpu_vram, gpu_pcie) on the discrete GPU. MANDATORY persistence via stress-runner: hardware_component, stress_test_run, stress_test_metric, stress_test_event. Returns run_id, verdict, and TDR/PCIe/ECC deltas. ~2 min default."
    )]
    async fn run_gpu_probe(
        &self,
        Parameters(args): Parameters<QcBenchmarkArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // WMI vs wgpu/NVML cross-check; a broken user-mode stack means the
        // probe stages silently run on the iGPU and the verdict must fail.
        let driver_stack =
            tokio::task::spawn_blocking(stress_kit::gpu_stack::check_gpu_stack)
                .await
                .map_err(|e| to_internal(format!("gpu_stack check join: {e}")))?;
        let stack_fault = driver_stack.summary();

        let snap_before = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .map(|a| a.snapshot());
        let tdr_before = snap_before
            .as_ref()
            .and_then(|s| s.tdr.as_ref())
            .map(|t| t.absolute_since_boot);
        let pcie_replay_before = snap_before
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.pcie_replay_counter)
            .map(|c| c as u64);
        let ecc_corrected_before = snap_before
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.ecc_errors_corrected);
        let ecc_uncorrected_before = snap_before
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.ecc_errors_uncorrected);

        let mult = args.duration_multiplier.unwrap_or(1.0).max(0.1).min(10.0);
        let stages = gpu_probe_stages(mult);
        let labels: Vec<String> = stages.iter().map(|s| s.label.clone()).collect();
        let floors: Vec<(Stressor, f64)> = stages
            .iter()
            .map(|s| (s.stressor, qc_floor_for(s.stressor)))
            .collect();

        let telemetry = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready"))?;
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();

        let mut spec =
            RunSpec::single_stresskit(computer, stages.first().map(|s| s.stressor).unwrap_or(Stressor::Gpu), None);
        spec.plan = RunPlan::Scenario {
            stages,
            total_wall_secs: None,
            repeat_until_total: false,
        };
        spec.tool = TestTool::StressKitScenario {
            name: Some(GPU_PROBE_PRESET.into()),
        };
        spec.preset_label = Some(GPU_PROBE_PRESET.into());
        spec.tags = vec!["origin:mcp".into(), "preset:gpu-probe".into()];

        let scenario = tokio::task::spawn_blocking(move || {
            drive_scenario_via_controller(spec, telemetry, labels.clone(), slot)
        })
        .await
        .map_err(|e| to_internal(format!("gpu_probe task join: {e}")))?;

        let snap_after = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .map(|a| a.snapshot());
        let tdr_after = snap_after
            .as_ref()
            .and_then(|s| s.tdr.as_ref())
            .map(|t| t.absolute_since_boot);
        let pcie_replay_after = snap_after
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.pcie_replay_counter)
            .map(|c| c as u64);
        let ecc_corrected_after = snap_after
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.ecc_errors_corrected);
        let ecc_uncorrected_after = snap_after
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.ecc_errors_uncorrected);

        let tdr_delta = pair_delta(tdr_before, tdr_after);
        let pcie_replay_delta = pair_delta(pcie_replay_before, pcie_replay_after);
        let ecc_corrected_delta = pair_delta(ecc_corrected_before, ecc_corrected_after);
        let ecc_uncorrected_delta = pair_delta(ecc_uncorrected_before, ecc_uncorrected_after);

        let mut stage_results: Vec<QcStageResult> = Vec::with_capacity(scenario.stages.len());
        let mut failed_count = 0usize;
        let mut warn_count = 0usize;
        for (idx, stage) in scenario.stages.iter().enumerate() {
            let (stressor, floor) = floors.get(idx).copied().unwrap_or((Stressor::Gpu, 0.0));
            let throughput = stage.last_metrics.as_ref().map(|m| m.throughput).unwrap_or(0.0);
            let last_error = stage.last_metrics.as_ref().and_then(|m| m.last_error.clone());
            let mut status = if throughput >= floor {
                "pass"
            } else if throughput >= floor * 0.9 {
                warn_count += 1;
                "warn"
            } else {
                failed_count += 1;
                "fail"
            };
            if last_error.is_some() {
                failed_count += 1;
                status = "fail";
            }
            stage_results.push(QcStageResult {
                index: idx,
                label: stage.label.clone(),
                stressor: stressor.label().to_string(),
                throughput,
                throughput_unit: stressor.throughput_unit().to_string(),
                floor,
                ratio: if floor > 0.0 { throughput / floor } else { 0.0 },
                last_error,
                status: status.into(),
            });
        }

        let verdict = if scenario.error.is_some() {
            "errored"
        } else if stack_fault.is_some() {
            "fail"
        } else if ecc_uncorrected_delta.unwrap_or(0) > 0 {
            "fail"
        } else if tdr_delta.unwrap_or(0) > 0 {
            "fail"
        } else if failed_count > 0 {
            "fail"
        } else if ecc_corrected_delta.unwrap_or(0) > 0 || pcie_replay_delta.unwrap_or(0) > 0 {
            "warn"
        } else if warn_count > 0 {
            "warn"
        } else if scenario.finished_reason != "completed" {
            "inconclusive"
        } else {
            "pass"
        };

        let reasoning = build_gpu_reasoning(
            verdict,
            stack_fault.as_deref(),
            tdr_delta,
            pcie_replay_delta,
            ecc_corrected_delta,
            ecc_uncorrected_delta,
            failed_count,
            warn_count,
            scenario.error.as_deref(),
        );

        let body = GpuProbeReport {
            preset: "gpu-probe-v1".into(),
            verdict: verdict.into(),
            finished_reason: scenario.finished_reason,
            total_elapsed_secs: scenario.total_elapsed_secs,
            duration_multiplier: mult,
            tdr_delta,
            pcie_replay_delta,
            ecc_corrected_delta,
            ecc_uncorrected_delta,
            gpu_snapshot: snap_after.and_then(|s| s.gpus.into_iter().next()),
            driver_stack_broken: driver_stack.is_broken(),
            driver_stack,
            stages: stage_results,
            reasoning,
            error: scenario.error,
        };
        let json = serde_json::to_string_pretty(&body)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_gpu_telemetry",
        description = "Return the latest per-GPU telemetry sample: NVML-backed for NVIDIA (temp, power, clocks, util, mem, PCIe replay counter, ECC errors, throttle reasons), sysinfo fallback otherwise. Use this between stress runs to spot-check the card."
    )]
    async fn get_gpu_telemetry(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let gpus = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .map(|a| a.snapshot().gpus)
            .unwrap_or_default();
        let json = serde_json::to_string_pretty(&gpus)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "list_stressors",
        description = "Enumerate the stressors this build accepts, with default human-readable labels and throughput units."
    )]
    async fn list_stressors(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let kinds = [
            StressorKind::Cpu,
            StressorKind::Memory,
            StressorKind::Disk,
            StressorKind::Matrix,
            StressorKind::Memcpy,
            StressorKind::Bitops,
            StressorKind::Cache,
            StressorKind::Vm,
            StressorKind::Stream,
            StressorKind::Branch,
            StressorKind::Atomic,
            StressorKind::Mutex,
            StressorKind::Switch,
            StressorKind::Prime,
            StressorKind::Fp,
            StressorKind::Hash,
            StressorKind::Prefetch,
            StressorKind::Icache,
            StressorKind::Tsc,
            StressorKind::MemTest,
            StressorKind::CpuVerify,
            StressorKind::Linpack,
            StressorKind::Psu,
            StressorKind::Gpu,
            StressorKind::GpuMatmul,
            StressorKind::GpuVram,
            StressorKind::GpuPcie,
        ];
        let rows: Vec<serde_json::Value> = kinds
            .iter()
            .map(|k| {
                let s: Stressor = (*k).into();
                serde_json::json!({
                    "kind": serde_json::to_value(k).unwrap_or(serde_json::Value::Null),
                    "label": s.label(),
                    "throughput_unit": s.throughput_unit(),
                    "detects_errors": s.detects_errors(),
                })
            })
            .collect();
        let json = serde_json::to_string_pretty(&rows)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_memtest",
        description = "Pattern write/verify memory test (moving inversions, walking ones, address-in-address, random). Counts actual data mismatches — the OCCT/MemTest86-style RAM check. Default 4096 MiB for 600 s. Persists stress_test_run + metrics + events; mismatches land as memory_error events and fail the run. Returns verdict, error count, throughput, temps."
    )]
    async fn run_memtest(
        &self,
        Parameters(args): Parameters<MemTestArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .run_verified_single(
                "memtest",
                Stressor::MemTest,
                args.threads.unwrap_or(0),
                args.duration_secs.unwrap_or(600),
                args.memory_cap_mb.unwrap_or(4096).max(64),
                "qc-mcp:memtest-v1",
                "preset:memtest",
            )
            .await?;
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_cpu_stability",
        description = "CPU stability test with error detection: a deterministic integer+FP workload executes twice per seed and digests are compared — any divergence is silent data corruption (the OCCT CPU-test equivalent). Default 300 s, all threads. Persists like every stress run. Returns verdict, error count, Mop/s, temps, WHEA delta."
    )]
    async fn run_cpu_stability(
        &self,
        Parameters(args): Parameters<DurationOnlyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .run_verified_single(
                "cpu_stability",
                Stressor::CpuVerify,
                0,
                args.duration_secs.unwrap_or(300),
                256,
                "qc-mcp:cpu-stability-v1",
                "preset:cpu-stability",
            )
            .await?;
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_linpack",
        description = "Linpack-style benchmark/stress: repeated LU solves with partial pivoting; every solve's normalized residual is checked against the HPL threshold, so it both scores GFLOPS and detects compute errors. Default 120 s with a 1024 MiB matrix budget. Returns verdict, GFLOPS score (avg + peak), residual-breach count, temps."
    )]
    async fn run_linpack(
        &self,
        Parameters(args): Parameters<LinpackArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .run_verified_single(
                "linpack",
                Stressor::Linpack,
                0,
                args.duration_secs.unwrap_or(120),
                args.memory_cap_mb.unwrap_or(1024).max(64),
                "qc-mcp:linpack-v1",
                "preset:linpack",
            )
            .await?;
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_stress_concurrent",
        description = "Run multiple stressors AT THE SAME TIME (OCCT-style combined test: e.g. cpu + memory + gpu concurrently), each as its own lane with its own live metrics, via stress_runner::RunController. Blocks until done. Persists hardware_component, stress_test_run (target_kind=system), per-lane stress_test_metric (~1 Hz, tagged by stage_index), stress_test_event. Threads are auto-budgeted across cores with one reserved for the GPU lane. Returns run_id + per-lane reports."
    )]
    async fn run_stress_concurrent(
        &self,
        Parameters(args): Parameters<RunConcurrentArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.lanes.is_empty() {
            return Err(to_internal("lanes cannot be empty"));
        }

        let lanes: Vec<RunStage> = args
            .lanes
            .iter()
            .map(|s| RunStage {
                label: s.label.clone(),
                stressor: s.stressor.into(),
                threads: s.threads,
                duration_secs: 0,
                memory_cap_mb: s.memory_cap_mb,
                disk_file_mb: s.disk_file_mb,
            })
            .collect();
        let labels: Vec<String> = lanes.iter().map(|s| s.label.clone()).collect();

        let telemetry = self
            .state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready (first frame pending)"))?;
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();

        // Seed off Combined so the run's target_kind is System (whole-system).
        let mut spec = RunSpec::single_stresskit(computer, Stressor::Combined, None);
        spec.plan = RunPlan::Concurrent {
            lanes,
            duration_secs: args.duration_secs,
        };
        spec.tool = TestTool::StressKitScenario {
            name: Some("qc-mcp:concurrent".to_string()),
        };
        spec.preset_label = Some("qc-mcp:concurrent".to_string());
        spec.tags = vec!["origin:mcp".into(), "preset:concurrent".into()];

        let report = tokio::task::spawn_blocking(move || {
            drive_scenario_via_controller(spec, telemetry, labels, slot)
        })
        .await
        .map_err(|e| to_internal(format!("concurrent task join: {e}")))?;

        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_psu_test",
        description = "Power-supply / VRM load test: saturates all CPU cores with FMA chains while a compute shader hammers the GPU simultaneously (OCCT Power-style). Default 300 s. Watches WHEA, temps, and GPU board power; reports max observed power draw. Runs CPU-only with a warning when no GPU is present."
    )]
    async fn run_psu_test(
        &self,
        Parameters(args): Parameters<DurationOnlyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .run_verified_single(
                "psu",
                Stressor::Psu,
                0,
                args.duration_secs.unwrap_or(300),
                256,
                "qc-mcp:psu-v1",
                "preset:psu",
            )
            .await?;
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_combined_test",
        description = "Combined whole-system torture as a SINGLE fused stressor: CPU FMA + RAM bandwidth + GPU compute in one session. Default 300 s. Reports combined CPU+GPU GFLOPS; runs CPU+RAM with a warning when no GPU is present. (For independent per-component lanes + metrics use run_stress_concurrent instead.)"
    )]
    async fn run_combined_test(
        &self,
        Parameters(args): Parameters<DurationOnlyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .run_verified_single(
                "combined",
                Stressor::Combined,
                0,
                args.duration_secs.unwrap_or(300),
                1024,
                "qc-mcp:combined-v1",
                "preset:combined",
            )
            .await?;
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_benchmark",
        description = "Run one scored benchmark and persist a benchmark_result row (plus the backing stress_test_run). Kinds: cpu_single, cpu_multi, matrix_single, matrix_multi, linpack, memory_bandwidth, memcpy, memory_latency, disk, gpu_compute, gpu_matmul, gpu_vram, gpu_pcie. Default 15 s measurement; warmup discarded; returns steady-state score, peak/low, temps, errors."
    )]
    async fn run_benchmark(
        &self,
        Parameters(args): Parameters<BenchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind = stress_runner::parse_benchmark_kind(&args.kind)
            .ok_or_else(|| to_internal(format!("unknown benchmark kind '{}'", args.kind)))?;
        let telemetry = self.telemetry_or_err()?;
        let computer = self.state.computer.clone();
        let secs = args.duration_secs.unwrap_or(stress_runner::DEFAULT_BENCH_SECS);

        let outcome = tokio::task::spawn_blocking(move || {
            stress_runner::run_benchmark(kind, computer, telemetry, secs)
        })
        .await
        .map_err(|e| to_internal(format!("benchmark task join: {e}")))?;

        let json = serde_json::to_string_pretty(&outcome)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "run_benchmark_suite",
        description = "Run the standard benchmark suite sequentially (cpu single/multi, matrix, linpack, memory bandwidth, memcpy, latency ladder, disk; GPU kinds appended when a GPU is detected or include_gpu=true). Each persists a benchmark_result row. Default 15 s per benchmark — roughly 2-3 minutes CPU-only. Returns all scores."
    )]
    async fn run_benchmark_suite(
        &self,
        Parameters(args): Parameters<BenchSuiteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let telemetry = self.telemetry_or_err()?;
        let computer = self.state.computer.clone();
        let secs = args.duration_secs.unwrap_or(stress_runner::DEFAULT_BENCH_SECS);
        let has_gpu = !telemetry.snapshot().gpus.is_empty();
        let include_gpu = args.include_gpu.unwrap_or(has_gpu);

        // Catches the broken-stack machine where telemetry sees no GPU (NVML
        // dead) so GPU kinds get skipped despite a WMI-active discrete card.
        let gpu_stack_warning =
            tokio::task::spawn_blocking(stress_kit::gpu_stack::check_gpu_stack)
                .await
                .map_err(|e| to_internal(format!("gpu_stack check join: {e}")))?
                .summary();

        let outcomes = tokio::task::spawn_blocking(move || {
            let mut kinds = stress_runner::default_suite();
            if include_gpu {
                kinds.extend([
                    stress_runner::BenchmarkKind::GpuCompute,
                    stress_runner::BenchmarkKind::GpuMatmul,
                    stress_runner::BenchmarkKind::GpuVram,
                    stress_runner::BenchmarkKind::GpuPcie,
                ]);
            }
            kinds
                .into_iter()
                .map(|k| stress_runner::run_benchmark(k, computer.clone(), telemetry.clone(), secs))
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| to_internal(format!("suite task join: {e}")))?;

        let total_errors: u32 = outcomes.iter().map(|o| o.errors).sum();
        let body = serde_json::json!({
            "count": outcomes.len(),
            "total_errors": total_errors,
            "gpu_stack_warning": gpu_stack_warning,
            "results": outcomes,
        });
        let json = serde_json::to_string_pretty(&body)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_benchmark_results",
        description = "Query persisted benchmark_result history from SurrealDB. Defaults to this machine, newest first; filter by kind; set all_machines=true for cross-machine population comparison of one kind."
    )]
    async fn get_benchmark_results(
        &self,
        Parameters(args): Parameters<BenchHistoryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::BenchmarkResult;
        let limit = args.limit.unwrap_or(20);
        let kind = match &args.kind {
            Some(k) => Some(
                stress_runner::parse_benchmark_kind(k)
                    .ok_or_else(|| to_internal(format!("unknown benchmark kind '{k}'")))?,
            ),
            None => None,
        };

        let rows = if args.all_machines.unwrap_or(false) {
            let kind = kind.ok_or_else(|| {
                to_internal("all_machines=true requires a `kind` to compare across machines")
            })?;
            BenchmarkResult::list_for_kind(kind, limit)
                .await
                .map_err(|e| to_internal(format!("benchmark_result query failed: {e}")))?
        } else {
            BenchmarkResult::list_for_computer(&self.state.computer, kind, limit)
                .await
                .map_err(|e| to_internal(format!("benchmark_result query failed: {e}")))?
        };

        let json = serde_json::to_string_pretty(&rows)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "measure_memory_latency",
        description = "One-shot memory-hierarchy ladder (4 KiB to 128 MiB): pointer-chase latency (ns/access) and sequential read bandwidth (GB/s) per working-set size. Takes a few seconds; persists a benchmark_result row with the full ladder in `detail`. L1/L2/L3/RAM transitions are visible as latency steps."
    )]
    async fn measure_memory_latency(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let computer = self.state.computer.clone();
        let telemetry = self.telemetry_or_err()?;
        let outcome = tokio::task::spawn_blocking(move || {
            stress_runner::run_benchmark(
                stress_runner::BenchmarkKind::MemoryLatency,
                computer,
                telemetry,
                0,
            )
        })
        .await
        .map_err(|e| to_internal(format!("ladder task join: {e}")))?;
        let json = serde_json::to_string_pretty(&outcome)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_temperatures",
        description = "All temperature sources in one flat list: per-core CPU °C, per-GPU °C, and ACPI thermal zones (Windows), plus max per source. Cheap — reads the latest telemetry sample."
    )]
    async fn get_temperatures(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let snapshot = self.telemetry_or_err()?.snapshot();

        let mut readings: Vec<serde_json::Value> = Vec::new();
        let mut max_cpu: Option<f32> = None;
        for c in &snapshot.cores {
            if let Some(t) = c.temp_c {
                max_cpu = Some(max_cpu.map_or(t, |m| m.max(t)));
                readings.push(serde_json::json!({
                    "source": "cpu_core",
                    "label": format!("core {}", c.index),
                    "temp_c": t,
                }));
            }
        }
        let mut max_gpu: Option<f32> = None;
        for g in &snapshot.gpus {
            if let Some(t) = g.temp_c {
                max_gpu = Some(max_gpu.map_or(t, |m| m.max(t)));
                readings.push(serde_json::json!({
                    "source": "gpu",
                    "label": g.name,
                    "temp_c": t,
                }));
            }
        }
        let mut max_zone: Option<f32> = None;
        for z in &snapshot.thermals {
            max_zone = Some(max_zone.map_or(z.temp_c, |m| m.max(z.temp_c)));
            readings.push(serde_json::json!({
                "source": "thermal_zone",
                "label": z.label,
                "temp_c": z.temp_c,
            }));
        }

        let body = serde_json::json!({
            "captured_at_unix_ms": snapshot.captured_at_unix_ms,
            "max_cpu_core_c": max_cpu,
            "max_gpu_c": max_gpu,
            "max_thermal_zone_c": max_zone,
            "readings": readings,
        });
        let json = serde_json::to_string_pretty(&body)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_system_identity",
        description = "Stable machine identity: machine_id, system + board serials, baseboard product, BIOS version, OA3 OEM key, GPU PCI device codes (WMI/SMBIOS, Windows)."
    )]
    async fn get_system_identity(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = tokio::task::spawn_blocking(crate::diagnostics::system_identity)
            .await
            .map_err(|e| to_internal(format!("identity task join: {e}")))?;
        let json = serde_json::to_string_pretty(&id).map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_smbios",
        description = "Parsed SMBIOS type 0/1/2 fields (system/board/BIOS manufacturer, product, serial, version) read natively via GetSystemFirmwareTable. Windows."
    )]
    async fn get_smbios(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let res = tokio::task::spawn_blocking(crate::provisioning::dmi::read_smbios)
            .await
            .map_err(|e| to_internal(format!("smbios task join: {e}")))?
            .map_err(|e| to_internal(e.to_string()))?;
        let json = serde_json::to_string_pretty(&res).map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_firmware_security",
        description = "Secure Boot enabled, boot mode (UEFI/Legacy), TPM presence/version/manufacturer, and Windows activation status. Windows."
    )]
    async fn get_firmware_security(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let fs = tokio::task::spawn_blocking(crate::diagnostics::firmware_security)
            .await
            .map_err(|e| to_internal(format!("firmware task join: {e}")))?;
        let json = serde_json::to_string_pretty(&fs).map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_storage_health",
        description = "Per-physical-disk health: friendly name, serial, size, media type (HDD/SSD), bus (SATA/NVMe/USB), and HealthStatus (Healthy/Warning/Unhealthy). Windows Storage WMI."
    )]
    async fn get_storage_health(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let disks = tokio::task::spawn_blocking(crate::diagnostics::storage_health)
            .await
            .map_err(|e| to_internal(format!("storage task join: {e}")))?;
        let json = serde_json::to_string_pretty(&disks).map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_driver_check",
        description = "Per-part driver comparison for this machine: installed driver (name + version) vs the catalog's expected/target driver for each part (chipset, GPU, audio, LAN, WiFi, Bluetooth, RAID), plus the list of expected drivers that are missing. Windows."
    )]
    async fn get_driver_check(
        &self,
        Parameters(_p): Parameters<NoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let rows = tokio::task::spawn_blocking(
            || -> Result<Vec<crate::driver_check::DriverCheckRow>, String> {
                let installed = crate::diagnostics::installed_drivers();
                let path = crate::db::default_sqlite_path();
                let conn = crate::db::open_or_create(&path).map_err(|e| e.to_string())?;
                let product = crate::hardware_id::read_baseboard_product().unwrap_or_default();
                let package = if product.is_empty() {
                    None
                } else {
                    crate::provisioning::catalog_query::package_drivers_for_baseboard(&conn, &product)
                        .map_err(|e| e.to_string())?
                }
                .unwrap_or_default();
                let mut gpu_targets = Vec::new();
                for code in crate::hardware_id::read_gpu_device_codes() {
                    if let Ok(Some(r)) =
                        crate::provisioning::catalog_query::gpu_driver_for_device(&conn, &code)
                    {
                        gpu_targets.push(crate::provisioning::catalog_query::TargetDriver {
                            file: r.file_name,
                            version: r.version,
                        });
                    }
                }
                Ok(crate::driver_check::build_driver_check(&installed, &package, &gpu_targets))
            },
        )
        .await
        .map_err(|e| to_internal(format!("driver check task join: {e}")))?
        .map_err(|e| to_internal(e))?;
        let json = serde_json::to_string_pretty(&rows).map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        name = "get_preboot_fingerprint",
        description = "Fetch the pre-boot UEFI hardware fingerprint the orchestrator stored for a serial (defaults to this machine's detected serial). Returns the stored fingerprint JSON, or an error if none/unconfigured."
    )]
    async fn get_preboot_fingerprint(
        &self,
        Parameters(args): Parameters<PrebootArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let base = database::orchestrator_url().to_string();
        if base.is_empty() {
            return Err(to_internal("no orchestrator URL configured"));
        }
        let serial = match args.serial {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => {
                let id = tokio::task::spawn_blocking(crate::diagnostics::system_identity)
                    .await
                    .map_err(|e| to_internal(format!("identity task join: {e}")))?;
                id.system_serial
                    .or(id.board_serial)
                    .ok_or_else(|| to_internal("no machine serial detected; pass `serial`"))?
            }
        };
        let url = format!("{base}/api/v1/qc/fingerprint/{serial}");
        let resp = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .map_err(|e| to_internal(format!("fetch {url}: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(to_internal(format!("orchestrator returned {status} for serial {serial}")));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(body)]))
    }

    #[tool(
        name = "get_preboot_history",
        description = "Per-boot UEFI fingerprint history + boot-to-boot variance for a serial (defaults to this machine's serial). Flags RAM/DIMM/disk counts changing between boots and any pcie_degraded / bert_error / mca / mem_errors — the key signal for an intermittent hardware fault."
    )]
    async fn get_preboot_history(
        &self,
        Parameters(args): Parameters<PrebootArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let base = database::orchestrator_url().to_string();
        if base.is_empty() {
            return Err(to_internal("no orchestrator URL configured"));
        }
        let serial = match args.serial {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => {
                let id = tokio::task::spawn_blocking(crate::diagnostics::system_identity)
                    .await
                    .map_err(|e| to_internal(format!("identity task join: {e}")))?;
                id.system_serial
                    .or(id.board_serial)
                    .ok_or_else(|| to_internal("no machine serial detected; pass `serial`"))?
            }
        };
        let url = format!("{base}/api/v1/qc/fingerprint/{serial}/history");
        let resp = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .map_err(|e| to_internal(format!("fetch {url}: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(to_internal(format!("orchestrator returned {status} for serial {serial}")));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(body)]))
    }
}

/// `get_preboot_fingerprint` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PrebootArgs {
    /// Machine serial to look up; omit to use this machine's detected serial.
    #[serde(default)]
    pub serial: Option<String>,
}

/// `run_qc_benchmark` arguments. Both fields optional — defaults give a ~2.7-minute
/// representative burn-in that exercises the eight subsystems most likely to
/// surface marginal silicon on a fresh PC build.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QcBenchmarkArgs {
    /// Scales every stage's `duration_secs`. `1.0` = ~20 s/stage (default).
    /// `0.5` = quick smoke (~80 s total), `2.0` = thorough (~5.5 min total).
    /// Clamped to `[0.1, 10.0]` server-side.
    #[serde(default, deserialize_with = "deser_opt_f32_or_str")]
    pub duration_multiplier: Option<f32>,
}

/// `run_certification` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunCertificationArgs {
    /// Preset name: "bronze", "silver", "gold", "platinum", or "power-virus".
    pub preset: String,
    /// Scales every stage's `duration_secs`; `1.0` = full certification.
    /// Clamped to `[0.001, 1.0]` server-side — use e.g. `0.005` for smoke.
    #[serde(default, deserialize_with = "deser_opt_f32_or_str")]
    pub duration_multiplier: Option<f32>,
}

/// `get_run_report` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetRunReportArgs {
    /// `stress_test_run:<key>` or bare key; omit for the session's latest run.
    #[serde(default)]
    pub run_id: Option<String>,
}

/// Accept both a JSON number and a JSON string for f32 fields.
/// The MCP harness sometimes encodes float arguments as `"0.25"` (string)
/// rather than `0.25` (number); this handles both without rejecting either.
fn deser_opt_f32_or_str<'de, D>(de: D) -> Result<Option<f32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, Visitor};
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<f32>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "a float or string-encoded float, or null")
        }
        fn visit_none<E: Error>(self) -> Result<Option<f32>, E> { Ok(None) }
        fn visit_unit<E: Error>(self) -> Result<Option<f32>, E> { Ok(None) }
        fn visit_f32<E: Error>(self, v: f32) -> Result<Option<f32>, E> { Ok(Some(v)) }
        fn visit_f64<E: Error>(self, v: f64) -> Result<Option<f32>, E> { Ok(Some(v as f32)) }
        fn visit_i64<E: Error>(self, v: i64) -> Result<Option<f32>, E> { Ok(Some(v as f32)) }
        fn visit_u64<E: Error>(self, v: u64) -> Result<Option<f32>, E> { Ok(Some(v as f32)) }
        fn visit_str<E: Error>(self, v: &str) -> Result<Option<f32>, E> {
            v.parse::<f32>().map(Some).map_err(|_| E::custom(format!("expected float, got {:?}", v)))
        }
        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Option<f32>, D2::Error> {
            d.deserialize_any(V)
        }
    }
    de.deserialize_option(V)
}

/// One stage's pass/fail breakdown in [`QcBenchmarkReport`].
#[derive(Debug, Serialize, JsonSchema)]
pub struct QcStageResult {
    pub index: usize,
    pub label: String,
    pub stressor: String,
    pub throughput: f64,
    pub throughput_unit: String,
    /// Hard-coded minimum throughput this build expects for `stressor`. Will be
    /// replaced by per-component `hardware_test_baseline` lookups in a later
    /// step. Until then, the floors are a permissive baseline that any modern
    /// (post-2018) consumer CPU should clear; warning band is 0.9× floor.
    pub floor: f64,
    /// `throughput / floor` for fast inspection.
    pub ratio: f64,
    pub last_error: Option<String>,
    /// `"pass"` / `"warn"` / `"fail"`.
    pub status: String,
}

/// Final report shape for `run_qc_benchmark`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QcBenchmarkReport {
    pub preset: String,
    /// `"pass"` / `"warn"` / `"fail"` / `"inconclusive"`.
    pub verdict: String,
    pub finished_reason: String,
    pub total_elapsed_secs: f64,
    pub duration_multiplier: f32,
    /// New WHEA records (machine-check exceptions) observed during the run.
    /// `None` on non-Windows where the counter isn't readable. Any value > 0
    /// forces a `fail` verdict.
    pub whea_delta: Option<u64>,
    pub stages: Vec<QcStageResult>,
    /// Human-readable explanation of how the verdict was derived.
    pub reasoning: String,
    /// Fatal error from the underlying RunController (DB create failure, etc).
    /// When present, `verdict == "errored"` and per-stage results should be
    /// disregarded — the run never produced real telemetry.
    pub error: Option<String>,
}

use crate::qc_benchmark::{gpu_probe_stages, qc_benchmark_stages, qc_floor_for, GPU_PROBE_PRESET, QC_BENCHMARK_PRESET};
use stress_kit::telemetry::GpuSample;

fn pair_delta<T: PartialOrd + std::ops::Sub<Output = T> + Copy>(before: Option<T>, after: Option<T>) -> Option<T> {
    match (before, after) {
        (Some(b), Some(a)) if a >= b => Some(a - b),
        (Some(_), Some(a)) => Some(a),
        _ => None,
    }
}

fn build_gpu_reasoning(
    verdict: &str,
    stack_fault: Option<&str>,
    tdr_delta: Option<u64>,
    pcie_replay_delta: Option<u64>,
    ecc_corrected_delta: Option<u64>,
    ecc_uncorrected_delta: Option<u64>,
    failed_count: usize,
    warn_count: usize,
    error: Option<&str>,
) -> String {
    match verdict {
        "errored" => {
            let mut s = format!(
                "RunController reported a fatal error before/during the GPU probe. \
                 Per-stage numbers are not reliable telemetry.\n\nError: {}",
                error.unwrap_or("<missing>")
            );
            if let Some(fault) = stack_fault {
                s.push_str(&format!("\n\nAdditionally: {fault}"));
            }
            s
        }
        "pass" => "All four GPU stages cleared their throughput floors. No VRAM mismatches, \
             no new TDR events, no ECC errors, no PCIe replay deltas. GPU subsystem healthy."
            .into(),
        "warn" => {
            let mut bits = Vec::new();
            if ecc_corrected_delta.unwrap_or(0) > 0 {
                bits.push(format!(
                    "{} corrected ECC error(s) during the run — VRAM error correction is engaging. \
                     Card is still functional but the cell health is degrading.",
                    ecc_corrected_delta.unwrap_or(0)
                ));
            }
            if pcie_replay_delta.unwrap_or(0) > 0 {
                bits.push(format!(
                    "{} new PCIe replay event(s) during the run — link instability under load. \
                     Suspect PSU sag, riser cable, or marginal PCIe slot.",
                    pcie_replay_delta.unwrap_or(0)
                ));
            }
            if warn_count > 0 {
                bits.push(format!(
                    "{warn_count} stage(s) finished between 90% and 100% of throughput floor."
                ));
            }
            if bits.is_empty() {
                "Run completed with marginal indicators. See per-stage status.".into()
            } else {
                bits.join("\n")
            }
        }
        "fail" => {
            let mut bits = Vec::new();
            if let Some(fault) = stack_fault {
                bits.push(format!(
                    "{fault}\nStage throughputs below came from whichever adapter wgpu \
                     could still see (typically the iGPU) — do not read them as \
                     discrete-GPU scores."
                ));
            }
            if ecc_uncorrected_delta.unwrap_or(0) > 0 {
                bits.push(format!(
                    "{} UNCORRECTED ECC error(s) during the run — VRAM is failing. Replace the card.",
                    ecc_uncorrected_delta.unwrap_or(0)
                ));
            }
            if tdr_delta.unwrap_or(0) > 0 {
                bits.push(format!(
                    "{} new TDR event(s) (nvlddmkm/amdkmdap Event 4101/4109) during the run — \
                     driver had to reset the GPU. Either the driver is broken or the hardware is.",
                    tdr_delta.unwrap_or(0)
                ));
            }
            if failed_count > 0 {
                bits.push(format!(
                    "{failed_count} stage(s) fell below 90% of the throughput floor or surfaced a runtime error \
                     (e.g. VRAM verify mismatch). Treat as hardware fault until ruled out."
                ));
            }
            if bits.is_empty() {
                "Run did not complete cleanly. See `finished_reason` and per-stage `status`.".into()
            } else {
                bits.join("\n")
            }
        }
        "inconclusive" => "Run did not complete normally (cancelled or aborted). Re-run for a clean result.".into(),
        _ => format!("Unknown verdict: {verdict}"),
    }
}

#[derive(Debug, Serialize)]
pub struct GpuProbeReport {
    pub preset: String,
    pub verdict: String,
    pub finished_reason: String,
    pub total_elapsed_secs: f64,
    pub duration_multiplier: f32,
    pub tdr_delta: Option<u64>,
    pub pcie_replay_delta: Option<u64>,
    pub ecc_corrected_delta: Option<u64>,
    pub ecc_uncorrected_delta: Option<u64>,
    pub gpu_snapshot: Option<GpuSample>,
    /// True when a WMI-active discrete controller is missing from wgpu/NVML;
    /// forces `verdict == "fail"` — stage scores came from the iGPU.
    pub driver_stack_broken: bool,
    pub driver_stack: stress_kit::gpu_stack::GpuStackReport,
    pub stages: Vec<QcStageResult>,
    pub reasoning: String,
    pub error: Option<String>,
}

fn build_reasoning(
    verdict: &str,
    whea_delta: Option<u64>,
    failed_count: usize,
    warn_count: usize,
    error: Option<&str>,
) -> String {
    match verdict {
        "errored" => format!(
            "RunController reported a fatal error before/during the run — most likely \
             the initial `stress_test_run` row failed to persist to SurrealDB, which \
             bails the worker before any stressor starts. Per-stage numbers below are \
             not real telemetry, just zeros from the empty event stream.\n\
             \nError: {}",
            error.unwrap_or("<missing>")
        ),
        "pass" => {
            "All 8 stages cleared their throughput floors; no machine-check exceptions \
             observed; no stage reported an inner error. Hardware looks healthy."
                .into()
        }
        "warn" => format!(
            "{warn_count} stage(s) finished between 90% and 100% of their throughput floor — \
             borderline, but no fatal failures. Re-run after letting the machine cool, \
             or compare to the per-component baseline if you have one."
        ),
        "fail" => {
            let mut bits = Vec::new();
            if let Some(d) = whea_delta {
                if d > 0 {
                    bits.push(format!(
                        "{d} new WHEA (machine-check) record(s) during the run — the CPU or \
                         memory controller raised a hardware error. Treat as fail regardless \
                         of throughput numbers."
                    ));
                }
            }
            if failed_count > 0 {
                bits.push(format!(
                    "{failed_count} stage(s) below 90% of the throughput floor or reported \
                     a runtime error."
                ));
            }
            if bits.is_empty() {
                "Run did not complete cleanly. See `finished_reason` and per-stage `status`.".into()
            } else {
                bits.join("\n")
            }
        }
        "inconclusive" => {
            "Run did not complete normally (cancelled or aborted). No verdict — \
             re-run to get a clean result."
                .into()
        }
        _ => format!("Unknown verdict: {verdict}"),
    }
}

/// One single-stressor invocation.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunStressorArgs {
    pub stressor: StressorKind,
    /// 0 = one worker per logical CPU.
    #[serde(default)]
    pub threads: usize,
    /// Stop after this many seconds. Required (the MCP path is for finite runs).
    pub duration_secs: u64,
    #[serde(default = "default_memory_cap_mb")]
    pub memory_cap_mb: u64,
    #[serde(default = "default_disk_file_mb")]
    pub disk_file_mb: u64,
}

/// Final report for a single-stressor run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StressorReport {
    pub run_id: String,
    pub label: String,
    pub throughput_unit: String,
    pub elapsed_secs: f64,
    pub last_metrics: Option<MetricsDto>,
    /// Controller verdict + telemetry rollup, when the run finished normally.
    pub verdict: Option<RunVerdictDto>,
    pub finished_reason: String,
    /// First fatal `Error` (or `Warning`) emitted by the underlying RunController,
    /// if any. `None` on clean runs. Present + `finished_reason: "controller_exited"`
    /// usually means the row create failed before the stressor even started.
    pub error: Option<String>,
}

/// `run_memtest` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MemTestArgs {
    /// MiB of RAM to test, split across workers. Default 4096. Leave
    /// headroom for the OS — testing more than ~75% of free RAM forces
    /// paging and slows verification without improving coverage.
    #[serde(default)]
    pub memory_cap_mb: Option<u64>,
    /// Test duration in seconds. Default 600. Patterns cycle until time is up.
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// 0 = one worker per logical CPU (default).
    #[serde(default)]
    pub threads: Option<usize>,
}

/// Duration-only arguments (`run_cpu_stability`, `run_psu_test`).
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DurationOnlyArgs {
    /// Test duration in seconds. Defaults per tool.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

/// `run_linpack` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct LinpackArgs {
    /// Test duration in seconds. Default 120.
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// Matrix memory budget in MiB, split across workers (sets N). Default 1024.
    #[serde(default)]
    pub memory_cap_mb: Option<u64>,
}

/// `run_benchmark` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BenchArgs {
    /// Benchmark kind — see `run_benchmark` description for the list.
    pub kind: String,
    /// Measurement window in seconds (warmup discarded). Default 15.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

/// `run_benchmark_suite` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BenchSuiteArgs {
    /// Append the GPU benchmarks. Default: auto (true when a GPU is detected).
    #[serde(default)]
    pub include_gpu: Option<bool>,
    /// Measurement window per benchmark in seconds. Default 15.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

/// `get_benchmark_results` arguments.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BenchHistoryArgs {
    /// Filter to one benchmark kind (e.g. "cpu_multi"). Optional.
    #[serde(default)]
    pub kind: Option<String>,
    /// Max rows, newest first. Default 20.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Compare across every machine instead of just this one (requires `kind`).
    #[serde(default)]
    pub all_machines: Option<bool>,
}

/// Uniform report for the verified test tools (`run_memtest`,
/// `run_cpu_stability`, `run_linpack`, `run_psu_test`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct VerifiedTestReport {
    pub test: String,
    /// `stress_test_run:<id>` of the persisted run.
    pub run_id: String,
    /// "pass" | "fail" | "errored" | "inconclusive".
    pub verdict: String,
    /// Test-detected errors (mismatches / residual breaches). Any non-zero
    /// value is a hardware fault, not noise.
    pub errors: u32,
    /// New WHEA events observed during the run (absolute before/after).
    pub whea_delta: Option<u64>,
    /// Steady-state average throughput — the score for linpack (GFLOPS) and
    /// psu (combined GFLOPS).
    pub score: Option<f64>,
    pub peak: Option<f64>,
    pub unit: String,
    pub elapsed_secs: f64,
    pub max_temp_c: Option<f32>,
    pub avg_temp_c: Option<f32>,
    /// Max GPU board power observed (NVML), the PSU-load proxy.
    pub max_power_w: Option<u32>,
    pub last_error: Option<String>,
    pub finished_reason: String,
    pub reasoning: String,
    pub error: Option<String>,
}

/// Stringify a `RecordId` for the MCP wire surface. Uses the table-qualified
/// `table:key` form so a client can paste it straight into a SurrealQL
/// `SELECT * FROM <run_id>;` query.
fn format_run_id(id: &RecordId) -> String {
    use database::schema::RecordIdExt;
    format!("stress_test_run:{}", id.key_string())
}

fn new_run_id() -> String {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("{ms:x}-{n:x}")
}

/// Drive a single-stressor run through `RunController` so it persists to
/// `stress_test_run` / `stress_test_metric` / `stress_test_event`. Publishes
/// live progress into `RunSlot` for `get_run_status` and bridges
/// `stop_stress_run`'s `AtomicBool` to `RunController::stop()`.
fn drive_single_via_controller(
    spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    label: String,
    unit: String,
    slot: Arc<Mutex<RunSlot>>,
) -> StressorReport {
    let started = Instant::now();
    let mode = "stressor".to_string();
    let mut report = drive_run(spec, telemetry, label.clone(), Some(unit), mode, slot)
        .into_stressor_report(started.elapsed().as_secs_f64());
    report.label = label;
    report
}

/// Drive a multi-stage scenario through `RunController`. Same persistence
/// guarantees as `drive_single_via_controller`.
fn drive_scenario_via_controller(
    spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    labels: Vec<String>,
    slot: Arc<Mutex<RunSlot>>,
) -> ScenarioReport {
    let started = Instant::now();
    let mode = "scenario".to_string();
    let label = labels.first().cloned().unwrap_or_default();
    let outcome = drive_run(spec, telemetry, label, None, mode, slot);
    outcome.into_scenario_report(labels, started.elapsed().as_secs_f64())
}

/// Internal: shared driver for both single and scenario runs. Wraps the
/// `RunController` event loop, mirrors progress to `RunSlot`, and translates
/// the bridge cancel atomic into `controller.stop()` calls.
struct DriveOutcome {
    /// Server-side run id (`stress_test_run:<uuid>` formatted).
    run_id: String,
    throughput_unit: String,
    last_metrics: Option<MetricsDto>,
    finished_reason: String,
    /// First `RunUpdate::Error` or `Warning` message the worker emitted,
    /// stashed here so the MCP caller sees *why* a run that returned
    /// `controller_exited` (or even a normally-finished run) had a
    /// failure. None on clean runs.
    error: Option<String>,
    /// Per-stage final metrics keyed by stage index; index 0 is always
    /// populated even for single-stressor runs.
    per_stage_final: std::collections::HashMap<u32, MetricsDto>,
    /// Controller verdict captured from `RunUpdate::Finished`.
    verdict: Option<RunVerdictDto>,
}

impl DriveOutcome {
    fn into_stressor_report(self, elapsed_secs: f64) -> StressorReport {
        StressorReport {
            run_id: self.run_id,
            label: String::new(), // filled by drive_single_via_controller via state.label
            throughput_unit: self.throughput_unit,
            elapsed_secs,
            last_metrics: self.last_metrics,
            verdict: self.verdict,
            finished_reason: self.finished_reason,
            error: self.error,
        }
    }

    fn into_scenario_report(self, labels: Vec<String>, elapsed_secs: f64) -> ScenarioReport {
        let stages = labels
            .iter()
            .enumerate()
            .map(|(i, label)| StageReport {
                index: i,
                label: label.clone(),
                last_metrics: self.per_stage_final.get(&(i as u32)).cloned(),
            })
            .collect();
        let _ = elapsed_secs;
        ScenarioReport {
            run_id: self.run_id,
            finished_reason: self.finished_reason,
            total_elapsed_secs: self
                .last_metrics
                .as_ref()
                .map(|m| m.elapsed_secs)
                .unwrap_or(0.0),
            stages,
            verdict: self.verdict,
            error: self.error,
        }
    }
}

fn drive_run(
    spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    initial_label: String,
    initial_unit: Option<String>,
    mode: String,
    slot: Arc<Mutex<RunSlot>>,
) -> DriveOutcome {
    let controller = RunController::start(spec, telemetry);
    let cancel_bridge = Arc::new(AtomicBool::new(false));
    let pre_run_id = format!("pending-{}", new_run_id());

    // Publish initial slot state with a placeholder run_id; replaced once the
    // controller fires `Started`.
    if let Ok(mut s) = slot.lock() {
        s.cancel = Some(cancel_bridge.clone());
        s.latest = RunSnapshot {
            run_id: pre_run_id.clone(),
            mode: mode.clone(),
            label: initial_label.clone(),
            elapsed_secs: 0.0,
            throughput: 0.0,
            throughput_unit: initial_unit.clone().unwrap_or_default(),
            last_error: None,
            finished: false,
            finished_reason: None,
        };
    }

    let mut run_id: String = pre_run_id.clone();
    let mut last: Option<MetricsDto> = None;
    let mut throughput_unit = initial_unit.unwrap_or_default();
    let mut finished_reason = "duration_elapsed".to_string();
    let mut per_stage_final: std::collections::HashMap<u32, MetricsDto> =
        std::collections::HashMap::new();
    // Sticky first-error: an `Error` from the controller is fatal and the
    // worker exits right after; we want the MCP caller to see the message
    // even on the `controller_exited` path.
    let mut first_error: Option<String> = None;

    loop {
        // Bridge: if the MCP `stop_stress_run` flipped our atomic, ask the
        // controller to wind down. The controller will still emit `Finished`,
        // which is when we exit this loop.
        if cancel_bridge.load(Ordering::Relaxed) {
            controller.stop();
        }

        let updates = controller.poll();
        for update in updates {
            match update {
                RunUpdate::Started { run_id: rid } => {
                    run_id = format_run_id(&rid);
                    if let Ok(mut s) = slot.lock() {
                        s.latest.run_id = run_id.clone();
                    }
                }
                RunUpdate::StageStarted {
                    index,
                    label,
                    stage_count: _,
                } => {
                    if let Ok(mut s) = slot.lock() {
                        s.latest.label = label;
                        let _ = index;
                    }
                }
                RunUpdate::Tick {
                    stage_index,
                    stage_label: _,
                    metrics,
                    telemetry: _,
                    throughput_unit: unit,
                } => {
                    let dto: MetricsDto = (&metrics).into();
                    if let Some(idx) = stage_index {
                        per_stage_final.insert(idx, dto.clone());
                    } else {
                        per_stage_final.insert(0, dto.clone());
                    }
                    throughput_unit = unit.to_string();
                    if let Ok(mut s) = slot.lock() {
                        s.latest.elapsed_secs = metrics.elapsed_secs;
                        s.latest.throughput = metrics.throughput;
                        s.latest.last_error = metrics.last_error.clone();
                        s.latest.throughput_unit = throughput_unit.clone();
                    }
                    last = Some(dto);
                }
                RunUpdate::StageFinished { index: _ } => {}
                // Stage verdicts arrive aggregated on the final RunVerdict.
                RunUpdate::StageVerdict { .. } => {}
                RunUpdate::Warning { message } => {
                    log::warn!("[qc-mcp/run] warning: {message}");
                    if let Ok(mut s) = slot.lock() {
                        s.latest.last_error = Some(message.clone());
                    }
                    if first_error.is_none() {
                        first_error = Some(format!("warning: {message}"));
                    }
                }
                RunUpdate::Error { message } => {
                    log::error!("[qc-mcp/run] fatal: {message}");
                    if let Ok(mut s) = slot.lock() {
                        s.latest.last_error = Some(message.clone());
                    }
                    if first_error.is_none() {
                        first_error = Some(message);
                    }
                }
                RunUpdate::Finished(verdict) => {
                    finished_reason = format!("{:?}", verdict.finish_reason).to_lowercase();
                    if let Ok(mut s) = slot.lock() {
                        s.cancel = None;
                        s.latest.finished = true;
                        s.latest.finished_reason = Some(finished_reason.clone());
                        s.latest.run_id = format_run_id(&verdict.run_id);
                    }
                    return DriveOutcome {
                        run_id: format_run_id(&verdict.run_id),
                        throughput_unit,
                        last_metrics: last,
                        finished_reason,
                        error: first_error,
                        per_stage_final,
                        verdict: Some((&verdict).into()),
                    };
                }
            }
        }

        if !controller.is_running() {
            // Controller exited without emitting Finished. Most common cause:
            // the row create at the top of `worker()` failed and the worker
            // bailed via `RunUpdate::Error`. `first_error` will hold the
            // message; the MCP report surfaces it so the caller can act.
            if let Ok(mut s) = slot.lock() {
                s.cancel = None;
                s.latest.finished = true;
                s.latest.finished_reason = Some("controller_exited".into());
            }
            return DriveOutcome {
                run_id,
                throughput_unit,
                last_metrics: last,
                finished_reason: "controller_exited".into(),
                error: first_error,
                per_stage_final,
                verdict: None,
            };
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

impl QcToolProvider {
    pub fn new(state: Arc<QcMcpState>) -> Self {
        Self {
            router: Self::tool_router(),
            state,
        }
    }

    fn telemetry_or_err(&self) -> Result<Arc<TelemetryAgent>, ErrorData> {
        self.state
            .telemetry
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| to_internal("telemetry sampler not yet ready (first frame pending)"))
    }

    /// Shared driver for the verified test tools: WHEA-delta bracketing,
    /// controller-backed persistence, and a uniform verdict rubric
    /// (errored > fail on errors/WHEA/controller-fail > inconclusive > pass).
    #[allow(clippy::too_many_arguments)]
    async fn run_verified_single(
        &self,
        test: &str,
        stressor: Stressor,
        threads: usize,
        duration_secs: u64,
        memory_cap_mb: u64,
        preset: &str,
        tag: &str,
    ) -> Result<VerifiedTestReport, ErrorData> {
        let duration_secs = duration_secs.clamp(5, 24 * 3600);
        let telemetry = self.telemetry_or_err()?;
        let whea_before = telemetry.snapshot().whea.map(|w| w.total_retained);
        let computer = self.state.computer.clone();
        let slot = self.state.run_slot.clone();
        let label = stressor.label().to_string();
        let unit = stressor.throughput_unit().to_string();

        let mut spec = RunSpec::single_stresskit(computer, stressor, Some(duration_secs));
        spec.plan = RunPlan::Single {
            stressor,
            threads,
            duration_secs: Some(duration_secs),
            memory_cap_mb,
            disk_file_mb: 16,
        };
        spec.tool = TestTool::StressKit {
            stressor: stressor_to_db(stressor),
        };
        spec.preset_label = Some(preset.to_string());
        spec.tags = vec!["origin:mcp".into(), tag.to_string()];

        let tele_for_run = telemetry.clone();
        let label_for_run = label.clone();
        let unit_for_run = unit.clone();
        let report = tokio::task::spawn_blocking(move || {
            drive_single_via_controller(spec, tele_for_run, label_for_run, unit_for_run, slot)
        })
        .await
        .map_err(|e| to_internal(format!("{test} task join: {e}")))?;

        let whea_after = telemetry.snapshot().whea.map(|w| w.total_retained);
        let whea_delta = match (whea_before, whea_after) {
            (Some(b), Some(a)) => Some(a.saturating_sub(b)),
            _ => None,
        };

        let errors = report
            .verdict
            .as_ref()
            .map(|v| v.test_errors)
            .or_else(|| {
                report
                    .last_metrics
                    .as_ref()
                    .map(|m| m.errors.min(u32::MAX as u64) as u32)
            })
            .unwrap_or(0);
        let result = report
            .verdict
            .as_ref()
            .map(|v| v.result.clone())
            .unwrap_or_default();
        let completed = matches!(
            report.finished_reason.as_str(),
            "completed" | "total_time" | "timeout"
        );

        let verdict = if report.error.is_some() {
            "errored"
        } else if errors > 0 || whea_delta.unwrap_or(0) > 0 || result == "fail" {
            "fail"
        } else if !completed {
            "inconclusive"
        } else {
            "pass"
        };

        let reasoning = match verdict {
            "pass" => format!(
                "{label} ran {duration_secs}s with zero detected errors and no new WHEA events."
            ),
            "fail" => {
                let mut bits = Vec::new();
                if errors > 0 {
                    bits.push(format!(
                        "{errors} test-detected error(s) — treat as a hardware fault"
                    ));
                }
                if whea_delta.unwrap_or(0) > 0 {
                    bits.push(format!("{} new WHEA event(s)", whea_delta.unwrap_or(0)));
                }
                if bits.is_empty() {
                    bits.push(format!(
                        "controller verdict '{result}' ({})",
                        report
                            .verdict
                            .as_ref()
                            .map(|v| v.failure_kind.as_str())
                            .unwrap_or("unknown")
                    ));
                }
                bits.join("; ")
            }
            "errored" => {
                "Infrastructure error before/while running — not a hardware verdict. See `error`."
                    .into()
            }
            _ => "Run did not complete normally (cancelled). Re-run for a clean verdict.".into(),
        };

        Ok(VerifiedTestReport {
            test: test.to_string(),
            run_id: report.run_id,
            verdict: verdict.to_string(),
            errors,
            whea_delta,
            score: report.verdict.as_ref().and_then(|v| v.avg_throughput),
            peak: report.verdict.as_ref().and_then(|v| v.peak_throughput),
            unit,
            elapsed_secs: report.elapsed_secs,
            max_temp_c: report.verdict.as_ref().and_then(|v| v.max_temp_c),
            avg_temp_c: report.verdict.as_ref().and_then(|v| v.avg_temp_c),
            max_power_w: report.verdict.as_ref().and_then(|v| v.max_power_w),
            last_error: report.last_metrics.as_ref().and_then(|m| m.last_error.clone()),
            finished_reason: report.finished_reason,
            reasoning,
            error: report.error,
        })
    }
}

#[tool_handler]
impl ServerHandler for QcToolProvider {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_instructions(
            "QC tools. Telemetry: `get_hw_snapshot`, `get_extended_telemetry`, `get_gpu_telemetry`, \
             `get_temperatures`. \
             Stress (all persist to SurrealDB via stress_runner): `list_stressors`, `run_stressor`, \
             `run_stress_scenario`, `run_stress_concurrent` (run CPU+RAM+GPU lanes at the SAME time, \
             per-lane metrics), `run_qc_benchmark` (curated 8-stage burn-in with pass/fail verdict), \
             `run_gpu_probe`, `stop_stress_run`, `get_run_status`. \
             Verified tests with error detection: `run_memtest` (RAM pattern verify), \
             `run_cpu_stability` (duplicate-execution compare), `run_linpack` (LU + residual check), \
             `run_psu_test` (CPU+GPU combined max load), \
             `run_combined_test` (single fused CPU+RAM+GPU torture). \
             Benchmarks with persisted scores: `run_benchmark`, `run_benchmark_suite`, \
             `measure_memory_latency`, `get_benchmark_results` (score history / cross-machine). \
             Reporting: `get_last_report`, `send_report`.",
        )
        .with_server_info(Implementation::from_build_env())
        .with_protocol_version(ProtocolVersion::LATEST)
    }
}

fn to_internal<S: Into<String>>(msg: S) -> ErrorData {
    use rmcp::model::ErrorCode;
    ErrorData::new(ErrorCode::INTERNAL_ERROR, msg.into(), None)
}

/// Spawn TCP :9100 and HTTP :9101 MCP tasks. Call once at startup.
pub fn spawn_mcp_servers(state: Arc<QcMcpState>) {
    // TCP 9100
    {
        let state = state.clone();
        tokio::spawn(async move {
            use tokio::net::TcpListener;
            match TcpListener::bind("127.0.0.1:9100").await {
                Ok(listener) => {
                    log::info!("[qc-mcp] TCP listener on 127.0.0.1:9100");
                    loop {
                        match listener.accept().await {
                            Ok((stream, addr)) => {
                                log::info!("[qc-mcp] TCP connection from {addr}");
                                let provider = QcToolProvider::new(state.clone());
                                tokio::spawn(async move {
                                    match rmcp::serve_server(provider, stream).await {
                                        Ok(handle) => {
                                            if let Err(e) = handle.waiting().await {
                                                let s = e.to_string();
                                                if !s.contains("connection closed")
                                                    && !s.contains("Connection reset")
                                                    && !s.contains("broken pipe")
                                                {
                                                    log::warn!("[qc-mcp] {addr}: {e}");
                                                }
                                            }
                                        }
                                        Err(e) => log::warn!("[qc-mcp] serve {addr}: {e}"),
                                    }
                                });
                            }
                            Err(e) => log::warn!("[qc-mcp] accept error: {e}"),
                        }
                    }
                }
                Err(e) => log::error!("[qc-mcp] Cannot bind TCP 9100: {e}"),
            }
        });
    }

    // HTTP /mcp
    {
        let state = state.clone();
        tokio::spawn(async move {
            use rmcp::transport::streamable_http_server::{
                session::local::LocalSessionManager,
                StreamableHttpServerConfig, StreamableHttpService,
            };

            let state2 = state.clone();
            let service = StreamableHttpService::new(
                move || Ok(QcToolProvider::new(state2.clone())),
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default(),
            );

            let app = axum::Router::new().nest_service("/mcp", service);
            let addr = "0.0.0.0:9105";
            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    log::info!(
                        "[qc-mcp] HTTP MCP http://{addr}/mcp"
                    );
                    if let Err(e) = axum::serve(listener, app).await {
                        log::error!("[qc-mcp] HTTP server error: {e}");
                    }
                }
                Err(e) => log::error!("[qc-mcp] Cannot bind HTTP {addr}: {e}"),
            }
        });
    }
}
