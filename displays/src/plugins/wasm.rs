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
//! | `host_fill_clock_json` | `(ptr: i32, max_len: i32) -> i32` | Writes UTC clock JSON into guest memory; returns byte length (≤ max_len) |
//! | `host_get_hostname` | `(ptr: i32, max_len: i32) -> i32` | Writes hostname into guest memory; returns byte length |
//! | `host_run_command` | `(cmd_ptr, cmd_len, out_ptr, out_max) -> i32` | Run a shell command (PowerShell on Windows, sh on Linux); writes stdout into guest memory; returns byte length |

use super::{MastertechPlugin, PluginEvent, PluginHost, PluginToolDescriptor};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::Mutex;
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

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
    pub wasi: WasiP1Ctx,
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

        // Build a minimal WASI context (no filesystem, no network, but allows std allocation).
        // This satisfies `wasi_snapshot_preview1::fd_write` and friends used by Rust's std.
        let wasi = WasiCtxBuilder::new().build_p1();
        let state = WasmPluginState {
            plugin_id: String::new(),
            event_tx: event_tx.clone(),
            wasi,
        };
        let mut store = wasmtime::Store::new(engine, state);
        let mut linker = wasmtime::Linker::<WasmPluginState>::new(engine);

        // Link all WASI preview1 host functions (fd_write, fd_read, proc_exit, etc.)
        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s: &mut WasmPluginState| &mut s.wasi)
            .map_err(|e| format!("WASI preview1 link failed: {e}"))?;

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

        linker
            .func_wrap(
                "env",
                "host_fill_clock_json",
                |mut caller: wasmtime::Caller<'_, WasmPluginState>, ptr: i32, max_len: i32| -> i32 {
                    let now = chrono::Utc::now();
                    let json = serde_json::json!({
                        "unix_ms": now.timestamp_millis(),
                        "iso_utc": now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                    });
                    let s = match serde_json::to_string(&json) {
                        Ok(s) => s,
                        Err(_) => r#"{"error":"json"}"#.to_string(),
                    };
                    let bytes = s.as_bytes();
                    let cap = max_len.max(0) as usize;
                    let n = bytes.len().min(cap);
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return 0;
                    };
                    if mem.write(&mut caller, ptr as usize, &bytes[..n]).is_err() {
                        return 0;
                    }
                    n as i32
                },
            )
            .map_err(|e| format!("host_fill_clock_json link failed: {e}"))?;

        // ── Diagnostic host imports ─────────────────────────────────────────

        linker
            .func_wrap(
                "env",
                "host_get_hostname",
                |mut caller: wasmtime::Caller<'_, WasmPluginState>, ptr: i32, max_len: i32| -> i32 {
                    let hostname = std::env::var("COMPUTERNAME")
                        .or_else(|_| std::env::var("HOSTNAME"))
                        .unwrap_or_else(|_| "unknown".to_string());
                    let bytes = hostname.as_bytes();
                    let cap = max_len.max(0) as usize;
                    let n = bytes.len().min(cap);
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return 0;
                    };
                    if mem.write(&mut caller, ptr as usize, &bytes[..n]).is_err() {
                        return 0;
                    }
                    n as i32
                },
            )
            .map_err(|e| format!("host_get_hostname link failed: {e}"))?;

        linker
            .func_wrap(
                "env",
                "host_run_command",
                |mut caller: wasmtime::Caller<'_, WasmPluginState>,
                 cmd_ptr: i32, cmd_len: i32,
                 out_ptr: i32, out_max: i32| -> i32 {
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return 0;
                    };
                    let data = mem.data(&caller);
                    let cmd_slice = match data.get(cmd_ptr as usize..(cmd_ptr as usize + cmd_len as usize)) {
                        Some(s) => s,
                        None => return 0,
                    };
                    let cmd_str = match std::str::from_utf8(cmd_slice) {
                        Ok(s) => s.to_string(),
                        Err(_) => return 0,
                    };

                    log::info!("[WASM {}] host_run_command: {}", caller.data().plugin_id, cmd_str);

                    #[cfg(target_os = "windows")]
                    let output = std::process::Command::new("powershell")
                        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd_str])
                        .output();
                    #[cfg(not(target_os = "windows"))]
                    let output = std::process::Command::new("sh")
                        .args(["-c", &cmd_str])
                        .output();

                    let result = match output {
                        Ok(out) => {
                            let stdout = String::from_utf8_lossy(&out.stdout);
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            if stderr.is_empty() {
                                stdout.to_string()
                            } else {
                                format!("{stdout}\n[stderr] {stderr}")
                            }
                        }
                        Err(e) => format!("[error] {e}"),
                    };

                    let bytes = result.as_bytes();
                    let cap = out_max.max(0) as usize;
                    let n = bytes.len().min(cap);
                    if mem.write(&mut caller, out_ptr as usize, &bytes[..n]).is_err() {
                        return 0;
                    }
                    n as i32
                },
            )
            .map_err(|e| format!("host_run_command link failed: {e}"))?;

        // ── Plugin UI host imports ────────────────────────────────────────────

        linker
            .func_wrap(
                "env",
                "host_ui_log",
                |mut caller: wasmtime::Caller<'_, WasmPluginState>, ptr: i32, len: i32| {
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return;
                    };
                    let data = mem.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                        if let Ok(json) = std::str::from_utf8(slice) {
                            let pid = caller.data().plugin_id.clone();
                            super::plugin_ui_append(&pid, json.to_string());
                            let _ = caller.data().event_tx.try_send(PluginEvent::RequestRepaint);
                        }
                    }
                },
            )
            .map_err(|e| format!("host_ui_log link failed: {e}"))?;

        linker
            .func_wrap(
                "env",
                "host_ui_clear",
                |caller: wasmtime::Caller<'_, WasmPluginState>| {
                    let pid = caller.data().plugin_id.clone();
                    super::plugin_ui_clear(&pid);
                    let _ = caller.data().event_tx.try_send(PluginEvent::RequestRepaint);
                },
            )
            .map_err(|e| format!("host_ui_clear link failed: {e}"))?;

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

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _host: &PluginHost) {
        let state = super::PLUGIN_UI_STATES.lock().ok()
            .and_then(|s| s.get(self.id).cloned());
        let Some(state) = state else { return; };
        if state.entries.is_empty() || !state.visible { return; }

        let ctx = ui.ctx().clone();
        let mut open = true;
        eframe::egui::Window::new(format!("📋 {} Report", self.name))
            .id(eframe::egui::Id::new(format!("plugin_ui_window_{}", self.id)))
            .default_width(680.0)
            .default_height(500.0)
            .open(&mut open)
            .vscroll(true)
            .show(&ctx, |ui| {
                render_plugin_ui_entries(ui, &state.entries);
            });

        if !open {
            if let Ok(mut map) = super::PLUGIN_UI_STATES.lock() {
                if let Some(s) = map.get_mut(self.id) {
                    s.visible = false;
                }
            }
        }
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

// ─── Plugin UI renderer ─────────────────────────────────────────────────────
//
// Parses JSON entries from PLUGIN_UI_STATES and renders them as egui widgets.
//
// Supported entry types:
//   header   — large heading + optional subtitle
//   section  — collapsing section with nested items
//   status   — colored pass/fail/warn/info badge + label + detail
//   text     — plain or colored text
//   separator
//   table    — column headers + rows
//   log      — timestamped log entry with level coloring
//   progress — labeled progress bar

fn render_plugin_ui_entries(ui: &mut eframe::egui::Ui, entries: &[super::PluginUiEntry]) {
    use eframe::egui;

    for entry in entries {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&entry.json) else {
            ui.colored_label(egui::Color32::RED, format!("⚠ Bad UI entry: {}", &entry.json[..entry.json.len().min(80)]));
            continue;
        };
        let entry_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("text");

        match entry_type {
            "header" => {
                let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("Report");
                let subtitle = v.get("subtitle").and_then(|t| t.as_str());
                ui.add_space(4.0);
                ui.heading(egui::RichText::new(title).strong().size(20.0));
                if let Some(sub) = subtitle {
                    ui.label(egui::RichText::new(sub).weak().italics());
                }
                ui.add_space(4.0);
            }

            "section" => {
                let header = v.get("header").and_then(|t| t.as_str()).unwrap_or("Section");
                let default_open = v.get("default_open").and_then(|b| b.as_bool()).unwrap_or(true);
                let status = v.get("status").and_then(|t| t.as_str());
                let badge = match status {
                    Some("pass") => "✅ ",
                    Some("fail") => "❌ ",
                    Some("warn") => "⚠️ ",
                    _ => "",
                };
                let id = egui::Id::new(format!("plugin_section_{}", header));
                egui::CollapsingHeader::new(
                    egui::RichText::new(format!("{badge}{header}")).strong()
                )
                .id_salt(id)
                .default_open(default_open)
                .show(ui, |ui| {
                    if let Some(items) = v.get("items").and_then(|a| a.as_array()) {
                        let sub_entries: Vec<super::PluginUiEntry> = items
                            .iter()
                            .map(|item| super::PluginUiEntry { json: item.to_string() })
                            .collect();
                        render_plugin_ui_entries(ui, &sub_entries);
                    }
                });
            }

            "status" => {
                let label = v.get("label").and_then(|t| t.as_str()).unwrap_or("");
                let status = v.get("status").and_then(|t| t.as_str()).unwrap_or("info");
                let detail = v.get("detail").and_then(|t| t.as_str()).unwrap_or("");
                let (icon, color) = match status {
                    "pass" => ("✅", egui::Color32::from_rgb(80, 200, 80)),
                    "fail" => ("❌", egui::Color32::from_rgb(220, 60, 60)),
                    "warn" => ("⚠️", egui::Color32::from_rgb(220, 180, 40)),
                    _ =>      ("ℹ️", egui::Color32::from_rgb(120, 170, 220)),
                };
                ui.horizontal(|ui| {
                    ui.label(icon);
                    ui.colored_label(color, egui::RichText::new(label).strong());
                    if !detail.is_empty() {
                        ui.label(egui::RichText::new(format!("— {detail}")).weak());
                    }
                });
            }

            "text" => {
                let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if let Some(color_arr) = v.get("color").and_then(|c| c.as_array()) {
                    let r = color_arr.first().and_then(|v| v.as_u64()).unwrap_or(200) as u8;
                    let g = color_arr.get(1).and_then(|v| v.as_u64()).unwrap_or(200) as u8;
                    let b = color_arr.get(2).and_then(|v| v.as_u64()).unwrap_or(200) as u8;
                    ui.colored_label(egui::Color32::from_rgb(r, g, b), text);
                } else {
                    let mono = v.get("mono").and_then(|b| b.as_bool()).unwrap_or(false);
                    if mono {
                        ui.label(egui::RichText::new(text).monospace().size(12.0));
                    } else {
                        ui.label(text);
                    }
                }
            }

            "separator" => {
                ui.separator();
            }

            "table" => {
                let headers = v.get("headers").and_then(|a| a.as_array());
                let rows = v.get("rows").and_then(|a| a.as_array());
                if let (Some(headers), Some(rows)) = (headers, rows) {
                    let col_count = headers.len();
                    egui::Grid::new(format!("plugin_table_{}", entries.len()))
                        .num_columns(col_count)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for h in headers {
                                let text = h.as_str().unwrap_or("");
                                ui.label(egui::RichText::new(text).strong().underline());
                            }
                            ui.end_row();
                            for row in rows {
                                if let Some(cells) = row.as_array() {
                                    for cell in cells {
                                        let text = cell.as_str().unwrap_or("");
                                        let rt = if text.contains("PASS") || text.contains("CLEAN") {
                                            egui::RichText::new(text).color(egui::Color32::from_rgb(80, 200, 80))
                                        } else if text.contains("FAIL") {
                                            egui::RichText::new(text).color(egui::Color32::from_rgb(220, 60, 60))
                                        } else if text.contains("WARN") || text.contains("ATTENTION") {
                                            egui::RichText::new(text).color(egui::Color32::from_rgb(220, 180, 40))
                                        } else {
                                            egui::RichText::new(text)
                                        };
                                        ui.label(rt);
                                    }
                                    ui.end_row();
                                }
                            }
                        });
                }
            }

            "log" => {
                let ts = v.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                let level = v.get("level").and_then(|t| t.as_str()).unwrap_or("info");
                let msg = v.get("message").and_then(|t| t.as_str()).unwrap_or("");
                let color = match level {
                    "error" => egui::Color32::from_rgb(220, 60, 60),
                    "warn"  => egui::Color32::from_rgb(220, 180, 40),
                    "pass" | "success" => egui::Color32::from_rgb(80, 200, 80),
                    _ =>       egui::Color32::from_rgb(160, 160, 160),
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ts).monospace().weak().size(11.0));
                    ui.colored_label(color, egui::RichText::new(format!("[{level}]")).monospace().size(11.0));
                    ui.label(egui::RichText::new(msg).monospace().size(11.0));
                });
            }

            "progress" => {
                let label = v.get("label").and_then(|t| t.as_str()).unwrap_or("");
                let value = v.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                ui.horizontal(|ui| {
                    ui.label(label);
                    let bar = egui::ProgressBar::new(value)
                        .text(format!("{:.0}%", value * 100.0))
                        .desired_width(200.0);
                    ui.add(bar);
                });
            }

            "space" => {
                let px = v.get("px").and_then(|v| v.as_f64()).unwrap_or(8.0) as f32;
                ui.add_space(px);
            }

            _ => {
                ui.label(egui::RichText::new(format!("Unknown UI type: {entry_type}")).weak());
            }
        }
    }
}
