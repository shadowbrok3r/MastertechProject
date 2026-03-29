//! MCP tool bridge for the Mastertech plugin system.
//!
//! Exposes a `PluginToolProvider` that aggregates MCP tools from all registered plugins
//! and provides management + authoring tools.
//!
//! ## Tools
//!
//! **Management:** `list_plugins`, `enable_plugin`, `disable_plugin`, `call_plugin_tool`
//!
//! **Authoring (WASM plugin lifecycle):**
//! - `plugin_source` — read or write Rust source for a plugin
//! - `plugin_compile` — compile source to a WASM artifact
//! - `plugin_deploy` — hot-swap a running plugin with a new artifact
//! - `plugin_rollback` — revert to the previous artifact
//! - `plugin_watch` — collect runtime behavior report over N frames
//!
//! - **TCP 9003** — raw MCP stream (`transport-async-rw`) for CLI/SDK clients.
//! - **HTTP 9004** — [Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports#streamable-http)
//!   at `http://127.0.0.1:9004/mcp` for Cursor and other HTTP MCP clients.
//!   (Pointing those clients at port 9003 fails: they send HTTP, not framed JSON-RPC bytes.)

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
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::PluginManager;

// ─── Artifact store ────────────────────────────────────────────────────────────

/// Stores compiled WASM artifacts and their previous versions for rollback.
struct ArtifactStore {
    current: HashMap<String, Vec<u8>>,
    previous: HashMap<String, Vec<u8>>,
}

impl ArtifactStore {
    fn new() -> Self {
        Self {
            current: HashMap::new(),
            previous: HashMap::new(),
        }
    }

    fn store(&mut self, plugin_id: &str, bytes: Vec<u8>) {
        if let Some(old) = self.current.remove(plugin_id) {
            self.previous.insert(plugin_id.to_string(), old);
        }
        self.current.insert(plugin_id.to_string(), bytes);
    }

    fn get_current(&self, plugin_id: &str) -> Option<&Vec<u8>> {
        self.current.get(plugin_id)
    }

    fn rollback(&mut self, plugin_id: &str) -> Option<Vec<u8>> {
        let prev = self.previous.remove(plugin_id)?;
        if let Some(cur) = self.current.remove(plugin_id) {
            self.previous.insert(plugin_id.to_string(), cur);
        }
        self.current.insert(plugin_id.to_string(), prev.clone());
        Some(prev)
    }
}

// ─── Plugin store directory ────────────────────────────────────────────────────

fn plugin_store_root() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local").join("share").join("mastertech").join("plugins")
    } else if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(appdata).join("Mastertech").join("plugins")
    } else {
        PathBuf::from(".mastertech").join("plugins")
    }
}

fn plugin_dir(plugin_id: &str) -> PathBuf {
    plugin_store_root().join(sanitize_id(plugin_id))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Standard Cargo.toml template for a WASM plugin crate.
fn plugin_cargo_toml(plugin_id: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
        name = sanitize_id(plugin_id),
    )
}

// ─── PluginToolProvider ────────────────────────────────────────────────────────

/// MCP server that exposes plugin management and plugin-provided tools.
#[derive(Clone)]
pub struct PluginToolProvider {
    tool_router: ToolRouter<Self>,
    manager: Arc<Mutex<PluginManager>>,
    artifacts: Arc<Mutex<ArtifactStore>>,
}

impl PluginToolProvider {
    pub fn new(manager: Arc<Mutex<PluginManager>>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            manager,
            artifacts: Arc::new(Mutex::new(ArtifactStore::new())),
        }
    }
}

