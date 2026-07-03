//! In-firmware WASM plugin runtime.
//!
//! Runs the same `wasm32-wasip1` Mastertech plugins the desktop app runs, on
//! the pure-Rust wasmi interpreter — firmware has no executable-page allocator,
//! so a JIT (wasmtime) is impossible here. Provides the plugin `env` host ABI
//! and a minimal WASI preview1 shim (enough for a Rust cdylib to instantiate).
//! Tool dispatch follows the packed `(ptr<<32)|len` convention; a tool result
//! is the plugin's own JSON string.

use wasmi::{AsContext, Caller, Engine, Extern, Instance, Linker, Memory, Module, Store};

use crate::logln;

/// Pre-collected firmware data + capability bits handed to a plugin run so the
/// read-only `host_fw_*` JSON queries need no live re-collection or borrows.
#[derive(Default, Clone)]
pub struct FwData {
    pub caps: u64,
    pub json: Vec<(String, String)>,
}

impl FwData {
    pub fn push(&mut self, kind: &str, json: String) {
        self.json.push((kind.to_string(), json));
    }
    fn get(&self, kind: &str) -> Option<&str> {
        self.json.iter().find(|(k, _)| k == kind).map(|(_, v)| v.as_str())
    }
}

/// Host-side state threaded through every plugin call.
pub struct HostState {
    hostname: String,
    log: Vec<String>,
    ui: Vec<String>,
    stdout: Vec<u8>,
    rng: u64,
    fw: FwData,
}

impl HostState {
    fn new(hostname: String, fw: FwData) -> Self {
        Self {
            hostname,
            log: Vec::new(),
            ui: Vec::new(),
            stdout: Vec::new(),
            rng: 0x2545_F491_4F6C_DD1D,
            fw,
        }
    }
    fn next_rand(&mut self) -> u8 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x >> 33) as u8
    }
}

/// Outcome of loading and (optionally) invoking a plugin.
pub struct PluginRun {
    pub id: String,
    pub name: String,
    pub version: String,
    pub tools: String,
    pub result: String,
    pub log: Vec<String>,
    pub stdout: String,
}

fn caller_mem(caller: &Caller<HostState>) -> Option<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Some(m),
        _ => None,
    }
}

fn read_bytes(mem: &Memory, ctx: impl AsContext, ptr: i32, len: i32) -> Vec<u8> {
    if ptr < 0 || len <= 0 || len > (16 << 20) {
        return Vec::new();
    }
    let mut buf = vec![0u8; len as usize];
    if mem.read(ctx, ptr as usize, &mut buf).is_ok() {
        buf
    } else {
        Vec::new()
    }
}

fn read_str(mem: &Memory, ctx: impl AsContext, ptr: i32, len: i32) -> String {
    String::from_utf8_lossy(&read_bytes(mem, ctx, ptr, len)).into_owned()
}

