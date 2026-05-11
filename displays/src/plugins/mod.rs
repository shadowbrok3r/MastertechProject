pub mod host;
pub mod demo_plugin;
pub mod remote_script_notify;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub mod mcp_bridge;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub mod remote_egui_control;
pub mod remote;
#[cfg(feature = "wasm-plugins")]
pub mod wasm;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "wasm-plugins"))]
pub mod plugin_wasm_factory;

pub use host::{PluginHost, PluginEvent, NotificationKind, ClientSnapshot, SystemInfoSnapshot, UserSnapshot};
pub use demo_plugin::{HelloMastertechPlugin, HELLO_PLUGIN_ID};
pub use remote::{
    apply_wire_textures_delta_for_viewer, EguiFrameCapture, EguiFrameMessage, EguiInputEvent,
    EguiRemoteViewer, WireClippedMesh, WireTextureId, WireTexturesDelta, WidgetAnchor, decompress,
    push_widget_anchor, wire_to_clipped_primitive, wire_to_clipped_primitive_for_viewer,
};
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub use mcp_bridge::{run_plugin_mcp_server, run_plugin_mcp_server_http};

use crossbeam::channel::{Receiver, Sender};
use eframe::egui;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── Global WASM plugin loading channel ─────────────────────────────────────────
//
// Bridges terminal mode (WebSocket handler) → egui (PluginManager).
// The WS handler sends (plugin_id, wasm_bytes); the PluginManager drains each frame.

static WASM_LOAD_CHANNEL: Lazy<(
    Sender<(String, Vec<u8>)>,
    Receiver<(String, Vec<u8>)>,
)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Get a cloneable sender for submitting WASM plugin bytes to be loaded by the PluginManager.
/// Call from anywhere (e.g. terminal mode WebSocket handler).
pub fn wasm_load_sender() -> Sender<(String, Vec<u8>)> {
    WASM_LOAD_CHANNEL.0.clone()
}

// ─── Global remote plugin tool call channel ─────────────────────────────────
//
// Bridges WebSocket handler → PluginManager for remote MCP tool calls.
// (request_id, plugin_id, tool_name, args_json)
type RemoteToolRequest = (String, String, String, String);
// (request_id, success, result_json)
type RemoteToolResponse = (String, bool, String);

static REMOTE_TOOL_CALL_CHANNEL: Lazy<(
    Sender<RemoteToolRequest>,
    Receiver<RemoteToolRequest>,
)> = Lazy::new(|| crossbeam::channel::unbounded());

static REMOTE_TOOL_RESULT_CHANNEL: Lazy<(
    Sender<RemoteToolResponse>,
    Receiver<RemoteToolResponse>,
)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Send a remote plugin tool call request for the PluginManager to handle.
pub fn remote_tool_call_sender() -> Sender<RemoteToolRequest> {
    REMOTE_TOOL_CALL_CHANNEL.0.clone()
}

/// Receive results from PluginManager after handling remote tool calls.
/// The WebSocket handler drains this to send `RemotePluginToolResult` back.
pub fn remote_tool_result_receiver() -> Receiver<RemoteToolResponse> {
    REMOTE_TOOL_RESULT_CHANNEL.1.clone()
}

// ─── Global egui input channel ────────────────────────────────────────────────
//
// Egui input events arrive from the admin via *any* transport (WebSocket relay
// or direct TCP).  To avoid threading `egui_input_tx` through every code path,
// we expose a process-wide channel here.  `EguiFrameCapture::input_hook` drains
// it every frame; the TCP listener (and WS path) send into it when they see
// `EGUI_INPUT_TAG` (0xEE) on an inbound binary frame.

static EGUI_INPUT_CHANNEL: Lazy<(
    Sender<EguiInputEvent>,
    Receiver<EguiInputEvent>,
)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Returns a sender that routes `EguiInputEvent`s into the `EguiFrameCapture`
/// plugin's `input_hook`.  Call this from any transport handler that receives
/// a frame tagged with `EGUI_INPUT_TAG`.
pub fn egui_input_sender() -> Sender<EguiInputEvent> {
    EGUI_INPUT_CHANNEL.0.clone()
}

