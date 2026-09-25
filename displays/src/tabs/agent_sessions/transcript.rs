//! Agent session transcript rows, and tool calls read back from the broker's `agent_event` rows.

use std::borrow::Cow;

use chrono::{DateTime, Local, Utc};
use database::schema::{AgentEvent, RecordIdExt};
use eframe::egui::{Id, Ui};
use serde_json::Value;

use crate::ui_tools::chat_bubble::{self, ChatKind, ChatRow, ChatStyle};

/// Longest header summary in characters; the header also truncates to its width.
const SUMMARY_CHARS: usize = 160;
/// Characters of arguments and of result kept in a chat line.
const LINE_ARGS_CHARS: usize = 2_000;
const LINE_DETAIL_CHARS: usize = 4_000;

/// Renders a transcript; shared with the bench-side progress window.
pub fn transcript_ui(ui: &mut Ui, salt: &str, events: &[AgentEvent], show_reasoning: bool) {
    let style = ChatStyle::from_ui(ui);
    let scope = Id::new(("agent_transcript", salt));
    let now = Local::now();
    for ev in events {
        event_row(ui, &style, scope, &now, ev, show_reasoning);
    }
}

fn event_time(ev: &AgentEvent, now: &DateTime<Local>) -> Option<String> {
    let at = ev.created_at.or(ev.updated_at)?;
    Some(chat_bubble::local_clock(DateTime::<Utc>::from(at), now))
}

fn event_row(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    now: &DateTime<Local>,
    ev: &AgentEvent,
    show_reasoning: bool,
) {
    let key = ev.id.key_string();
    let time = event_time(ev, now);
    let text = ev.text.as_str();
    let has_text = !text.trim().is_empty();
    match ev.kind.as_str() {
        "turn_started" => {
            chat_bubble::divider(ui, style, &chat_bubble::turn_title(ev.turn_id.as_deref()))
        }
        "turn_completed" => {
            ui.add_space(2.0);
        }
        "user" => {
            ChatRow::new(ChatKind::User, &key, "Technician")
                .time(time)
                .copy(text)
                .has_body(has_text)
                .show(ui, style, scope, |ui, id| {
                    chat_bubble::markdown(ui, style, text, style.text, id)
                });
        }
        "agent" => {
            ChatRow::new(ChatKind::Agent, &key, "Agent")
                .time(time)
                .copy(text)
                .streaming(!ev.done)
                .has_body(has_text)
                .show(ui, style, scope, |ui, id| {
                    chat_bubble::markdown(ui, style, text, style.text, id)
                });
        }
        "reasoning" => {
            if show_reasoning && has_text {
                ChatRow::new(ChatKind::Reasoning, &key, "Thinking")
                    .time(time)
                    .copy(text)
                    .streaming(!ev.done)
                    .summary(chat_bubble::summary_line(text, SUMMARY_CHARS))
                    .show(ui, style, scope, |ui, id| {
                        chat_bubble::markdown(ui, style, text, style.text, id)
                    });
            }
        }
        "tool_call" => tool_row(ui, style, scope, &key, time, ev),
        "command" => command_row(ui, style, scope, &key, time, ev),
        "file_change" => file_change_row(ui, style, scope, &key, time, ev),
        "approval" => {
            let details = ev.item.as_ref().filter(|i| !i.is_null());
            ChatRow::new(ChatKind::Approval, &key, "Approval")
                .time(time)
                .copy(text)
                .summary(chat_bubble::summary_line(text, SUMMARY_CHARS))
                .show(ui, style, scope, |ui, id| {
                    chat_bubble::markdown(ui, style, text, style.text, id);
                    if let Some(item) = details {
                        chat_bubble::caption(ui, style, "Details");
                        chat_bubble::json(ui, style, item, id.with("details"));
                    }
                });
        }
        "error" => {
            ChatRow::new(ChatKind::Error, &key, "Error")
                .time(time)
                .copy(text)
                .has_body(has_text)
                .show(ui, style, scope, |ui, id| {
                    chat_bubble::markdown(ui, style, text, style.error, id)
                });
        }
        _ => {
            if has_text {
                chat_bubble::notice(ui, style, text, time.as_deref());
            }
        }
    }
}

fn tool_row(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    key: &str,
    time: Option<String>,
    ev: &AgentEvent,
) {
    let call = ToolCall::from_event(ev);
    let mut row = ChatRow::new(ChatKind::Tool, key, chat_bubble::tool_label(call.name))
        .time(time)
        .streaming(call.running)
        .default_open(call.failed)
        .summary(call.summary(ui));
    if call.running {
        row = row.badge("running", style.link);
    } else if call.failed {
        row = row.badge("failed", style.error);
    } else if let Some(ms) = call.duration_ms {
        row = row.badge(chat_bubble::duration_label(ms), style.weak);
    }
    if row.show(ui, style, scope, |ui, id| call.body(ui, style, id)) {
        ui.ctx().copy_text(call.copy_text());
    }
}

