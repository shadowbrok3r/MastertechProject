//! Newline/one-per-frame JSON exchanged with the Mastertech master over the relay room.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One inbound command with an optional correlation id.
#[derive(Debug, Deserialize)]
pub struct Envelope {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(flatten)]
    pub request: Request,
}

/// A command from the master.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    Arm,
    Disarm,
    Type { text: String },
    Key { chord: String },
    MouseMove { x: i32, y: i32 },
    Click {
        #[serde(default)]
        button: Button,
    },
    Combo { steps: Vec<Step> },
    ReleaseAll,
}

/// One step of a macro sequence.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    Type { text: String },
    Key { chord: String },
    MouseMove { x: i32, y: i32 },
    Click {
        #[serde(default)]
        button: Button,
    },
    Delay { ms: u32 },
    ReleaseAll,
}

/// Mouse button for a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

/// A reply to one [`Envelope`], echoing its id.
#[derive(Debug, Serialize)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: Option<String>, result: Value) -> Self {
        Self { id, ok: true, result: Some(result), error: None }
    }

    pub fn err(id: Option<String>, error: impl Into<String>) -> Self {
        Self { id, ok: false, result: None, error: Some(error.into()) }
    }

    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"ok":false,"error":"response serialize failed"}"#.to_string()
        })
    }
}

/// Relay control notices forwarded on the room channel, told apart from JSON commands.
pub fn is_relay_control(text: &str) -> bool {
    !text.trim_start().starts_with('{')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Envelope {
        serde_json::from_str(line).expect("parses")
    }

    #[test]
    fn ping_and_status_carry_an_id() {
        let e = parse(r#"{"id":"7","cmd":"ping"}"#);
        assert_eq!(e.id.as_deref(), Some("7"));
        assert_eq!(e.request, Request::Ping);
        assert_eq!(parse(r#"{"cmd":"status"}"#).request, Request::Status);
    }

    #[test]
    fn arm_disarm_release_parse() {
        assert_eq!(parse(r#"{"cmd":"arm"}"#).request, Request::Arm);
        assert_eq!(parse(r#"{"cmd":"disarm"}"#).request, Request::Disarm);
        assert_eq!(parse(r#"{"cmd":"release_all"}"#).request, Request::ReleaseAll);
    }

    #[test]
    fn typing_and_keys() {
        assert_eq!(parse(r#"{"cmd":"type","text":"hello"}"#).request, Request::Type { text: "hello".into() });
        assert_eq!(parse(r#"{"cmd":"key","chord":"ctrl+alt+del"}"#).request, Request::Key { chord: "ctrl+alt+del".into() });
    }

    #[test]
    fn mouse_and_click_default_left() {
        assert_eq!(parse(r#"{"cmd":"mouse_move","x":100,"y":-5}"#).request, Request::MouseMove { x: 100, y: -5 });
        assert_eq!(parse(r#"{"cmd":"click"}"#).request, Request::Click { button: Button::Left });
        assert_eq!(parse(r#"{"cmd":"click","button":"right"}"#).request, Request::Click { button: Button::Right });
    }

    #[test]
    fn combo_holds_ordered_steps() {
        let e = parse(r#"{"id":"m1","cmd":"combo","steps":[{"op":"key","chord":"F2"},{"op":"delay","ms":50},{"op":"type","text":"BIOS"},{"op":"release_all"}]}"#);
        let Request::Combo { steps } = e.request else { panic!("combo") };
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0], Step::Key { chord: "F2".into() });
        assert_eq!(steps[1], Step::Delay { ms: 50 });
    }

    #[test]
    fn responses_omit_empty_fields_and_echo_id() {
        assert_eq!(Response::ok(Some("7".into()), serde_json::json!({"pong":true})).to_line(), r#"{"id":"7","ok":true,"result":{"pong":true}}"#);
        assert_eq!(Response::err(None, "disarmed").to_line(), r#"{"ok":false,"error":"disarmed"}"#);
    }

    #[test]
    fn relay_control_lines_are_not_json_commands() {
        assert!(is_relay_control("MASTER_CONNECTED"));
        assert!(is_relay_control("NO_AGENT_IN_ROOM"));
        assert!(!is_relay_control(r#"{"cmd":"ping"}"#));
        assert!(!is_relay_control("  {\"cmd\":\"ping\"}"));
    }
}
