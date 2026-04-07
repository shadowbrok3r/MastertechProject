//! MCP tool bridge for the Mastertech plugin system.
//!
//! Exposes a `PluginToolProvider` that aggregates MCP tools from all registered plugins
//! and provides management + authoring tools.
//!
//! ## Tools
//!
//! **Management:** `list_plugins`, `enable_plugin`, `disable_plugin`, `call_plugin_tool`
//!
//! **Remote egui (admin Web Console):** `remote_egui_list_targets`, `remote_egui_get_last_frame_meta`,
//! `remote_egui_list_widget_anchors`, `remote_egui_click_anchor`,
//! `remote_egui_perform_steps` (batch: click, click_anchor, text, key_tap, move_pointer, scroll, sleep_ms),
//! `remote_egui_send_input`, `remote_egui_click`, `remote_egui_type` — inject
//! [`EguiInputEvent`](super::remote::EguiInputEvent) when an operator has an active Web Console
//! WebSocket session (same binary format as the Mastertech Viewer tab).
//!
//! **Authoring (WASM plugin lifecycle):**
//! - `plugin_source` — read or write Rust source for a plugin
//! - `plugin_compile` — compile source to a WASM artifact
//! - `plugin_deploy` — hot-swap a running plugin with a new artifact
//! - `plugin_rollback` — revert to the previous artifact
//! - `plugin_watch` — collect runtime behavior report over N frames
//! - `plugin_emit_clock_wasm` — build a **clock** guest via WAT + `wat` + **wasmtime** validation (no nested `cargo build`)
//! - `plugin_compile_wat` — turn arbitrary WAT into wasm bytes (validated with wasmtime)
//!
//! - **TCP 9003** — raw MCP stream (`transport-async-rw`) for CLI/SDK clients.
//! - **HTTP 9004** — [Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports#streamable-http)
//!   at `http://127.0.0.1:9004/mcp` for Cursor and other HTTP MCP clients.
//!   (Pointing those clients at port 9003 fails: they send HTTP, not framed JSON-RPC bytes.)
//!
//!   **Session lifecycle:** After `initialize`, the client must POST `notifications/initialized`
//!   on the same `Mcp-Session-Id` before `tools/call` or other requests. Skipping that leaves the
//!   session worker waiting and the POST appears to hang.
//!
//!   **PluginManager:** Wrapped in `Arc<RwLock<PluginManager>>`. The UI holds a **write** lock during
//!   plugin hooks. MCP tools that only read metadata use `try_read()` so they can run concurrently
//!   with each other and do not block behind the UI unless it is mutating plugins. Mutating tools use
//!   `try_write()` and fail fast if the UI holds the writer.
//!
//! **Server `instructions` field** (initialize response) lists every main **View** tab, typical use, and
//! `nav.tab.*` anchor slugs for `remote_egui_click_anchor` after opening **View** (`nav.menu.view`).

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
use std::sync::{Arc, Mutex, RwLock};

use super::PluginManager;

// ─── Remote plugin tool call response routing ───────────────────────────────────

use once_cell::sync::Lazy;
use tokio::sync::oneshot;

type PendingRequests = std::sync::Mutex<HashMap<String, oneshot::Sender<(bool, String)>>>;
static REMOTE_TOOL_PENDING: Lazy<PendingRequests> = Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

/// Register a pending request and return a receiver that resolves when the remote client replies.
fn register_pending_request(request_id: String) -> oneshot::Receiver<(bool, String)> {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut map) = REMOTE_TOOL_PENDING.lock() {
        map.insert(request_id, tx);
    }
    rx
}

/// Called by the admin console's receive handler when a `RemotePluginToolResult` arrives.
pub fn resolve_pending_request(request_id: &str, success: bool, result_json: String) {
    if let Ok(mut map) = REMOTE_TOOL_PENDING.lock() {
        if let Some(tx) = map.remove(request_id) {
            let _ = tx.send((success, result_json));
        }
    }
}

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
    manager: Arc<RwLock<PluginManager>>,
    artifacts: Arc<Mutex<ArtifactStore>>,
}