// ─── Parameter types ───────────────────────────────────────────────────────────

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

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginSourceParams {
    #[schemars(description = "Plugin ID (e.g. 'com.mastertech.my-plugin')")]
    pub plugin_id: String,
    #[schemars(description = "If provided, writes this Rust source as the plugin's lib.rs. If omitted, reads the current source.")]
    pub source: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginCompileParams {
    #[schemars(description = "Plugin ID to compile")]
    pub plugin_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginDeployParams {
    #[schemars(description = "Plugin ID to deploy (must have been compiled first)")]
    pub plugin_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginRollbackParams {
    #[schemars(description = "Plugin ID to rollback to its previous artifact")]
    pub plugin_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginWatchParams {
    #[schemars(description = "Plugin ID to observe")]
    pub plugin_id: String,
    #[schemars(description = "Number of seconds to observe (default 5)")]
    pub duration_secs: Option<u64>,
}

// ─── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl PluginToolProvider {
    // ── Management tools ────────────────────────────────────────────────

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

    // ── Authoring tools ─────────────────────────────────────────────────

    #[tool(
        name = "plugin_source",
        description = "Read or write the Rust source for a WASM plugin. If 'source' is provided, writes it as src/lib.rs and creates the Cargo.toml scaffold. If omitted, reads the current source. Returns the source code."
    )]
    async fn plugin_source(
        &self,
        Parameters(p): Parameters<PluginSourceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = plugin_dir(&p.plugin_id);
        let src_dir = dir.join("src");
        let lib_rs = src_dir.join("lib.rs");
        let cargo_toml = dir.join("Cargo.toml");

        if let Some(source) = p.source {
            tokio::fs::create_dir_all(&src_dir)
                .await
                .map_err(|e| to_internal(format!("mkdir: {e}")))?;

            tokio::fs::write(&lib_rs, &source)
                .await
                .map_err(|e| to_internal(format!("write lib.rs: {e}")))?;

            if !cargo_toml.exists() {
                tokio::fs::write(&cargo_toml, plugin_cargo_toml(&p.plugin_id))
                    .await
                    .map_err(|e| to_internal(format!("write Cargo.toml: {e}")))?;
            }

            Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "action": "written",
                    "path": lib_rs.display().to_string(),
                    "bytes": source.len(),
                }),
            )
            .map_err(to_internal)?]))
        } else {
            let source = tokio::fs::read_to_string(&lib_rs)
                .await
                .map_err(|e| to_internal(format!("read lib.rs: {e}")))?;

            Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "source": source,
                }),
            )
            .map_err(to_internal)?]))
        }
    }

    #[tool(
        name = "plugin_compile",
        description = "Compile a WASM plugin from its source directory. Requires `wasm32-wasip1` (classic core module for wasmtime::Module). `wasm32-wasip2` emits components and will not load. Returns compiler output or artifact size."
    )]
    async fn plugin_compile(
        &self,
        Parameters(p): Parameters<PluginCompileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = plugin_dir(&p.plugin_id);
        let lib_rs = dir.join("src").join("lib.rs");

        if !lib_rs.exists() {
            return Err(to_internal(format!(
                "No source found for plugin '{}'. Use plugin_source to write source first.",
                p.plugin_id
            )));
        }

        let output = tokio::process::Command::new("cargo")
            .args([
                "build",
                "--target",
                "wasm32-wasip1",
                "--release",
                "--message-format=json",
            ])
            .current_dir(&dir)
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .await
            .map_err(|e| to_internal(format!("Failed to run cargo: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "success": false,
                    "stderr": stderr,
                    "stdout": stdout,
                }),
            )
            .map_err(to_internal)?]));
        }

        let crate_name = sanitize_id(&p.plugin_id);
        // Cargo names the cdylib `.wasm` with hyphens turned into underscores.
        let release_dir = dir.join("target").join("wasm32-wasip1").join("release");
        let primary = release_dir.join(format!("{}.wasm", crate_name.replace('-', "_")));
        let fallback = release_dir.join(format!("{crate_name}.wasm"));
        let (wasm_path, artifact_bytes) = if tokio::fs::try_exists(&primary).await.unwrap_or(false) {
            let bytes = tokio::fs::read(&primary)
                .await
                .map_err(|e| to_internal(format!("Read artifact: {e}")))?;
            (primary, bytes)
        } else {
            let bytes = tokio::fs::read(&fallback).await.map_err(|e| {
                to_internal(format!(
                    "Read artifact: {e} (tried {} and {})",
                    primary.display(),
                    fallback.display()
                ))
            })?;
            (fallback, bytes)
        };

        let size = artifact_bytes.len();

        self.artifacts
            .lock()
            .map_err(|e| to_internal(e.to_string()))?
            .store(&p.plugin_id, artifact_bytes);

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({
                "plugin_id": p.plugin_id,
                "success": true,
                "artifact_bytes": size,
                "wasm_path": wasm_path.display().to_string(),
            }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "plugin_deploy",
        description = "Deploy (hot-swap) a compiled WASM plugin. Unregisters the old instance and loads the new artifact. Requires the 'wasm-plugins' feature."
    )]
    async fn plugin_deploy(
        &self,
        Parameters(p): Parameters<PluginDeployParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let artifact = {
            let store = self
                .artifacts
                .lock()
                .map_err(|e| to_internal(e.to_string()))?;
            store
                .get_current(&p.plugin_id)
                .cloned()
                .ok_or_else(|| {
                    to_internal(format!(
                        "No artifact for '{}'. Run plugin_compile first.",
                        p.plugin_id
                    ))
                })?
        };

        let mut mgr = self
            .manager
            .lock()
            .map_err(|e| to_internal(e.to_string()))?;

        mgr.unregister(&p.plugin_id);

        #[cfg(feature = "wasm-plugins")]
        {
            mgr.load_wasm(artifact)
                .map_err(|e| to_internal(format!("WASM load failed: {e}")))?;

            Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "deployed": true,
                }),
            )
            .map_err(to_internal)?]))
        }

        #[cfg(not(feature = "wasm-plugins"))]
        {
            drop(mgr);
            let _ = artifact;
            Err(to_internal(
                "WASM plugin support not enabled. Rebuild with feature 'wasm-plugins'.",
            ))
        }
    }

    #[tool(
        name = "plugin_rollback",
        description = "Rollback a deployed WASM plugin to its previous artifact version."
    )]
    async fn plugin_rollback(
        &self,
        Parameters(p): Parameters<PluginRollbackParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let prev_artifact = self
            .artifacts
            .lock()
            .map_err(|e| to_internal(e.to_string()))?
            .rollback(&p.plugin_id)
            .ok_or_else(|| {
                to_internal(format!("No previous artifact for '{}'", p.plugin_id))
            })?;

        let mut mgr = self
            .manager
            .lock()
            .map_err(|e| to_internal(e.to_string()))?;

        mgr.unregister(&p.plugin_id);

        #[cfg(feature = "wasm-plugins")]
        {
            mgr.load_wasm(prev_artifact)
                .map_err(|e| to_internal(format!("Rollback load failed: {e}")))?;

            Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "rolled_back": true,
                }),
            )
            .map_err(to_internal)?]))
        }

        #[cfg(not(feature = "wasm-plugins"))]
        {
            drop(mgr);
            let _ = prev_artifact;
            Err(to_internal(
                "WASM plugin support not enabled. Rebuild with feature 'wasm-plugins'.",
            ))
        }
    }

    #[tool(
        name = "plugin_watch",
        description = "Observe a plugin's behavior for a specified duration. Returns timing stats and any events it emitted."
    )]
    async fn plugin_watch(
        &self,
        Parameters(p): Parameters<PluginWatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let duration = std::time::Duration::from_secs(p.duration_secs.unwrap_or(5));
        let start = std::time::Instant::now();

        let (info, broadcast_rx) = {
            let mgr = self
                .manager
                .lock()
                .map_err(|e| to_internal(e.to_string()))?;

            let info = mgr
                .list_plugins()
                .into_iter()
                .find(|info| info.id == p.plugin_id)
                .ok_or_else(|| to_internal(format!("Plugin '{}' not found", p.plugin_id)))?;

            let broadcast_rx = mgr.host().broadcast_rx.clone();
            (info, broadcast_rx)
        };

        let mut events_captured: Vec<String> = Vec::new();

        while start.elapsed() < duration {
            while let Ok(event) = broadcast_rx.try_recv() {
                events_captured.push(format!("{event:?}"));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let elapsed_ms = start.elapsed().as_millis();

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({
                "plugin_id": p.plugin_id,
                "observed_ms": elapsed_ms,
                "plugin_info": {
                    "name": info.name,
                    "version": info.version,
                    "enabled": info.enabled,
                    "tool_count": info.tool_count,
                },
                "broadcast_events_seen": events_captured.len(),
                "sample_events": events_captured.into_iter().take(20).collect::<Vec<_>>(),
            }),
        )
        .map_err(to_internal)?]))
    }
}

