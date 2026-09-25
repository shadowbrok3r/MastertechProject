//! `agent_event` rows as terminal transcript rows, like the desktop agent chat:
//! a header per row in its kind's colour with the time, folding tool, shell,
//! file-change, approval and thinking rows, and markdown bodies.

use std::collections::HashMap;
use std::rc::Rc;

use chrono::{DateTime, Local, NaiveDate, Utc};
use database::schema::{AgentEvent, RecordId, RecordIdExt};
use displays::tabs::agent_sessions::ToolCall;
use displays::ui_tools::chat_bubble::markdown::{as_json, split_json};
use displays::ui_tools::chat_bubble::{self as chat, ChatKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use super::{json, markdown, wrap};
use crate::terminal_mode::styling::{AppTheme, THEME, glyphs};

/// Longest header summary in characters; the header is also cut to the pane.
const SUMMARY_CHARS: usize = 160;
/// Longest monospace payload shown in a row body.
const MONO_MAX_CHARS: usize = 16_000;
/// Columns a message body is inset by.
const BODY_INDENT: usize = 2;
/// Narrowest width rows are laid out for.
const MIN_WIDTH: usize = 16;

/// How rows are drawn this frame.
#[derive(Clone, Copy)]
pub struct Options {
    pub width: usize,
    pub show_reasoning: bool,
    /// Unfolds every tool, shell, file-change and approval row.
    pub expand: bool,
    pub spinner: &'static str,
}

/// One rendered transcript row.
pub struct Row {
    /// Record key of the event.
    pub key: String,
    /// Open state of a folding row; `None` for rows that do not fold.
    pub fold: Option<bool>,
    /// Kept apart from neighbouring rows by blank lines.
    pub gap: bool,
    pub lines: Vec<Line<'static>>,
}

/// A cached row and the width and open state it was drawn for.
struct Cached {
    width: usize,
    fold: Option<bool>,
    row: Rc<Row>,
}

/// Rows of finished events, redrawn when the event, the width, the open state, the day or the theme changes.
#[derive(Default)]
pub struct Transcript {
    cache: HashMap<RecordId, Cached>,
    day: Option<NaiveDate>,
    theme: Option<AppTheme>,
}

impl Transcript {
    pub fn forget(&mut self, id: &RecordId) {
        self.cache.remove(id);
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Rows for `events`; `toggled` holds the open state of rows the technician clicked.
    pub fn rows(
        &mut self,
        events: &[AgentEvent],
        opts: &Options,
        toggled: &HashMap<String, bool>,
    ) -> Vec<Rc<Row>> {
        let now = Local::now();
        let theme = *THEME;
        if self.day != Some(now.date_naive()) || self.theme != Some(theme) {
            self.cache.clear();
            self.day = Some(now.date_naive());
            self.theme = Some(theme);
        }
        events
            .iter()
            .filter_map(|ev| self.row(ev, opts, toggled, &now))
            .collect()
    }

    fn row(
        &mut self,
        ev: &AgentEvent,
        opts: &Options,
        toggled: &HashMap<String, bool>,
        now: &DateTime<Local>,
    ) -> Option<Rc<Row>> {
        let key = ev.id.key_string();
        let fold = fold_state(ev, opts, toggled.get(&key).copied());
        if let Some(cached) = self
            .cache
            .get(&ev.id)
            .filter(|c| ev.done && c.width == opts.width && c.fold == fold)
        {
            return Some(cached.row.clone());
        }
        let row = Rc::new(render(ev, key, fold, opts, now)?);
        if ev.done {
            let cached = Cached {
                width: opts.width,
                fold,
                row: row.clone(),
            };
            self.cache.insert(ev.id.clone(), cached);
        }
        Some(row)
    }
}

/// One transcript line, the row it belongs to and its index within that row.
pub struct Slot<'r> {
    pub line: &'r Line<'static>,
    pub row: Option<&'r Row>,
    pub index: usize,
}

impl Slot<'_> {
    /// The row's header line.
    pub fn is_head(&self) -> bool {
        self.row.is_some() && self.index == 0
    }

    /// True for line `index` of the row keyed `key`.
    pub fn is(&self, key: &str, index: usize) -> bool {
        self.index == index && self.row.is_some_and(|r| r.key == key)
    }
}

