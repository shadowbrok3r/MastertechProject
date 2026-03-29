//! WASM plugin runtime stub.
//!
//! This module is only compiled when `feature = "wasm-plugins"` is enabled.
//! It provides `WasmRuntime` and `WasmPlugin` -- an adapter that wraps a wasmtime
//! module and implements `MastertechPlugin` by calling into WASM exports.
//!
//! ## ABI Contract
//!
//! A WASM plugin module must export the following functions:
//!
//! | Export            | Signature                     | Description                          |
//! |-------------------|-------------------------------|--------------------------------------|
//! | `plugin_id`       | `() -> *const u8, len: i32`   | Return plugin ID as UTF-8 bytes      |
//! | `plugin_name`     | `() -> *const u8, len: i32`   | Return plugin display name           |
//! | `plugin_version`  | `() -> *const u8, len: i32`   | Return semver version string         |
//! | `on_load`         | `() -> ()`                    | Called once when registered           |
//! | `on_unload`       | `() -> ()`                    | Called when removed                   |
//! | `logic`           | `() -> ()`                    | Called each frame (logic phase)       |
//! | `ui_commands`     | `() -> *const u8, len: i32`   | Returns serialized draw commands      |
//! | `mcp_tools`       | `() -> *const u8, len: i32`   | Returns JSON array of tool descriptors|
//! | `handle_mcp_call` | `(tool_ptr, tool_len, args_ptr, args_len) -> *const u8, len: i32` | |
//!
//! The host provides these imports to the WASM module:
//!
//! | Import            | Signature                     | Description                          |
//! |-------------------|-------------------------------|--------------------------------------|
//! | `host_log`        | `(ptr: *const u8, len: i32)`  | Log a message                        |
//! | `host_emit_event` | `(ptr: *const u8, len: i32)`  | Emit a PluginEvent (JSON serialized) |
//! | `host_repaint`    | `()`                          | Request a UI repaint                 |

use super::{MastertechPlugin, PluginHost, PluginToolDescriptor};

/// The WASM runtime environment. Holds the wasmtime engine and manages stores.
///
/// Future: will support loading multiple WASM modules, each as a `WasmPlugin`.
pub struct WasmRuntime {
    engine: wasmtime::Engine,
}

impl WasmRuntime {
    pub fn new() -> Result<Self, String> {
        let engine =
            wasmtime::Engine::default();
        Ok(Self { engine })
    }

    pub fn engine(&self) -> &wasmtime::Engine {
        &self.engine
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create wasmtime engine")
    }
}

/// State stored in the wasmtime `Store` for host↔guest communication.
pub struct WasmPluginState {
    pub plugin_id: String,
    pub event_tx: Option<crossbeam::channel::Sender<super::PluginEvent>>,
}

/// A WASM-backed plugin that implements `MastertechPlugin` by calling into WASM exports.
///
/// Currently a structural stub -- the actual wasmtime instantiation, memory management,
/// and ABI bridging are defined as types but not fully wired. This provides the contract
/// and compilation scaffolding so real WASM plugins can be developed against it.
pub struct WasmPlugin {
    id: String,
    name: String,
    version: String,
    enabled: bool,
    _module_bytes: Vec<u8>,
}

impl WasmPlugin {
    /// Create a `WasmPlugin` from raw WASM module bytes.
    ///
    /// In a full implementation, this would:
    /// 1. Compile the WASM module via `wasmtime::Module::new(engine, bytes)`
    /// 2. Create a `Store<WasmPluginState>`
    /// 3. Instantiate with host function imports
    /// 4. Call `plugin_id`, `plugin_name`, `plugin_version` exports
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        // Stub: extract metadata from a header or use defaults
        Ok(Self {
            id: "wasm.unknown".into(),
            name: "WASM Plugin (stub)".into(),
            version: "0.0.0".into(),
            enabled: true,
            _module_bytes: bytes,
        })
    }
}

impl MastertechPlugin for WasmPlugin {
    fn id(&self) -> &'static str {
        // Leak the string so we get a 'static reference.
        // In production, use an interned string pool instead.
        Box::leak(self.id.clone().into_boxed_str())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn description(&self) -> &str {
        "WASM-loaded plugin (stub implementation)"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn on_load(&mut self, _host: &PluginHost) {
        log::info!("WasmPlugin::on_load stub for {}", self.id);
        // TODO: call wasm export `on_load`
    }

    fn on_unload(&mut self) {
        log::info!("WasmPlugin::on_unload stub for {}", self.id);
        // TODO: call wasm export `on_unload`
    }

    fn logic(&mut self, _host: &PluginHost) {
        // TODO: call wasm export `logic`
    }

    fn ui(&mut self, _ui: &mut eframe::egui::Ui, _host: &PluginHost) {
        // TODO: call wasm export `ui_commands`, deserialize draw commands, replay via ctx
    }

    fn mcp_tools(&self) -> Vec<PluginToolDescriptor> {
        // TODO: call wasm export `mcp_tools`, deserialize JSON
        vec![]
    }

    fn handle_mcp_call(
        &mut self,
        tool: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err(format!(
            "WasmPlugin::handle_mcp_call stub -- tool '{tool}' not implemented"
        ))
    }
}
