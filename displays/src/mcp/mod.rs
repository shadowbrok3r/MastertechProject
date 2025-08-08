// Model Context Protocol implementation for computer diagnostics and AI tools
pub mod tools;
pub mod types;
pub mod mcp;
pub mod openai_bridge;

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
use tokio::net::TcpListener;
pub use tools::{DiagnosticTools, DiagnosticTool};
pub use mcp::DiagnosticToolProvider;
pub use types::*;
pub use openai_bridge::*;

/// MCP service for handling AI-powered computer diagnostics
pub struct McpService {
    pub rmcp_provider: std::sync::Arc<DiagnosticToolProvider>, // rmcp tool provider
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub command_tx: tokio::sync::mpsc::UnboundedSender<DiagnosticCommand>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub response_rx: tokio::sync::mpsc::UnboundedReceiver<DiagnosticResponse>,
    /// OpenAI <-> MCP bridge session (initialized asynchronously on startup)
    pub openai_session: std::sync::Arc<tokio::sync::Mutex<Option<OpenAiMcpSession>>>,
}

impl Default for McpService {
    fn default() -> Self {
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    let (_response_tx, response_rx) = tokio::sync::mpsc::unbounded_channel();
        
        Self {
            rmcp_provider: std::sync::Arc::new(DiagnosticToolProvider::new()),
            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
            command_tx,
            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
            response_rx,
            openai_session: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }
}

impl McpService {
    /// Spawn an async task to connect the OpenAI bridge to the MCP TCP server and store the session.
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub fn spawn_openai_connect(&self, addr: &str, model: &str, system_prompt: Option<String>) {
        let addr = addr.to_string();
        let model = model.to_string();
        let session_slot = self.openai_session.clone();
        <crate::PlatformSpawner as crate::Spawner>::spawn(async move {
            match OpenAiMcpSession::connect(&addr, model, system_prompt).await {
                Ok(sess) => {
                    let mut guard = session_slot.lock().await;
                    *guard = Some(sess);
                    log::info!("OpenAI-MCP session connected to {}", addr);
                }
                Err(e) => log::error!("Failed to connect OpenAI-MCP session: {e:?}"),
            }
        });
    }
}

// --- TCP Server Function ---
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub fn run_mcp_server_tcp() -> anyhow::Result<()> {
    <crate::PlatformSpawner as crate::Spawner>::spawn(async move {
        let result: anyhow::Result<(), anyhow::Error> = async {
            let addr = "127.0.0.1:9002"; // The TCP address to listen on
            let listener = TcpListener::bind(addr).await?;
            log::info!("MCP Server listening on TCP {addr}");

            let tool_provider = DiagnosticToolProvider::new(); // Create the tool provider instance

            loop {
                let (stream, client_addr) = listener.accept().await?;
                log::info!("Accepted TCP connection from: {}", client_addr);
                log::info!("Serving client {}...", client_addr);
                match rmcp::serve_server(tool_provider.clone(), stream).await {
                    Ok(server_handle) => {
                        if let Err(e) = server_handle.waiting().await {
                            if !e.to_string().contains("connection closed")
                                && !e.to_string().contains("Connection reset by peer")
                                && !e.to_string().contains("broken pipe")
                            {
                                log::error!("Client {} error: {:?}", client_addr, e);
                            } else {
                                log::info!("Client {} disconnected.", client_addr);
                            }
                        }
                    }
                    Err(e) => log::error!("Failed to start serving client {client_addr}: {e:?}")
                }
            }
        }.await;

        log::warn!("mcp server run result: {result:?}");
    });
    Ok(())
}