//! Localhost MCP: JSON-RPC TCP `127.0.0.1:9100`, streamable HTTP `http://127.0.0.1:9101/mcp`.
//! No auth on loopback (same idea as `displays` MCP bridge).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::{
    handler::server::{wrapper::Parameters, tool::ToolRouter, ServerHandler},
    model::{
        CallToolResult, Content, ErrorData, Implementation, ProtocolVersion,
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
    // GPU stressors (wgpu — D3D12 on Windows / Vulkan on Linux / Metal on macOS).
    // Single shared device per run; thread_count is ignored. memory_cap_mb caps
    // the buffer for gpu_vram and gpu_pcie.
    Gpu,
    GpuMatmul,
    GpuVram,
    GpuPcie,
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
            StressorKind::Gpu => Stressor::Gpu,
            StressorKind::GpuMatmul => Stressor::GpuMatmul,
            StressorKind::GpuVram => Stressor::GpuVram,
            StressorKind::GpuPcie => Stressor::GpuPcie,
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScenarioReport {
    pub finished_reason: String,
    pub total_elapsed_secs: f64,
    pub stages: Vec<StageReport>,
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
}

impl From<&Metrics> for MetricsDto {
    fn from(m: &Metrics) -> Self {
        Self {
            elapsed_secs: m.elapsed_secs,
            throughput: m.throughput,
            last_error: m.last_error.clone(),
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
        Ok(CallToolResult::success(vec![Content::text(json)]))
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
        Ok(CallToolResult::success(vec![Content::text(json)]))
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
                return Ok(CallToolResult::success(vec![Content::text(
                    "Report queued for upload.".to_string(),
                )]));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(
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
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "run_stress_scenario",
        description = "Run a multi-stage stress scenario via stress_runner::RunController. Blocks until done; every tick lands in `stress_test_metric` + the final verdict in `stress_test_run`."
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
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "run_stressor",
        description = "Run a single stressor via stress_runner::RunController for `duration_secs` seconds. Persists to stress_test_run + stress_test_metric; blocks until done or cancelled."
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
        Ok(CallToolResult::success(vec![Content::text(json)]))
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
        Ok(CallToolResult::success(vec![Content::text(body)]))
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
        Ok(CallToolResult::success(vec![Content::text(body)]))
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
            .and_then(|a| a.snapshot().whea.map(|w| w.absolute_since_boot));

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
            .and_then(|a| a.snapshot().whea.map(|w| w.absolute_since_boot));
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
        Ok(CallToolResult::success(vec![Content::text(json)]))
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
                })
            })
            .collect();
        let json = serde_json::to_string_pretty(&rows)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
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

use crate::qc_benchmark::{qc_benchmark_stages, qc_floor_for, QC_BENCHMARK_PRESET};

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
    pub finished_reason: String,
    /// First fatal `Error` (or `Warning`) emitted by the underlying RunController,
    /// if any. `None` on clean runs. Present + `finished_reason: "controller_exited"`
    /// usually means the row create failed before the stressor even started.
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
}

impl DriveOutcome {
    fn into_stressor_report(self, elapsed_secs: f64) -> StressorReport {
        StressorReport {
            run_id: self.run_id,
            label: String::new(), // filled by drive_single_via_controller via state.label
            throughput_unit: self.throughput_unit,
            elapsed_secs,
            last_metrics: self.last_metrics,
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
            finished_reason: self.finished_reason,
            total_elapsed_secs: self
                .last_metrics
                .as_ref()
                .map(|m| m.elapsed_secs)
                .unwrap_or(0.0),
            stages,
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
}

#[tool_handler]
impl ServerHandler for QcToolProvider {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_instructions(
            "QC tools. Telemetry: `get_hw_snapshot`, `get_extended_telemetry`. \
             Stress (all persist to SurrealDB via stress_runner): `list_stressors`, `run_stressor`, \
             `run_stress_scenario`, `run_qc_benchmark` (curated 8-stage burn-in with pass/fail verdict), \
             `stop_stress_run`, `get_run_status`. Reporting: `get_last_report`, `send_report`.",
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
