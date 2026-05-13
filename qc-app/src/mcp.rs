//! Localhost MCP: JSON-RPC TCP `127.0.0.1:9100`, streamable HTTP `http://127.0.0.1:9101/mcp`.
//! No auth on loopback (same idea as `displays` MCP bridge).

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
use stress_kit::{Metrics, StressConfig, Stressor};

use crate::hw_sampler::CoreRow;
use crate::reporting::ReportSink;
use crate::telemetry::{HwSnapshot, QcReport};

pub struct QcMcpState {
    pub latest_cores: Arc<Mutex<Vec<CoreRow>>>,
    pub last_report: Arc<Mutex<Option<QcReport>>>,
    pub report_sink: Arc<Mutex<Option<ReportSink>>>,
    /// Shared telemetry agent. Held in `Option` so the state can be constructed
    /// before the sampler boots on the first frame.
    pub telemetry: Arc<Mutex<Option<Arc<TelemetryAgent>>>>,
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

        // Driving the runner is blocking. Hop to a blocking thread so we don't
        // stall the tokio runtime.
        let report = tokio::task::spawn_blocking(move || drive_scenario(def, labels))
            .await
            .map_err(|e| to_internal(format!("scenario task join: {e}")))?;

        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| to_internal(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

fn drive_scenario(def: ScenarioDefinition, labels: Vec<String>) -> ScenarioReport {
    let runner = ScenarioRunner::start(def);
    let mut last_metrics: Vec<Option<MetricsDto>> = vec![None; labels.len().max(1)];
    let mut finished_reason = FinishReason::Cancelled;
    let mut total_elapsed_secs = 0.0_f64;

    // Drain events until we see `Finished`.
    loop {
        for event in runner.try_recv_all() {
            match event {
                ScenarioEvent::Tick { stage_index, metrics } => {
                    if let Some(slot) = last_metrics.get_mut(stage_index) {
                        *slot = Some((&metrics).into());
                    }
                }
                ScenarioEvent::Finished { reason, total_elapsed_secs: t } => {
                    finished_reason = reason;
                    total_elapsed_secs = t;
                    return ScenarioReport {
                        finished_reason: finish_reason_label(finished_reason),
                        total_elapsed_secs,
                        stages: labels
                            .iter()
                            .enumerate()
                            .map(|(i, label)| StageReport {
                                index: i,
                                label: label.clone(),
                                last_metrics: last_metrics.get(i).cloned().flatten(),
                            })
                            .collect(),
                    };
                }
                _ => {}
            }
        }
        // No event yet — short sleep, then poll again.
        std::thread::sleep(Duration::from_millis(100));
        let _ = Instant::now(); // silence unused if Instant import is otherwise unused
    }
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

    // HTTP 9101 /mcp
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
            let addr = "127.0.0.1:9101";
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
