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
//! - **stdio** — single-session MCP over the process's stdin/stdout (`run_plugin_mcp_server_stdio`).
//!   Designed for Claude Desktop and other launcher-based clients that spawn the server as a
//!   child process and speak JSON-RPC on its stdio. **Only safe when the global logger writes
//!   to stderr** — any byte that lands on stdout corrupts the JSON-RPC framing.
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

use super::remote_script_notify::{
    RemoteScriptSession, REMOTE_SCRIPT_ACCUM, REMOTE_SCRIPT_PENDING,
};
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

/// Best-effort cleanup of a pending request id.  Called when
/// `call_remote_plugin_tool` aborts (timeout, channel-closed, panic
/// through the `?` operator) — without this, every timed-out call
/// leaked a sender into `REMOTE_TOOL_PENDING` forever, eventually
/// taking up a noticeable amount of memory after a long debugging
/// session.  Idempotent: cheap no-op when the entry has already been
/// resolved.
fn unregister_pending_request(request_id: &str) {
    if let Ok(mut map) = REMOTE_TOOL_PENDING.lock() {
        map.remove(request_id);
    }
}

/// RAII guard that calls [`unregister_pending_request`] on drop.
/// Held next to the receiver inside `call_remote_plugin_tool` so the
/// registry slot evaporates on every exit path, including the
/// timeout-via-`?`-propagation path the previous code leaked through.
struct PendingRequestGuard {
    request_id: String,
}
impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        unregister_pending_request(&self.request_id);
    }
}

// ─── Local script run request routing ─────────────────────────────────────────
//
// Bridges the MCP `scripts_run` tool to the host Mastertech4.0 Scripts tab
// (egui or terminal mode). The MCP tool sends a `ScriptRunRequest` over the
// global crossbeam channel in `crate::scripts::mcp_channel`; the host's
// Scripts tab drains it each frame, runs the script through its existing
// handlers, and publishes a `ScriptRunResult` back. A single drainer task
// (spawned at MCP server start) reads results off the global crossbeam
// channel and dispatches them to per-request `oneshot` receivers so the MCP
// tool call can `await` its specific completion.

type ScriptRunPending = std::sync::Mutex<HashMap<String, oneshot::Sender<crate::scripts::ScriptRunResult>>>;
static SCRIPT_RUN_PENDING: Lazy<ScriptRunPending> = Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

fn register_pending_script_run(request_id: String) -> oneshot::Receiver<crate::scripts::ScriptRunResult> {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut map) = SCRIPT_RUN_PENDING.lock() {
        map.insert(request_id, tx);
    }
    rx
}

/// Spawn the global drainer that forwards every incoming `ScriptRunResult`
/// from the crossbeam channel into the matching per-request `oneshot` slot.
/// Idempotent — uses a `std::sync::Once` so calling more than once is safe.
fn ensure_script_run_drainer_spawned() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let rx = crate::scripts::script_run_result_receiver();
        tokio::task::spawn_blocking(move || {
            use crossbeam::channel::RecvTimeoutError;
            loop {
                match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                    Ok(result) => {
                        let pending_tx = SCRIPT_RUN_PENDING
                            .lock()
                            .ok()
                            .and_then(|mut map| map.remove(&result.request_id));
                        match pending_tx {
                            Some(tx) => {
                                let _ = tx.send(result);
                            }
                            None => log::warn!(
                                "Received ScriptRunResult for unknown request_id {} (caller may have timed out)",
                                result.request_id
                            ),
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        log::error!("ScriptRunResult channel disconnected; drainer exiting");
                        break;
                    }
                }
            }
        });
    });
}

// Remote script accumulation + MCP waiter live in `remote_script_notify` (WASM-safe).

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