fn command_row(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    key: &str,
    time: Option<String>,
    ev: &AgentEvent,
) {
    let field = |k: &str| ev.item.as_ref().and_then(|i| i.get(k));
    let exit = field("exitCode").and_then(Value::as_i64);
    let duration = field("durationMs").and_then(Value::as_u64);
    let (head, output) = ev.text.split_once('\n').unwrap_or((ev.text.as_str(), ""));
    let failed = exit.is_some_and(|c| c != 0);
    let command = head.trim().trim_start_matches("$ ");
    let mut row = ChatRow::new(ChatKind::Command, key, "Shell")
        .time(time)
        .copy(&ev.text)
        .streaming(!ev.done)
        .default_open(failed)
        .summary(chat_bubble::clip(command, SUMMARY_CHARS));
    if let Some(code) = exit.filter(|c| *c != 0) {
        row = row.badge(format!("exit {code}"), style.error);
    } else if let Some(ms) = duration {
        row = row.badge(chat_bubble::duration_label(ms), style.weak);
    }
    row.show(ui, style, scope, |ui, _| {
        if !command.is_empty() {
            chat_bubble::shell_line(ui, style, &format!("$ {command}"));
        }
        if !output.trim().is_empty() {
            chat_bubble::mono_text(ui, style, output.trim_end(), style.text);
        }
    });
}