impl PluginToolProvider {
    pub fn new(manager: Arc<RwLock<PluginManager>>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            manager,
            artifacts: Arc::new(Mutex::new(ArtifactStore::new())),
        }
    }

    fn try_read_manager(&self) -> Result<std::sync::RwLockReadGuard<'_, PluginManager>, ErrorData> {
        match self.manager.try_read() {
            Ok(guard) => Ok(guard),
            Err(std::sync::TryLockError::Poisoned(e)) => Err(to_internal(e.to_string())),
            Err(std::sync::TryLockError::WouldBlock) => Err(to_internal(
                "PluginManager is locked for writing by the Mastertech UI (egui). Retry shortly.",
            )),
        }
    }

    fn try_write_manager(&self) -> Result<std::sync::RwLockWriteGuard<'_, PluginManager>, ErrorData> {
        match self.manager.try_write() {
            Ok(guard) => Ok(guard),
            Err(std::sync::TryLockError::Poisoned(e)) => Err(to_internal(e.to_string())),
            Err(std::sync::TryLockError::WouldBlock) => Err(to_internal(
                "PluginManager is locked by the Mastertech UI or another MCP tool. Retry shortly.",
            )),
        }
    }

    fn try_lock_artifacts(&self) -> Result<std::sync::MutexGuard<'_, ArtifactStore>, ErrorData> {
        match self.artifacts.try_lock() {
            Ok(guard) => Ok(guard),
            Err(std::sync::TryLockError::Poisoned(e)) => Err(to_internal(e.to_string())),
            Err(std::sync::TryLockError::WouldBlock) => Err(to_internal(
                "Plugin artifact store is busy (another MCP tool is using it). Retry shortly.",
            )),
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

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginEmitClockParams {
    #[schemars(description = "Plugin ID (e.g. com.mastertech.wasm-clock)")]
    pub plugin_id: String,
    #[schemars(description = "Display name shown in list_plugins (default: Mastertech Clock)")]
    pub display_name: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginCompileWatParams {
    #[schemars(description = "Plugin ID for artifact store / plugin directory")]
    pub plugin_id: String,
    #[schemars(description = "Full WebAssembly text (WAT) module source")]
    pub wat_source: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginDeployRemoteParams {
    #[schemars(description = "Plugin ID whose compiled artifact will be deployed")]
    pub plugin_id: String,
    #[schemars(description = "Web Console connection_string of the remote client to deploy to")]
    pub connection_string: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CallRemotePluginToolParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Plugin ID on the remote client (e.g. 'com.mastertech.status-reporter')")]
    pub plugin_id: String,
    #[schemars(description = "Tool name exposed by the remote plugin")]
    pub tool_name: String,
    #[schemars(description = "JSON arguments for the tool (default: {})")]
    pub args: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiListTargetsParams {}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiSendInputParams {
    #[schemars(description = "Web Console room id — same as ConnectedClient.connection_string; admin must be connected.")]
    pub connection_string: String,
    #[schemars(description = "One EguiInputEvent as JSON (serde externally-tagged enum), e.g. {\"PointerMoved\":{\"x\":100.0,\"y\":200.0}}, {\"PointerButton\":{\"x\":100.0,\"y\":200.0,\"button\":0,\"pressed\":true}}, \"PointerLeave\", {\"Key\":{\"key_name\":\"Enter\",\"pressed\":true,\"modifiers\":{\"alt\":false,\"ctrl\":false,\"shift\":false,\"command\":false}}}, {\"Text\":\"hello\"}, {\"Scroll\":{\"delta_x\":0.0,\"delta_y\":1.0}}")]
    pub event: serde_json::Value,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiGetLastFrameMetaParams {
    #[schemars(description = "Web Console room id; metadata is updated when egui frames arrive on the admin socket.")]
    pub connection_string: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiClickParams {
    #[schemars(description = "Web Console room id.")]
    pub connection_string: String,
    pub x: f32,
    pub y: f32,
    #[schemars(description = "Mouse button: 0=primary, 1=secondary, 2=middle")]
    #[serde(default)]
    pub button: u8,
    #[schemars(description = "If true (default), enqueue PointerMoved before press/release.")]
    #[serde(default = "default_remote_egui_true")]
    pub hover_first: bool,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiTypeParams {
    #[schemars(description = "Web Console room id.")]
    pub connection_string: String,
    #[schemars(description = "Unicode text to inject as egui Text events (focused widget receives input).")]
    pub text: String,
}

/// One step for [`remote_egui_perform_steps`]. Use tag `"step"` (snake_case values).
#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum RemoteEguiStep {
    /// Primary click: optional hover PointerMoved, then press + release.
    Click {
        x: f32,
        y: f32,
        #[serde(default)]
        #[schemars(description = "0=primary, 1=secondary, 2=middle")]
        button: u8,
        #[serde(default = "default_remote_egui_true")]
        hover_first: bool,
    },
    /// Single egui `Text` event (focused widget).
    Text {
        value: String,
    },
    /// Key down + up (e.g. key Tab, Enter, A).
    KeyTap {
        key: String,
        #[serde(default)]
        modifiers: super::remote::EguiModifiers,
    },
    PointerMoved {
        x: f32,
        y: f32,
    },
    Scroll {
        delta_x: f32,
        delta_y: f32,
    },
    /// Pause between steps so the remote UI can process input (focus changes, popups).
    SleepMs {
        ms: u64,
    },
    /// Click using a widget key from [`remote_egui_list_widget_anchors`] (host must register anchors).
    ClickAnchor {
        key: String,
        #[serde(default)]
        button: u8,
        #[serde(default = "default_remote_egui_true")]
        hover_first: bool,
        #[serde(default = "default_anchor_placement")]
        placement: String,
    },
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiPerformStepsParams {
    #[schemars(description = "Web Console room id.")]
    pub connection_string: String,
    #[schemars(description = "Ordered steps. Prefer this over many separate tool calls when filling forms (no View-menu toggles). Example: click service field, text, key_tap Tab, text, …")]
    pub steps: Vec<RemoteEguiStep>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiListWidgetAnchorsParams {
    #[schemars(description = "Web Console room id.")]
    pub connection_string: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiClickAnchorParams {
    #[schemars(description = "Web Console room id.")]
    pub connection_string: String,
    #[schemars(description = "Exact key from remote_egui_list_widget_anchors (e.g. tur.service_number).")]
    pub key: String,
    #[schemars(description = "center (default) or top_left")]
    #[serde(default = "default_anchor_placement")]
    pub placement: String,
}

// ─── Shared param helpers ────────────────────────────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema, Clone)]
pub struct PluginUsageRefParam {
    pub plugin_id: String,
    pub tool_name: String,
}

// ─── Plugin Registry parameter types ────────────────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct SearchPluginsParams {
    #[schemars(description = "Keyword to search across plugin names, descriptions, tool names, and IDs")]
    pub query: String,
    #[schemars(description = "Optional tag filter — only return plugins that have at least one of these tags")]
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct GetPluginInfoParams {
    #[schemars(description = "Plugin ID to look up (e.g. 'com.mastertech.hw-diag')")]
    pub plugin_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PublishPluginParams {
    #[schemars(description = "Plugin ID to publish (must have been compiled first)")]
    pub plugin_id: String,
    #[schemars(description = "Human-readable description of what this plugin does")]
    pub description: String,
    #[schemars(description = "Tags for searchability (e.g. ['diagnostics', 'gpu', 'windows'])")]
    pub tags: Option<Vec<String>>,
    #[schemars(description = "Whether to store the Rust source code alongside the WASM binary (default: true)")]
    pub store_source: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct FetchPluginParams {
    #[schemars(description = "Plugin ID to fetch from the SurrealDB registry")]
    pub plugin_id: String,
    #[schemars(description = "Specific version to fetch (default: latest)")]
    pub version: Option<String>,
}

// ─── Diagnostic Knowledge Base parameter types ──────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CreateDiagnosticSessionParams {
    #[schemars(description = "Web Console connection_string of the client being diagnosed")]
    pub connection_string: String,
    #[schemars(description = "Hostname of the machine being diagnosed")]
    pub hostname: String,
    #[schemars(description = "Customer name (if known)")]
    pub customer_name: Option<String>,
    #[schemars(description = "Technician performing the diagnosis")]
    pub tech: Option<String>,
    #[schemars(description = "Initial tags for categorizing this session")]
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct LogDiagnosticEntryParams {
    #[schemars(description = "Session ID returned by create_diagnostic_session")]
    pub session_id: String,
    #[schemars(description = "Category: finding, action, resolution, or note")]
    pub category: String,
    #[schemars(description = "Short title for this entry")]
    pub title: String,
    #[schemars(description = "Detailed description of the finding/action/resolution")]
    pub detail: String,
    #[schemars(description = "Optional structured data (JSON) — e.g. event logs, command output")]
    pub data: Option<serde_json::Value>,
    #[schemars(description = "Plugins used for this entry, e.g. [{\"plugin_id\": \"com.mastertech.hw-diag\", \"tool_name\": \"whea_errors\"}]")]
    pub plugins_used: Option<Vec<PluginUsageRefParam>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CloseDiagnosticSessionParams {
    #[schemars(description = "Session ID to close")]
    pub session_id: String,
    #[schemars(description = "Final status: resolved, escalated, or open")]
    pub status: String,
    #[schemars(description = "AI-written summary of findings, actions taken, and outcome")]
    pub summary: String,
    #[schemars(description = "Final tags to apply (appends to/replaces existing tags)")]
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct SearchDiagnosticsParams {
    #[schemars(description = "Free-text search across session summaries, hostnames, customer names, and tags")]
    pub query: String,
    #[schemars(description = "Filter by exact hostname")]
    pub hostname: Option<String>,
    #[schemars(description = "Filter by customer name (fuzzy)")]
    pub customer_name: Option<String>,
    #[schemars(description = "Filter by exact connection_string")]
    pub connection_string: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct GetDiagnosticSessionParams {
    #[schemars(description = "Session ID to retrieve (with all entries)")]
    pub session_id: String,
}

// ─── Customer / Service data parameter types ────────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct SearchCustomersParams {
    #[schemars(description = "Search by name, email, or phone number")]
    pub query: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct GetCustomerDetailsParams {
    #[schemars(description = "Customer record ID (e.g. 'customer:abc123' or just the key 'abc123')")]
    pub customer_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct GetServiceOrderParams {
    #[schemars(description = "Service number to look up (e.g. 'SO-12345')")]
    pub service_number: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct SearchServiceOrdersParams {
    #[schemars(description = "Search by customer name, service number, or checkin notes")]
    pub query: String,
    #[schemars(description = "Filter by technician name")]
    pub tech: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct GetComputerDetailsParams {
    #[schemars(description = "Computer record ID (e.g. 'computer:abc123' or just the key)")]
    pub computer_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct SearchPrestashopOrdersParams {
    #[schemars(description = "Customer email, customer name, or order reference to search for")]
    pub query: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct SearchOdooInventoryParams {
    #[schemars(description = "Product code or name to search for")]
    pub query: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct QuerySurrealDbParams {
    #[schemars(description = "Read-only SurrealQL query. Must start with SELECT or RETURN.")]
    pub query: String,
}

fn default_remote_egui_true() -> bool {
    true
}

fn default_anchor_placement() -> String {
    "center".to_string()
}

fn resolve_widget_anchor<'a>(
    anchors: &'a [super::remote::WidgetAnchor],
    key: &str,
) -> Option<&'a super::remote::WidgetAnchor> {
    anchors.iter().find(|a| a.key == key)
}

fn remote_egui_point_for_anchor(
    a: &super::remote::WidgetAnchor,
    placement: &str,
) -> (f32, f32) {
    match placement {
        "top_left" => a.top_left(),
        _ => a.center(),
    }
}

fn remote_egui_apply_step(
    hub: &super::remote_egui_control::RemoteEguiControlHub,
    connection_string: &str,
    step: &RemoteEguiStep,
) -> Result<usize, String> {
    use super::remote::EguiInputEvent;
    match step {
        RemoteEguiStep::SleepMs { .. } => Ok(0),
        RemoteEguiStep::Click {
            x,
            y,
            button,
            hover_first,
        } => {
            let mut seq = Vec::with_capacity(3);
            if *hover_first {
                seq.push(EguiInputEvent::PointerMoved { x: *x, y: *y });
            }
            seq.push(EguiInputEvent::PointerButton {
                x: *x,
                y: *y,
                button: *button,
                pressed: true,
            });
            seq.push(EguiInputEvent::PointerButton {
                x: *x,
                y: *y,
                button: *button,
                pressed: false,
            });
            let n = seq.len();
            hub.send_events(connection_string, &seq)?;
            Ok(n)
        }
        RemoteEguiStep::Text { value } => {
            hub.send_event(connection_string, EguiInputEvent::Text(value.clone()))?;
            Ok(1)
        }
        RemoteEguiStep::KeyTap { key, modifiers } => {
            let k = key.clone();
            let m = modifiers.clone();
            hub.send_event(
                connection_string,
                EguiInputEvent::Key {
                    key_name: k.clone(),
                    pressed: true,
                    modifiers: m.clone(),
                },
            )?;
            hub.send_event(
                connection_string,
                EguiInputEvent::Key {
                    key_name: k,
                    pressed: false,
                    modifiers: m,
                },
            )?;
            Ok(2)
        }
        RemoteEguiStep::PointerMoved { x, y } => {
            hub.send_event(
                connection_string,
                EguiInputEvent::PointerMoved { x: *x, y: *y },
            )?;
            Ok(1)
        }
        RemoteEguiStep::Scroll { delta_x, delta_y } => {
            hub.send_event(
                connection_string,
                EguiInputEvent::Scroll {
                    delta_x: *delta_x,
                    delta_y: *delta_y,
                },
            )?;
            Ok(1)
        }
        RemoteEguiStep::ClickAnchor {
            key,
            button,
            hover_first,
            placement,
        } => {
            let anchors = hub.get_last_widget_anchors(connection_string);
            let a = resolve_widget_anchor(&anchors, key).ok_or_else(|| {
                format!(
                    "unknown anchor key {key:?}; call remote_egui_list_widget_anchors (remote UI must expose anchors)"
                )
            })?;
            let (x, y) = remote_egui_point_for_anchor(a, placement.as_str());
            let mut seq = Vec::with_capacity(3);
            if *hover_first {
                seq.push(EguiInputEvent::PointerMoved { x, y });
            }
            seq.push(EguiInputEvent::PointerButton {
                x,
                y,
                button: *button,
                pressed: true,
            });
            seq.push(EguiInputEvent::PointerButton {
                x,
                y,
                button: *button,
                pressed: false,
            });
            let n = seq.len();
            hub.send_events(connection_string, &seq)?;
            Ok(n)
        }
    }
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
        let mgr = self.try_read_manager()?;
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
        let mut mgr = self.try_write_manager()?;
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
        let mut mgr = self.try_write_manager()?;
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
        let mut mgr = self.try_write_manager()?;
        let args = p.args.unwrap_or(serde_json::Value::Null);
        let result = mgr
            .dispatch_mcp_call(&p.plugin_id, &p.tool_name, args)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![
            Content::json(result).map_err(to_internal)?
        ]))
    }

    // ── Remote egui (MCP → Web Console WebSocket) ───────────────────────────────

    #[tool(
        name = "remote_egui_list_targets",
        description = "List connection_string values for remote clients that currently have an active admin Web Console WebSocket session. MCP can only inject remote egui input for these targets."
    )]
    async fn remote_egui_list_targets(
        &self,
        Parameters(_p): Parameters<RemoteEguiListTargetsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let targets = super::remote_egui_control::hub().list_targets();
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "targets": targets,
            "note": "Connect from Web Console first. Use remote_egui_list_widget_anchors + click_anchor when the remote app registers anchors; else perform_steps.",
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remote_egui_get_last_frame_meta",
        description = "Return the latest remote egui frame metadata (width, height, pixels_per_point, screen origin) for a connected client. Updated when frames stream in; use before choosing click coordinates."
    )]
    async fn remote_egui_get_last_frame_meta(
        &self,
        Parameters(p): Parameters<RemoteEguiGetLastFrameMetaParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = super::remote_egui_control::hub();
        if let Some(meta) = hub.get_last_frame_meta(&p.connection_string) {
            Ok(CallToolResult::success(vec![
                Content::json(meta).map_err(to_internal)?,
            ]))
        } else {
            Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
                "ok": false,
                "connection_string": p.connection_string,
                "detail": "No egui frame recorded yet. Open Mastertech Viewer for this client and wait until the remote UI appears.",
            }))
            .map_err(to_internal)?]))
        }
    }

    #[tool(
        name = "remote_egui_send_input",
        description = "Send one remote egui input event to the connected Mastertech client (bincode + EGUI_INPUT_TAG). Coordinates are in remote screen space from the captured frame."
    )]
    async fn remote_egui_send_input(
        &self,
        Parameters(p): Parameters<RemoteEguiSendInputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let event: super::remote::EguiInputEvent =
            serde_json::from_value(p.event).map_err(|e| {
                to_internal(format!("invalid event JSON (expect EguiInputEvent): {e}"))
            })?;
        super::remote_egui_control::hub()
            .send_event(&p.connection_string, event)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "ok": true,
            "connection_string": p.connection_string,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remote_egui_click",
        description = "Primary/secondary/middle click at (x,y) in remote screen space: optional PointerMoved, then button press and release."
    )]
    async fn remote_egui_click(
        &self,
        Parameters(p): Parameters<RemoteEguiClickParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut seq: Vec<super::remote::EguiInputEvent> = Vec::with_capacity(3);
        if p.hover_first {
            seq.push(super::remote::EguiInputEvent::PointerMoved { x: p.x, y: p.y });
        }
        seq.push(super::remote::EguiInputEvent::PointerButton {
            x: p.x,
            y: p.y,
            button: p.button,
            pressed: true,
        });
        seq.push(super::remote::EguiInputEvent::PointerButton {
            x: p.x,
            y: p.y,
            button: p.button,
            pressed: false,
        });
        let n = seq.len();
        super::remote_egui_control::hub()
            .send_events(&p.connection_string, &seq)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "ok": true,
            "connection_string": p.connection_string,
            "events_enqueued": n,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remote_egui_type",
        description = "Inject text as a single egui Text event (same as typing when a text field is focused on the remote UI)."
    )]
    async fn remote_egui_type(
        &self,
        Parameters(p): Parameters<RemoteEguiTypeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::remote_egui_control::hub()
            .send_event(
                &p.connection_string,
                super::remote::EguiInputEvent::Text(p.text.clone()),
            )
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "ok": true,
            "connection_string": p.connection_string,
            "chars": p.text.chars().count(),
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remote_egui_perform_steps",
        description = "Run multiple remote egui actions in order in one call: click, text, key_tap (key down+up), pointer_moved, scroll, sleep_ms. Use when a tab is already open—avoid re-clicking View menu entries (they toggle tabs off)."
    )]
    async fn remote_egui_perform_steps(
        &self,
        Parameters(p): Parameters<RemoteEguiPerformStepsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = super::remote_egui_control::hub();
        let mut events_enqueued = 0usize;
        for step in &p.steps {
            match step {
                RemoteEguiStep::SleepMs { ms } => {
                    tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                }
                other => {
                    events_enqueued +=
                        remote_egui_apply_step(&hub, &p.connection_string, other).map_err(to_internal)?;
                }
            }
        }
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "ok": true,
            "connection_string": p.connection_string,
            "steps_run": p.steps.len(),
            "events_enqueued": events_enqueued,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remote_egui_list_widget_anchors",
        description = "List widget rectangles (keys + bounds in host screen space) from the last remote frame. Keys are registered by the remote app (e.g. TUR: tur.service_number). Use remote_egui_click_anchor or perform_steps ClickAnchor to hit them without guessing coordinates."
    )]
    async fn remote_egui_list_widget_anchors(
        &self,
        Parameters(p): Parameters<RemoteEguiListWidgetAnchorsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let anchors = super::remote_egui_control::hub().get_last_widget_anchors(&p.connection_string);
        let listed: Vec<serde_json::Value> = anchors
            .iter()
            .map(|a| {
                let (cx, cy) = a.center();
                serde_json::json!({
                    "key": a.key,
                    "min_x": a.min_x,
                    "min_y": a.min_y,
                    "max_x": a.max_x,
                    "max_y": a.max_y,
                    "center_x": cx,
                    "center_y": cy,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "connection_string": p.connection_string,
            "anchors": listed,
            "count": listed.len(),
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remote_egui_click_anchor",
        description = "Primary click at the center (or top_left) of a registered widget anchor. Requires a recent frame that included anchors; refresh by having the remote UI visible."
    )]
    async fn remote_egui_click_anchor(
        &self,
        Parameters(p): Parameters<RemoteEguiClickAnchorParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = super::remote_egui_control::hub();
        let anchors = hub.get_last_widget_anchors(&p.connection_string);
        let a = resolve_widget_anchor(&anchors, &p.key).ok_or_else(|| {
            to_internal(format!(
                "unknown anchor key {:?}; use remote_egui_list_widget_anchors",
                p.key
            ))
        })?;
        let (x, y) = remote_egui_point_for_anchor(a, p.placement.as_str());
        let seq = vec![
            super::remote::EguiInputEvent::PointerMoved { x, y },
            super::remote::EguiInputEvent::PointerButton {
                x,
                y,
                button: 0,
                pressed: true,
            },
            super::remote::EguiInputEvent::PointerButton {
                x,
                y,
                button: 0,
                pressed: false,
            },
        ];
        hub.send_events(&p.connection_string, &seq)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "ok": true,
            "connection_string": p.connection_string,
            "key": p.key,
            "x": x,
            "y": y,
            "events_enqueued": seq.len(),
        }))
        .map_err(to_internal)?]))
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

        self.try_lock_artifacts()?
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
            let store = self.try_lock_artifacts()?;
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

        let mut mgr = self.try_write_manager()?;

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
            .try_lock_artifacts()?
            .rollback(&p.plugin_id)
            .ok_or_else(|| {
                to_internal(format!("No previous artifact for '{}'", p.plugin_id))
            })?;

        let mut mgr = self.try_write_manager()?;

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
            let mgr = self.try_read_manager()?;

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

    #[tool(
        name = "plugin_emit_clock_wasm",
        description = "Generate a WASM plugin (WAT → wasm via wat crate) that exposes MCP tool `current_time` with real UTC from the host (`host_fill_clock_json`). Validated with wasmtime::Module::new. Stores artifact for plugin_deploy — no cargo build in the plugin folder."
    )]
    async fn plugin_emit_clock_wasm(
        &self,
        Parameters(p): Parameters<PluginEmitClockParams>,
    ) -> Result<CallToolResult, ErrorData> {
        #[cfg(not(feature = "wasm-plugins"))]
        return Err(to_internal(
            "plugin_emit_clock_wasm requires displays built with feature wasm-plugins.",
        ));

        #[cfg(feature = "wasm-plugins")]
        {
            let display = p
                .display_name
                .clone()
                .unwrap_or_else(|| "Mastertech Clock".to_string());
            let wat = super::plugin_wasm_factory::clock_plugin_wat(&p.plugin_id, &display);
            let wasm = super::plugin_wasm_factory::wat_to_wasm_validated(&wat)
                .map_err(to_internal)?;
            let dir = plugin_dir(&p.plugin_id);
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| to_internal(format!("mkdir: {e}")))?;
            tokio::fs::write(dir.join("clock_pluginEmitted.wat"), wat.as_bytes())
                .await
                .map_err(|e| to_internal(format!("write wat: {e}")))?;
            let sz = wasm.len();
            self.try_lock_artifacts()?.store(&p.plugin_id, wasm);
            Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "display_name": display,
                    "artifact_bytes": sz,
                    "wat_path": dir.join("clock_pluginEmitted.wat").display().to_string(),
                    "next": "plugin_deploy with this plugin_id",
                }),
            )
            .map_err(to_internal)?]))
        }
    }

    #[tool(
        name = "plugin_deploy_remote",
        description = "Deploy a compiled WASM plugin to a remote Mastertech client over the admin WebSocket session. Requires: (1) a compiled artifact in the artifact store (run plugin_compile or plugin_emit_clock_wasm first), (2) an active Web Console session to the target (check remote_egui_list_targets). The remote client loads the plugin into its PluginManager without recompiling."
    )]
    async fn plugin_deploy_remote(
        &self,
        Parameters(p): Parameters<PluginDeployRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let artifact = {
            let store = self.try_lock_artifacts()?;
            store
                .get_current(&p.plugin_id)
                .cloned()
                .ok_or_else(|| {
                    to_internal(format!(
                        "No artifact for '{}'. Run plugin_compile or plugin_emit_clock_wasm first.",
                        p.plugin_id
                    ))
                })?
        };

        let size = artifact.len();
        let cmd = crate::Cmd::LoadWasmPlugin {
            plugin_id: p.plugin_id.clone(),
            wasm_bytes: artifact,
        };
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

        super::remote_egui_control::hub()
            .send_raw_binary(&p.connection_string, serialized)
            .map_err(to_internal)?;

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({
                "plugin_id": p.plugin_id,
                "connection_string": p.connection_string,
                "deployed_remote": true,
                "artifact_bytes": size,
                "note": "Plugin bytes sent to remote client. It will load asynchronously; check list_plugins on the remote MCP or watch for a toast notification.",
            }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "call_remote_plugin_tool",
        description = "Call an MCP tool on a remote client's plugin over the admin WebSocket session. The call is proxied: admin → remote client → PluginManager → plugin's handle_mcp_call → result back. Requires an active Web Console session and a deployed plugin on the remote."
    )]
    async fn call_remote_plugin_tool(
        &self,
        Parameters(p): Parameters<CallRemotePluginToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let request_id = format!("rpt-{}", uuid::Uuid::new_v4());
        let args = p.args.unwrap_or(serde_json::json!({}));

        let cmd = crate::Cmd::CallRemotePluginTool {
            request_id: request_id.clone(),
            plugin_id: p.plugin_id.clone(),
            tool_name: p.tool_name.clone(),
            args_json: serde_json::to_string(&args).map_err(|e| to_internal(e.to_string()))?,
        };
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

        let rx = register_pending_request(request_id.clone());

        super::remote_egui_control::hub()
            .send_raw_binary(&p.connection_string, serialized)
            .map_err(to_internal)?;

        let (success, result_json) = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            rx,
        )
        .await
        .map_err(|_| to_internal("Remote plugin tool call timed out after 300 seconds"))?
        .map_err(|_| to_internal("Response channel closed (remote client may have disconnected)"))?;

        if success {
            let value: serde_json::Value = serde_json::from_str(&result_json)
                .unwrap_or(serde_json::Value::String(result_json));
            Ok(CallToolResult::success(vec![
                Content::json(value).map_err(to_internal)?,
            ]))
        } else {
            Ok(CallToolResult::error(vec![
                Content::text(result_json),
            ]))
        }
    }

    #[tool(
        name = "plugin_compile_wat",
        description = "Parse WAT source to a WebAssembly 1.0 module (wat crate) and validate with wasmtime::Module::new. Writes plugin.wat under the plugin directory and stores bytes for plugin_deploy."
    )]
    async fn plugin_compile_wat(
        &self,
        Parameters(p): Parameters<PluginCompileWatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        #[cfg(not(feature = "wasm-plugins"))]
        return Err(to_internal(
            "plugin_compile_wat requires displays built with feature wasm-plugins.",
        ));

        #[cfg(feature = "wasm-plugins")]
        {
            let wasm = super::plugin_wasm_factory::wat_to_wasm_validated(&p.wat_source)
                .map_err(to_internal)?;
            let dir = plugin_dir(&p.plugin_id);
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| to_internal(format!("mkdir: {e}")))?;
            tokio::fs::write(dir.join("plugin.wat"), p.wat_source.as_bytes())
                .await
                .map_err(|e| to_internal(format!("write wat: {e}")))?;
            let sz = wasm.len();
            self.try_lock_artifacts()?.store(&p.plugin_id, wasm);
            Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "plugin_id": p.plugin_id,
                    "success": true,
                    "artifact_bytes": sz,
                    "wat_path": dir.join("plugin.wat").display().to_string(),
                }),
            )
            .map_err(to_internal)?]))
        }
    }

    // ── Plugin Registry tools ────────────────────────────────────────────

    #[tool(
        name = "search_plugins",
        description = "Search the SurrealDB plugin registry by keyword across plugin names, descriptions, tool names, and tags. Use this BEFORE writing a new plugin to check if one already exists."
    )]
    async fn search_plugins(
        &self,
        Parameters(p): Parameters<SearchPluginsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let tags = p.tags.as_deref();
        let results = database::schema::PluginRegistryEntry::search(&p.query, tags)
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![
            Content::json(serde_json::json!({
                "count": results.len(),
                "plugins": results.iter().map(|r| serde_json::json!({
                    "plugin_id": r.plugin_id,
                    "name": r.name,
                    "description": r.description,
                    "version": r.version,
                    "tags": r.tags,
                    "tools": r.tools,
                    "has_source": r.source_code.is_some(),
                    "has_wasm": r.wasm_bucket_path.is_some(),
                })).collect::<Vec<_>>(),
            })).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "get_plugin_info",
        description = "Get full details for a plugin from the SurrealDB registry, including source code and tool list."
    )]
    async fn get_plugin_info(
        &self,
        Parameters(p): Parameters<GetPluginInfoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let entry = database::schema::PluginRegistryEntry::get_by_plugin_id(&p.plugin_id)
            .await
            .map_err(to_internal)?;
        match entry {
            Some(e) => Ok(CallToolResult::success(vec![
                Content::json(serde_json::json!(e)).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                Content::text(format!("No plugin found with ID '{}'", p.plugin_id))
            ])),
        }
    }

    #[tool(
        name = "publish_plugin",
        description = "Publish a compiled plugin to the SurrealDB registry. Stores the WASM binary in the 'plugins' bucket and metadata in the plugin_registry table. Call after plugin_compile for reusable plugins."
    )]
    async fn publish_plugin(
        &self,
        Parameters(p): Parameters<PublishPluginParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let wasm_bytes = self
            .try_lock_artifacts()?
            .get_current(&p.plugin_id)
            .cloned();

        let store_source = p.store_source.unwrap_or(true);

        let source_code = if store_source {
            let lib_rs = plugin_dir(&p.plugin_id).join("src").join("lib.rs");
            tokio::fs::read_to_string(&lib_rs).await.ok()
        } else {
            None
        };

        let (name, version, tools_json) = {
            let mgr = self.try_read_manager()?;
            let plugin_info = mgr.list_plugins();
            let matching = plugin_info.iter().find(|pi| pi.id == p.plugin_id);

            let name = matching
                .map(|pi| pi.name.clone())
                .unwrap_or_else(|| p.plugin_id.clone());
            let version = matching
                .map(|pi| pi.version.clone())
                .unwrap_or_else(|| "0.1.0".to_string());

            let tools_json: Vec<database::schema::PluginToolInfo> = mgr.plugins.iter()
                .find(|plug| plug.id() == p.plugin_id)
                .map(|plug| plug.mcp_tools())
                .unwrap_or_default()
                .iter()
                .map(|td| database::schema::PluginToolInfo {
                    name: td.name.clone(),
                    description: td.description.clone(),
                })
                .collect();

            (name, version, tools_json)
        };

        let wasm_path = if let Some(bytes) = &wasm_bytes {
            let bucket_path = format!("/{}/{}.wasm", sanitize_id(&p.plugin_id), version);
            database::schema::put_file("plugins", &bucket_path, bytes.clone())
                .await
                .map_err(to_internal)?;
            Some(bucket_path)
        } else {
            None
        };

        let entry = database::schema::PluginRegistryEntry {
            plugin_id: p.plugin_id.clone(),
            name,
            description: p.description.clone(),
            version: version.clone(),
            tools: tools_json,
            tags: p.tags.clone().unwrap_or_default(),
            wasm_bucket_path: wasm_path.clone(),
            source_code,
            ..Default::default()
        };

        database::schema::PluginRegistryEntry::upsert(&entry)
            .await
            .map_err(to_internal)?;

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({
                "plugin_id": p.plugin_id,
                "published": true,
                "version": version,
                "wasm_stored": wasm_path.is_some(),
                "wasm_bucket_path": wasm_path,
                "source_stored": entry.source_code.is_some(),
            }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "fetch_plugin",
        description = "Download a plugin's WASM binary from the SurrealDB registry into the local artifact store, so it can be deployed via plugin_deploy or plugin_deploy_remote."
    )]
    async fn fetch_plugin(
        &self,
        Parameters(p): Parameters<FetchPluginParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let entry = database::schema::PluginRegistryEntry::get_by_plugin_id(&p.plugin_id)
            .await
            .map_err(to_internal)?
            .ok_or_else(|| to_internal(format!("Plugin '{}' not found in registry", p.plugin_id)))?;

        let wasm_path = entry
            .wasm_bucket_path
            .as_deref()
            .ok_or_else(|| to_internal("Plugin has no WASM binary in the registry"))?;

        let bytes = database::schema::get_file("plugins", wasm_path)
            .await
            .map_err(to_internal)?
            .ok_or_else(|| to_internal(format!("WASM file not found at bucket path: {}", wasm_path)))?;

        let sz = bytes.len();
        self.try_lock_artifacts()?.store(&p.plugin_id, bytes);

        if let Some(source) = &entry.source_code {
            let dir = plugin_dir(&p.plugin_id);
            let src_dir = dir.join("src");
            let _ = tokio::fs::create_dir_all(&src_dir).await;
            let _ = tokio::fs::write(src_dir.join("lib.rs"), source).await;
            let cargo_toml_path = dir.join("Cargo.toml");
            if !cargo_toml_path.exists() {
                let _ = tokio::fs::write(&cargo_toml_path, plugin_cargo_toml(&p.plugin_id)).await;
            }
        }

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({
                "plugin_id": p.plugin_id,
                "fetched": true,
                "version": entry.version,
                "artifact_bytes": sz,
                "source_restored": entry.source_code.is_some(),
            }),
        )
        .map_err(to_internal)?]))
    }

    // ── Diagnostic Knowledge Base tools ──────────────────────────────────

    #[tool(
        name = "create_diagnostic_session",
        description = "Start a new diagnostic session. Call at the beginning of any diagnostic engagement. Returns a session_id to use with log_diagnostic_entry and close_diagnostic_session."
    )]
    async fn create_diagnostic_session(
        &self,
        Parameters(p): Parameters<CreateDiagnosticSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = database::schema::DiagnosticSession {
            connection_string: p.connection_string,
            hostname: p.hostname,
            customer_name: p.customer_name,
            tech: p.tech,
            tags: p.tags.unwrap_or_default(),
            ..Default::default()
        };
        let id = database::schema::DiagnosticSession::create(&session)
            .await
            .map_err(to_internal)?;
        use database::schema::RecordIdExt;
        let id_str = id.key_string();
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "session_id": id_str }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "log_diagnostic_entry",
        description = "Log a finding, action, resolution, or note to an open diagnostic session. Use categories: 'finding' for discovered issues, 'action' for steps taken, 'resolution' for fixes applied, 'note' for general observations."
    )]
    async fn log_diagnostic_entry(
        &self,
        Parameters(p): Parameters<LogDiagnosticEntryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let entry = database::schema::DiagnosticEntry {
            session_ref: database::schema::RecordId::new(
                database::schema::DIAGNOSTIC_SESSION_TABLE,
                p.session_id.clone(),
            ),
            category: p.category,
            title: p.title,
            detail: p.detail,
            data: p.data,
            plugins_used: p.plugins_used.unwrap_or_default().into_iter().map(|pu| {
                database::schema::PluginUsageRef {
                    plugin_id: pu.plugin_id,
                    tool_name: pu.tool_name,
                }
            }).collect(),
            ..Default::default()
        };
        let id = database::schema::DiagnosticEntry::create(&entry)
            .await
            .map_err(to_internal)?;
        use database::schema::RecordIdExt;
        let id_str = id.key_string();
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "entry_id": id_str, "session_id": p.session_id }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "close_diagnostic_session",
        description = "Close a diagnostic session with a final status and AI-written summary. Status should be 'resolved', 'escalated', or 'open'."
    )]
    async fn close_diagnostic_session(
        &self,
        Parameters(p): Parameters<CloseDiagnosticSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        database::schema::DiagnosticSession::close(
            &p.session_id,
            &p.status,
            &p.summary,
            p.tags.as_deref(),
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "session_id": p.session_id, "closed": true, "status": p.status }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "search_diagnostics",
        description = "Search past diagnostic sessions by hostname, customer name, tags, or free text. Use to check if a machine/customer has been diagnosed before."
    )]
    async fn search_diagnostics(
        &self,
        Parameters(p): Parameters<SearchDiagnosticsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let sessions = database::schema::DiagnosticSession::search(
            &p.query,
            p.hostname.as_deref(),
            p.customer_name.as_deref(),
            p.connection_string.as_deref(),
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "count": sessions.len(), "sessions": sessions }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "get_diagnostic_session",
        description = "Retrieve a full diagnostic session with all its log entries."
    )]
    async fn get_diagnostic_session(
        &self,
        Parameters(p): Parameters<GetDiagnosticSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let full = database::schema::DiagnosticSession::get_full(&p.session_id)
            .await
            .map_err(to_internal)?;
        match full {
            Some(f) => Ok(CallToolResult::success(vec![
                Content::json(serde_json::json!(f)).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                Content::text(format!("No diagnostic session found with ID '{}'", p.session_id))
            ])),
        }
    }

    // ── Customer / Service data tools ────────────────────────────────────

    #[tool(
        name = "search_customers",
        description = "Search the SurrealDB customer table by name, email, or phone number."
    )]
    async fn search_customers(
        &self,
        Parameters(p): Parameters<SearchCustomersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let q = p.query.to_lowercase();
        let customers: Vec<serde_json::Value> = database::DATABASE
            .query(
                "SELECT * FROM customer WHERE \
                 string::lowercase(name) CONTAINS $q \
                 OR string::lowercase(email) CONTAINS $q \
                 OR phone_number CONTAINS $q \
                 OR phone_number_2 CONTAINS $q \
                 OR cust_code CONTAINS $q \
                 LIMIT 25"
            )
            .bind(("q", q))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "count": customers.len(), "customers": customers }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "get_customer_details",
        description = "Get a full customer record including linked service orders and computers."
    )]
    async fn get_customer_details(
        &self,
        Parameters(p): Parameters<GetCustomerDetailsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let key = if p.customer_id.contains(':') {
            p.customer_id.split(':').last().unwrap_or(&p.customer_id).to_string()
        } else {
            p.customer_id.clone()
        };
        let rid = database::schema::RecordId::new("customer", key);
        let result: Option<serde_json::Value> = database::DATABASE
            .query(
                "SELECT *, \
                   (SELECT * FROM service_order WHERE customer == $rid FETCH computer) AS services \
                 FROM $rid"
            )
            .bind(("rid", rid))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        match result {
            Some(v) => Ok(CallToolResult::success(vec![
                Content::json(v).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                Content::text(format!("No customer found with ID '{}'", p.customer_id))
            ])),
        }
    }

    #[tool(
        name = "get_service_order",
        description = "Get a service order by service number, with customer and computer details fetched."
    )]
    async fn get_service_order(
        &self,
        Parameters(p): Parameters<GetServiceOrderParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result: Option<serde_json::Value> = database::DATABASE
            .query(
                "SELECT * FROM service_order WHERE service_number == $sn FETCH computer, customer LIMIT 1"
            )
            .bind(("sn", p.service_number.clone()))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        match result {
            Some(v) => Ok(CallToolResult::success(vec![
                Content::json(v).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                Content::text(format!("No service order found with number '{}'", p.service_number))
            ])),
        }
    }

    #[tool(
        name = "search_service_orders",
        description = "Search service orders by customer name, service number, tech, or checkin notes."
    )]
    async fn search_service_orders(
        &self,
        Parameters(p): Parameters<SearchServiceOrdersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let q = p.query.to_lowercase();
        let mut conditions = vec![
            "(string::lowercase(service_number) CONTAINS $q \
             OR string::lowercase(checkin_notes ?? '') CONTAINS $q \
             OR string::lowercase(salesman ?? '') CONTAINS $q \
             OR string::lowercase(doc_alias ?? '') CONTAINS $q)".to_string()
        ];
        if p.tech.is_some() {
            conditions.push("tech == $tech".to_string());
        }
        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT * FROM service_order WHERE {where_clause} ORDER BY created_at DESC LIMIT 25 FETCH computer, customer"
        );
        let results: Vec<serde_json::Value> = database::DATABASE
            .query(&sql)
            .bind(("q", q))
            .bind(("tech", p.tech.unwrap_or_default()))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "count": results.len(), "orders": results }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "get_computer_details",
        description = "Get full computer details including hostname, CPU, GPU, RAM, drives, serials, and installed programs."
    )]
    async fn get_computer_details(
        &self,
        Parameters(p): Parameters<GetComputerDetailsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let key = if p.computer_id.contains(':') {
            p.computer_id.split(':').last().unwrap_or(&p.computer_id).to_string()
        } else {
            p.computer_id.clone()
        };
        let rid = database::schema::RecordId::new("computer", key);
        let result: Option<serde_json::Value> = database::DATABASE
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        match result {
            Some(v) => Ok(CallToolResult::success(vec![
                Content::json(v).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                Content::text(format!("No computer found with ID '{}'", p.computer_id))
            ])),
        }
    }

    #[tool(
        name = "search_prestashop_orders",
        description = "Search PrestaShop orders by customer email or order reference."
    )]
    async fn search_prestashop_orders(
        &self,
        Parameters(p): Parameters<SearchPrestashopOrdersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let api = database::schema::prestashop::Prestashop::default();
        let filter_val = format!("%[{}]%", p.query);
        let mut query_params = std::collections::HashMap::new();
        query_params.insert("filter[reference]", filter_val.as_str());
        query_params.insert("output_format", "JSON");

        let orders: Result<Vec<database::schema::prestashop::Order>, _> =
            api.request_resources_wasm("orders", query_params).await;

        match orders {
            Ok(o) => Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({ "count": o.len(), "orders": o }),
            )
            .map_err(to_internal)?])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("PrestaShop search error: {e}"))
            ])),
        }
    }

    #[tool(
        name = "search_odoo_inventory",
        description = "Search Odoo product catalog by part number or product name."
    )]
    async fn search_odoo_inventory(
        &self,
        Parameters(p): Parameters<SearchOdooInventoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match database::schema::odoo::search_odoo_products(&p.query).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({ "count": resp.result.len(), "products": resp.result }),
            )
            .map_err(to_internal)?])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("Odoo search error: {e}"))
            ])),
        }
    }

    // ── SurrealDB query tool ─────────────────────────────────────────────

    #[tool(
        name = "query_surrealdb",
        description = "Run a read-only SurrealQL query against the Mastertech database. Only SELECT and RETURN statements are allowed."
    )]
    async fn query_surrealdb(
        &self,
        Parameters(p): Parameters<QuerySurrealDbParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let trimmed = p.query.trim();
        let upper = trimmed.to_uppercase();
        if !upper.starts_with("SELECT") && !upper.starts_with("RETURN") {
            return Err(to_internal(
                "Only SELECT and RETURN queries are allowed. Mutations (CREATE, UPDATE, DELETE, etc.) are not permitted.",
            ));
        }
        let result: Vec<serde_json::Value> = database::DATABASE
            .query(trimmed)
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "results": result }),
        )
        .map_err(to_internal)?]))
    }
}

