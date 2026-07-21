//! Runtime dispatch helpers referenced by the macro expansion.

use crate::error::{ErrorCode, SdkError};

/// Trims and single-space-joins doc text.
pub fn normalize_doc(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Re-parses a `Value::String` that looks like JSON, recursing through nested layers.
pub fn lenient_args(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            let looks_like_json = (trimmed.starts_with('{') && trimmed.ends_with('}'))
                || (trimmed.starts_with('[') && trimmed.ends_with(']'));
            if looks_like_json {
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(inner) => lenient_args(inner),
                    Err(_) => serde_json::Value::String(s),
                }
            } else {
                serde_json::Value::String(s)
            }
        }
        other => other,
    }
}

/// Unwraps double-encoded args, then deserializes into the handler's argument type.
pub fn parse_args<A: serde::de::DeserializeOwned>(
    tool: &'static str,
    v: serde_json::Value,
) -> Result<A, SdkError> {
    let v = lenient_args(v);
    serde_json::from_value(v).map_err(|e| SdkError::invalid_args(e.to_string()).with_tool(tool))
}

/// Serializes a handler's success value, or an error envelope on failure.
pub fn ok_json<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_string(v) {
        Ok(s) => s,
        Err(e) => SdkError::new(ErrorCode::Serialize, e.to_string()).to_json(),
    }
}

/// Runs a tool closure, stamping the tool name onto any error envelope.
pub fn run_tool(tool: &'static str, f: impl FnOnce() -> Result<String, SdkError>) -> String {
    match f() {
        Ok(s) => s,
        Err(e) => e.with_tool(tool).to_json(),
    }
}

/// Logs the panic message and location to the host before the trap.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        crate::host::log(&format!("panic at {loc}: {msg}"));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Args {
        published_name: String,
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_doc("  First   line.\n  Second line. "), "First line. Second line.");
    }

    #[test]
    fn lenient_unwraps_double_encoded_string() {
        let v = serde_json::Value::String(r#"{"published_name":"oem1.inf"}"#.to_string());
        let unwrapped = lenient_args(v);
        assert_eq!(unwrapped["published_name"], "oem1.inf");
    }

    #[test]
    fn lenient_leaves_plain_string() {
        let v = serde_json::Value::String("just text".to_string());
        assert_eq!(lenient_args(v), serde_json::Value::String("just text".to_string()));
    }

    #[test]
    fn parse_args_unwraps_double_encoded() {
        let v = serde_json::Value::String(r#"{"published_name":"oem1.inf"}"#.to_string());
        let a: Args = parse_args("export_driver", v).unwrap();
        assert_eq!(a.published_name, "oem1.inf");
    }

    #[test]
    fn parse_args_error_is_invalid_args_envelope() {
        let v = serde_json::json!({ "wrong": 1 });
        let err = parse_args::<Args>("export_driver", v).unwrap_err();
        let j: serde_json::Value = serde_json::from_str(&err.to_json()).unwrap();
        assert_eq!(j["error_code"], "invalid_args");
        assert_eq!(j["tool"], "export_driver");
        assert!(j["error"].as_str().unwrap().contains("published_name"));
    }

    #[test]
    fn run_tool_ok_passthrough() {
        let out = run_tool("t", || Ok(String::from("{\"ok\":true}")));
        assert_eq!(out, "{\"ok\":true}");
    }

    #[test]
    fn run_tool_stamps_error() {
        let out = run_tool("t", || Err(SdkError::internal("boom")));
        let j: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["error_code"], "internal");
        assert_eq!(j["tool"], "t");
    }

    #[test]
    fn ok_json_serializes_value() {
        let out = ok_json(&serde_json::json!({ "tool": "snapshot", "n": 3 }));
        let j: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["tool"], "snapshot");
        assert_eq!(j["n"], 3);
    }

    #[test]
    fn from_serde_error_maps_to_invalid_args() {
        let e: SdkError = serde_json::from_str::<Args>("not json").unwrap_err().into();
        let j: serde_json::Value = serde_json::from_str(&e.to_json()).unwrap();
        assert_eq!(j["error_code"], "invalid_args");
    }
}
