//! Native compile + dispatch check for a full `mtech_plugin!` invocation.

use facet::Facet;
use mtech_plugin_sdk::{mtech_plugin, SdkError};
use serde::Deserialize;

#[derive(Facet, Deserialize)]
struct EchoArgs {
    /// Text to echo back.
    text: String,
}

fn echo(a: EchoArgs) -> Result<serde_json::Value, SdkError> {
    Ok(serde_json::json!({ "tool": "echo", "echoed": a.text }))
}

fn ping() -> Result<serde_json::Value, SdkError> {
    Ok(serde_json::json!({ "tool": "ping", "pong": true }))
}

mtech_plugin! {
    id: "com.example.macro_test",
    name: "Macro Test",
    version: "0.1.0",
    tools: {
        /// Echo the provided text.
        echo(EchoArgs) => echo,
        /// No-argument ping.
        ping() => ping,
    }
}

#[test]
fn tools_json_lists_both_tools() {
    let ts = __mtech_tools();
    let json = ts.to_tools_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "echo");
    assert_eq!(arr[0]["description"], "Echo the provided text.");
    assert_eq!(arr[0]["parameters_schema"]["properties"]["text"]["type"], "string");
    assert_eq!(arr[1]["name"], "ping");
    assert_eq!(arr[1]["parameters_schema"], serde_json::json!({ "type": "object", "properties": {} }));
}

#[test]
fn dispatch_typed_ok() {
    let out = __mtech_dispatch("echo", serde_json::json!({ "text": "hi" }));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["echoed"], "hi");
}

#[test]
fn dispatch_no_arg_ok() {
    let out = __mtech_dispatch("ping", serde_json::Value::Null);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["pong"], true);
}

#[test]
fn dispatch_double_encoded_args() {
    let out = __mtech_dispatch("echo", serde_json::Value::String(r#"{"text":"wrapped"}"#.to_string()));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["echoed"], "wrapped");
}

#[test]
fn dispatch_unknown_tool() {
    let out = __mtech_dispatch("nope", serde_json::Value::Null);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error_code"], "not_found");
    assert_eq!(v["tool"], "nope");
}

#[test]
fn dispatch_invalid_args() {
    let out = __mtech_dispatch("echo", serde_json::json!({ "wrong": 1 }));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error_code"], "invalid_args");
    assert_eq!(v["tool"], "echo");
}