// ─── Server handler ────────────────────────────────────────────────────────────

/// Shown to MCP clients in `initialize` (`ServerInfo.instructions`). Keep in sync with View menu + `nav_tab_anchor_key` in Mastertech `menu_bar.rs`.
const INSTRUCTIONS: &str = r#"Mastertech Plugin System MCP (MasterTech desktop + admin Web Console).

=== Session (HTTP :9004/mcp) ===
After initialize, POST notifications/initialized with the same Mcp-Session-Id before tools/call.

=== AI Workflow ===
Before writing a new WASM plugin, ALWAYS call search_plugins first to check if a suitable plugin already exists in the registry. If one exists, use fetch_plugin to download it and plugin_deploy / plugin_deploy_remote to deploy it.
After compiling a useful plugin, call publish_plugin to store it in the SurrealDB registry for future sessions.
When performing diagnostics:
  1. Call search_diagnostics to check if this machine/customer has been diagnosed before.
  2. Call create_diagnostic_session at the start.
  3. Call log_diagnostic_entry for each finding, action taken, or resolution.
  4. Call close_diagnostic_session with a summary when done.
Use search_customers / get_customer_details / search_service_orders to pull customer context and service history.
Use get_computer_details to see full hardware info for a machine.
Use search_prestashop_orders for purchase/invoice lookup and search_odoo_inventory for parts availability.
Use query_surrealdb for any ad-hoc read-only data needs (SELECT/RETURN only).