// ─── Server handler ────────────────────────────────────────────────────────────

const INSTRUCTIONS: &str = "Mastertech Plugin System MCP Server. \
Use list_plugins to see registered plugins. \
Use enable_plugin/disable_plugin to control plugin lifecycle. \
Use call_plugin_tool to invoke tools exposed by individual plugins. \
Use plugin_source → plugin_compile → plugin_deploy for the WASM authoring loop. \
Use plugin_rollback to revert a bad deploy. Use plugin_watch to observe behavior.";

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

// ─── TCP server ────────────────────────────────────────────────────────────────

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

/// Streamable HTTP MCP (MCP spec 2025-06-18 / Cursor “HTTP” transport).
///
/// Cursor and similar clients must use `http://127.0.0.1:9004/mcp`, **not** TCP 9003.
pub async fn run_plugin_mcp_server_http(manager: Arc<Mutex<PluginManager>>) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager,
        StreamableHttpServerConfig, StreamableHttpService,
    };

    let addr = "127.0.0.1:9004";
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let mgr = manager.clone();
    let service = StreamableHttpService::new(
        move || Ok(PluginToolProvider::new(mgr.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);

    log::info!(
        "Plugin MCP (Streamable HTTP) listening at http://{addr}/mcp — set Cursor MCP URL to this (not :9003 TCP)"
    );

    axum::serve(listener, router).await?;
    Ok(())
}
