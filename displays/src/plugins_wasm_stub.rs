//! WASM build: the desktop plugin host, MCP bridge, and wasmtime-backed tooling
//! are not compiled. Shared UI code still calls into a small surface — provide
//! no-op hooks so `trunk`/wasm32 builds succeed.

use eframe::egui::Rect;

#[inline]
pub fn push_widget_anchor(_key: impl Into<String>, _rect: Rect) {}

pub mod mcp_bridge {
    #[inline]
    pub fn notify_remote_script_log(_msg: String) {}

    #[inline]
    pub fn notify_remote_script_result(_name: String, _status: String) {}

    #[inline]
    pub fn notify_remote_scripts_complete() {}
}
