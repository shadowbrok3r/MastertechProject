//! WASM plugin runtime.
//!
//! Compiled only when `feature = "wasm-plugins"` is enabled.
//! Provides `WasmRuntime` and `WasmPlugin` — an adapter that wraps a wasmtime
//! module and implements `MastertechPlugin` by calling into WASM exports.
//!
//! ## ABI Contract
//!
//! A WASM plugin module must export the following functions.
//! String-returning exports use a packed `u64`: high 32 bits = pointer, low 32 bits = length.
//!
//! | Export            | Signature                                        | Description                           |
//! |-------------------|--------------------------------------------------|---------------------------------------|
//! | `plugin_id`       | `() -> u64`                                      | Return plugin ID (packed ptr\|len)    |
//! | `plugin_name`     | `() -> u64`                                      | Return display name (packed ptr\|len) |
//! | `plugin_version`  | `() -> u64`                                      | Return semver string (packed ptr\|len)|
//! | `on_load`         | `() -> ()`                                       | Called once when registered            |
//! | `on_unload`       | `() -> ()`                                       | Called when removed                    |
//! | `logic`           | `() -> ()`                                       | Called each frame (logic phase)        |
//! | `ui_commands`     | `() -> u64`                                      | Serialized draw commands (packed)      |
//! | `mcp_tools`       | `() -> u64`                                      | JSON array of tool descriptors (packed)|
//! | `handle_mcp_call` | `(tool_ptr, tool_len, args_ptr, args_len) -> u64`| JSON result (packed ptr\|len)         |
//! | `alloc`           | `(size: i32) -> i32`                             | Allocate guest memory                  |
//! | `dealloc`         | `(ptr: i32, size: i32) -> ()`                    | Free guest memory                      |
//!
//! The host provides these imports (module `"env"`):
//!
//! | Import            | Signature                    | Description                          |
//! |-------------------|------------------------------|--------------------------------------|
//! | `host_log`        | `(ptr: i32, len: i32) -> ()` | Log a UTF-8 message                  |
//! | `host_emit_event` | `(ptr: i32, len: i32) -> ()` | Emit a PluginEvent (JSON bytes)      |
//! | `host_repaint`    | `() -> ()`                   | Request a UI repaint                 |

use super::{MastertechPlugin, PluginEvent, PluginHost, PluginToolDescriptor};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::Mutex;

// ─── String interning ──────────────────────────────────────────────────────────

static INTERNED_IDS: Lazy<Mutex<HashSet<&'static str>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

/// Intern a `String` into a `&'static str`, deduplicating repeated values.
fn intern_string(s: String) -> &'static str {
    let mut set = INTERNED_IDS.lock().unwrap();
    if let Some(&existing) = set.get(s.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(s.into_boxed_str());
    set.insert(leaked);
    leaked
}

// ─── Packed pointer helpers ────────────────────────────────────────────────────

fn unpack_ptr_len(packed: u64) -> (i32, i32) {
    let ptr = (packed >> 32) as i32;
    let len = (packed & 0xFFFF_FFFF) as i32;
    (ptr, len)
}

// ─── WasmRuntime ───────────────────────────────────────────────────────────────

/// Holds the wasmtime `Engine` shared across all WASM plugin instances.
pub struct WasmRuntime {
    engine: wasmtime::Engine,
}