/// Drain all pending `EguiInputEvent`s. Called from `EguiFrameCapture::input_hook`.
pub fn drain_egui_inputs() -> impl Iterator<Item = EguiInputEvent> {
    std::iter::from_fn(|| EGUI_INPUT_CHANNEL.1.try_recv().ok())
}

// ─── Global WASM bytes cache + background engine ──────────────────────────────
//
// Stores the raw bytes of every loaded WASM plugin so that remote tool calls
// can dispatch on a background thread (fresh Store per call) without blocking
// the egui main thread during `on_begin_pass`.
// WASM_BG_ENGINE is a dedicated wasmtime Engine for background dispatch threads;
// it shares compiled module caches with the main engine via ARC internals.
#[cfg(feature = "wasm-plugins")]
static WASM_BYTES_CACHE: Lazy<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(feature = "wasm-plugins")]
static WASM_BG_ENGINE: Lazy<wasmtime::Engine> = Lazy::new(wasmtime::Engine::default);

// ─── Plugin UI state store ──────────────────────────────────────────────────
//
// Host-side storage for plugin UI panels. WASM plugins (including background
// dispatch threads) write structured entries here via `host_ui_log`.  The main
// plugin instance's `ui()` reads from here each frame and renders egui widgets.

/// A single UI entry emitted by a plugin.  Stored as raw JSON so the renderer
/// can parse it lazily and the schema can evolve without recompiling the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginUiEntry {
    pub json: String,
}

/// Accumulated UI state for one plugin.
#[derive(Debug, Clone, Default)]
pub struct PluginUiState {
    pub entries: Vec<PluginUiEntry>,
    pub visible: bool,
}

pub static PLUGIN_UI_STATES: Lazy<std::sync::Mutex<std::collections::HashMap<String, PluginUiState>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Append a UI log entry for a plugin (callable from any thread).
pub fn plugin_ui_append(plugin_id: &str, json: String) {
    if let Ok(mut map) = PLUGIN_UI_STATES.lock() {
        let state = map.entry(plugin_id.to_string()).or_default();
        state.entries.push(PluginUiEntry { json });
        state.visible = true;
    }
}

/// Clear all UI entries for a plugin.
pub fn plugin_ui_clear(plugin_id: &str) {
    if let Ok(mut map) = PLUGIN_UI_STATES.lock() {
        if let Some(state) = map.get_mut(plugin_id) {
            state.entries.clear();
        }
    }
}

// ─── Global frame capture enable/disable channel ────────────────────────────
//
// Bridges WebSocket handler → egui (PluginManager).
// The WS handler sends a bool (true=enable, false=disable); the PluginManager
// drains each frame and calls set_enabled on EguiFrameCapture.

static FRAME_CAPTURE_CHANNEL: Lazy<(Sender<bool>, Receiver<bool>)> =
    Lazy::new(|| crossbeam::channel::bounded(4));

/// Send a frame-capture enable/disable request from any thread.
pub fn frame_capture_sender() -> Sender<bool> {
    FRAME_CAPTURE_CHANNEL.0.clone()
}

/// Descriptor for an MCP tool that a plugin exposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolDescriptor {
    pub name: String,
    pub description: String,
    /// Accept both "parameters_schema" (new style) and "input_schema" (legacy style).
    #[serde(alias = "input_schema", default)]
    pub parameters_schema: serde_json::Value,
}

/// Metadata about a registered plugin, returned by `PluginManager::list_plugins`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: &'static str,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub tool_count: usize,
}

// ─── Core Plugin Trait ─────────────────────────────────────────────────────────

