use std::cell::Cell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{AgentEvent, RecordId, RecordIdExt};
use mtech_tui::events::action_handler::WidgetId;
use mtech_tui::styling::{APP_BACKGROUND, THEME};
use mtech_tui::widgets::{
    button::ButtonState, input_field::InputField, ButtonType, HandleWidget, SHORTCUT_SET,
};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    prelude::Backend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, WidgetRef},
    Frame,
};

use crate::terminal_mode::agent_backend::{self, BackendMsg, ChatMessage, ChatMessageType, SentFrom, TOOL_PREFIX};

const INPUT_ID: &str = "AiPrompt";
const POLL_EVERY: Duration = Duration::from_secs(2);

/// Chat with the Codex agent about this machine: its Mastertech session when one is live, else the technician's records session.
pub struct AiTab<'a> {
    input: InputField<'a>,
    messages: Vec<ChatMessage>,
    scroll_back: Cell<usize>,
    busy: bool,
    replied: bool,
    tech_email: Option<String>,
    session: Option<Session>,
    seen: HashSet<String>,
    last_poll: Option<Instant>,
    polling: bool,
    channel: (Sender<BackendMsg>, Receiver<BackendMsg>),
}

/// The agent session this conversation lives in, and the event seq it started after.
struct Session {
    thread: RecordId,
    after_seq: i64,
    target: String,
}

impl<'a> AiTab<'a> {
    pub fn new() -> Self {
        let input = InputField::new("Ask about this PC", WidgetId(INPUT_ID.to_string()));
        input.set_state(ButtonState::Active);
        Self {
            input,
            messages: Vec::new(),
            scroll_back: Cell::new(0),
            busy: false,
            replied: false,
            tech_email: None,
            session: None,
            seen: HashSet::new(),
            last_poll: None,
            polling: false,
            channel: unbounded(),
        }
    }

    /// The technician signed in on the Order QC tab, whose records session is the fallback.
    pub fn set_tech_email(&mut self, email: Option<String>) {
        self.tech_email = email;
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.channel.1.try_recv() {
            match msg {
                BackendMsg::Opened { thread, after_seq, target } => {
                    if self.session.as_ref().is_none_or(|s| s.thread != thread) {
                        self.session = Some(Session { thread, after_seq, target });
                    }
                    self.last_poll = None;
                }
                BackendMsg::Rows { rows, status } => {
                    self.polling = false;
                    for row in rows {
                        self.absorb(row);
                    }
                    if let Some(status) = status {
                        let settled = matches!(status.as_str(), "idle" | "closed" | "failed");
                        if settled && (self.replied || status != "idle") {
                            self.busy = false;
                        }
                    }
                }
                BackendMsg::Error(e) => {
                    self.busy = false;
                    self.push(SentFrom::Assistant, ChatMessageType::Error(e));
                }
            }
        }
        self.schedule_poll();
    }

    /// Renders one finished agent row the first time it is seen.
    fn absorb(&mut self, row: AgentEvent) {
        if !row.done {
            return;
        }
        let id = row.id.key_string();
        if self.seen.contains(&id) {
            return;
        }
        let first_line = row.text.lines().next().unwrap_or_default().to_string();
        let content = match row.kind.as_str() {
            "agent" => ChatMessageType::Text(row.text),
            "error" => ChatMessageType::Error(row.text),
            "tool_call" | "command" => ChatMessageType::Text(format!("{TOOL_PREFIX}{first_line}")),
            "approval" => ChatMessageType::Text(format!("{TOOL_PREFIX}approval needed in Mastertech: {first_line}")),
            _ => return,
        };
        if matches!(row.kind.as_str(), "agent" | "error") {
            self.replied = true;
        }
        self.seen.insert(id);
        self.messages.push(ChatMessage { from: SentFrom::Assistant, content });
    }

    fn push(&mut self, from: SentFrom, content: ChatMessageType) {
        self.messages.push(ChatMessage { from, content });
    }

    fn schedule_poll(&mut self) {
        let Some(session) = &self.session else { return };
        if self.polling || self.last_poll.is_some_and(|t| t.elapsed() < POLL_EVERY) {
            return;
        }
        self.polling = true;
        self.last_poll = Some(Instant::now());
        let tx = self.channel.0.clone();
        tokio::spawn(agent_backend::poll(session.thread.clone(), session.after_seq, tx));
    }

    fn submit(&mut self) {
        if self.busy {
            return;
        }
        let input = self.input.get_raw_text().trim().to_string();
        if input.is_empty() {
            return;
        }
        self.push(SentFrom::Me, ChatMessageType::Text(input.clone()));
        self.input.set_text("");
        self.busy = true;
        self.replied = false;
        self.scroll_back.set(0);

        let first = self.session.is_none();
        let tx = self.channel.0.clone();
        tokio::spawn(agent_backend::send(self.tech_email.clone(), input, first, tx));
    }