/// Rows as consecutive lines, blank lines around rows that keep a gap, then `tail`.
pub fn flatten<'r>(
    rows: &'r [Rc<Row>],
    tail: &'r [Line<'static>],
    blank: &'r Line<'static>,
) -> Vec<Slot<'r>> {
    let loose = |line| Slot {
        line,
        row: None,
        index: 0,
    };
    let mut out = Vec::new();
    let mut spaced = true;
    for row in rows {
        if row.gap && !spaced {
            out.push(loose(blank));
        }
        out.extend(row.lines.iter().enumerate().map(|(index, line)| Slot {
            line,
            row: Some(row.as_ref()),
            index,
        }));
        spaced = row.gap;
        if row.gap {
            out.push(loose(blank));
        }
    }
    if !tail.is_empty() && !spaced {
        out.push(loose(blank));
    }
    out.extend(tail.iter().map(loose));
    out
}

/// Open state of a folding row: the technician's toggle, else Ctrl+T for thinking and Ctrl+O or a failure for the rest.
fn fold_state(ev: &AgentEvent, opts: &Options, toggled: Option<bool>) -> Option<bool> {
    let kind = kind_of(&ev.kind).filter(|k| k.collapsible())?;
    Some(toggled.unwrap_or_else(|| match kind {
        ChatKind::Reasoning => opts.show_reasoning,
        _ => opts.expand || failed(ev),
    }))
}

fn failed(ev: &AgentEvent) -> bool {
    match ev.kind.as_str() {
        "tool_call" => ToolCall::from_event(ev).failed,
        "command" => Shell::from_event(ev).exit.is_some_and(|c| c != 0),
        _ => false,
    }
}

fn kind_of(kind: &str) -> Option<ChatKind> {
    Some(match kind {
        "user" => ChatKind::User,
        "agent" => ChatKind::Agent,
        "reasoning" => ChatKind::Reasoning,
        "tool_call" => ChatKind::Tool,
        "command" => ChatKind::Command,
        "file_change" => ChatKind::FileChange,
        "approval" => ChatKind::Approval,
        "error" => ChatKind::Error,
        _ => return None,
    })
}

fn label(kind: ChatKind) -> &'static str {
    match kind {
        ChatKind::User => "You",
        ChatKind::Agent => "Agent",
        ChatKind::Reasoning => "Thinking",
        ChatKind::Tool => "Tool",
        ChatKind::Command => "Shell",
        ChatKind::FileChange => "File change",
        ChatKind::Approval => "Approval",
        ChatKind::Error => "Error",
    }
}

/// Header colour of a row kind, by the desktop chat's accent roles.
fn color(kind: ChatKind) -> Color {
    match kind {
        ChatKind::User | ChatKind::Command | ChatKind::Approval => THEME.warning,
        ChatKind::Agent | ChatKind::Tool => THEME.accent,
        ChatKind::Reasoning | ChatKind::FileChange => THEME.tertiary,
        ChatKind::Error => THEME.error,
    }
}