/// Deserialize `Option<Vec<String>>` accepting either a JSON array (`["a","b"]`)
/// or a JSON-stringified array (`"[\"a\",\"b\"]"`). Some MCP clients stringify
/// nested array arguments before sending; this lets the schema-correct array
/// form continue to work while also accepting the stringified form.
fn deserialize_optional_string_vec<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Vec(Vec<String>),
        Str(String),
    }

    match Option::<Input>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Input::Vec(v)) => Ok(Some(v)),
        Some(Input::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            serde_json::from_str::<Vec<String>>(trimmed)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
    }
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
    #[schemars(description = "REQUIRED. Customer record id (e.g. 'customer:abc123' or just 'abc123'). Look up first via find_customer_by_email/phone or via the connected_client.computer.customer graph. If you cannot resolve a customer, ask the user before retrying — do not fabricate.")]
    pub customer_id: String,
    #[schemars(description = "REQUIRED. Computer record id (e.g. 'computer:abc123' or just 'abc123'). Look up first via get_computer_details or via connected_client.computer. If you cannot resolve a computer, ask the user before retrying — do not fabricate.")]
    pub computer_id: String,
    #[schemars(description = "Optional task record id (e.g. 'task:abc123') if this diagnostic corresponds to an in-house service task. Can be linked later via link_diagnostic_to_task.")]
    pub task_id: Option<String>,
    #[schemars(description = "Optional service order record id (e.g. 'service_order:abc123') if a check-in service order exists for this device.")]
    pub service_order_id: Option<String>,
    #[schemars(description = "Customer display name (if known)")]
    pub customer_name: Option<String>,
    #[schemars(description = "Technician performing the diagnosis")]
    pub tech: Option<String>,
    #[schemars(description = "Initial tags for categorizing this session")]
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct LinkDiagnosticToTaskParams {
    #[schemars(description = "Session ID to update (e.g. session_id returned by create_diagnostic_session)")]
    pub session_id: String,
    #[schemars(description = "Task record id to associate with this session (e.g. 'task:abc123' or just 'abc123')")]
    pub task_id: Option<String>,
    #[schemars(description = "Optional service order record id to associate with this session")]
    pub service_order_id: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct LogDiagnosticEntryParams {
    #[schemars(description = "Session ID returned by create_diagnostic_session")]
    pub session_id: String,
    #[schemars(description = "Category. Allowed values: finding, action, note, error, system_info, network_info, security_alert, performance_note, customer_note, recommendation. Anything else is treated as 'note'.")]
    pub category: String,
    #[schemars(description = "Short title for this entry")]
    pub title: String,
    #[schemars(description = "Detailed description of the finding/action/resolution")]
    pub detail: String,
    #[schemars(description = "Optional structured data — MUST be a JSON object or array, NOT a stringified JSON. Pass e.g. {\"complaint\":\"bsod\", \"events\": [...]} not \"{\\\"complaint\\\":...\\\"}\". The server will defensively parse stringified JSON but a real object is preferred.")]
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

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsListParams {}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsRunParams {
    #[schemars(
        description = "Script category. One of: 'Tuneup', 'Informational', 'JunkwareRemoval'."
    )]
    pub category: String,
    #[schemars(
        description = "Display name of the script as listed by scripts_list (e.g. 'Activate Webroot', 'Disable OneDrive Startup')."
    )]
    pub script_name: String,
    #[schemars(
        description = "Optional service number override. Required for activation scripts (Webroot, SuperAnti, SEB)."
    )]
    pub service_number: Option<String>,
    #[schemars(
        description = "Optional customer email override. Required for SuperEasyBackup activation."
    )]
    pub customer_email: Option<String>,
    #[schemars(
        description = "Timeout in seconds to wait for the script to finish. Default 600 (10 minutes). Increase for Windows Updates / scans."
    )]
    pub timeout_secs: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsRunRemoteParams {
    #[schemars(description = "Web Console connection_string of the remote client (from remote_egui_list_targets).")]
    pub connection_string: String,
    #[schemars(description = "Script category. One of: 'Tuneup', 'Informational', 'JunkwareRemoval'.")]
    pub category: String,
    #[schemars(description = "Display name of the script as listed by scripts_list (e.g. 'Activate Webroot', 'Activate SEB').")]
    pub script_name: String,
    #[schemars(description = "Service order number. Required for activation scripts (Webroot, SuperAnti, SEB).")]
    pub service_number: Option<String>,
    #[schemars(description = "Customer email. Required for SuperEasyBackup activation.")]
    pub customer_email: Option<String>,
    #[schemars(description = "Timeout in seconds to wait for the script to complete on the remote. Default 600.")]
    pub timeout_secs: Option<u64>,
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

// ─── Remote build worker param types ──────────────────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ListBuildWorkersParams {}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginCompileRemoteParams {
    #[schemars(description = "Plugin ID (source must already exist via plugin_source)")]
    pub plugin_id: String,
    #[schemars(description = "Optional: connection_string of a specific build worker. Defaults to the first online worker that advertises the requested target.")]
    pub worker_connection_string: Option<String>,
    #[schemars(description = "Rustc target triple (default: wasm32-wasip1)")]
    pub target: Option<String>,
    #[schemars(description = "Cargo profile (default: release)")]
    pub profile: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PluginCompileStatusParams {
    #[schemars(description = "Job ID returned by plugin_compile_remote")]
    pub job_id: String,
    #[schemars(description = "If true and the job succeeded, remove it from the in-memory pending table after returning (default: false). The compiled bytes have already been stored in the ArtifactStore so plugin_deploy / plugin_deploy_remote still work.")]
    pub forget_on_done: Option<bool>,
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
                        "No artifact for '{}' in the local ArtifactStore. \
                         Populate it first with `fetch_plugin` (registry), \
                         `plugin_compile` (cargo on this host), `plugin_compile_remote` \
                         (worker / local fallback), or `plugin_emit_clock_wasm` \
                         (clock plugin).",
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
        description = "Deploy a compiled WASM plugin to a remote Mastertech client over the admin WebSocket session. PRECONDITIONS (ALL required): (1) artifact in the local store — populate via `fetch_plugin` (registry), `plugin_compile`/`plugin_compile_remote` (build from source), or `plugin_emit_clock_wasm` (clock plugin); (2) active Web Console session to the target — check `remote_egui_list_targets`. If you see `No artifact for '<id>'`, you skipped step 1: call `search_plugins` then `fetch_plugin` for registry plugins, OR `plugin_source` + `plugin_compile_remote` for new code. The remote client loads the plugin into its PluginManager without recompiling."
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
                        "No artifact for '{}' in the local ArtifactStore. \
                         You must populate it BEFORE calling plugin_deploy_remote. \
                         Pick one: \
                         (a) `search_plugins` for an existing registry plugin and then `fetch_plugin` with its plugin_id; \
                         (b) `plugin_source` + `plugin_compile` if Rust is on this host; \
                         (c) `plugin_source` + `plugin_compile_remote` (auto-falls back to local cargo when no plugin_builder workers are live).",
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
        // RAII: registry slot evaporates on any exit path (Ok, Err,
        // panic propagation through `?`).  Without this every timeout
        // leaks a sender into REMOTE_TOOL_PENDING.
        let _guard = PendingRequestGuard { request_id: request_id.clone() };

        super::remote_egui_control::hub()
            .send_raw_binary(&p.connection_string, serialized)
            .map_err(to_internal)?;

        log::info!(
            "call_remote_plugin_tool start: req={request_id} cs={} plugin={} tool={}",
            p.connection_string,
            p.plugin_id,
            p.tool_name
        );

        // Periodic stall warnings while waiting: the previous shape was a
        // single 300 s `tokio::time::timeout` that revealed nothing about
        // *which* request was stuck or *how long* it had been silent.
        // Now we wake every 30 s, log a warn naming the request, and
        // continue waiting up to the hard deadline.  Each wake is cheap
        // (oneshot polls return immediately when nothing is ready) and
        // lets the operator see in real time which tool call is the one
        // holding everything else up.
        const HARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(300);
        const STALL_TICK: std::time::Duration = std::time::Duration::from_secs(30);
        let started_at = std::time::Instant::now();
        let mut rx = rx;
        let result_pair: Option<(bool, String)> = loop {
            let remaining = HARD_DEADLINE.saturating_sub(started_at.elapsed());
            if remaining.is_zero() {
                break None;
            }
            let next_tick = remaining.min(STALL_TICK);
            match tokio::time::timeout(next_tick, &mut rx).await {
                Ok(Ok(pair)) => break Some(pair),
                Ok(Err(_)) => {
                    // Sender dropped — receive-side resolve never came
                    // and never will.  Bail out as a fast error rather
                    // than waiting out the deadline.
                    log::warn!(
                        "call_remote_plugin_tool: response channel closed for req={request_id} \
                         cs={} plugin={} tool={} after {:?} — remote client may have \
                         disconnected mid-call",
                        p.connection_string,
                        p.plugin_id,
                        p.tool_name,
                        started_at.elapsed()
                    );
                    return Err(to_internal(format!(
                        "Response channel closed for {}::{} req={request_id} \
                         (remote client {} may have disconnected mid-call)",
                        p.plugin_id, p.tool_name, p.connection_string
                    )));
                }
                Err(_) => {
                    let waited = started_at.elapsed();
                    log::warn!(
                        "call_remote_plugin_tool STALL: req={request_id} cs={} plugin={} \
                         tool={} — no response for {:?}; deadline at {:?} total",
                        p.connection_string,
                        p.plugin_id,
                        p.tool_name,
                        waited,
                        HARD_DEADLINE
                    );
                    // Loop and wait another STALL_TICK.
                }
            }
        };

        let (success, result_json) = match result_pair {
            Some(pair) => {
                log::info!(
                    "call_remote_plugin_tool ok: req={request_id} cs={} plugin={} tool={} \
                     after {:?}",
                    p.connection_string,
                    p.plugin_id,
                    p.tool_name,
                    started_at.elapsed()
                );
                pair
            }
            None => {
                log::error!(
                    "call_remote_plugin_tool TIMEOUT: req={request_id} cs={} plugin={} \
                     tool={} after {:?} (hard deadline {:?})",
                    p.connection_string,
                    p.plugin_id,
                    p.tool_name,
                    started_at.elapsed(),
                    HARD_DEADLINE
                );
                return Err(to_internal(format!(
                    "Remote plugin tool call timed out after {:?}: \
                     req={request_id} cs={} plugin={} tool={}.  \
                     The kernel TCP socket may still be open (no peer-closed \
                     event seen on the admin transport) — check the client log \
                     for whether the call completed there but the response \
                     never made it back.",
                    HARD_DEADLINE,
                    p.connection_string,
                    p.plugin_id,
                    p.tool_name
                )));
            }
        };

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
            // First check if the plugin is already loaded.
            let already_loaded = {
                let mgr = self.try_read_manager()?;
                mgr.list_plugins().iter().any(|pi| pi.id == p.plugin_id)
            };

            // If not loaded but we have a compiled artifact, hot-load it locally
            // so we can call plugin_name() / mcp_tools() for the registry entry.
            if !already_loaded {
                if let Some(artifact) = self.try_lock_artifacts()?.get_current(&p.plugin_id).cloned() {
                    #[cfg(feature = "wasm-plugins")]
                    {
                        let mut mgr = self.try_write_manager()?;
                        mgr.unregister(&p.plugin_id);
                        let _ = mgr.load_wasm(artifact); // ignore load error — metadata extraction is best-effort
                    }
                    let _ = artifact; // suppress unused warning in non-wasm build
                }
            }

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
        description = "Start a new diagnostic session. Call at the beginning of any diagnostic engagement. customer_id and computer_id are REQUIRED — every diagnostic must belong to a known customer and computer. Resolve them first via find_customer_by_email/phone, get_computer_details, or by following connected_client.computer.customer; if you cannot resolve, ASK THE USER instead of fabricating. Returns a session_id to use with log_diagnostic_entry and close_diagnostic_session."
    )]
    async fn create_diagnostic_session(
        &self,
        Parameters(p): Parameters<CreateDiagnosticSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let customer_id = parse_record_id(
            &p.customer_id,
            database::schema::CUSTOMER_TABLE,
        );
        let computer_id = parse_record_id(
            &p.computer_id,
            database::schema::COMPUTER_TABLE,
        );
        let task_ref = p.task_id.as_deref().map(|s| {
            parse_record_id(s, database::schema::TASK_TABLE)
        });
        let service_order = p.service_order_id.as_deref().map(|s| {
            parse_record_id(s, database::schema::TICKET_TABLE)
        });

        // Validate referenced rows actually exist before stamping them
        // onto a new diagnostic session.  Without this, a malformed
        // record id (the `parse_record_id` colon-stripping regression
        // that produced `computer:b57a7e8f9` instead of
        // `computer:DESKTOP-HQAF13L:b57a7e8f9`) silently lands and the
        // session points at nothing.  Same protection for customer_id —
        // the connected_client.customer field has historically been
        // populated with default UUIDs that have no backing row, and
        // an AI that trusts that value would create an orphan session.
        //
        // Failure surfaces as `invalid_params` so the AI can fix the
        // call rather than retrying blind.
        use database::schema::utilities::record_exists;
        match record_exists(customer_id.clone()).await {
            Ok(Some(true)) => {}
            _ => {
                return Err(ErrorData::invalid_params(
                    format!(
                        "create_diagnostic_session: customer_id {customer_id:?} does not \
                         resolve to an existing customer row.  Use find_customer_by_email/phone \
                         or follow connected_client.computer.customer; do NOT pass a raw UUID \
                         from connected_client.customer without verifying it exists.",
                    ),
                    None,
                ));
            }
        }
        match record_exists(computer_id.clone()).await {
            Ok(Some(true)) => {}
            _ => {
                return Err(ErrorData::invalid_params(
                    format!(
                        "create_diagnostic_session: computer_id {computer_id:?} does not \
                         resolve to an existing computer row.  Resolve via \
                         get_computer_details or connected_client.computer.  Note: the \
                         canonical computer key includes the hostname, e.g. \
                         `computer:DESKTOP-XYZ:abc123456`, NOT just the bare hash.",
                    ),
                    None,
                ));
            }
        }

        let session = database::schema::DiagnosticSession {
            connection_string: p.connection_string,
            hostname: p.hostname,
            customer_name: p.customer_name,
            customer_id,
            computer_id,
            task_ref,
            service_order,
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
        name = "link_diagnostic_to_task",
        description = "Retroactively link an existing diagnostic_session to an in-house task and/or service_order. Use after a customer checks in their device for service so the diagnostic appears in the task modal's Diagnostics tab."
    )]
    async fn link_diagnostic_to_task(
        &self,
        Parameters(p): Parameters<LinkDiagnosticToTaskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_record_id(
            &p.session_id,
            database::schema::DIAGNOSTIC_SESSION_TABLE,
        );
        let task_ref = p.task_id.as_deref().map(|s| {
            parse_record_id(s, database::schema::TASK_TABLE)
        });
        let service_order = p.service_order_id.as_deref().map(|s| {
            parse_record_id(s, database::schema::TICKET_TABLE)
        });
        if task_ref.is_none() && service_order.is_none() {
            return Err(ErrorData::invalid_params(
                "link_diagnostic_to_task: at least one of task_id or service_order_id must be provided".to_string(),
                None,
            ));
        }
        database::schema::DiagnosticSession::link_to_task(
            &session_id,
            task_ref.as_ref(),
            service_order.as_ref(),
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({
                "session_id": p.session_id,
                "task_id": p.task_id,
                "service_order_id": p.service_order_id,
                "linked": true
            }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "log_diagnostic_entry",
        description = "Log an entry against an open diagnostic_session. Allowed categories: 'finding' (discovered issue), 'action' (step taken), 'note' (general observation), 'error' (tool/command failed), 'system_info', 'network_info', 'security_alert', 'performance_note', 'customer_note', 'recommendation'. Anything else is recorded as 'note'."
    )]
    async fn log_diagnostic_entry(
        &self,
        Parameters(p): Parameters<LogDiagnosticEntryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Defensive-parse `data`: the AI sometimes hands us a *stringified*
        // JSON object (e.g. data: "{\"complaint\":\"…\"}") instead of a
        // real JSON object.  SurrealDB's `diagnostic_entry.data` field is
        // typed `none | object`, so a String value gets rejected with
        // "Couldn't coerce value for field `data`… Expected `none |
        // object` but found `'{...}'`".  When the value is a String
        // that parses as a JSON object/array, swap it for the parsed
        // form before storage.  Bare strings (non-JSON-looking) are
        // dropped to None — they're not what the schema expects either.
        let data = match p.data {
            None => None,
            Some(serde_json::Value::String(s)) => {
                match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(v) if v.is_object() || v.is_array() => Some(v),
                    _ => {
                        log::warn!(
                            "log_diagnostic_entry: `data` was a non-JSON string \
                             ({} chars); dropping to None to satisfy schema",
                            s.len()
                        );
                        None
                    }
                }
            }
            Some(v) => Some(v),
        };

        let entry = database::schema::DiagnosticEntry {
            session_ref: database::schema::RecordId::new(
                database::schema::DIAGNOSTIC_SESSION_TABLE,
                p.session_id.clone(),
            ),
            category: database::schema::DiagnosticCategory::from_str(&p.category),
            title: p.title,
            detail: p.detail,
            data,
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
        // SurrealDB: `LIMIT` cannot follow `FETCH` in the same statement; omit
        // `LIMIT` here (service_number should be unique) and take the first row.
        let result: Option<serde_json::Value> = database::DATABASE
            .query(
                "SELECT * FROM service_order WHERE service_number == $sn FETCH computer, customer",
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

    #[tool(
        name = "scripts_list",
        description = "List every script available in the host Mastertech Scripts tab catalog (Tuneup / QC, Informational, Junkware Removal). Use the returned `category` + `script_name` values verbatim with scripts_run. Works whether or not the host is currently running — it's a static catalog."
    )]
    async fn scripts_list(
        &self,
        Parameters(_p): Parameters<ScriptsListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::scripts::categories::get_all_categories;
        use crate::scripts::ScriptCategory;

        let cats = get_all_categories();
        let mut out: Vec<serde_json::Value> = Vec::new();
        for cat_key in [
            ScriptCategory::Tuneup,
            ScriptCategory::Informational,
            ScriptCategory::JunkwareRemoval,
        ] {
            let cat_name = match cat_key {
                ScriptCategory::Tuneup => "Tuneup",
                ScriptCategory::Informational => "Informational",
                ScriptCategory::JunkwareRemoval => "JunkwareRemoval",
                _ => continue,
            };
            if let Some(scripts) = cats.get(&cat_key) {
                let items: Vec<serde_json::Value> = scripts
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "description": s.description,
                            "pass_criteria": s.pass_criteria,
                            "warning_criteria": s.warning_criteria,
                            "error_criteria": s.error_criteria,
                        })
                    })
                    .collect();
                out.push(serde_json::json!({
                    "category": cat_name,
                    "display_name": format!("{}", cat_key),
                    "scripts": items,
                }));
            }
        }

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "categories": out }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "scripts_run",
        description = "Run a single named script on the local host (Mastertech4.0 in egui or terminal mode). Sends the request over a crossbeam channel to the running Scripts tab, which dispatches to its existing handler and reports back when done. Returns success flag, summary message, and the log lines emitted during the run. Activation scripts (Webroot, SuperAnti, SEB) require a service_number; SEB additionally needs customer_email. Use scripts_list first to see exact script_name values."
    )]
    async fn scripts_run(
        &self,
        Parameters(p): Parameters<ScriptsRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::scripts::{script_run_request_sender, ScriptCategory, ScriptRunRequest};

        let category = match p.category.as_str() {
            "Tuneup" | "tuneup" | "Tuneup / QC" => ScriptCategory::Tuneup,
            "Informational" | "informational" => ScriptCategory::Informational,
            "JunkwareRemoval" | "junkware" | "Junkware Removal" => ScriptCategory::JunkwareRemoval,
            other => {
                return Err(to_internal(format!(
                    "Unknown category '{other}'. Expected one of: Tuneup, Informational, JunkwareRemoval."
                )));
            }
        };

        let request_id = uuid::Uuid::new_v4().to_string();
        let req = ScriptRunRequest {
            request_id: request_id.clone(),
            category,
            script_name: p.script_name.clone(),
            service_number: p.service_number.clone(),
            customer_email: p.customer_email.clone(),
        };

        let rx = register_pending_script_run(request_id.clone());

        script_run_request_sender()
            .send(req)
            .map_err(|e| to_internal(format!("Failed to enqueue script request: {e}")))?;

        let timeout = std::time::Duration::from_secs(p.timeout_secs.unwrap_or(600));
        let result = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                return Err(to_internal(
                    "Script result channel closed before a response arrived (host may have shut down)",
                ));
            }
            Err(_) => {
                if let Ok(mut map) = SCRIPT_RUN_PENDING.lock() {
                    map.remove(&request_id);
                }
                return Err(to_internal(format!(
                    "Timed out after {}s waiting for script '{}' to complete. The script may still be running on the host; check the Scripts tab log.",
                    timeout.as_secs(),
                    p.script_name
                )));
            }
        };

        Ok(CallToolResult::success(vec![Content::json(
            serde_json::to_value(&result).map_err(to_internal)?,
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "scripts_run_remote",
        description = "Run a named script on a REMOTE Mastertech client that is connected via the admin Web Console. Unlike scripts_run (which drives the LOCAL host), this sends Cmd::RunRemoteScripts over the existing admin WebSocket/TCP session to the target client and waits for results. Use for any QC / Tuneup script that must execute on the customer's machine, not on the admin machine. Requires an active Web Console session (check remote_egui_list_targets). Activation scripts (Webroot, SuperAnti, SEB) require service_number; SEB additionally needs customer_email."
    )]
    async fn scripts_run_remote(
        &self,
        Parameters(p): Parameters<ScriptsRunRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::Cmd;

        let service_number = p.service_number.clone().unwrap_or_default();
        let customer_email = p.customer_email.clone().unwrap_or_default();

        // Build the RunRemoteScripts command with a single named script.
        let cmd = Cmd::RunRemoteScripts {
            scripts: vec![crate::RemoteScriptItem {
                name: p.script_name.clone(),
                category: p.category.clone(),
                content: None,
            }],
            service_number: service_number.clone(),
            customer_email: customer_email.clone(),
        };
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

        // Register a pending oneshot for this script run.
        let (tx, rx) = tokio::sync::oneshot::channel::<RemoteScriptSession>();
        {
            let mut guard = REMOTE_SCRIPT_PENDING
                .lock()
                .map_err(|_| to_internal("REMOTE_SCRIPT_PENDING poisoned"))?;
            // Clear any stale accumulator from a previous run.
            if let Ok(mut accum) = REMOTE_SCRIPT_ACCUM.lock() {
                *accum = RemoteScriptSession::default();
            }
            *guard = Some((p.script_name.clone(), tx));
        }

        super::remote_egui_control::hub()
            .send_raw_binary(&p.connection_string, serialized)
            .map_err(to_internal)?;

        let timeout = std::time::Duration::from_secs(p.timeout_secs.unwrap_or(600));
        let session = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => {
                let _ = REMOTE_SCRIPT_PENDING.lock().map(|mut g| g.take());
                return Err(to_internal("Remote script channel closed unexpectedly"));
            }
            Err(_) => {
                let _ = REMOTE_SCRIPT_PENDING.lock().map(|mut g| g.take());
                return Err(to_internal(format!(
                    "Timed out after {}s waiting for remote script '{}' to complete.",
                    timeout.as_secs(),
                    p.script_name
                )));
            }
        };

        let overall_success = session.results.iter().all(|(_, s)| s == "Success" || s == "success");
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "script": p.script_name,
            "connection_string": p.connection_string,
            "success": overall_success,
            "results": session.results.iter().map(|(n, s)| serde_json::json!({"name": n, "status": s})).collect::<Vec<_>>(),
            "logs": session.logs,
        }))
        .map_err(to_internal)?]))
    }

    // ── Remote build workers (no local Rust toolchain required) ────────
    //
    // Slice 4: these three tools are now backed by the SurrealDB
    // `build_job` table. Workers subscribe via `LIVE SELECT` and write
    // results back, which means jobs survive Mastertech4.0 restarts and
    // are observable straight from the database. The WS-based
    // `builder_transport` module is retained in the codebase as a
    // fallback path; see `axum_server/src/routes/api/build/` for the
    // matching public HTTP surface.

    #[tool(
        name = "list_build_workers",
        description = "List `plugin_builder` workers currently visible in the SurrealDB `connected_client` table (rows with `client_kind = 'build_worker'` whose `last_update` is within the last 90 s — workers heartbeat every 30 s). Use these `connection_string`s as the optional `worker_connection_string` for `plugin_compile_remote`."
    )]
    async fn list_build_workers(
        &self,
        Parameters(_): Parameters<ListBuildWorkersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut response = database::DATABASE
            .query(
                "SELECT * FROM connected_client \
                 WHERE client_kind = 'build_worker' \
                   AND last_update > time::now() - 90s \
                 ORDER BY last_update DESC",
            )
            .await
            .map_err(|e| to_internal(format!("list workers query: {e}")))?;
        let workers: Vec<database::schema::ConnectedClient> = response
            .take(0)
            .map_err(|e| to_internal(format!("decode workers: {e}")))?;
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "count": workers.len(),
            "workers": workers.iter().map(|w| serde_json::json!({
                "connection_string": w.connection_string,
                "hostname": w.friendly_name,
                "record_id": format!("{}:{}", w.id.table, database::schema::RecordIdExt::key_string(&w.id)),
                "last_update": w.last_update.as_ref().map(|d| d.to_string()),
            })).collect::<Vec<_>>(),
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "plugin_compile_remote",
        description = "Dispatch a `cargo build --target <triple> --release` for a plugin to a remote `plugin_builder` worker by writing a row into the SurrealDB `build_job` table. Returns immediately with a `job_id`; poll `plugin_compile_status` to fetch the artifact bytes. **Auto-falls-back to local `plugin_compile` when no live `plugin_builder` workers are present** (rows with `client_kind = 'build_worker'` heartbeating within the last 90 s) — in that case the response carries `status: 'done'` directly and no polling is needed."
    )]
    async fn plugin_compile_remote(
        &self,
        Parameters(p): Parameters<PluginCompileRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = plugin_dir(&p.plugin_id);
        let lib_rs_path = dir.join("src").join("lib.rs");
        if !lib_rs_path.exists() {
            return Err(to_internal(format!(
                "No source found for plugin '{}'. Use plugin_source to write source first.",
                p.plugin_id
            )));
        }
        let cargo_toml_path = dir.join("Cargo.toml");
        if !cargo_toml_path.exists() {
            tokio::fs::write(&cargo_toml_path, plugin_cargo_toml(&p.plugin_id))
                .await
                .map_err(|e| to_internal(format!("write default Cargo.toml: {e}")))?;
        }

        let lib_rs = tokio::fs::read_to_string(&lib_rs_path)
            .await
            .map_err(|e| to_internal(format!("read lib.rs: {e}")))?;
        let cargo_toml = tokio::fs::read_to_string(&cargo_toml_path)
            .await
            .map_err(|e| to_internal(format!("read Cargo.toml: {e}")))?;

        let target = p.target.unwrap_or_else(|| "wasm32-wasip1".to_string());
        let profile = p.profile.unwrap_or_else(|| "release".to_string());

        // ── Worker-presence check + auto-fallback to local compile ──────
        //
        // The remote-build path is only useful when something is actually
        // claiming jobs.  If the SurrealDB `connected_client` table has no
        // rows with `client_kind = 'build_worker'` heartbeating within
        // 90 s, the job we'd write would sit `pending` forever (and the AI
        // would burn its retry budget polling `plugin_compile_status`).
        // Instead, when there's no worker AND `cargo` is available on the
        // host, transparently run the local compile inline and return the
        // result as a synthetic `done` job — the AI's deploy step works
        // unchanged because both paths populate the same ArtifactStore.
        let workers_resp = database::DATABASE
            .query(
                "SELECT count() FROM connected_client \
                 WHERE client_kind = 'build_worker' \
                   AND last_update > time::now() - 90s \
                 GROUP ALL",
            )
            .await;
        let live_worker_count: i64 = match workers_resp {
            Ok(mut r) => r
                .take::<Option<serde_json::Value>>(0)
                .ok()
                .flatten()
                .and_then(|v| v.get("count").and_then(|c| c.as_i64()))
                .unwrap_or(0),
            Err(e) => {
                log::warn!(
                    "plugin_compile_remote: worker-count probe failed: {e:?} \
                     — assuming 0 workers and falling back to local compile"
                );
                0
            }
        };
        if live_worker_count == 0
            && target == "wasm32-wasip1"
            && local_cargo_available().await
        {
            log::info!(
                "plugin_compile_remote: no live build_worker rows — \
                 transparently running local cargo compile for '{}'",
                p.plugin_id
            );
            let (success, stdout, stderr, artifact_size) =
                run_local_cargo_compile(&dir, &p.plugin_id, &self).await?;
            return Ok(CallToolResult::success(vec![Content::json(
                serde_json::json!({
                    "job_id": format!("local-fallback:{}", p.plugin_id),
                    "plugin_id": p.plugin_id,
                    "target": target,
                    "profile": profile,
                    "status": if success { "done" } else { "failed" },
                    "fell_back_to_local": true,
                    "reason": "No live plugin_builder workers; ran local cargo",
                    "artifact_bytes": artifact_size,
                    "stdout_tail": tail_n_lines(&stdout, 20),
                    "stderr_tail": tail_n_lines(&stderr, 40),
                    "next": if success {
                        "plugin_deploy or plugin_deploy_remote — artifact already in local store, no polling needed"
                    } else {
                        "fix source from stderr and retry plugin_compile_remote"
                    },
                }),
            )
            .map_err(to_internal)?]));
        }
        if live_worker_count == 0 && !local_cargo_available().await {
            return Err(to_internal(
                "plugin_compile_remote: no live plugin_builder workers AND \
                 no local `cargo` on PATH.  Start a plugin_builder (see \
                 `plugin_builder/Dockerfile` or `plugin_builder/k3s.yaml`) \
                 or install Rust + the wasm32-wasip1 target on this host.",
            ));
        }

        // Resolve a worker pin if the caller gave us one. Either a
        // connection_string (we look up the matching row) or a raw
        // record id (`connected_client:build_worker_alpha`) works.
        let assigned_worker_id = match p.worker_connection_string.as_deref() {
            None => None,
            Some(s) if s.contains(':') => Some(parse_record_id(s, "connected_client")),
            Some(cs) => {
                let mut response = database::DATABASE
                    .query("SELECT id FROM connected_client WHERE connection_string = $cs AND client_kind = 'build_worker' LIMIT 1")
                    .bind(("cs", cs.to_string()))
                    .await
                    .map_err(|e| to_internal(format!("resolve worker: {e}")))?;
                let rows: Vec<database::schema::ConnectedClient> = response
                    .take(0)
                    .map_err(|e| to_internal(format!("decode worker row: {e}")))?;
                let row = rows.into_iter().next().ok_or_else(|| {
                    to_internal(format!(
                        "No build_worker row with connection_string = '{cs}'. \
                         Run list_build_workers to see what's online."
                    ))
                })?;
                Some(row.id)
            }
        };

        let job = database::schema::BuildJob::create(
            &p.plugin_id,
            &cargo_toml,
            &lib_rs,
            &target,
            &profile,
            assigned_worker_id,
        )
        .await
        .map_err(|e| to_internal(format!("CREATE build_job: {e}")))?;

        let job_id_str = format!(
            "{}:{}",
            job.id.table,
            database::schema::RecordIdExt::key_string(&job.id)
        );
        Ok(CallToolResult::success(vec![Content::json(serde_json::json!({
            "job_id": job_id_str,
            "plugin_id": p.plugin_id,
            "target": target,
            "profile": profile,
            "next": "poll plugin_compile_status until status != 'pending'",
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "plugin_compile_status",
        description = "Poll a remote compile job started by `plugin_compile_remote`. Reads the SurrealDB `build_job` row by id and returns one of `pending` (queued), `claimed` (worker is building), `done` (compiled — bytes copied into the local ArtifactStore so `plugin_deploy` / `plugin_deploy_remote` work as usual), or `failed` (stderr included)."
    )]
    async fn plugin_compile_status(
        &self,
        Parameters(p): Parameters<PluginCompileStatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let rid = parse_record_id(&p.job_id, database::schema::BUILD_JOB_TABLE);
        let job = database::schema::BuildJob::get(&rid)
            .await
            .map_err(|e| to_internal(format!("SELECT build_job: {e}")))?
            .ok_or_else(|| {
                to_internal(format!(
                    "No build_job with id '{}'. Either the id is wrong or the job was deleted.",
                    p.job_id
                ))
            })?;

        let json = match job.status.as_str() {
            "pending" | "claimed" => serde_json::json!({
                "job_id": p.job_id,
                "status": job.status,
                "plugin_id": job.plugin_id,
                "claimed_worker_id": job.claimed_worker_id.as_ref().map(|r| format!("{}:{}", r.table, database::schema::RecordIdExt::key_string(r))),
                "elapsed_ms_since_created": (chrono::Utc::now() - chrono::DateTime::<chrono::Utc>::from(job.created_at.clone())).num_milliseconds(),
            }),
            "done" => {
                let bytes = job
                    .wasm_bytes
                    .as_ref()
                    .map(|b| (&b[..]).to_vec())
                    .unwrap_or_default();
                let size = bytes.len();
                self.try_lock_artifacts()?.store(&job.plugin_id, bytes);
                if p.forget_on_done.unwrap_or(false) {
                    let _: Result<Option<database::schema::BuildJob>, _> =
                        database::DATABASE.delete(rid.clone()).await;
                }
                serde_json::json!({
                    "job_id": p.job_id,
                    "status": "done",
                    "plugin_id": job.plugin_id,
                    "artifact_bytes": size,
                    "duration_ms": job.duration_ms,
                    "stderr_tail": tail_n_lines(&job.stderr, 40),
                    "next": "plugin_deploy or plugin_deploy_remote with this plugin_id",
                })
            }
            "failed" => serde_json::json!({
                "job_id": p.job_id,
                "status": "failed",
                "plugin_id": job.plugin_id,
                "duration_ms": job.duration_ms,
                "stderr": job.stderr,
            }),
            other => serde_json::json!({
                "job_id": p.job_id,
                "status": other,
                "plugin_id": job.plugin_id,
                "note": "unrecognized status value; schema may have evolved",
            }),
        };
        Ok(CallToolResult::success(vec![Content::json(json).map_err(to_internal)?]))
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

=== Plugin Deploy Preconditions (MUST READ) ===
`plugin_deploy` and `plugin_deploy_remote` need a compiled artifact in the local ArtifactStore first. If you see `No artifact for '<plugin_id>'. Run plugin_compile or plugin_emit_clock_wasm first.`, the artifact isn't loaded — you skipped a step. Pick ONE of these BEFORE calling any deploy tool:
  1. **Use a registry plugin** (preferred when one exists): `search_plugins` → `fetch_plugin` with the registry plugin_id. `fetch_plugin` populates the artifact store directly; no compile needed.
  2. **Compile locally**: `plugin_source` (write Rust source) → `plugin_compile`. Requires Rust + the `wasm32-wasip1` target on this host.
  3. **Compile remotely**: `plugin_source` → `plugin_compile_remote` (auto-falls-back to local compile when no `plugin_builder` workers are live) → `plugin_compile_status` until `status == 'done'`.
Only AFTER one of those three has populated the ArtifactStore is `plugin_deploy` / `plugin_deploy_remote` a valid call.

=== Plugin / Worker SurrealDB Tables — Canonical Names ===
When using `query_surrealdb` for plugin work, the ONLY valid tables are:
  - `plugin_registry`     — published plugins. Use `search_plugins` instead unless you really need raw SQL.
  - `build_job`           — remote-compile work queue (rows written by `plugin_compile_remote`).
  - `connected_client`    — every running Mastertech / plugin_builder process. Filter `WHERE client_kind = 'build_worker'` for build workers; use `list_build_workers` for the curated view.
  - `diagnostic_session` / `diagnostic_entry` — AI diagnostic work product; manage via the diagnostic tools (`create_diagnostic_session`, `log_diagnostic_entry`, …), NOT raw SQL.
**There is NO `client_plugin` table.** A query against it returns "The table 'client_plugin' does not exist". If you wanted "plugins installed on a connected client", call `list_plugins` against the remote MCP via `call_remote_plugin_tool` or read `plugin_registry` for what's been published.

=== Known Plugins in Registry ===
Always check search_plugins before building new plugins. Current registry (as of last sync):
- **com.mastertech.hw-diag** ("HW Diagnostics") — system_info, bsod_events, critical_events, whea_errors, disk_health, reliability_records, tdr_gpu_events, driver_errors, disk_errors, wer_hardware, list_software, uninstall_armoury_crate, uninstall_ryzen_master, download_ddu, check_ddu_status, find_ryzen_master, remove_ryzen_master_remnants, analyze_minidumps, night_light_status, display_connections, **webroot_license**, **sas_license** (CPS / Webroot + SuperAntiSpyware activation and days-remaining when those tools are published on the remote build). Use for GPU/display/BSOD/crash/Night Light diagnostics and CPS license checks.
- **com.mastertech.repair** ("System Repair") — dism_restore_health, sfc_scannow, uninstall_superantispyware, chkdsk_schedule, run_command (arbitrary PowerShell). Use for Windows system file repair.
- **com.mastertech.diagnostics** ("Diagnostics") — system_summary, top_processes, disk_info, recent_system_errors, recent_app_crashes, stopped_auto_services, network_info, startup_programs, wifi_status, wifi_event_logs, wifi_fix, find_uninstall_targets, uninstall_msi_software, cpu_power_health, crash_deep_dive, verify_fix, detect_hardware, burn_cpu, burn_memory, burn_disk, burn_combined, stability_report, stress_and_monitor, analyze_dump_files, disable_orphaned_drivers, kill_problematic_processes. General-purpose system health overview, stress testing, and crash analysis.
- **com.mastertech.status-reporter** — status_report (returns UTC clock from remote host, confirms plugin is live). Lightweight connectivity test.

When in doubt, call search_plugins with relevant keywords — the registry is the source of truth.

=== Diagnostic Prior-History Lookup (MUST do before diagnosing) ===
When performing diagnostics on a machine, ALWAYS gather prior history first.
This reveals repeat-visit patterns, previous fixes that failed, and known issues.

Step 1 — Identify the customer from the computer:
  SELECT VALUE customer FROM computer WHERE hostname = '<HOSTNAME>'
  Then: get_customer_details with the returned customer ID.

Step 2 — Pull tasks (tech notes) for this customer's machines:
  First try by customer link:
    SELECT * FROM task WHERE service_ticket.customer = customer:`<ID>`
  If that returns nothing, look up service orders for the customer:
    search_service_orders with the customer name.
  Then query tasks by each service number found:
    SELECT * FROM task WHERE service_number = '<SERVICE_NUMBER>'

Step 3 — Search PrestaShop for purchase/invoice history:
  search_prestashop_orders with the customer name or email.
  If PrestaShop returns order references tied to service numbers, pull tasks:
    SELECT * FROM task WHERE service_number = '<SERVICE_NUMBER>'

Step 4 — Search previous diagnostic sessions:
  search_diagnostics with the hostname and/or customer name.
  If results found, get_diagnostic_session for full details + entries.

Step 5 — Read prior findings before starting new diagnosis:
  Review all returned tasks, diagnostic entries, and order notes.
  Identify: repeat visits, previously attempted fixes, escalation notes
  (e.g. "if he comes back we need to replace GPU"), and unresolved items.
  Factor these into the current diagnosis — do not re-attempt known-failed fixes.

=== Service Context Identification (run BEFORE choosing a workflow) ===
Every connected client is in the shop for one of three reasons: a New Computer / QC build,
a Tuneup, or a Diagnostic. The order record + linked task tell you which one. Run these
steps before doing any work, and use the result to route to the correct workflow below.

Step 1 — Pull the order:
  get_service_order with the service number (visible in the Mastertech Scripts tab field
  scripts.service_number, or in TUR Sheet tur.service_number). The result includes:
    - doc_alias       — order_type string from PrestaShop (the primary classifier)
    - checkin_notes   — free-text notes from the check-in tech
    - customer / computer — already linked records

Step 2 — Pull the linked task(s) for tech-specific instructions:
  SELECT * FROM task WHERE service_ticket = ticket:`<SERVICE_NUMBER>`
  New computer builds almost always have a task assigned to the build tech with a
  `description` field that names the exact work (e.g. "Customer wants OneDrive removed,
  no LibreOffice, transfer data from old drive on the bench"). Read it carefully — it
  overrides the standard checklist for that build.

Step 2b — STALENESS + OWNERSHIP SANITY CHECK (MANDATORY — do this before any work):
  Cross-reference the order/task against today's date and the connected machine's identity.
  If ANY of the following mismatches exist, STOP and ask the user to confirm before
  proceeding — do not silently assume the order is correct:

  a) DATE GAP: order or task `created_at` is more than ~2 weeks before today's date.
     A new-computer QC shouldn't happen months after purchase; a tuneup shouldn't be
     linked to a ticket from a prior visit. Flag it: "This order is from <date> — is
     this the right ticket for today's work?"

  b) HOSTNAME MISMATCH: the `connected_client.computer` hostname does not match the
     computer linked on the service order. The customer may have traded in or swapped
     the machine since the order was created (e.g. traded a desktop for a laptop —
     the desktop got recertified and resold, but we are now connected to it under the
     original owner's name). Always verify: SELECT * FROM connected_client WHERE
     connection_string = '<connection_string>' and compare its `.computer` field against
     service_order.computer.

  c) CUSTOMER MISMATCH: the friendly_name on the connected_client or the customer
     linked to the computer record doesn't match the customer on the service order.
     Cross-check: connected_client → computer → customer vs. service_order → customer.
     If they differ, the machine may have been sold/transferred since the order was made.

  d) RECERTIFIED / RESOLD INDICATOR: if a previous service order for this computer
     shows "refurb", "recertified", "resold", "trade-in", or the machine's customer
     has changed since the most-recent prior order, treat that as a strong signal that
     the correct order belongs to the new customer/owner, not the previous one.

  If a mismatch is detected, search for the most recent active (non-Complete) task or
  service order whose computer matches the connected machine's hostname, then present
  the discrepancy clearly to the user and ask which order to proceed with. Never guess.

Step 3 — Classify by doc_alias (case-insensitive contains match):
  - "new", "build", "setup", "qc"           → New Computer / QC      (use the workflow below)
  - "tune", "tuneup", "clean", "maintenance" → Tuneup                 (use the workflow below)
  - "diag", "diagnostic", "issue", "repair", "no boot", "won't start"
                                            → Diagnostic             (use the workflow below)
  - Anything else / ambiguous                → read checkin_notes + task description; if
                                              still unclear, ASK THE USER. Never guess.

Step 4 — Read checkin_notes + task description and extract conditional flags before
running anything. Common signals to watch for:
  - "data transfer" / "transfer files" / "old drive"  → run Data Transfer
  - "install office" / "libreoffice" / "openoffice"   → Install LibreOffice
  - "bitlocker"                                       → Disable BitLocker (if currently on)
  - "bring your own key" / "customer key"             → use customer-supplied keys
  - "ram", "ssd", "hdd", "gpu", "battery"             → hardware swap; do that first
  - "no AV" / "they have <vendor>"                    → skip Webroot/SAS activation
  - Specific junkware named (Avast, McAfee, Norton, OneLaunch, etc.) → run those
    items from the Junkware Removal checklist explicitly

=== Diagnostic Session Workflow ===
When the Service Context Identification step routes to Diagnostic (or the operator
explicitly asks for diagnosis):
  1. Complete the Prior-History Lookup above.
  2. Resolve `customer_id` and `computer_id` BEFORE calling create_diagnostic_session — both are required.
     - Try connected_client.computer (and computer.customer) first if you have a connection_string.
     - Fall back to find_customer_by_email / find_customer_by_phone, then get_computer_details.
     - If you still cannot resolve, ASK THE USER. Never fabricate ids.
  3. Call create_diagnostic_session with the resolved ids (and optional task_id / service_order_id if a check-in exists).
  4. Call log_diagnostic_entry for each finding, action taken, or resolution. Use the
     allowed category vocabulary: finding, action, note, error, system_info,
     network_info, security_alert, performance_note, customer_note, recommendation.
  5. If the customer later checks in for service, call link_diagnostic_to_task to
     associate the session with the new task / service_order so it shows up in the
     task modal's Diagnostics tab.
  6. Call close_diagnostic_session with a summary when done.

=== New Computer / QC Workflow ===
When Service Context Identification routes to New Computer / QC. The Mastertech Scripts
tab on the connected client owns the actual execution — drive it via remote_egui (see the
Scripts Tab Navigation section below). Activation scripts (Webroot, SAS, SEB) REQUIRE the
service number to be entered in the Scripts tab field before clicking Run, otherwise they
short-circuit with "requires SO number" in the log.

Always run, in this order (Tuneup / QC checklist column unless noted):
   1. Run Prechecks                  (Informational column — connects Wi-Fi, aligns taskbar, scans network)
   2. Install Windows Updates        (may require multiple reboots; re-check after each cycle until clean)
   3. Activate Webroot               (needs SO number → CPS keys)
   4. Activate SuperAnti + Change SuperAntiSpyware settings
                                     (CHECK BOTH in the same Run; the tab detects the combo and
                                      runs them as one sequential install→configure flow)
   5. Activate SEB                   (needs SO number AND customer email)
   6. Disable Sleep / Hibernation
   7. Disable Startup Apps
   8. Disable Notifications
   9. Unpin Copilot
  10. Align Taskbar to left
  11. Change Timezone to Mountain
  12. Disable proxy settings
  13. Disable OneDrive Startup       (Junkware Removal column)
  14. Disable Edge Startup Boost     (Junkware Removal column)
  15. Run Webroot Scan
  16. Run SuperAntiSpyware Scan

Conditional, gated on task description / checkin_notes (see Service Context Identification Step 4):
  - Data Transfer                    — only when notes mention transfer / old drive / migration
  - Install LibreOffice              — only when explicitly requested
  - Disable BitLocker                — only if Informational shows BitLocker enabled
  - Run Junkware Category            — when prechecks/Informational flag PUPs, or notes name them
  - Uninstall Microsoft 365 / OneDrive / specific browsers — only when explicitly named

After execution, run the full Informational checklist as a verification pass:
  Is SuperEasyBackup installed? / Is Webroot installed? / Is SuperAntiSpyware installed? /
  Are there scheduled tasks for it? / Is Windows Activated? / Is Hibernation/Sleep enabled? /
  Any Recent Blue Screens? / When Was The Last Service Date? / Windows Version
Then create_diagnostic_session (category 'note' / 'system_info') summarizing what passed,
link_diagnostic_to_task to the build task, and close_diagnostic_session with status 'resolved'.

=== Tuneup Workflow ===
When Service Context Identification routes to Tuneup. Same execution surface (Scripts tab)
as New Computer / QC, but the goal is cleanup + verification rather than a full build.

Always run, in this order:
   1. Run Prechecks                  (Look up customer via tasks table, if no tasks, look up customer via service orders, 
                                     then lookup by prestashop order number if all else fails) So you have notes on what we are doing
                                     to the computer. verify its a QC, Tuneup, or Diagnostic.
   2. Check Updates                  (Tuneup / QC — check-only first; only escalate to
                                      "Install Windows Updates" if the customer's task notes
                                      ask for it OR the machine is significantly behind)
   3. Run the full Informational checklist FIRST to establish baseline:
        Is SuperEasyBackup installed?, Is Webroot installed?, Is SuperAntiSpyware installed?,
        Are there scheduled tasks for it?, Is Windows Activated?, Is Hibernation/Sleep enabled?,
        Any Recent Blue Screens?, When Was The Last Service Date?, Windows Version
   4. Run Junkware Category          (broad PUP scan / removal)
   5. Run SuperAntiSpyware Scan
   6. Run Webroot Scan
   7. Disable Sleep / Hibernation    (only if Informational showed it enabled)
   8. Disable Startup Apps
   9. Disable Notifications
  10. Unpin Copilot
  11. Align Taskbar to left
  12. Disable proxy settings
  13. Disable OneDrive Startup + Disable Edge Startup Boost   (Junkware Removal column)
  14. Empty recycle bin + clean tmp folders via execute_script with this PowerShell:
        Clear-RecycleBin -Force -ErrorAction SilentlyContinue
        Remove-Item "$env:TEMP\*" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item "C:\Windows\Temp\*" -Recurse -Force -ErrorAction SilentlyContinue

Conditional, gated on Informational results + checkin_notes:
  - Webroot not installed            → Activate Webroot
  - SAS not installed                → Activate SuperAnti + Change SuperAntiSpyware settings
  - SEB not installed                → Activate SEB
  - Specific junkware named in notes → run the matching Junkware Removal items
  - "data transfer" in notes         → Data Transfer
  - "office" in notes + Office not installed → Install LibreOffice (or installed Office per notes)

After execution, log a diagnostic_session entry per finding (category 'finding' for issues
removed, 'action' for scripts that ran, 'recommendation' for anything the customer should
follow up on), link_diagnostic_to_task to the tuneup task, then close_diagnostic_session.

=== Local Scripts Execution (admin machine only — do NOT use for QC on a customer's computer) ===
For the machine running this MCP server, prefer the dedicated script tools below
over `remote_egui_*` clicking. They drive the local Scripts tab (egui mode) or
terminal Scripts tab (ratatui mode) over a crossbeam channel and report results
back synchronously.

⚠️  CRITICAL — LOCAL vs. REMOTE DISTINCTION:
  scripts_run  → executes on the ADMIN machine (the machine running Mastertech/MCP).
                 NEVER use this to run QC steps on a customer's computer.
  scripts_run_remote → executes on a REMOTE CLIENT connected via the admin Web Console.
                 ALWAYS use this when running QC, Tuneup, or any activation script
                 on a customer's machine. Requires connection_string from
                 remote_egui_list_targets.

Tools:
- scripts_list — returns the catalog of every available script grouped by category
  (Tuneup, Informational, JunkwareRemoval). No host required; the catalog is static.
- scripts_run — runs ONE named script on the LOCAL admin machine. Args:
    category       : "Tuneup" | "Informational" | "JunkwareRemoval"
    script_name    : exact display name from scripts_list (e.g. "Activate Webroot")
    service_number : required for Activate Webroot, Activate SuperAnti, Activate SEB
    customer_email : required for Activate SEB
    timeout_secs   : default 600. Bump for Windows Updates / full AV scans.
  Returns: { request_id, success, message, logs[] }.
  ⚠️  Only use for admin-machine operations (e.g. testing, admin-side installs).
      For customer QC, use scripts_run_remote instead.
- scripts_run_remote — runs ONE named script on a REMOTE client over the admin
  WebSocket/TCP session. Same script catalog as scripts_run. Extra required arg:
    connection_string : from remote_egui_list_targets (e.g. "DESKTOP-HKBCJ74:ac4ebfe00")
  All other args (category, script_name, service_number, customer_email, timeout_secs)
  work identically to scripts_run. Returns { script, success, results[], logs[] }.
  Use this for ALL QC / Tuneup activation steps on customer machines.

Workflow integration (customer QC / New Computer build on a REMOTE client):
- Use scripts_run_remote for every QC step: Activate CPS, Activate SEB, Install Windows
  Updates, Disable OneDrive Startup, etc.
- For multi-script combos like Activate SuperAnti + Change SuperAntiSpyware settings,
  run as two back-to-back scripts_run_remote calls.
- Activation scripts require service_number; SEB also requires customer_email.
- Always call remote_egui_list_targets first to confirm the client is connected.

When to use remote_egui instead:
- The target machine is running the Mastertech egui UI and has frame capture enabled.
  For terminal-mode (ratatui) clients, use scripts_run_remote — not remote_egui.

=== Scripts Tab Navigation (remote_egui) ===
The Mastertech client's Scripts tab is the actual execution surface for both the
New Computer / QC and Tuneup workflows. Driving it from MCP:

  1. Open the tab:
       remote_egui_perform_steps with steps:
         click_anchor nav.menu.view → sleep_ms 450 → click_anchor nav.tab.scripts
       (View-menu tab anchors only exist while the menu is open — see Remote egui section.)
  2. Enter the service number into the Scripts tab field BEFORE selecting any activation
     script. Webroot / SuperAnti / SEB short-circuit without it.
  3. Each checklist column (Tuneup / QC, Informational, Junkware Removal) has clickable
     items. Toggle the desired ones, then click the Run button.
  4. Run is sequential: it walks selected items top-to-bottom. The "Activate SuperAnti" +
     "Change SuperAntiSpyware settings" combo is auto-detected — if both are checked in
     one run, the settings step waits for activation to finish; otherwise leave them
     unchecked together.
  5. Watch the script log area for completion messages and the checklist green-check
     state to confirm each item finished successfully.

Anchor keys for the Scripts tab (currently being added — see follow-up note below):
  - scripts.service_number           — the SO input field
  - scripts.run_btn                  — the Run button
  - scripts.tuneup.<slug>            — Tuneup / QC items (slug = item text lowercased,
                                       non-alphanumeric → '_', e.g. scripts.tuneup.activate_webroot)
  - scripts.junkware.<slug>          — Junkware Removal items
  - scripts.informational.<slug>     — Informational items

Until the Scripts tab fully registers all anchors, after opening the tab call
remote_egui_list_widget_anchors to see what is actually exposed, and fall back to
remote_egui_click with coordinates from remote_egui_get_last_frame_meta for any item
not yet anchored.

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
- create_diagnostic_session — start logging a diagnostic engagement (REQUIRES customer_id + computer_id).
- log_diagnostic_entry — append entries with structured category vocabulary + optional data.
- close_diagnostic_session — finalize with status and summary.
- link_diagnostic_to_task — retroactively link a session to an in-house task / service_order.
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
- TUR Sheet widgets (when that tab is visible): tur.service_number, tur.customer_name, tur.phone_number, tur.customer_email, tur.salesman, tur.tech, tur.checkin_notes, tur.recommendations, tur.get_prestashop_order (button — loads order from PrestaShop into the rest of the form).
- **TUR remote typing — service # vs recommendations**: The service order number belongs **only** in `tur.service_number`. Before sending `Text` with digits that look like an SO#, you **must** focus that field: `ClickAnchor` on `tur.service_number` with **placement `top_left`** (clicks inside the text cell; `center` can miss the editable area in a tight grid), then `SleepMs` 200–400, then `Text`. Only after the SO# is correct should you click `tur.recommendations` and type sales copy. If digits appear in the wrong box, click `tur.service_number` again with `top_left`, sleep, re-type the SO#, then fix recommendations.

=== TUR sheets (Trade-In / Upgrade / Repair) — purpose for the AI ===
- **What TUR is**: After diagnostics or bench work, the tech fills the **TUR Sheet** tab so **sales** can read specs + **Recommendations** and pitch **upgrades**, **trade-ins**, **parts for repair**, or a **new PC**. The recommendations block is the main handoff to sales — write for a salesperson, not for the customer ticket prose.
- **Workflow**: (1) Enter **Service #** in `tur.service_number`. (2) Click **Get PrestaShop Order** (`tur.get_prestashop_order`) — that pulls PrestaShop + linked data into the sheet (customer, products, etc.). (3) Fill **Recommendations** (`tur.recommendations`) with concrete, actionable upsell angles. (4) Only click **Submit TUR** when the human operator has approved — never submit autonomously unless explicitly asked.
- **Odoo / stock**: Before recommending a specific part (e.g. larger NVMe, RAM kit), use **search_odoo_inventory** with part codes or product names. If no row is returned, say in the recommendation that live Odoo must be checked — do not invent SKU availability.
- **Windows / lifecycle**: If the machine is still on **Windows 10**, recommend moving to **Windows 11** (when hardware qualifies). If already on Win11, do not push that angle.
- **Storage angle**: If system drive is nearly full (e.g. high % used), recommend a larger SSD/NVMe **with** an Odoo-backed part code when possible.
- **Gaming / GPU**: If Steam or games are present but GPU is integrated or weak, qualify discrete GPU or new-build upsell.
- **CPS (Webroot + SuperAntiSpyware)**: On a **connected remote** (`remote_egui_list_targets`), prefer **`call_remote_plugin_tool`** with `plugin_id` **com.mastertech.hw-diag** and tool names **`webroot_license`** and **`sas_license`** (empty `{}` args unless the tool schema says otherwise). Use `get_plugin_info` / `search_plugins` if tool names differ on that registry build. Fold **active/inactive** and **days remaining** from the JSON into recommendations. If tools are missing or error, say CPS status is unknown and still mention renewal if the customer may be due. If either product is **inactive** or has **fewer than 30 days** left, recommend **CPS renewal**.

=== View tabs (names match menu; add/close tab toggles dock) ===
- TUR Sheet — Service intake / walk-in form (customer, tech, notes, recommendations). Use for sales handoff after service/diag work.
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

/// Last `n` lines of `s`, joined with `\n`. Used to keep
/// `plugin_compile_status` payloads compact when a build prints
/// kilobytes of warnings before succeeding.
fn tail_n_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= n {
        s.to_string()
    } else {
        lines[lines.len() - n..].join("\n")
    }
}

/// Returns `true` iff `cargo` is on PATH AND `wasm32-wasip1` is an
/// installed target.  Used by the `plugin_compile_remote` auto-fallback
/// — if no live `plugin_builder` workers are present but this host has
/// the toolchain, we transparently run the compile here.
async fn local_cargo_available() -> bool {
    let cargo_ok = tokio::process::Command::new("cargo")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !cargo_ok {
        return false;
    }
    let targets = tokio::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .await;
    match targets {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.lines().any(|l| l.trim() == "wasm32-wasip1")
        }
        // If `rustup` isn't installed (e.g. system rust via apt), assume
        // the target is present and let cargo error out informatively
        // rather than gating early.
        _ => true,
    }
}

/// Runs the same `cargo build --target wasm32-wasip1 --release` that
/// `plugin_compile` runs, stores the resulting artifact in the
/// `ArtifactStore`, and returns `(success, stdout, stderr,
/// artifact_size)` so the remote-fallback caller can return a synthetic
/// `plugin_compile_status`-shaped response.
async fn run_local_cargo_compile(
    dir: &std::path::Path,
    plugin_id: &str,
    server: &PluginToolProvider,
) -> Result<(bool, String, String, usize), ErrorData> {
    let output = tokio::process::Command::new("cargo")
        .args([
            "build",
            "--target",
            "wasm32-wasip1",
            "--release",
            "--message-format=json",
        ])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .output()
        .await
        .map_err(|e| to_internal(format!("Failed to run cargo: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Ok((false, stdout, stderr, 0));
    }

    let crate_name = sanitize_id(plugin_id);
    let release_dir = dir.join("target").join("wasm32-wasip1").join("release");
    let primary = release_dir.join(format!("{}.wasm", crate_name.replace('-', "_")));
    let fallback = release_dir.join(format!("{crate_name}.wasm"));
    let bytes = if tokio::fs::try_exists(&primary).await.unwrap_or(false) {
        tokio::fs::read(&primary)
            .await
            .map_err(|e| to_internal(format!("Read artifact: {e}")))?
    } else {
        tokio::fs::read(&fallback).await.map_err(|e| {
            to_internal(format!(
                "Read artifact: {e} (tried {} and {})",
                primary.display(),
                fallback.display()
            ))
        })?
    };
    let size = bytes.len();
    server.try_lock_artifacts()?.store(plugin_id, bytes);
    Ok((true, stdout, stderr, size))
}

/// Parse a Surreal record id from an MCP-supplied string. Accepts either
/// the full `table:key` form or just the bare `key`, returning a record
/// id on the requested table in either case. Used by the diagnostic
/// tools to convert AI-supplied identifiers into the typed `RecordId`
/// the schema now requires.
///
/// **Critical**: only strip the *first* colon (the table prefix).  The
/// previous `split(':').last()` form stripped *every* colon, which
/// silently truncated keys that themselves contain colons — like the
/// canonical `computer:DESKTOP-HQAF13L:b57a7e8f9` (hostname:hash9) the
/// rest of the codebase uses for client/computer records.  An AI tool
/// call with that id would land in the DB as `computer:b57a7e8f9`
/// (bare hash), orphaning the row off the real client.  Exact
/// regression seen on diagnostic_session:91833f0d-… where the
/// `computer_id` came in as `computer:b57a7e8f9` while the
/// connected_client + computer table both use the full
/// `DESKTOP-HQAF13L:b57a7e8f9` key.
fn parse_record_id(s: &str, table: &'static str) -> database::schema::RecordId {
    // Only treat the first segment as a table prefix when it actually
    // looks like one (alphanumeric + underscore).  Otherwise the input
    // is a raw key whose first segment happens to contain a colon (e.g.
    // a hostname:hash9 key passed without the table prefix).
    let key = match s.split_once(':') {
        Some((prefix, rest)) if !prefix.is_empty()
            && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            // Looks like `table:key…`, strip only the prefix.
            rest.to_string()
        }
        _ => s.to_string(),
    };
    database::schema::RecordId::new(table, key)
}

// ─── TCP server ────────────────────────────────────────────────────────────────

/// Start the plugin MCP server on TCP port 9003.
pub async fn run_plugin_mcp_server(manager: Arc<RwLock<PluginManager>>) -> anyhow::Result<()> {
    use tokio::net::TcpListener;

    ensure_script_run_drainer_spawned();

    let addr = "127.0.0.1:9003";
    let listener = TcpListener::bind(addr).await?;
    log::info!("Plugin MCP Server listening on TCP {addr}");

    let provider = PluginToolProvider::new(manager);

    loop {
        tokio::select! {
            biased;
            _ = crate::wait_for_shutdown() => {
                log::info!("Plugin MCP TCP (:9003) -> shutdown signaled; stopping accept loop");
                return Ok(());
            }
            res = listener.accept() => {
                let (stream, client_addr) = res?;
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
    }
}

// ─── stdio server ──────────────────────────────────────────────────────────────

/// Start the plugin MCP server on the process's stdin/stdout.
///
/// Intended for Claude Desktop and other launcher-based MCP clients that spawn
/// the server as a child process and pipe JSON-RPC over its stdio. Unlike the
/// TCP/HTTP variants, stdio is a single in-process pipe — one session per
/// process — so when the peer closes its end this function returns.
///
/// # Stdout must stay clean
///
/// The MCP framing on this transport rides directly on stdout. **The global
/// logger must write to stderr** (env_logger's default), and nothing else in
/// the process may `println!` / write to stdout while this function is
/// running. A single stray byte on stdout will desync the client.
///
/// Because of that constraint, callers usually gate this behind a CLI flag
/// (e.g. `--mcp-stdio`) and skip the GUI/eframe path entirely when the flag
/// is present.
pub async fn run_plugin_mcp_server_stdio(manager: Arc<RwLock<PluginManager>>) -> anyhow::Result<()> {
    if let Err(e) = database::schema::define_bucket("plugins", "memory").await {
        log::warn!("Failed to define 'plugins' bucket (non-fatal): {e}");
    } else {
        log::info!("SurrealDB 'plugins' bucket initialized");
    }

    ensure_script_run_drainer_spawned();

    log::info!("Plugin MCP Server attaching to stdio (single session)");

    let provider = PluginToolProvider::new(manager);
    // `transport-async-rw` is already enabled; the (Stdin, Stdout) tuple
    // implements rmcp's IntoTransport via its AsyncRead/AsyncWrite impls, so
    // we don't need to pull in the `transport-io` feature just for the
    // `rmcp::transport::stdio()` helper.
    let transport = (tokio::io::stdin(), tokio::io::stdout());

    tokio::select! {
        biased;
        _ = crate::wait_for_shutdown() => {
            log::info!("Plugin MCP stdio -> shutdown signaled; exiting");
            Ok(())
        }
        served = rmcp::serve_server(provider, transport) => {
            match served {
                Ok(handle) => {
                    if let Err(e) = handle.waiting().await {
                        let msg = e.to_string();
                        if msg.contains("connection closed")
                            || msg.contains("Connection reset")
                            || msg.contains("broken pipe")
                        {
                            log::info!("Plugin MCP stdio peer disconnected.");
                        } else {
                            log::error!("Plugin MCP stdio session error: {e:?}");
                            return Err(e.into());
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    log::error!("Plugin MCP stdio: failed to serve session: {e:?}");
                    Err(e.into())
                }
            }
        }
    }
}

/// Streamable HTTP MCP (MCP spec 2025-06-18 / Cursor “HTTP” transport).
///
/// Cursor and similar clients must use `http://127.0.0.1:9004/mcp`, **not** TCP 9003.
pub async fn run_plugin_mcp_server_http(manager: Arc<RwLock<PluginManager>>) -> anyhow::Result<()> {
    use std::time::Duration;

    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager,
        StreamableHttpServerConfig, StreamableHttpService,
    };

    if let Err(e) = database::schema::define_bucket("plugins", "memory").await {
        log::warn!("Failed to define 'plugins' bucket (non-fatal): {e}");
    } else {
        log::info!("SurrealDB 'plugins' bucket initialized");
    }

    ensure_script_run_drainer_spawned();

    let addr = "127.0.0.1:9004";
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let mgr = manager.clone();

    // rmcp's LocalSessionManager defaults to a 5-minute idle keep-alive that
    // fatally kills the session worker as soon as no events flow. Streamable-HTTP
    // clients that speak the protocol natively (Cursor, web) just re-handshake,
    // but stdio shims like `uvx mcp-proxy` retry the SSE GET against the dead
    // session id forever and never re-`initialize`. Stretching the idle window
    // to 30 minutes keeps long-lived diagnostic sessions alive without giving
    // up the zombie-session safety net entirely.
    let mut session_manager = LocalSessionManager::default();
    session_manager.session_config.keep_alive = Some(Duration::from_secs(1800));

    let service = StreamableHttpService::new(
        move || Ok(PluginToolProvider::new(mgr.clone())),
        Arc::new(session_manager),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);

    log::info!(
        "Plugin MCP (Streamable HTTP) listening at http://{addr}/mcp — set Cursor MCP URL to this (not :9003 TCP)"
    );

    // `with_graceful_shutdown` lets us tear down the axum server when the
    // global shutdown signal fires, so the `#[tokio::main]` runtime drop after
    // `eframe::run_native` returns doesn't have to abort an in-flight HTTP
    // accept loop (which on Windows can stall process exit).
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            crate::wait_for_shutdown().await;
            log::info!("Plugin MCP HTTP (:9004) -> shutdown signaled; stopping axum server");
        })
        .await?;
    Ok(())
}