/// Define the plugin `env` host functions and a WASI preview1 shim. Defining
/// more imports than a given module uses is harmless; a missing one it *does*
/// import would fail instantiation, so the set is deliberately broad.
fn link_host(linker: &mut Linker<HostState>) -> Result<(), String> {
    macro_rules! wrap {
        ($m:expr, $n:expr, $f:expr) => {
            linker
                .func_wrap($m, $n, $f)
                .map(|_| ())
                .map_err(|e| format!("link {}.{}: {e:?}", $m, $n))?;
        };
    }

    // --- plugin "env" ABI ---
    wrap!("env", "host_log", |mut caller: Caller<HostState>, ptr: i32, len: i32| {
        if let Some(mem) = caller_mem(&caller) {
            let s = read_str(&mem, &caller, ptr, len);
            logln(format!("[wasm] {s}"));
            caller.data_mut().log.push(s);
        }
    });
    wrap!("env", "host_emit_event", |mut caller: Caller<HostState>, ptr: i32, len: i32| {
        if let Some(mem) = caller_mem(&caller) {
            let s = read_str(&mem, &caller, ptr, len);
            caller.data_mut().log.push(format!("event: {s}"));
        }
    });
    wrap!("env", "host_repaint", |_caller: Caller<HostState>| {});
    wrap!("env", "host_ui_log", |mut caller: Caller<HostState>, ptr: i32, len: i32| {
        if let Some(mem) = caller_mem(&caller) {
            let s = read_str(&mem, &caller, ptr, len);
            caller.data_mut().ui.push(s);
        }
    });
    wrap!("env", "host_ui_clear", |mut caller: Caller<HostState>| {
        caller.data_mut().ui.clear();
    });
    wrap!("env", "host_fill_clock_json", |mut caller: Caller<HostState>, ptr: i32, max: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return 0 };
        let json = clock_json();
        let n = (json.len() as i32).min(max.max(0));
        let _ = mem.write(&mut caller, ptr as usize, &json.as_bytes()[..n as usize]);
        n
    });
    wrap!("env", "host_get_hostname", |mut caller: Caller<HostState>, ptr: i32, max: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return 0 };
        let name = caller.data().hostname.clone();
        let n = (name.len() as i32).min(max.max(0));
        let _ = mem.write(&mut caller, ptr as usize, &name.as_bytes()[..n as usize]);
        n
    });
    // No shell in firmware: return a JSON error into the out buffer.
    wrap!("env", "host_run_command", |mut caller: Caller<HostState>, _cp: i32, _cl: i32, out: i32, max: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return 0 };
        let msg = br#"{"error":"host_run_command unavailable in firmware"}"#;
        let n = (msg.len() as i32).min(max.max(0));
        let _ = mem.write(&mut caller, out as usize, &msg[..n as usize]);
        n
    });

    // --- WASI preview1 shim (module name wasi_snapshot_preview1) ---
    const WASI: &str = "wasi_snapshot_preview1";
    // Route stdout/stderr writes into the captured buffer + host log.
    wrap!(WASI, "fd_write", |mut caller: Caller<HostState>, _fd: i32, iovs: i32, iovs_len: i32, nwritten: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return 8 };
        let mut collected = Vec::new();
        let mut total: u32 = 0;
        for i in 0..iovs_len.max(0) {
            let base = iovs as usize + i as usize * 8;
            let mut hdr = [0u8; 8];
            if mem.read(&caller, base, &mut hdr).is_err() {
                break;
            }
            let bptr = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
            let blen = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
            let b = read_bytes(&mem, &caller, bptr as i32, blen as i32);
            total = total.saturating_add(b.len() as u32);
            collected.extend_from_slice(&b);
        }
        let _ = mem.write(&mut caller, nwritten as usize, &total.to_le_bytes());
        if !collected.is_empty() {
            caller.data_mut().stdout.extend_from_slice(&collected);
        }
        0
    });
    wrap!(WASI, "fd_read", |_c: Caller<HostState>, _fd: i32, _iovs: i32, _n: i32, _nr: i32| -> i32 { 0 });
    wrap!(WASI, "fd_close", |_c: Caller<HostState>, _fd: i32| -> i32 { 0 });
    wrap!(WASI, "fd_seek", |_c: Caller<HostState>, _fd: i32, _off: i64, _w: i32, _np: i32| -> i32 { 0 });
    wrap!(WASI, "fd_fdstat_get", |_c: Caller<HostState>, _fd: i32, _buf: i32| -> i32 { 0 });
    wrap!(WASI, "fd_prestat_get", |_c: Caller<HostState>, _fd: i32, _buf: i32| -> i32 { 8 });
    wrap!(WASI, "fd_prestat_dir_name", |_c: Caller<HostState>, _fd: i32, _p: i32, _l: i32| -> i32 { 8 });
    // environ/args: report empty and zero the count/size out-params.
    wrap!(WASI, "environ_get", |_c: Caller<HostState>, _e: i32, _b: i32| -> i32 { 0 });
    wrap!(WASI, "environ_sizes_get", |mut caller: Caller<HostState>, count: i32, size: i32| -> i32 {
        if let Some(mem) = caller_mem(&caller) {
            let _ = mem.write(&mut caller, count as usize, &0u32.to_le_bytes());
            let _ = mem.write(&mut caller, size as usize, &0u32.to_le_bytes());
        }
        0
    });
    wrap!(WASI, "args_get", |_c: Caller<HostState>, _a: i32, _b: i32| -> i32 { 0 });
    wrap!(WASI, "args_sizes_get", |mut caller: Caller<HostState>, argc: i32, size: i32| -> i32 {
        if let Some(mem) = caller_mem(&caller) {
            let _ = mem.write(&mut caller, argc as usize, &0u32.to_le_bytes());
            let _ = mem.write(&mut caller, size as usize, &0u32.to_le_bytes());
        }
        0
    });
    wrap!(WASI, "clock_time_get", |mut caller: Caller<HostState>, _id: i32, _prec: i64, time: i32| -> i32 {
        if let Some(mem) = caller_mem(&caller) {
            let _ = mem.write(&mut caller, time as usize, &clock_unix_ns().to_le_bytes());
        }
        0
    });
    wrap!(WASI, "random_get", |mut caller: Caller<HostState>, buf: i32, len: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return 8 };
        for i in 0..len.max(0) {
            let b = caller.data_mut().next_rand();
            if mem.write(&mut caller, (buf + i) as usize, &[b]).is_err() {
                break;
            }
        }
        0
    });
    wrap!(WASI, "sched_yield", |_c: Caller<HostState>| -> i32 { 0 });
    // proc_exit: log and return; a tool call should never reach it.
    wrap!(WASI, "proc_exit", |mut caller: Caller<HostState>, code: i32| {
        caller.data_mut().log.push(format!("proc_exit({code})"));
    });

    // --- firmware primitives (read-only Tier 1) ---
    wrap!("env", "host_fw_abi_version", |_c: Caller<HostState>| -> i32 { FW_ABI_VERSION });
    wrap!("env", "host_fw_capabilities", |caller: Caller<HostState>| -> i64 {
        caller.data().fw.caps as i64
    });
    // Read an IA32 MSR (x86 only; returns 0 elsewhere). Ring 0 in firmware.
    wrap!("env", "host_fw_rdmsr", |_c: Caller<HostState>, msr: i32| -> i64 {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            crate::stress::rdmsr(msr as u32) as i64
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = msr;
            0
        }
    });
    // PCI config dword read; -1 on failure.
    wrap!("env", "host_fw_pci_read32", |_c: Caller<HostState>, bus: i32, dev: i32, func: i32, reg: i32| -> i64 {
        match crate::rb_open() {
            Some(mut root) => crate::cfg_rd32(&mut root, bus as u8, dev as u8, func as u8, reg as u8)
                .map(|v| v as i64)
                .unwrap_or(-1),
            None => -1,
        }
    });
    // CMOS/RTC register read (0x0E-0x3F config region); -1 on failure.
    wrap!("env", "host_fw_cmos_read", |_c: Caller<HostState>, idx: i32| -> i32 {
        match crate::rb_open() {
            Some(mut root) => crate::cmos_read(&mut root, idx as u8).map(|v| v as i32).unwrap_or(-1),
            None => -1,
        }
    });
    // SMBus word read at a 7-bit address/register (SPD, PMIC, temp); -1 on failure.
    wrap!("env", "host_fw_smbus_read_word", |_c: Caller<HostState>, addr7: i32, reg: i32| -> i32 {
        match crate::rb_open() {
            Some(mut root) => match crate::smbus_base(&mut root) {
                Some(base) => crate::smb_read_word(&mut root, base, addr7 as u8, reg as u8)
                    .map(|v| v as i32)
                    .unwrap_or(-1),
                None => -1,
            },
            None => -1,
        }
    });
    // Read a GLOBAL UEFI variable by name into the guest buffer; returns the
    // variable's full length (write is truncated to max), or -1 if absent.
    wrap!("env", "host_fw_get_variable", |mut caller: Caller<HostState>, np: i32, nl: i32, op: i32, om: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return -1 };
        let name = read_str(&mem, &caller, np, nl);
        let Ok(cn) = uefi::CString16::try_from(name.as_str()) else { return -1 };
        match uefi::runtime::get_variable_boxed(&cn, &uefi::runtime::VariableVendor::GLOBAL_VARIABLE) {
            Ok((data, _)) => {
                let n = (data.len() as i32).min(om.max(0));
                let _ = mem.write(&mut caller, op as usize, &data[..n as usize]);
                data.len() as i32
            }
            Err(_) => -1,
        }
    });
    // Fetch a pre-collected JSON blob by kind ("bert","fpdt","msdm","smbios",
    // "esrt","bootdiag","bios_settings") into the guest buffer; returns full
    // length (write truncated to max), or -1 if the kind is unknown.
    wrap!("env", "host_fw_read_json", |mut caller: Caller<HostState>, kp: i32, kl: i32, op: i32, om: i32| -> i32 {
        let Some(mem) = caller_mem(&caller) else { return -1 };
        let kind = read_str(&mem, &caller, kp, kl);
        let blob = caller.data().fw.get(&kind).map(|s| s.to_string());
        match blob {
            Some(s) => {
                let n = (s.len() as i32).min(om.max(0));
                let _ = mem.write(&mut caller, op as usize, &s.as_bytes()[..n as usize]);
                s.len() as i32
            }
            None => -1,
        }
    });

    Ok(())
}