fn render(
    ev: &AgentEvent,
    key: String,
    fold: Option<bool>,
    opts: &Options,
    now: &DateTime<Local>,
) -> Option<Row> {
    let width = opts.width.max(MIN_WIDTH);
    let time = event_time(ev, now);
    let spinner = (!ev.done).then_some(opts.spinner);
    let text = ev.text.as_str();
    let kind = match ev.kind.as_str() {
        "turn_started" => {
            let caption = chat::turn_title(ev.turn_id.as_deref());
            return Some(Row {
                key,
                fold: None,
                gap: true,
                lines: vec![divider(&caption, width)],
            });
        }
        "turn_completed" => return None,
        other => match kind_of(other) {
            Some(kind) => kind,
            None => return notice(text, time, key, width),
        },
    };
    let mut head = Head::new(kind, time);
    match kind {
        ChatKind::User | ChatKind::Agent | ChatKind::Error => {
            head.spinner = spinner.filter(|_| kind == ChatKind::Agent);
            let ink = if kind == ChatKind::Error {
                THEME.error
            } else {
                THEME.text
            };
            Some(message(head, text, ink, width, key))
        }
        ChatKind::Reasoning => {
            if text.trim().is_empty() {
                return None;
            }
            head.spinner = spinner;
            head.summary = chat::summary_line(text, SUMMARY_CHARS);
            Some(folding(head, fold, width, key, |w| {
                markdown::render(text, w, THEME.text_muted)
            }))
        }
        ChatKind::Tool => {
            let call = ToolCall::from_event(ev);
            head.name = Some(chat::tool_label(call.name).to_string());
            if call.running {
                head.badge = Some(("running".into(), THEME.accent_soft));
                head.spinner = spinner;
            } else if call.failed {
                head.badge = Some(("failed".into(), THEME.error));
            }
            head.duration = call.duration_ms.map(chat::duration_label);
            head.summary = tool_summary(&call);
            Some(folding(head, fold, width, key, |w| tool_body(&call, w)))
        }
        ChatKind::Command => {
            let shell = Shell::from_event(ev);
            match shell.exit.filter(|c| *c != 0) {
                Some(code) => head.badge = Some((format!("exit {code}"), THEME.error)),
                None if !ev.done => {
                    head.badge = Some(("running".into(), THEME.accent_soft));
                    head.spinner = spinner;
                }
                None => {}
            }
            head.duration = shell.duration_ms.map(chat::duration_label);
            head.summary = chat::clip(&wrap::one_line(shell.command), SUMMARY_CHARS).into_owned();
            Some(folding(head, fold, width, key, |w| shell.body(w)))
        }
        ChatKind::FileChange => {
            let changes = file_changes(ev);
            head.spinner = spinner;
            head.summary = if changes.is_empty() {
                chat::summary_line(text, SUMMARY_CHARS)
            } else {
                let paths: Vec<&str> = changes.iter().map(|(path, _)| *path).collect();
                chat::clip(&paths.join(", "), SUMMARY_CHARS).into_owned()
            };
            Some(folding(head, fold, width, key, |w| {
                let mut out = Vec::new();
                if changes.is_empty() {
                    out.extend(markdown::render(text, w, THEME.text));
                }
                for (path, diff) in &changes {
                    out.extend(caption(path, THEME.text, w));
                    if !diff.trim().is_empty() {
                        out.extend(markdown::code_block("diff", &wrap::sanitize(diff), w));
                    }
                }
                out
            }))
        }
        ChatKind::Approval => {
            let details = ev.item.as_ref().filter(|i| !i.is_null());
            head.summary = chat::summary_line(text, SUMMARY_CHARS);
            Some(folding(head, fold, width, key, |w| {
                let mut out = markdown::render(text, w, THEME.text);
                if let Some(item) = details {
                    out.extend(caption("Details", THEME.text_muted, w));
                    out.extend(json::lines(item, w));
                }
                out
            }))
        }
    }
}

/// The parts of a row's header line.
struct Head {
    kind: ChatKind,
    name: Option<String>,
    spinner: Option<&'static str>,
    badge: Option<(String, Color)>,
    duration: Option<String>,
    time: Option<String>,
    summary: String,
}

impl Head {
    fn new(kind: ChatKind, time: Option<String>) -> Self {
        Self {
            kind,
            name: None,
            spinner: None,
            badge: None,
            duration: None,
            time,
            summary: String::new(),
        }
    }