/// The trait that all Mastertech plugins must implement.
///
/// Compiled-in plugins implement this directly.
/// WASM plugins get an adapter (`WasmPlugin`) that bridges across the sandbox boundary.
pub trait MastertechPlugin: Send + Sync + 'static {
    /// Unique stable identifier (e.g. `"com.mastertech.diagnostics-overlay"`).
    fn id(&self) -> &'static str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str { "" }
    fn enabled(&self) -> bool { true }
    fn set_enabled(&mut self, _enabled: bool) {}

    /// Called once when the plugin is registered with the manager.
    fn on_load(&mut self, _host: &PluginHost) {}

    /// Called when the plugin is removed or the application shuts down.
    fn on_unload(&mut self) {}

    /// Called during `on_begin_pass` -- pure data/logic, no UI.
    fn logic(&mut self, _host: &PluginHost) {}

    /// Called during `on_end_pass` -- may render egui windows, panels, overlays.
    /// Use `ui.ctx()` to access the Context for showing Windows.
    fn ui(&mut self, _ui: &mut egui::Ui, _host: &PluginHost) {}

    /// Called just before egui processes input. No UI allowed.
    fn input_hook(&mut self, _input: &mut egui::RawInput) {}

    /// Called just before egui output is sent to the backend. No UI allowed.
    fn output_hook(&mut self, _output: &mut egui::FullOutput) {}

    /// Return MCP tool descriptors this plugin exposes (empty vec if none).
    fn mcp_tools(&self) -> Vec<PluginToolDescriptor> { vec![] }

    /// Handle an MCP tool call routed to this plugin.
    fn handle_mcp_call(
        &mut self,
        _tool: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("No tools registered".into())
    }
}

// ─── Event Dispatcher ──────────────────────────────────────────────────────────

/// Trait for dispatching plugin events to the host application.
///
/// Injected into `PluginManager` to decouple the plugin system from the app's
/// WebSocket client, notification system, and script runner. The host app
/// implements this trait and passes in an `Arc`.
pub trait EventDispatcher: Send + Sync + 'static {
    /// Run a script on a connected remote client.
    fn run_script(&self, client_id: &str, filename: &str, content: &str);

    /// Send a raw WebSocket command payload to a connected client.
    fn send_ws_command(&self, client_id: &str, payload: &[u8]);

    /// Display a notification in the host app's UI.
    fn show_notification(&self, title: &str, body: &str, kind: &NotificationKind);
}

// ─── Plugin Manager ────────────────────────────────────────────────────────────

/// Manages all `MastertechPlugin` instances and bridges them into egui's `Plugin` system.
///
/// Wrap in `Arc<Mutex<>>` and register via `ctx.add_plugin(PluginManagerHandle(arc))`.
pub struct PluginManager {
    pub(crate) plugins: Vec<Box<dyn MastertechPlugin>>,
    pub(crate) host: PluginHost,
    event_rx: Receiver<PluginEvent>,
    pub(crate) setup_done: bool,
    dispatcher: Option<Arc<dyn EventDispatcher>>,
    #[cfg(feature = "wasm-plugins")]
    wasm_runtime: wasm::WasmRuntime,
}

