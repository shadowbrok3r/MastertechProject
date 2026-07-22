//! Structured warnings attached to MCP tool results.
//!
//! A warning tells the calling AI what data gap survived the call and names
//! the exact follow-up tool call that fills it. Emitted only when true — a
//! clean call carries no `warnings` array at all. Hard failures stay errors;
//! warnings are for calls that succeeded but left completeness on the table.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ToolWarning {
    /// Stable machine-checkable code, e.g. `no_open_session`.
    pub code: &'static str,
    /// `info` | `warn`.
    pub severity: &'static str,
    pub message: String,
    /// Exact follow-up call that resolves the warning, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl ToolWarning {
    pub fn info(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: "info",
            message: message.into(),
            fix: None,
        }
    }

    pub fn warn(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: "warn",
            message: message.into(),
            fix: None,
        }
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

/// Insert a `warnings` array into a tool-result object when any exist.
pub fn attach_warnings(
    mut value: serde_json::Value,
    warnings: Vec<ToolWarning>,
) -> serde_json::Value {
    if warnings.is_empty() {
        return value;
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "warnings".to_string(),
            serde_json::to_value(warnings).unwrap_or_default(),
        );
    }
    value
}
