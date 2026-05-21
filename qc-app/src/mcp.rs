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
use stress_kit::scenario::{
    FinishReason, ScenarioDefinition, ScenarioEvent, ScenarioRunner, ScenarioStage,
};
use stress_kit::telemetry::TelemetryAgent;
use stress_kit::{Metrics, StressConfig, StressSession, Stressor};

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
        description = "Run a multi-stage stress scenario and return per-stage final metrics. Blocks until the scenario completes or its wall cap fires."
    )]
    async fn run_stress_scenario(
        &self,
        Parameters(args): Parameters<RunScenarioArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // Build the stress-kit scenario definition off the DTO.
        let stages: Vec<ScenarioStage> = args
            .stages
            .iter()
            .map(|s| ScenarioStage {
                label: s.label.clone(),
                duration_secs: s.duration_secs.max(1),
                config: StressConfig {
                    stressor: s.stressor.into(),
                    threads: s.threads,
                    timeout: None,
                    memory_cap_mb: s.memory_cap_mb,
                    disk_file_mb: s.disk_file_mb,
                },
            })
            .collect();

        if stages.is_empty() {
            return Err(to_internal("stages cannot be empty"));
        }

        let labels: Vec<String> = stages.iter().map(|s| s.label.clone()).collect();
        let def = ScenarioDefinition {
            stages,
            total_wall_secs: args.total_wall_secs,
            repeat_until_total: args.repeat_until_total,
        };

        let slot = self.state.run_slot.clone();
        // Driving the runner is blocking. Hop to a blocking thread so we don't
        // stall the tokio runtime.
        let report = tokio::task::spawn_blocking(move || drive_scenario(def, labels, slot))
            .await
            .map_err(|e| to_internal(format!("scenario task join: {e}")))?;

        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "run_stressor",
        description = "Run a single stressor for a fixed duration and return final metrics. Blocks until the duration elapses or `stop_stress_run` cancels."
    )]
    async fn run_stressor(
        &self,
        Parameters(args): Parameters<RunStressorArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind = args.stressor;
        let duration = Duration::from_secs(args.duration_secs.max(1));
        let config = StressConfig {
            stressor: kind.into(),
            threads: args.threads,
            timeout: Some(duration),
            memory_cap_mb: args.memory_cap_mb,
            disk_file_mb: args.disk_file_mb,
        };
        let label: String = stress_kit::Stressor::from(kind).label().to_string();
        let unit: String = stress_kit::Stressor::from(kind).throughput_unit().to_string();
        let slot = self.state.run_slot.clone();
        let run_id = format!("stressor-{}", new_run_id());

        let report = tokio::task::spawn_blocking(move || {
            drive_single(config, label, unit, run_id, duration, slot)
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

fn drive_single(
    config: StressConfig,
    label: String,
    unit: String,
    run_id: String,
    duration: Duration,
    slot: Arc<Mutex<RunSlot>>,
) -> StressorReport {
    let session = StressSession::start(config);
    let cancel = std::sync::Arc::new(AtomicBool::new(false));

    // Publish initial slot state.
    if let Ok(mut s) = slot.lock() {
        s.cancel = Some(cancel.clone());
        s.latest = RunSnapshot {
            run_id: run_id.clone(),
            mode: "stressor".into(),
            label: label.clone(),
            elapsed_secs: 0.0,
            throughput: 0.0,
            throughput_unit: unit.clone(),
            last_error: None,
            finished: false,
            finished_reason: None,
        };
    }

    let started = Instant::now();
    let mut last: Option<MetricsDto> = None;
    let mut finished_reason = "duration_elapsed".to_string();

    loop {
        if cancel.load(Ordering::Relaxed) {
            session.stop();
            finished_reason = "cancelled".into();
            break;
        }
        if started.elapsed() >= duration {
            session.stop();
            break;
        }
        if let Some(m) = session.try_recv() {
            last = Some((&m).into());
            if let Ok(mut s) = slot.lock() {
                s.latest.elapsed_secs = m.elapsed_secs;
                s.latest.throughput = m.throughput;
                s.latest.last_error = m.last_error.clone();
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Final tick drain.
    if let Some(m) = session.try_recv() {
        last = Some((&m).into());
    }

    // Clear slot.
    if let Ok(mut s) = slot.lock() {
        s.cancel = None;
        s.latest.finished = true;
        s.latest.finished_reason = Some(finished_reason.clone());
        if let Some(m) = &last {
            s.latest.elapsed_secs = m.elapsed_secs;
            s.latest.throughput = m.throughput;
            s.latest.last_error = m.last_error.clone();
        }
    }

    StressorReport {
        run_id,
        label,
        throughput_unit: unit,
        elapsed_secs: started.elapsed().as_secs_f64(),
        last_metrics: last,
        finished_reason,
    }
}

fn drive_scenario(
    def: ScenarioDefinition,
    labels: Vec<String>,
    slot: Arc<Mutex<RunSlot>>,
) -> ScenarioReport {
    let runner = ScenarioRunner::start(def);
    let cancel = runner.cancel_handle();
    let run_id = format!("scenario-{}", new_run_id());

    // Publish initial slot state so `stop_stress_run` can find the cancel
    // handle and `get_run_status` can read live progress.
    if let Ok(mut s) = slot.lock() {
        s.cancel = Some(cancel.clone());
        s.latest = RunSnapshot {
            run_id: run_id.clone(),
            mode: "scenario".into(),
            label: labels.first().cloned().unwrap_or_default(),
            elapsed_secs: 0.0,
            throughput: 0.0,
            throughput_unit: String::new(),
            last_error: None,
            finished: false,
            finished_reason: None,
        };
    }

    let mut last_metrics: Vec<Option<MetricsDto>> = vec![None; labels.len().max(1)];
    let mut report: Option<ScenarioReport> = None;

    while report.is_none() {
        for event in runner.try_recv_all() {
            match event {
                ScenarioEvent::StageStarted { index, .. } => {
                    if let Ok(mut s) = slot.lock() {
                        if let Some(lbl) = labels.get(index) {
                            s.latest.label = lbl.clone();
                        }
                    }
                }
                ScenarioEvent::Tick { stage_index, metrics } => {
                    if let Some(stage) = last_metrics.get_mut(stage_index) {
                        *stage = Some((&metrics).into());
                    }
                    if let Ok(mut s) = slot.lock() {
                        s.latest.elapsed_secs = metrics.elapsed_secs;
                        s.latest.throughput = metrics.throughput;
                        s.latest.last_error = metrics.last_error.clone();
                    }
                }
                ScenarioEvent::Finished {
                    reason,
                    total_elapsed_secs: t,
                } => {
                    report = Some(ScenarioReport {
                        finished_reason: finish_reason_label(reason),
                        total_elapsed_secs: t,
                        stages: labels
                            .iter()
                            .enumerate()
                            .map(|(i, label)| StageReport {
                                index: i,
                                label: label.clone(),
                                last_metrics: last_metrics.get(i).cloned().flatten(),
                            })
                            .collect(),
                    });
                    break;
                }
                _ => {}
            }
        }
        if report.is_none() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    let report = report.expect("scenario loop must produce a report before exiting");

    // Clear slot so subsequent get_run_status returns null.
    if let Ok(mut s) = slot.lock() {
        s.cancel = None;
        s.latest.finished = true;
        s.latest.finished_reason = Some(report.finished_reason.clone());
    }

    report
}

fn finish_reason_label(reason: FinishReason) -> String {
    match reason {
        FinishReason::Completed => "completed".into(),
        FinishReason::Cancelled => "cancelled".into(),
        FinishReason::TotalTime => "total_time".into(),
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
            "QC tools: `get_hw_snapshot`, `get_last_report`, `send_report`, `get_extended_telemetry`, `run_stress_scenario`."
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
