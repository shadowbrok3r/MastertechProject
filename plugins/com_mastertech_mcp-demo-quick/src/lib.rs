//! Minimal Mastertech WASM plugin for MCP round-trip.

const BUF: usize = 65536;
static mut HEAP: [u8; BUF] = [0; BUF];
static mut HEAP_POS: usize = 0;

fn align_up(pos: usize, align: usize) -> usize {
    (pos + align - 1) & !(align - 1)
}

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe {
        let size = size as usize;
        if size == 0 {
            return 1;
        }
        let p = align_up(HEAP_POS, 16);
        if p + size > BUF {
            return 0;
        }
        HEAP_POS = p + size;
        HEAP.as_mut_ptr().add(p) as i32
    }
}

#[no_mangle]
pub extern "C" fn dealloc(_ptr: i32, _size: i32) {}

fn leak_bytes(slice: &[u8]) -> u64 {
    unsafe {
        let len = slice.len() as i32;
        let ptr = alloc(len);
        if ptr == 0 {
            return 0;
        }
        std::ptr::copy_nonoverlapping(slice.as_ptr(), ptr as *mut u8, slice.len());
        ((ptr as u64) << 32) | (len as u64 & 0xffff_ffff)
    }
}

#[no_mangle]
pub extern "C" fn plugin_id() -> u64 {
    leak_bytes(b"com.mastertech.mcp-demo-quick")
}

#[no_mangle]
pub extern "C" fn plugin_name() -> u64 {
    leak_bytes(b"MCP Demo Quick")
}

#[no_mangle]
pub extern "C" fn plugin_version() -> u64 {
    leak_bytes(b"0.1.0")
}

#[no_mangle]
pub extern "C" fn on_load() {}

#[no_mangle]
pub extern "C" fn on_unload() {}

#[no_mangle]
pub extern "C" fn logic() {}

#[no_mangle]
pub extern "C" fn ui_commands() -> u64 {
    leak_bytes(b"[]")
}

#[no_mangle]
pub extern "C" fn mcp_tools() -> u64 {
    leak_bytes(
        br#"[{"name":"hello","description":"Returns pong","parameters_schema":{"type":"object","properties":{}}}]"#,
    )
}

#[no_mangle]
pub extern "C" fn handle_mcp_call(
    tool_ptr: i32,
    tool_len: i32,
    _args_ptr: i32,
    _args_len: i32,
) -> u64 {
    if tool_len <= 0 || tool_ptr <= 0 {
        return leak_bytes(br#"{"error":"bad tool"}"#);
    }
    let tool = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            tool_ptr as *const u8,
            tool_len as usize,
        ))
        .unwrap_or("")
    };
    if tool == "hello" {
        return leak_bytes(br#"{"message":"pong from wasm"}"#);
    }
    leak_bytes(br#"{"error":"unknown tool"}"#)
}
