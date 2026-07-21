//! Safe wrappers over the host-provided `"env"` imports.
//!
//! Native builds use stubs so the rest of the SDK is unit-testable off-target.

const DEFAULT_CMD_CAP: usize = 1024 * 1024;
const SMALL_BUF: usize = 256;

#[cfg(target_arch = "wasm32")]
mod imports {
    unsafe extern "C" {
        pub fn host_log(ptr: i32, len: i32);
        pub fn host_emit_event(ptr: i32, len: i32);
        pub fn host_repaint();
        pub fn host_fill_clock_json(ptr: i32, max_len: i32) -> i32;
        pub fn host_get_hostname(ptr: i32, max_len: i32) -> i32;
        pub fn host_run_command(cmd_ptr: i32, cmd_len: i32, out_ptr: i32, out_max: i32) -> i32;
        pub fn host_ui_log(ptr: i32, len: i32);
        pub fn host_ui_clear();
    }
}

#[cfg(target_arch = "wasm32")]
pub fn log(msg: &str) {
    unsafe { imports::host_log(msg.as_ptr() as i32, msg.len() as i32) }
}

/// Runs a host shell command with a 1 MiB output cap.
#[cfg(target_arch = "wasm32")]
pub fn run_command(cmd: &str) -> String {
    run_command_capped(cmd, DEFAULT_CMD_CAP)
}

/// Runs a host shell command with an explicit output cap; scratch is plain `Vec`.
#[cfg(target_arch = "wasm32")]
pub fn run_command_capped(cmd: &str, cap: usize) -> String {
    let mut buf = vec![0u8; cap];
    let n = unsafe {
        imports::host_run_command(
            cmd.as_ptr() as i32,
            cmd.len() as i32,
            buf.as_mut_ptr() as i32,
            buf.len() as i32,
        )
    };
    if n <= 0 {
        return String::new();
    }
    buf.truncate(n as usize);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(target_arch = "wasm32")]
pub fn hostname() -> String {
    let mut buf = [0u8; SMALL_BUF];
    let n = unsafe { imports::host_get_hostname(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}

#[cfg(target_arch = "wasm32")]
pub fn clock_json() -> String {
    let mut buf = [0u8; SMALL_BUF];
    let n = unsafe { imports::host_fill_clock_json(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}

#[cfg(target_arch = "wasm32")]
pub fn emit_event(json: &str) {
    unsafe { imports::host_emit_event(json.as_ptr() as i32, json.len() as i32) }
}

#[cfg(target_arch = "wasm32")]
pub fn repaint() {
    unsafe { imports::host_repaint() }
}

#[cfg(target_arch = "wasm32")]
pub fn ui_clear() {
    unsafe { imports::host_ui_clear() }
}

#[cfg(target_arch = "wasm32")]
fn ui_log_str(json: &str) {
    unsafe { imports::host_ui_log(json.as_ptr() as i32, json.len() as i32) }
}

// ── Native stubs ────────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub fn log(msg: &str) {
    eprintln!("[host_log] {msg}");
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_command(_cmd: &str) -> String {
    String::new()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_command_capped(_cmd: &str, _cap: usize) -> String {
    let _ = DEFAULT_CMD_CAP;
    String::new()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn hostname() -> String {
    let _ = SMALL_BUF;
    String::from("test-host")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn clock_json() -> String {
    String::from(r#"{"unix_ms":0,"iso_utc":"1970-01-01T00:00:00.000Z"}"#)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn emit_event(_json: &str) {}

#[cfg(not(target_arch = "wasm32"))]
pub fn repaint() {}

#[cfg(not(target_arch = "wasm32"))]
pub fn ui_clear() {
    eprintln!("[ui_clear]");
}

#[cfg(not(target_arch = "wasm32"))]
fn ui_log_str(json: &str) {
    eprintln!("[ui_log] {json}");
}

// ── Cross-target generic wrappers ────────────────────────────────────────────

pub fn emit_event_value<T: serde::Serialize>(v: &T) {
    if let Ok(s) = serde_json::to_string(v) {
        emit_event(&s);
    }
}

pub fn ui_log<T: serde::Serialize>(entry: &T) {
    if let Ok(s) = serde_json::to_string(entry) {
        ui_log_str(&s);
    }
}
