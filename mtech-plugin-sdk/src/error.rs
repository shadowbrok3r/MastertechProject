//! Error envelope shared by every SDK code path.

use crate::schema::json_escape;

/// Machine-readable error category emitted as `"error_code"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidArgs,
    NotFound,
    HostCommandFailed,
    Serialize,
    Internal,
}

impl ErrorCode {
    /// Stable snake_case wire string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::InvalidArgs => "invalid_args",
            ErrorCode::NotFound => "not_found",
            ErrorCode::HostCommandFailed => "host_command_failed",
            ErrorCode::Serialize => "serialize",
            ErrorCode::Internal => "internal",
        }
    }
}

/// A tool error carrying a category, message, and optional originating tool.
#[derive(Debug, Clone)]
pub struct SdkError {
    code: ErrorCode,
    message: String,
    tool: Option<String>,
}

impl SdkError {
    pub fn new(code: ErrorCode, msg: impl Into<String>) -> Self {
        Self { code, message: msg.into(), tool: None }
    }

    pub fn invalid_args(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgs, msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }

    pub fn host_failed(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::HostCommandFailed, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    /// Serializes to `{"error":..,"error_code":..[,"tool":..]}`.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"error\":\"");
        json_escape(&mut out, &self.message);
        out.push_str("\",\"error_code\":\"");
        out.push_str(self.code.as_str());
        out.push('"');
        if let Some(t) = &self.tool {
            out.push_str(",\"tool\":\"");
            json_escape(&mut out, t);
            out.push('"');
        }
        out.push('}');
        out
    }
}

impl std::fmt::Display for SdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for SdkError {}

impl From<serde_json::Error> for SdkError {
    fn from(e: serde_json::Error) -> Self {
        SdkError::invalid_args(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_strings() {
        assert_eq!(ErrorCode::InvalidArgs.as_str(), "invalid_args");
        assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
        assert_eq!(ErrorCode::HostCommandFailed.as_str(), "host_command_failed");
        assert_eq!(ErrorCode::Serialize.as_str(), "serialize");
        assert_eq!(ErrorCode::Internal.as_str(), "internal");
    }

    #[test]
    fn to_json_is_valid_with_special_chars() {
        let e = SdkError::invalid_args("bad \"quote\"\nand newline").with_tool("t");
        let v: serde_json::Value = serde_json::from_str(&e.to_json()).unwrap();
        assert_eq!(v["error"], "bad \"quote\"\nand newline");
        assert_eq!(v["error_code"], "invalid_args");
        assert_eq!(v["tool"], "t");
    }

    #[test]
    fn to_json_omits_tool_when_absent() {
        let e = SdkError::not_found("nope");
        let v: serde_json::Value = serde_json::from_str(&e.to_json()).unwrap();
        assert!(v.get("tool").is_none());
    }
}
