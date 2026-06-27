use std::cell::Cell;

use crossbeam::channel::{unbounded, Receiver, Sender};
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

use crate::terminal_mode::ai_backend::{self, ChatMessage, ChatMessageType, SentFrom, TOOL_PREFIX};

const INPUT_ID: &str = "AiPrompt";

/// In-process AI self-diagnosis chat for the local QC machine. Streams an
/// OpenAI-compatible model that calls qc-app's own MCP tools (telemetry,
/// stress, benchmarks, reports) — the TUI twin of the displays AI tab.
pub struct AiTab<'a> {
    input: InputField<'a>,
    messages: Vec<ChatMessage>,
    scroll_back: Cell<usize>,
    busy: bool,
    use_tools: bool,
    user_seq: u64,
    channel: (Sender<ChatMessage>, Receiver<ChatMessage>),
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
            use_tools: true,
            user_seq: 0,
            channel: unbounded(),
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.channel.1.try_recv() {
            match msg.content.clone() {
                ChatMessageType::Done => self.busy = false,
                ChatMessageType::Error(_) => {
                    self.busy = false;
                    self.messages.push(msg);
                }
                ChatMessageType::Text(chunk) => self.upsert(&msg.id, msg.from.clone(), chunk, false),
                ChatMessageType::Reasoning(chunk) => self.upsert(&msg.id, msg.from.clone(), chunk, true),
            }
        }
    }

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
            self.messages.push(ChatMessage { id: id.to_string(), from, content });
        }
    }

    fn submit(&mut self) {
        if self.busy {
            return;
        }
        let input = self.input.get_raw_text().trim().to_string();
        if input.is_empty() {
            return;
        }
        let prior = ai_backend::history_json_from_messages(&self.messages);
        self.messages.push(ChatMessage {
            id: format!("u{}", self.user_seq),
            from: SentFrom::Me,
            content: ChatMessageType::Text(input.clone()),
        });
        self.user_seq += 1;
        self.input.set_text("");
        self.busy = true;
        self.scroll_back.set(0);

        let tx = self.channel.0.clone();
        let use_tools = self.use_tools;
        tokio::spawn(async move {
            if let Err(e) = ai_backend::stream_chat(input, prior, use_tools, tx).await {
                log::error!("ai_chat stream error: {e:?}");
            }
        });
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
                    "The assistant inspects this machine with the QC tools.",
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
                (SentFrom::Gpt, ChatMessageType::Reasoning(t)) => {
                    lines.push(Line::from(Span::styled(
                        "\u{00B7} thinking",
                        Style::default().fg(THEME.text_muted).add_modifier(Modifier::ITALIC),
                    )));
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.text_muted)));
                    }
                    lines.push(Line::from(""));
                }
                (SentFrom::Gpt, ChatMessageType::Text(t)) if t.starts_with(TOOL_PREFIX) => {
                    for w in wrap(t, width) {
                        lines.push(Line::from(w).style(Style::default().fg(THEME.tertiary)));
                    }
                }
                (SentFrom::Gpt, ChatMessageType::Text(t)) => {
                    lines.push(Line::from(Span::styled(
                        "\u{258C} Assistant",
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

        let tools = if self.use_tools { "on" } else { "off" };
        let state = if self.busy { "  thinking\u{2026}" } else { "" };
        let footer = format!(
            "Enter send  \u{00B7}  Alt+Enter newline  \u{00B7}  PgUp/PgDn scroll  \u{00B7}  Ctrl+T tools:{tools}  \u{00B7}  Ctrl+L clear{state}"
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
            KeyCode::Char('t') if ctrl => {
                self.use_tools = !self.use_tools;
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
