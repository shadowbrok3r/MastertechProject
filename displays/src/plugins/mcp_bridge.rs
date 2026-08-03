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
//! **RemoteExec (elevated shell jobs owned by a connected client):**
//! `remote_exec_capabilities`, `remote_exec_arm`, `remote_exec_disarm`, `remote_exec_start`,
//! `remote_exec_tail`, `remote_exec_wait`, `remote_exec_signal`, `remote_exec_list` — for work no
//! named script or plugin tool can do. Unlike `call_remote_plugin_tool`, a job outlives the
//! PluginManager watchdog and the admin's connection, and reports a real exit code. Gated by the
//! client's consent banner ([`crate::remote_exec`]); replies arrive as `Cmd::RemotePluginToolResult`
//! under [`crate::remote_exec::NATIVE_REMOTE_EXEC_PLUGIN_ID`], so there is no separate result route.
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
        CallToolResult, ContentBlock, ErrorCode, ErrorData, Implementation, ProtocolVersion,
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
pub(crate) fn register_pending_request(request_id: String) -> oneshot::Receiver<(bool, String)> {
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
pub(crate) fn unregister_pending_request(request_id: &str) {
    if let Ok(mut map) = REMOTE_TOOL_PENDING.lock() {
        map.remove(request_id);
    }
}

// ─── Headless (MCP-triggered) crash-dump fetch routing ──────────────────────
//
// connection_string → (destination zip path, pending request_id). Set by
// `crash_dumps_fetch`; the admin receive loop opens a writer at the dest when
// the client's DownloadCrashDumps chunks arrive and resolves the request on
// completion. Keyed by connection_string because FileChunk wire frames carry
// no per-transfer id.
static HEADLESS_DUMP_FETCH: Lazy<std::sync::Mutex<HashMap<String, (std::path::PathBuf, String)>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

pub fn register_headless_dump_fetch(
    connection_string: String,
    dest: std::path::PathBuf,
    request_id: String,
) {
    if let Ok(mut m) = HEADLESS_DUMP_FETCH.lock() {
        m.insert(connection_string, (dest, request_id));
    }
}

/// Destination for a pending headless fetch, without removing it.
pub fn peek_headless_dump_fetch(connection_string: &str) -> Option<std::path::PathBuf> {
    HEADLESS_DUMP_FETCH
        .lock()
        .ok()
        .and_then(|m| m.get(connection_string).map(|(p, _)| p.clone()))
}

/// Remove and return a pending headless fetch `(dest, request_id)`.
pub fn take_headless_dump_fetch(connection_string: &str) -> Option<(std::path::PathBuf, String)> {
    HEADLESS_DUMP_FETCH
        .lock()
        .ok()
        .and_then(|mut m| m.remove(connection_string))
}

/// Connection strings with a driver_snapshot_take in progress, so two
/// concurrent takes for one client can't race for the same result row.
static SNAPSHOT_INFLIGHT: Lazy<std::sync::Mutex<std::collections::HashSet<String>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// RAII: clears the in-flight marker for a connection on every exit path.
struct SnapshotInflightGuard {
    cs: String,
}
impl Drop for SnapshotInflightGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = SNAPSHOT_INFLIGHT.lock() {
            set.remove(&self.cs);
        }
    }
}

/// Default admin-side destination for pulled files: the user's Downloads.
fn default_download_dir() -> std::path::PathBuf {
    std::env::var("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Downloads"))
        .unwrap_or_else(|_| std::env::temp_dir())
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

/// Ingest a locally-produced dump-triage payload into fleet crash intel.
/// `link_connection_string` attributes the sightings to that client and its
/// open diagnostic session / service task. Guaranteed-logging path for local
/// `minidump_analyze`; best-effort, returns a summary for the tool response.
async fn ingest_local_triage(
    payload: &serde_json::Value,
    link_connection_string: Option<&str>,
) -> serde_json::Value {
    use database::schema::crash_intel::{
        parse_kernel_triage_payload, CrashSignature, SightingContext,
    };
    let crashes = parse_kernel_triage_payload(payload);
    if crashes.is_empty() {
        return serde_json::json!({ "recorded": 0, "note": "no bugcheck parsed" });
    }
    let links = match link_connection_string {
        Some(cs) => super::crash_intel_hooks::resolve_sighting_links(cs, None).await,
        None => super::crash_intel_hooks::SightingLinks::default(),
    };
    let mut recorded = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for parsed in &crashes {
        let dump_kind = parsed
            .triage
            .as_ref()
            .and_then(|t| t.get("dump_type_name").and_then(|v| v.as_str()))
            .map(|dt| if dt.contains("live") { "livekernel" } else { "minidump" })
            .unwrap_or("minidump")
            .to_string();
        let ctx = SightingContext {
            connection_string: link_connection_string.map(str::to_string),
            computer: links.computer.clone(),
            session_ref: links.session_ref.clone(),
            task_ref: links.task_ref.clone(),
            dump_kind,
        };
        match CrashSignature::ingest(parsed, &ctx).await {
            Ok(_) => recorded += 1,
            Err(e) => errors.push(e.to_string()),
        }
    }
    serde_json::json!({
        "recorded": recorded,
        "errors": errors,
        "table": "crash_sighting",
        "session_ref": links.session_ref.as_ref().map(RecordIdExt::key_string),
        "task_ref": links.task_ref.as_ref().map(RecordIdExt::key_string),
    })
}

// ─── Entity link validation (MCP ↔ operator modal) ───────────────────────────

use crate::plugins::entity_link_pending::{
    entity_link_ui_active, register_entity_link_resolution, EntityLinkOutcome, EntityLinkRequest,
};
use database::schema::entity_link::{repair_connection_links, validate_link_bundle, LinkBundle};
use database::schema::RecordIdExt;
use std::time::Duration;

/// Parse an id param, treating blank as "infer from connection_string".
fn optional_record_id(input: &str, table: &'static str) -> Option<database::schema::RecordId> {
    let trimmed = input.trim();
    (!trimmed.is_empty()).then(|| parse_record_id(trimmed, table))
}

async fn resolve_entity_links_mcp(
    connection_string: Option<String>,
    customer_id_str: &str,
    computer_id_str: &str,
) -> Result<(database::schema::RecordId, database::schema::RecordId), ErrorData> {
    let bundle = LinkBundle {
        connection_string: connection_string.clone(),
        customer_id: optional_record_id(customer_id_str, database::schema::CUSTOMER_TABLE),
        computer_id: optional_record_id(computer_id_str, database::schema::COMPUTER_TABLE),
    };
    let validation = validate_link_bundle(&bundle).await;
    if validation.ok {
        if let (Some(cust), Some(comp)) = (
            validation.resolved_customer_id.clone(),
            validation.resolved_computer_id.clone(),
        ) {
            return Ok((cust, comp));
        }
    }

    if entity_link_ui_active() {
        let request_id = uuid::Uuid::new_v4().to_string();
        let rx = register_entity_link_resolution(EntityLinkRequest {
            request_id,
            connection_string,
            customer_id: customer_id_str.to_string(),
            computer_id: computer_id_str.to_string(),
            issues: validation.issues.clone(),
        });
        match tokio::time::timeout(Duration::from_secs(900), rx).await {
            Ok(Ok(EntityLinkOutcome::Resolved {
                customer_id,
                computer_id,
            })) => Ok((
                parse_record_id(&customer_id, database::schema::CUSTOMER_TABLE),
                parse_record_id(&computer_id, database::schema::COMPUTER_TABLE),
            )),
            Ok(Ok(EntityLinkOutcome::Cancelled { reason })) => Err(ErrorData::invalid_params(
                format!("entity link cancelled: {reason}"),
                None,
            )),
            Ok(Err(_)) => Err(ErrorData::invalid_params(
                "entity link channel closed".to_string(),
                None,
            )),
            Err(_) => Err(ErrorData::invalid_params(
                "entity link resolution timed out (15 min)".to_string(),
                None,
            )),
        }
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "entity link validation failed: {:?}. Open the Mastertech displays UI for \
                 the blocking repair modal, or call repair_entity_links / \
                 validate_connection_links first.",
                validation.issues
            ),
            Some(
                serde_json::json!({
                    "issues": validation.issues,
                    "resolution_hint": "Open displays UI or call repair_entity_links(connection_string)",
                })
                .into(),
            ),
        ))
    }
}

/// Parse a record id param and require that the row exists.
async fn require_record(
    input: &str,
    table: &'static str,
    param_name: &str,
) -> Result<database::schema::RecordId, ErrorData> {
    database::schema::entity_link::resolve_record_id(input, table)
        .await
        .map_err(|_| {
            ErrorData::invalid_params(
                format!("{param_name} '{input}' does not match an existing {table} record"),
                None,
            )
        })
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

// One store shared by every transport and session; the streamable-HTTP factory
// constructs a fresh PluginToolProvider per session, which must not reset artifacts.
static GLOBAL_ARTIFACTS: Lazy<Arc<Mutex<ArtifactStore>>> =
    Lazy::new(|| Arc::new(Mutex::new(ArtifactStore::new())));

// 1s-cadence sampler shared by telemetry_snapshot and stress_scenario_run; started on first use.
static TELEMETRY_AGENT: Lazy<Arc<stress_runner::TelemetryAgent>> =
    Lazy::new(|| Arc::new(stress_runner::TelemetryAgent::start(1000)));

// ─── Pre-boot direct link (MCP → UEFI firmware over the :9209 socket) ──────────

/// Handle to the admin console's direct-link hub, installed once the console
/// starts its listener. `None` until then; every preboot tool reports that.
static PREBOOT_HUB: Lazy<std::sync::Mutex<Option<crate::tabs::admin_console::preboot_direct::DirectHub>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

/// Publish the console's hub so the preboot MCP tools can reach the firmware.
pub fn set_preboot_hub(hub: crate::tabs::admin_console::preboot_direct::DirectHub) {
    if let Ok(mut g) = PREBOOT_HUB.lock() {
        *g = Some(hub);
    }
}

fn preboot_hub() -> Result<crate::tabs::admin_console::preboot_direct::DirectHub, ErrorData> {
    PREBOOT_HUB
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .ok_or_else(|| {
            ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                "pre-boot direct hub not started; open the Admin Console so it binds :9209".to_string(),
                None,
            )
        })
}

/// Map a tool-supplied key name to the firmware's lossy key code.
fn parse_pb_key(s: &str) -> Option<tcp_protocol::preboot::PbKeyCode> {
    use tcp_protocol::preboot::PbKeyCode as K;
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(K::Char(c));
    }
    let lower = s.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=12).contains(&n).then_some(K::F(n));
    }
    Some(match lower.as_str() {
        "enter" | "return" => K::Enter,
        "esc" | "escape" => K::Esc,
        "backspace" => K::Backspace,
        "tab" => K::Tab,
        "up" => K::Up,
        "down" => K::Down,
        "left" => K::Left,
        "right" => K::Right,
        "home" => K::Home,
        "end" => K::End,
        "pageup" | "pgup" => K::PageUp,
        "pagedown" | "pgdn" => K::PageDown,
        "delete" | "del" => K::Delete,
        "insert" | "ins" => K::Insert,
        "space" => K::Char(' '),
        _ => return None,
    })
}

/// Flatten a decoded firmware frame into one string per terminal row.
fn preboot_frame_lines(f: &tcp_protocol::preboot::PreBootFrame) -> Vec<String> {
    let cols = f.cols.max(1) as usize;
    f.cells
        .chunks(cols)
        .map(|row| row.iter().map(|c| c.symbol.as_str()).collect::<String>().trim_end().to_string())
        .collect()
}

// ─── Plugin store directory ────────────────────────────────────────────────────

