//! Minimal SDK guest used to measure the wasm32-wasip1 artifact size.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

#[derive(Facet, Deserialize)]
struct EchoArgs {
    /// Message to echo back to the caller.
    message: String,
    /// Optional repeat count.
    repeat: Option<u32>,
}

fn snapshot() -> Result<serde_json::Value, SdkError> {
    let out = host::run_command("echo hello");
    Ok(serde_json::json!({ "tool": "snapshot", "output": out }))
}

fn echo(a: EchoArgs) -> Result<serde_json::Value, SdkError> {
    let n = a.repeat.unwrap_or(1);
    if n == 0 {
        return Err(SdkError::invalid_args("repeat must be >= 1"));
    }
    Ok(serde_json::json!({ "tool": "echo", "message": a.message, "repeat": n }))
}

fn boot() {
    host::log("minimal guest booting");
}

fn ui() -> String {
    String::from("[]")
}

mtech_plugin! {
    id: "com.mastertech.minimal_guest",
    name: "Minimal Guest",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    on_load: boot,
    ui_commands: ui,
    tools: {
        /// Capture a snapshot via a host command.
        snapshot() => snapshot,
        /// Echo a message an optional number of times.
        echo(EchoArgs) => echo,
    }
}
