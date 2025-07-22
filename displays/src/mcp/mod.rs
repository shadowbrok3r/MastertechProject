// Model Context Protocol implementation for computer diagnostics and AI tools

pub mod client;
pub mod server;
pub mod tools;
pub mod types;

pub use client::*;
pub use server::*;
pub use tools::*;
pub use types::*;

use anyhow::{Context, Result};
use rmcp::McpServer;
use tokio::sync::mpsc;

/// MCP service for handling AI-powered computer diagnostics
pub struct McpService {
    pub server: Option<McpServer>,
    pub client: Option<McpClient>,
    pub command_tx: mpsc::UnboundedSender<DiagnosticCommand>,
    pub response_rx: mpsc::UnboundedReceiver<DiagnosticResponse>,
}

impl Default for McpService {
    fn default() -> Self {
        let (command_tx, _command_rx) = mpsc::unbounded_channel();
        let (_response_tx, response_rx) = mpsc::unbounded_channel();
        
        Self {
            server: None,
            client: None,
            command_tx,
            response_rx,
        }
    }
}

impl McpService {
    /// Initialize MCP server with diagnostic tools
    pub async fn init_server(&mut self) -> Result<()> {
        let server = create_diagnostic_server().await
            .context("Failed to create MCP diagnostic server")?;
        
        self.server = Some(server);
        Ok(())
    }

    /// Initialize MCP client for external AI providers
    pub async fn init_client(&mut self, provider: LlmProvider) -> Result<()> {
        let client = McpClient::new(provider).await
            .context("Failed to create MCP client")?;
        
        self.client = Some(client);
        Ok(())
    }

    /// Execute a diagnostic command through MCP
    pub async fn execute_diagnostic(&self, command: DiagnosticCommand) -> Result<DiagnosticResponse> {
        if let Some(client) = &self.client {
            client.execute_diagnostic(command).await
        } else {
            Err(anyhow::anyhow!("MCP client not initialized"))
        }
    }
}