    /// Marker and label in the kind's colour, then name, badge, duration, time and, while folded, the summary.
    fn line(self, fold: Option<bool>, width: usize) -> Line<'static> {
        let ink = color(self.kind);
        let mark = match fold {
            Some(true) => glyphs::ROW_OPEN,
            Some(false) => glyphs::ROW_CLOSED,
            None => glyphs::GUTTER,
        };
        let mut spans = vec![
            Span::styled(mark, Style::default().fg(ink)),
            Span::raw(" "),
            Span::styled(
                label(self.kind),
                Style::default().fg(ink).add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(name) = self.name {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                name,
                Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
            ));
        }
        let spinner = self
            .spinner
            .map(|s| Span::styled(format!(" {s}"), Style::default().fg(ink)));
        match self.badge {
            Some((text, c)) => {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(text, Style::default().fg(c)));
                spans.extend(spinner);
            }
            None => spans.extend(spinner),
        }
        let parts = [
            self.duration.map(|d| (d, muted())),
            self.time.map(|t| (t, dim())),
        ];
        for (text, style) in parts.into_iter().flatten() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(text, style));
        }
        if fold == Some(false) && !self.summary.is_empty() {
            spans.push(Span::raw("  "));
            let summary = wrap::sanitize(&self.summary).replace('\n', " ");
            spans.push(Span::styled(summary, muted()));
        }
        wrap::fit(spans, width)
    }
}

/// A message row: the header, then the markdown body inset under it.
fn message(head: Head, text: &str, ink: Color, width: usize, key: String) -> Row {
    let mut lines = vec![head.line(None, width)];
    if !text.trim().is_empty() {
        let body = markdown::render(text, width - BODY_INDENT, ink);
        lines.extend(wrap::indent(body, &[wrap::pad(BODY_INDENT)]));
    }
    Row {
        key,
        fold: None,
        gap: true,
        lines,
    }
}

/// A folding row: the header, then while open the body behind a left rule.
fn folding(
    head: Head,
    fold: Option<bool>,
    width: usize,
    key: String,
    body: impl FnOnce(usize) -> Vec<Line<'static>>,
) -> Row {
    let open = fold == Some(true);
    let mut lines = vec![head.line(fold, width)];
    if open {
        let rule = [Span::styled(
            format!("{} ", glyphs::DETAIL_RULE),
            Style::default().fg(THEME.border_muted),
        )];
        lines.extend(wrap::indent(body(width - 2), &rule));
    }
    Row {
        key,
        fold,
        gap: open,
        lines,
    }
}

fn tool_body(call: &ToolCall<'_>, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if let Some(args) = call.arguments {
        out.extend(caption("Arguments", THEME.text_muted, width));
        out.extend(json::lines(args, width));
    } else if let Some(raw) = call.raw_arguments.filter(|r| !r.trim().is_empty()) {
        out.extend(caption("Arguments", THEME.text_muted, width));
        out.extend(payload(raw, width, THEME.text));
    }
    if let Some(error) = call.error() {
        out.extend(caption("Error", THEME.error, width));
        out.extend(payload(&error, width, THEME.error));
    } else if let Some(output) = call.output() {
        out.extend(caption("Result", THEME.text_muted, width));
        out.extend(payload(&output, width, THEME.text));
    } else if call.running {
        out.extend(note("Waiting for the tool to finish.", width));
    }
    out
}

/// One-line summary of a call's arguments: `key=value` members for JSON, else their first line.
fn tool_summary(call: &ToolCall<'_>) -> String {
    match (call.arguments, call.raw_arguments) {
        (Some(args), _) => chat::json_summary(args, SUMMARY_CHARS),
        (None, Some(raw)) => match as_json(raw).as_deref() {
            Some([only]) => chat::json_summary(only, SUMMARY_CHARS),
            _ => {
                let first = raw.lines().map(str::trim).find(|l| !l.is_empty());
                chat::clip(first.unwrap_or(""), SUMMARY_CHARS).into_owned()
            }
        },
        (None, None) => String::new(),
    }
}

/// A shell command from its stored `commandExecution` item, or from the row text while it runs.
struct Shell<'a> {
    command: &'a str,
    output: &'a str,
    exit: Option<i64>,
    duration_ms: Option<u64>,
}

impl<'a> Shell<'a> {
    fn from_event(ev: &'a AgentEvent) -> Self {
        let field = |k: &str| ev.item.as_ref().and_then(|i| i.get(k));
        let (command, output) = match ev.text.strip_prefix("$ ") {
            Some(rest) => rest.split_once('\n').unwrap_or((rest, "")),
            None => ("", ev.text.as_str()),
        };
        Self {
            command: field("command").and_then(Value::as_str).unwrap_or(command),
            output: field("aggregatedOutput")
                .and_then(Value::as_str)
                .unwrap_or(output),
            exit: field("exitCode").and_then(Value::as_i64),
            duration_ms: field("durationMs").and_then(Value::as_u64),
        }
    }