/// Firmware host-ABI version plugins can probe via `host_fw_abi_version`.
pub const FW_ABI_VERSION: i32 = 1;

/// Capability bits reported by `host_fw_capabilities`.
pub mod caps {
    pub const MSR: u64 = 1 << 0;
    pub const PCI: u64 = 1 << 1;
    pub const SMBUS: u64 = 1 << 2;
    pub const VARIABLES: u64 = 1 << 3;
    pub const NVME: u64 = 1 << 4;
}

/// UEFI RTC as a WASI-style unix nanosecond count (best-effort; 0 if no clock).
fn clock_unix_ns() -> i64 {
    match uefi::runtime::get_time() {
        Ok(t) => {
            // Days-from-civil (Howard Hinnant) → unix seconds, then ns.
            let (y, m, d) = (t.year() as i64, t.month() as i64, t.day() as i64);
            let yy = if m <= 2 { y - 1 } else { y };
            let era = (if yy >= 0 { yy } else { yy - 399 }) / 400;
            let yoe = yy - era * 400;
            let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
            let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
            let days = era * 146097 + doe - 719468;
            let secs = days * 86400 + t.hour() as i64 * 3600 + t.minute() as i64 * 60 + t.second() as i64;
            secs * 1_000_000_000
        }
        Err(_) => 0,
    }
}