impl WasmRuntime {
    pub fn new() -> Result<Self, String> {
        let engine = wasmtime::Engine::default();
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

// ─── WasmPluginState ───────────────────────────────────────────────────────────

/// Per-instance state stored inside `wasmtime::Store` for host↔guest communication.
pub struct WasmPluginState {
    pub plugin_id: String,
    pub event_tx: crossbeam::channel::Sender<PluginEvent>,
}

// ─── WasmPlugin internals ──────────────────────────────────────────────────────

struct WasmPluginInner {
    store: wasmtime::Store<WasmPluginState>,
    instance: wasmtime::Instance,
}

/// A WASM-backed plugin that implements `MastertechPlugin` by calling exports.
///
/// The inner `Store` + `Instance` are wrapped in a `Mutex` so the outer struct
/// satisfies `Send + Sync` (required by `MastertechPlugin`). All access is
/// serialized through the `PluginManager`'s mutex anyway.
pub struct WasmPlugin {
    id: &'static str,
    name: String,
    version: String,
    enabled: bool,
    inner: Mutex<WasmPluginInner>,
}

impl WasmPlugin {
    /// Compile a WASM module from raw bytes, instantiate it with host imports,
    /// and call the identity exports to fill metadata.
    pub fn from_bytes(
        bytes: Vec<u8>,
        engine: &wasmtime::Engine,
        event_tx: crossbeam::channel::Sender<PluginEvent>,
    ) -> Result<Self, String> {
        let module = wasmtime::Module::new(engine, &bytes)
            .map_err(|e| format!("WASM compilation failed: {e}"))?;

        let state = WasmPluginState {
            plugin_id: String::new(),
            event_tx: event_tx.clone(),
        };
        let mut store = wasmtime::Store::new(engine, state);
        let mut linker = wasmtime::Linker::<WasmPluginState>::new(engine);

        // ── Host imports ────────────────────────────────────────────────────

        linker
            .func_wrap(
                "env",
                "host_log",
                |mut caller: wasmtime::Caller<'_, WasmPluginState>, ptr: i32, len: i32| {
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return;
                    };
                    let data = mem.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                        if let Ok(msg) = std::str::from_utf8(slice) {
                            log::info!("[WASM {}] {msg}", caller.data().plugin_id);
                        }
                    }
                },
            )
            .map_err(|e| format!("host_log link failed: {e}"))?;

        linker
            .func_wrap(
                "env",
                "host_emit_event",
                |mut caller: wasmtime::Caller<'_, WasmPluginState>, ptr: i32, len: i32| {
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return;
                    };
                    let data = mem.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                        if let Ok(event) = serde_json::from_slice::<PluginEvent>(slice) {
                            let _ = caller.data().event_tx.try_send(event);
                        }
                    }
                },
            )
            .map_err(|e| format!("host_emit_event link failed: {e}"))?;

        linker
            .func_wrap(
                "env",
                "host_repaint",
                |caller: wasmtime::Caller<'_, WasmPluginState>| {
                    let _ = caller.data().event_tx.try_send(PluginEvent::RequestRepaint);
                },
            )
            .map_err(|e| format!("host_repaint link failed: {e}"))?;

        // ── Instantiate ─────────────────────────────────────────────────────

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("WASM instantiation failed: {e}"))?;

        // ── Read identity exports ───────────────────────────────────────────

        let id_str = call_string_export(&instance, &mut store, "plugin_id")?;
        let name = call_string_export(&instance, &mut store, "plugin_name")?;
        let version = call_string_export(&instance, &mut store, "plugin_version")?;

        store.data_mut().plugin_id = id_str.clone();
        let id = intern_string(id_str);

        log::info!("WASM plugin loaded: {id} ({name} v{version})");

        Ok(Self {
            id,
            name,
            version,
            enabled: true,
            inner: Mutex::new(WasmPluginInner { store, instance }),
        })
    }
}

// ─── WASM memory helpers ───────────────────────────────────────────────────────

fn get_memory(
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<WasmPluginState>,
) -> Result<wasmtime::Memory, String> {
    instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| "WASM module has no 'memory' export".into())
}

fn call_string_export(
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<WasmPluginState>,
    name: &str,
) -> Result<String, String> {
    let func = instance
        .get_typed_func::<(), u64>(&mut *store, name)
        .map_err(|e| format!("Missing export '{name}': {e}"))?;
    let packed = func
        .call(&mut *store, ())
        .map_err(|e| format!("'{name}' call failed: {e}"))?;
    let (ptr, len) = unpack_ptr_len(packed);
    let memory = get_memory(instance, store)?;
    read_wasm_string(&memory, store, ptr, len)
}

