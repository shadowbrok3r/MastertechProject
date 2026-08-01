//! Build small Mastertech WASM plugins **without a nested Cargo crate**.
//!
//! Flow (aligned with [wasmtime’s embedding model](https://docs.rs/wasmtime/latest/wasmtime/)):
//! 1. Emit **WAT** (text) with the Mastertech plugin ABI (`plugin_id`, `alloc`, `handle_mcp_call`, …).
//! 2. **`wat::parse_str`** → canonical **WebAssembly 1.0** module bytes.
//! 3. **`wasmtime::Module::new`** — validates the module the same way the runtime will load it.
//!
//! The **`plugin_emit_clock_wasm`** MCP tool uses this path so agents can produce a **useful** plugin
//! (`current_time` → real UTC via host import `host_fill_clock_json`) without `cargo build` in the
//! plugin directory.

use std::fmt::Write as _;

/// Escape bytes for a WAT `data` segment (`\hh` per byte).
fn wat_data_escape(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 4);
    for b in bytes {
        write!(&mut s, "\\{:02x}", b).unwrap();
    }
    s
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Layout of the active data segment starting at linear memory offset 1024.
struct ClockDataLayout {
    id_off: i32,
    id_len: i32,
    name_off: i32,
    name_len: i32,
    ver_off: i32,
    ver_len: i32,
    tools_off: i32,
    tools_len: i32,
    tool_cmp_off: i32,
    err_off: i32,
    err_len: i32,
    ui_off: i32,
    blob: Vec<u8>,
}

fn build_clock_data(plugin_id: &str, display_name: &str) -> ClockDataLayout {
    const BASE: i32 = 1024;
    let mut rel: usize = 0;
    let mut b = vec![0u8; 2048];

    let id_b = plugin_id.as_bytes();
    let id_len = id_b.len().min(64);
    b[rel..rel + id_len].copy_from_slice(&id_b[..id_len]);
    let id_off = BASE;
    rel += 64;

    let nb = display_name.as_bytes();
    let nl = nb.len().min(64);
    b[rel..rel + nl].copy_from_slice(&nb[..nl]);
    let name_off = BASE + 64;
    rel += 64;

    let ver = b"0.2.0";
    let ver_len = ver.len();
    b[rel..rel + ver_len].copy_from_slice(ver);
    let ver_off = BASE + 128;
    rel += 32;

    let tools = format!(
        r#"[{{"name":"current_time","description":"UTC clock from Mastertech host (unix_ms + iso_utc JSON)","parameters_schema":{{"type":"object","properties":{{}}}}}}]"#
    );
    let tb = tools.as_bytes();
    let tl = tb.len().min(384);
    b[rel..rel + tl].copy_from_slice(&tb[..tl]);
    let tools_off = BASE + 160;
    rel += 384;

    let tn = b"current_time";
    b[rel..rel + 12].copy_from_slice(tn);
    let tool_cmp_off = BASE + rel as i32;
    rel += 16;

    let err = br#"{"error":"unknown_tool"}"#;
    let el = err.len();
    b[rel..rel + el].copy_from_slice(err);
    let err_off = BASE + rel as i32;
    let err_len = el as i32;
    rel += el;

    rel = align4(rel);
    let ui = b"[]";
    b[rel..rel + ui.len()].copy_from_slice(ui);
    let ui_off = BASE + rel as i32;
    rel += ui.len();

    b.truncate(rel);

    ClockDataLayout {
        id_off,
        id_len: id_len as i32,
        name_off,
        name_len: nl as i32,
        ver_off,
        ver_len: ver_len as i32,
        tools_off,
        tools_len: tl as i32,
        tool_cmp_off,
        err_off,
        err_len,
        ui_off,
        blob: b,
    }
}

/// Produce WAT source for the clock plugin (Mastertech WASM ABI + `host_fill_clock_json`).
pub fn clock_plugin_wat(plugin_id: &str, display_name: &str) -> String {
    let layout = build_clock_data(plugin_id, display_name);
    let hex = wat_data_escape(&layout.blob);
    let tpl = include_str!("clock_plugin_template.wat");
    tpl.replace("___DATA___", &hex)
        .replace("__ID_OFF__", &layout.id_off.to_string())
        .replace("__ID_LEN__", &layout.id_len.to_string())
        .replace("__NAME_OFF__", &layout.name_off.to_string())
        .replace("__NAME_LEN__", &layout.name_len.to_string())
        .replace("__VER_OFF__", &layout.ver_off.to_string())
        .replace("__VER_LEN__", &layout.ver_len.to_string())
        .replace("__TOOLS_OFF__", &layout.tools_off.to_string())
        .replace("__TOOLS_LEN__", &layout.tools_len.to_string())
        .replace("__TOOL_CMP_OFF__", &layout.tool_cmp_off.to_string())
        .replace("__ERR_OFF__", &layout.err_off.to_string())
        .replace("__ERR_LEN__", &layout.err_len.to_string())
        .replace("__UI_OFF__", &layout.ui_off.to_string())
}

/// WAT → wasm bytes, then validate with **wasmtime** (`Module::new`).
pub fn wat_to_wasm_validated(wat: &str) -> Result<Vec<u8>, String> {
    let wasm = wat::parse_str(wat).map_err(|e| format!("wat parse: {e}"))?;
    let engine = wasmtime::Engine::default();
    wasmtime::Module::new(&engine, &wasm).map_err(|e| format!("wasmtime Module::new: {e}"))?;
    Ok(wasm)
}

/// Full pipeline for the clock guest.
pub fn clock_plugin_wasm_bytes(plugin_id: &str, display_name: &str) -> Result<Vec<u8>, String> {
    let wat = clock_plugin_wat(plugin_id, display_name);
    wat_to_wasm_validated(&wat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_plugin_wat_parses_and_validates() {
        let wasm = clock_plugin_wasm_bytes("com.mastertech.test-clock", "Test Clock").unwrap();
        assert!(wasm.starts_with(b"\0asm"));
        assert!(wasm.len() > 64);
    }
}
