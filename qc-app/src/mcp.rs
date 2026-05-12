//! Localhost MCP: JSON-RPC TCP `127.0.0.1:9100`, streamable HTTP `http://127.0.0.1:9101/mcp`.
//! No auth on loopback (same idea as `displays` MCP bridge).

use std::sync::{Arc, Mutex};

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

use crate::hw_sampler::CoreRow;
use crate::reporting::ReportSink;
use crate::telemetry::{HwSnapshot, QcReport};

pub struct QcMcpState {
    pub latest_cores: Arc<Mutex<Vec<CoreRow>>>,
    pub last_report: Arc<Mutex<Option<QcReport>>>,
    pub report_sink: Arc<Mutex<Option<ReportSink>>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct NoArgs {}

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
            "QC tools: `get_hw_snapshot`, `get_last_report`, `send_report`."
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