=== Plugins & WASM ===
- list_plugins, enable_plugin, disable_plugin, call_plugin_tool — native + WASM plugin MCP tools.
- plugin_source → plugin_compile (wasm32-wasip1) → plugin_deploy (local) or plugin_deploy_remote (to a connected client); or plugin_emit_clock_wasm / plugin_compile_wat → plugin_deploy / plugin_deploy_remote; plugin_rollback; plugin_watch.

=== Plugin Registry (SurrealDB) ===
- search_plugins — search by keyword/tags before writing new plugins.
- get_plugin_info — full details including source code for a registered plugin.
- publish_plugin — store compiled WASM + metadata after plugin_compile.
- fetch_plugin — download WASM from registry into local artifact store for deploy.

=== Diagnostic Knowledge Base ===
- create_diagnostic_session — start logging a diagnostic engagement.
- log_diagnostic_entry — append findings/actions/resolutions with optional structured data.
- close_diagnostic_session — finalize with status and summary.
- search_diagnostics — find past sessions by hostname, customer, tags, or free text.
- get_diagnostic_session — retrieve a full session with all entries.

=== Customer & Service Data ===
- search_customers — search SurrealDB customer table by name/email/phone.
- get_customer_details — full customer record with linked service orders.
- get_service_order — look up by service number (with computer + customer fetched).
- search_service_orders — search by customer name, tech, service number.
- get_computer_details — full hardware record (CPU, GPU, RAM, drives, serials, programs).
- search_prestashop_orders — search PrestaShop orders by reference/email.
- search_odoo_inventory — search Odoo product catalog by part number or name.
- query_surrealdb — run arbitrary read-only SurrealQL (SELECT/RETURN only).