pub(crate) fn plugin_store_root() -> PathBuf {
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
edition = "2024"

[workspace]

[lib]
crate-type = ["cdylib"]

[dependencies]
mtech-plugin-sdk = {{ path = "../_mtech_sdk_vendor" }}
facet = "=0.46.5"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[profile.release]
opt-level = "z"
lto = true
strip = true
panic = "abort"
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
            artifacts: GLOBAL_ARTIFACTS.clone(),
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
pub struct PrebootListParams {}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PrebootScreenParams {
    #[schemars(description = "Firmware serial, from preboot_list_clients")]
    pub serial: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PrebootStreamParams {
    #[schemars(description = "Firmware serial, from preboot_list_clients")]
    pub serial: String,
    #[schemars(description = "true starts frame streaming, false stops it")]
    pub stream: bool,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PrebootInputParams {
    #[schemars(description = "Firmware serial, from preboot_list_clients")]
    pub serial: String,
    #[schemars(
        description = "Key to send: a single character, or one of enter, esc, backspace, tab, \
                       up, down, left, right, home, end, pageup, pagedown, delete, insert, f1-f12"
    )]
    pub key: String,
    #[schemars(description = "Hold Ctrl (default false)")]
    pub ctrl: Option<bool>,
    #[schemars(description = "Hold Alt (default false)")]
    pub alt: Option<bool>,
    #[schemars(description = "Hold Shift (default false)")]
    pub shift: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PrebootTypeParams {
    #[schemars(description = "Firmware serial, from preboot_list_clients")]
    pub serial: String,
    #[schemars(description = "Literal text to type, one character key per char")]
    pub text: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PrebootRunPluginParams {
    #[schemars(description = "Firmware serial, from preboot_list_clients")]
    pub serial: String,
    #[schemars(
        description = "Registry plugin id or an http URL the firmware fetches. \
                       Empty runs the firmware's embedded demo plugin."
    )]
    pub source: Option<String>,
    #[schemars(description = "Tool name to invoke; empty picks the plugin's first advertised tool")]
    pub tool: Option<String>,
    #[schemars(description = "JSON-encoded argument string passed to the plugin tool")]
    pub args: Option<String>,
    #[schemars(description = "How long to wait for the firmware's result (default 30000ms)")]
    pub timeout_ms: Option<u64>,
}

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
    #[serde(default, deserialize_with = "deserialize_lenient_args")]
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
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
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
    #[serde(default, deserialize_with = "deserialize_lenient_args")]
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
pub struct RemoteChannelHealthParams {
    #[schemars(description = "Web Console connection_string of the remote client (from remote_egui_list_targets).")]
    pub connection_string: String,
    #[schemars(description = "Per-probe timeout in seconds (default 5, clamped to 1-30).")]
    pub probe_timeout_secs: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct TelemetrySnapshotParams {
    #[schemars(description = "Wait this many ms for a fresh sample when the agent just started (default 1200, max 5000).")]
    pub warmup_ms: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct TelemetrySnapshotRemoteParams {
    #[schemars(description = "Web Console connection_string of the remote client (from remote_egui_list_targets).")]
    pub connection_string: String,
    #[schemars(description = "Milliseconds the client may wait for its sampler's first populated tick (default 3000, clamped 500-15000).")]
    pub warmup_ms: Option<u64>,
}

/// String schema enumerating the reflected stressor labels.
fn wire_stressor_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let labels: Vec<&'static str> = stress_runner::Stressor::labels().collect();
    let description = stress_runner::Stressor::wire_description();
    schemars::json_schema!({
        "type": "string",
        "enum": labels,
        "description": description,
    })
}

#[derive(Deserialize, Debug, Serialize, JsonSchema, Clone)]
#[schemars(inline)]
pub struct ScenarioStageParam {
    #[schemars(schema_with = "wire_stressor_schema")]
    pub stressor: String,
    #[schemars(description = "Stage length in seconds (1-1800)")]
    pub duration_secs: u64,
    #[schemars(description = "Worker threads; 0 = logical CPU count")]
    #[serde(default)]
    pub threads: usize,
    #[schemars(description = "Heap cap per memory worker in MiB (default 256)")]
    pub memory_cap_mb: Option<u64>,
    #[schemars(description = "Temp-file size for the disk stressor in MiB (default 512)")]
    pub disk_file_mb: Option<u64>,
    #[schemars(description = "Stage label (defaults to the stressor name)")]
    pub label: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct StressRunsReapParams {
    #[schemars(description = "Reap runs whose started_at is older than now minus this many seconds (default 3600, min 600).")]
    pub grace_secs: Option<u64>,
    #[schemars(description = "Only reap runs for this hostname.")]
    pub hostname: Option<String>,
    #[schemars(description = "Preview the rows without mutating (default false).")]
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct StressScenarioRunParams {
    #[schemars(description = "Ordered stages; each runs one stressor for duration_secs. 1-16 stages, total runtime capped at 7200s.")]
    pub stages: Vec<ScenarioStageParam>,
    #[schemars(description = "Optional wall-clock cap in seconds for the whole scenario.")]
    pub total_wall_secs: Option<u64>,
    #[schemars(description = "With total_wall_secs set, loop the stage list until the wall cap.")]
    #[serde(default)]
    pub repeat_until_total: bool,
    #[schemars(description = "Service order number for stress_test_run.service_order linkage (e.g. '2147605').")]
    pub service_number: Option<String>,
    #[schemars(description = "Diagnostic session id to link as session_ref.")]
    pub diagnostic_session_id: Option<String>,
    #[schemars(description = "Preset label recorded on the run (default 'mcp:scenario-v1').")]
    pub preset_label: Option<String>,
    #[schemars(description = "Free-form notes recorded on the run.")]
    pub notes: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct StressConcurrentRunParams {
    #[schemars(description = "Lanes that run AT THE SAME TIME (e.g. cpu + memory + gpu — OCCT-style combined load). 1-8 lanes. Each lane's threads default to an auto-budget across the core pool; per-lane duration_secs is IGNORED (the run uses the shared duration_secs below).")]
    pub lanes: Vec<ScenarioStageParam>,
    #[schemars(description = "How long to run all lanes together, in seconds (1-7200).")]
    pub duration_secs: u64,
    #[schemars(description = "Service order number for stress_test_run.service_order linkage (e.g. '2147605').")]
    pub service_number: Option<String>,
    #[schemars(description = "Diagnostic session id to link as session_ref.")]
    pub diagnostic_session_id: Option<String>,
    #[schemars(description = "Preset label recorded on the run (default 'mcp:concurrent-v1').")]
    pub preset_label: Option<String>,
    #[schemars(description = "Free-form notes recorded on the run.")]
    pub notes: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct StressScenarioRunRemoteParams {
    #[schemars(description = "Web Console room id — same as ConnectedClient.connection_string; admin must be connected.")]
    pub connection_string: String,
    #[schemars(description = "Ordered stages; each runs one stressor for duration_secs. 1-16 stages, total runtime capped at 7200s.")]
    pub stages: Vec<ScenarioStageParam>,
    #[schemars(description = "Optional wall-clock cap in seconds for the whole scenario.")]
    pub total_wall_secs: Option<u64>,
    #[schemars(description = "With total_wall_secs set, loop the stage list until the wall cap.")]
    #[serde(default)]
    pub repeat_until_total: bool,
    #[schemars(description = "Service order number for stress_test_run.service_order linkage (e.g. '2147605'). REQUIRED.")]
    pub service_number: Option<String>,
    #[schemars(description = "Diagnostic session id to link as session_ref. Auto-resolved from the open session for connection_string when omitted.")]
    pub diagnostic_session_id: Option<String>,
    #[schemars(description = "Preset label recorded on the run (default 'mcp:scenario-remote-v1').")]
    pub preset_label: Option<String>,
    #[schemars(description = "Free-form notes recorded on the run.")]
    pub notes: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct StressConcurrentRunRemoteParams {
    #[schemars(description = "Web Console room id — same as ConnectedClient.connection_string; admin must be connected.")]
    pub connection_string: String,
    #[schemars(description = "Lanes that run AT THE SAME TIME (e.g. cpu + memory + gpu — OCCT-style combined load). 1-8 lanes. Each lane's threads default to an auto-budget across the core pool; per-lane duration_secs is IGNORED (the run uses the shared duration_secs below).")]
    pub lanes: Vec<ScenarioStageParam>,
    #[schemars(description = "How long to run all lanes together, in seconds (1-7200).")]
    pub duration_secs: u64,
    #[schemars(description = "Service order number for stress_test_run.service_order linkage (e.g. '2147605'). REQUIRED.")]
    pub service_number: Option<String>,
    #[schemars(description = "Diagnostic session id to link as session_ref. Auto-resolved from the open session for connection_string when omitted.")]
    pub diagnostic_session_id: Option<String>,
    #[schemars(description = "Preset label recorded on the run (default 'mcp:concurrent-remote-v1').")]
    pub preset_label: Option<String>,
    #[schemars(description = "Free-form notes recorded on the run.")]
    pub notes: Option<String>,
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
pub struct EguiInspectScreenshotParams {
    #[schemars(description = "Output resolution in pixels-per-point (default 1.0 = logical-point size).")]
    #[serde(default)]
    pub pixels_per_point: Option<f32>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct EguiInspectClickParams {
    #[schemars(description = "X in logical points (read from a tree node's bounds center).")]
    pub x: f32,
    #[schemars(description = "Y in logical points.")]
    pub y: f32,
    #[schemars(description = "primary|secondary|middle|extra1|extra2 (default primary).")]
    #[serde(default)]
    pub button: Option<String>,
    #[schemars(description = "Double-click if true.")]
    #[serde(default)]
    pub double: bool,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct EguiInspectTypeParams {
    #[schemars(description = "Text to type into the focused widget.")]
    pub text: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct EguiInspectKeyParams {
    #[schemars(description = "egui key name, e.g. Enter, Tab, Escape, ArrowDown, Backspace, A.")]
    pub key: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteEguiTypeParams {
    #[schemars(description = "Web Console room id.")]
    pub connection_string: String,
    #[schemars(description = "Unicode text to inject as egui Text events (focused widget receives input).")]
    pub text: String,
}

/// One step for [`remote_egui_perform_steps`]. Use tag `"step"` (snake_case values).
// Inlined so clients that strip $defs still see the full step schema.
#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[schemars(inline)]
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
#[schemars(inline)]
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
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ListRegistryPluginsParams {
    #[schemars(description = "Max plugins to return, newest-updated first. Default 200 (the registry is small), cap 1000.")]
    pub limit: Option<u32>,
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
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
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

#[derive(Deserialize, Debug, Clone, Serialize, JsonSchema)]
pub struct ValidateConnectionLinksParams {
    #[schemars(description = "connected_client.connection_string (HOST:hash9)")]
    pub connection_string: String,
    #[schemars(description = "Optional customer id to validate. Omit to use the customer the connected_client (or its computer) already points at. Accepts `customer:key`, bare key, or SurrealQL `customer:`key`` (backticks when key contains `:`).")]
    pub customer_id: Option<String>,
    #[schemars(description = "Optional computer id to validate. Omit to use the canonical `computer:HOST:hash9` for connection_string. Accepts `computer:key`, bare key, or SurrealQL `computer:`key`` (backticks when key contains `:`).")]
    pub computer_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Serialize, JsonSchema)]
pub struct RepairEntityLinksParams {
    #[schemars(description = "connected_client.connection_string to repair")]
    pub connection_string: String,
}

#[derive(Deserialize, Debug, Clone, Serialize, JsonSchema)]
pub struct LinkConnectedClientParams {
    #[schemars(description = "connected_client.connection_string (HOST:hash9) to link")]
    pub connection_string: String,
    #[schemars(description = "Customer id to link. Accepts `customer:key`, bare key, or SurrealQL `customer:`key``.")]
    pub customer_id: String,
    #[schemars(description = "Optional friendly_name for the client row (e.g. 'Kellie Boisse - 2147807'). Omit to leave any existing name unchanged.")]
    pub friendly_name: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Serialize, JsonSchema)]
pub struct CreateDiagnosticSessionParams {
    #[schemars(description = "Web Console connection_string of the client being diagnosed")]
    pub connection_string: String,
    #[schemars(description = "Hostname of the machine being diagnosed")]
    pub hostname: String,
    #[schemars(description = "REQUIRED. Customer record id — `customer:197987`, bare `197987`, or SurrealQL `customer:`197987`` / `customer:`DESKTOP-HQAF13L:b57a7e8f9`` when copied from query results. Backticks are stripped automatically.")]
    pub customer_id: String,
    #[schemars(description = "REQUIRED. Computer record id — canonical `computer:HOSTNAME:hash9`, bare key, or SurrealQL backtick-quoted form from SurrealDB. Backticks are stripped automatically.")]
    pub computer_id: String,
    #[schemars(description = "Optional task record id (`task:key` or SurrealQL `task:`key``).")]
    pub task_id: Option<String>,
    #[schemars(description = "Optional service order record id (`service_order:key` or SurrealQL quoted form).")]
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
    #[schemars(description = "Session ID to update (UUID string, or `diagnostic_session:`uuid`` from SurrealDB).")]
    pub session_id: String,
    #[schemars(description = "Task record id (`task:key` or SurrealQL quoted form).")]
    pub task_id: Option<String>,
    #[schemars(description = "Optional service order record id (`service_order:key` or SurrealQL quoted form).")]
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
    #[serde(default, deserialize_with = "de_plugins_used")]
    pub plugins_used: Option<Vec<PluginUsageRefParam>>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CreateAiTaskParams {
    #[schemars(description = "Session ID of the diagnostic session proposing the hands-on work. Omit to auto-resolve from connection_string via the active-session registry.")]
    pub session_id: Option<String>,
    #[schemars(description = "Web Console connection_string — alternative to session_id when a session is active for this client.")]
    pub connection_string: Option<String>,
    #[schemars(description = "Concrete hands-on steps for the technician, in order (1-30). Each becomes one checkbox. Be specific and actionable, e.g. 'Disable XMP in BIOS (JEDEC defaults)' not 'check BIOS'.")]
    pub steps: Vec<String>,
    #[schemars(description = "SHORT summary of the work ONLY — 3-6 words. The card automatically prefixes '{customer} - {service#}', so provide JUST the summary: do NOT include the customer name, service number, or hostname. Good: 'Clear Device Manager errors', 'Reseat RAM + retest', 'Replace SATA cable'. Bad: 'DESKTOP-XYZ - clear 3 Device Manager yellow-bangs (Dell Precision 5540)'. Default: 'Hands-on work needed'.")]
    pub title: Option<String>,
    #[schemars(description = "Task record id (`task:key`) to attach to. Optional — defaults to the session's task_ref, and if that is unset it auto-resolves the task from the session's (or the connection's) open service order and links it to the session. Only pass this to override that resolution.")]
    pub task_id: Option<String>,
    #[schemars(description = "Explicit assignee override (user email or exact name). Default resolution: service ticket's technician, else the task's assignee.")]
    pub assignee_email: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct AddAiTaskStepsParams {
    #[schemars(description = "AI task record id (`ai_task:key` or bare key) returned by create_ai_task")]
    pub ai_task_id: String,
    #[schemars(description = "Additional hands-on steps to append (1-30). Reopens the AI task and re-notifies the technician.")]
    pub steps: Vec<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct GetAiTaskStatusParams {
    #[schemars(description = "AI task record id (`ai_task:key` or bare key). Either this or session_id is required.")]
    pub ai_task_id: Option<String>,
    #[schemars(description = "Diagnostic session ID — resolves the newest non-closed AI task on that session.")]
    pub session_id: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct EditAiTaskItemParams {
    #[schemars(description = "AI task item record id (`ai_task_item:key` or bare key) — from get_ai_task_status items[].id or create_ai_task item_ids[].")]
    pub item_id: String,
    #[schemars(description = "New step text: a short, imperative, self-contained physical action (same rules as create_ai_task steps).")]
    pub text: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoveAiTaskItemParams {
    #[schemars(description = "AI task item record id (`ai_task_item:key` or bare key) to delete — from get_ai_task_status items[].id or create_ai_task item_ids[].")]
    pub item_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CrashIntelSearchParams {
    #[schemars(description = "Search term matched against module, bugcheck code, and bugcheck name. Omit for the most recently seen signatures.")]
    pub query: Option<String>,
    #[schemars(description = "Max signatures to return (default 20)")]
    pub limit: Option<u32>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CrashIntelSignatureParams {
    #[schemars(description = "Bugcheck code — '0x133', '133', or 'DPC_WATCHDOG_VIOLATION (133)'")]
    pub bugcheck_code: String,
    #[schemars(description = "Faulting module, e.g. 'rtwlane.sys'")]
    pub module: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CrashVerdictRecordParams {
    #[schemars(description = "Bugcheck code — '0x133', '133', or 'DPC_WATCHDOG_VIOLATION (133)'")]
    pub bugcheck_code: String,
    #[schemars(description = "Faulting module, e.g. 'rtwlane.sys'")]
    pub module: String,
    #[schemars(description = "Diagnosis: what this crash class actually is")]
    pub verdict: String,
    #[schemars(description = "Remediation that resolved it (if known)")]
    pub fix: Option<String>,
    #[schemars(description = "Confidence: low | medium | high | confirmed (default medium)")]
    pub confidence: Option<String>,
    #[schemars(description = "Tech name or AI identifier recording this verdict")]
    pub author: Option<String>,
    #[schemars(description = "Source: tech | ai | autopilot (default ai)")]
    pub source: Option<String>,
    #[schemars(description = "Diagnostic session this verdict came from — links the verdict to the session's service task. Omit to resolve via connection_string.")]
    pub session_id: Option<String>,
    #[schemars(description = "Connection string of the client this verdict came from — resolves the active session's service task when session_id is omitted.")]
    pub connection_string: Option<String>,
    #[schemars(description = "Explicit task record id to link (`task:key`). Overrides session/connection resolution.")]
    pub task_id: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct MinidumpAnalyzeParams {
    #[schemars(description = "Absolute path to a .dmp file. LOCAL mode (no connection_string): a file on THIS admin machine. REMOTE mode (with connection_string): a specific dump on the client; omit to analyze ALL of the client's dumps. Kernel/BSOD dumps only (PAGEDU64: triage minidumps, full, BMP, kernel, live); user-mode MDMP app-crash dumps are rejected.")]
    #[serde(default)]
    pub path: Option<String>,
    #[schemars(description = "Web Console connection_string of a connected client. When set, analysis runs ON THAT CLIENT (built-in parser, no plugin/cdb) over MEMORY.DMP + Minidump + LiveKernelReports (or the single `path` if given). When omitted, analyzes the local `path` on this machine. Either way the results auto-log to fleet crash intel (crash_signature/crash_sighting).")]
    #[serde(default)]
    pub connection_string: Option<String>,
    #[schemars(description = "LOCAL mode only: connection_string of the client the local dump file came from (e.g. after crash_dumps_fetch). Links the recorded sightings to that client's open diagnostic session and service task. Remote mode links automatically.")]
    #[serde(default)]
    pub link_connection_string: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CrashDumpsFetchParams {
    #[schemars(description = "Web Console connection_string of the connected client to pull crash dumps from")]
    pub connection_string: String,
    #[schemars(description = "Destination directory on THIS admin machine (default: %USERPROFILE%\\Downloads). Saved as MTech-CrashDumps-<client>.zip.")]
    #[serde(default)]
    pub dest_dir: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct KnownBadDriverAddParams {
    #[schemars(description = "Driver module or INF stem, e.g. 'rtwlane' or 'rtwlane.sys'")]
    pub module: String,
    #[schemars(description = "Bugcheck code of the crash class this driver causes ('0x133', '133'). When given, links the entry to the matching crash_signature.")]
    pub bugcheck_code: Option<String>,
    #[schemars(description = "Bad version matchers (exact or prefix, e.g. '6001.15'). Empty matches every version.")]
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub bad_versions: Option<Vec<String>>,
    #[schemars(description = "Version known to fix the issue, if any")]
    pub fixed_version: Option<String>,
    #[schemars(description = "Symptom this driver causes, e.g. '0x133 DPC_WATCHDOG BSOD on Wi-Fi'")]
    pub symptom: Option<String>,
    #[schemars(description = "Recommended fix, e.g. 'update via vendor package' or 'disable adapter'")]
    pub fix: Option<String>,
    #[schemars(description = "Severity: info | warn | critical (default warn)")]
    pub severity: Option<String>,
    #[schemars(description = "Driver vendor name")]
    pub vendor: Option<String>,
    #[schemars(description = "Human-readable driver/device name")]
    pub display_name: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct DriverSnapshotsListParams {
    #[schemars(description = "Web Console connection_string of the client")]
    pub connection_string: String,
    #[schemars(description = "Max snapshots to return (default 10)")]
    pub limit: Option<u32>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct DriverSnapshotDiffParams {
    #[schemars(description = "Web Console connection_string of the client")]
    pub connection_string: String,
    #[schemars(description = "Older snapshot record id (`driver_snapshot:key`). Omit to use the second-newest.")]
    pub older_id: Option<String>,
    #[schemars(description = "Newer snapshot record id (`driver_snapshot:key`). Omit to use the newest.")]
    pub newer_id: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct DriverSnapshotTakeParams {
    #[schemars(description = "Web Console connection_string of the client to snapshot")]
    pub connection_string: String,
    #[schemars(description = "Capture label: intake | pre_service | post_service | manual (default manual)")]
    pub label: Option<String>,
}

/// Accepts a real array or a stringified JSON array from clients with degraded schemas.
fn de_plugins_used<'de, D>(d: D) -> Result<Option<Vec<PluginUsageRefParam>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(d)?;
    match v {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => {
            serde_json::from_str(&s).map(Some).map_err(serde::de::Error::custom)
        }
        Some(other) => serde_json::from_value(other).map(Some).map_err(serde::de::Error::custom),
    }
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[schemars(inline)]
pub struct RecordStressTestEventParams {
    #[schemars(description = "Event kind: stage_started, unexpected_shutdown, tdr, bsod, custom, operator_note, …")]
    pub kind: String,
    #[schemars(description = "Human-readable event detail")]
    pub detail: String,
    #[schemars(description = "ISO-8601 timestamp (defaults to now if omitted)")]
    pub at: Option<String>,
    #[schemars(description = "Optional vendor code (BSOD bugcheck, WHEA code, dump filename, …)")]
    pub code: Option<String>,
    #[schemars(description = "Event source (default: operator)")]
    pub source: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RecordStressTestRunParams {
    #[schemars(description = "Computer record id (e.g. 'DESKTOP-3F0BA5T:f4ac11309')")]
    pub computer_id: String,
    #[schemars(description = "Optional diagnostic session id to link")]
    pub session_id: Option<String>,
    #[schemars(description = "Optional service order id (numeric SO or record key)")]
    pub service_order_id: Option<String>,
    #[schemars(description = "Target kind: cpu, gpu, memory, system, mixed, … (default gpu)")]
    pub target_kind: Option<String>,
    #[schemars(description = "Optional hardware_component id for target_component (GPU under test)")]
    pub target_component_id: Option<String>,
    #[schemars(description = "Preset label (default qc-mcp:gpu-probe-v1)")]
    pub preset_label: Option<String>,
    #[schemars(description = "Run result: pass, fail, aborted, inconclusive (default fail)")]
    pub result: Option<String>,
    #[schemars(description = "Failure kind tag: none, reboot, timeout, tdr, bsod, … (default reboot for failed GPU hangs)")]
    pub failure_kind: Option<String>,
    #[schemars(description = "ISO-8601 started_at (required for backfill)")]
    pub started_at: String,
    #[schemars(description = "ISO-8601 ended_at")]
    pub ended_at: Option<String>,
    #[schemars(description = "Actual duration in seconds")]
    pub duration_actual_secs: Option<f64>,
    #[schemars(description = "Hostname at run time")]
    pub hostname: Option<String>,
    #[schemars(description = "Free-form notes (symptom match, MCP timeout, dump paths, …)")]
    pub notes: Option<String>,
    #[schemars(description = "Tags (default includes backfill + preset:gpu-probe)")]
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub tags: Option<Vec<String>>,
    #[schemars(description = "Discrete timeline events to attach to the run")]
    #[serde(default)]
    pub events: Vec<RecordStressTestEventParams>,
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
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub tags: Option<Vec<String>>,
    #[schemars(description = "Close despite a missing escalation handoff. The escalated-without-AI-task gate is the only check this bypasses; use deliberately.")]
    #[serde(default)]
    pub force: Option<bool>,
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
pub struct BenchmarkResultsQueryParams {
    #[schemars(description = "Filter by the machine's hostname as reported by the client (e.g. \
        \"BENCH-07\"). Omit to query across all machines.")]
    pub hostname: Option<String>,
    #[schemars(description = "Filter by benchmark kind: cpu_single, cpu_multi, matrix_single, \
        matrix_multi, linpack, memory_bandwidth, memcpy, memory_latency, disk, gpu_compute, \
        gpu_matmul, gpu_vram, gpu_pcie. Omit for all kinds.")]
    pub kind: Option<String>,
    #[schemars(description = "Max rows, newest first. Default 20, cap 200.")]
    pub limit: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsListParams {}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsRunParams {
    #[schemars(
        description = "Script category. One of: 'Tuneup', 'Informational', 'JunkwareRemoval', 'StressTests'."
    )]
    pub category: String,
    #[schemars(
        description = "Display name of the script as listed by scripts_list (e.g. 'Activate Webroot', 'Disable OneDrive Startup', 'GPU Stress Test', 'Stress: CPU')."
    )]
    pub script_name: String,
    #[schemars(
        description = "Service number. Required for activation scripts (Webroot, SuperAnti, SEB) and for stress/verify StressTests scripts — populates stress_test_run.service_order so the run is linked to the customer / computer / ticket. NOT required for 'Benchmark Suite' / 'Benchmark: ...' scripts (scores are machine-keyed)."
    )]
    pub service_number: Option<String>,
    #[schemars(
        description = "Optional customer email override. Required for SuperEasyBackup activation."
    )]
    pub customer_email: Option<String>,
    #[schemars(
        description = "Timeout in seconds to wait for the script to finish. Defaults to the per-script budget (600 for most; 3600 for updates/scans; 7200 for Tron/Data Transfer)."
    )]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub timeout_secs: Option<u64>,
    #[schemars(
        description = "Optional diagnostic_session id (from create_diagnostic_session). Links stress_test_run.session_ref when running any StressTests script."
    )]
    pub diagnostic_session_id: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsRunRemoteParams {
    #[schemars(description = "Web Console connection_string of the remote client (from remote_egui_list_targets).")]
    pub connection_string: String,
    #[schemars(description = "Script category. One of: 'Tuneup', 'Informational', 'JunkwareRemoval', 'StressTests'.")]
    pub category: String,
    #[schemars(description = "Display name of the script as listed by scripts_list (e.g. 'Activate Webroot', 'Activate SEB', 'GPU Stress Test', 'Stress: CPU').")]
    pub script_name: String,
    #[schemars(description = "Service order number. Required for activation scripts (Webroot, SuperAnti, SEB) and for stress/verify StressTests scripts — populates stress_test_run.service_order so the run is linked to the customer / computer / ticket. NOT required for 'Benchmark Suite' / 'Benchmark: ...' scripts (scores are machine-keyed).")]
    pub service_number: Option<String>,
    #[schemars(description = "Customer email. Required for SuperEasyBackup activation.")]
    pub customer_email: Option<String>,
    #[schemars(description = "Timeout in seconds to wait for the script to complete on the remote. Defaults to the per-script budget (600 for most; 3600 for updates/scans; 7200 for Tron/Data Transfer).")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub timeout_secs: Option<u64>,
    #[schemars(
        description = "Optional diagnostic_session id (from create_diagnostic_session). Auto-resolved from the open session for connection_string when omitted. Links stress_test_run.session_ref on any StressTests script."
    )]
    pub diagnostic_session_id: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScriptsRunStressSuiteRemoteParams {
    #[schemars(description = "Web Console connection_string of the remote client (from remote_egui_list_targets).")]
    pub connection_string: String,
    #[schemars(
        description = "Service order number — required so every stress_test_run carries service_order / customer / computer linkage."
    )]
    pub service_number: String,
    #[schemars(
        description = "Optional diagnostic_session id (from create_diagnostic_session). Auto-resolved from the open session for connection_string when omitted."
    )]
    pub diagnostic_session_id: Option<String>,
    #[schemars(
        description = "Script display names to skip (e.g. ['GPU Stress Test'] when it already ran). Default: run the full StressTests catalog."
    )]
    pub skip: Option<Vec<String>>,
    #[schemars(
        description = "Per-script timeout override in seconds. When omitted, QC Benchmark uses 900s and all other stress scripts use 300s."
    )]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub timeout_secs: Option<u64>,
}

fn stress_suite_script_names(skip: &[String]) -> Vec<String> {
    let skip_set: std::collections::HashSet<&str> =
        skip.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let mut names: Vec<String> = stress_runner::STRESS_SCRIPT_NAMES
        .iter()
        .filter(|n| !skip_set.contains(*n))
        .map(|n| (*n).to_string())
        .collect();
    if !skip_set.contains("QC Benchmark") {
        let insert_at = names
            .iter()
            .position(|n| n == "GPU Stress Test")
            .map(|i| i + 1)
            .unwrap_or(0);
        names.insert(insert_at, "QC Benchmark".into());
    }
    names
}

fn default_stress_script_timeout_secs(script_name: &str, override_secs: Option<u64>) -> u64 {
    if let Some(t) = override_secs {
        return t;
    }
    if script_name == "QC Benchmark" {
        900
    } else {
        300
    }
}

async fn execute_one_remote_script(
    p: ScriptsRunRemoteParams,
) -> Result<serde_json::Value, ErrorData> {
    use crate::Cmd;

    let service_number = p.service_number.clone().unwrap_or_default();
    let customer_email = p.customer_email.clone().unwrap_or_default();
    let diagnostic_session_id = p
        .diagnostic_session_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| super::diagnostic_session_registry::get(&p.connection_string));

    if stress_runner::is_stress_script(&p.script_name) && service_number.trim().is_empty() {
        return Err(to_internal(format!(
            "service_number is required for StressTests scripts (so stress_test_run carries service_order / customer / computer linkage). Pass service_number with script '{}'.",
            p.script_name
        )));
    }

    let cmd = Cmd::RunRemoteScripts {
        scripts: vec![crate::RemoteScriptItem {
            name: p.script_name.clone(),
            category: p.category.clone(),
            content: None,
        }],
        service_number: service_number.clone(),
        customer_email: customer_email.clone(),
        diagnostic_session_id: diagnostic_session_id.clone().unwrap_or_default(),
    };
    let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
        .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

    let (tx, rx) = tokio::sync::oneshot::channel::<super::remote_script_notify::RemoteScriptSession>();
    {
        let mut guard = super::remote_script_notify::REMOTE_SCRIPT_PENDING
            .lock()
            .map_err(|_| to_internal("REMOTE_SCRIPT_PENDING poisoned"))?;
        // Reject only if THIS client already has a live waiter; reclaim if its receiver is gone.
        if let Some((pending_name, pending_tx)) = guard.remove(&p.connection_string) {
            if !pending_tx.is_closed() {
                let busy = format!(
                    "Remote script '{pending_name}' is still awaiting completion on {}; that client runs one script at a time. Retry after it finishes or times out.",
                    p.connection_string
                );
                guard.insert(p.connection_string.clone(), (pending_name, pending_tx));
                return Err(to_internal(busy));
            }
        }
        if let Ok(mut accum) = super::remote_script_notify::REMOTE_SCRIPT_ACCUM.lock() {
            accum.insert(
                p.connection_string.clone(),
                super::remote_script_notify::RemoteScriptSession::default(),
            );
        }
        guard.insert(p.connection_string.clone(), (p.script_name.clone(), tx));
    }

    super::remote_egui_control::hub()
        .send_raw_binary(&p.connection_string, serialized)
        .map_err(to_internal)?;

    let timeout = std::time::Duration::from_secs(p.timeout_secs.unwrap_or_else(|| {
        crate::scripts::default_remote_script_timeout_secs(&p.script_name)
    }));
    let session = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(s)) => s,
        Ok(Err(_)) => {
            let _ = super::remote_script_notify::REMOTE_SCRIPT_PENDING.lock().map(|mut g| g.remove(&p.connection_string));
            return Err(to_internal("Remote script channel closed unexpectedly"));
        }
        Err(_) => {
            let _ = super::remote_script_notify::REMOTE_SCRIPT_PENDING.lock().map(|mut g| g.remove(&p.connection_string));
            if super::stress_test_verify::is_persisted_stress_script(&p.script_name) {
                let partial_logs = super::remote_script_notify::REMOTE_SCRIPT_ACCUM
                    .lock()
                    .ok()
                    .and_then(|a| a.get(&p.connection_string).map(|s| s.logs.clone()))
                    .unwrap_or_default();
                let run_hint = super::stress_test_verify::extract_stress_run_id_from_logs(
                    &partial_logs,
                );
                let computer_id = super::stress_test_verify::computer_id_for_connection(
                    &p.connection_string,
                )
                .await;
                let persistence = super::stress_test_verify::verify_stress_test_persistence(
                    computer_id.as_deref(),
                    run_hint.as_deref(),
                    diagnostic_session_id.as_deref(),
                )
                .await;
                return Ok(serde_json::json!({
                    "script": p.script_name,
                    "connection_string": p.connection_string,
                    "success": false,
                    "timed_out": true,
                    "message": format!(
                        "Timed out after {}s — script may still be running or machine hung",
                        timeout.as_secs()
                    ),
                    "logs": partial_logs,
                    "computer_id": computer_id,
                    "diagnostic_session_id": diagnostic_session_id,
                    "stress_test_persistence": persistence,
                }));
            }
            return Err(to_internal(format!(
                "Timed out after {}s waiting for remote script '{}' to complete.",
                timeout.as_secs(),
                p.script_name
            )));
        }
    };

    let overall_success = session
        .results
        .iter()
        .all(|(_, s)| s == "Success" || s == "success");
    let reboot_recommended = session
        .logs
        .iter()
        .any(|l| l.contains(crate::scripts::REBOOT_RECOMMENDED_MARKER));
    let mut payload = serde_json::json!({
        "script": p.script_name,
        "connection_string": p.connection_string,
        "success": overall_success,
        "diagnostic_session_id": diagnostic_session_id,
        "results": session.results.iter().map(|(n, s)| serde_json::json!({"name": n, "status": s})).collect::<Vec<_>>(),
        "logs": session.logs,
    });
    if reboot_recommended {
        payload["reboot_recommended"] = serde_json::json!(true);
        payload["reboot_hint"] = serde_json::json!(
            "Webroot was re-keyed over an existing install; a reboot finalizes the new device identity. Ask the tech/admin to reboot the client (admin console Power > Reboot keeps MasterTech persistent across the restart)."
        );
    }

    if super::stress_test_verify::is_persisted_stress_script(&p.script_name) {
        let run_hint =
            super::stress_test_verify::extract_stress_run_id_from_logs(&session.logs);
        let computer_id =
            super::stress_test_verify::computer_id_for_connection(&p.connection_string).await;
        let persistence = super::stress_test_verify::verify_stress_test_persistence(
            computer_id.as_deref(),
            run_hint.as_deref(),
            diagnostic_session_id.as_deref(),
        )
        .await;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("stress_test_persistence".into(), persistence);
            if let Some(cid) = computer_id {
                obj.insert("computer_id".into(), serde_json::json!(cid));
            }
        }
    }

    Ok(payload)
}

/// Validate scenario/concurrent stages identically to the local tools and map them to wire stages.
fn validate_remote_stress_stages(
    stages: &[ScenarioStageParam],
    concurrent: bool,
) -> Result<Vec<crate::RemoteScenarioStage>, ErrorData> {
    let max = if concurrent { 8 } else { 16 };
    if stages.is_empty() || stages.len() > max {
        return Err(to_internal(if concurrent {
            "Provide 1-8 concurrent lanes."
        } else {
            "Provide 1-16 stages."
        }));
    }
    let mut out: Vec<crate::RemoteScenarioStage> = Vec::with_capacity(stages.len());
    let mut stage_sum: u64 = 0;
    for s in stages {
        if !concurrent && (s.duration_secs == 0 || s.duration_secs > 1800) {
            return Err(to_internal(format!(
                "Stage '{}' duration_secs must be 1-1800.",
                s.label.clone().unwrap_or_else(|| s.stressor.clone())
            )));
        }
        stress_runner::Stressor::from_str(&s.stressor).ok_or_else(|| {
            to_internal(format!(
                "Unknown stressor '{}'. Valid: {}",
                s.stressor,
                stress_runner::Stressor::labels_csv()
            ))
        })?;
        stage_sum += s.duration_secs;
        out.push(crate::RemoteScenarioStage {
            stressor: s.stressor.clone(),
            duration_secs: s.duration_secs,
            threads: s.threads,
            memory_cap_mb: s.memory_cap_mb,
            disk_file_mb: s.disk_file_mb,
            label: s.label.clone(),
        });
    }
    if !concurrent && stage_sum > 7200 {
        return Err(to_internal("Total stage time exceeds the 7200s cap."));
    }
    Ok(out)
}

/// Send a `RunRemoteScenario`/`RunRemoteConcurrent` Cmd to a client, await the
/// shared remote-script reply, then verify stress_test persistence on the client.
async fn execute_remote_stress_plan(
    connection_string: String,
    cmd: crate::Cmd,
    result_name: &str,
    service_number: &str,
    diagnostic_session_id: Option<String>,
    budget_secs: u64,
) -> Result<serde_json::Value, ErrorData> {
    if service_number.trim().is_empty() {
        return Err(to_internal(
            "service_number is required (stress_test_run.service_order linkage).",
        ));
    }
    let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
        .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

    let (tx, rx) = tokio::sync::oneshot::channel::<super::remote_script_notify::RemoteScriptSession>();
    {
        let mut guard = super::remote_script_notify::REMOTE_SCRIPT_PENDING
            .lock()
            .map_err(|_| to_internal("REMOTE_SCRIPT_PENDING poisoned"))?;
        if let Some((pending_name, pending_tx)) = guard.remove(&connection_string) {
            if !pending_tx.is_closed() {
                let busy = format!(
                    "Remote script '{pending_name}' is still awaiting completion on {connection_string}; that client runs one stress op at a time. Retry after it finishes or times out."
                );
                guard.insert(connection_string.clone(), (pending_name, pending_tx));
                return Err(to_internal(busy));
            }
        }
        if let Ok(mut accum) = super::remote_script_notify::REMOTE_SCRIPT_ACCUM.lock() {
            accum.insert(
                connection_string.clone(),
                super::remote_script_notify::RemoteScriptSession::default(),
            );
        }
        guard.insert(connection_string.clone(), (result_name.to_string(), tx));
    }

    super::remote_egui_control::hub()
        .send_raw_binary(&connection_string, serialized)
        .map_err(to_internal)?;

    let timeout = std::time::Duration::from_secs(budget_secs + 300);
    let session = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(s)) => s,
        Ok(Err(_)) => {
            let _ = super::remote_script_notify::REMOTE_SCRIPT_PENDING
                .lock()
                .map(|mut g| g.remove(&connection_string));
            return Err(to_internal("Remote stress channel closed unexpectedly"));
        }
        Err(_) => {
            let _ = super::remote_script_notify::REMOTE_SCRIPT_PENDING
                .lock()
                .map(|mut g| g.remove(&connection_string));
            let partial_logs = super::remote_script_notify::REMOTE_SCRIPT_ACCUM
                .lock()
                .ok()
                .and_then(|a| a.get(&connection_string).map(|s| s.logs.clone()))
                .unwrap_or_default();
            let run_hint =
                super::stress_test_verify::extract_stress_run_id_from_logs(&partial_logs);
            let computer_id =
                super::stress_test_verify::computer_id_for_connection(&connection_string).await;
            let persistence = super::stress_test_verify::verify_stress_test_persistence(
                computer_id.as_deref(),
                run_hint.as_deref(),
                diagnostic_session_id.as_deref(),
            )
            .await;
            return Ok(serde_json::json!({
                "op": result_name,
                "connection_string": connection_string,
                "success": false,
                "timed_out": true,
                "message": format!(
                    "Timed out after {}s — the stress plan may still be running or the machine hung; check stress_test_run for the in_progress row.",
                    timeout.as_secs()
                ),
                "logs": partial_logs,
                "computer_id": computer_id,
                "diagnostic_session_id": diagnostic_session_id,
                "stress_test_persistence": persistence,
            }));
        }
    };

    let overall_success = session
        .results
        .iter()
        .all(|(_, s)| s == "Success" || s == "success");
    let run_hint = super::stress_test_verify::extract_stress_run_id_from_logs(&session.logs);
    let computer_id =
        super::stress_test_verify::computer_id_for_connection(&connection_string).await;
    let persistence = super::stress_test_verify::verify_stress_test_persistence(
        computer_id.as_deref(),
        run_hint.as_deref(),
        diagnostic_session_id.as_deref(),
    )
    .await;

    Ok(serde_json::json!({
        "op": result_name,
        "connection_string": connection_string,
        "success": overall_success,
        "diagnostic_session_id": diagnostic_session_id,
        "computer_id": computer_id,
        "results": session.results.iter().map(|(n, s)| serde_json::json!({"name": n, "status": s})).collect::<Vec<_>>(),
        "logs": session.logs,
        "stress_test_persistence": persistence,
    }))
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

// ─── RemoteExec param types ───────────────────────────────────────────────────

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecCapabilitiesParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecArmParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Technician or agent identity recorded on every job")]
    pub tech: String,
    #[schemars(description = "Diagnostic session id this remote-control lease belongs to (from create_diagnostic_session)")]
    pub diagnostic_session_id: String,
    #[schemars(description = "Why remote control is needed. Shown verbatim on the client's consent banner.")]
    pub reason: String,
    #[schemars(description = "Lease lifetime in seconds (default 3600, clamped to 8h by the client)")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub ttl_secs: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecDisarmParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Also terminate every running job (default false — jobs keep running, but no new ones are admitted)")]
    pub kill_running: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecStartParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Script body. Written to a temp file on the client and run by the chosen shell — no quoting/escaping needed.")]
    pub script: String,
    #[schemars(description = "Technician or agent identity recorded on the job")]
    pub tech: String,
    #[schemars(description = "Why this job is being run. REQUIRED (non-empty) when risk is 'destructive'.")]
    pub reason: String,
    #[schemars(description = "Interpreter: 'powershell' (default), 'pwsh', or 'cmd'")]
    pub shell: Option<String>,
    #[schemars(description = "Risk tier: 'read' (default, changes nothing), 'mutate' (reversible change), 'destructive' (removes data or changes boot/driver/security state)")]
    pub risk: Option<String>,
    #[schemars(description = "Working directory for the process")]
    pub cwd: Option<String>,
    #[schemars(description = "Extra environment variables as a JSON object, e.g. {\"KEY\":\"value\"}")]
    #[serde(default, deserialize_with = "deserialize_lenient_args")]
    pub env: Option<serde_json::Value>,
    #[schemars(description = "Hard wall-clock cap in seconds (client default 3600). A job producing no output for 600s is killed as wedged regardless.")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub timeout_secs: Option<u64>,
    #[schemars(description = "Discard captured output instead of buffering it. Use when the script handles credentials.")]
    pub redact: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecTailParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Job id returned by remote_exec_start")]
    pub job_id: String,
    #[schemars(description = "Resume from this sequence number (use the previous call's next_seq). Default 0 = from the start of what the ring still holds.")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub from_seq: Option<u64>,
    #[schemars(description = "Cap on output bytes returned (default 65536)")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub max_bytes: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecWaitParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Job id returned by remote_exec_start")]
    pub job_id: String,
    #[schemars(description = "Give up waiting after this many seconds (default 300, max 900). Returning early does NOT stop the job — poll again or call remote_exec_tail.")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub timeout_secs: Option<u64>,
    #[schemars(description = "Seconds between polls (default 3, min 1)")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub poll_interval_secs: Option<u64>,
    #[schemars(description = "Resume output from this sequence number (default 0)")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub from_seq: Option<u64>,
    #[schemars(description = "Cap on output bytes returned (default 65536)")]
    #[serde(default, deserialize_with = "deserialize_lenient_u64")]
    pub max_bytes: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecSignalParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
    #[schemars(description = "Job id to signal")]
    pub job_id: String,
    #[schemars(description = "'cancel' (stop, then terminate the tree), 'kill' (terminate the tree now), or 'detach' (leave it running, stop streaming)")]
    pub signal: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RemoteExecListParams {
    #[schemars(description = "Web Console connection_string of the remote client")]
    pub connection_string: String,
}

/// Sends one RemoteExec `Cmd` and waits for the client's reply.
///
/// Every RemoteExec handler on the client answers without awaiting job
/// completion, so a short deadline is correct here — a slow reply means the
/// channel is wedged, not that the work is long.
async fn remote_exec_roundtrip(
    connection_string: &str,
    label: &str,
    make_cmd: impl FnOnce(String) -> crate::Cmd,
) -> Result<serde_json::Value, ErrorData> {
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

    let request_id = format!("rex-{}", uuid::Uuid::new_v4());
    let cmd = make_cmd(request_id.clone());
    let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
        .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

    let rx = register_pending_request(request_id.clone());
    let _guard = PendingRequestGuard { request_id: request_id.clone() };

    super::remote_egui_control::hub()
        .send_raw_binary(connection_string, serialized)
        .map_err(to_internal)?;

    let (success, result_json) = match tokio::time::timeout(DEADLINE, rx).await {
        Ok(Ok(pair)) => pair,
        Ok(Err(_)) => {
            return Err(to_internal(format!(
                "{label}: response channel closed for req={request_id} (client {connection_string} \
                 disconnected mid-call)"
            )));
        }
        Err(_) => {
            return Err(to_internal(format!(
                "{label}: no reply from {connection_string} within {}s. RemoteExec handlers answer \
                 immediately, so this means the channel is wedged or the client build predates \
                 RemoteExec — check remote_channel_health and remote_exec_capabilities.",
                DEADLINE.as_secs()
            )));
        }
    };

    let value: serde_json::Value = serde_json::from_str(&result_json)
        .unwrap_or_else(|_| serde_json::json!({ "raw": result_json }));

    if !success {
        let why = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("client refused the request");
        return Err(to_internal(format!("{label}: {why}")));
    }
    Ok(value)
}

/// Turns the client's `JobSnapshot` JSON into something readable: base64 chunk
/// payloads become text, and the caller gets the `next_seq` to resume from.
///
/// `requested_from_seq` is what this read asked for. `next_seq` is derived only
/// from the chunks actually returned — the snapshot's `last_seq` is the ring's
/// newest chunk, so seeding from it would skip everything a byte-capped read
/// did not serve.
fn render_job_snapshot(mut snap: serde_json::Value, requested_from_seq: u64) -> serde_json::Value {
    let chunks = snap
        .get_mut("chunks")
        .map(serde_json::Value::take)
        .unwrap_or(serde_json::Value::Null);

    let mut next_seq = requested_from_seq;
    let mut elided: u64 = 0;
    let mut served = 0usize;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut meta = String::new();

    if let Some(list) = chunks.as_array() {
        use base64::Engine;
        served = list.len();
        for c in list {
            if let Some(seq) = c.get("seq").and_then(|v| v.as_u64()) {
                next_seq = next_seq.max(seq + 1);
            }
            elided += c.get("elided_before").and_then(|v| v.as_u64()).unwrap_or(0);
            let bytes = c
                .get("data")
                .and_then(|v| v.as_str())
                .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
                .unwrap_or_default();
            // Console output is not guaranteed UTF-8; replace rather than drop.
            let text = String::from_utf8_lossy(&bytes);
            match c.get("stream").and_then(|v| v.as_str()) {
                Some("Stderr") => stderr.push_str(&text),
                Some("Meta") => meta.push_str(&text),
                _ => stdout.push_str(&text),
            }
        }
    }

    let last_seq = snap.get("last_seq").and_then(|v| v.as_u64()).unwrap_or(0);
    // last_seq names the newest chunk the ring holds, so anything from next_seq
    // through it is output the byte cap did not serve. Gated on having served
    // something: the client reports last_seq 0 both for "one chunk" and for "no
    // output at all", and an empty read means there is nothing more to fetch.
    let more_pending = served > 0 && next_seq <= last_seq;

    if let Some(obj) = snap.as_object_mut() {
        obj.insert("stdout".into(), serde_json::json!(stdout));
        obj.insert("stderr".into(), serde_json::json!(stderr));
        if !meta.is_empty() {
            obj.insert("runtime_notes".into(), serde_json::json!(meta));
        }
        obj.insert("next_seq".into(), serde_json::json!(next_seq));
        if more_pending {
            obj.insert("more_output_pending".into(), serde_json::json!(true));
            obj.insert(
                "more_output_note".into(),
                serde_json::json!(
                    "This read hit its byte cap. Call remote_exec_tail again with from_seq=next_seq \
                     — the output above is not the whole job."
                ),
            );
        }
        if elided > 0 {
            obj.insert(
                "elided_bytes".into(),
                serde_json::json!(elided),
            );
            obj.insert(
                "elided_note".into(),
                serde_json::json!(
                    "Output was dropped by the client's in-memory ring before this read. Poll more \
                     often, or have the script tee to a file."
                ),
            );
        }
    }
    snap
}

fn parse_shell(s: Option<&str>) -> Result<crate::remote_exec::ShellKind, ErrorData> {
    match s.unwrap_or("powershell").trim().to_ascii_lowercase().as_str() {
        "powershell" | "ps" | "ps1" => Ok(crate::remote_exec::ShellKind::PowerShell),
        "pwsh" => Ok(crate::remote_exec::ShellKind::Pwsh),
        "cmd" | "bat" | "batch" => Ok(crate::remote_exec::ShellKind::Cmd),
        other => Err(to_internal(format!(
            "unknown shell {other:?}; use 'powershell', 'pwsh' or 'cmd'"
        ))),
    }
}

fn parse_risk(s: Option<&str>) -> Result<crate::remote_exec::RiskTier, ErrorData> {
    match s.unwrap_or("read").trim().to_ascii_lowercase().as_str() {
        "read" => Ok(crate::remote_exec::RiskTier::Read),
        "mutate" | "write" => Ok(crate::remote_exec::RiskTier::Mutate),
        "destructive" => Ok(crate::remote_exec::RiskTier::Destructive),
        other => Err(to_internal(format!(
            "unknown risk {other:?}; use 'read', 'mutate' or 'destructive'"
        ))),
    }
}

fn parse_signal(s: &str) -> Result<crate::remote_exec::JobSignal, ErrorData> {
    match s.trim().to_ascii_lowercase().as_str() {
        "cancel" => Ok(crate::remote_exec::JobSignal::Cancel),
        "kill" => Ok(crate::remote_exec::JobSignal::Kill),
        "detach" => Ok(crate::remote_exec::JobSignal::Detach),
        other => Err(to_internal(format!(
            "unknown signal {other:?}; use 'cancel', 'kill' or 'detach'"
        ))),
    }
}

/// Pulls one job out of a `RemoteJobQuery` reply.
fn take_job(value: serde_json::Value, job_id: &str) -> Result<serde_json::Value, ErrorData> {
    let jobs = value
        .get("jobs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    jobs.into_iter()
        .find(|j| j.get("job_id").and_then(|v| v.as_str()) == Some(job_id))
        .ok_or_else(|| {
            to_internal(format!(
                "client is not retaining job {job_id}. Terminal jobs are dropped 10 minutes after \
                 they finish, and a client restart marks everything Orphaned."
            ))
        })
}

// ─── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl PluginToolProvider {
    // ── Management tools ────────────────────────────────────────────────

    #[tool(
        name = "list_plugins",
        description = "List the plugins CURRENTLY REGISTERED in this Mastertech process's PluginManager (with status, version, tool count). \
                       Only the two built-in egui plumbing plugins (`com.mastertech.egui-frame-capture`, `com.mastertech.egui-remote-viewer`) are registered by default. \
                       Diagnostic plugins (hw-diag, repair, diagnostics, status-reporter, bsod-fixer, etc.) are NOT auto-loaded even if their compiled .wasm exists in `%LOCALAPPDATA%/Mastertech/plugins/` (Windows) or `$HOME/.local/share/mastertech/plugins/` (Linux). \
                       To use them: call `search_plugins` → `fetch_plugin` → `plugin_deploy` (local) or `plugin_deploy_remote` (target client). \
                       See `search_plugins` for the registry-side catalog and the 'Known Plugins in Registry' section of the server instructions for the canonical list."
    )]
    async fn list_plugins(
        &self,
        Parameters(_p): Parameters<ListPluginsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mgr = self.try_read_manager()?;
        let plugins = mgr.list_plugins();
        Ok(CallToolResult::success(vec![
            ContentBlock::json(plugins).map_err(to_internal)?
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
        Ok(CallToolResult::success(vec![ContentBlock::json(
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
        Ok(CallToolResult::success(vec![ContentBlock::json(
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
        Ok(CallToolResult::success(vec![plugin_value_to_content(
            result,
        )?]))
    }

    // ── RemoteExec (long-running privileged jobs on a connected client) ─────────

    #[tool(
        name = "remote_exec_capabilities",
        description = "Probe what RemoteExec a connected client supports (job kinds, shells, protocol version, ring size, default timeout). \
                       Call this first: a client build that predates RemoteExec will time out here, which is how you tell it apart from a wedged channel. \
                       RemoteExec runs shell jobs the client owns — unlike `call_remote_plugin_tool`, which is capped by the PluginManager watchdog, \
                       a RemoteExec job survives the admin disconnecting and reports a real exit code."
    )]
    async fn remote_exec_capabilities(
        &self,
        Parameters(p): Parameters<RemoteExecCapabilitiesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_capabilities", |request_id| {
            crate::Cmd::RemoteExecCapabilities { request_id }
        })
        .await?;
        Ok(CallToolResult::success(vec![
            ContentBlock::json(value).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_arm",
        description = "Open the consent gate on a client so RemoteExec jobs may run. Fails closed: until the client paints its consent banner \
                       (which names you and your stated reason to whoever is at the machine), every remote_exec_start is refused. \
                       If start keeps reporting 'consent banner not rendering', the client's UI is minimised, wedged, or on a build without the banner. \
                       Arm once per diagnostic session, not per job, and call remote_exec_disarm when you are done."
    )]
    async fn remote_exec_arm(
        &self,
        Parameters(p): Parameters<RemoteExecArmParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.reason.trim().is_empty() {
            return Err(to_internal(
                "reason is shown to the person at the machine and must not be empty",
            ));
        }
        let ttl = p.ttl_secs.unwrap_or(3600);
        let cs = p.connection_string.clone();
        let session_id = cs.clone();
        let value = remote_exec_roundtrip(&cs, "remote_exec_arm", move |request_id| {
            crate::Cmd::RemoteControlArm {
                request_id,
                session_id,
                tech: p.tech,
                diagnostic_session_id: p.diagnostic_session_id,
                reason: p.reason,
                ttl_secs: ttl,
            }
        })
        .await?;
        Ok(CallToolResult::success(vec![
            ContentBlock::json(value).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_disarm",
        description = "Close the consent gate. New jobs stop being admitted immediately. Running jobs keep running unless kill_running is true \
                       — a half-finished install is usually worse than a finished one, so killing is opt-in."
    )]
    async fn remote_exec_disarm(
        &self,
        Parameters(p): Parameters<RemoteExecDisarmParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let kill_running = p.kill_running.unwrap_or(false);
        let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_disarm", |request_id| {
            crate::Cmd::RemoteControlDisarm { request_id, kill_running }
        })
        .await?;
        Ok(CallToolResult::success(vec![
            ContentBlock::json(value).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_start",
        description = "Submit a shell job to a connected client and return immediately with its job_id. The client owns the process: it keeps running \
                       if the admin disconnects, and its exit code is real (not a proxy's guess). Poll with remote_exec_tail or block with remote_exec_wait. \
                       Requires remote_exec_arm first. Scripts run elevated — the client process is requireAdministrator — so state a real reason; \
                       risk 'destructive' additionally requires a non-empty reason and is recorded in the client's on-disk journal."
    )]
    async fn remote_exec_start(
        &self,
        Parameters(p): Parameters<RemoteExecStartParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.script.trim().is_empty() {
            return Err(to_internal("script is empty"));
        }
        let shell = parse_shell(p.shell.as_deref())?;
        let risk = parse_risk(p.risk.as_deref())?;

        let env: Vec<(String, String)> = p
            .env
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        let s = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        (k.clone(), s)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let spec = crate::remote_exec::RemoteJobSpec::Shell {
            shell,
            script: p.script,
            cwd: p.cwd,
            env,
            timeout_secs: p.timeout_secs,
            redact: p.redact.unwrap_or(false),
        };

        let job_id = format!("job-{}", uuid::Uuid::new_v4());
        let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_start", {
            let job_id = job_id.clone();
            move |request_id| crate::Cmd::RemoteJobStart {
                request_id,
                job_id,
                tech: p.tech,
                reason: p.reason,
                risk,
                spec,
            }
        })
        .await?;

        let mut body = render_job_snapshot(value, 0);
        if let Some(obj) = body.as_object_mut() {
            obj.insert("job_id".into(), serde_json::json!(job_id));
        }
        Ok(CallToolResult::success(vec![
            ContentBlock::json(body).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_tail",
        description = "Read a job's current state and buffered output. Pass the previous call's `next_seq` as `from_seq` to page forward without \
                       re-reading what you already have. If the response carries `elided_bytes`, the client's in-memory ring overflowed and that \
                       output is gone for good — poll more often or have the script tee to a file."
    )]
    async fn remote_exec_tail(
        &self,
        Parameters(p): Parameters<RemoteExecTailParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max_bytes = p.max_bytes.unwrap_or(65_536).min(u32::MAX as u64) as u32;
        let from_seq = p.from_seq.unwrap_or(0);
        let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_tail", {
            let job_id = p.job_id.clone();
            move |request_id| crate::Cmd::RemoteJobQuery {
                request_id,
                job_id: Some(job_id),
                from_seq: Some(from_seq),
                max_bytes: Some(max_bytes),
            }
        })
        .await?;
        let job = take_job(value, &p.job_id)?;
        Ok(CallToolResult::success(vec![
            ContentBlock::json(render_job_snapshot(job, from_seq)).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_wait",
        description = "Poll a job until it reaches a terminal state, then return its output and exit code. The waiting happens here on the admin side; \
                       the client is never blocked. Returning on timeout does NOT stop the job — `state` will still be Running, and you can keep \
                       polling with remote_exec_tail from the returned `next_seq`."
    )]
    async fn remote_exec_wait(
        &self,
        Parameters(p): Parameters<RemoteExecWaitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        /// Ceiling on output accumulated across polls, so a chatty job cannot
        /// return a multi-megabyte tool result.
        const WAIT_OUTPUT_CAP: usize = 512 * 1024;

        let deadline_secs = p.timeout_secs.unwrap_or(300).min(900);
        let interval = std::time::Duration::from_secs(p.poll_interval_secs.unwrap_or(3).max(1));
        let max_bytes = p.max_bytes.unwrap_or(65_536).min(u32::MAX as u64) as u32;
        let started = std::time::Instant::now();

        let mut from_seq = p.from_seq.unwrap_or(0);
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut notes = String::new();
        let mut elided: u64 = 0;
        // Every exit from the loop below runs after this is assigned.
        let mut last;

        loop {
            let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_wait", {
                let job_id = p.job_id.clone();
                move |request_id| crate::Cmd::RemoteJobQuery {
                    request_id,
                    job_id: Some(job_id),
                    from_seq: Some(from_seq),
                    max_bytes: Some(max_bytes),
                }
            })
            .await?;

            let job = render_job_snapshot(take_job(value, &p.job_id)?, from_seq);
            from_seq = job.get("next_seq").and_then(|v| v.as_u64()).unwrap_or(from_seq);
            elided += job.get("elided_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(s) = job.get("stdout").and_then(|v| v.as_str()) {
                stdout.push_str(s);
            }
            if let Some(s) = job.get("stderr").and_then(|v| v.as_str()) {
                stderr.push_str(s);
            }
            if let Some(s) = job.get("runtime_notes").and_then(|v| v.as_str()) {
                notes.push_str(s);
            }

            let state = job
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let more_pending = job
                .get("more_output_pending")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            last = job;

            let terminal = !matches!(state.as_str(), "Queued" | "Running");
            let room_left = stdout.len() + stderr.len() < WAIT_OUTPUT_CAP;
            // Keep paging without sleeping while the byte cap is holding output
            // back; reporting a finished job with a truncated tail reads as if
            // that were all it produced. Once the accumulated cap is reached
            // there is nothing more to collect, so fall back to normal pacing.
            if more_pending && room_left && started.elapsed().as_secs() < deadline_secs {
                continue;
            }
            if terminal {
                break;
            }
            if started.elapsed().as_secs() >= deadline_secs {
                break;
            }
            tokio::time::sleep(interval).await;
        }

        let capped = stdout.len() + stderr.len() >= WAIT_OUTPUT_CAP;
        if let Some(obj) = last.as_object_mut() {
            obj.insert("stdout".into(), serde_json::json!(stdout));
            obj.insert("stderr".into(), serde_json::json!(stderr));
            obj.insert("next_seq".into(), serde_json::json!(from_seq));
            obj.insert("waited_secs".into(), serde_json::json!(started.elapsed().as_secs()));
            if notes.is_empty() {
                obj.remove("runtime_notes");
            } else {
                obj.insert("runtime_notes".into(), serde_json::json!(notes));
            }
            if capped {
                obj.insert(
                    "output_cap_reached".into(),
                    serde_json::json!(format!(
                        "Stopped collecting at {WAIT_OUTPUT_CAP} bytes. Resume with \
                         remote_exec_tail from_seq=next_seq."
                    )),
                );
            } else {
                obj.remove("more_output_pending");
                obj.remove("more_output_note");
            }
            if elided > 0 {
                obj.insert("elided_bytes".into(), serde_json::json!(elided));
            } else {
                obj.remove("elided_bytes");
                obj.remove("elided_note");
            }
        }
        Ok(CallToolResult::success(vec![
            ContentBlock::json(last).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_signal",
        description = "Cancel, kill or detach a running job. 'cancel' and 'kill' both terminate the whole process tree via the client's Win32 job \
                       object, so child processes cannot outlive it. Idempotent on jobs that already finished."
    )]
    async fn remote_exec_signal(
        &self,
        Parameters(p): Parameters<RemoteExecSignalParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let signal = parse_signal(&p.signal)?;
        let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_signal", {
            let job_id = p.job_id.clone();
            move |request_id| crate::Cmd::RemoteJobSignal { request_id, job_id, signal }
        })
        .await?;
        // The reply carries no output by design; use remote_exec_tail for that.
        Ok(CallToolResult::success(vec![
            ContentBlock::json(value).map_err(to_internal)?
        ]))
    }

    #[tool(
        name = "remote_exec_list",
        description = "List every job the client is retaining, plus the current consent-gate state (armed, by whom, time left, running count). \
                       Output is omitted here — use remote_exec_tail for a specific job. Terminal jobs are dropped 10 minutes after they finish."
    )]
    async fn remote_exec_list(
        &self,
        Parameters(p): Parameters<RemoteExecListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = remote_exec_roundtrip(&p.connection_string, "remote_exec_list", |request_id| {
            crate::Cmd::RemoteJobQuery {
                request_id,
                job_id: None,
                from_seq: None,
                max_bytes: Some(0),
            }
        })
        .await?;
        Ok(CallToolResult::success(vec![
            ContentBlock::json(value).map_err(to_internal)?
        ]))
    }

    // ── Pre-boot direct link (MCP → UEFI firmware over :9209) ───────────────────

    #[tool(
        name = "preboot_list_clients",
        description = "List UEFI pre-boot boxes currently linked to this console over the direct :9209 socket. \
                       These are firmware clients running before any OS, so none of the Windows-agent tools \
                       (remote_egui_*, driver_snapshot_*, scripts_run_remote) apply to them - use the preboot_* \
                       tools instead. Returns serial, socket peer, and seconds since the last frame."
    )]
    async fn preboot_list_clients(
        &self,
        Parameters(_p): Parameters<PrebootListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = preboot_hub()?;
        let agents: Vec<serde_json::Value> = hub
            .agents()
            .into_iter()
            .map(|a| serde_json::json!({ "serial": a.serial, "peer": a.peer, "idle_secs": a.idle_secs }))
            .collect();
        Ok(CallToolResult::success(vec![
            ContentBlock::json(serde_json::json!({ "clients": agents })).map_err(to_internal)?,
        ]))
    }

    #[tool(
        name = "preboot_stream_ctl",
        description = "Start or stop TUI frame streaming from a pre-boot box. Frames are only pushed while \
                       streaming is on, so call this with stream=true before preboot_screen or the screen \
                       will be stale or empty."
    )]
    async fn preboot_stream_ctl(
        &self,
        Parameters(p): Parameters<PrebootStreamParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = preboot_hub()?;
        let sent = hub.send_stream_ctl(&p.serial, p.stream);
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "serial": p.serial, "stream": p.stream, "sent": sent }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "preboot_screen",
        description = "Read the pre-boot box's current TUI screen as text, one string per terminal row. \
                       This is how to see what the firmware is displaying - the Storage tab's SMART/ATA \
                       attributes, the log ring, stress results. Requires preboot_stream_ctl {stream:true} first."
    )]
    async fn preboot_screen(
        &self,
        Parameters(p): Parameters<PrebootScreenParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = preboot_hub()?;
        if !hub.is_connected(&p.serial) {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("no direct link for serial '{}'; call preboot_list_clients", p.serial),
                None,
            ));
        }
        let Some(bytes) = hub.latest_frame(&p.serial) else {
            return Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
                "serial": p.serial,
                "lines": Vec::<String>::new(),
                "note": "no frame yet - call preboot_stream_ctl {stream:true} and retry",
            }))
            .map_err(to_internal)?]));
        };
        let frame = tcp_protocol::preboot::decode_frame(&bytes).ok_or_else(|| {
            ErrorData::new(ErrorCode::INTERNAL_ERROR, "frame decode failed".to_string(), None)
        })?;
        let lines = preboot_frame_lines(&frame);
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "serial": p.serial,
            "frame": frame.frame,
            "cols": frame.cols,
            "rows": frame.rows,
            "lines": lines,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "preboot_send_key",
        description = "Send one keypress to a pre-boot box, as if typed on its keyboard. Use this to drive \
                       the firmware TUI: tab switches panes, and single letters are its command keys \
                       (for example 'c' connect, 'd' DHCP, 'v' stream, 'e' edit target)."
    )]
    async fn preboot_send_key(
        &self,
        Parameters(p): Parameters<PrebootInputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = preboot_hub()?;
        let code = parse_pb_key(&p.key).ok_or_else(|| {
            ErrorData::new(ErrorCode::INVALID_PARAMS, format!("unrecognized key '{}'", p.key), None)
        })?;
        let ev = tcp_protocol::preboot::PreBootEvent::Key(tcp_protocol::preboot::PreBootKey {
            code,
            ctrl: p.ctrl.unwrap_or(false),
            alt: p.alt.unwrap_or(false),
            shift: p.shift.unwrap_or(false),
        });
        let sent = hub.send_input(&p.serial, &ev);
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "serial": p.serial, "key": p.key, "sent": sent }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "preboot_type",
        description = "Type literal text into a pre-boot box, one character key per character. Use for \
                       fields like the relay target editor; use preboot_send_key for Enter and named keys."
    )]
    async fn preboot_type(
        &self,
        Parameters(p): Parameters<PrebootTypeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = preboot_hub()?;
        let mut sent = 0usize;
        for ch in p.text.chars() {
            let ev = tcp_protocol::preboot::PreBootEvent::Key(tcp_protocol::preboot::PreBootKey {
                code: tcp_protocol::preboot::PbKeyCode::Char(ch),
                ctrl: false,
                alt: false,
                shift: false,
            });
            if hub.send_input(&p.serial, &ev) {
                sent += 1;
            }
        }
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "serial": p.serial, "chars": p.text.chars().count(), "sent": sent }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "preboot_run_plugin",
        description = "Run a WASM plugin inside the UEFI firmware and return its result. `source` is a registry \
                       plugin id or an http URL the firmware fetches itself; empty runs the embedded demo plugin. \
                       This is the extension point for pre-OS diagnostics - the plugin executes before any OS \
                       is booted, so it works on machines that cannot boot at all."
    )]
    async fn preboot_run_plugin(
        &self,
        Parameters(p): Parameters<PrebootRunPluginParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hub = preboot_hub()?;
        if !hub.is_connected(&p.serial) {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("no direct link for serial '{}'; call preboot_list_clients", p.serial),
                None,
            ));
        }
        // Drain any result left by a previous run so the poll below can't return it.
        let _ = hub.take_plugin_result(&p.serial);
        let req = tcp_protocol::preboot::PbPluginRun {
            source: p.source.unwrap_or_default(),
            tool: p.tool.unwrap_or_default(),
            args: p.args.unwrap_or_default(),
        };
        if !hub.run_plugin(&p.serial, &req) {
            return Err(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                "direct link dropped the plugin-run frame".to_string(),
                None,
            ));
        }
        let budget = std::time::Duration::from_millis(p.timeout_ms.unwrap_or(30_000));
        let deadline = std::time::Instant::now() + budget;
        while std::time::Instant::now() < deadline {
            if let Some(r) = hub.take_plugin_result(&p.serial) {
                return Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
                    "serial": p.serial,
                    "ok": r.ok,
                    "plugin": { "id": r.id, "name": r.name, "version": r.version, "tools": r.tools },
                    "tool": r.tool,
                    "result": r.result,
                    "log": r.log,
                    "stdout": r.stdout,
                    "error": r.error,
                }))
                .map_err(to_internal)?]));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err(ErrorData::new(
            ErrorCode::INTERNAL_ERROR,
            format!("firmware returned no plugin result within {}ms", budget.as_millis()),
            None,
        ))
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
                ContentBlock::json(meta).map_err(to_internal)?,
            ]))
        } else {
            Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
                "ok": false,
                "connection_string": p.connection_string,
                "detail": "No egui frame recorded yet. Open Mastertech Viewer for this client and wait until the remote UI appears.",
            }))
            .map_err(to_internal)?]))
        }
    }

    #[tool(
        name = "remote_channel_health",
        description = "Probe every admin↔client subchannel for one connected client in a single call with short timeouts: DB heartbeat freshness, admin WS session presence, egui frame stream freshness, scripts round-trip (GetRemoteScriptList echo), and plugin-call round-trip (sentinel tool call). Returns a per-channel matrix plus a verdict (healthy / one_way_alive_round_trips_dead / no_session / degraded). Call this BEFORE scripts_run_remote, call_remote_plugin_tool, or a stress suite instead of discovering a wedged channel through a 60s+ tool timeout."
    )]
    async fn remote_channel_health(
        &self,
        Parameters(p): Parameters<RemoteChannelHealthParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let probe_timeout =
            std::time::Duration::from_secs(p.probe_timeout_secs.unwrap_or(5).clamp(1, 30));
        let cs = p.connection_string.clone();
        let hub = super::remote_egui_control::hub();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // DB heartbeat freshness; bounded by probe_timeout so a dead DB socket reports instead of hanging.
        let dbh = database::db();
        let heartbeat_probe = dbh
            .query("SELECT connected, last_update FROM connected_client WHERE connection_string = $cs;")
            .bind(("cs", cs.clone()));
        let heartbeat = match tokio::time::timeout(probe_timeout, heartbeat_probe).await {
            Err(_) => serde_json::json!({
                "status": "timeout",
                "timeout_secs": probe_timeout.as_secs(),
                "detail": "DB query never returned — admin's SurrealDB websocket is wedged; all DB-backed tools will hang until it reconnects.",
            }),
            Ok(Err(e)) => serde_json::json!({ "status": "query_error", "detail": e.to_string() }),
            Ok(Ok(mut resp)) => match resp.take::<Vec<serde_json::Value>>(0) {
                Ok(rows) => rows
                    .into_iter()
                    .next()
                    .map(|r| {
                        let staleness_ms = r
                            .get("last_update")
                            .and_then(|v| v.as_str())
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_milliseconds());
                        serde_json::json!({
                            "status": "ok",
                            "connected": r.get("connected").cloned().unwrap_or(serde_json::Value::Null),
                            "staleness_ms": staleness_ms,
                        })
                    })
                    .unwrap_or_else(|| serde_json::json!({ "status": "no_row" })),
                Err(e) => serde_json::json!({ "status": "query_error", "detail": e.to_string() }),
            },
        };

        // Admin WS session + egui frame stream freshness.
        let session_present = hub.list_targets().iter().any(|t| t == &cs);
        let frame_stream = match hub.get_last_frame_meta(&cs) {
            Some(meta) => {
                let meta_value = serde_json::to_value(&meta).unwrap_or_default();
                let staleness_ms = meta_value
                    .get("timestamp_ms")
                    .and_then(|v| v.as_u64())
                    .map(|t| now_ms.saturating_sub(t));
                serde_json::json!({
                    "status": "ok",
                    "staleness_ms": staleness_ms,
                    "frame_count": meta_value.get("frame_count").cloned().unwrap_or(serde_json::Value::Null),
                })
            }
            None => serde_json::json!({ "status": "no_frames" }),
        };

        // Scripts round-trip: GetRemoteScriptList must echo a RemoteScriptListResponse.
        let scripts_round_trip = if !session_present {
            serde_json::json!({ "status": "no_session" })
        } else {
            let rx = super::remote_script_notify::register_script_list_waiter();
            let cmd = crate::Cmd::GetRemoteScriptList;
            match bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
                .map_err(|e| e.to_string())
                .and_then(|bytes| hub.send_raw_binary(&cs, bytes))
            {
                Err(e) => serde_json::json!({ "status": "send_failed", "detail": e }),
                Ok(()) => {
                    let t0 = std::time::Instant::now();
                    match tokio::time::timeout(probe_timeout, rx).await {
                        Ok(Ok(categories)) => serde_json::json!({
                            "status": "ok",
                            "latency_ms": t0.elapsed().as_millis() as u64,
                            "categories": categories,
                        }),
                        Ok(Err(_)) => serde_json::json!({ "status": "waiter_dropped" }),
                        Err(_) => serde_json::json!({
                            "status": "timeout",
                            "timeout_secs": probe_timeout.as_secs(),
                        }),
                    }
                }
            }
        };

        // Plugin-call round-trip: sentinel tool on the always-registered frame-capture plugin.
        let plugin_round_trip = if !session_present {
            serde_json::json!({ "status": "no_session" })
        } else {
            let request_id = format!("hp-{}", uuid::Uuid::new_v4());
            let rx = register_pending_request(request_id.clone());
            let _guard = PendingRequestGuard { request_id: request_id.clone() };
            let cmd = crate::Cmd::CallRemotePluginTool {
                request_id: request_id.clone(),
                plugin_id: "com.mastertech.egui-frame-capture".to_string(),
                tool_name: "__channel_health_probe__".to_string(),
                args_json: "{}".to_string(),
            };
            match bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
                .map_err(|e| e.to_string())
                .and_then(|bytes| hub.send_raw_binary(&cs, bytes))
            {
                Err(e) => serde_json::json!({ "status": "send_failed", "detail": e }),
                Ok(()) => {
                    let t0 = std::time::Instant::now();
                    match tokio::time::timeout(probe_timeout, rx).await {
                        // Any response — even "unknown tool" — proves the responder loop is alive.
                        Ok(Ok((_success, _body))) => serde_json::json!({
                            "status": "ok",
                            "latency_ms": t0.elapsed().as_millis() as u64,
                        }),
                        Ok(Err(_)) => serde_json::json!({ "status": "waiter_dropped" }),
                        Err(_) => serde_json::json!({
                            "status": "timeout",
                            "timeout_secs": probe_timeout.as_secs(),
                        }),
                    }
                }
            }
        };

        let scripts_ok = scripts_round_trip["status"] == "ok";
        let plugin_ok = plugin_round_trip["status"] == "ok";
        let frames_fresh = frame_stream["staleness_ms"].as_u64().map(|s| s < 15_000).unwrap_or(false);

        let (verdict, advice) = if !session_present {
            ("no_session", "No admin Web Console WS session for this connection_string. Connect from Web Console first; remote tools cannot reach this client at all.")
        } else if scripts_ok && plugin_ok {
            ("healthy", "All round-trip channels respond. Safe to run remote scripts, plugin tools, and stress suites.")
        } else if frames_fresh {
            ("one_way_alive_round_trips_dead", "Frames stream but request/response channels don't answer — the client's responder loop is wedged or the build predates these channels. Restart the client app (or drive it via remote egui input as a fallback). Do not start remote scripts; they will time out.")
        } else {
            ("degraded", "Session registered but neither fresh frames nor round-trip responses. Client likely disconnected uncleanly; expect reconnect or restart before remote operations.")
        };

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "connection_string": cs,
            "verdict": verdict,
            "advice": advice,
            "channels": {
                "db_heartbeat": heartbeat,
                "admin_ws_session": { "status": if session_present { "ok" } else { "missing" } },
                "egui_frame_stream": frame_stream,
                "scripts_round_trip": scripts_round_trip,
                "plugin_call_round_trip": plugin_round_trip,
            },
        }))
        .map_err(to_internal)?]))
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ok": true,
            "connection_string": p.connection_string,
            "key": p.key,
            "x": x,
            "y": y,
            "events_enqueued": seq.len(),
        }))
        .map_err(to_internal)?]))
    }

    // ── egui inspection (native egui 0.35 InspectionPlugin, local app) ───

    #[tool(
        name = "egui_inspect_status",
        description = "Check the LOCAL app's native egui inspection server (egui_inspection on 127.0.0.1:5719). Returns app label + egui version when connected."
    )]
    async fn egui_inspect_status(&self) -> Result<CallToolResult, ErrorData> {
        let result = super::egui_inspect::request(egui_inspection::Request::GetInfo).await;
        let json = match result {
            Ok(egui_inspection::Response::Info { label, egui_version }) => serde_json::json!({
                "connected": true, "label": label, "egui_version": egui_version,
                "addr": super::egui_inspect::INSPECT_ADDR,
            }),
            Ok(other) => serde_json::json!({ "connected": true, "unexpected": format!("{other:?}") }),
            Err(e) => serde_json::json!({ "connected": false, "error": e.to_string() }),
        };
        Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(to_internal)?]))
    }

    #[tool(
        name = "egui_inspect_tree",
        description = "Read the LOCAL app's live AccessKit widget tree (roles, labels, values, bounds in logical points) as JSON. Use a node's bounds to drive egui_inspect_click."
    )]
    async fn egui_inspect_tree(&self) -> Result<CallToolResult, ErrorData> {
        match super::egui_inspect::request(egui_inspection::Request::GetTree)
            .await
            .map_err(to_internal)?
        {
            egui_inspection::Response::Tree { step, pixels_per_point, accesskit } => {
                let tree = accesskit
                    .map(|t| serde_json::to_value(&t).unwrap_or(serde_json::Value::Null));
                Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
                    "step": step,
                    "pixels_per_point": pixels_per_point,
                    "tree": tree,
                }))
                .map_err(to_internal)?]))
            }
            egui_inspection::Response::Error { message } => Err(to_internal(message)),
            other => Err(to_internal(format!("unexpected response: {other:?}"))),
        }
    }

    #[tool(
        name = "egui_inspect_screenshot",
        description = "Capture a PNG screenshot of the LOCAL app as base64. pixels_per_point defaults to 1.0 (logical-point size)."
    )]
    async fn egui_inspect_screenshot(
        &self,
        Parameters(p): Parameters<EguiInspectScreenshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use base64::Engine;
        match super::egui_inspect::request(egui_inspection::Request::GetScreenshot {
            pixels_per_point: p.pixels_per_point,
        })
        .await
        .map_err(to_internal)?
        {
            egui_inspection::Response::Screenshot(png) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&png.bytes);
                Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
                    "width": png.size[0],
                    "height": png.size[1],
                    "mime": "image/png",
                    "base64": b64,
                }))
                .map_err(to_internal)?]))
            }
            egui_inspection::Response::Error { message } => Err(to_internal(message)),
            other => Err(to_internal(format!("unexpected response: {other:?}"))),
        }
    }

    #[tool(
        name = "egui_inspect_click",
        description = "Click at (x,y) in the LOCAL app (logical points; read coords from an egui_inspect_tree node's bounds)."
    )]
    async fn egui_inspect_click(
        &self,
        Parameters(p): Parameters<EguiInspectClickParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let button = super::egui_inspect::parse_button(p.button.as_deref().unwrap_or("primary"));
        let events = super::egui_inspect::click_events(p.x, p.y, button, p.double);
        super::egui_inspect::request(egui_inspection::Request::ApplyEvents { events })
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ok": true, "x": p.x, "y": p.y, "double": p.double,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "egui_inspect_type",
        description = "Type text into the LOCAL app's focused widget (egui Text event). Click/focus the field first."
    )]
    async fn egui_inspect_type(
        &self,
        Parameters(p): Parameters<EguiInspectTypeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let events = vec![eframe::egui::Event::Text(p.text.clone())];
        super::egui_inspect::request(egui_inspection::Request::ApplyEvents { events })
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ok": true, "text": p.text,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "egui_inspect_press_key",
        description = "Press a key in the LOCAL app (down+up). Key name e.g. Enter, Tab, Escape, ArrowDown, Backspace, A."
    )]
    async fn egui_inspect_press_key(
        &self,
        Parameters(p): Parameters<EguiInspectKeyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let key = eframe::egui::Key::from_name(&p.key)
            .ok_or_else(|| to_internal(format!("unknown egui key: {}", p.key)))?;
        let events = super::egui_inspect::key_events(key);
        super::egui_inspect::request(egui_inspection::Request::ApplyEvents { events })
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ok": true, "key": p.key,
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

            if let Err(e) = super::sdk_vendor::ensure_vendored_sdk() {
                log::warn!("ensure_vendored_sdk failed: {e}");
            }

            if !cargo_toml.exists() {
                tokio::fs::write(&cargo_toml, plugin_cargo_toml(&p.plugin_id))
                    .await
                    .map_err(|e| to_internal(format!("write Cargo.toml: {e}")))?;
            }

            Ok(CallToolResult::success(vec![ContentBlock::json(
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

            Ok(CallToolResult::success(vec![ContentBlock::json(
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

        if let Err(e) = super::sdk_vendor::ensure_vendored_sdk() {
            log::warn!("ensure_vendored_sdk failed: {e}");
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
            return Ok(CallToolResult::success(vec![ContentBlock::json(
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

        Ok(CallToolResult::success(vec![ContentBlock::json(
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

            Ok(CallToolResult::success(vec![ContentBlock::json(
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

            Ok(CallToolResult::success(vec![ContentBlock::json(
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

        Ok(CallToolResult::success(vec![ContentBlock::json(
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
            Ok(CallToolResult::success(vec![ContentBlock::json(
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

        // Register the ack waiter before sending so the result can't race past us.
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<(bool, String)>();
        if let Ok(mut pending) = super::remote_script_notify::DEPLOY_ACK_PENDING.lock() {
            pending.insert(p.plugin_id.clone(), ack_tx);
        }

        if let Err(e) = super::remote_egui_control::hub()
            .send_raw_binary(&p.connection_string, serialized)
        {
            if let Ok(mut pending) = super::remote_script_notify::DEPLOY_ACK_PENDING.lock() {
                pending.remove(&p.plugin_id);
            }
            return Err(to_internal(e));
        }

        let ack = tokio::time::timeout(std::time::Duration::from_secs(20), ack_rx).await;
        if ack.is_err() {
            if let Ok(mut pending) = super::remote_script_notify::DEPLOY_ACK_PENDING.lock() {
                pending.remove(&p.plugin_id);
            }
        }

        let body = match ack {
            Ok(Ok((load_success, load_message))) => serde_json::json!({
                "plugin_id": p.plugin_id,
                "connection_string": p.connection_string,
                "deployed_remote": load_success,
                "load_acknowledged": true,
                "load_message": load_message,
                "artifact_bytes": size,
            }),
            _ => serde_json::json!({
                "plugin_id": p.plugin_id,
                "connection_string": p.connection_string,
                "deployed_remote": true,
                "load_acknowledged": false,
                "artifact_bytes": size,
                "note": "Bytes sent but no LoadWasmPluginResult ack within 20s — old client build or wedged channel. Verify with call_remote_plugin_tool or the remote MCP's list_plugins.",
            }),
        };

        Ok(CallToolResult::success(vec![ContentBlock::json(body).map_err(to_internal)?]))
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
        let args_json = serde_json::to_string(&args).map_err(|e| to_internal(e.to_string()))?;

        let cmd = crate::Cmd::CallRemotePluginTool {
            request_id: request_id.clone(),
            plugin_id: p.plugin_id.clone(),
            tool_name: p.tool_name.clone(),
            args_json: args_json.clone(),
        };
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;
        // (MCP-level start_call is fired by the `call_tool` interceptor on
        // the `ServerHandler` impl — no per-tool hook needed here.)
        let _ = args_json; // consumed by `cmd` above

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
            Ok(CallToolResult::success(vec![plugin_value_to_content(
                value,
            )?]))
        } else {
            Ok(CallToolResult::error(vec![
                ContentBlock::text(result_json),
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
            Ok(CallToolResult::success(vec![ContentBlock::json(
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
            ContentBlock::json(serde_json::json!({
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
                ContentBlock::json(serde_json::json!(e)).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("No plugin found with ID '{}'", p.plugin_id))
            ])),
        }
    }

    #[tool(
        name = "list_registry_plugins",
        description = "List EVERY plugin in the SurrealDB plugin_registry (id, plugin_id, name, description, version, author, tags, tools, wasm_bucket_path, created_at, updated_at) — the whole catalog MINUS the heavy source_code. The registry is small, so this is the cheap way to see all available plugins at a glance and avoid missing one (search_plugins only returns keyword matches). Distinct from `list_plugins`, which lists plugins currently loaded in THIS process. Flow: discover here → fetch_plugin → plugin_deploy / plugin_deploy_remote."
    )]
    async fn list_registry_plugins(
        &self,
        Parameters(p): Parameters<ListRegistryPluginsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(200).clamp(1, 1000) as i64;
        let rows: Vec<serde_json::Value> = database::db()
            .query(
                "SELECT id, plugin_id, name, description, version, author, tags, tools, \
                        wasm_bucket_path, created_at, updated_at \
                 FROM plugin_registry ORDER BY updated_at DESC LIMIT $limit",
            )
            .bind(("limit", limit))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "count": rows.len(), "plugins": rows }),
        )
        .map_err(to_internal)?]))
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

        let (name, version, tools_json, abi_version, fingerprint) = {
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
            let abi_version = matching.and_then(|pi| pi.abi_version);
            let fingerprint = matching.and_then(|pi| pi.fingerprint);

            let tools_json: Vec<database::schema::PluginToolInfo> = mgr.plugins.iter()
                .find(|plug| plug.id() == p.plugin_id)
                .map(|plug| plug.mcp_tools())
                .unwrap_or_default()
                .iter()
                .map(|td| database::schema::PluginToolInfo {
                    name: td.name.clone(),
                    description: td.description.clone(),
                    parameters_schema: td.parameters_schema.clone(),
                })
                .collect();

            (name, version, tools_json, abi_version, fingerprint)
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
            abi_version,
            fingerprint: fingerprint.map(|f| f as i64),
            ..Default::default()
        };

        database::schema::PluginRegistryEntry::upsert(&entry)
            .await
            .map_err(to_internal)?;

        Ok(CallToolResult::success(vec![ContentBlock::json(
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
            if let Err(e) = super::sdk_vendor::ensure_vendored_sdk() {
                log::warn!("ensure_vendored_sdk failed: {e}");
            }
            let cargo_toml_path = dir.join("Cargo.toml");
            if !cargo_toml_path.exists() {
                let _ = tokio::fs::write(&cargo_toml_path, plugin_cargo_toml(&p.plugin_id)).await;
            }
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(
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
        use super::tool_warnings::{attach_warnings, ToolWarning};
        use database::schema::RecordIdExt;

        let task_ref = match p.task_id.as_deref() {
            Some(s) => Some(require_record(s, database::schema::TASK_TABLE, "task_id").await?),
            None => None,
        };
        let service_order = match p.service_order_id.as_deref() {
            Some(s) => {
                Some(require_record(s, database::schema::TICKET_TABLE, "service_order_id").await?)
            }
            None => None,
        };

        let (customer_id, computer_id) = resolve_entity_links_mcp(
            Some(p.connection_string.clone()),
            &p.customer_id,
            &p.computer_id,
        )
        .await?;

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
        let id_str = id.key_string();
        super::diagnostic_session_registry::register(&session.connection_string, &id_str);

        // Reconcile sees the created id plus whatever task link resolves below.
        let mut created = session.clone();
        created.id = id.clone();
        created.started_at = chrono::Utc::now().into();

        let mut warnings: Vec<ToolWarning> = Vec::new();
        if created.task_ref.is_none() {
            match created.resolve_open_service_task().await {
                Ok(Some((task, so))) => {
                    match database::schema::DiagnosticSession::link_to_task(
                        &id,
                        Some(&task),
                        Some(&so),
                    )
                    .await
                    {
                        Ok(()) => {
                            created.task_ref = Some(task);
                            created.service_order = Some(so);
                        }
                        Err(e) => {
                            log::warn!("create_diagnostic_session: task auto-link failed: {e}")
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("create_diagnostic_session: task resolution failed: {e}"),
            }
        }
        if created.task_ref.is_none() {
            warnings.push(
                ToolWarning::warn(
                    "session_unlinked",
                    "No service task could be resolved for this session; records created \
                     against it will carry no task_ref until it is linked.",
                )
                .with_fix(format!(
                    "link_diagnostic_to_task {{ session_id: \"{id_str}\", task_id: \"task:<key>\" }} once the service task is known"
                )),
            );
        }

        let reconciled = database::schema::crash_intel::reconcile_session_links(&created)
            .await
            .unwrap_or_else(|e| {
                log::warn!("create_diagnostic_session: reconcile failed: {e}");
                Default::default()
            });
        if reconciled.total() > 0 {
            warnings.push(ToolWarning::info(
                "orphans_claimed",
                format!("Reconcile on session create: {}.", reconciled.summary()),
            ));
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "session_id": id_str,
                "task_ref": created.task_ref.as_ref().map(RecordIdExt::key_string),
                "service_order": created.service_order.as_ref().map(RecordIdExt::key_string),
                "reconciled": reconciled,
            }),
            warnings,
        ))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "validate_connection_links",
        description = "Validate customer/computer FK health for a connected client before create_diagnostic_session. Returns issues and resolved canonical ids when valid. Call with connection_string alone to check the links the client already carries; pass customer_id/computer_id only to validate specific ids against it."
    )]
    async fn validate_connection_links(
        &self,
        Parameters(p): Parameters<ValidateConnectionLinksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bundle = LinkBundle {
            connection_string: Some(p.connection_string.clone()),
            customer_id: p
                .customer_id
                .as_deref()
                .and_then(|s| optional_record_id(s, database::schema::CUSTOMER_TABLE)),
            computer_id: p
                .computer_id
                .as_deref()
                .and_then(|s| optional_record_id(s, database::schema::COMPUTER_TABLE)),
        };
        let validation = validate_link_bundle(&bundle).await;
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ok": validation.ok,
            "issues": validation.issues,
            "resolved_customer_id": validation.resolved_customer_id.map(|r| r.key_string()),
            "resolved_computer_id": validation.resolved_computer_id.map(|r| r.key_string()),
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "repair_entity_links",
        description = "Repair FK graph for a connected client: repoint to canonical computer:HOST:hash9, fix diagnostic_session computer_id, set connected_client.computer. Use for DESKTOP-HQAF13L-style bad linkage."
    )]
    async fn repair_entity_links(
        &self,
        Parameters(p): Parameters<RepairEntityLinksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = repair_connection_links(&p.connection_string)
            .await
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::json(report).map_err(to_internal)?]))
    }

    #[tool(
        name = "link_connected_client",
        description = "Link a connected client to a customer and its canonical computer:HOST:hash9 record, creating the computer row when missing. Use for the hardware-swap case: a machine reconnects under a new disk-persistent client id with null customer/computer (repair_entity_links can't fix that — it only repoints existing links). Upserts the computer (sets customer + hostname only; never clobbers existing specs), then sets connected_client.customer/computer and optionally friendly_name. Component specs repopulate from the client's own check-in."
    )]
    async fn link_connected_client(
        &self,
        Parameters(p): Parameters<LinkConnectedClientParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = database::schema::entity_link::link_connected_client_record(
            &p.connection_string,
            &p.customer_id,
            p.friendly_name.as_deref(),
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::json(report).map_err(to_internal)?]))
    }

    #[tool(
        name = "link_diagnostic_to_task",
        description = "Retroactively link an existing diagnostic_session to an in-house task and/or service_order. Use after a customer checks in their device for service so the diagnostic appears in the task modal's Diagnostics tab."
    )]
    async fn link_diagnostic_to_task(
        &self,
        Parameters(p): Parameters<LinkDiagnosticToTaskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use super::tool_warnings::{attach_warnings, ToolWarning};
        use database::schema::RecordIdExt;

        let session_id = parse_record_id(
            &p.session_id,
            database::schema::DIAGNOSTIC_SESSION_TABLE,
        );
        let task_ref = match p.task_id.as_deref() {
            Some(s) => Some(require_record(s, database::schema::TASK_TABLE, "task_id").await?),
            None => None,
        };
        let service_order = match p.service_order_id.as_deref() {
            Some(s) => {
                Some(require_record(s, database::schema::TICKET_TABLE, "service_order_id").await?)
            }
            None => None,
        };
        if task_ref.is_none() && service_order.is_none() {
            return Err(ErrorData::invalid_params(
                "link_diagnostic_to_task: at least one of task_id or service_order_id must be provided".to_string(),
                None,
            ));
        }
        let session = match database::schema::DiagnosticSession::get(&session_id.key_string())
            .await
        {
            Ok(Some(s)) => Some(s),
            Ok(None) => {
                return Err(ErrorData::invalid_params(
                    format!("diagnostic_session '{}' not found", p.session_id),
                    None,
                ))
            }
            // Legacy row shapes must stay linkable; skip the reconcile sweep.
            Err(e) => {
                log::warn!(
                    "link_diagnostic_to_task: session row unreadable, linking without \
                     reconcile: {e}"
                );
                None
            }
        };
        database::schema::DiagnosticSession::link_to_task(
            &session_id,
            task_ref.as_ref(),
            service_order.as_ref(),
        )
        .await
        .map_err(to_internal)?;

        // Sweep the now-linked task onto the session's existing records.
        let mut warnings: Vec<ToolWarning> = Vec::new();
        let reconciled = match session {
            Some(mut session) => {
                if task_ref.is_some() {
                    session.task_ref = task_ref.clone();
                }
                if service_order.is_some() {
                    session.service_order = service_order.clone();
                }
                database::schema::crash_intel::reconcile_session_links(&session)
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("link_diagnostic_to_task: reconcile failed: {e}");
                        Default::default()
                    })
            }
            None => {
                warnings.push(ToolWarning::warn(
                    "completeness_skipped",
                    "Session row could not be read (legacy shape) — the link was written but \
                     the reconcile sweep was skipped.",
                ));
                Default::default()
            }
        };
        if reconciled.total() > 0 {
            warnings.push(ToolWarning::info(
                "orphans_claimed",
                format!("Reconcile on task link: {}.", reconciled.summary()),
            ));
        }
        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "session_id": p.session_id,
                "task_id": p.task_id,
                "service_order_id": p.service_order_id,
                "linked": true,
                "reconciled": reconciled,
            }),
            warnings,
        ))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "log_diagnostic_entry",
        description = "Log an entry against an open diagnostic_session. Allowed categories: 'finding' (discovered issue), 'action' (step taken), 'note' (general observation), 'error' (tool/command failed), 'system_info', 'network_info', 'security_alert', 'performance_note', 'customer_note', 'recommendation'. Anything else is recorded as 'note'. IMPORTANT: 'recommendation' entries are informational only — nobody is notified and nothing is tracked. If a recommendation requires HANDS-ON tech work (BIOS, hardware, bench tools, physical access), you MUST ALSO call create_ai_task so the tech actually receives it as a tracked checklist. Embeddings (title + detail, 768-dim HNSW index `diag_embedding`) are computed app-side via the shared Ollama endpoint on insert; entries are stored without an embedding if the endpoint is unreachable."
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
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "entry_id": id_str, "session_id": p.session_id }),
        )
        .map_err(to_internal)?]))
    }

    // ── AI Task (hands-on handoff) tools ─────────────────────────────────

    /// Resolve a user record by email or exact (case-insensitive) name.
    async fn resolve_user_ident(ident: &str) -> Result<Option<database::schema::User>, ErrorData> {
        let users: Vec<database::schema::User> = database::db()
            .query("SELECT * FROM user WHERE email = $ident OR string::lowercase(name) = string::lowercase($ident) LIMIT 1")
            .bind(("ident", ident.to_string()))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(users.into_iter().next())
    }

    /// Write-once mirror of checklist steps into diagnostic_entry (embeddings
    /// computed per entry; failures degrade to unlinked items). Runs detached,
    /// so it re-reads each item: a step edited before the mirror runs is
    /// mirrored with its current text, and one removed before the mirror runs
    /// is skipped (no orphan entry). If the item vanishes between create and
    /// backlink, the just-created entry is deleted rather than left dangling.
    async fn mirror_ai_task_steps(
        session_ref: database::schema::RecordId,
        ai_task_id: database::schema::RecordId,
        item_ids: Vec<database::schema::RecordId>,
        position_offset: usize,
    ) {
        use database::schema::RecordIdExt;
        for (idx, item_id) in item_ids.into_iter().enumerate() {
            let current = match database::schema::AiTaskItem::get(&item_id).await {
                Ok(Some(it)) => it,
                Ok(None) => continue, // removed before the mirror ran
                Err(e) => {
                    log::warn!("mirror_ai_task_steps: item re-read failed: {e}");
                    continue;
                }
            };
            if current.entry_ref.is_some() {
                continue; // already mirrored
            }
            let entry = database::schema::DiagnosticEntry {
                session_ref: session_ref.clone(),
                category: database::schema::DiagnosticCategory::Recommendation,
                title: format!("Hands-on step {}", position_offset + idx + 1),
                detail: current.text.clone(),
                data: Some(serde_json::json!({
                    "ai_task": ai_task_id.key_string(),
                    "ai_task_item": item_id.key_string(),
                })),
                ..Default::default()
            };
            match database::schema::DiagnosticEntry::create(&entry).await {
                Ok(entry_id) => {
                    let updated: Vec<database::schema::AiTaskItem> = database::db()
                        .query("UPDATE $item SET entry_ref = $entry RETURN AFTER")
                        .bind(("item", item_id))
                        .bind(("entry", entry_id.clone()))
                        .await
                        .and_then(|mut r| r.take(0))
                        .unwrap_or_default();
                    // Item deleted between re-read and backlink → drop the orphan.
                    if updated.is_empty() {
                        let _ = database::db()
                            .query("DELETE $entry")
                            .bind(("entry", entry_id))
                            .await;
                    }
                }
                Err(e) => log::warn!("mirror_ai_task_steps: mirror entry failed: {e}"),
            }
        }
    }

    #[tool(
        name = "create_ai_task",
        description = "Hand off HANDS-ON work to the technician. Call when a diagnosis concludes physical/BIOS/bench work is required (e.g. 'disable XMP', 'reseat DIMMs', 're-run OCCT at stock'). Creates an AI Task — a checklist overlay on the service task — which pops a 'requires your attention' modal on the assigned tech's desktop and appears in their AI Tasks column. Steps also log to the diagnostic session as recommendation entries. The task to attach to auto-resolves (explicit task_id > the session's task_ref > the task on the connection's open service order, which then gets linked to the session) — you do NOT need to call link_diagnostic_to_task first. Assignee resolves: explicit assignee_email > service ticket technician > task assignee. Poll get_ai_task_status to see progress; the operator (not the AI) closes the task.\n\nEACH STEP IS A PHYSICAL TODO FOR A HUMAN — never anything else. RULES for the steps array:\n1. This list is NOT a log. Never add an item that records what happened, notes a mistake you made, states a finding, or explains context — that ALL belongs in log_diagnostic_entry, not here. An item with no concrete human action to perform does not belong on the list.\n2. Never add a step you can do yourself through the plugin system or an MCP tool (run a script, read events, snapshot drivers, analyze a dump, query the DB, toggle a setting reachable via com.mastertech.repair, etc.). Do it, then log it. The list is ONLY for actions that require physical access or a human decision you cannot perform remotely.\n3. Write each step short, imperative, and self-contained: one concrete action a tech can check off, with the specific target. 'Reseat both DIMMs in slots A2/B2' — not 'RAM'. 'Replace SATA data cable on the D: drive (WD20EZBX) and move to port 3' — not 'look at the disk'. Thorough but terse; no narration, no rationale paragraphs."
    )]
    async fn create_ai_task(
        &self,
        Parameters(p): Parameters<CreateAiTaskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::RecordIdExt;

        if p.steps.is_empty() || p.steps.len() > 30 || p.steps.iter().any(|s| s.trim().is_empty()) {
            return Err(ErrorData::invalid_params(
                "create_ai_task: steps must be 1-30 non-empty strings".to_string(), None));
        }

        // Resolve the diagnostic session (explicit id, or active-registry lookup).
        let session_key = match (&p.session_id, &p.connection_string) {
            (Some(sid), _) => sid.clone(),
            (None, Some(cs)) => super::diagnostic_session_registry::get(cs).ok_or_else(|| {
                ErrorData::invalid_params(format!(
                    "create_ai_task: no active diagnostic session for '{cs}' — pass session_id"), None)
            })?,
            (None, None) => return Err(ErrorData::invalid_params(
                "create_ai_task: session_id or connection_string is required".to_string(), None)),
        };
        let session_ref = parse_record_id(&session_key, database::schema::DIAGNOSTIC_SESSION_TABLE);
        let session: Option<database::schema::DiagnosticSession> =
            database::db().select(session_ref.clone()).await.map_err(to_internal)?;
        let session = session.ok_or_else(|| ErrorData::invalid_params(
            format!("create_ai_task: diagnostic session '{session_key}' not found"), None))?;

        // One open AI task per session — retries must append, not spam popups.
        if let Some(existing) = database::schema::AiTask::get_open_for_session(&session_ref)
            .await.map_err(to_internal)?
        {
            return Err(ErrorData::invalid_params(format!(
                "create_ai_task: ai_task '{}' already open for this session — use add_ai_task_steps",
                existing.id.key_string()), None));
        }

        // Task to attach to: explicit task_id > session.task_ref > auto-resolve
        // from the connection's open service order (then link it to the session).
        let (task_ref, auto_linked) = match p.task_id.as_deref() {
            Some(t) => (
                require_record(t, database::schema::TASK_TABLE, "task_id").await?,
                false,
            ),
            None => match session.task_ref.clone() {
                Some(t) => (t, false),
                None => match session.resolve_open_service_task().await.map_err(to_internal)? {
                    Some((task, service_order)) => {
                        if let Err(e) = database::schema::DiagnosticSession::link_to_task(
                            &session_ref,
                            Some(&task),
                            Some(&service_order),
                        )
                        .await
                        {
                            log::warn!("create_ai_task: auto-link session→task failed: {e}");
                        }
                        (task, true)
                    }
                    None => return Err(ErrorData::invalid_params(
                        "create_ai_task: no task to attach to — session has no task_ref and no \
                         service order/task was found for this connection. Pass task_id or run \
                         link_diagnostic_to_task first.".to_string(),
                        None)),
                },
            },
        };
        // Sweep the resolved task onto the session's unlinked records.
        {
            let mut linked_session = session.clone();
            linked_session.task_ref = Some(task_ref.clone());
            if let Err(e) =
                database::schema::crash_intel::reconcile_session_links(&linked_session).await
            {
                log::warn!("create_ai_task: reconcile failed: {e}");
            }
        }
        let task: Option<database::schema::LiveTaskPayload> =
            database::db().select(task_ref.clone()).await.map_err(to_internal)?;
        let task = task.ok_or_else(|| ErrorData::invalid_params(
            format!("create_ai_task: task '{}' not found", task_ref.key_string()), None))?;

        // Ticket tech + customer name (used for assignee default and popup text).
        let (ticket_tech, ticket_customer): (Option<String>, Option<String>) =
            match task.service_ticket.as_ref() {
                Some(ticket) => {
                    let mut res = database::db()
                        .query("SELECT VALUE tech FROM $t")
                        .query("SELECT VALUE customer.name FROM $t")
                        .bind(("t", ticket.clone()))
                        .await
                        .map_err(to_internal)?;
                    let tech: Vec<Option<String>> = res.take(0).unwrap_or_default();
                    let cust: Vec<Option<String>> = res.take(1).unwrap_or_default();
                    (
                        tech.into_iter().flatten().find(|s| !s.trim().is_empty()),
                        cust.into_iter().flatten().find(|s| !s.trim().is_empty()),
                    )
                }
                None => (None, None),
            };

        // Assignee chain: explicit override > ticket tech > task assignee. Never $auth.
        let assignee_user = match p.assignee_email.as_deref() {
            Some(ident) => Some(Self::resolve_user_ident(ident).await?.ok_or_else(|| {
                ErrorData::invalid_params(format!(
                    "create_ai_task: no user matches assignee_email '{ident}'"), None)
            })?),
            None => match ticket_tech.as_deref() {
                Some(tech) => Self::resolve_user_ident(tech).await?,
                None => None,
            },
        };
        let assignee = assignee_user.as_ref()
            .map(|u| u.get_id())
            .unwrap_or_else(|| task.assignee.clone());

        let service_number = task.service_number.clone().unwrap_or_default();
        // Popup text fallback: task_name is conventionally "{customer} - {service#}".
        let customer_name = ticket_customer.unwrap_or_else(|| {
            let suffix = format!(" - {service_number}");
            match task.task_name.strip_suffix(suffix.as_str()) {
                Some(prefix) if !service_number.is_empty() => prefix.to_string(),
                _ => task.task_name.clone(),
            }
        });

        let ai_task = database::schema::AiTask {
            task_ref: task_ref.clone(),
            session_ref: session_ref.clone(),
            assignee: assignee.clone(),
            requested_by: assignee.clone(), // placeholder; DEFAULT $auth.id would be ideal but
                                            // .content() writes all fields — overwritten below.
            title: p.title.clone().unwrap_or_else(|| "Hands-on work needed".to_string()),
            customer_name,
            service_number,
            connection_string: Some(session.connection_string.clone()),
            ..Default::default()
        };
        let (ai_task_id, item_ids) =
            database::schema::AiTask::create_with_items(&ai_task, &p.steps)
                .await
                .map_err(to_internal)?;

        // .content() bypassed the DEFAULT — stamp the true operator explicitly.
        let _ = database::db()
            .query("UPDATE $id SET requested_by = $auth.id")
            .bind(("id", ai_task_id.clone()))
            .await;

        tokio::spawn(Self::mirror_ai_task_steps(session_ref, ai_task_id.clone(), item_ids.clone(), 0));

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ai_task_id": ai_task_id.key_string(),
            "task_id": task_ref.key_string(),
            "auto_linked": auto_linked,
            "assignee": {
                "id": assignee.key_string(),
                "name": assignee_user.as_ref().map(|u| u.get_name().to_string()),
            },
            "item_ids": item_ids.iter().map(|i| i.key_string()).collect::<Vec<_>>(),
            "note": "Tech has been notified. Poll get_ai_task_status for progress; a human operator closes the AI task.",
        })).map_err(to_internal)?]))
    }

    #[tool(
        name = "add_ai_task_steps",
        description = "Append hands-on steps to an existing AI task. Reopens it (card returns to the tech's board, tech is re-notified) — use after reviewing completed work when more is needed. The AI cannot close AI tasks; a human operator does."
    )]
    async fn add_ai_task_steps(
        &self,
        Parameters(p): Parameters<AddAiTaskStepsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::RecordIdExt;

        if p.steps.is_empty() || p.steps.len() > 30 || p.steps.iter().any(|s| s.trim().is_empty()) {
            return Err(ErrorData::invalid_params(
                "add_ai_task_steps: steps must be 1-30 non-empty strings".to_string(), None));
        }
        let id = parse_record_id(&p.ai_task_id, database::schema::AI_TASK_TABLE);
        let full = database::schema::AiTask::get_full(&id).await.map_err(to_internal)?;
        let (task, existing_items) = full.ok_or_else(|| ErrorData::invalid_params(
            format!("add_ai_task_steps: ai_task '{}' not found", p.ai_task_id), None))?;
        if task.status == database::schema::AiTaskStatus::Closed {
            return Err(ErrorData::invalid_params(
                "add_ai_task_steps: ai_task is closed — create_ai_task for new work".to_string(), None));
        }

        let new_item_ids = database::schema::AiTask::add_steps(&id, &p.steps)
            .await
            .map_err(to_internal)?;

        tokio::spawn(Self::mirror_ai_task_steps(
            task.session_ref.clone(), id.clone(), new_item_ids.clone(), existing_items.len()));

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ai_task_id": id.key_string(),
            "new_item_ids": new_item_ids.iter().map(|i| i.key_string()).collect::<Vec<_>>(),
            "status": "open",
        })).map_err(to_internal)?]))
    }

    #[tool(
        name = "edit_ai_task_item",
        description = "Rewrite the text of ONE unchecked checklist item — use to fix a poorly-worded or incorrect step you added, not to add or reword completed work. Only items the tech has not yet checked can be edited, and only on a non-closed task. Keep the new text a short, imperative, self-contained physical action (same rules as create_ai_task steps). Also updates the mirrored diagnostic-session recommendation entry."
    )]
    async fn edit_ai_task_item(
        &self,
        Parameters(p): Parameters<EditAiTaskItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::RecordIdExt;

        let text = p.text.trim();
        if text.is_empty() || text.len() > 500 {
            return Err(ErrorData::invalid_params(
                "edit_ai_task_item: text must be a non-empty step under 500 chars".to_string(),
                None,
            ));
        }
        let item_id = parse_record_id(&p.item_id, database::schema::AI_TASK_ITEM_TABLE);
        let item = database::schema::AiTaskItem::get(&item_id)
            .await
            .map_err(to_internal)?
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!("edit_ai_task_item: item '{}' not found", p.item_id),
                    None,
                )
            })?;
        if item.checked {
            return Err(ErrorData::invalid_params(
                "edit_ai_task_item: item is already checked off by the tech — editing completed \
                 work would misrepresent the record. Add a new step with add_ai_task_steps instead."
                    .to_string(),
                None,
            ));
        }
        match database::schema::AiTask::get_full(&item.ai_task_ref)
            .await
            .map_err(to_internal)?
        {
            None => {
                return Err(ErrorData::invalid_params(
                    format!(
                        "edit_ai_task_item: parent ai_task '{}' not found",
                        item.ai_task_ref.key_string()
                    ),
                    None,
                ))
            }
            Some((task, _)) if task.status == database::schema::AiTaskStatus::Closed => {
                return Err(ErrorData::invalid_params(
                    "edit_ai_task_item: the AI task is closed".to_string(),
                    None,
                ))
            }
            _ => {}
        }

        // Atomic write re-asserts unchecked + task-open, closing the window
        // where a tech checks the box between the read above and this update.
        let updated = database::schema::AiTaskItem::edit_text_if_editable(&item_id, text)
            .await
            .map_err(to_internal)?
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    "edit_ai_task_item: item was checked off or its task closed concurrently — \
                     re-check with get_ai_task_status"
                        .to_string(),
                    None,
                )
            })?;

        // Keep the mirrored session recommendation in sync (text + embedding).
        if let Some(entry) = updated.entry_ref.as_ref() {
            if let Err(e) =
                database::schema::DiagnosticEntry::update_detail(entry, text).await
            {
                log::warn!("edit_ai_task_item: mirror entry update failed: {e}");
            }
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "item_id": item_id.key_string(),
            "ai_task_id": item.ai_task_ref.key_string(),
            "text": text,
            "edited": true,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "remove_ai_task_item",
        description = "Delete ONE unchecked checklist item from an AI task — use to drop a step you added in error (e.g. something informational, a duplicate, or work you can do yourself via MCP). Only unchecked items on a non-closed task can be removed, and never the task's last remaining item (to end a task, the operator closes it). Also deletes the mirrored diagnostic-session recommendation entry. If removing the item leaves every remaining item checked, the task advances to awaiting_followup."
    )]
    async fn remove_ai_task_item(
        &self,
        Parameters(p): Parameters<RemoveAiTaskItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::RecordIdExt;

        let item_id = parse_record_id(&p.item_id, database::schema::AI_TASK_ITEM_TABLE);
        let item = database::schema::AiTaskItem::get(&item_id)
            .await
            .map_err(to_internal)?
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!("remove_ai_task_item: item '{}' not found", p.item_id),
                    None,
                )
            })?;
        if item.checked {
            return Err(ErrorData::invalid_params(
                "remove_ai_task_item: item is checked off by the tech — removing it would erase \
                 completed-work history. The tech can uncheck it if it was done in error."
                    .to_string(),
                None,
            ));
        }
        let parent = database::schema::AiTask::get_full(&item.ai_task_ref)
            .await
            .map_err(to_internal)?;
        let Some((task, items)) = parent else {
            return Err(ErrorData::invalid_params(
                format!(
                    "remove_ai_task_item: parent ai_task '{}' not found",
                    item.ai_task_ref.key_string()
                ),
                None,
            ));
        };
        if task.status == database::schema::AiTaskStatus::Closed {
            return Err(ErrorData::invalid_params(
                "remove_ai_task_item: the AI task is closed".to_string(),
                None,
            ));
        }
        if items.len() <= 1 {
            return Err(ErrorData::invalid_params(
                "remove_ai_task_item: this is the task's only item — a checklist cannot be empty. \
                 Add a replacement step first, or have the operator close the task."
                    .to_string(),
                None,
            ));
        }

        // Atomic delete re-asserts unchecked + task-open, closing the window
        // where a tech checks the box between the read above and this delete.
        let removed = database::schema::AiTaskItem::remove_if_unchecked(&item_id)
            .await
            .map_err(to_internal)?
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    "remove_ai_task_item: item was checked off or its task closed concurrently — \
                     re-check with get_ai_task_status"
                        .to_string(),
                    None,
                )
            })?;
        // Drop the mirrored session recommendation so it doesn't linger.
        if let Some(entry) = removed.entry_ref.as_ref() {
            let res = database::db()
                .query("DELETE $entry")
                .bind(("entry", entry.clone()))
                .await;
            if let Err(e) = res {
                log::warn!("remove_ai_task_item: mirror entry delete failed: {e}");
            }
        }
        database::schema::AiTask::reevaluate_completion(&item.ai_task_ref)
            .await
            .map_err(to_internal)?;

        let full = database::schema::AiTask::get_full(&item.ai_task_ref)
            .await
            .map_err(to_internal)?;
        let (status, remaining) = full
            .map(|(t, its)| {
                (
                    t.status.as_str().to_string(),
                    its.iter().filter(|i| !i.checked).count(),
                )
            })
            .unwrap_or_else(|| ("open".to_string(), 0));

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "removed_item_id": item_id.key_string(),
            "ai_task_id": item.ai_task_ref.key_string(),
            "status": status,
            "remaining": remaining,
            "removed": true,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "get_ai_task_status",
        description = "Read an AI task's checklist progress: items with checked state + who/when, and the remaining count. Resolve by ai_task_id or by session_id (newest non-closed AI task on that session). status: open (tech working) | awaiting_followup (all checked — verify results, then add_ai_task_steps or tell the operator to close) | closed."
    )]
    async fn get_ai_task_status(
        &self,
        Parameters(p): Parameters<GetAiTaskStatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::RecordIdExt;

        let id = match (&p.ai_task_id, &p.session_id) {
            (Some(aid), _) => parse_record_id(aid, database::schema::AI_TASK_TABLE),
            (None, Some(sid)) => {
                let session_ref = parse_record_id(sid, database::schema::DIAGNOSTIC_SESSION_TABLE);
                database::schema::AiTask::get_open_for_session(&session_ref)
                    .await.map_err(to_internal)?
                    .map(|t| t.id)
                    .ok_or_else(|| ErrorData::invalid_params(format!(
                        "get_ai_task_status: no non-closed ai_task on session '{sid}'"), None))?
            }
            (None, None) => return Err(ErrorData::invalid_params(
                "get_ai_task_status: ai_task_id or session_id is required".to_string(), None)),
        };
        let full = database::schema::AiTask::get_full(&id).await.map_err(to_internal)?;
        let (task, items) = full.ok_or_else(|| ErrorData::invalid_params(
            format!("get_ai_task_status: ai_task '{}' not found", id.key_string()), None))?;

        let remaining = items.iter().filter(|i| !i.checked).count();
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "ai_task": {
                "id": task.id.key_string(),
                "title": task.title,
                "status": task.status.as_str(),
                "task_id": task.task_ref.key_string(),
                "session_id": task.session_ref.key_string(),
                "customer_name": task.customer_name,
                "service_number": task.service_number,
                "completed_at": task.completed_at.as_ref().map(|d| d.to_string()),
            },
            "items": items.iter().map(|i| serde_json::json!({
                "id": i.id.key_string(),
                "text": i.text,
                "checked": i.checked,
                "checked_by": i.checked_by.as_ref().map(|u| u.key_string()),
                "checked_at": i.checked_at.as_ref().map(|d| d.to_string()),
            })).collect::<Vec<_>>(),
            "remaining": remaining,
        })).map_err(to_internal)?]))
    }

    #[tool(
        name = "record_stress_test_run",
        description = "Persist a completed stress_test_run row plus optional stress_test_event timeline entries. REQUIRED backfill when scripts_run_remote GPU Probe times out or hangs and stress_test_persistence.verified is false. Also use for third-party bench results. Creates stress_test_run + stress_test_event rows; does not write stress_test_metric samples."
    )]
    async fn record_stress_test_run(
        &self,
        Parameters(p): Parameters<RecordStressTestRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::{
            EventKind, FailureMode, FinishReason, RecordIdExt, RunResult, RunSummary,
            StressTestEvent, StressTestRun, TargetKind, TestTool, STRESS_TEST_RUN_TABLE,
        };

        let computer = parse_record_id(&p.computer_id, database::schema::COMPUTER_TABLE);
        let session_ref = p
            .session_id
            .as_deref()
            .map(|s| parse_record_id(s, database::schema::DIAGNOSTIC_SESSION_TABLE));
        let service_order = p.service_order_id.as_deref().map(|s| {
            parse_record_id(s, database::schema::TICKET_TABLE)
        });
        let target_component = p.target_component_id.as_deref().map(|s| {
            parse_record_id(s, database::schema::HARDWARE_COMPONENT_TABLE)
        });

        let target_kind = match p.target_kind.as_deref().unwrap_or("gpu") {
            "cpu" => TargetKind::Cpu,
            "gpu" => TargetKind::Gpu,
            "memory" => TargetKind::Memory,
            "storage" => TargetKind::Storage,
            "psu" => TargetKind::Psu,
            "motherboard" => TargetKind::Motherboard,
            "system" => TargetKind::System,
            "mixed" => TargetKind::Mixed,
            other => {
                return Err(to_internal(format!(
                    "unknown target_kind '{other}' — use cpu, gpu, memory, storage, psu, motherboard, system, or mixed"
                )));
            }
        };

        let preset = p
            .preset_label
            .clone()
            .unwrap_or_else(|| "qc-mcp:gpu-probe-v1".into());
        let tool = TestTool::StressKitScenario {
            name: Some(preset.clone()),
        };

        let started_at: chrono::DateTime<chrono::Utc> = p
            .started_at
            .parse()
            .map_err(|e| to_internal(format!("invalid started_at: {e}")))?;
        let ended_at: Option<chrono::DateTime<chrono::Utc>> = match &p.ended_at {
            Some(s) => Some(s.parse().map_err(|e| to_internal(format!("invalid ended_at: {e}")))?),
            None => None,
        };

        let result = match p.result.as_deref().unwrap_or("fail") {
            "pass" => RunResult::Pass,
            "fail" => RunResult::Fail,
            "aborted" => RunResult::Aborted,
            "inconclusive" => RunResult::Inconclusive,
            other => {
                return Err(to_internal(format!(
                    "unknown result '{other}' — use pass, fail, aborted, or inconclusive"
                )));
            }
        };

        let failure_mode = match p.failure_kind.as_deref().unwrap_or("reboot") {
            "none" => FailureMode::None,
            "reboot" => FailureMode::Reboot,
            "timeout" => FailureMode::Timeout,
            "tdr" => FailureMode::Tdr { count: 1 },
            "gpu_device_lost" => FailureMode::GpuDeviceLost {
                message: p.notes.clone().unwrap_or_default(),
            },
            "bsod" => FailureMode::Bsod {
                code: None,
                bugcheck_args: None,
            },
            "whea_error" => FailureMode::WheaError { count: 1 },
            "app_error" => FailureMode::AppError {
                exit_code: None,
                message: p.notes.clone().unwrap_or_default(),
            },
            other => {
                return Err(to_internal(format!(
                    "unknown failure_kind '{other}'"
                )));
            }
        };

        let mut tags = p.tags.unwrap_or_default();
        if !tags.iter().any(|t| t == "backfill") {
            tags.push("backfill".into());
        }
        if !tags.iter().any(|t| t == "preset:gpu-probe") {
            tags.push("preset:gpu-probe".into());
        }

        let mut run = StressTestRun::new_for(computer, tool, target_kind);
        run.id = database::schema::random_record_id(STRESS_TEST_RUN_TABLE);
        run.service_order = service_order;
        run.session_ref = session_ref;
        run.target_component = target_component.clone();
        if let Some(ref gpu) = target_component {
            run.touched_components = vec![gpu.clone()];
        }
        run.preset_label = Some(preset);
        run.started_at = started_at.into();
        run.ended_at = ended_at.map(Into::into);
        run.duration_planned_secs = Some(125);
        run.duration_actual_secs = p.duration_actual_secs;
        run.hostname = p.hostname.clone();
        run.notes = p.notes.clone();
        run.tags = tags;
        run.result = result;
        run.finish_reason = Some(FinishReason::Crashed);
        run.set_failure_mode(failure_mode);
        run.summary = RunSummary::default();

        let mut events: Vec<StressTestEvent> = Vec::new();
        for ev in &p.events {
            let kind = match ev.kind.as_str() {
                "stage_started" => EventKind::StageStarted,
                "stage_finished" => EventKind::StageFinished,
                "unexpected_shutdown" => EventKind::UnexpectedShutdown,
                "tdr" => EventKind::Tdr,
                "bsod" => EventKind::Bsod,
                "whea_hit" => EventKind::WheaHit,
                "operator_note" => EventKind::OperatorNote,
                "custom" => EventKind::Custom,
                other => {
                    return Err(to_internal(format!("unknown event kind '{other}'")));
                }
            };
            let mut row = StressTestEvent::new(run.id.clone(), kind, ev.source.as_deref().unwrap_or("operator"));
            if let Some(at) = &ev.at {
                row.at = at
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .map_err(|e| to_internal(format!("invalid event at: {e}")))?
                    .into();
            }
            row.code = ev.code.clone();
            row.detail = ev.detail.clone();
            events.push(row);
        }

        let run_id = StressTestRun::create_completed(&run, &events)
            .await
            .map_err(to_internal)?;

        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({
                "run_id": run_id.key_string(),
                "event_count": events.len(),
                "result": run.result.as_str(),
                "failure_kind": run.failure_kind,
            }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "close_diagnostic_session",
        description = "Close a diagnostic session with a final status and AI-written summary. Status must be 'resolved', 'escalated', or 'open'. Closing as 'escalated' REQUIRES an AI-task handoff on the session (create_ai_task) — enforced, a summary is not a handoff. The close runs a final link-reconcile sweep and returns completeness warnings (unverdicted crash signatures, missing driver snapshot, open AI task); resolve them when they matter before closing. force: true bypasses only the escalation gate."
    )]
    async fn close_diagnostic_session(
        &self,
        Parameters(p): Parameters<CloseDiagnosticSessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use super::tool_warnings::{attach_warnings, ToolWarning};
        use database::schema::RecordIdExt;

        let status = p.status.trim().to_ascii_lowercase();
        if !["resolved", "escalated", "open"].contains(&status.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "close_diagnostic_session: status '{}' is not one of resolved | escalated | open",
                    p.status
                ),
                None,
            ));
        }

        let session_rid =
            parse_record_id(&p.session_id, database::schema::DIAGNOSTIC_SESSION_TABLE);
        let session_key = session_rid.key_string();
        let session = match database::schema::DiagnosticSession::get(&session_key).await {
            Ok(Some(s)) => Some(s),
            Ok(None) => {
                return Err(ErrorData::invalid_params(
                    format!("diagnostic_session '{}' not found", p.session_id),
                    None,
                ))
            }
            // Legacy row shapes (pre-required fields) must stay closable.
            Err(e) => {
                log::warn!(
                    "close_diagnostic_session: session row unreadable, closing without \
                     completeness checks: {e}"
                );
                None
            }
        };
        let Some(session) = session else {
            database::schema::DiagnosticSession::close(
                &session_key,
                &status,
                &p.summary,
                p.tags.as_deref(),
            )
            .await
            .map_err(to_internal)?;
            super::diagnostic_session_registry::clear_session(&session_key);
            return Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
                serde_json::json!({ "session_id": session_key, "closed": true, "status": status }),
                vec![ToolWarning::warn(
                    "completeness_skipped",
                    "Session row could not be read (legacy shape) — gates and completeness \
                     checks were skipped for this close.",
                )],
            ))
            .map_err(to_internal)?]));
        };
        if session.status != "open" {
            return Err(ErrorData::invalid_params(
                format!(
                    "diagnostic_session '{session_key}' is already closed (status '{}')",
                    session.status
                ),
                None,
            ));
        }

        // Escalated work is only handed off through a tracked AI-task checklist.
        // Unknown (lookup error) never trips the gate — only a definite absence.
        let has_ai_task: Option<bool> =
            match database::schema::AiTask::any_for_session(&session.id).await {
                Ok(b) => Some(b),
                Err(e) => {
                    log::warn!("close_diagnostic_session: ai_task lookup failed: {e}");
                    None
                }
            };
        if status == "escalated" && has_ai_task == Some(false) && !p.force.unwrap_or(false) {
            return Err(ErrorData::invalid_params(
                "close_diagnostic_session: escalated close requires an AI-task handoff — call \
                 create_ai_task with the hands-on steps first (or pass force: true for a \
                 deliberate exception)"
                    .to_string(),
                None,
            ));
        }

        // Final link-reconcile sweep before the completeness report.
        let reconciled = database::schema::crash_intel::reconcile_session_links(&session)
            .await
            .unwrap_or_else(|e| {
                log::warn!("close_diagnostic_session: reconcile failed: {e}");
                Default::default()
            });

        let mut warnings: Vec<ToolWarning> = Vec::new();
        if reconciled.total() > 0 {
            warnings.push(ToolWarning::info(
                "orphans_claimed",
                format!("Final reconcile sweep: {}.", reconciled.summary()),
            ));
        }

        if let Ok(Some(open)) = database::schema::AiTask::get_open_for_session(&session.id).await
        {
            warnings.push(ToolWarning::warn(
                "open_ai_task",
                format!(
                    "AI task '{}' is still '{}' — verify the tech's outcome (re-run the failing \
                     check) before considering this engagement finished; reopen with \
                     add_ai_task_steps if more work surfaces.",
                    open.id.key_string(),
                    open.status.as_str()
                ),
            ));
        }

        // Signatures sighted in this session that carry no recorded verdict.
        let sightings = database::schema::crash_intel::sightings_for_session(&session.id)
            .await
            .unwrap_or_default();
        let mut checked_signatures: Vec<String> = Vec::new();
        let mut unverdicted = 0usize;
        for s in &sightings {
            let sig_key = s.signature.key_string();
            if checked_signatures.contains(&sig_key) {
                continue;
            }
            checked_signatures.push(sig_key);
            let sig: Option<database::schema::CrashSignature> = database::db()
                .select(s.signature.clone())
                .await
                .unwrap_or(None);
            if let Some(sig) = sig {
                if sig.latest_verdict.is_none() {
                    unverdicted += 1;
                    warnings.push(
                        ToolWarning::info(
                            "signature_missing_verdict",
                            format!(
                                "Crash signature {} {} sighted in this session has no recorded verdict.",
                                sig.bugcheck_code, sig.module
                            ),
                        )
                        .with_fix(format!(
                            "crash_verdict_record {{ bugcheck_code: \"{}\", module: \"{}\", verdict: \"<diagnosis>\", fix: \"<remediation>\" }}",
                            sig.bugcheck_code, sig.module
                        )),
                    );
                }
            }
        }

        // Driver snapshot captured in the session window? Unknown (lookup
        // error) emits nothing — warnings only state what is definitely true.
        let has_driver_snapshot: Option<bool> = match database::db()
            .query(
                "SELECT VALUE id FROM driver_snapshot WHERE connection_string == $cs \
                 AND taken_at >= ($started - 15m) LIMIT 1",
            )
            .bind(("cs", session.connection_string.clone()))
            .bind(("started", session.started_at.clone()))
            .await
            .and_then(|mut r| r.take::<Vec<database::schema::RecordId>>(0))
        {
            Ok(rows) => Some(!rows.is_empty()),
            Err(e) => {
                log::warn!("close_diagnostic_session: driver_snapshot lookup failed: {e}");
                None
            }
        };
        if has_driver_snapshot == Some(false) {
            warnings.push(
                ToolWarning::info(
                    "no_driver_snapshot",
                    "No driver-inventory snapshot was captured during this session; driver drift \
                     for this engagement is unrecorded.",
                )
                .with_fix(format!(
                    "call_remote_plugin_tool {{ connection_string: \"{}\", plugin_id: \"com.mastertech.driverstore\", tool_name: \"snapshot\" }}",
                    session.connection_string
                )),
            );
        }

        database::schema::DiagnosticSession::close(
            &session_key,
            &status,
            &p.summary,
            p.tags.as_deref(),
        )
        .await
        .map_err(to_internal)?;
        super::diagnostic_session_registry::clear_session(&session_key);

        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "session_id": session_key,
                "closed": true,
                "status": status,
                "reconciled": reconciled,
                "completeness": {
                    "sightings": sightings.len(),
                    "unverdicted_signatures": unverdicted,
                    "has_ai_task": has_ai_task,
                    "has_driver_snapshot": has_driver_snapshot,
                },
            }),
            warnings,
        ))
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
        Ok(CallToolResult::success(vec![ContentBlock::json(
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
                ContentBlock::json(serde_json::json!(f)).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("No diagnostic session found with ID '{}'", p.session_id))
            ])),
        }
    }

    // ── Fleet crash-signature intelligence ──────────────────────────────

    #[tool(
        name = "crash_intel_search",
        description = "Search fleet crash signatures (normalized bugcheck+module classes with sighting counts and recorded verdicts). Omit query for the most recently seen. Use this FIRST when diagnosing a BSOD — a prior verdict may already answer it."
    )]
    async fn crash_intel_search(
        &self,
        Parameters(p): Parameters<CrashIntelSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(20).min(100);
        let signatures = match p.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
            Some(term) => database::schema::CrashSignature::search(term, limit).await,
            None => database::schema::CrashSignature::recent(limit).await,
        }
        .map_err(to_internal)?;

        let mut out = Vec::with_capacity(signatures.len());
        for sig in signatures {
            let verdicts = database::schema::CrashSignature::verdicts(&sig.id, 3)
                .await
                .unwrap_or_default();
            out.push(serde_json::json!({ "signature": sig, "verdicts": verdicts }));
        }
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "count": out.len(), "signatures": out }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "crash_intel_signature",
        description = "Full detail for one crash signature: fleet sightings (which machines, when) and every recorded verdict."
    )]
    async fn crash_intel_signature(
        &self,
        Parameters(p): Parameters<CrashIntelSignatureParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let found = database::schema::CrashSignature::find(&p.bugcheck_code, &p.module)
            .await
            .map_err(to_internal)?;
        match found {
            Some(sig) => {
                let sightings = database::schema::CrashSignature::sightings(&sig.id, 20)
                    .await
                    .unwrap_or_default();
                let verdicts = database::schema::CrashSignature::verdicts(&sig.id, 10)
                    .await
                    .unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::json(
                    serde_json::json!({ "signature": sig, "sightings": sightings, "verdicts": verdicts }),
                )
                .map_err(to_internal)?]))
            }
            None => Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                "No crash signature recorded for {} {}",
                p.bugcheck_code, p.module
            ))])),
        }
    }

    #[tool(
        name = "minidump_analyze",
        description = "Analyze Windows kernel crash dumps (BSOD) — no cdb/WinDbg needed. Open a diagnostic_session for the client FIRST so the recorded sightings link to it; running this before a session exists records them unlinked (a later create_diagnostic_session / intel_links_reap can claim them). LOCAL (path, no connection_string): parse a .dmp on this admin machine — pass link_connection_string so sightings link and dedup stays on. REMOTE (connection_string): run the CLIENT's built-in parser over ALL its dumps (MEMORY.DMP + Minidump + LiveKernelReports), or a single `path` on the client — no plugin deploy required. Handles triage minidumps plus full/BMP/kernel/live dumps: bugcheck code/name, decoded parameters, crash-time RIP, driver-list blame, and fleet matches (prior verdicts, known-bad drivers). Results ALWAYS auto-log to fleet crash intel (crash_signature/crash_sighting). This is the primary BSOD triage tool; use com.mastertech.dump-decode only for a deep cdb `!analyze` pass or Microsoft FAILURE_BUCKET_ID."
    )]
    async fn minidump_analyze(
        &self,
        Parameters(p): Parameters<MinidumpAnalyzeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = p.path.as_deref().map(str::trim).filter(|s| !s.is_empty());

        // REMOTE mode: run the client's built-in dump-triage parser over its
        // own dumps. The result arrives as a RemotePluginToolResult whose
        // receive hook auto-ingests into fleet crash intel.
        if let Some(cs) = p.connection_string.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let request_id = format!("acd-{}", uuid::Uuid::new_v4());
            let cmd = crate::Cmd::AnalyzeCrashDumps {
                request_id: request_id.clone(),
                paths: path.map(|s| vec![s.to_string()]),
            };
            let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
                .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

            let rx = register_pending_request(request_id.clone());
            let _guard = PendingRequestGuard { request_id: request_id.clone() };
            super::remote_egui_control::hub()
                .send_raw_binary(cs, serialized)
                .map_err(to_internal)?;
            log::info!("minidump_analyze remote: req={request_id} cs={cs}");

            let (success, result_json) =
                match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
                    Ok(Ok(pair)) => pair,
                    Ok(Err(_)) => {
                        return Err(to_internal(format!(
                            "remote client {cs} disconnected before returning analysis"
                        )))
                    }
                    Err(_) => {
                        return Err(to_internal(format!(
                            "remote analysis on {cs} timed out after 300s"
                        )))
                    }
                };
            let result: serde_json::Value =
                serde_json::from_str(&result_json).unwrap_or(serde_json::json!(result_json));

            // Fleet enrichment + completeness warnings (parity with LOCAL mode).
            use super::tool_warnings::{attach_warnings, ToolWarning};
            let mut warnings: Vec<ToolWarning> = Vec::new();
            // Mirror the ingest hook's resolution: registry pin, then open
            // session by connection string with the client's computer fallback.
            let open_session = match super::diagnostic_session_registry::get(cs) {
                Some(sid) => database::schema::DiagnosticSession::get(&sid)
                    .await
                    .unwrap_or(None),
                None => {
                    let computer: Option<database::schema::RecordId> = database::db()
                        .query(
                            "SELECT VALUE computer FROM connected_client \
                             WHERE connection_string == $cs LIMIT 1",
                        )
                        .bind(("cs", cs.to_string()))
                        .await
                        .and_then(|mut r| {
                            r.take::<Vec<Option<database::schema::RecordId>>>(0)
                        })
                        .map(|v| v.into_iter().flatten().next())
                        .unwrap_or(None);
                    database::schema::DiagnosticSession::latest_open_for_connection(
                        cs,
                        computer.as_ref(),
                    )
                    .await
                    .unwrap_or(None)
                }
            };
            if open_session.is_none() {
                warnings.push(
                    ToolWarning::warn(
                        "no_open_session",
                        "No open diagnostic session exists for this client — the recorded \
                         sightings carry no session/task link until one claims them.",
                    )
                    .with_fix(format!(
                        "create_diagnostic_session {{ connection_string: \"{cs}\", ... }} — its reconcile sweep claims these sightings"
                    )),
                );
            }

            let crashes = database::schema::crash_intel::parse_kernel_triage_payload(&result);
            let known_bad = database::schema::KnownBadDriver::active()
                .await
                .unwrap_or_default();
            let mut fleet: Vec<serde_json::Value> = Vec::new();
            let mut seen_sigs: Vec<String> = Vec::new();
            let mut prior_verdicts = 0usize;
            let mut known_bad_hits: Vec<serde_json::Value> = Vec::new();
            let mut seen_modules: Vec<String> = Vec::new();
            for c in &crashes {
                let sig_key = format!("{}_{}", c.bugcheck_code, c.module);
                if !seen_sigs.contains(&sig_key) {
                    seen_sigs.push(sig_key);
                    let signature =
                        database::schema::CrashSignature::find(&c.bugcheck_code, &c.module)
                            .await
                            .unwrap_or(None);
                    let verdicts = match &signature {
                        Some(sig) => database::schema::CrashSignature::verdicts(&sig.id, 3)
                            .await
                            .unwrap_or_default(),
                        None => Vec::new(),
                    };
                    prior_verdicts += verdicts.len();
                    if let Some(v) = verdicts.first() {
                        warnings.push(ToolWarning::warn(
                            "prior_verdict",
                            format!(
                                "{} {} is a KNOWN crash class: {} Fix: {}",
                                c.bugcheck_code, c.module, v.verdict, v.fix
                            ),
                        ));
                    }
                    fleet.push(serde_json::json!({
                        "bugcheck_code": c.bugcheck_code,
                        "module": c.module,
                        "signature": signature,
                        "verdicts": verdicts,
                    }));
                }
                for m in &c.loaded_modules {
                    if seen_modules.contains(m) {
                        continue;
                    }
                    seen_modules.push(m.clone());
                    if let Some(k) = known_bad.iter().find(|k| &k.module == m) {
                        known_bad_hits.push(serde_json::json!({ "driver": m, "entry": k }));
                    }
                }
            }
            if !known_bad_hits.is_empty() {
                warnings.push(ToolWarning::warn(
                    "known_bad_hit",
                    format!(
                        "{} blocklisted driver(s) were loaded at crash time — see fleet.known_bad_hits.",
                        known_bad_hits.len()
                    ),
                ));
            }

            return Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
                serde_json::json!({
                    "mode": "remote",
                    "connection_string": cs,
                    "success": success,
                    "ingested": "auto → crash_signature/crash_sighting",
                    "session_ref": open_session.as_ref().map(|s| s.id.key_string()),
                    "fleet": {
                        "signatures": fleet,
                        "known_bad_hits": known_bad_hits,
                        "prior_verdicts": prior_verdicts,
                    },
                    "result": result,
                }),
                warnings,
            ))
            .map_err(to_internal)?]));
        }

        // LOCAL mode: analyze a file on this admin machine.
        let path = path.ok_or_else(|| {
            to_internal("`path` is required for local analysis (or set `connection_string` for a remote client)".to_string())
        })?;
        let path_buf = std::path::PathBuf::from(path);
        let dump_name = path_buf
            .file_name()
            .map(|f| f.to_string_lossy().to_string());
        let triage = tokio::task::spawn_blocking({
            let p = path_buf.clone();
            move || dump_triage::analyze_file(&p)
        })
        .await
        .map_err(|e| to_internal(format!("analysis task: {e}")))?
        .map_err(to_internal)?;

        use super::tool_warnings::{attach_warnings, ToolWarning};
        let mut warnings: Vec<ToolWarning> = Vec::new();

        // Guaranteed logging: ingest the local analysis into fleet crash intel.
        // An explicit link wins; otherwise fall back to the sole active session
        // so the common one-engagement case links (and dedup stays on).
        let explicit_cs = p
            .link_connection_string
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let defaulted = explicit_cs.is_none();
        let link_cs = explicit_cs
            .or_else(super::diagnostic_session_registry::single_active_connection);
        if let (true, Some(cs)) = (defaulted, link_cs.as_deref()) {
            warnings.push(ToolWarning::info(
                "link_defaulted",
                format!("Linked to the only active session's client ({cs}); pass link_connection_string to override."),
            ));
        }
        if link_cs.is_none() {
            warnings.push(
                ToolWarning::warn(
                    "dedup_disabled",
                    "No link_connection_string and no single active session — sightings are \
                     recorded unlinked AND dedup is off, so re-analyzing this dump double-counts.",
                )
                .with_fix(
                    "pass link_connection_string (the client this dump came from) to link and enable dedup",
                ),
            );
        }
        let ingest_payload = serde_json::json!({ "dump_name": dump_name, "triage": triage });
        let ingested = ingest_local_triage(&ingest_payload, link_cs.as_deref()).await;

        // Fleet enrichment: prior signature/verdicts + known-bad drivers.
        let module = triage
            .blamed_module
            .clone()
            .or_else(|| triage.rip_module.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let signature = database::schema::CrashSignature::find(&triage.bugcheck_code, &module)
            .await
            .ok()
            .flatten();
        let verdicts = match &signature {
            Some(sig) => database::schema::CrashSignature::verdicts(&sig.id, 5)
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let known_bad = database::schema::KnownBadDriver::active()
            .await
            .unwrap_or_default();
        let known_bad_hits: Vec<serde_json::Value> = triage
            .drivers
            .iter()
            .filter_map(|d| {
                let stem = database::schema::module_stem(&d.name);
                known_bad
                    .iter()
                    .find(|k| k.module == stem)
                    .map(|k| serde_json::json!({ "driver": d.name, "entry": k }))
            })
            .collect();

        if !verdicts.is_empty() {
            if let Some(v) = verdicts.first() {
                warnings.push(ToolWarning::warn(
                    "prior_verdict",
                    format!(
                        "{} {} is a KNOWN crash class: {} Fix: {}",
                        triage.bugcheck_code, module, v.verdict, v.fix
                    ),
                ));
            }
        }
        if !known_bad_hits.is_empty() {
            warnings.push(ToolWarning::warn(
                "known_bad_hit",
                format!(
                    "{} blocklisted driver(s) were loaded at crash time — see fleet.known_bad_hits.",
                    known_bad_hits.len()
                ),
            ));
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "mode": "local",
                "ingested": ingested,
                "triage": triage,
                "fleet": {
                    "signature_module": module,
                    "signature": signature,
                    "verdicts": verdicts,
                    "known_bad_hits": known_bad_hits,
                },
                "result_schema": serde_json::from_str::<serde_json::Value>(
                    &crate::plugins::dump_triage_schema::kernel_triage_result_schema(),
                )
                .unwrap_or_default(),
            }),
            warnings,
        ))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "minidump_analyze_schema",
        description = "JSON schema of the minidump_analyze triage result and the cross-dump diff object."
    )]
    async fn minidump_analyze_schema(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "triage": serde_json::from_str::<serde_json::Value>(
                &crate::plugins::dump_triage_schema::kernel_triage_result_schema(),
            )
            .unwrap_or_default(),
            "diff": serde_json::from_str::<serde_json::Value>(
                &crate::plugins::dump_triage_schema::triage_diff_schema(),
            )
            .unwrap_or_default(),
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "crash_dumps_fetch",
        description = "Pull ALL of a connected client's Windows crash dumps (MEMORY.DMP + Minidump\\* + LiveKernelReports\\*) as one zip, streamed to THIS admin machine. Returns the saved path + size. Use this to hand raw dumps to WinDbg/cdb for a deep pass; for a bugcheck/blame verdict use minidump_analyze with connection_string instead (faster, and it auto-logs to fleet intel)."
    )]
    async fn crash_dumps_fetch(
        &self,
        Parameters(p): Parameters<CrashDumpsFetchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cs = p.connection_string.trim();
        if cs.is_empty() {
            return Err(to_internal("connection_string is required".to_string()));
        }
        if peek_headless_dump_fetch(cs).is_some() {
            return Err(to_internal(format!(
                "a crash-dump fetch is already in progress for {cs}"
            )));
        }

        let dir = p
            .dest_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(default_download_dir);
        let dest = dir.join(format!("MTech-CrashDumps-{}.zip", sanitize_id(cs)));

        let request_id = format!("cdf-{}", uuid::Uuid::new_v4());
        register_headless_dump_fetch(cs.to_string(), dest.clone(), request_id.clone());
        let rx = register_pending_request(request_id.clone());
        let _guard = PendingRequestGuard { request_id: request_id.clone() };

        let cmd = crate::Cmd::DownloadCrashDumps;
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;
        super::remote_egui_control::hub()
            .send_raw_binary(cs, serialized)
            .map_err(to_internal)?;
        log::info!("crash_dumps_fetch: req={request_id} cs={cs} -> {}", dest.display());

        // Multi-GB dumps stream slowly; allow up to 30 minutes.
        let result = tokio::time::timeout(std::time::Duration::from_secs(1800), rx).await;
        // Clean up the registry if the transfer never completed.
        let _ = take_headless_dump_fetch(cs);
        match result {
            Ok(Ok((true, saved_path))) => {
                let size = std::fs::metadata(&saved_path).map(|m| m.len()).unwrap_or(0);
                Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
                    "connection_string": cs,
                    "saved_path": saved_path,
                    "size_bytes": size,
                    "note": "zip of MEMORY.DMP + Minidump + LiveKernelReports",
                }))
                .map_err(to_internal)?]))
            }
            Ok(Ok((false, msg))) => Err(to_internal(format!("fetch failed: {msg}"))),
            Ok(Err(_)) => Err(to_internal(format!("client {cs} disconnected during fetch"))),
            Err(_) => Err(to_internal(format!(
                "crash-dump fetch from {cs} timed out after 30 minutes"
            ))),
        }
    }

    #[tool(
        name = "crash_verdict_record",
        description = "Record a diagnosis+fix verdict against a crash signature so every future machine hitting the same bugcheck+module surfaces it automatically. Creates the signature if it doesn't exist yet."
    )]
    async fn crash_verdict_record(
        &self,
        Parameters(p): Parameters<CrashVerdictRecordParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use super::tool_warnings::{attach_warnings, ToolWarning};
        use database::schema::RecordIdExt;

        let sig = database::schema::CrashSignature::ensure(&p.bugcheck_code, &p.module)
            .await
            .map_err(to_internal)?;

        // Task linkage: explicit task_id > session (explicit id, registry-pinned,
        // or newest open for the connection) → session.task_ref → the session's
        // open service task.
        let mut task_ref: Option<database::schema::RecordId> = match p.task_id.as_deref() {
            Some(t) => Some(require_record(t, database::schema::TASK_TABLE, "task_id").await?),
            None => None,
        };
        if task_ref.is_none() {
            let session = match (p.session_id.as_deref(), p.connection_string.as_deref()) {
                (Some(sid), _) => {
                    let rid =
                        parse_record_id(sid, database::schema::DIAGNOSTIC_SESSION_TABLE);
                    database::schema::DiagnosticSession::get(&rid.key_string())
                        .await
                        .unwrap_or(None)
                }
                (None, Some(cs)) => match super::diagnostic_session_registry::get(cs) {
                    Some(sid) => database::schema::DiagnosticSession::get(&sid)
                        .await
                        .unwrap_or(None),
                    None => database::schema::DiagnosticSession::latest_open_for_connection(
                        cs, None,
                    )
                    .await
                    .unwrap_or(None),
                },
                (None, None) => None,
            };
            if let Some(session) = session {
                task_ref = session.task_ref.clone();
                if task_ref.is_none() {
                    task_ref = session
                        .resolve_open_service_task()
                        .await
                        .ok()
                        .flatten()
                        .map(|(task, _)| task);
                }
            }
        }

        let mut warnings: Vec<ToolWarning> = Vec::new();
        if task_ref.is_none() {
            warnings.push(
                ToolWarning::info(
                    "verdict_unlinked",
                    "Verdict recorded without a task_ref — it will not surface on the service task.",
                )
                .with_fix(
                    "pass session_id or connection_string (or an explicit task_id) when recording verdicts during an engagement",
                ),
            );
        }

        let verdict_id = database::schema::CrashVerdict::create(
            &sig.id,
            &p.verdict,
            p.fix.as_deref().unwrap_or(""),
            p.confidence.as_deref().unwrap_or("medium"),
            p.author.as_deref().unwrap_or(""),
            p.source.as_deref().unwrap_or("ai"),
            task_ref.clone(),
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "signature_id": sig.id,
                "verdict_id": verdict_id,
                "task_ref": task_ref.as_ref().map(RecordIdExt::key_string),
                "recorded": true,
            }),
            warnings,
        ))
        .map_err(to_internal)?]))
    }

    // ── Driver time machine ──────────────────────────────────────────────

    #[tool(
        name = "known_bad_driver_add",
        description = "Add a driver to the fleet blocklist. Intake triage flags any machine carrying a matching module+version and cross-references crash modules against it."
    )]
    async fn known_bad_driver_add(
        &self,
        Parameters(p): Parameters<KnownBadDriverAddParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use super::tool_warnings::{attach_warnings, ToolWarning};
        use database::schema::RecordIdExt;

        // Link the crash class when a bugcheck code is given. Signature modules
        // carry extensions; retry with `.sys` when the blocklist form is a stem.
        let mut signature_ref: Option<database::schema::RecordId> = None;
        let mut warnings: Vec<ToolWarning> = Vec::new();
        if let Some(code) = p.bugcheck_code.as_deref() {
            let mut found = database::schema::CrashSignature::find(code, &p.module)
                .await
                .unwrap_or(None);
            if found.is_none() && !p.module.contains('.') {
                found =
                    database::schema::CrashSignature::find(code, &format!("{}.sys", p.module))
                        .await
                        .unwrap_or(None);
            }
            match found {
                Some(sig) => signature_ref = Some(sig.id),
                None => warnings.push(ToolWarning::info(
                    "signature_not_found",
                    format!(
                        "No crash signature exists for {code} {} yet; the blocklist entry was added without a signature_ref.",
                        p.module
                    ),
                )),
            }
        }

        let entry = database::schema::KnownBadDriver {
            id: database::schema::random_record_id(database::schema::KNOWN_BAD_DRIVER_TABLE),
            module: p.module.clone(),
            display_name: p.display_name.unwrap_or_default(),
            vendor: p.vendor.unwrap_or_default(),
            bad_versions: p.bad_versions.unwrap_or_default(),
            fixed_version: p.fixed_version,
            symptom: p.symptom.unwrap_or_default(),
            fix: p.fix.unwrap_or_default(),
            severity: p.severity.unwrap_or_else(|| "warn".to_string()),
            signature_ref: signature_ref.clone(),
            active: true,
            created_at: chrono::Utc::now().into(),
            updated_at: chrono::Utc::now().into(),
        };
        let id = database::schema::KnownBadDriver::create(&entry)
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "id": id,
                "added": true,
                "signature_ref": signature_ref.as_ref().map(RecordIdExt::key_string),
            }),
            warnings,
        ))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "known_bad_driver_list",
        description = "List all active fleet driver-blocklist entries."
    )]
    async fn known_bad_driver_list(&self) -> Result<CallToolResult, ErrorData> {
        let entries = database::schema::KnownBadDriver::active()
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "count": entries.len(), "entries": entries }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "driver_snapshots_list",
        description = "List driver-inventory snapshots recorded for a connected client (metadata only — id, label, taken_at, driver_count)."
    )]
    async fn driver_snapshots_list(
        &self,
        Parameters(p): Parameters<DriverSnapshotsListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let snapshots = database::schema::DriverSnapshot::list_meta_for_connection(
            &p.connection_string,
            p.limit.unwrap_or(10).min(50),
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "count": snapshots.len(), "snapshots": snapshots }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "driver_snapshot_diff",
        description = "Diff two driver snapshots of a client (added/removed/version-changed packages) and match the newer inventory against the known-bad-driver blocklist. Defaults to the two newest snapshots."
    )]
    async fn driver_snapshot_diff(
        &self,
        Parameters(p): Parameters<DriverSnapshotDiffParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use database::schema::entity_link::parse_record_id;
        let (older, newer) = match (&p.older_id, &p.newer_id) {
            (Some(o), Some(n)) => {
                let older = database::schema::DriverSnapshot::get(&parse_record_id(
                    o,
                    database::schema::DRIVER_SNAPSHOT_TABLE,
                ))
                .await
                .map_err(to_internal)?;
                let newer = database::schema::DriverSnapshot::get(&parse_record_id(
                    n,
                    database::schema::DRIVER_SNAPSHOT_TABLE,
                ))
                .await
                .map_err(to_internal)?;
                (older, newer)
            }
            _ => {
                let mut snaps =
                    database::schema::DriverSnapshot::list_for_connection(&p.connection_string, 2)
                        .await
                        .map_err(to_internal)?;
                let newer = if snaps.is_empty() { None } else { Some(snaps.remove(0)) };
                let older = if snaps.is_empty() { None } else { Some(snaps.remove(0)) };
                (older, newer)
            }
        };
        let (Some(older), Some(newer)) = (older, newer) else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "Need at least two driver snapshots for this client to diff. Take snapshots with the com.mastertech.driverstore plugin first.",
            )]));
        };
        let diff = database::schema::driver_intel::diff_driver_sets(&older.drivers, &newer.drivers);
        let blocklist = database::schema::KnownBadDriver::active()
            .await
            .unwrap_or_default();
        let hits = database::schema::KnownBadDriver::match_inventory(&blocklist, &newer.drivers);
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({
                "older": { "id": older.id, "label": older.label, "taken_at": older.taken_at, "driver_count": older.driver_count },
                "newer": { "id": newer.id, "label": newer.label, "taken_at": newer.taken_at, "driver_count": newer.driver_count },
                "diff": diff,
                "known_bad_hits": hits,
            }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "driver_snapshot_take",
        description = "Capture a driver-inventory snapshot on a connected client (via com.mastertech.driverstore) and record it to fleet intel, linked to the client's open diagnostic session. label categorizes the capture: intake | pre_service | post_service | manual. Take one at intake and one post-service so driver drift over the engagement is recorded; driver_snapshot_diff then compares them. Deploy com.mastertech.driverstore first if it is not present on the client."
    )]
    async fn driver_snapshot_take(
        &self,
        Parameters(p): Parameters<DriverSnapshotTakeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use super::tool_warnings::{attach_warnings, ToolWarning};
        use database::schema::RecordIdExt;

        let cs = p.connection_string.trim();
        if cs.is_empty() {
            return Err(ErrorData::invalid_params(
                "driver_snapshot_take: connection_string is required".to_string(),
                None,
            ));
        }
        let label = p.label.as_deref().map(str::trim).unwrap_or("manual");
        const LABELS: [&str; 4] = ["intake", "pre_service", "post_service", "manual"];
        if !LABELS.contains(&label) {
            return Err(ErrorData::invalid_params(
                format!(
                    "driver_snapshot_take: label '{label}' is not one of intake | pre_service | post_service | manual"
                ),
                None,
            ));
        }

        // Reject a second concurrent take for this client so two calls can't
        // claim each other's result row. RAII clears the marker on any exit.
        match SNAPSHOT_INFLIGHT.lock() {
            Ok(mut set) => {
                if !set.insert(cs.to_string()) {
                    return Err(to_internal(format!(
                        "driver_snapshot_take: a snapshot is already in progress for {cs}"
                    )));
                }
            }
            Err(_) => return Err(to_internal("driver_snapshot_take: inflight lock poisoned".to_string())),
        }
        let _inflight = SnapshotInflightGuard { cs: cs.to_string() };

        // Remember the newest existing snapshot so we only accept a row this
        // call newly produced, not a pre-existing one.
        let before_id = database::schema::DriverSnapshot::list_for_connection(cs, 1)
            .await
            .ok()
            .and_then(|mut v| v.pop())
            .map(|s| s.id);

        // Stamp the label the ingest hook will apply, then invoke the plugin.
        super::driver_intel_hooks::set_pending_label(cs, label);
        let started: database::schema::Datetime = chrono::Utc::now().into();

        let request_id = format!("dst-{}", uuid::Uuid::new_v4());
        let cmd = crate::Cmd::CallRemotePluginTool {
            request_id: request_id.clone(),
            plugin_id: super::driver_intel_hooks::DRIVERSTORE_PLUGIN_ID.to_string(),
            tool_name: "snapshot".to_string(),
            args_json: "{}".to_string(),
        };
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;
        let rx = register_pending_request(request_id.clone());
        let _guard = PendingRequestGuard { request_id: request_id.clone() };
        super::remote_egui_control::hub()
            .send_raw_binary(cs, serialized)
            .map_err(to_internal)?;

        let (success, _result_json) =
            match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
                Ok(Ok(pair)) => pair,
                Ok(Err(_)) => {
                    super::driver_intel_hooks::clear_pending_label(cs);
                    return Err(to_internal(format!(
                        "driver_snapshot_take: client {cs} disconnected before returning \
                         a snapshot (is com.mastertech.driverstore deployed?)"
                    )));
                }
                Err(_) => {
                    super::driver_intel_hooks::clear_pending_label(cs);
                    return Err(to_internal(format!(
                        "driver_snapshot_take: {cs} did not return a snapshot within 120s"
                    )));
                }
            };
        if !success {
            super::driver_intel_hooks::clear_pending_label(cs);
            return Err(to_internal(format!(
                "driver_snapshot_take: the driverstore snapshot call failed on {cs} — deploy \
                 com.mastertech.driverstore (fetch_plugin + plugin_deploy_remote) and retry"
            )));
        }

        // The receive hook persists the row asynchronously; poll for a NEW row
        // (id differs from before_id) so we never report a pre-existing one.
        let mut snapshot: Option<database::schema::DriverSnapshot> = None;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            let newest = database::schema::DriverSnapshot::list_for_connection(cs, 1)
                .await
                .ok()
                .and_then(|mut v| v.pop());
            if let Some(row) = newest {
                if row.taken_at >= started && before_id.as_ref() != Some(&row.id) {
                    snapshot = Some(row);
                    break;
                }
            }
        }

        let mut warnings: Vec<ToolWarning> = Vec::new();
        let Some(snapshot) = snapshot else {
            return Err(to_internal(format!(
                "driver_snapshot_take: snapshot call succeeded on {cs} but no new driver_snapshot \
                 row appeared — the pnputil output may have been empty or unparseable"
            )));
        };
        if snapshot.session_ref.is_none() {
            warnings.push(
                ToolWarning::warn(
                    "no_open_session",
                    "Snapshot recorded but not linked to a diagnostic session — no open session \
                     for this client.",
                )
                .with_fix(format!(
                    "create_diagnostic_session {{ connection_string: \"{cs}\", ... }} before snapshotting (its reconcile sweep also claims this row)"
                )),
            );
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(attach_warnings(
            serde_json::json!({
                "connection_string": cs,
                "snapshot_id": snapshot.id.key_string(),
                "label": snapshot.label,
                "driver_count": snapshot.driver_count,
                "taken_at": snapshot.taken_at,
                "session_ref": snapshot.session_ref.as_ref().map(RecordIdExt::key_string),
            }),
            warnings,
        ))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "intel_links_reap",
        description = "Sweep every open diagnostic session through the link reconciler: claim orphan crash sightings and driver snapshots into their session, propagate task links, and enrich same-dump sighting siblings. Reports per-session claims plus fleet-wide remaining-orphan counts. Safe to run anytime (idempotent, coalesce-only); use it to backfill links after out-of-order ingest."
    )]
    async fn intel_links_reap(&self) -> Result<CallToolResult, ErrorData> {
        use database::schema::RecordIdExt;

        let sessions = database::schema::DiagnosticSession::list_open(500)
            .await
            .map_err(to_internal)?;
        let mut swept: Vec<serde_json::Value> = Vec::new();
        let mut total = 0usize;
        for session in &sessions {
            match database::schema::crash_intel::reconcile_session_links(session).await {
                Ok(r) if r.total() > 0 => {
                    total += r.total();
                    swept.push(serde_json::json!({
                        "session_id": session.id.key_string(),
                        "connection_string": session.connection_string,
                        "sightings_claimed": r.sightings_claimed,
                        "sightings_task_linked": r.sightings_task_linked,
                        "snapshots_claimed": r.snapshots_claimed,
                        "sightings_enriched": r.sightings_enriched,
                    }));
                }
                Ok(_) => {}
                Err(e) => log::warn!(
                    "intel_links_reap: reconcile failed for {}: {e}",
                    session.id.key_string()
                ),
            }
        }

        let (orphan_sightings, orphan_snapshots) =
            database::schema::crash_intel::count_orphan_links()
                .await
                .unwrap_or((0, 0));

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "open_sessions_swept": sessions.len(),
            "sessions_with_changes": swept.len(),
            "total_rows_linked": total,
            "changes": swept,
            "remaining_orphans": {
                "crash_sightings": orphan_sightings,
                "driver_snapshots": orphan_snapshots,
                "note": "orphans with no open session to claim them — expected for closed/pre-session engagements; link the session manually if one should own them",
            },
        }))
        .map_err(to_internal)?]))
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
        let customers: Vec<serde_json::Value> = database::db()
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
        Ok(CallToolResult::success(vec![ContentBlock::json(
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
        let result: Option<serde_json::Value> = database::db()
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
                ContentBlock::json(v).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("No customer found with ID '{}'", p.customer_id))
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
        let result: Option<serde_json::Value> = database::db()
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
                ContentBlock::json(v).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("No service order found with number '{}'", p.service_number))
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
        let results: Vec<serde_json::Value> = database::db()
            .query(&sql)
            .bind(("q", q))
            .bind(("tech", p.tech.unwrap_or_default()))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(
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
        let rid = database::schema::entity_link::resolve_computer_id(
            &p.computer_id,
            None,
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let result: Option<serde_json::Value> = database::db()
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        match result {
            Some(v) => Ok(CallToolResult::success(vec![
                ContentBlock::json(v).map_err(to_internal)?
            ])),
            None => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("No computer found with ID '{}'", p.computer_id))
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
            Ok(o) => Ok(CallToolResult::success(vec![ContentBlock::json(
                serde_json::json!({ "count": o.len(), "orders": o }),
            )
            .map_err(to_internal)?])),
            Err(e) => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("PrestaShop search error: {e}"))
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
            Ok(resp) => Ok(CallToolResult::success(vec![ContentBlock::json(
                serde_json::json!({ "count": resp.result.len(), "products": resp.result }),
            )
            .map_err(to_internal)?])),
            Err(e) => Ok(CallToolResult::success(vec![
                ContentBlock::text(format!("Odoo search error: {e}"))
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
        let result: Vec<serde_json::Value> = database::db()
            .query(trimmed)
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "results": result }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "benchmark_results_query",
        description = "Query persisted benchmark scores (benchmark_result table), newest first. Benchmarks run via the StressTests scripts ('Benchmark Suite', 'Benchmark: CPU Multi', 'Benchmark: Memory Latency', ...) — use scripts_run_remote to run them on a connected client, then read the scores here. Filter by hostname and/or kind to compare one machine against the population. Each row carries score/unit/peak/low, threads, temps, an errors count (non-zero invalidates the score), and run_ref linking the backing stress_test_run."
    )]
    async fn benchmark_results_query(
        &self,
        Parameters(p): Parameters<BenchmarkResultsQueryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind = match p.kind.as_deref() {
            Some(k) => Some(
                stress_runner::parse_benchmark_kind(k)
                    .ok_or_else(|| to_internal(format!("unknown benchmark kind '{k}'")))?
                    .as_str()
                    .to_string(),
            ),
            None => None,
        };
        let limit = p.limit.unwrap_or(20).clamp(1, 200) as i64;
        // Hostname compare is case-insensitive: benchmark rows store sysinfo
        // casing ("OwnerPC") while stress_test_run stores COMPUTERNAME ("OWNERPC").
        let rows: Vec<serde_json::Value> = database::db()
            .query(
                "SELECT * FROM benchmark_result \
                 WHERE ($h = NONE OR string::lowercase(hostname ?? '') = string::lowercase($h)) \
                   AND ($k = NONE OR kind_label = $k) \
                 ORDER BY captured_at DESC LIMIT $l",
            )
            .bind(("h", p.hostname.clone()))
            .bind(("k", kind))
            .bind(("l", limit))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "count": rows.len(), "results": rows }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "telemetry_snapshot",
        description = "Live hardware telemetry snapshot from THIS host's stress-kit TelemetryAgent (the machine running this MCP server — the admin/tech workstation, NOT a customer's computer): per-core load/frequency, memory, disk and network rates, GPU, top processes, plus Windows WHEA error / GPU TDR counters and thermal state. The sampler starts on first call and refreshes every 1s. Use during or after LOCAL stress runs, or for a quick health read of this workstation. To read temperatures, thermals, or board voltages on a connected customer machine, call telemetry_snapshot_remote instead — this tool cannot see remote hardware."
    )]
    async fn telemetry_snapshot(
        &self,
        Parameters(p): Parameters<TelemetrySnapshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let agent = TELEMETRY_AGENT.clone();
        let mut snap = agent.snapshot();
        if snap.captured_at_unix_ms == 0 {
            let warmup = p.warmup_ms.unwrap_or(1200).min(5000);
            tokio::time::sleep(std::time::Duration::from_millis(warmup)).await;
            snap = agent.snapshot();
        }
        Ok(CallToolResult::success(vec![
            ContentBlock::json(snap).map_err(to_internal)?,
        ]))
    }

    #[tool(
        name = "telemetry_snapshot_remote",
        description = "Live hardware telemetry read from a REMOTE connected client, proxied over the admin session (admin → client's stress-kit TelemetryAgent → result back). Use THIS when diagnosing a customer machine; `telemetry_snapshot` samples the admin workstation only and tells you nothing about the remote hardware. \
Returns: per-core usage/frequency/temperature, memory + page file, per-GPU temperature/power/power-limit/clocks/fan/throttle reasons, every labelled thermal zone in `thermals[]`, the SuperIO board voltage rails in `voltages[]` (each with `label`, `volts`, `calibrated`), and WHEA / GPU-TDR counters. \
ABSENT MEANS NOT MEASURED: a null field, a rail absent from `voltages[]`, and `whea: null` all mean the reading was never taken — never read one as 0, as a cold CPU, as a dead rail, or as 'no errors'. `sensor_availability` grades every sensor; check it before quoting any number and keep investigating whatever it does not report as read. \
CPU TEMPERATURE HAS TWO POSSIBLE SOURCES: `sensor_availability.cpu_package_temp` (repeated as `cpu.package_temp_kind`) is `cpu_die_sensor` = a real CPU sensor answered (`CPU Package`, `CPU (Tctl)`, or `CPU Core N` — a core is one core, not the package); `acpi_zone_only` = NO CPU sensor answered and `cpu.package_temp_c` is a firmware-named CPU ACPI zone (`CPUZ_0`, `TCPU`) that runs far below the die, so it must NOT be quoted as a CPU temperature; `unavailable` = no CPU-side thermal at all — including every machine whose only zones are bare board zones (`TZ00_0`), which are never reported as a CPU temperature. `cpu.package_temp_source` names the exact sensor the value came from, and a die sensor is always preferred over a hotter zone. `thermals[]` carries every labelled zone (including bare board zones and `NVMe Disk N` drive temps) so you can read them individually. \
VOLTAGE RAILS ARE GRADED PER RAIL: `sensor_availability.voltage_rails` is `ok` (every expected rail read), `partial` (some read, some not), or `unavailable` (no rail answered). `sensor_availability.rails` gives `Vcore`, `+5V`, `3VCC (chip)`, `+12V`, `VBAT` each as `read` / `missing` / `unavailable`, and `rails_missing[]` lists the gaps. A `missing` rail was suppressed — unmapped channel, implausible read, or a collapse awaiting confirmation — so it is neither 0 V nor healthy: '+12V missing' NEVER means the +12V rail is fine. \
WHEA: `sensor_availability.whea` is `ok` (counters read), `unavailable` (the WHEA event source could not be opened, so no count was taken — absence of evidence, NOT a clean result; never clear a machine of hardware errors on it), or `not_sampled` (no sampler tick yet). \
WHY READINGS GO MISSING: the CPU die sensor and every entry in `voltages[]` come from the WinRing0 kernel driver, which will NOT load while Memory Integrity (HVCI) or the Vulnerable Driver Blocklist is enabled. `sensor_availability.hvci_enabled` / `.vulnerable_driver_blocklist_enabled` / `.detail` report exactly which one is blocking; SetDriverProtections can turn them off (needs a reboot) if the customer consents. \
VOLTAGES ARE UNCALIBRATED: they are nominal-divider values (`calibrated: false` means no per-board ratio is known), so read them as trend and droop under load, not as absolute volts. Never fail a board on an absolute number from this tool; compare idle vs loaded instead. `3VCC (chip)` is the sensor chip's OWN 3.3V supply, NOT the board's +3.3V PSU rail — there is no +3.3V PSU reading here."
    )]
    async fn telemetry_snapshot_remote(
        &self,
        Parameters(p): Parameters<TelemetrySnapshotRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let warmup_ms = p.warmup_ms.unwrap_or(3000).clamp(500, 15_000);
        let request_id = format!("tel-{}", uuid::Uuid::new_v4());

        let cmd = crate::Cmd::RequestTelemetrySnapshot {
            request_id: request_id.clone(),
            warmup_ms: Some(warmup_ms),
        };
        let serialized = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
            .map_err(|e| to_internal(format!("bincode serialize: {e}")))?;

        let rx = register_pending_request(request_id.clone());
        let _guard = PendingRequestGuard { request_id: request_id.clone() };

        super::remote_egui_control::hub()
            .send_raw_binary(&p.connection_string, serialized)
            .map_err(to_internal)?;

        log::info!(
            "telemetry_snapshot_remote start: req={request_id} cs={} warmup_ms={warmup_ms}",
            p.connection_string
        );

        let deadline = std::time::Duration::from_millis(warmup_ms + 20_000);
        let (success, result_json) = match tokio::time::timeout(deadline, rx).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(_)) => {
                return Err(to_internal(format!(
                    "telemetry_snapshot_remote: response channel closed for req={request_id} \
                     (remote client {} may have disconnected mid-call)",
                    p.connection_string
                )));
            }
            Err(_) => {
                log::error!(
                    "telemetry_snapshot_remote TIMEOUT: req={request_id} cs={} after {:?}",
                    p.connection_string,
                    deadline
                );
                return Err(to_internal(format!(
                    "telemetry_snapshot_remote timed out after {:?} (req={request_id} cs={}). \
                     Run remote_channel_health: a wedged responder loop or a client build \
                     predating this command never answers.",
                    deadline, p.connection_string
                )));
            }
        };

        if !success {
            return Err(to_internal(format!(
                "remote telemetry read failed on {}: {result_json}",
                p.connection_string
            )));
        }

        let mut value: serde_json::Value = serde_json::from_str(&result_json)
            .map_err(|e| to_internal(format!("remote telemetry payload was not JSON: {e}")))?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "connection_string".to_string(),
                serde_json::json!(p.connection_string),
            );
        }
        Ok(CallToolResult::success(vec![
            ContentBlock::json(value).map_err(to_internal)?,
        ]))
    }

    #[tool(
        name = "stress_scenario_run",
        description = "Run a CUSTOM staged stress scenario on this host via stress-runner (persisted like catalog scripts: stress_test_run + stress_test_event + stress_test_metric + hardware_component). Compose any sequence of stress-kit stressors with per-stage durations, e.g. ramp cpu → fp → stream while watching telemetry_snapshot. Caps: 16 stages, 1800s/stage, 7200s total. Blocks until the scenario finishes; returns run_id, verdict, and per-stage final metrics."
    )]
    async fn stress_scenario_run(
        &self,
        Parameters(p): Parameters<StressScenarioRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use stress_runner::{RunPlan, RunSpec, RunStage, RunUpdate, TargetKind, TestTool};

        if p.stages.is_empty() || p.stages.len() > 16 {
            return Err(to_internal("Provide 1-16 stages."));
        }
        let mut stages: Vec<RunStage> = Vec::with_capacity(p.stages.len());
        for s in &p.stages {
            if s.duration_secs == 0 || s.duration_secs > 1800 {
                return Err(to_internal(format!(
                    "Stage '{}' duration_secs must be 1-1800.",
                    s.label.clone().unwrap_or_else(|| s.stressor.clone())
                )));
            }
            let stressor = stress_runner::Stressor::from_str(&s.stressor).ok_or_else(|| {
                to_internal(format!(
                    "Unknown stressor '{}'. Valid: {}",
                    s.stressor,
                    stress_runner::Stressor::labels_csv()
                ))
            })?;
            stages.push(RunStage {
                label: s.label.clone().unwrap_or_else(|| s.stressor.clone()),
                stressor,
                threads: s.threads,
                duration_secs: s.duration_secs,
                memory_cap_mb: s.memory_cap_mb.unwrap_or(256),
                disk_file_mb: s.disk_file_mb.unwrap_or(512),
            });
        }
        let stage_sum: u64 = stages.iter().map(|s| s.duration_secs).sum();
        let budget_secs = p.total_wall_secs.unwrap_or(stage_sum).min(7200).max(1);
        if stage_sum > 7200 {
            return Err(to_internal("Total stage time exceeds the 7200s cap."));
        }

        let target_kind = {
            let kinds: Vec<TargetKind> = stages
                .iter()
                .map(|s| stress_runner::default_target_kind(s.stressor))
                .collect();
            if kinds.windows(2).all(|w| w[0] == w[1]) {
                kinds[0]
            } else {
                TargetKind::Mixed
            }
        };

        let preset = p
            .preset_label
            .clone()
            .unwrap_or_else(|| "mcp:scenario-v1".to_string());
        let spec = RunSpec {
            computer: stress_runner::local_computer_record(),
            tool: TestTool::StressKitScenario {
                name: Some(preset.clone()),
            },
            target_kind,
            target_component: None,
            touched_components: Vec::new(),
            service_order: p
                .service_number
                .as_deref()
                .map(|n| parse_record_id(n.trim(), "service_order")),
            session_ref: p
                .diagnostic_session_id
                .as_deref()
                .map(|s| parse_record_id(s.trim(), "diagnostic_session")),
            task_ref: None,
            tech: Some("mcp".to_string()),
            hostname: None,
            machine_id: None,
            bios_settings: Default::default(),
            driver_versions: Default::default(),
            notes: p.notes.clone(),
            preset_label: Some(preset),
            tags: vec!["origin:mcp".to_string(), "preset:scenario".to_string()],
            plan: RunPlan::Scenario {
                stages,
                total_wall_secs: p.total_wall_secs,
                repeat_until_total: p.repeat_until_total,
            },
            rules: None,
        };

        let telemetry = TELEMETRY_AGENT.clone();
        let handle = tokio::task::spawn_blocking(move || {
            let mut run_id: Option<String> = None;
            let mut logs: Vec<String> = Vec::new();
            let mut stage_metrics: std::collections::HashMap<String, serde_json::Value> =
                std::collections::HashMap::new();
            let verdict = stress_runner::drive_blocking(spec, telemetry, |update| match update {
                RunUpdate::Started { run_id: id } => {
                    use database::schema::RecordIdExt;
                    run_id = Some(id.key_string());
                }
                RunUpdate::StageStarted { index, label, stage_count } => {
                    logs.push(format!("Stage {}/{}: {}", index + 1, stage_count, label));
                }
                RunUpdate::Tick { stage_label, metrics, .. } => {
                    let key = stage_label.unwrap_or_else(|| "run".to_string());
                    if let Ok(v) = serde_json::to_value(&metrics) {
                        stage_metrics.insert(key, v);
                    }
                }
                RunUpdate::StageFinished { .. } => {}
                RunUpdate::StageVerdict { label, pass, violations, .. } => {
                    if pass {
                        logs.push(format!("Stage verdict: {label} PASS"));
                    } else {
                        logs.push(format!(
                            "Stage verdict: {label} FAIL ({})",
                            violations.join("; ")
                        ));
                    }
                }
                RunUpdate::Warning { message } => logs.push(format!("warning: {message}")),
                RunUpdate::Error { message } => logs.push(format!("error: {message}")),
                RunUpdate::Finished(_) => {}
            });
            (run_id, logs, stage_metrics, verdict)
        });

        let grace = std::time::Duration::from_secs(budget_secs + 180);
        let (run_id, logs, stage_metrics, verdict) = match tokio::time::timeout(grace, handle).await
        {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(to_internal(format!("scenario worker panicked: {e}"))),
            Err(_) => {
                return Err(to_internal(format!(
                    "Scenario exceeded budget+grace ({}s) — the run thread may still be finishing; check stress_test_run for the in_progress row and backfill with record_stress_test_run if needed.",
                    grace.as_secs()
                )))
            }
        };

        let verdict_json = verdict.map(|v| {
            use database::schema::RecordIdExt;
            serde_json::json!({
                "run_id": v.run_id.key_string(),
                "result": serde_json::to_value(&v.result).unwrap_or_default(),
                "finish_reason": serde_json::to_value(&v.finish_reason).unwrap_or_default(),
                "failure_mode": serde_json::to_value(&v.failure_mode).unwrap_or_default(),
                "summary": serde_json::to_value(&v.summary).unwrap_or_default(),
                "duration_secs": v.duration_secs,
            })
        });

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "run_id": run_id,
            "verdict": verdict_json,
            "stage_metrics": stage_metrics,
            "logs": logs,
            "persisted": true,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "stress_concurrent_run",
        description = "Run multiple stressors AT THE SAME TIME on this host (OCCT-style combined test: e.g. cpu + memory + gpu concurrently), each as its own lane with its own live metrics. Persisted like scenario runs (stress_test_run + stress_test_event + stress_test_metric + hardware_component, target_kind=system, one metric stream per lane tagged by stage_index). Threads are auto-budgeted across the core pool with one core reserved for the GPU lane. Caps: 1-8 lanes, 1-7200s. Blocks until the run finishes; returns run_id, verdict, and per-lane final metrics."
    )]
    async fn stress_concurrent_run(
        &self,
        Parameters(p): Parameters<StressConcurrentRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use stress_runner::{RunPlan, RunSpec, RunStage, RunUpdate, TargetKind, TestTool};

        if p.lanes.is_empty() || p.lanes.len() > 8 {
            return Err(to_internal("Provide 1-8 concurrent lanes."));
        }
        if p.duration_secs == 0 || p.duration_secs > 7200 {
            return Err(to_internal("duration_secs must be 1-7200."));
        }
        let mut lanes: Vec<RunStage> = Vec::with_capacity(p.lanes.len());
        for s in &p.lanes {
            let stressor = stress_runner::Stressor::from_str(&s.stressor).ok_or_else(|| {
                to_internal(format!(
                    "Unknown stressor '{}'. Valid: {}",
                    s.stressor,
                    stress_runner::Stressor::labels_csv()
                ))
            })?;
            lanes.push(RunStage {
                label: s.label.clone().unwrap_or_else(|| s.stressor.clone()),
                stressor,
                threads: s.threads,
                duration_secs: 0,
                memory_cap_mb: s.memory_cap_mb.unwrap_or(1024),
                disk_file_mb: s.disk_file_mb.unwrap_or(512),
            });
        }

        let preset = p
            .preset_label
            .clone()
            .unwrap_or_else(|| "mcp:concurrent-v1".to_string());
        let spec = RunSpec {
            computer: stress_runner::local_computer_record(),
            tool: TestTool::StressKitScenario {
                name: Some(preset.clone()),
            },
            target_kind: TargetKind::System,
            target_component: None,
            touched_components: Vec::new(),
            service_order: p
                .service_number
                .as_deref()
                .map(|n| parse_record_id(n.trim(), "service_order")),
            session_ref: p
                .diagnostic_session_id
                .as_deref()
                .map(|s| parse_record_id(s.trim(), "diagnostic_session")),
            task_ref: None,
            tech: Some("mcp".to_string()),
            hostname: None,
            machine_id: None,
            bios_settings: Default::default(),
            driver_versions: Default::default(),
            notes: p.notes.clone(),
            preset_label: Some(preset),
            tags: vec!["origin:mcp".to_string(), "preset:concurrent".to_string()],
            plan: RunPlan::Concurrent {
                lanes,
                duration_secs: Some(p.duration_secs),
            },
            rules: None,
        };

        let budget_secs = p.duration_secs.min(7200).max(1);
        let telemetry = TELEMETRY_AGENT.clone();
        let handle = tokio::task::spawn_blocking(move || {
            let mut run_id: Option<String> = None;
            let mut logs: Vec<String> = Vec::new();
            let mut lane_metrics: std::collections::HashMap<String, serde_json::Value> =
                std::collections::HashMap::new();
            let verdict = stress_runner::drive_blocking(spec, telemetry, |update| match update {
                RunUpdate::Started { run_id: id } => {
                    use database::schema::RecordIdExt;
                    run_id = Some(id.key_string());
                }
                RunUpdate::StageStarted { label, .. } => {
                    logs.push(format!("Concurrent run started: {label}"));
                }
                RunUpdate::Tick { stage_label, metrics, .. } => {
                    let key = stage_label.unwrap_or_else(|| "lane".to_string());
                    if let Ok(v) = serde_json::to_value(&metrics) {
                        lane_metrics.insert(key, v);
                    }
                }
                RunUpdate::StageFinished { .. } => {}
                RunUpdate::StageVerdict { label, pass, violations, .. } => {
                    if pass {
                        logs.push(format!("Lane verdict: {label} PASS"));
                    } else {
                        logs.push(format!("Lane verdict: {label} FAIL ({})", violations.join("; ")));
                    }
                }
                RunUpdate::Warning { message } => logs.push(format!("warning: {message}")),
                RunUpdate::Error { message } => logs.push(format!("error: {message}")),
                RunUpdate::Finished(_) => {}
            });
            (run_id, logs, lane_metrics, verdict)
        });

        let grace = std::time::Duration::from_secs(budget_secs + 180);
        let (run_id, logs, lane_metrics, verdict) = match tokio::time::timeout(grace, handle).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(to_internal(format!("concurrent worker panicked: {e}"))),
            Err(_) => {
                return Err(to_internal(format!(
                    "Concurrent run exceeded budget+grace ({}s) — the run thread may still be finishing; check stress_test_run for the in_progress row.",
                    grace.as_secs()
                )))
            }
        };

        let verdict_json = verdict.map(|v| {
            use database::schema::RecordIdExt;
            serde_json::json!({
                "run_id": v.run_id.key_string(),
                "result": serde_json::to_value(&v.result).unwrap_or_default(),
                "finish_reason": serde_json::to_value(&v.finish_reason).unwrap_or_default(),
                "failure_mode": serde_json::to_value(&v.failure_mode).unwrap_or_default(),
                "summary": serde_json::to_value(&v.summary).unwrap_or_default(),
                "duration_secs": v.duration_secs,
            })
        });

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "run_id": run_id,
            "verdict": verdict_json,
            "lane_metrics": lane_metrics,
            "logs": logs,
            "persisted": true,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "stress_runs_reap",
        description = "Finalize zombie stress_test_run rows stuck at result='in_progress' past their window (client hang/reboot prevented finalize). Marks them aborted with ended_at=now and a reap note appended. Use dry_run:true to preview. Returns the affected run ids."
    )]
    async fn stress_runs_reap(
        &self,
        Parameters(p): Parameters<StressRunsReapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const REAP_SELECT: &str = "SELECT id, hostname, preset_label, started_at FROM stress_test_run WHERE result = 'in_progress' AND started_at < <datetime>$cutoff AND ($hostname IS NONE OR hostname = $hostname);";
        const REAP_UPDATE: &str = "UPDATE stress_test_run SET result = 'aborted', ended_at = time::now(), notes = string::concat(notes ?? '', ' [reaped: stale in_progress past planned window]') WHERE result = 'in_progress' AND started_at < <datetime>$cutoff AND ($hostname IS NONE OR hostname = $hostname) RETURN id, hostname, preset_label, started_at;";

        let grace = p.grace_secs.unwrap_or(3600).max(600);
        let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(grace as i64)).to_rfc3339();
        let rows: Vec<serde_json::Value> = database::db()
            .query(if p.dry_run { REAP_SELECT } else { REAP_UPDATE })
            .bind(("cutoff", cutoff))
            .bind(("hostname", p.hostname.clone()))
            .await
            .map_err(to_internal)?
            .take(0)
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "dry_run": p.dry_run,
            "grace_secs": grace,
            "affected": rows.len(),
            "runs": rows,
        }))
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "scripts_list",
        description = "List every script available in the host Mastertech Scripts tab catalog (Tuneup / QC, Informational, Junkware Removal, Stress Tests). Use the returned `category` + `script_name` values verbatim with scripts_run. Works whether or not the host is currently running — it's a static catalog. The Stress Tests category exposes the persisted stress catalog (GPU Stress Test, QC Benchmark, Memory Test, verified CPU/Linpack/PSU tests, singles for every stress-kit stressor) plus the scored 'Benchmark Suite' / 'Benchmark: ...' entries that persist benchmark_result rows readable via benchmark_results_query."
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
            ScriptCategory::StressTests,
        ] {
            let cat_name = match cat_key {
                ScriptCategory::Tuneup => "Tuneup",
                ScriptCategory::Informational => "Informational",
                ScriptCategory::JunkwareRemoval => "JunkwareRemoval",
                ScriptCategory::StressTests => "StressTests",
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

        Ok(CallToolResult::success(vec![ContentBlock::json(
            serde_json::json!({ "categories": out }),
        )
        .map_err(to_internal)?]))
    }

    #[tool(
        name = "scripts_run",
        description = "Run a single named script on the local host (Mastertech4.0 in egui or terminal mode). For stress tests use category 'StressTests' with any catalog entry ('GPU Stress Test', 'QC Benchmark', or any 'Stress: …' single) — every entry persists stress_test_run, stress_test_event, stress_test_metric, and hardware_component via stress-runner. Plugin burn_* tools do NOT persist. Returns stress_test_persistence verification for every StressTests script."
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
            "StressTests" | "stresstests" | "Stress Tests" | "stress" => ScriptCategory::StressTests,
            other => {
                return Err(to_internal(format!(
                    "Unknown category '{other}'. Expected one of: Tuneup, Informational, JunkwareRemoval, StressTests."
                )));
            }
        };

        // Benchmark scripts are exempt: scores are machine-keyed, not order-keyed.
        if category == ScriptCategory::StressTests
            && !stress_runner::is_benchmark_script(&p.script_name)
            && p.service_number.as_deref().map(str::trim).unwrap_or("").is_empty()
        {
            return Err(to_internal(format!(
                "service_number is required for StressTests scripts (so stress_test_run carries service_order / customer / computer linkage). Pass service_number with script '{}'.",
                p.script_name
            )));
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let req = ScriptRunRequest {
            request_id: request_id.clone(),
            category,
            script_name: p.script_name.clone(),
            service_number: p.service_number.clone(),
            customer_email: p.customer_email.clone(),
            diagnostic_session_id: p.diagnostic_session_id.clone(),
        };

        let rx = register_pending_script_run(request_id.clone());

        script_run_request_sender()
            .send(req)
            .map_err(|e| to_internal(format!("Failed to enqueue script request: {e}")))?;

        let timeout = std::time::Duration::from_secs(p.timeout_secs.unwrap_or_else(|| {
            crate::scripts::default_remote_script_timeout_secs(&p.script_name)
        }));
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

        let mut payload = serde_json::to_value(&result).map_err(to_internal)?;
        if super::stress_test_verify::is_persisted_stress_script(&p.script_name) {
            let run_hint = super::stress_test_verify::extract_stress_run_id_from_logs(&result.logs);
            let persistence = super::stress_test_verify::verify_stress_test_persistence(
                None,
                run_hint.as_deref(),
                p.diagnostic_session_id.as_deref(),
            )
            .await;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stress_test_persistence".into(), persistence);
            }
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(payload).map_err(to_internal)?]))
    }

    #[tool(
        name = "scripts_run_remote",
        description = "Run a named script on a REMOTE Mastertech client connected via the admin Web Console. For persisted stress tests use category 'StressTests' with any catalog entry ('GPU Stress Test', 'QC Benchmark', or any 'Stress: …' single) — every entry persists stress_test_run, stress_test_event, stress_test_metric, and hardware_component on the client via stress-runner. Do NOT use call_remote_plugin_tool burn_cpu/burn_memory/burn_disk for persisted stress tests. Returns stress_test_persistence verification after StressTests scripts."
    )]
    async fn scripts_run_remote(
        &self,
        Parameters(p): Parameters<ScriptsRunRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = execute_one_remote_script(p).await?;
        Ok(CallToolResult::success(vec![ContentBlock::json(payload).map_err(to_internal)?]))
    }

    #[tool(
        name = "stress_scenario_run_remote",
        description = "Run a CUSTOM staged stress scenario on a REMOTE Mastertech client connected via the admin Web Console (mirror of stress_scenario_run, but pushed to the client over the same transport as scripts_run_remote). Compose any sequence of stress-kit stressors with per-stage durations; the client persists stress_test_run + stress_test_event + stress_test_metric + hardware_component via stress-runner, linked to service_order. Caps: 16 stages, 1800s/stage, 7200s total. service_number is REQUIRED. Blocks until the scenario finishes; returns success, per-run logs, and stress_test_persistence verification."
    )]
    async fn stress_scenario_run_remote(
        &self,
        Parameters(p): Parameters<StressScenarioRunRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let stages = validate_remote_stress_stages(&p.stages, false)?;
        let stage_sum: u64 = stages.iter().map(|s| s.duration_secs).sum();
        let budget_secs = p.total_wall_secs.unwrap_or(stage_sum).min(7200).max(1);

        let service_number = p.service_number.clone().unwrap_or_default();
        if service_number.trim().is_empty() {
            return Err(to_internal(
                "service_number is required for remote stress scenarios (stress_test_run.service_order linkage).",
            ));
        }
        let diagnostic_session_id = p
            .diagnostic_session_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| super::diagnostic_session_registry::get(&p.connection_string));

        let cmd = crate::Cmd::RunRemoteScenario {
            stages,
            total_wall_secs: p.total_wall_secs,
            repeat_until_total: p.repeat_until_total,
            service_number: Some(service_number.clone()),
            diagnostic_session_id: diagnostic_session_id.clone(),
            preset_label: p.preset_label.clone(),
            notes: p.notes.clone(),
        };

        let payload = execute_remote_stress_plan(
            p.connection_string.clone(),
            cmd,
            crate::REMOTE_SCENARIO_RESULT_NAME,
            &service_number,
            diagnostic_session_id,
            budget_secs,
        )
        .await?;
        Ok(CallToolResult::success(vec![ContentBlock::json(payload).map_err(to_internal)?]))
    }

    #[tool(
        name = "stress_concurrent_run_remote",
        description = "Run multiple stressors AT THE SAME TIME on a REMOTE Mastertech client (OCCT-style combined test: e.g. cpu + memory + gpu concurrently; mirror of stress_concurrent_run pushed to the client). Each lane persists into stress_test_run + stress_test_event + stress_test_metric + hardware_component via stress-runner, linked to service_order, target_kind=system. Caps: 1-8 lanes, 1-7200s. service_number is REQUIRED. Blocks until the run finishes; returns success, per-run logs, and stress_test_persistence verification."
    )]
    async fn stress_concurrent_run_remote(
        &self,
        Parameters(p): Parameters<StressConcurrentRunRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.duration_secs == 0 || p.duration_secs > 7200 {
            return Err(to_internal("duration_secs must be 1-7200."));
        }
        let lanes = validate_remote_stress_stages(&p.lanes, true)?;
        let budget_secs = p.duration_secs.min(7200).max(1);

        let service_number = p.service_number.clone().unwrap_or_default();
        if service_number.trim().is_empty() {
            return Err(to_internal(
                "service_number is required for remote concurrent stress runs (stress_test_run.service_order linkage).",
            ));
        }
        let diagnostic_session_id = p
            .diagnostic_session_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| super::diagnostic_session_registry::get(&p.connection_string));

        let cmd = crate::Cmd::RunRemoteConcurrent {
            lanes,
            duration_secs: p.duration_secs,
            service_number: Some(service_number.clone()),
            diagnostic_session_id: diagnostic_session_id.clone(),
            preset_label: p.preset_label.clone(),
            notes: p.notes.clone(),
        };

        let payload = execute_remote_stress_plan(
            p.connection_string.clone(),
            cmd,
            crate::REMOTE_CONCURRENT_RESULT_NAME,
            &service_number,
            diagnostic_session_id,
            budget_secs,
        )
        .await?;
        Ok(CallToolResult::success(vec![ContentBlock::json(payload).map_err(to_internal)?]))
    }

    #[tool(
        name = "scripts_run_stress_suite_remote",
        description = "Run the full StressTests catalog sequentially on a remote client (GPU Stress Test, QC Benchmark, and every 'Stress: …' single). Each script persists stress_test_run, stress_test_event, stress_test_metric, and hardware_component via stress-runner. Use `skip` to omit scripts that already ran. Returns per-script results plus suite summary counts."
    )]
    async fn scripts_run_stress_suite_remote(
        &self,
        Parameters(p): Parameters<ScriptsRunStressSuiteRemoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.service_number.trim().is_empty() {
            return Err(to_internal(
                "service_number is required for stress suite runs (stress_test_run.service_order linkage).",
            ));
        }

        let skip = p.skip.unwrap_or_default();
        let scripts = stress_suite_script_names(&skip);
        if scripts.is_empty() {
            return Err(to_internal(
                "No stress scripts left to run after applying skip list.",
            ));
        }

        let diagnostic_session_id = p
            .diagnostic_session_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| super::diagnostic_session_registry::get(&p.connection_string));

        let mut runs = Vec::with_capacity(scripts.len());
        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut persistence_verified = 0u32;

        for script_name in &scripts {
            let timeout_secs =
                default_stress_script_timeout_secs(script_name, p.timeout_secs);
            let one = execute_one_remote_script(ScriptsRunRemoteParams {
                connection_string: p.connection_string.clone(),
                category: "StressTests".into(),
                script_name: script_name.clone(),
                service_number: Some(p.service_number.clone()),
                customer_email: None,
                timeout_secs: Some(timeout_secs),
                diagnostic_session_id: diagnostic_session_id.clone(),
            })
            .await?;
            let success = one.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            if success {
                passed += 1;
            } else {
                failed += 1;
            }
            if one
                .get("stress_test_persistence")
                .and_then(|v| v.get("verified"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                persistence_verified += 1;
            }
            runs.push(one);
        }

        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
            "connection_string": p.connection_string,
            "service_number": p.service_number,
            "diagnostic_session_id": diagnostic_session_id,
            "scripts_requested": scripts,
            "summary": {
                "total": runs.len(),
                "passed": passed,
                "failed": failed,
                "persistence_verified": persistence_verified,
            },
            "runs": runs,
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
        let mut response = database::db()
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
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
        let workers_resp = database::db()
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
            if let Err(e) = super::sdk_vendor::ensure_vendored_sdk() {
                log::warn!("ensure_vendored_sdk failed: {e}");
            }
            let (success, stdout, stderr, artifact_size) =
                run_local_cargo_compile(&dir, &p.plugin_id, &self).await?;
            return Ok(CallToolResult::success(vec![ContentBlock::json(
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
                let mut response = database::db()
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

        let extra_files: Vec<database::schema::BuildFile> = if super::sdk_vendor::cargo_toml_uses_sdk(&cargo_toml) {
            // SDK plugins need a multifile-capable worker to receive the
            // vendored sibling crate. Probe for one heartbeating within
            // 90 s; if none, fall back to local cargo (same as the
            // no-worker path) so the AI isn't left polling forever.
            if count_live_multifile_workers().await == 0 {
                if target == "wasm32-wasip1" && local_cargo_available().await {
                    log::info!(
                        "plugin_compile_remote: SDK plugin '{}' but no multifile-capable \
                         build_worker is live — running local cargo compile",
                        p.plugin_id
                    );
                    if let Err(e) = super::sdk_vendor::ensure_vendored_sdk() {
                        log::warn!("ensure_vendored_sdk failed: {e}");
                    }
                    let (success, stdout, stderr, artifact_size) =
                        run_local_cargo_compile(&dir, &p.plugin_id, &self).await?;
                    return Ok(CallToolResult::success(vec![ContentBlock::json(
                        serde_json::json!({
                            "job_id": format!("local-fallback:{}", p.plugin_id),
                            "plugin_id": p.plugin_id,
                            "target": target,
                            "profile": profile,
                            "status": if success { "done" } else { "failed" },
                            "fell_back_to_local": true,
                            "reason": "SDK plugin and no multifile-capable build_worker; ran local cargo",
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
                return Err(to_internal(
                    "This plugin depends on mtech-plugin-sdk, but no multifile-capable \
                     build_worker is live and no local `cargo` + wasm32-wasip1 target is \
                     available. Update your plugin_builder workers (they advertise the \
                     'multifile' capability once rebuilt) or install Rust locally and use \
                     plugin_compile.",
                ));
            }
            super::sdk_vendor::vendored_sdk_files()
                .into_iter()
                .map(|(rel, content)| database::schema::BuildFile {
                    path: format!("_mtech_sdk_vendor/{rel}"),
                    content,
                })
                .collect()
        } else {
            Vec::new()
        };

        let job = database::schema::BuildJob::create(
            &p.plugin_id,
            &cargo_toml,
            &lib_rs,
            &target,
            &profile,
            assigned_worker_id,
            extra_files,
        )
        .await
        .map_err(|e| to_internal(format!("CREATE build_job: {e}")))?;

        let job_id_str = format!(
            "{}:{}",
            job.id.table,
            database::schema::RecordIdExt::key_string(&job.id)
        );
        Ok(CallToolResult::success(vec![ContentBlock::json(serde_json::json!({
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
                        database::db().delete(rid.clone()).await;
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
        Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(to_internal)?]))
    }
}

// ─── Server handler ────────────────────────────────────────────────────────────

/// Shown to MCP clients in `initialize` (`ServerInfo.instructions`). Keep in sync with View menu + `nav_tab_anchor_key` in Mastertech `menu_bar.rs`.
pub const INSTRUCTIONS: &str = r#"Mastertech Plugin System MCP (MasterTech desktop + admin Web Console).

=== Diagnostic Flow (crash/hardware engagements — follow this ORDER) ===
Open the session BEFORE running analyzers so every record links to it (analyzers that run first are recorded unlinked and only get claimed retroactively).
  1. remote_channel_health — confirm the client responds.
  2. create_diagnostic_session — FIRST. Auto-resolves the service task and claims any pre-session orphan records. Everything after inherits its session/task link.
  3. driver_snapshot_take {label:'intake'} — baseline the driver inventory.
  4. minidump_analyze {connection_string} — triage all dumps; sightings auto-link to the open session. The result carries a fleet block (prior verdicts, known-bad hits) and warnings.
  5. Escalate to com.mastertech.dump-decode (cdb) only when triage blame is ambiguous.
  6. log_diagnostic_entry as you go — findings, actions, observations, and anything informational live HERE.
  7. crash_verdict_record / known_bad_driver_add — pass session_id or connection_string so the verdict links to the task.
  8. create_ai_task — ONLY for hands-on/physical steps a human must do (see that tool's rules). Not for anything you can do via MCP, and not for logging.
  9. driver_snapshot_take {label:'post_service'} after the fix, then driver_snapshot_diff.
 10. close_diagnostic_session — a flight check: it gates status + escalation-handoff, runs a final link sweep, and returns completeness warnings. Resolve warnings that matter first.
Tool results may include a `warnings` array ({code, severity, message, fix}) — each names the exact follow-up call. Act on warn-severity items; info items are advisory. intel_links_reap backfills links fleet-wide if ingest ran out of order.

=== Session (HTTP :9004/mcp) ===
After initialize, POST notifications/initialized with the same Mcp-Session-Id before tools/call.

=== AI Workflow ===
Before writing a new WASM plugin, ALWAYS call search_plugins first to check if a suitable plugin already exists in the registry. If one exists, use fetch_plugin to download it and plugin_deploy / plugin_deploy_remote to deploy it.
After compiling a useful plugin, call publish_plugin to store it in the SurrealDB registry for future sessions.

=== Hands-On Handoff (AI Tasks) — MANDATORY for any human/tech step ===
Whenever ANY part of a diagnosis or repair requires a HUMAN — physical access, BIOS/firmware changes, reseating or swapping hardware, plugging/unplugging cables, running bench tools (OCCT/MemTest86/HD Tune), reboots you cannot drive remotely, customer contact/approval, or anything else your remote tools cannot do — you MUST call `create_ai_task` with concrete, ordered steps. This is the ONLY tracked handoff mechanism: it puts a checklist card on the technician's board, pops a blocking attention modal on their desktop, and reports progress back to you.
Do NOT hand off human work any other way. Specifically:
  - Do NOT bury hands-on steps in `log_diagnostic_entry` recommendation entries (informational only — nobody is notified, nothing is tracked).
  - Do NOT put them only in `close_diagnostic_session` summaries or chat text.
  - Do NOT tell the operator "a tech should now do X" without ALSO creating the AI task.
The loop:
  1. `create_ai_task` (steps 1-30, each ONE concrete action; assignee auto-resolves to the service tech).
  2. Poll `get_ai_task_status` to see which steps are checked (who + when per step).
  3. When status becomes 'awaiting_followup' (all steps done), VERIFY the outcome (re-run the failing test, re-check telemetry).
  4. If more work is needed, `add_ai_task_steps` — the card reopens and the tech is re-notified.
  5. You cannot close an AI task; a human operator closes it from the UI after review.
One open AI task per diagnostic session — create_ai_task errors if one exists (append with add_ai_task_steps instead).

=== Plugin Deploy Preconditions (MUST READ) ===
`plugin_deploy` and `plugin_deploy_remote` need a compiled artifact in the local ArtifactStore first. If you see `No artifact for '<plugin_id>'. Run plugin_compile or plugin_emit_clock_wasm first.`, the artifact isn't loaded — you skipped a step. Pick ONE of these BEFORE calling any deploy tool:
  1. **Use a registry plugin** (preferred when one exists): `search_plugins` → `fetch_plugin` with the registry plugin_id. `fetch_plugin` populates the artifact store directly; no compile needed.
  2. **Compile locally**: `plugin_source` (write Rust source) → `plugin_compile`. Requires Rust + the `wasm32-wasip1` target on this host.
  3. **Compile remotely**: `plugin_source` → `plugin_compile_remote` (auto-falls-back to local compile when no `plugin_builder` workers are live) → `plugin_compile_status` until `status == 'done'`.
Only AFTER one of those three has populated the ArtifactStore is `plugin_deploy` / `plugin_deploy_remote` a valid call.

=== Default Plugin Set + AppData Fallback (CRITICAL — read before "no diagnostic tools available" panic) ===
**`list_plugins` only returns the two built-in egui plumbing plugins by default** (`com.mastertech.egui-frame-capture` + `com.mastertech.egui-remote-viewer`, both `tool_count: 0`). It does NOT scan the on-disk plugin store at startup. If you see only those two and conclude "no diagnostic capability is available", you have skipped the fallback.

Three places diagnostic plugins live, in order of preference:
  1. **SurrealDB registry** — published via `publish_plugin`. Discover with `search_plugins`, pull with `fetch_plugin`. As of writing the registry contains hw-diag, repair, diagnostics, status-reporter (see "Known Plugins in Registry" below).
  2. **Local on-disk store** (`%LOCALAPPDATA%/Mastertech/plugins/<sanitized_id>/` on Windows, `$HOME/.local/share/mastertech/plugins/<sanitized_id>/` on Linux) — created by `plugin_source` / `plugin_compile`. Contains `Cargo.toml`, `src/lib.rs`, and the compiled `.wasm` under `target/wasm32-wasip1/release/`. The MCP server does NOT currently auto-scan-and-load this directory; you must `plugin_compile` (rebuild) or `fetch_plugin` (registry copy) before `plugin_deploy` will find an artifact. Future work: a startup scanner that auto-registers compiled artifacts here.
  3. **Write a new one** — `plugin_source` + `plugin_compile` (or `plugin_compile_remote`). Last resort — only after `search_plugins` confirms nothing suitable already exists in #1 and #2.

Practical workflow when starting a diagnostic session that needs hw/system telemetry:
  a. `list_plugins` → see only the two egui ones; don't panic.
  b. `search_plugins "diagnostic"` (or "hw" / "gpu" / "bsod" / whichever symptom area).
  c. `fetch_plugin <registry-id>` → populates the ArtifactStore.
  d. `plugin_deploy <id>` → registers locally; `plugin_deploy_remote <id> <connection_string>` → pushes to the target client.
  e. `call_plugin_tool` / `call_remote_plugin_tool` → execute the tools.
Every NEW session repeats steps (b)–(d); registrations don't persist across server restarts.

=== Plugin / Worker SurrealDB Tables — Canonical Names ===
When using `query_surrealdb` for plugin work, the ONLY valid tables are:
  - `plugin_registry`     — published plugins. Use `search_plugins` instead unless you really need raw SQL.
  - `build_job`           — remote-compile work queue (rows written by `plugin_compile_remote`).
  - `connected_client`    — every running Mastertech / plugin_builder process. Filter `WHERE client_kind = 'build_worker'` for build workers; use `list_build_workers` for the curated view.
  - `diagnostic_session` / `diagnostic_entry` — AI diagnostic work product; manage via the diagnostic tools (`create_diagnostic_session`, `log_diagnostic_entry`, …), NOT raw SQL.
  - `stress_test_run` / `stress_test_metric` / `stress_test_event` / `hardware_component` — stress-runner telemetry; created automatically by every `scripts_run`/`scripts_run_remote` call in the `StressTests` category (GPU Stress Test, QC Benchmark, 23 single-stressor scripts) and by qc-app stress tools. Backfill with `record_stress_test_run` when a hang prevents finalize.
**There is NO `client_plugin` table.** A query against it returns "The table 'client_plugin' does not exist". If you wanted "plugins installed on a connected client", call `list_plugins` against the remote MCP via `call_remote_plugin_tool` or read `plugin_registry` for what's been published.

=== Known Plugins in Registry ===
Always check search_plugins before building new plugins. Current registry (as of last sync):
- **com.mastertech.hw-diag** ("HW Diagnostics") — system_info, bsod_events, critical_events, whea_errors, disk_health, reliability_records, tdr_gpu_events, driver_errors, disk_errors, wer_hardware, list_software, uninstall_armoury_crate, uninstall_ryzen_master, download_ddu, check_ddu_status, find_ryzen_master, remove_ryzen_master_remnants, analyze_minidumps, night_light_status, display_connections, **webroot_license**, **sas_license** (CPS / Webroot + SuperAntiSpyware activation and days-remaining when those tools are published on the remote build). Use for GPU/display/BSOD/crash/Night Light diagnostics and CPS license checks.
- **com.mastertech.repair** ("System Repair") — dism_restore_health, sfc_scannow, uninstall_superantispyware, chkdsk_schedule, run_command (arbitrary PowerShell). Use for Windows system file repair.
- **com.mastertech.diagnostics** ("Diagnostics") — system_summary, top_processes, disk_info, recent_system_errors, recent_app_crashes, stopped_auto_services, network_info, startup_programs, wifi_status, wifi_event_logs, wifi_fix, find_uninstall_targets, uninstall_msi_software, cpu_power_health, crash_deep_dive, verify_fix, detect_hardware, analyze_dump_files, disable_orphaned_drivers, kill_problematic_processes. **Do NOT use burn_cpu / burn_memory / burn_disk / burn_combined / stress_and_monitor for persisted stress tests** — they do not write stress_test_run / stress_test_event / hardware_component rows. Use scripts_run_remote with category 'StressTests' (e.g. 'GPU Stress Test', 'QC Benchmark', 'Stress: CPU') instead.
- **com.mastertech.status-reporter** — status_report (returns UTC clock from remote host, confirms plugin is live). Lightweight connectivity test.
- **com.mastertech.driver-fetch** ("TechDB Driver Fetch") — list_techdb_models, list_model_drivers, fetch_model_path, audio_power_crash_check, install_senary_audio, audio_power_mitigation, schedule_restart, cancel_restart. Automates the OPK Driver / BIOS Server workflow below (mount + list + robocopy + install) so you don't `run_command` it by hand every time. See "OPK Driver / BIOS Server" for what it does and when to reach for it. **IMPORTANT:** its tool args must be passed as a bare string (e.g. `"GX5HRXG"` or `"GX5HRXG:AMD_HawkPoint_GX_IDL_IDG\3.AUDIO"`), NOT a JSON object — `call_plugin_tool`/`call_remote_plugin_tool`'s `args` channel currently double-encodes objects into a string the WASM side can't read as key/value (same root cause as the `run_command` args-drop issue on `com.mastertech.repair`; a source fix has been written for this — see "args double-encoding fix" below — but it needs a rebuild+restart of the admin app to take effect; until then, no-arg and bare-string tool designs are the working pattern). **install_senary_audio / any installer-launching tool can legitimately take several minutes** — a timed-out call does NOT mean it's hung; re-check `audio_power_crash_check`'s `acp_services` field for a newly-created `*.Svc` service before assuming failure, and do NOT re-invoke the same install tool while a prior call may still be in flight (the MCP client layer has been observed to auto-retry on its own shorter timeout even while the admin app's 300s internal deadline is still legitimately running, causing duplicate dispatch of the same installer).

When in doubt, call search_plugins with relevant keywords — the registry is the source of truth.

=== OPK Driver / BIOS Server (PC Laptops internal — QC builds & driver remediation) ===
The OPK server hosts the canonical drivers + BIOS updates for every laptop/desktop PC Laptops sells.
Share: \\opk-riv\winbits\Drivers\7\TechDB  (host `opk-riv` = 192.168.22.21, on the 192.168.22.0/24 LAN that all shop machines and connected clients share). Credentials: user `Images` (see `com.mastertech.driver-fetch` source / a tech for the password — do not paste it into chat transcripts unnecessarily).
Layout: one folder per chassis MODEL NUMBER (e.g. `X5KK4NAG`, `GX5HRXG`). Each model folder holds component subfolders — sometimes flat (1AMDChipset, 2AMDVGA, 3Senary_Audio, 4LAN, 5GL_CardReader, 6WLAN&BT, 7DRTM, 8AMDMEP, 9CameraMEP, Nahimic, RAID, …), sometimes nested one level deeper under a platform-codename folder (e.g. `GX5HRXG\AMD_HawkPoint_GX_IDL_IDG\3.AUDIO\{Realtek,Senary}`) — **do not assume the category layout, always `list_model_drivers` first**. Driver payloads are archives (.zip / .7z — extract before installing), there's usually an `AMD STP Installation procedure.pdf` / driver-list .xlsx (authoritative install order), and on a freshly-built machine the drivers are staged to D:\Driver.
Find the model: it is the chassis/barebone model (TongFang/Mechrevo OEM), NOT Win32_ComputerSystem.Model (often reads "Standard"). Read it from Win32_BaseBoard.Product / SMBIOS or the OPK/service record.

Access / auth: the share needs LAN credentials. Domain-joined or authorized shop machines (and the admin host) read it directly; a workgroup service CLIENT logged in as a local account gets "System error 5 — Access is denied" (TCP 445 open + name resolves, so it is purely an auth gate).

**Preferred path — use the `com.mastertech.driver-fetch` plugin** instead of hand-running `net use`/`robocopy` every time: `search_plugins "driver-fetch"` → `fetch_plugin` → `plugin_deploy_remote` onto the target client → `list_techdb_models` (no args) to confirm the model folder exists → `list_model_drivers` (bare model string, e.g. `"GX5HRXG"`) to find the real category path (layout varies per chassis, see above) → `fetch_model_path` (bare `"Model:RelPath"` string, e.g. `"GX5HRXG:AMD_HawkPoint_GX_IDL_IDG\3.AUDIO"`) to robocopy that folder straight onto the target machine's local disk (`C:\ProgramData\MTechDrivers\<model>\<relpath>\`). This runs the mount+robocopy ON the target machine via its own `host_run_command`, so — same as the manual `net use`/`robocopy` approach — it is NOT pushing bulk bytes through the Mastertech WS/plugin channel; only the small JSON listing/status responses cross that channel. Stage the files and let a tech extract/run the installer rather than auto-installing on customer hardware sight-unseen.
Manual fallback if the plugin isn't deployed: (a) `net use \\opk-riv\winbits <pass> /user:<DOMAIN\user>` then `robocopy \\opk-riv\winbits\Drivers\7\TechDB\<MODEL> D:\Driver /E`, or (b) relay from an authorized admin host over the LAN (HTTP/SMB).

Install order (per the PDF, when present): AMD Chipset (.exe) -> AMD VGA (Setup.exe) -> Senary Audio (Install.bat, as admin) -> LAN -> Cardreader (GIPciSD.inf) -> WLAN (.inf) -> Bluetooth (.inf) -> DRTM (amddrtm.inf) -> MEP (AmdMepEnum.inf) -> CameraMEP (Setup.cmd) -> Nahimic -> ControlCenter. INF-only components install non-interactively with `pnputil /add-driver <inf> /install`.
For BSOD/driver remediation: outdated AMD drivers are a known DRIVER_POWER_STATE_FAILURE (0x9F via pci.sys; the kernel triage bucket names amdhdaudbus) and HYPERVISOR_ERROR (amdppm idle/halt) cause. The AMD VGA package ships amdhdaudbus.sys; the AMD Chipset package covers CPU power management. On chassis that also ship a Senary smart-amp (see `3.AUDIO\Senary` / `3Senary_Audio`), a missing/uninstalled Senary package — `Get-PnpDevice` shows `Senary Audio*` entries stuck in Problem 45 — independently causes buzzing/cutout audio even when the Realtek codec driver itself is current; run `com.mastertech.driver-fetch`'s `audio_power_crash_check` (no args) on the target client to check both signals (0x9F/Kernel-Power-41 count AND Senary PnP status) in one shot before re-pushing just the Realtek driver again.

Fixing the Senary gap end to end (verified working on a GX5HRXG, 2026-06-30): `fetch_model_path` the model's `*.AUDIO` folder → `install_senary_audio` (extracts the Preinstall zip, runs `Install.bat`; takes several minutes, see the timing note above) → `schedule_restart` (the new Senary PnP devices stay Problem 45 until reboot — that is expected, not a failure, since the audio APO/service chain doesn't bind to the live HD Audio function until next boot) → after reboot, `audio_power_crash_check` again to confirm `Senary Audio` / `Speakers (Senary Audio)` / `Senary Audio Effects` / `Senary Audio Service` all read Status OK / Problem 0, and a `Senary*.Svc` Windows service is Running. Stale Problem-45 ghost entries under old instance IDs are normal residue and not a sign of failure. Follow up with `audio_power_mitigation` to disable "allow the computer to turn off this device to save power" on the AMD Audio CoProcessor and AMD HD Audio Controller (reduces 0x9F recurrence risk going forward); not every audio device exposes a power-management tab, so a "no MSPower_DeviceEnable instance" result for one sub-device is expected, not an error.

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

=== Crash / BSOD Dump Analysis (kernel dumps — the ordered playbook) ===
Windows BSOD/kernel dumps (C:\Windows\Minidump\*.dmp, MEMORY.DMP, LiveKernelReports\**) are
PAGEDU64 format — NOT the user-mode MDMP the old app-crash tools read. There is now a built-in
pure-Rust parser (no cdb/WinDbg, no symbols, seconds), available locally AND on every connected
client, and EVERY analysis auto-logs to fleet crash intel. Ordered workflow:

1. **crash_intel_search** FIRST (free, no client needed). A prior verdict for this bugcheck+module
   may already answer it — if so, apply the recorded fix and skip the rest.
2. **minidump_analyze** — the primary triage tool:
   - REMOTE: `minidump_analyze { connection_string }` runs the client's OWN parser over ALL its
     dumps (or one `path`). No plugin deploy, no cdb. Returns bugcheck/params/RIP/driver-blame per
     dump; results auto-ingest into crash_signature/crash_sighting.
   - LOCAL: `minidump_analyze { path }` parses a .dmp already on this admin machine (e.g. one you
     pulled). Also auto-ingests.
   - Reading the output: a recurring third-party `.sys` across dumps = that driver (cross-ref
     known_bad_driver_list; fix via com.mastertech.driver-fetch). Varied faults all in nt/ntoskrnl
     with NO recurring third-party module = memory-subsystem instability (bad RAM / unstable
     XMP / FCLK), NOT a driver — non-ECC bit-flips raise AV bugchecks with no WHEA event, so clean
     WHEA-monitored stress does NOT clear RAM.
3. **crash_verdict_record** once you know root cause + fix — every future machine with the same
   signature then surfaces it automatically in step 1.
4. **Guaranteed logging:** you do NOT need to manually persist — minidump_analyze (local+remote),
   the com.mastertech.dump-decode plugin (cdb `!analyze`), and the com.mastertech.dump-triage
   plugin all auto-ingest on result. Do NOT assume a dump was "just viewed" and skip recording a
   verdict; the sighting is already stored, but the human-useful verdict is your job.

Deep pass (only when step 2 is inconclusive or you need Microsoft's FAILURE_BUCKET_ID / a full
call stack): com.mastertech.dump-decode runs real cdb `!analyze -v` on the client (installs
WinDbg via winget on first use; cold symbol cache 2-4 min). Use decode_whea only on 0x124 dumps.

Pull raw dumps to the bench: **crash_dumps_fetch { connection_string }** zips MEMORY.DMP + Minidump
+ LiveKernelReports and streams them to this admin machine (default %USERPROFILE%\Downloads),
returning the saved path — for handing to WinDbg manually. Prefer minidump_analyze for a verdict;
only fetch when you need the raw files. Multi-GB MEMORY.DMP transfers are streamed with backpressure
(they no longer time the client out) but can take minutes.

=== Stress Test Persistence (MANDATORY for MCP-driven stress) ===
Every stress test you run through MCP MUST land rows in SurrealDB so bench history,
baselines, and AI triage work. This is enforced automatically for the approved paths below.

**Approved tools (persist automatically via stress-runner on the executing host):**
  - `scripts_run_remote` with `category: "StressTests"` on a connected customer client — preferred
    for remote diagnostics. The catalog has 25 entries (GPU Stress Test 4-stage preset,
    QC Benchmark 8-stage preset, and a `Stress: <name>` single for every stress-kit stressor
    including CPU, Matrix, FP/FMA, Cache, Stream, Memory, Disk, GPU Compute, GPU VRAM, etc.).
    Every entry creates:
      * `hardware_component` rows (CPU + GPU upserted from telemetry snapshot)
      * `stress_test_run` (created at run start, finalized on completion or hang recovery)
      * `stress_test_metric` (~1 Hz telemetry samples while the machine stays up)
      * `stress_test_event` (stage_started, stage_finished, errors, etc.)
  - qc-app MCP on QC machines: `run_gpu_probe`, `run_qc_benchmark`, `run_stress_scenario`,
    `run_stressor` — same stress-runner persistence (separate MCP server on QC hardware).

**NOT persisted — do not use for stress tests that need DB history:**
  - `call_remote_plugin_tool` … `burn_cpu`, `burn_memory`, `burn_disk`, `burn_combined`,
    `stress_and_monitor` on com.mastertech.diagnostics — ephemeral plugin stress only.

**After every StressTests run (remote or local):**
  1. Ensure a diagnostic_session is open (`create_diagnostic_session`) before running the script.
     `scripts_run_remote` auto-links the run: pass `diagnostic_session_id` explicitly, or omit it and
     the MCP server resolves the open session for `connection_string`.
  2. Read `stress_test_persistence` in the tool response (`verified`, `run_id`, `session_linked`,
     `event_count`, `target_component`, `hardware_component_present`).
  3. If `verified: false` (timeout, hard hang, old client build), call `record_stress_test_run`
     to backfill the run + timeline events. Pass the same `session_id` so `session_ref` is set.
  4. Log a `log_diagnostic_entry` (category `action` or `finding`) citing the `run_id`.

**Verification query (manual):**
  SELECT id, result, failure_kind, target_component FROM stress_test_run
  WHERE computer = <computer_record> ORDER BY started_at DESC LIMIT 1;
  SELECT count() FROM stress_test_event WHERE run_ref = <run_id> GROUP ALL;

=== RemoteExec (arbitrary elevated shell on a connected client) ===
Last resort, for when no named script and no plugin tool can do the job. Prefer, in order:
  1. scripts_run_remote — a named, reviewed script. Always try this first.
  2. call_remote_plugin_tool — a plugin tool (hw-diag, repair, diagnostics).
  3. remote_exec_* — a raw shell job. Nothing above it fits.
Why it exists: call_remote_plugin_tool is capped by the PluginManager watchdog, so anything
longer than that is unreachable through a plugin. A RemoteExec job is owned by the CLIENT — it
keeps running if the admin disconnects, its whole process tree dies together (Win32 job object),
and it reports a real exit code instead of a proxy's guess.

Flow:
  remote_exec_capabilities → remote_exec_arm → remote_exec_start → remote_exec_wait/_tail → remote_exec_disarm

Consent gate — fails closed, by design:
  Nothing runs until remote_exec_arm succeeds AND the client is painting its consent banner,
  which names you and your stated `reason` to whoever is sitting at the machine. If start keeps
  answering "consent banner not rendering", the client's window is minimised or its UI is wedged
  — that is the interlock working, not a bug to route around. Arm once per diagnostic session,
  pass the diagnostic_session_id, and disarm when finished. Every submitted and denied job is
  written to a journal on the client's own disk.

Paging — the part that silently loses output if you ignore it:
  Reads are byte-capped. When a response carries `more_output_pending`, the output you got is
  NOT the whole job — call remote_exec_tail again with from_seq=next_seq until the flag clears.
  `elided_bytes` is different and worse: the client's 2 MiB ring already dropped that output and
  it is unrecoverable, so for a chatty job have the script tee to a file and fetch the file.

Timeouts: wall-clock defaults to 3600s. Separately, a job producing NO output for 600s is killed
as wedged — a job still printing has not hung, however long it runs.

Risk tier is recorded, not advisory: use 'destructive' for anything that removes data or touches
boot/driver/security state, and give it a real reason (empty reasons are refused for that tier).

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
  (Tuneup, Informational, JunkwareRemoval, StressTests). No host required; the catalog is static.
- scripts_run — runs ONE named script on the LOCAL admin machine. Args:
    category       : "Tuneup" | "Informational" | "JunkwareRemoval" | "StressTests"
    script_name    : exact display name from scripts_list (e.g. "Activate Webroot", "Stress: CPU")
    service_number : required for Activate Webroot, Activate SuperAnti, Activate SEB
    customer_email : required for Activate SEB
    timeout_secs   : defaults to the per-script budget (600 for most; 3600 for updates/scans; 7200 for Tron/Data Transfer).
  Returns: { request_id, success, message, logs[] }.
  ⚠️  Only use for admin-machine operations (e.g. testing, admin-side installs).
      For customer QC, use scripts_run_remote instead.
- scripts_run_remote — runs ONE named script on a REMOTE client over the admin
  WebSocket/TCP session. Same script catalog as scripts_run. Extra required arg:
    connection_string : from remote_egui_list_targets (e.g. "DESKTOP-HKBCJ74:ac4ebfe00")
  All other args (category, script_name, service_number, customer_email, timeout_secs,
  diagnostic_session_id) work identically to scripts_run. Returns { script, success, results[], logs[],
  diagnostic_session_id?, stress_test_persistence? }.
  For any StressTests script, always inspect `stress_test_persistence.verified` — if false after a hang,
  call `record_stress_test_run` to backfill before closing the diagnostic session.
  Use this for ALL QC / Tuneup activation steps on customer machines.
- scripts_run_stress_suite_remote — run the FULL StressTests catalog sequentially on a remote
  client (GPU Stress Test, QC Benchmark, every "Stress: …" single). Args: connection_string,
  service_number, optional diagnostic_session_id, optional skip[] (names to omit),
  optional timeout_secs (per-script override; default QC Benchmark=900s, others=300s).
  Returns { summary: { total, passed, failed, persistence_verified }, runs[] }.
  Use this instead of external scripts or repeated scripts_run_remote loops.
- stress_scenario_run_remote — run a CUSTOM staged stress scenario on a REMOTE client (the
  remote analog of stress_scenario_run, not limited to catalog scripts). Args: connection_string,
  stages[] (each: stressor, duration_secs, optional threads/memory_cap_mb/disk_file_mb/label),
  optional total_wall_secs + repeat_until_total, required service_number, optional
  diagnostic_session_id/preset_label/notes. Caps: 16 stages, 1800s/stage, 7200s total. The client
  persists stress_test_run/metric/event via stress-runner. Returns { success, logs[], computer_id?,
  stress_test_persistence } — inspect stress_test_persistence.verified after a hang.
- stress_concurrent_run_remote — run multiple stressors AT THE SAME TIME on a REMOTE client
  (OCCT-style cpu+memory+gpu combined load; remote analog of stress_concurrent_run). Args:
  connection_string, lanes[] (per-lane duration_secs IGNORED), shared duration_secs (1-7200),
  required service_number, optional diagnostic_session_id/preset_label/notes. Caps 1-8 lanes.
  Returns the same shape as stress_scenario_run_remote.
- telemetry_snapshot_remote — live temperatures, per-core load/clock, memory, GPU power, and the
  SuperIO board voltage rails for a REMOTE client. Args: connection_string, optional warmup_ms.
  This is the ONLY way to see a customer machine's thermals from MCP — `telemetry_snapshot`
  reads the admin workstation. An absent reading means NOT MEASURED, never 0 and never healthy;
  `sensor_availability` grades each sensor: `cpu_package_temp` = cpu_die_sensor / acpi_zone_only
  (a firmware CPU zone, not a die temp) / unavailable, `rails` = per-rail read / missing / unavailable
  with `rails_missing[]`, `whea` = ok / unavailable (event source unreadable, NOT 'no errors') /
  not_sampled. CPU die temp and the rails need the WinRing0 driver, which cannot load while
  Memory Integrity or the Vulnerable Driver Blocklist is on. Voltages are
  UNCALIBRATED nominal-divider values — judge droop under load, not absolute volts.
  Pair with a stress run: read it before, during, and after to catch thermal throttling
  and rail droop that a pass/fail verdict alone hides.

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
- list_registry_plugins — list the WHOLE registry catalog (all metadata + tool lists, minus the heavy source_code). Cheap; use to see every published plugin at a glance instead of relying on search_plugins keyword hits.
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
- create_ai_task — hand off hands-on work to the tech as a checklist (see Hands-On Handoff above).
- add_ai_task_steps — append steps to an AI task; reopens it and re-notifies the tech.
- get_ai_task_status — poll checklist progress (checked/by/when, remaining count).

=== Crash Intel & Dump Analysis (see the BSOD playbook above) ===
- crash_intel_search — search fleet crash signatures + verdicts. Call FIRST when diagnosing a BSOD.
- crash_intel_signature — exact lookup by bugcheck_code + module (sightings + verdicts).
- minidump_analyze — parse kernel dumps LOCAL (path) or REMOTE (connection_string → all of a client's
  dumps, no plugin/cdb). Auto-logs to fleet intel. The primary BSOD triage tool.
- crash_dumps_fetch — pull a client's raw dumps (MEMORY.DMP + Minidump + LiveKernelReports) as one zip
  to this admin machine. Only for a manual WinDbg deep pass; use minidump_analyze for a verdict.
- crash_verdict_record — record diagnosis+fix against a signature (surfaces on every future match).
- known_bad_driver_add / known_bad_driver_list — fleet driver blocklist, cross-referenced on every crash.
- driver_snapshots_list / driver_snapshot_diff — installed-driver time machine + known-bad matching.

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
- Scripts — Saved scripts and tooling.
- File Browser 📂 — File browser / workspace files (includes My Tools via combobox).
- Minidump Analysis — Crash dump analysis (Windows; when enabled).
- Ai — AI playground (models, prompts).
- Resource Monitor — Live hardware telemetry, machine info, and processes.
- My Tasks — Personal task queue layout.
- Store Tasks — Store-wide open tasks layout.
- Completed Tasks — Completed task layout.
- Bug Tracker — GitHub issue tracking.
- Admin Console — Remote clients: shell, files, viewers (connect to agents here).
- Web Console — In-app web/shell console.
- Inventory — Stock / inventory tables.
- Task Audit — History and audit of task changes.
- Create Prestashop Order — PrestaShop order entry.
- Plugins — Plugin list; MCP :9004; enable frame capture / remote viewer on the client being viewed.
- Downloads — App releases / downloads.
- Threads — Operator chat threads.
- Logs — Egui log viewer (filters/categories).

Other dock tabs (context menus / layouts, not all in View list): Part Order, QC, Query Editor (admins). Use dock UI or existing flows to open them.

=== Remote egui pitfalls ===
Do not skip notifications/initialized. Prefer perform_steps with sleep_ms between opening View menu and clicking nav.tab.*. If click_anchor fails with unknown key, call list_widget_anchors again (stale frame)."#;

#[tool_handler(router = self.tool_router)]
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

    /// Intercept every tool invocation so the admin's MCP Tool Log captures
    /// what the AI is asking for and what came back — same shape as Claude
    /// Desktop's expandable tool-call rows. Without this override the
    /// `#[tool_handler]` macro would just forward to the router and the log
    /// would never see anything except the special `call_remote_plugin_tool`
    /// proxy path.
    ///
    /// Routing key: if the tool's arguments carry a `connection_string`
    /// field (every `remote_egui_*` tool, `call_remote_plugin_tool`, the
    /// stress runners, etc.) the entry lands under that client so the
    /// per-client viewer shows it. Otherwise it goes under
    /// [`mcp_tool_log::GLOBAL_KEY`] and the viewer's `get_for_client`
    /// merges those in alongside the per-client list.
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let tool_name = request.name.to_string();
        let args_value: serde_json::Value = request
            .arguments
            .as_ref()
            .map(|a| serde_json::Value::Object(a.clone()))
            .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
        let connection_string = args_value
            .get("connection_string")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| crate::mcp_tool_log::GLOBAL_KEY.to_string());

        let request_id = format!("mcp-{}", uuid::Uuid::new_v4());
        let args_pretty = serde_json::to_string_pretty(&args_value).unwrap_or_default();
        crate::mcp_tool_log::start_call(
            &connection_string,
            request_id.clone(),
            "mcp".to_string(),
            tool_name.clone(),
            args_pretty,
        );

        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tcc).await;

        match &result {
            Ok(response) => {
                // Only the completed payload serializes; the rest log their debug form.
                let body = match response {
                    rmcp::model::CallToolResponse::Complete(call_result) => {
                        serde_json::to_string_pretty(call_result)
                            .unwrap_or_else(|_| "{}".to_string())
                    }
                    other => format!("{other:?}"),
                };
                crate::mcp_tool_log::finish_call(&request_id, true, body);
            }
            Err(err) => {
                let body = serde_json::to_string_pretty(&serde_json::json!({
                    "error": err.message,
                    "code": err.code.0,
                }))
                .unwrap_or_else(|_| format!("{{\"error\":{:?}}}", err.message));
                crate::mcp_tool_log::finish_call(&request_id, false, body);
            }
        }

        result
    }
}

fn to_internal<E: std::fmt::Display>(e: E) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
}

// Emits an image content block when a plugin result carries base64 image bytes; otherwise JSON.
fn plugin_value_to_content(v: serde_json::Value) -> Result<ContentBlock, ErrorData> {
    let img = {
        let b64 = v
            .get("image_base64")
            .or_else(|| v.get("base64"))
            .or_else(|| v.get("data"));
        let mime = v
            .get("mime")
            .or_else(|| v.get("mime_type"))
            .or_else(|| v.get("mimeType"));
        match (b64, mime) {
            (Some(serde_json::Value::String(b)), Some(serde_json::Value::String(m)))
                if m.starts_with("image/") =>
            {
                let raw = b
                    .strip_prefix("data:")
                    .and_then(|s| s.split_once(',').map(|(_, d)| d))
                    .unwrap_or(b);
                Some((raw.to_string(), m.clone()))
            }
            _ => None,
        }
    };
    match img {
        Some((data, mime)) => Ok(ContentBlock::image(data, mime)),
        None => ContentBlock::json(v).map_err(to_internal),
    }
}

/// Last `n` lines of `s`, joined with `\n`. Used to keep
/// `plugin_compile_status` payloads compact when a build prints
/// kilobytes of warnings before succeeding.
/// Lenient `Option<u64>` deserializer for AI-supplied MCP params.
///
/// The AI repeatedly hands us numeric strings (e.g. `"300"`) where the
/// schema expects a real number, and strict serde refuses with
/// `failed to deserialize parameters: invalid type: string "300", expected u64`
/// (see the 12:10:04/11 block in the log).  Accept either form so a
/// stringified value doesn't short-circuit an otherwise-valid tool call.
/// Companion to the existing `deserialize_optional_string_vec` which
/// handles the same shape for `Option<Vec<String>>`.
fn deserialize_lenient_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match v {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n.as_u64().map(Some).ok_or_else(|| {
            D::Error::custom(format!(
                "expected non-negative integer, got fractional or negative: {n}"
            ))
        }),
        Some(serde_json::Value::String(s)) => s.trim().parse::<u64>().map(Some).map_err(|e| {
            D::Error::custom(format!(
                "expected u64 or numeric string, got string {s:?} that doesn't parse: {e}"
            ))
        }),
        Some(other) => Err(D::Error::custom(format!(
            "expected u64 or numeric string, got: {other}"
        ))),
    }
}

/// Lenient `args` deserializer: some tool-calling clients double-encode an
/// object into a JSON string because the `args` schema declares no `"type"`,
/// which silently breaks every plugin's `args.get(key)`. Re-parses it back.
fn deserialize_lenient_args<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(v.map(unwrap_json_string_layers))
}

/// Re-parses a `Value::String` that looks like JSON, recursing through nested layers.
fn unwrap_json_string_layers(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            let looks_like_json = (trimmed.starts_with('{') && trimmed.ends_with('}'))
                || (trimmed.starts_with('[') && trimmed.ends_with(']'));
            if looks_like_json {
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(inner) => unwrap_json_string_layers(inner),
                    Err(_) => serde_json::Value::String(s),
                }
            } else {
                serde_json::Value::String(s)
            }
        }
        other => other,
    }
}

fn tail_n_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= n {
        s.to_string()
    } else {
        lines[lines.len() - n..].join("\n")
    }
}

/// Count `build_worker` rows heartbeating within 90 s that advertise
/// the `multifile` capability. Returns 0 on query failure so callers
/// fall back to local compile rather than enqueue an unclaimable job.
async fn count_live_multifile_workers() -> i64 {
    let resp = database::db()
        .query(
            "SELECT count() FROM connected_client \
             WHERE client_kind = 'build_worker' \
               AND last_update > time::now() - 90s \
               AND 'multifile' IN (capabilities ?? []) \
             GROUP ALL",
        )
        .await;
    match resp {
        Ok(mut r) => r
            .take::<Option<serde_json::Value>>(0)
            .ok()
            .flatten()
            .and_then(|v| v.get("count").and_then(|c| c.as_i64()))
            .unwrap_or(0),
        Err(e) => {
            log::warn!("plugin_compile_remote: multifile-worker probe failed: {e:?} — assuming 0");
            0
        }
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

/// Parse a Surreal record id from an MCP-supplied string. Accepts `table:key`,
/// bare `key`, and SurrealQL backtick-quoted keys (`table:`key-with:colons``).
/// Only strips the first colon when the prefix is a table name so
/// `computer:DESKTOP-HQAF13L:b57a7e8f9` keeps the full hostname:hash key.
fn parse_record_id(s: &str, table: &'static str) -> database::schema::RecordId {
    database::schema::entity_link::parse_record_id(s, table)
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

    // Idle keep-alive before a Streamable-HTTP MCP session is evicted (rmcp default 5min).
    // 8h so multi-hour Claude Code / Cursor diagnostic sessions aren't dropped mid-call.
    let mut session_manager = LocalSessionManager::default();
    session_manager.session_config.keep_alive = Some(Duration::from_secs(28_800));

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

#[cfg(test)]
mod remote_exec_tests {
    use super::*;
    use base64::Engine;

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    /// A `JobSnapshot`-shaped value: `last_seq` is the ring's newest chunk,
    /// which is independent of how many chunks this read served.
    fn snap(state: &str, last_seq: u64, chunks: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({
            "job_id": "job-1",
            "state": state,
            "spec_summary": "PowerShell: whoami",
            "risk": "Read",
            "reason": "test",
            "tech": "t",
            "started_at_ms": 0,
            "last_seq": last_seq,
            "pid": 42,
            "exit": null,
            "chunks": chunks,
            "chunks_truncated": false,
        })
    }

    fn chunk(seq: u64, stream: &str, text: &str, elided_before: u64) -> serde_json::Value {
        serde_json::json!({
            "job_id": "job-1",
            "seq": seq,
            "stream": stream,
            "data": b64(text),
            "elided_before": elided_before,
        })
    }

    #[test]
    fn next_seq_follows_served_chunks_not_the_rings_newest() {
        // The ring holds 0..=99 but this read only served 0..=2. Seeding
        // next_seq from last_seq would skip chunks 3..=99 forever.
        let out = render_job_snapshot(
            snap(
                "Running",
                99,
                vec![
                    chunk(0, "Stdout", "a", 0),
                    chunk(1, "Stdout", "b", 0),
                    chunk(2, "Stdout", "c", 0),
                ],
            ),
            0,
        );
        assert_eq!(out["next_seq"], 3);
        assert_eq!(out["more_output_pending"], true);
        assert_eq!(out["stdout"], "abc");
    }

    #[test]
    fn caught_up_read_reports_nothing_pending() {
        let out = render_job_snapshot(
            snap("Succeeded", 1, vec![chunk(0, "Stdout", "x", 0), chunk(1, "Stdout", "y", 0)]),
            0,
        );
        assert_eq!(out["next_seq"], 2);
        assert!(
            out.get("more_output_pending").is_none(),
            "a fully served read must not claim more output is pending"
        );
    }

    #[test]
    fn job_with_no_output_is_not_reported_as_pending() {
        // last_seq is 0 both for "one chunk at seq 0" and for "no output"; an
        // empty read must not be read as the former.
        let out = render_job_snapshot(snap("Succeeded", 0, vec![]), 0);
        assert_eq!(out["stdout"], "");
        assert!(out.get("more_output_pending").is_none());
        assert_eq!(out["next_seq"], 0, "an empty read must not advance the cursor");
    }

    #[test]
    fn resuming_keeps_the_cursor_when_nothing_new_arrived() {
        let out = render_job_snapshot(snap("Running", 6, vec![]), 7);
        assert_eq!(out["next_seq"], 7, "an empty read must preserve from_seq");
    }

    #[test]
    fn streams_are_separated_and_eviction_is_surfaced() {
        let out = render_job_snapshot(
            snap(
                "Failed",
                2,
                vec![
                    chunk(0, "Stdout", "out", 0),
                    chunk(1, "Stderr", "err", 512),
                    chunk(2, "Meta", "note", 0),
                ],
            ),
            0,
        );
        assert_eq!(out["stdout"], "out");
        assert_eq!(out["stderr"], "err");
        assert_eq!(out["runtime_notes"], "note");
        assert_eq!(out["elided_bytes"], 512);
        assert!(out.get("elided_note").is_some());
    }

    #[test]
    fn non_utf8_output_survives_as_replacement_chars() {
        let raw = base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe, b'h', b'i']);
        let mut c = chunk(0, "Stdout", "", 0);
        c["data"] = serde_json::json!(raw);
        let out = render_job_snapshot(snap("Succeeded", 0, vec![c]), 0);
        let s = out["stdout"].as_str().unwrap();
        assert!(s.ends_with("hi"), "valid trailing bytes must survive: {s:?}");
    }

    #[test]
    fn take_job_names_retention_when_the_job_is_gone() {
        let err = take_job(serde_json::json!({ "jobs": [] }), "job-9").unwrap_err();
        assert!(err.message.contains("job-9"));
        assert!(err.message.contains("retaining"));
    }

    #[test]
    fn shell_risk_and_signal_parsing_rejects_unknown_values() {
        assert!(parse_shell(None).is_ok());
        assert!(parse_shell(Some("PowerShell")).is_ok());
        assert!(parse_shell(Some("bash")).is_err());
        assert!(parse_risk(None).is_ok());
        assert!(parse_risk(Some("DESTRUCTIVE")).is_ok());
        assert!(parse_risk(Some("yolo")).is_err());
        assert!(parse_signal("Kill").is_ok());
        assert!(parse_signal("sigterm").is_err());
    }
}
