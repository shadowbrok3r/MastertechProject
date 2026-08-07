//! Terminal-mode chat with a headless Claude Code session (subscription auth,
//! Mastertech MCP on :9004) via `displays::ai::claude_code::ClaudeCodeSession`.

use crossbeam::channel::{unbounded, Receiver, Sender};
use displays::ai::claude_code::{ClaudeCodeSession, TOOL_PREFIX};
use displays::tabs::ai_playground::{ChatMessage, ChatMessageType, SentFrom};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    prelude::Backend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::cell::Cell;

use crate::terminal_mode::{
    events::action_handler::WidgetId,
    styling::THEME,
    widgets::{button::ButtonState, input_field::InputField, ButtonType, HandleWidget, SHORTCUT_SET},
};

const THREAD_ID: &str = "terminal-assistant";

pub struct AssistantTab<'a> {
    input: InputField<'a>,
    messages: Vec<ChatMessage>,
    scroll_back: Cell<usize>,
    user_seq: u64,
    session: ClaudeCodeSession,
    channel: (Sender<ChatMessage>, Receiver<ChatMessage>),
}

impl<'a> AssistantTab<'a> {
    pub fn new() -> Self {
        let input = InputField::new("Ask Claude Code", WidgetId("AssistantInput".to_string()));
        input.set_state(ButtonState::Active);
        Self {
            input,
            messages: Vec::new(),
            scroll_back: Cell::new(0),
            user_seq: 0,
            session: ClaudeCodeSession::new(),
            channel: unbounded(),
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.channel.1.try_recv() {
            match msg.content.clone() {
                ChatMessageType::Done => {}
                ChatMessageType::Error(_) => self.messages.push(msg),
                ChatMessageType::Text(chunk) | ChatMessageType::Code(chunk) => {
                    self.upsert(&msg.id, msg.from.clone(), chunk, false)
                }
                ChatMessageType::Reasoning(chunk) => self.upsert(&msg.id, msg.from.clone(), chunk, true),
                ChatMessageType::FileId(_) | ChatMessageType::Image(_) => {}
            }
        }
    }

    /// Appends a chunk to the message with the same id or starts a new one.
    fn upsert(&mut self, id: &str, from: SentFrom, chunk: String, reasoning: bool) {
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
            match &mut m.content {
                ChatMessageType::Text(s) if !reasoning => s.push_str(&chunk),
                ChatMessageType::Reasoning(s) if reasoning => s.push_str(&chunk),
                _ => {}
            }
        } else {
            let content = if reasoning {
                ChatMessageType::Reasoning(chunk)
            } else {
                ChatMessageType::Text(chunk)
            };
            self.messages.push(ChatMessage {
                id: id.to_string(),
                thread_id: THREAD_ID.to_string(),
                ts: displays::tabs::ai_playground::now_ts(),
                from,
                content,
            });
        }
    }

    fn submit(&mut self) {
        if self.session.is_busy() {
            return;
        }
        let input = self.input.get_raw_text().trim().to_string();
        if input.is_empty() {
            return;
        }
        self.messages.push(ChatMessage {
            id: format!("u{}", self.user_seq),
            thread_id: THREAD_ID.to_string(),
            ts: displays::tabs::ai_playground::now_ts(),
            from: SentFrom::Me,
            content: ChatMessageType::Text(input.clone()),
        });
        self.user_seq += 1;
        self.input.set_text("");
        self.scroll_back.set(0);
        self.session.send(input, None, THREAD_ID.to_string(), self.channel.0.clone());
    }