=== Remote egui (operator must connect Web Console to a client first) ===
Flow: remote_egui_list_targets → optional remote_egui_get_last_frame_meta → remote_egui_list_widget_anchors (see keys) → remote_egui_click_anchor and/or remote_egui_type, or remote_egui_perform_steps (click_anchor, text, sleep_ms, key_tap, etc.). Same binary path as inline viewer: EGUI_INPUT_TAG + EguiInputEvent.
- nav.menu.view — click to open the View menu (top bar).
- nav.tab.<slug> — tab row inside View menu. Slug = tab label lowercased with non-alphanumeric → '_', trim '_' (e.g. KOTH → nav.tab.koth; TUR Sheet → nav.tab.tur_sheet; File Browser 📂 → nav.tab.file_browser). Tab anchors exist only while View menu is open: click nav.menu.view, sleep ~400–500ms, then click nav.tab.* .
- TUR Sheet widgets (when that tab is visible): tur.service_number, tur.customer_name, tur.phone_number, tur.customer_email, tur.salesman, tur.tech, tur.checkin_notes, tur.recommendations.

=== View tabs (names match menu; add/close tab toggles dock) ===
- TUR Sheet — Service intake / walk-in form (customer, tech, notes, recommendations).
- KOTH — Store “king of the hill” / display board.
- Sales Tracker — Sales totals and tracking.
- Scene Editor — Scene/layout tools (dock tab).
- Scripts — Saved scripts and tooling.
- File Browser 📂 — File browser / workspace files.
- SysInfo — Machine and environment summary.
- Minidump Analysis — Crash dump analysis (Windows; when enabled).
- Ai — AI playground (models, prompts).
- Resource Monitor — Processes and resource usage.
- My Tasks — Personal task queue layout.
- Store Tasks — Store-wide open tasks layout.
- Completed Tasks — Completed task layout.
- Bug Tracker — GitHub issue tracking.
- Websockets — WebSocket sessions and messaging.
- Admin Console — Remote clients: shell, files, viewers (connect to agents here).
- Web Console — In-app web/shell console.
- Inventory — Stock / inventory tables.
- Task Audit — History and audit of task changes.
- Create Prestashop Order — PrestaShop order entry.
- Plugins — Plugin list; MCP :9004; enable frame capture / remote viewer on the client being viewed.
- Downloads — App releases / downloads.
- Threads — Operator chat threads.
- Logs — Egui log viewer (filters/categories).

Other dock tabs (context menus / layouts, not all in View list): Part Order, My Tools, QC, Query Editor (admins). Use dock UI or existing flows to open them.

=== Remote egui pitfalls ===
Do not skip notifications/initialized. Prefer perform_steps with sleep_ms between opening View menu and clicking nav.tab.*. If click_anchor fails with unknown key, call list_widget_anchors again (stale frame)."#;

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
pub async fn run_plugin_mcp_server(manager: Arc<RwLock<PluginManager>>) -> anyhow::Result<()> {
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
pub async fn run_plugin_mcp_server_http(manager: Arc<RwLock<PluginManager>>) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager,
        StreamableHttpServerConfig, StreamableHttpService,
    };

    if let Err(e) = database::schema::define_bucket("plugins", "memory").await {
        log::warn!("Failed to define 'plugins' bucket (non-fatal): {e}");
    } else {
        log::info!("SurrealDB 'plugins' bucket initialized");
    }

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