fn clock_json() -> String {
    let ns = clock_unix_ns();
    format!("{{\"unix_ms\":{}}}", ns / 1_000_000)
}

/// Read a packed-string export (`() -> i64`), decode and read guest memory.
fn read_packed(
    instance: &Instance,
    store: &mut Store<HostState>,
    mem: &Memory,
    name: &str,
) -> String {
    let Ok(f) = instance.get_typed_func::<(), i64>(&*store, name) else {
        return String::new();
    };
    let Ok(packed) = f.call(&mut *store, ()) else {
        return String::new();
    };
    let packed = packed as u64;
    let ptr = (packed >> 32) as u32 as i32;
    let len = (packed & 0xFFFF_FFFF) as u32 as i32;
    read_str(mem, &*store, ptr, len)
}

/// Load a plugin and read its metadata + tool descriptors. When `call` is
/// `Some((tool, args_json))`, also invoke `handle_mcp_call` and capture the
/// result. Never runs the module's start/`_start`; only the named exports.
pub fn run(
    bytes: &[u8],
    hostname: &str,
    call: Option<(&str, &str)>,
    fw: FwData,
) -> Result<PluginRun, String> {
    if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
        return Err("not a WASM module (bad magic)".into());
    }
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).map_err(|e| format!("compile: {e}"))?;
    let mut store = Store::new(&engine, HostState::new(hostname.to_string(), fw));
    let mut linker = Linker::new(&engine);
    link_host(&mut linker)?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| format!("instantiate: {e}"))?;
    let mem = instance
        .get_memory(&store, "memory")
        .ok_or("plugin exports no `memory`")?;

    let id = read_packed(&instance, &mut store, &mem, "plugin_id");
    let name = read_packed(&instance, &mut store, &mem, "plugin_name");
    let version = read_packed(&instance, &mut store, &mem, "plugin_version");

    if let Ok(f) = instance.get_typed_func::<(), ()>(&store, "on_load") {
        let _ = f.call(&mut store, ());
    }

    let tools = read_packed(&instance, &mut store, &mem, "mcp_tools");

    let mut result = String::new();
    if let Some((tool, args)) = call {
        result = invoke(&instance, &mut store, &mem, tool, args)?;
    }

    if let Ok(f) = instance.get_typed_func::<(), ()>(&store, "on_unload") {
        let _ = f.call(&mut store, ());
    }

    let state = store.into_data();
    Ok(PluginRun {
        id,
        name,
        version,
        tools,
        result,
        log: state.log,
        stdout: String::from_utf8_lossy(&state.stdout).into_owned(),
    })
}

/// Call `handle_mcp_call(tool, args)`: alloc guest buffers, write inputs, read
/// the packed JSON result back out.
fn invoke(
    instance: &Instance,
    store: &mut Store<HostState>,
    mem: &Memory,
    tool: &str,
    args: &str,
) -> Result<String, String> {
    let alloc = instance
        .get_typed_func::<i32, i32>(&*store, "alloc")
        .map_err(|_| "plugin exports no `alloc`")?;
    let handle = instance
        .get_typed_func::<(i32, i32, i32, i32), i64>(&*store, "handle_mcp_call")
        .map_err(|_| "plugin exports no `handle_mcp_call`")?;

    let tool_ptr = alloc.call(&mut *store, tool.len() as i32).map_err(|e| format!("alloc: {e}"))?;
    mem.write(&mut *store, tool_ptr as usize, tool.as_bytes())
        .map_err(|e| format!("write tool: {e}"))?;
    let args_ptr = alloc.call(&mut *store, args.len() as i32).map_err(|e| format!("alloc: {e}"))?;
    mem.write(&mut *store, args_ptr as usize, args.as_bytes())
        .map_err(|e| format!("write args: {e}"))?;

    let packed = handle
        .call(
            &mut *store,
            (tool_ptr, tool.len() as i32, args_ptr, args.len() as i32),
        )
        .map_err(|e| format!("handle_mcp_call trap: {e}"))? as u64;
    let ptr = (packed >> 32) as u32 as i32;
    let len = (packed & 0xFFFF_FFFF) as u32 as i32;
    if len == 0 {
        return Err("plugin returned empty result".into());
    }
    Ok(read_str(mem, &*store, ptr, len))
}