fn file_change_row(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    key: &str,
    time: Option<String>,
    ev: &AgentEvent,
) {
    let changes: Vec<(&str, &str)> = ev
        .item
        .as_ref()
        .and_then(|i| i.get("changes"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    Some((
                        c.get("path")?.as_str()?,
                        c.get("diff").and_then(Value::as_str).unwrap_or(""),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let summary = if changes.is_empty() {
        chat_bubble::summary_line(&ev.text, SUMMARY_CHARS)
    } else {
        let paths: Vec<&str> = changes.iter().map(|(p, _)| *p).collect();
        chat_bubble::clip(&paths.join(", "), SUMMARY_CHARS).into_owned()
    };
    ChatRow::new(ChatKind::FileChange, key, "File change")
        .time(time)
        .copy(&ev.text)
        .streaming(!ev.done)
        .summary(summary)
        .show(ui, style, scope, |ui, id| {
            if changes.is_empty() {
                chat_bubble::markdown(ui, style, &ev.text, style.text, id);
            }
            for (n, (path, diff)) in changes.iter().enumerate() {
                chat_bubble::caption(ui, style, path);
                if !diff.is_empty() {
                    chat_bubble::code(ui, style, "diff", diff, id.with(("diff", n)));
                }
            }
        });
}

/// A tool call from its stored `dynamicToolCall`, `mcpToolCall` or `functionCallOutput` item, or its row text.
#[derive(Debug, PartialEq)]
pub(crate) struct ToolCall<'a> {
    pub name: &'a str,
    pub arguments: Option<&'a Value>,
    /// Arguments recovered from the row text when no item is stored yet.
    pub raw_arguments: Option<&'a str>,
    item: Option<&'a Value>,
    pub failed: bool,
    pub running: bool,
    pub duration_ms: Option<u64>,
}

impl<'a> ToolCall<'a> {
    pub(crate) fn from_event(ev: &'a AgentEvent) -> Self {
        Self::parse(ev.item.as_ref(), &ev.text, ev.done)
    }

    pub(crate) fn parse(item: Option<&'a Value>, text: &'a str, done: bool) -> Self {
        let (text_name, text_args) = split_call(text);
        let field = |k: &str| item.and_then(|i| i.get(k));
        let name = field("tool")
            .or_else(|| field("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(text_name);
        Self {
            name,
            arguments: field("arguments").filter(|a| !a.is_null()),
            raw_arguments: if item.is_none() { text_args } else { None },
            item,
            failed: item.is_some_and(item_failed),
            running: !done,
            duration_ms: field("durationMs").and_then(Value::as_u64),
        }
    }

    /// What the call returned, joined across its content items.
    pub(crate) fn output(&self) -> Option<Cow<'a, str>> {
        output_text(self.item?)
    }

    /// Why the call failed: the MCP `error`, else the output of a call answered with `success: false`.
    pub(crate) fn error(&self) -> Option<Cow<'a, str>> {
        if !self.failed {
            return None;
        }
        let item = self.item?;
        Some(
            mcp_error(item)
                .or_else(|| output_text(item))
                .unwrap_or(Cow::Borrowed("The tool reported a failure.")),
        )
    }

    fn summary(&self, ui: &Ui) -> String {
        match (self.arguments, self.raw_arguments) {
            (Some(args), _) => chat_bubble::json_summary(args, SUMMARY_CHARS),
            (None, Some(raw)) => chat_bubble::payload_summary(ui.ctx(), raw, SUMMARY_CHARS),
            _ => String::new(),
        }
    }

    fn arguments_text(&self) -> String {
        match (self.arguments, self.raw_arguments) {
            (Some(args), _) => args.to_string(),
            (None, Some(raw)) => raw.to_string(),
            _ => String::new(),
        }
    }

    fn body(&self, ui: &mut Ui, style: &ChatStyle, id: Id) {
        if let Some(args) = self.arguments {
            chat_bubble::caption(ui, style, "Arguments");
            chat_bubble::json(ui, style, args, id.with("args"));
        } else if let Some(raw) = self.raw_arguments.filter(|r| !r.trim().is_empty()) {
            chat_bubble::caption(ui, style, "Arguments");
            chat_bubble::payload(ui, style, raw, style.text, id.with("args"));
        }
        if let Some(error) = self.error() {
            chat_bubble::caption(ui, style, "Error");
            chat_bubble::payload(ui, style, &error, style.error, id.with("error"));
        } else if let Some(output) = self.output() {
            chat_bubble::caption(ui, style, "Result");
            chat_bubble::payload(ui, style, &output, style.text, id.with("result"));
        } else if self.running {
            chat_bubble::caption(ui, style, "Waiting for the tool to finish.");
        }
    }

    fn copy_text(&self) -> String {
        let mut out = format!("{}({})", self.name, self.arguments_text());
        if let Some(error) = self.error() {
            out.push_str("\n\nerror: ");
            out.push_str(&error);
        } else if let Some(output) = self.output() {
            out.push_str("\n\n");
            out.push_str(&output);
        }
        out
    }
}

/// True when a stored tool item records a failure in any of the fields codex uses for one.
fn item_failed(item: &Value) -> bool {
    item.get("error").is_some_and(|e| !e.is_null())
        || item.get("success").and_then(Value::as_bool) == Some(false)
        || item
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("failed"))
}

fn mcp_error(item: &Value) -> Option<Cow<'_, str>> {
    let error = item.get("error").filter(|e| !e.is_null())?;
    Some(
        match error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
        {
            Some(message) => Cow::Borrowed(message),
            None => Cow::Owned(error.to_string()),
        },
    )
}

/// Text parts of a stored result: `contentItems[].text`, `result.content[].text`, or `output`.
fn output_text(item: &Value) -> Option<Cow<'_, str>> {
    let parts: Vec<&str> = if let Some(items) = item.get("contentItems").and_then(Value::as_array) {
        items
            .iter()
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .collect()
    } else if let Some(content) = item.pointer("/result/content").and_then(Value::as_array) {
        content
            .iter()
            .filter_map(|c| match c.get("type").and_then(Value::as_str) {
                Some("image") => Some("[image]"),
                _ => c.get("text").and_then(Value::as_str),
            })
            .collect()
    } else {
        item.get("output")
            .and_then(Value::as_str)
            .into_iter()
            .collect()
    };
    let text = match parts.as_slice() {
        [] => return None,
        [only] => Cow::Borrowed(*only),
        many => Cow::Owned(many.join("\n")),
    };
    (!text.trim().is_empty()).then_some(text)
}

/// The name and the text inside the outer parentheses of a stored `name(arguments)` line.
fn split_call(text: &str) -> (&str, Option<&str>) {
    let head = text.lines().next().unwrap_or("").trim();
    match head.find('(') {
        Some(open) => {
            let inner = &head[open + 1..];
            (
                head[..open].trim(),
                Some(inner.strip_suffix(')').unwrap_or(inner)),
            )
        }
        None => (head, None),
    }
}