    fn body(&self, width: usize) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        let command = wrap::sanitize(self.command.trim());
        if !command.is_empty() {
            out.extend(markdown::command(&command, width));
        }
        if !self.output.trim().is_empty() {
            out.extend(mono(self.output, width, THEME.text));
        }
        out
    }
}

/// `(path, diff)` of each change in a stored `fileChange` item.
fn file_changes(ev: &AgentEvent) -> Vec<(&str, &str)> {
    ev.item
        .as_ref()
        .and_then(|i| i.get("changes"))
        .and_then(Value::as_array)
        .map(|changes| {
            changes
                .iter()
                .filter_map(|c| {
                    let path = c.get("path")?.as_str()?;
                    Some((path, c.get("diff").and_then(Value::as_str).unwrap_or("")))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A tool payload: JSON as JSON, a sentence ending in JSON as both, anything else as monospace text.
fn payload(text: &str, width: usize, ink: Color) -> Vec<Line<'static>> {
    if let Some(values) = as_json(text) {
        return values.iter().flat_map(|v| json::lines(v, width)).collect();
    }
    if let Some((head, values)) = split_json(text) {
        let mut out = mono(head, width, ink);
        out.extend(values.iter().flat_map(|v| json::lines(v, width)));
        return out;
    }
    mono(text, width, ink)
}

/// Monospace text in `ink`, cut at [`MONO_MAX_CHARS`] with a note of how much was left out.
fn mono(text: &str, width: usize, ink: Color) -> Vec<Line<'static>> {
    let text = wrap::sanitize(text);
    let (shown, rest) = match text.char_indices().nth(MONO_MAX_CHARS) {
        Some((cut, _)) => (&text[..cut], text[cut..].chars().count()),
        None => (&text[..], 0),
    };
    let style = Style::default().fg(ink);
    let mut out: Vec<Line<'static>> = shown
        .trim_end()
        .split('\n')
        .flat_map(|line| wrap::mono(&[(style, line)], width, &[], &[]))
        .collect();
    if rest > 0 {
        out.extend(note(
            &format!("{} {rest} more characters", glyphs::ELLIPSIS),
            width,
        ));
    }
    out
}

/// A turn boundary: a centred caption on a rule across the pane.
pub fn divider(caption: &str, width: usize) -> Line<'static> {
    let caption = wrap::clip(&format!(" {caption} "), width);
    let side = width.saturating_sub(wrap::width(&caption));
    let left = side / 2;
    Line::from(vec![
        Span::styled(glyphs::RULE.repeat(left), dim()),
        Span::styled(caption, dim()),
        Span::styled(glyphs::RULE.repeat(side - left), dim()),
    ])
}

/// A row for an event of no known kind: its text after a dot, then the time.
fn notice(text: &str, time: Option<String>, key: String, width: usize) -> Option<Row> {
    let text = wrap::sanitize(text.trim());
    if text.is_empty() {
        return None;
    }
    let mut runs = vec![(
        muted(),
        format!("{} {}", glyphs::DOT, wrap::one_line(&text)),
    )];
    if let Some(time) = time {
        runs.push((dim(), format!("  {time}")));
    }
    let lines = wrap::mono(&runs, width, &[], &[wrap::pad(2)]);
    Some(Row {
        key,
        fold: None,
        gap: false,
        lines,
    })
}

fn caption(text: &str, color: Color, width: usize) -> Vec<Line<'static>> {
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    wrap::words(&[(style, text)], width, &[], &[])
}

fn note(text: &str, width: usize) -> Vec<Line<'static>> {
    wrap::words(
        &[(muted().add_modifier(Modifier::ITALIC), text)],
        width,
        &[],
        &[],
    )
}

fn muted() -> Style {
    Style::default().fg(THEME.text_muted)
}

fn dim() -> Style {
    Style::default().fg(THEME.overlay)
}