    /// Render the conversation into a flat list of display lines, word-wrapped
    /// to `width`, with a styled header per message.
    fn display_lines(&self, width: usize) -> Vec<Line<'static>> {
        if self.messages.is_empty() {
            return vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Ask about this computer — its hardware, temps, or stability.",
                    Style::default().fg(THEME.text_muted),
                )),
                Line::from(Span::styled(
                    "The agent answers in this machine's Mastertech session when Mastertech runs here,",
                    Style::default().fg(THEME.text_muted),
                )),
                Line::from(Span::styled(
                    "otherwise in your own session, with a snapshot of this machine's telemetry.",
                    Style::default().fg(THEME.text_muted),
                )),
            ];
        }

        let mut lines: Vec<Line<'static>> = Vec::new();
        let bold = Modifier::BOLD;
        for m in &self.messages {
            match (&m.from, &m.content) {
                (SentFrom::Me, ChatMessageType::Text(t)) => {
                    lines.push(Line::from(Span::styled(
                        "\u{258C} You",
                        Style::default().fg(THEME.accent).add_modifier(bold),
                    )));
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.text)));
                    }
                    lines.push(Line::from(""));
                }
                (SentFrom::Assistant, ChatMessageType::Text(t)) if t.starts_with(TOOL_PREFIX) => {
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.tertiary)));
                    }
                }
                (SentFrom::Assistant, ChatMessageType::Text(t)) => {
                    lines.push(Line::from(Span::styled(
                        "\u{258C} Agent",
                        Style::default().fg(THEME.success).add_modifier(bold),
                    )));
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.text)));
                    }
                    lines.push(Line::from(""));
                }
                (_, ChatMessageType::Error(t)) => {
                    lines.push(Line::from(Span::styled(
                        "! error",
                        Style::default().fg(THEME.error).add_modifier(bold),
                    )));
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.error)));
                    }
                    lines.push(Line::from(""));
                }
            }
        }
        lines
    }
}

/// Word-wrap `text` to `width` columns, hard-breaking words longer than a line.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let mut cur = String::new();
        for word in raw.split(' ') {
            if cur.is_empty() {
                cur.push_str(word);
            } else if cur.chars().count() + 1 + word.chars().count() <= width {
                cur.push(' ');
                cur.push_str(word);
            } else {
                out.push(std::mem::take(&mut cur));
                cur.push_str(word);
            }
            while cur.chars().count() > width {
                let head: String = cur.chars().take(width).collect();
                out.push(head);
                cur = cur.chars().skip(width).collect();
            }
        }
        out.push(cur);
    }
    out
}

impl<'a> Default for AiTab<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> HandleWidget<'a> for AiTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.poll();

        let rows = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Diagnose this computer");

        let inner_w = rows[0].width.saturating_sub(2) as usize;
        let inner_h = rows[0].height.saturating_sub(2) as usize;
        let lines = self.display_lines(inner_w);
        let total = lines.len();
        let max_back = total.saturating_sub(inner_h);
        let back = self.scroll_back.get().min(max_back);
        self.scroll_back.set(back);
        let end = total.saturating_sub(back);
        let start = end.saturating_sub(inner_h);
        let view: Vec<Line> = lines[start..end].to_vec();

        f.render_widget(
            Paragraph::new(view)
                .block(block)
                .style(Style::default().bg(APP_BACKGROUND)),
            rows[0],
        );

        self.input.render_ref(rows[1], f.buffer_mut());

        let state = if self.busy { "  agent working\u{2026}" } else { "" };
        let target = self.session.as_ref().map(|s| format!("  \u{00B7}  {}", s.target)).unwrap_or_default();
        let footer = format!(
            "Enter send  \u{00B7}  Alt+Enter newline  \u{00B7}  PgUp/PgDn scroll  \u{00B7}  Ctrl+L clear{target}{state}"
        );
        f.render_widget(
            Paragraph::new(footer).style(Style::default().fg(THEME.text_muted).bg(APP_BACKGROUND)),
            rows[2],
        );
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Enter if alt => {
                self.input
                    .input
                    .borrow_mut()
                    .input_without_shortcuts(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                true
            }
            KeyCode::Enter => {
                self.submit();
                true
            }
            KeyCode::PageUp => {
                self.scroll_back.set(self.scroll_back.get().saturating_add(5));
                true
            }
            KeyCode::PageDown => {
                self.scroll_back.set(self.scroll_back.get().saturating_sub(5));
                true
            }
            KeyCode::Char('l') if ctrl => {
                self.messages.clear();
                self.scroll_back.set(0);
                true
            }
            _ => self.input.handle_key_event(&key),
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => self.scroll_back.set(self.scroll_back.get().saturating_add(3)),
            MouseEventKind::ScrollDown => self.scroll_back.set(self.scroll_back.get().saturating_sub(3)),
            _ => self.input.handle_mouse_event(mouse_event),
        }
    }
}