    /// Flattens messages into styled, wrapped lines for the viewport.
    fn display_lines(&self, width: usize) -> Vec<Line<'static>> {
        let width = width.max(8);
        let bold = Modifier::BOLD;
        let mut lines: Vec<Line> = Vec::new();
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
                (SentFrom::Assistant, ChatMessageType::Reasoning(t)) => {
                    lines.push(Line::from(Span::styled(
                        "\u{00B7} thinking",
                        Style::default().fg(THEME.text_muted).add_modifier(Modifier::ITALIC),
                    )));
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.text_muted)));
                    }
                    lines.push(Line::from(""));
                }
                (SentFrom::Assistant, ChatMessageType::Text(t)) if t.starts_with(TOOL_PREFIX) => {
                    // Result rides after the first newline; a TUI pane cannot
                    // collapse it, so show the call and a one-line preview.
                    let (head, body) = t.split_once('\n').unwrap_or((t.as_str(), ""));
                    for w in wrap(head, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.tertiary)));
                    }
                    let body = body.trim();
                    if !body.is_empty() {
                        let preview: String =
                            body.chars().take(160).collect::<String>().replace('\n', " ");
                        for w in wrap(&preview, width.saturating_sub(2)) {
                            lines.push(
                                Line::from(format!("  {w}"))
                                    .style(Style::default().fg(THEME.text_muted)),
                            );
                        }
                    }
                }
                (SentFrom::Assistant, ChatMessageType::Text(t)) => {
                    lines.push(Line::from(Span::styled(
                        "\u{258C} Claude",
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
                _ => {}
            }
        }
        lines
    }
}

impl<'a> HandleWidget<'a> for AssistantTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.poll();

        let rows = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(area);

        let busy = self.session.is_busy();
        let title = if busy {
            "Claude Code \u{00B7} working\u{2026}"
        } else {
            "Claude Code"
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title(title);

        let inner_w = rows[0].width.saturating_sub(2) as usize;
        let inner_h = rows[0].height.saturating_sub(2) as usize;
        let lines = if self.messages.is_empty() {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Ask about this machine, a customer, or a service order.",
                    Style::default().fg(THEME.text_muted),
                )),
                Line::from(Span::styled(
                    "  Claude Code answers with the Mastertech tools (needs `claude login` on this PC).",
                    Style::default().fg(THEME.text_muted),
                )),
            ]
        } else {
            self.display_lines(inner_w)
        };
        let total = lines.len();
        let max_back = total.saturating_sub(inner_h);
        let back = self.scroll_back.get().min(max_back);
        self.scroll_back.set(back);
        let end = total.saturating_sub(back);
        let start = end.saturating_sub(inner_h);
        let view: Vec<Line> = lines[start..end].to_vec();

        f.render_widget(
            Paragraph::new(view).block(block).style(Style::default().bg(THEME.bg)),
            rows[0],
        );

        f.render_widget(&self.input, rows[1]);

        let state = if busy { "  \u{00B7}  Esc stop  \u{00B7}  working\u{2026}" } else { "" };
        let session = self
            .session
            .session_id()
            .map(|s| format!("  \u{00B7}  session {}", s.chars().take(8).collect::<String>()))
            .unwrap_or_default();
        let footer = format!(
            "Enter send  \u{00B7}  Alt+Enter newline  \u{00B7}  PgUp/PgDn scroll  \u{00B7}  Ctrl+L new session{session}{state}"
        );
        f.render_widget(
            Paragraph::new(footer).style(Style::default().fg(THEME.text_muted).bg(THEME.bg)),
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
            KeyCode::Esc if self.session.is_busy() => {
                self.session.cancel();
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
                self.session.reset();
                self.messages.clear();
                self.scroll_back.set(0);
                true
            }
            _ => ButtonType::handle_key_event(&self.input, &key),
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => self.scroll_back.set(self.scroll_back.get().saturating_add(3)),
            MouseEventKind::ScrollDown => self.scroll_back.set(self.scroll_back.get().saturating_sub(3)),
            _ => ButtonType::handle_mouse_event(&self.input, mouse_event),
        }
    }
}

/// Word-wraps to `width` columns with hard breaks for overlong words.
fn wrap(text: &str, width: usize) -> Vec<String> {
    fn push_word(out: &mut Vec<String>, line: &mut String, width: usize, word: &str) {
        let wlen = word.chars().count();
        let llen = line.chars().count();
        if llen > 0 && llen + 1 + wlen <= width {
            line.push(' ');
            line.push_str(word);
            return;
        }
        if llen > 0 {
            out.push(std::mem::take(line));
        }
        let mut chars = word.chars().peekable();
        while chars.peek().is_some() {
            let chunk: String = chars.by_ref().take(width).collect();
            if chars.peek().is_some() {
                out.push(chunk);
            } else {
                *line = chunk;
            }
        }
    }

    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in raw.split_whitespace() {
            push_word(&mut out, &mut line, width, word);
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}