impl PluginManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = crossbeam::channel::unbounded();
        let (broadcast_tx, broadcast_rx) = crossbeam::channel::unbounded();
        let host = PluginHost {
            event_tx,
            broadcast_rx,
            broadcast_tx,
            ctx: None,
            connected_clients: Vec::new(),
            current_user: None,
            system_info: None,
        };
        Self {
            plugins: Vec::new(),
            host,
            event_rx,
            setup_done: false,
            dispatcher: None,
            #[cfg(feature = "wasm-plugins")]
            wasm_runtime: wasm::WasmRuntime::default(),
        }
    }

    pub fn register(&mut self, mut plugin: Box<dyn MastertechPlugin>) {
        log::info!("Plugin registered: {} v{}", plugin.name(), plugin.version());
        if self.setup_done {
            plugin.on_load(&self.host);
        }
        self.plugins.push(plugin);
    }

    pub fn unregister(&mut self, id: &str) -> bool {
        if let Some(pos) = self.plugins.iter().position(|p| p.id() == id) {
            self.plugins[pos].on_unload();
            self.plugins.remove(pos);
            log::info!("Plugin unregistered: {id}");
            true
        } else {
            false
        }
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| PluginInfo {
                id: p.id(),
                name: p.name().to_string(),
                version: p.version().to_string(),
                description: p.description().to_string(),
                enabled: p.enabled(),
                tool_count: p.mcp_tools().len(),
            })
            .collect()
    }

    pub fn get_plugin_mut(&mut self, id: &str) -> Option<&mut Box<dyn MastertechPlugin>> {
        self.plugins.iter_mut().find(|p| p.id() == id)
    }

    pub fn set_plugin_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Some(p) = self.get_plugin_mut(id) {
            p.set_enabled(enabled);
            true
        } else {
            false
        }
    }

    pub fn all_mcp_tools(&self) -> Vec<(String, PluginToolDescriptor)> {
        self.plugins
            .iter()
            .filter(|p| p.enabled())
            .flat_map(|p| {
                let id = p.id().to_string();
                p.mcp_tools().into_iter().map(move |t| (id.clone(), t))
            })
            .collect()
    }

    pub fn dispatch_mcp_call(
        &mut self,
        plugin_id: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let plugin = self
            .plugins
            .iter_mut()
            .find(|p| p.id() == plugin_id && p.enabled())
            .ok_or_else(|| format!("Plugin not found or disabled: {plugin_id}"))?;
        plugin.handle_mcp_call(tool_name, args)
    }

    pub fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match &event {
                PluginEvent::RequestRepaint => {
                    if let Some(ctx) = &self.host.ctx {
                        ctx.request_repaint();
                    }
                }
                PluginEvent::ShowNotification { title, body, kind } => {
                    if let Some(d) = &self.dispatcher {
                        d.show_notification(title, body, kind);
                    } else {
                        log::info!("Plugin notification [{kind:?}]: {title} - {body}");
                    }
                }
                PluginEvent::RunScript { client_id, filename, content } => {
                    if let Some(d) = &self.dispatcher {
                        d.run_script(client_id, filename, content);
                    } else {
                        log::info!("Plugin requests script run: {filename} on {client_id}");
                    }
                }
                PluginEvent::SendWsCommand { client_id, payload } => {
                    if let Some(d) = &self.dispatcher {
                        d.send_ws_command(client_id, payload);
                    } else {
                        log::info!("Plugin sends WS command to {client_id}");
                    }
                }
                PluginEvent::Custom { plugin_id, event_type, .. } => {
                    log::debug!("Custom event from {plugin_id}: {event_type}");
                }
                _ => {}
            }
        }

        // Drain WASM plugin loading channel (bytes arrive from WebSocket handler / terminal mode)
        #[cfg(feature = "wasm-plugins")]
        while let Ok((plugin_id, wasm_bytes)) = WASM_LOAD_CHANNEL.1.try_recv() {
            let size = wasm_bytes.len();
            log::info!("Loading remote WASM plugin '{plugin_id}' ({size} bytes)...");
            self.unregister(&plugin_id);

            // Cache bytes before loading so remote tool calls can dispatch on background threads
            if let Ok(mut cache) = WASM_BYTES_CACHE.lock() {
                cache.insert(plugin_id.clone(), wasm_bytes.clone());
            }

            match self.load_wasm(wasm_bytes) {
                Ok(()) => {
                    log::info!("Remote WASM plugin '{plugin_id}' loaded successfully");
                    if let Some(d) = &self.dispatcher {
                        d.show_notification(
                            "Plugin Loaded",
                            &format!("{plugin_id} ({size} bytes)"),
                            &NotificationKind::Success,
                        );
                    }
                }
                Err(e) => {
                    log::error!("Failed to load remote WASM plugin '{plugin_id}': {e}");
                    if let Some(d) = &self.dispatcher {
                        d.show_notification(
                            "Plugin Load Failed",
                            &format!("{plugin_id}: {e}"),
                            &NotificationKind::Error,
                        );
                    }
                }
            }
        }

        // Drain remote plugin tool call requests.
        // Each call is dispatched on a background OS thread using a fresh WASM Store
        // so the egui main thread (`on_begin_pass`) is never blocked by PowerShell execution.
        while let Ok((request_id, plugin_id, tool_name, args_json)) = REMOTE_TOOL_CALL_CHANNEL.1.try_recv() {
            #[cfg(feature = "wasm-plugins")]
            {
                let bytes_opt = WASM_BYTES_CACHE.lock().ok()
                    .and_then(|c| c.get(&plugin_id).cloned());

                if let Some(wasm_bytes) = bytes_opt {
                    let result_tx = REMOTE_TOOL_RESULT_CHANNEL.0.clone();
                    std::thread::spawn(move || {
                        log::info!("[remote-dispatch] Running {plugin_id}::{tool_name} on background thread");
                        let args: serde_json::Value =
                            serde_json::from_str(&args_json).unwrap_or(serde_json::json!({}));
                        // Inner channel: worker → watchdog (bounded(1) so we don't block)
                        let (done_tx, done_rx) = crossbeam::channel::bounded::<(bool, String)>(1);
                        let wasm_bytes2 = wasm_bytes.clone();
                        let tool_name2 = tool_name.clone();
                        let plugin_id2 = plugin_id.clone();
                        std::thread::spawn(move || {
                            let (event_tx, _event_rx) = crossbeam::channel::bounded::<crate::plugins::PluginEvent>(16);
                            let engine = &*WASM_BG_ENGINE;
                            let outcome = match crate::plugins::wasm::WasmPlugin::from_bytes(wasm_bytes2, engine, event_tx) {
                                Ok(mut plugin) => match plugin.handle_mcp_call(&tool_name2, args) {
                                    Ok(val) => (true, serde_json::to_string(&val).unwrap_or_default()),
                                    Err(e) => (false, e),
                                },
                                Err(e) => (false, format!("WASM reload failed: {e}")),
                            };
                            let _ = done_tx.try_send(outcome);
                        });
                        // Wait up to 60 s; if the worker process hangs (e.g. WRSA.exe), report timeout.
                        const PLUGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
                        let (success, result_json) = match done_rx.recv_timeout(PLUGIN_TIMEOUT) {
                            Ok(r) => r,
                            Err(_) => (false, format!("Plugin {plugin_id}::{tool_name} timed out after 60s (host process may have hung)")),
                        };
                        log::info!("[remote-dispatch] {plugin_id}::{tool_name} done (success={success})");
                        let _ = result_tx.try_send((request_id, success, result_json));
                    });
                } else {
                    // Plugin not in bytes cache — try dispatch through live plugin (fallback)
                    let args: serde_json::Value = serde_json::from_str(&args_json).unwrap_or(serde_json::json!({}));
                    let result = self.dispatch_mcp_call(&plugin_id, &tool_name, args);
                    let (success, result_json) = match result {
                        Ok(val) => (true, serde_json::to_string(&val).unwrap_or_default()),
                        Err(e) => (false, e),
                    };
                    let _ = REMOTE_TOOL_RESULT_CHANNEL.0.try_send((request_id, success, result_json));
                }
            }
            #[cfg(not(feature = "wasm-plugins"))]
            {
                // No WASM support: attempt live dispatch (for native Rust plugins)
                let args: serde_json::Value = serde_json::from_str(&args_json).unwrap_or(serde_json::json!({}));
                let result = self.dispatch_mcp_call(&plugin_id, &tool_name, args);
                let (success, result_json) = match result {
                    Ok(val) => (true, serde_json::to_string(&val).unwrap_or_default()),
                    Err(e) => (false, e),
                };
                let _ = REMOTE_TOOL_RESULT_CHANNEL.0.try_send((request_id, success, result_json));
            }
        }

        // Drain frame-capture enable/disable channel
        while let Ok(enabled) = FRAME_CAPTURE_CHANNEL.1.try_recv() {
            const CAPTURE_ID: &str = "com.mastertech.egui-frame-capture";
            if let Some(p) = self.plugins.iter_mut().find(|p| p.id() == CAPTURE_ID) {
                log::info!("SetFrameCapture: setting EguiFrameCapture enabled={enabled}");
                p.set_enabled(enabled);
            }
        }
    }

    /// Set the event dispatcher for routing plugin events to the host app.
    pub fn set_dispatcher(&mut self, dispatcher: Arc<dyn EventDispatcher>) {
        self.dispatcher = Some(dispatcher);
    }

    pub fn host(&self) -> &PluginHost {
        &self.host
    }

    pub fn update_clients(&mut self, clients: Vec<ClientSnapshot>) {
        self.host.connected_clients = clients;
    }

    pub fn update_user(&mut self, user: Option<UserSnapshot>) {
        self.host.current_user = user;
    }

    pub fn update_system_info(&mut self, info: Option<SystemInfoSnapshot>) {
        self.host.system_info = info;
    }

    #[cfg(feature = "wasm-plugins")]
    pub fn load_wasm(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let event_tx = self.host.event_tx.clone();
        let plugin = wasm::WasmPlugin::from_bytes(bytes, self.wasm_runtime.engine(), event_tx)?;
        self.register(Box::new(plugin));
        Ok(())
    }

    // ── Broadcast (host → plugins) ──────────────────────────────────────

    pub fn broadcast_client_connected(&mut self, client: ClientSnapshot) {
        self.host.broadcast(PluginEvent::ClientConnected(client));
    }

    pub fn broadcast_client_disconnected(&mut self, connection_string: String) {
        self.host.broadcast(PluginEvent::ClientDisconnected(connection_string));
    }

    pub fn broadcast_system_info_updated(&mut self, info: SystemInfoSnapshot) {
        self.host.broadcast(PluginEvent::SystemInfoUpdated(info));
    }

    pub fn broadcast_script_completed(
        &mut self,
        client_id: String,
        filename: String,
        success: bool,
        output: String,
    ) {
        self.host.broadcast(PluginEvent::ScriptCompleted {
            client_id,
            filename,
            success,
            output,
        });
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Plugin Client Command ─────────────────────────────────────────────────────

/// Commands routed from plugins to connected clients via the `EventDispatcher`.
///
/// The app drains these from the `DefaultEventDispatcher`'s receiver and routes
/// them to the appropriate WebSocket `ConnectionManager`.
#[derive(Debug, Clone)]
pub enum PluginClientCommand {
    RunScript {
        client_id: String,
        filename: String,
        content: String,
    },
    SendPayload {
        client_id: String,
        payload: Vec<u8>,
    },
}

// ─── Default EventDispatcher ───────────────────────────────────────────────────

/// A ready-to-use `EventDispatcher` that routes:
/// - Notifications → the global `ToastMessage` channel
/// - Client commands → a dedicated `PluginClientCommand` channel
///
/// Create with `DefaultEventDispatcher::new()`, which returns the dispatcher
/// and a `Receiver<PluginClientCommand>` the host app should drain each frame.
pub struct DefaultEventDispatcher {
    toast_tx: crossbeam::channel::Sender<crate::ToastMessage>,
    cmd_tx: crossbeam::channel::Sender<PluginClientCommand>,
}

impl DefaultEventDispatcher {
    pub fn new() -> (Arc<Self>, crossbeam::channel::Receiver<PluginClientCommand>) {
        let (cmd_tx, cmd_rx) = crossbeam::channel::unbounded();
        let dispatcher = Arc::new(Self {
            toast_tx: crate::get_toast_sender(),
            cmd_tx,
        });
        (dispatcher, cmd_rx)
    }
}

impl EventDispatcher for DefaultEventDispatcher {
    fn run_script(&self, client_id: &str, filename: &str, content: &str) {
        let _ = self.cmd_tx.try_send(PluginClientCommand::RunScript {
            client_id: client_id.to_string(),
            filename: filename.to_string(),
            content: content.to_string(),
        });
    }

    fn send_ws_command(&self, client_id: &str, payload: &[u8]) {
        let _ = self.cmd_tx.try_send(PluginClientCommand::SendPayload {
            client_id: client_id.to_string(),
            payload: payload.to_vec(),
        });
    }

    fn show_notification(&self, title: &str, body: &str, kind: &NotificationKind) {
        let msg = format!("{title}: {body}");
        let toast = match kind {
            NotificationKind::Success => crate::ToastMessage::Success(msg),
            NotificationKind::Error => crate::ToastMessage::Error(msg),
            NotificationKind::Warning => crate::ToastMessage::Warning(msg),
            NotificationKind::Info => crate::ToastMessage::Info(msg),
        };
        let _ = self.toast_tx.try_send(toast);
    }
}

// ─── egui::Plugin bridge ───────────────────────────────────────────────────────

/// Thin wrapper around `Arc<RwLock<PluginManager>>` that implements `egui::Plugin`.
///
/// Register with `ctx.add_plugin(PluginManagerHandle(arc.clone()))`.
/// The `Arc` remains accessible for MCP bridge and external callers. The UI takes a **write** lock
/// during each hook; MCP uses `try_read` / `try_write` so read-only tools need not block on the frame.
pub struct PluginManagerHandle(pub std::sync::Arc<std::sync::RwLock<PluginManager>>);

impl PluginManagerHandle {
    /// Render every enabled plugin's UI for the current frame.
    ///
    /// MUST be called from the user-frame callback (e.g. `eframe::App::ui`),
    /// NOT from inside an `egui::Plugin` hook. Calling it from
    /// `on_end_pass`/`on_begin_pass` will deadlock because egui holds an
    /// internal `Mutex<PluginHandle>` across those hooks; any interactive
    /// widget our plugins create then re-enters that same mutex.
    pub fn render_plugin_uis(&self, ui: &mut egui::Ui) {
        if let Ok(mut guard) = self.0.write() {
            let mgr = &mut *guard;
            for plugin in &mut mgr.plugins {
                if plugin.enabled() {
                    plugin.ui(ui, &mgr.host);
                }
            }
        }
    }
}

impl egui::Plugin for PluginManagerHandle {
    fn debug_name(&self) -> &'static str {
        "MastertechPluginManager"
    }

    fn setup(&mut self, ctx: &egui::Context) {
        if let Ok(mut guard) = self.0.write() {
            let mgr = &mut *guard;
            mgr.host.ctx = Some(ctx.clone());
            mgr.setup_done = true;
            for plugin in &mut mgr.plugins {
                plugin.on_load(&mgr.host);
            }
            log::info!(
                "PluginManager setup complete: {} plugin(s) loaded",
                mgr.plugins.len()
            );
        }
    }

    fn on_begin_pass(&mut self, _ui: &mut egui::Ui) {
        if let Ok(mut guard) = self.0.write() {
            let mgr = &mut *guard;
            mgr.process_events();
            for plugin in &mut mgr.plugins {
                if plugin.enabled() {
                    plugin.logic(&mgr.host);
                }
            }
        }
    }

    fn on_end_pass(&mut self, _ui: &mut egui::Ui) {
        // Intentionally NOT calling `plugin.ui(...)` here. egui holds an
        // internal `Mutex<PluginHandle>` for the duration of `on_end_pass`
        // (so its plugin list cannot be mutated mid-iteration). Any widget
        // we create from inside this hook eventually calls
        // `Context::create_widget` → `on_widget_under_pointer`, which tries
        // to re-acquire that same mutex and deadlocks (10s timeout panic in
        // `epaint::mutex`). We hit this every time a Mastertech plugin's
        // `ui()` opened a `Window` and the user hovered the resize edge.
        //
        // Plugin UIs are now driven from `MasterTechApp::ui` via
        // `PluginManagerHandle::render_plugin_uis`, which runs inside the
        // user-frame callback where the egui plugin mutex is NOT held.
    }

    fn input_hook(&mut self, input: &mut egui::RawInput) {
        if let Ok(mut guard) = self.0.write() {
            for plugin in &mut guard.plugins {
                if plugin.enabled() {
                    plugin.input_hook(input);
                }
            }
        }
    }

    fn output_hook(&mut self, output: &mut egui::FullOutput) {
        if let Ok(mut guard) = self.0.write() {
            for plugin in &mut guard.plugins {
                if plugin.enabled() {
                    plugin.output_hook(output);
                }
            }
        }
    }
}