fn event_time(ev: &AgentEvent, now: &DateTime<Local>) -> Option<String> {
    let at = ev.created_at.or(ev.updated_at)?;
    Some(chat::local_clock(DateTime::<Utc>::from(at), now))
}

/// Text of the first question's option `n` (1-based), when the agent offered options.
pub fn option_label(questions: Option<&Value>, n: usize) -> Option<String> {
    let q = questions?.as_array()?.first()?;
    let opt = q.get("options")?.as_array()?.get(n.checked_sub(1)?)?;
    opt.get("label").and_then(Value::as_str).map(str::to_string)
}

/// `{qid: [answer]}` for the first question, the shape the broker forwards to codex.
pub fn question_answers(questions: Option<&Value>, answer: &str) -> Value {
    let qid = questions
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|q| q.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("answer");
    serde_json::json!({ qid: [answer] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::Datetime;
    use serde_json::json;

    fn event(key: &str, kind: &str, text: &str, done: bool, item: Option<Value>) -> AgentEvent {
        AgentEvent {
            id: RecordId::new("agent_event", format!("t1:{key}").as_str()),
            thread: RecordId::new("agent_thread", "t1"),
            seq: 1,
            turn_id: Some("t3".into()),
            item_id: Some(key.into()),
            kind: kind.into(),
            text: text.into(),
            done,
            item,
            created_at: Some(Datetime::from_timestamp(1_790_000_000, 0).expect("valid time")),
            updated_at: None,
        }
    }

    fn opts(width: usize, expand: bool) -> Options {
        Options {
            width,
            show_reasoning: false,
            expand,
            spinner: glyphs::SPINNER[0],
        }
    }

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn lines_of(row: &Row) -> Vec<String> {
        row.lines.iter().map(text).collect()
    }

    fn rows(events: &[AgentEvent], opts: &Options) -> Vec<Rc<Row>> {
        Transcript::default().rows(events, opts, &HashMap::new())
    }

    fn sample() -> Vec<AgentEvent> {
        let long = "word ".repeat(300);
        let agent = format!(
            "# Result\n\nSome **bold** `code` [link](https://x.y)\n```powershell\nGet-PhysicalDisk | Select-Object Health\n```\n```json\n{{\"a\": [1, {{\"b\": \"{long}\"}}]}}\n```\n{long}\n> quote\n1. one\n   - nested\n---\n$ echo {long}"
        );
        vec![
            event("a", "turn_started", "", true, None),
            event(
                "b",
                "user",
                "Check **disk** health:\n```\nfans are loud\n```",
                true,
                None,
            ),
            event("c", "reasoning", "## Plan\n- read SMART", false, None),
            event(
                "d",
                "tool_call",
                "get_client_info({\"connection_string\":\"PC-1\",\"cut…)",
                true,
                Some(
                    json!({"type": "dynamicToolCall", "tool": "mastertech__get_client_info",
                    "arguments": {"connection_string": "PC-1", "script": long}, "status": "completed",
                    "success": true, "durationMs": 1234,
                    "contentItems": [{"type": "inputText", "text": "{\"hostname\":\"PC-1\",\"ok\":true,\"error\":null}"}]}),
                ),
            ),
            event(
                "e",
                "tool_call",
                "run_script({})",
                true,
                Some(
                    json!({"tool": "run_script", "arguments": {}, "success": false, "status": "failed",
                    "contentItems": [{"type": "inputText", "text": format!("Declined by the technician. {long}")}]}),
                ),
            ),
            event(
                "f",
                "tool_call",
                "query_surrealdb({\"query\":\"SELECT 1\"})",
                false,
                None,
            ),
            event(
                "g",
                "command",
                &format!("$ ls -la\n{long}"),
                true,
                Some(
                    json!({"command": "ls -la /tmp", "aggregatedOutput": "total 0\n", "exitCode": 2, "durationMs": 40}),
                ),
            ),
            event(
                "h",
                "file_change",
                "file changes",
                true,
                Some(json!({"changes": [{"path": "a.rs", "diff": "@@ -1 +1 @@\n-old\n+new"}]})),
            ),
            event(
                "i",
                "approval",
                "Waiting for a technician to approve: run remote_exec_start on PC-1",
                true,
                Some(json!({"approval": "x", "tool": "remote_exec_start", "arguments": {"a": 1}})),
            ),
            event("j", "agent", &agent, false, None),
            event("k", "error", "Agent error: boom", true, None),
            event("l", "other", "Reconnected to the agent host.", true, None),
            event("m", "turn_completed", "", true, None),
        ]
    }

    #[test]
    fn every_row_fits_the_pane_folded_and_unfolded() {
        let events = sample();
        for width in [16, 40, 97] {
            let closed: usize = rows(&events, &opts(width, false))
                .iter()
                .map(|r| r.lines.len())
                .sum();
            let open: usize = rows(&events, &opts(width, true))
                .iter()
                .map(|r| r.lines.len())
                .sum();
            assert!(
                open > closed + 10,
                "{width}: unfolding grew {closed} lines to {open}"
            );
            for expand in [false, true] {
                for row in rows(&events, &opts(width, expand)) {
                    for line in &row.lines {
                        assert!(
                            wrap::spans_width(&line.spans) <= width,
                            "{width}: {:?}",
                            text(line)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn only_glyphs_the_terminal_font_carries_are_drawn() {
        let allowed = |c: char| {
            (' '..='~').contains(&c)
                || matches!(c as u32, 0x2190..=0x2195 | 0x2500..=0x257F | 0x2580..=0x259F | 0x25A0..=0x25FF)
                || "\u{00a7}\u{00b7}\u{2013}\u{2014}\u{2026}\u{2713}".contains(c)
        };
        let events = sample();
        for expand in [false, true] {
            for row in rows(&events, &opts(60, expand)) {
                for line in &row.lines {
                    let t = text(line);
                    assert!(t.chars().all(allowed), "{t:?}");
                }
            }
        }
    }

    #[test]
    fn headers_carry_label_name_badge_duration_time_and_the_folded_summary() {
        let events = sample();
        let out = rows(&events, &opts(120, false));
        let tool = out.iter().find(|r| r.key == "t1:d").expect("tool row");
        let head = text(&tool.lines[0]);
        assert!(
            head.starts_with("\u{25b8} Tool get_client_info  1.2 s  "),
            "{head}"
        );
        assert!(head.contains("connection_string=PC-1"), "{head}");
        assert_eq!(tool.fold, Some(false));
        assert_eq!(tool.lines.len(), 1);

        let failed = out.iter().find(|r| r.key == "t1:e").expect("failed row");
        assert!(text(&failed.lines[0]).starts_with("\u{25be} Tool run_script  failed"));
        assert_eq!(failed.fold, Some(true));
        assert!(
            lines_of(failed)
                .iter()
                .any(|l| l.contains("Declined by the technician."))
        );

        let running = out.iter().find(|r| r.key == "t1:f").expect("running row");
        let spinner = format!(
            "\u{25b8} Tool query_surrealdb  running {}",
            glyphs::SPINNER[0]
        );
        assert!(
            text(&running.lines[0]).starts_with(&spinner),
            "{}",
            text(&running.lines[0])
        );

        let shell = out.iter().find(|r| r.key == "t1:g").expect("shell row");
        assert!(text(&shell.lines[0]).starts_with("\u{25be} Shell  exit 2  40 ms"));

        let user = out.iter().find(|r| r.key == "t1:b").expect("user row");
        let user_lines = lines_of(user);
        assert!(
            user_lines[0].starts_with("\u{258c} You  "),
            "{user_lines:?}"
        );
        assert!(
            user_lines
                .iter()
                .any(|l| l.trim_end() == "  \u{258e} fans are loud"),
            "{user_lines:?}"
        );
        assert!(user_lines.iter().all(|l| !l.contains("```")));

        let divider = out.iter().find(|r| r.key == "t1:a").expect("divider");
        assert!(text(&divider.lines[0]).contains(" Turn 3 "));
        assert!(!out.iter().any(|r| r.key == "t1:m"));
    }

    #[test]
    fn tool_rows_read_the_full_arguments_and_colour_the_result_json() {
        let events = sample();
        let out = rows(&events, &opts(200, true));
        let tool = out.iter().find(|r| r.key == "t1:d").expect("tool row");
        let body = lines_of(tool);
        assert!(
            body.iter()
                .any(|l| l.contains("\"connection_string\": \"PC-1\"")),
            "{body:?}"
        );
        assert!(!body.iter().any(|l| l.contains("cut\u{2026}")), "{body:?}");
        let null = tool
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == "null")
            .expect("null span");
        let string = tool
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == "\"PC-1\"")
            .expect("string span");
        assert_ne!(null.style, string.style);
    }

    #[test]
    fn a_toggle_beats_the_expand_mode_and_the_default() {
        let events = sample();
        let mut toggled = HashMap::new();
        toggled.insert("t1:d".to_string(), true);
        toggled.insert("t1:e".to_string(), false);
        let out = Transcript::default().rows(&events, &opts(80, false), &toggled);
        assert_eq!(
            out.iter().find(|r| r.key == "t1:d").and_then(|r| r.fold),
            Some(true)
        );
        assert_eq!(
            out.iter().find(|r| r.key == "t1:e").and_then(|r| r.fold),
            Some(false)
        );
        let reasoning = out.iter().find(|r| r.key == "t1:c").expect("thinking row");
        assert_eq!(reasoning.fold, Some(false));
        let shown = Transcript::default().rows(
            &events,
            &Options {
                show_reasoning: true,
                ..opts(80, false)
            },
            &HashMap::new(),
        );
        assert_eq!(
            shown.iter().find(|r| r.key == "t1:c").and_then(|r| r.fold),
            Some(true)
        );
    }

    #[test]
    fn finished_rows_are_reused_until_forgotten_or_redrawn_at_another_width() {
        let events = sample();
        let mut t = Transcript::default();
        let none = HashMap::new();
        let first = t.rows(&events, &opts(80, false), &none);
        let again = t.rows(&events, &opts(80, false), &none);
        let same = |a: &[Rc<Row>], b: &[Rc<Row>], key: &str| {
            let find = |rows: &[Rc<Row>]| rows.iter().find(|r| r.key == key).cloned().expect("row");
            Rc::ptr_eq(&find(a), &find(b))
        };
        assert!(same(&first, &again, "t1:d"));
        assert!(
            !same(&first, &again, "t1:j"),
            "a streaming row is redrawn every frame"
        );
        t.forget(&events[3].id);
        let after = t.rows(&events, &opts(80, false), &none);
        assert!(!same(&first, &after, "t1:d"));
        assert!(same(&first, &after, "t1:b"));
        let wider = t.rows(&events, &opts(90, false), &none);
        assert!(!same(&after, &wider, "t1:b"));
    }

    #[test]
    fn flattening_spaces_message_rows_and_stacks_folded_ones() {
        let row = |key: &str, gap: bool| {
            Rc::new(Row {
                key: key.into(),
                fold: None,
                gap,
                lines: vec![Line::from(key.to_string()), Line::from(format!("{key}2"))],
            })
        };
        let rows = vec![
            row("a", true),
            row("b", false),
            row("c", false),
            row("d", true),
        ];
        let blank = Line::default();
        let tail = vec![Line::from("tail")];
        let slots = flatten(&rows, &tail, &blank);
        let flat: Vec<String> = slots.iter().map(|s| text(s.line)).collect();
        assert_eq!(
            flat,
            vec![
                "a", "a2", "", "b", "b2", "c", "c2", "", "d", "d2", "", "tail"
            ]
        );
        assert_eq!(slots.iter().filter(|s| s.is_head()).count(), 4);
        assert_eq!(slots.iter().position(|s| s.is("c", 1)), Some(6));
        assert!(!slots[11].is_head());
    }

    #[test]
    fn answers_take_the_first_question_id() {
        let q = json!([{ "id": "q7", "options": [{ "label": "Yes" }, { "label": "No" }] }]);
        assert_eq!(option_label(Some(&q), 2).as_deref(), Some("No"));
        assert_eq!(question_answers(Some(&q), "Yes"), json!({ "q7": ["Yes"] }));
        assert_eq!(question_answers(None, "x"), json!({ "answer": ["x"] }));
    }
}
