//! MCP tool bridge for the Mastertech plugin system.
//!
//! Exposes a `PluginToolProvider` that aggregates MCP tools from all registered plugins
//! and provides management tools (list, enable, disable, call).
//!
//! Binds on TCP 9003, separate from the existing desktop (9001) and diagnostic (9002) servers.

use rmcp::{
    handler::server::{wrapper::Parameters, tool::ToolRouter, ServerHandler},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData, Implementation, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use super::PluginManager;

/// MCP server that exposes plugin management and plugin-provided tools.
#[derive(Clone)]
pub struct PluginToolProvider {
    tool_router: ToolRouter<Self>,
    manager: Arc<Mutex<PluginManager>>,
}

impl PluginToolProvider {
    pub fn new(manager: Arc<Mutex<PluginManager>>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            manager,
        }
    }
}

// ── Parameter types ────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ListPluginsParams {}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct EnablePluginParams {
    #[schemars(description = "Plugin ID to enable")]
    pub plugin_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct DisablePluginParams {
    #[schemars(description = "Plugin ID to disable")]
    pub plugin_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CallPluginToolParams {
    #[schemars(description = "Plugin ID that owns the tool")]
    pub plugin_id: String,
    #[schemars(description = "Tool name to call")]
    pub tool_name: String,
    #[schemars(description = "JSON arguments for the tool")]
    pub args: Option<serde_json::Value>,
}

// ── Tool implementations ───────────────────────────────────────────────────────

#[tool_router]
impl PluginToolProvider {
    #[tool(
        name = "list_plugins",
        description = "List all registered Mastertech plugins with their status, version, and tool count."
    )]
    async fn list_plugins(
        &self,
        Parameters(_p): Parameters<ListPluginsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mgr = self.manager.lock().map_err(|e| to_internal(e.to_string()))?;
        let plugins = mgr.list_plugins();
        Ok(CallToolResult::success(vec![
            Content::json(plugins).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "enable_plugin",
        description = "Enable a previously disabled plugin by its ID."
    )]
    async fn enable_plugin(
        &self,
        Parameters(p): Parameters<EnablePluginParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut mgr = self.manager.lock().map_err(|e| to_internal(e.to_string()))?;
        let ok = mgr.set_plugin_enabled(&p.plugin_id, true);
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "plugin_id": p.plugin_id, "enabled": ok }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "disable_plugin",
        description = "Disable a plugin by its ID. The plugin remains registered but stops receiving lifecycle calls."
    )]
    async fn disable_plugin(
        &self,
        Parameters(p): Parameters<DisablePluginParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut mgr = self.manager.lock().map_err(|e| to_internal(e.to_string()))?;
        let ok = mgr.set_plugin_enabled(&p.plugin_id, false);
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "plugin_id": p.plugin_id, "disabled": ok }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "call_plugin_tool",
        description = "Call an MCP tool registered by a specific plugin. Use list_plugins to discover available plugin tools."
    )]
    async fn call_plugin_tool(
        &self,
        Parameters(p): Parameters<CallPluginToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut mgr = self.manager.lock().map_err(|e| to_internal(e.to_string()))?;
        let args = p.args.unwrap_or(serde_json::Value::Null);
        let result = mgr
            .dispatch_mcp_call(&p.plugin_id, &p.tool_name, args)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![
            Content::json(result).map_err(to_internal)?
        ]))
    }
}

const INSTRUCTIONS: &str = "Mastertech Plugin System MCP Server. \
Use list_plugins to see registered plugins. \
Use enable_plugin/disable_plugin to control plugin lifecycle. \
Use call_plugin_tool to invoke tools exposed by individual plugins.";

#[tool_handler]
impl ServerHandler for PluginToolProvider {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_experimental()
                .build(),
        )
        .with_instructions(INSTRUCTIONS.to_string())
        .with_server_info(Implementation::from_build_env())
        .with_protocol_version(ProtocolVersion::LATEST)
    }
}

fn to_internal<E: std::fmt::Display>(e: E) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
}

// ── TCP server ─────────────────────────────────────────────────────────────────

/// Start the plugin MCP server on TCP port 9003.
pub async fn run_plugin_mcp_server(manager: Arc<Mutex<PluginManager>>) -> anyhow::Result<()> {
    use tokio::net::TcpListener;

    let addr = "127.0.0.1:9003";
    let listener = TcpListener::bind(addr).await?;
    log::info!("Plugin MCP Server listening on TCP {addr}");

    let provider = PluginToolProvider::new(manager);

    loop {
        let (stream, client_addr) = listener.accept().await?;
        log::info!("Plugin MCP: accepted connection from {client_addr}");
        match rmcp::serve_server(provider.clone(), stream).await {
            Ok(handle) => {
                if let Err(e) = handle.waiting().await {
                    let msg = e.to_string();
                    if !msg.contains("connection closed")
                        && !msg.contains("Connection reset")
                        && !msg.contains("broken pipe")
                    {
                        log::error!("Plugin MCP client {client_addr} error: {e:?}");
                    } else {
                        log::info!("Plugin MCP client {client_addr} disconnected.");
                    }
                }
            }
            Err(e) => log::error!("Plugin MCP: failed to serve {client_addr}: {e:?}"),
        }
    }
}