/// The AI chat's `name (arguments) status` line for a tool or shell row, then its result or error.
pub(crate) fn chat_line(ev: &AgentEvent) -> Option<String> {
    match ev.kind.as_str() {
        "tool_call" => {
            let call = ToolCall::from_event(ev);
            let status = if call.failed {
                "failed".to_string()
            } else {
                call.duration_ms
                    .map(chat_bubble::duration_label)
                    .unwrap_or_default()
            };
            let detail = call.error().or_else(|| call.output()).unwrap_or_default();
            Some(line(call.name, &call.arguments_text(), &status, &detail))
        }
        "command" => {
            let (head, rest) = ev.text.split_once('\n').unwrap_or((ev.text.as_str(), ""));
            let field = |k: &str| ev.item.as_ref().and_then(|i| i.get(k));
            let status = match field("exitCode").and_then(Value::as_i64) {
                Some(code) if code != 0 => format!("exit {code}"),
                _ => field("durationMs")
                    .and_then(Value::as_u64)
                    .map(chat_bubble::duration_label)
                    .unwrap_or_default(),
            };
            Some(line("shell", head.trim(), &status, rest))
        }
        _ => None,
    }
}

fn line(name: &str, args: &str, status: &str, detail: &str) -> String {
    let mut out = format!("{name} ({})", chat_bubble::clip(args, LINE_ARGS_CHARS));
    if !status.is_empty() {
        out.push(' ');
        out.push_str(status);
    }
    let detail = detail.trim();
    if !detail.is_empty() {
        out.push('\n');
        out.push_str(&chat_bubble::clip(detail, LINE_DETAIL_CHARS));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::{Datetime, RecordId};
    use serde_json::json;

    fn event(kind: &str, text: &str, done: bool, item: Option<Value>) -> AgentEvent {
        keyed_event("i1", kind, text, done, item)
    }

    fn keyed_event(
        key: &str,
        kind: &str,
        text: &str,
        done: bool,
        item: Option<Value>,
    ) -> AgentEvent {
        AgentEvent {
            id: RecordId::new("agent_event", format!("t1:{key}").as_str()),
            thread: RecordId::new("agent_thread", "t1"),
            seq: 1,
            turn_id: Some("t1".into()),
            item_id: Some(key.into()),
            kind: kind.into(),
            text: text.into(),
            done,
            item,
            created_at: Some(Datetime::from_timestamp(1_790_000_000, 0).expect("valid time")),
            updated_at: None,
        }
    }

    /// Draws `events` in a 420 px viewport and returns the size the content took.
    fn render(ctx: &eframe::egui::Context, events: &[AgentEvent]) -> eframe::egui::Vec2 {
        use eframe::egui::{RawInput, Rect, ScrollArea, pos2, vec2};
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 900.0))),
            ..Default::default()
        };
        let mut size = vec2(0.0, 0.0);
        let mut out = ctx.run_ui(input, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                transcript_ui(ui, "test", events, true);
                size = ui.min_rect().size();
            });
        });
        out.textures_delta.clear();
        size
    }

    #[test]
    fn every_row_kind_draws_open_and_closed_inside_the_viewport() {
        let long = "x".repeat(3_000);
        let agent = format!(
            "# Result\n\nSome **bold** `code` [link](https://x.y)\n```rust\nfn main() {{}}\n```\n```bash\nls -la | grep x\n```\n```json\n{{\"a\": [1, {{\"b\": \"{long}\"}}]}}\n```\n{long}\n> quote\n1. one\n   - nested\n---\n$ echo {long}"
        );
        let events = vec![
            keyed_event("a", "turn_started", "", true, None),
            keyed_event("b", "user", "Check **disk** health on `PC-1`.", true, None),
            keyed_event("c", "reasoning", "## Plan\n- read SMART", false, None),
            keyed_event(
                "d",
                "tool_call",
                "get_client_info({})",
                true,
                Some(
                    json!({"tool": "get_client_info", "arguments": {"script": long}, "success": true, "durationMs": 40,
                    "contentItems": [{"type": "inputText", "text": format!("{{\"k\":\"{long}\"}}")}]}),
                ),
            ),
            keyed_event(
                "e",
                "tool_call",
                "run_script({})",
                true,
                Some(json!({"tool": "run_script", "success": false,
                "contentItems": [{"type": "inputText", "text": format!("failed: {long}")}]})),
            ),
            keyed_event("f", "tool_call", "query_surrealdb({\"q\":1})", false, None),
            keyed_event(
                "g",
                "command",
                &format!("$ ls {long}\n{long}"),
                true,
                Some(json!({"exitCode": 1})),
            ),
            keyed_event(
                "h",
                "file_change",
                "file changes",
                true,
                Some(json!({"changes": [{"path": "a.rs", "diff": format!("+{long}")}]})),
            ),
            keyed_event(
                "i",
                "approval",
                "Waiting for a technician to approve: run x",
                true,
                Some(json!({"arguments": {"a": 1}})),
            ),
            keyed_event("j", "agent", &agent, false, None),
            keyed_event("k", "error", "Agent error: boom", true, None),
            keyed_event("l", "other", "Reconnected to the agent host.", true, None),
            keyed_event("m", "turn_completed", "", true, None),
        ];
        let ctx = eframe::egui::Context::default();
        let closed = render(&ctx, &events);
        let scope = Id::new(("agent_transcript", "test"));
        ctx.data_mut(|d| {
            for ev in &events {
                d.insert_temp(scope.with(ev.id.key_string()).with("open"), true);
            }
        });
        let open = render(&ctx, &events);
        let settled = render(&ctx, &events);
        for size in [closed, open, settled] {
            assert!(
                (300.0..=421.0).contains(&size.x),
                "content {} px wide in a 420 px viewport",
                size.x
            );
        }
        assert!(
            open.y > closed.y + 400.0,
            "opening every row grew the transcript from {} to {} px",
            closed.y,
            open.y
        );
    }

    #[test]
    fn a_broker_tool_call_reads_its_result_from_content_items() {
        let item = json!({
            "type": "dynamicToolCall", "tool": "get_client_info", "arguments": {"connection_string": "PC-1"},
            "status": "completed", "success": true, "durationMs": 1234,
            "contentItems": [{"type": "inputText", "text": "{\"hostname\":\"PC-1\"}"}]
        });
        let ev = event(
            "tool_call",
            "get_client_info({\"connection_string\":\"PC-1\"})",
            true,
            Some(item),
        );
        let call = ToolCall::from_event(&ev);
        assert_eq!(call.name, "get_client_info");
        assert!(!call.failed && !call.running);
        assert_eq!(call.duration_ms, Some(1234));
        assert_eq!(call.arguments, Some(&json!({"connection_string": "PC-1"})));
        assert_eq!(call.output().as_deref(), Some("{\"hostname\":\"PC-1\"}"));
        assert_eq!(call.error(), None);
    }

    #[test]
    fn a_broker_failure_is_the_output_of_an_unsuccessful_call() {
        let item = json!({
            "type": "dynamicToolCall", "tool": "run_script", "arguments": {}, "status": "failed", "success": false,
            "contentItems": [{"type": "inputText", "text": "Declined by the technician."}]
        });
        let call = ToolCall::parse(Some(&item), "run_script({})", true);
        assert!(call.failed);
        assert_eq!(call.error().as_deref(), Some("Declined by the technician."));
        assert!(
            call.copy_text()
                .ends_with("error: Declined by the technician.")
        );
    }

    #[test]
    fn mcp_items_still_read_their_result_and_error() {
        let ok = json!({"tool": "q", "result": {"content": [{"type": "text", "text": "a"}, {"type": "image", "data": "x"}, {"type": "text", "text": "b"}]}});
        assert_eq!(
            ToolCall::parse(Some(&ok), "", true).output().as_deref(),
            Some("a\n[image]\nb")
        );
        let bad = json!({"tool": "q", "error": {"message": "boom"}});
        let call = ToolCall::parse(Some(&bad), "", true);
        assert!(call.failed);
        assert_eq!(call.error().as_deref(), Some("boom"));
    }

    #[test]
    fn a_running_call_is_named_from_its_row_text() {
        let call = ToolCall::parse(None, "query_surrealdb({\"query\":\"SELECT 1\"})", false);
        assert_eq!(call.name, "query_surrealdb");
        assert_eq!(call.raw_arguments, Some("{\"query\":\"SELECT 1\"}"));
        assert!(call.running && !call.failed);
        assert_eq!(call.output(), None);
        assert_eq!(
            split_call("tool({\"a\":\"cut…)"),
            ("tool", Some("{\"a\":\"cut…"))
        );
        assert_eq!(split_call("bare"), ("bare", None));
    }

    #[test]
    fn chat_lines_carry_the_status_and_the_result() {
        let item = json!({"tool": "t", "arguments": {"a": 1}, "success": false, "contentItems": [{"type": "inputText", "text": "nope"}]});
        let ev = event("tool_call", "t({\"a\":1})", true, Some(item));
        assert_eq!(
            chat_line(&ev).as_deref(),
            Some("t ({\"a\":1}) failed\nnope")
        );
        let cmd = event(
            "command",
            "$ ls\nfile.txt\n",
            true,
            Some(json!({"exitCode": 2})),
        );
        assert_eq!(
            chat_line(&cmd).as_deref(),
            Some("shell ($ ls) exit 2\nfile.txt")
        );
        assert_eq!(chat_line(&event("agent", "hi", true, None)), None);
    }
}
