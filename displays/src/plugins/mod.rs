pub mod host;
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub mod mcp_bridge;
pub mod remote;
#[cfg(feature = "wasm-plugins")]
pub mod wasm;

pub use host::{PluginHost, PluginEvent, NotificationKind, ClientSnapshot, SystemInfoSnapshot, UserSnapshot};
pub use remote::{EguiFrameCapture, EguiRemoteViewer, EguiFrameMessage, EguiInputEvent};

use crossbeam::channel::Receiver;
use eframe::egui;
use serde::{Deserialize, Serialize};

/// Descriptor for an MCP tool that a plugin exposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolDescriptor {
    pub name: String,
    pub description: String,
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

// ─── Plugin Manager ────────────────────────────────────────────────────────────

/// Manages all `MastertechPlugin` instances and bridges them into egui's `Plugin` system.
///
/// Wrap in `Arc<Mutex<>>` and register via `ctx.add_plugin(PluginManagerHandle(arc))`.
pub struct PluginManager {
    pub(crate) plugins: Vec<Box<dyn MastertechPlugin>>,
    pub(crate) host: PluginHost,
    event_rx: Receiver<PluginEvent>,
    pub(crate) setup_done: bool,
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

    pub(crate) fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match &event {
                PluginEvent::RequestRepaint => {
                    if let Some(ctx) = &self.host.ctx {
                        ctx.request_repaint();
                    }
                }
                PluginEvent::ShowNotification { title, body, kind } => {
                    log::info!("Plugin notification [{kind:?}]: {title} - {body}");
                }
                PluginEvent::RunScript { client_id, filename, .. } => {
                    log::info!("Plugin requests script run: {filename} on {client_id}");
                }
                PluginEvent::SendWsCommand { client_id, .. } => {
                    log::info!("Plugin sends WS command to {client_id}");
                }
                PluginEvent::Custom { plugin_id, event_type, .. } => {
                    log::debug!("Custom event from {plugin_id}: {event_type}");
                }
                _ => {}
            }
        }
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
    pub fn load_wasm(&mut self, _bytes: Vec<u8>) -> Result<(), String> {
        let plugin = wasm::WasmPlugin::from_bytes(_bytes)?;
        self.register(Box::new(plugin));
        Ok(())
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── egui::Plugin bridge ───────────────────────────────────────────────────────

/// Thin wrapper around `Arc<Mutex<PluginManager>>` that implements `egui::Plugin`.
///
/// Register with `ctx.add_plugin(PluginManagerHandle(arc.clone()))`.
/// The `Arc<Mutex<PluginManager>>` remains accessible for MCP bridge and external callers.
pub struct PluginManagerHandle(pub std::sync::Arc<std::sync::Mutex<PluginManager>>);

impl egui::Plugin for PluginManagerHandle {
    fn debug_name(&self) -> &'static str {
        "MastertechPluginManager"
    }

    fn setup(&mut self, ctx: &egui::Context) {
        if let Ok(mut guard) = self.0.lock() {
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
        if let Ok(mut guard) = self.0.lock() {
            let mgr = &mut *guard;
            mgr.process_events();
            for plugin in &mut mgr.plugins {
                if plugin.enabled() {
                    plugin.logic(&mgr.host);
                }
            }
        }
    }

    fn on_end_pass(&mut self, ui: &mut egui::Ui) {
        if let Ok(mut guard) = self.0.lock() {
            let mgr = &mut *guard;
            for plugin in &mut mgr.plugins {
                if plugin.enabled() {
                    plugin.ui(ui, &mgr.host);
                }
            }
        }
    }

    fn input_hook(&mut self, input: &mut egui::RawInput) {
        if let Ok(mut guard) = self.0.lock() {
            for plugin in &mut guard.plugins {
                if plugin.enabled() {
                    plugin.input_hook(input);
                }
            }
        }
    }

    fn output_hook(&mut self, output: &mut egui::FullOutput) {
        if let Ok(mut guard) = self.0.lock() {
            for plugin in &mut guard.plugins {
                if plugin.enabled() {
                    plugin.output_hook(output);
                }
            }
        }
    }
}