fn read_wasm_string(
    memory: &wasmtime::Memory,
    store: &wasmtime::Store<WasmPluginState>,
    ptr: i32,
    len: i32,
) -> Result<String, String> {
    let data = memory.data(store);
    let start = ptr as usize;
    let end = start + len as usize;
    let slice = data
        .get(start..end)
        .ok_or_else(|| format!("Out-of-bounds WASM read at {start}..{end}"))?;
    String::from_utf8(slice.to_vec()).map_err(|e| format!("Invalid UTF-8 from WASM: {e}"))
}

/// Allocate space inside the guest and write `data` into it.
/// Returns `(ptr, len)` in guest address space.
fn write_to_wasm(
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<WasmPluginState>,
    data: &[u8],
) -> Result<(i32, i32), String> {
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut *store, "alloc")
        .map_err(|e| format!("Missing 'alloc' export: {e}"))?;
    let len = data.len() as i32;
    let ptr = alloc
        .call(&mut *store, len)
        .map_err(|e| format!("alloc({len}) failed: {e}"))?;
    let memory = get_memory(instance, store)?;
    memory
        .write(&mut *store, ptr as usize, data)
        .map_err(|e| format!("Memory write at offset {ptr} failed: {e}"))?;
    Ok((ptr, len))
}

fn call_void_export(
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<WasmPluginState>,
    name: &str,
) {
    if let Ok(func) = instance.get_typed_func::<(), ()>(&mut *store, name) {
        if let Err(e) = func.call(&mut *store, ()) {
            log::warn!("WASM export '{name}' failed: {e}");
        }
    }
}

// ─── MastertechPlugin impl ────────────────────────────────────────────────────

impl MastertechPlugin for WasmPlugin {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn description(&self) -> &str {
        "WASM-loaded plugin"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn on_load(&mut self, _host: &PluginHost) {
        if let Ok(mut inner) = self.inner.lock() {
            let WasmPluginInner { ref instance, ref mut store } = *inner;
            call_void_export(instance, store, "on_load");
        }
    }

    fn on_unload(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            let WasmPluginInner { ref instance, ref mut store } = *inner;
            call_void_export(instance, store, "on_unload");
        }
    }

    fn logic(&mut self, _host: &PluginHost) {
        if let Ok(mut inner) = self.inner.lock() {
            let WasmPluginInner { ref instance, ref mut store } = *inner;
            call_void_export(instance, store, "logic");
        }
    }

    fn ui(&mut self, _ui: &mut eframe::egui::Ui, _host: &PluginHost) {
        // WASM draw commands require the WireShape deserialization protocol.
        // The guest calls `ui_commands()` → packed ptr|len of serialized commands.
        // Rendering these into egui will be wired in a future iteration.
    }

    fn mcp_tools(&self) -> Vec<PluginToolDescriptor> {
        let Ok(mut inner) = self.inner.lock() else {
            return vec![];
        };
        let WasmPluginInner { ref instance, ref mut store } = *inner;
        match call_string_export(instance, store, "mcp_tools") {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => vec![],
        }
    }

    fn handle_mcp_call(
        &mut self,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let WasmPluginInner { ref instance, ref mut store } = *inner;

        let tool_bytes = tool.as_bytes();
        let args_bytes = serde_json::to_vec(&args).map_err(|e| e.to_string())?;

        let (tool_ptr, tool_len) = write_to_wasm(instance, store, tool_bytes)?;
        let (args_ptr, args_len) = write_to_wasm(instance, store, &args_bytes)?;

        let func = instance
            .get_typed_func::<(i32, i32, i32, i32), u64>(&mut *store, "handle_mcp_call")
            .map_err(|e| format!("Missing 'handle_mcp_call' export: {e}"))?;

        let packed = func
            .call(&mut *store, (tool_ptr, tool_len, args_ptr, args_len))
            .map_err(|e| format!("handle_mcp_call failed: {e}"))?;

        let (rptr, rlen) = unpack_ptr_len(packed);
        let memory = get_memory(instance, store)?;
        let json_str = read_wasm_string(&memory, store, rptr, rlen)?;
        serde_json::from_str(&json_str).map_err(|e| format!("Invalid JSON from WASM: {e}"))
    }
}
