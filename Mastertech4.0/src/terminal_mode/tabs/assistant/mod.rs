//! Terminal-mode view of this machine's Codex agent session. The broker in
//! admin-agent talks to codex; this page reads `agent_event`, queues
//! `agent_turn` rows and decides `agent_approval` rows, like the desktop tab.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    AgentApproval, AgentDecideOutcome, AgentEvent, AgentThread, AgentTurn, AssistRequest, RecordId, RecordIdExt,
};
use displays::{PlatformSpawner, Spawner};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    layout::{Alignment, Constraint, Layout, Rect},
    prelude::Backend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use serde_json::Value;

use crate::terminal_mode::{
    events::action_handler::WidgetId,
    styling::{glyphs, THEME},
    widgets::{button::ButtonState, input_field::InputField, ButtonType, HandleWidget, SHORTCUT_SET},
};

mod transcript;

const THREAD_POLL: Duration = Duration::from_secs(4);
const THREAD_POLL_WAITING: Duration = Duration::from_secs(2);
const EVENT_POLL: Duration = Duration::from_millis(1500);
const APPROVAL_POLL: Duration = Duration::from_secs(2);
const EVENT_PAGE: usize = 400;
const SNOOZE: Duration = Duration::from_secs(60);
/// Longest wait for the broker to open a requested session before it reads as a failure.
const REQUEST_WAIT: Duration = Duration::from_secs(120);

enum Msg {
    Thread(Result<Option<AgentThread>, String>),
    Events(RecordId, Result<Vec<AgentEvent>, String>),
    Approvals(RecordId, Result<Vec<AgentApproval>, String>),
    Turn(Result<(), String>),
    Requested(Result<(), String>),
    Decided(RecordId, Result<AgentDecideOutcome, String>),
}

pub struct AssistantTab<'a> {
    input: InputField<'a>,
    connection_string: String,
    hostname: String,
    thread: Option<AgentThread>,
    events: Vec<AgentEvent>,
    last_seq: i64,
    approvals: Vec<AgentApproval>,
    snoozed: HashMap<RecordId, Instant>,
    in_flight: HashSet<RecordId>,
    show_reasoning: bool,
    note: String,
    requested_at: Option<Instant>,
    scroll_back: Cell<usize>,
    frame: Cell<usize>,
    last_thread_poll: Option<Instant>,
    last_event_poll: Option<Instant>,
    last_approval_poll: Option<Instant>,
    loading_thread: bool,
    loading_events: bool,
    loading_approvals: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl<'a> AssistantTab<'a> {
    pub fn new() -> Self {
        let input = InputField::new("Message the agent", WidgetId("AssistantInput".to_string()));
        input.set_state(ButtonState::Active);
        let connection_string = crate::filesystem::get_client_hash().connection_string;
        let hostname = connection_string.split(':').next().unwrap_or_default().to_string();
        let (tx, rx) = unbounded();
        Self {
            input,
            connection_string,
            hostname,
            thread: None,
            events: Vec::new(),
            last_seq: 0,
            approvals: Vec::new(),
            snoozed: HashMap::new(),
            in_flight: HashSet::new(),
            show_reasoning: false,
            note: String::new(),
            requested_at: None,
            scroll_back: Cell::new(0),
            frame: Cell::new(0),
            last_thread_poll: None,
            last_event_poll: None,
            last_approval_poll: None,
            loading_thread: false,
            loading_events: false,
            loading_approvals: false,
            tx,
            rx,
        }
    }

    fn running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|t| matches!(t.status.as_str(), "running" | "waiting_approval"))
    }

    fn open_thread(&self) -> Option<&AgentThread> {
        self.thread.as_ref().filter(|t| t.is_open())
    }

    /// The decision shown right now: the oldest pending one that is neither snoozed nor lapsed.
    fn active_approval(&self) -> Option<AgentApproval> {
        let now = Instant::now();
        self.approvals
            .iter()
            .find(|r| r.secs_remaining() > 0 && !self.snoozed.get(&r.id).is_some_and(|until| *until > now))
            .cloned()
    }

    fn poll_thread(&mut self) {
        if self.loading_thread {
            return;
        }
        self.loading_thread = true;
        self.last_thread_poll = Some(Instant::now());
        let tx = self.tx.clone();
        let cs = self.connection_string.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentThread::latest_for_connection(&cs).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Thread(r));
        });
    }

    fn poll_events(&mut self) {
        let Some(thread) = self.thread.as_ref().map(|t| t.id.clone()) else { return };
        if self.loading_events {
            return;
        }
        self.loading_events = true;
        self.last_event_poll = Some(Instant::now());
        let tx = self.tx.clone();
        let after = self.last_seq;
        PlatformSpawner::spawn(async move {
            let r = AgentEvent::since(&thread, after, EVENT_PAGE).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Events(thread, r));
        });
    }

    fn poll_approvals(&mut self) {
        let Some(thread) = self.open_thread().map(|t| t.id.clone()) else { return };
        if self.loading_approvals {
            return;
        }
        self.loading_approvals = true;
        self.last_approval_poll = Some(Instant::now());
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentApproval::list_pending_for_thread(&thread).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Approvals(thread, r));
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Thread(Ok(found)) => {
                    self.loading_thread = false;
                    let changed = found.as_ref().map(|t| &t.id) != self.thread.as_ref().map(|t| &t.id);
                    if changed {
                        self.events.clear();
                        self.approvals.clear();
                        self.last_seq = 0;
                        self.last_event_poll = None;
                        self.last_approval_poll = None;
                        self.scroll_back.set(0);
                    }
                    if found.as_ref().is_some_and(AgentThread::is_open) {
                        self.requested_at = None;
                    }
                    self.thread = found;
                }
                Msg::Thread(Err(e)) => {
                    self.loading_thread = false;
                    self.note = e;
                }
                Msg::Events(thread, Ok(rows)) => {
                    self.loading_events = false;
                    if self.thread.as_ref().map(|t| &t.id) != Some(&thread) {
                        continue;
                    }
                    for row in rows {
                        self.last_seq = self.last_seq.max(row.seq);
                        match self.events.iter_mut().find(|e| e.id == row.id) {
                            Some(existing) => *existing = row,
                            None => self.events.push(row),
                        }
                    }
                    self.events.sort_by_key(|e| e.seq);
                }
                Msg::Events(_, Err(e)) => {
                    self.loading_events = false;
                    self.note = e;
                }
                Msg::Approvals(thread, Ok(rows)) => {
                    self.loading_approvals = false;
                    if self.thread.as_ref().map(|t| &t.id) != Some(&thread) {
                        continue;
                    }
                    self.in_flight.retain(|id| rows.iter().any(|r| &r.id == id));
                    self.approvals = rows;
                }
                Msg::Approvals(_, Err(e)) => {
                    self.loading_approvals = false;
                    self.note = e;
                }
                Msg::Turn(Ok(())) => self.note = "sent".into(),
                Msg::Turn(Err(e)) => self.note = format!("send failed: {e}"),
                Msg::Requested(Ok(())) => self.note = "asked for the agent; waiting for the host to open the session".into(),
                Msg::Requested(Err(e)) => {
                    self.requested_at = None;
                    self.note = format!("request failed: {e}");
                }
                Msg::Decided(id, outcome) => {
                    self.in_flight.remove(&id);
                    self.note = match outcome {
                        Ok(AgentDecideOutcome::Recorded) => "decision recorded".into(),
                        Ok(AgentDecideOutcome::AlreadyResolved(held)) => format!("already {held} elsewhere"),
                        Ok(AgentDecideOutcome::Missing) => "that request was withdrawn".into(),
                        Err(e) => format!("decision failed: {e}"),
                    };
                    self.last_approval_poll = None;
                }
            }
        }
    }

    /// Schedules the periodic reads; called once per frame.
    fn tick(&mut self) {
        self.drain();
        self.frame.set(self.frame.get().wrapping_add(1));
        let thread_every = if self.requested_at.is_some() { THREAD_POLL_WAITING } else { THREAD_POLL };
        if self.last_thread_poll.is_none_or(|t| t.elapsed() >= thread_every) {
            self.poll_thread();
        }
        if self.thread.is_some() && self.last_event_poll.is_none_or(|t| t.elapsed() >= EVENT_POLL) {
            self.poll_events();
        }
        if self.open_thread().is_some() && self.last_approval_poll.is_none_or(|t| t.elapsed() >= APPROVAL_POLL) {
            self.poll_approvals();
        }
    }

    fn turn(&mut self, thread: RecordId, kind: &'static str, text: String) {
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentTurn::ask(&thread, kind, &text).await.map(|_| ()).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Turn(r));
        });
    }

    /// Files an assist request for this machine; the broker opens the session.
    fn request_session(&mut self, text: String) {
        let cs = self.connection_string.clone();
        let user = displays::get_current_user_from_auth();
        let tech = user.as_ref().map(|u| u.get_email().to_string());
        let store = user
            .as_ref()
            .and_then(|u| serde_json::to_value(u).ok())
            .and_then(|v| v.get("store").and_then(Value::as_str).map(str::to_string));
        self.requested_at = Some(Instant::now());
        self.note = "asking for the agent\u{2026}".into();
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AssistRequest::create_from_chat(&cs, tech.as_deref(), store.as_deref(), None, &text)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Requested(r));
        });
    }

    fn decide(&mut self, id: RecordId, status: &'static str, note: Option<String>, answers: Option<Value>) {
        if !self.in_flight.insert(id.clone()) {
            return;
        }
        let by = displays::get_current_user_from_auth().map(|u| u.get_id());
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentApproval::decide(&id, status, by, note, answers).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Decided(id, r));
        });
    }

    fn submit(&mut self) {
        let text = self.input.get_raw_text().trim().to_string();
        // Typed text answers the agent's open question before it is a message.
        if let Some(req) = self.active_approval().filter(|r| r.kind == "question") {
            if text.is_empty() {
                return;
            }
            let answers = transcript::question_answers(req.questions.as_ref(), &text);
            self.input.set_text("");
            self.decide(req.id, "answered", None, Some(answers));
            return;
        }
        if text.is_empty() {
            return;
        }
        self.input.set_text("");
        self.scroll_back.set(0);
        match self.open_thread().map(|t| (t.id.clone(), t.status.clone())) {
            Some((id, status)) => {
                // A message during a turn steers it; otherwise it starts the next one.
                let kind = if matches!(status.as_str(), "running" | "waiting_approval") { "steer" } else { "start" };
                self.turn(id, kind, text);
            }
            None => {
                if self.requested_at.is_some_and(|t| t.elapsed() < REQUEST_WAIT) {
                    self.note = "still waiting for the host to open the session".into();
                    return;
                }
                self.request_session(text);
            }
        }
    }

    /// Single-key decisions while the composer is empty; `Ctrl+N` declines with the typed note.
    fn approval_hotkey(&mut self, req: &AgentApproval, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        let typed = self.input.get_raw_text().trim().to_string();
        if req.kind == "question" {
            let KeyCode::Char(c) = key.code else { return false };
            if ctrl || !typed.is_empty() {
                return false;
            }
            let Some(n) = c.to_digit(10) else { return false };
            let Some(label) = transcript::option_label(req.questions.as_ref(), n as usize) else { return false };
            let answers = transcript::question_answers(req.questions.as_ref(), &label);
            self.decide(req.id.clone(), "answered", None, Some(answers));
            return true;
        }
        match key.code {
            KeyCode::Char('n') if ctrl => {
                self.input.set_text("");
                self.decide(req.id.clone(), "declined", (!typed.is_empty()).then_some(typed), None);
                true
            }
            _ if ctrl || !typed.is_empty() => false,
            KeyCode::Char('y') => {
                self.decide(req.id.clone(), "accepted", None, None);
                true
            }
            KeyCode::Char('s') if req.may_approve_for_session() => {
                self.decide(req.id.clone(), "accepted_for_session", None, None);
                true
            }
            KeyCode::Char('n') => {
                self.decide(req.id.clone(), "declined", None, None);
                true
            }
            KeyCode::Char('x') => {
                self.decide(req.id.clone(), "cancelled", None, None);
                true
            }
            _ => false,
        }
    }

    fn approval_lines(req: &AgentApproval, width: usize, busy: bool) -> Vec<Line<'static>> {
        let warn = Style::default().fg(THEME.warning);
        let muted = Style::default().fg(THEME.text_muted);
        let strong = Style::default().fg(THEME.text).add_modifier(Modifier::BOLD);
        let mut lines = Vec::new();
        let secs = req.secs_remaining();
        let expires = format!("expires in {}m {:02}s", secs / 60, secs % 60);
        if req.kind == "question" {
            for w in transcript::wrap(&req.summary, width) {
                lines.push(Line::from(Span::styled(w, strong)));
            }
            let questions = req.questions.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(q) = questions.first() {
                for (i, opt) in q.get("options").and_then(Value::as_array).into_iter().flatten().enumerate().take(9) {
                    let label = opt.get("label").and_then(Value::as_str).unwrap_or("");
                    let desc = opt.get("description").and_then(Value::as_str).unwrap_or("");
                    let text = if desc.is_empty() { format!("{}) {label}", i + 1) } else { format!("{}) {label} \u{2014} {desc}", i + 1) };
                    lines.push(Line::from(Span::styled(transcript::clip(&text, width), Style::default().fg(THEME.text))));
                }
            }
            lines.push(Line::from(Span::styled(
                transcript::clip(&format!("digit picks an option \u{00b7} or type an answer and press Enter \u{00b7} Esc later \u{00b7} {expires}"), width),
                muted,
            )));
        } else {
            for w in transcript::wrap(&req.summary, width) {
                lines.push(Line::from(Span::styled(w, strong)));
            }
            if let Some(args) = req.arguments.as_ref().filter(|a| !a.is_null()) {
                lines.push(Line::from(Span::styled(transcript::clip(&args.to_string(), width), muted)));
            }
            let session = if req.may_approve_for_session() { "  [s] approve for session" } else { "" };
            lines.push(Line::from(Span::styled(
                transcript::clip(&format!("[y] approve{session}  [n] decline  [Ctrl+N] decline with the typed note  [x] stop agent  [Esc] later"), width),
                warn,
            )));
            lines.push(Line::from(Span::styled(expires, muted)));
        }
        if busy {
            lines.push(Line::from(Span::styled("sending\u{2026}", muted)));
        }
        lines
    }

    fn empty_lines(&self) -> Vec<Line<'static>> {
        let muted = Style::default().fg(THEME.text_muted);
        let mut lines = vec![Line::from("")];
        match (&self.thread, self.requested_at) {
            (_, Some(at)) if at.elapsed() >= REQUEST_WAIT => lines.push(Line::from(Span::styled(
                "  No session opened after two minutes; check that admin-agent is running.",
                Style::default().fg(THEME.error),
            ))),
            (_, Some(_)) => lines.push(Line::from(Span::styled(
                "  Asking the agent host\u{2026} the broker opens the session within a few seconds.",
                muted,
            ))),
            (Some(t), None) => lines.push(Line::from(Span::styled(
                format!("  Session {} \u{00b7} {} \u{2014} the agent is starting up.", short_key(&t.id), status_word(&t.status)),
                muted,
            ))),
            (None, None) => {
                lines.push(Line::from(Span::styled("  This machine has no agent session yet.", muted)));
                lines.push(Line::from(Span::styled(
                    "  Type what you want checked and press Enter; the Codex agent on the admin host opens one.",
                    muted,
                )));
                lines.push(Line::from(Span::styled(
                    "  Anything that would run a command here waits for your approval in this pane.",
                    muted,
                )));
            }
        }
        lines
    }
}

impl<'a> HandleWidget<'a> for AssistantTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.tick();
        if let Some(thread) = &self.thread {
            displays::ui_data::agent_session_notify::mark_in_view(&thread.id);
        }
        let approval = self.active_approval();
        let busy = approval.as_ref().is_some_and(|r| self.in_flight.contains(&r.id));
        let inner_w = area.width.saturating_sub(2) as usize;
        let approval_lines = approval.as_ref().map(|r| Self::approval_lines(r, inner_w.max(8), busy));
        let approval_h = approval_lines
            .as_ref()
            .map(|l| (l.len() as u16 + 2).min(area.height / 2))
            .unwrap_or(0);

        let rows = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(approval_h),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(area);

        let running = self.running();
        let spinner = glyphs::SPINNER[self.frame.get() % glyphs::SPINNER.len()];
        let word = match (&self.thread, self.requested_at) {
            (_, Some(_)) => "asking".to_string(),
            (Some(t), None) => status_word(&t.status).to_string(),
            (None, None) => "no session".to_string(),
        };
        let title = if running || self.requested_at.is_some() {
            format!(" Agent \u{00b7} {} \u{00b7} {word} {spinner} ", self.hostname)
        } else {
            format!(" Agent \u{00b7} {} \u{00b7} {word} ", self.hostname)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(running))
            .title_style(THEME.title())
            .title(title);

        let inner_h = rows[0].height.saturating_sub(2) as usize;
        let lines = if self.events.is_empty() {
            self.empty_lines()
        } else {
            transcript::render(&self.events, inner_w, self.show_reasoning, spinner)
        };
        let total = lines.len();
        let max_back = total.saturating_sub(inner_h);
        let back = self.scroll_back.get().min(max_back);
        self.scroll_back.set(back);
        let end = total.saturating_sub(back);
        let start = end.saturating_sub(inner_h);
        let view: Vec<Line> = lines[start..end].to_vec();
        f.render_widget(Paragraph::new(view).block(block).style(Style::default().bg(THEME.bg)), rows[0]);

        if let (Some(req), Some(lines)) = (approval.as_ref(), approval_lines) {
            let title = if req.kind == "question" { " The agent asks " } else { " Approval needed " };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(SHORTCUT_SET)
                .border_style(Style::default().fg(THEME.warning))
                .title_style(Style::default().fg(THEME.warning).add_modifier(Modifier::BOLD))
                .title(title);
            f.render_widget(Paragraph::new(lines).block(block).style(Style::default().bg(THEME.bg)), rows[1]);
        }

        f.render_widget(&self.input, rows[2]);

        let mut footer = String::from(
            "Enter send  \u{00b7}  Alt+Enter newline  \u{00b7}  PgUp/PgDn scroll  \u{00b7}  Ctrl+T thinking  \u{00b7}  Ctrl+X close session",
        );
        if running {
            footer.push_str("  \u{00b7}  Esc stop");
        }
        if let Some(t) = &self.thread {
            footer.push_str(&format!("  \u{00b7}  session {}", short_key(&t.id)));
        }
        if !self.note.is_empty() {
            footer.push_str(&format!("  \u{00b7}  {}", self.note));
        }
        let muted = Style::default().fg(THEME.text_muted).bg(THEME.bg);
        let context = self.thread.as_ref().and_then(AgentThread::context_usage);
        let context_w = context.as_ref().map_or(0, |c| c.chars().count() as u16 + 1);
        let [hints, usage] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(context_w)]).areas(rows[3]);
        f.render_widget(Paragraph::new(transcript::clip(&footer, hints.width as usize)).style(muted), hints);
        if let Some(context) = context {
            f.render_widget(Paragraph::new(context).alignment(Alignment::Right).style(muted), usage);
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if let Some(req) = self.active_approval() {
            if self.approval_hotkey(&req, &key) {
                return true;
            }
        }
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
            KeyCode::Esc => {
                if let Some(req) = self.active_approval() {
                    self.snoozed.insert(req.id, Instant::now() + SNOOZE);
                } else if self.running() {
                    if let Some(id) = self.thread.as_ref().map(|t| t.id.clone()) {
                        self.turn(id, "interrupt", String::new());
                    }
                }
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
                self.show_reasoning = !self.show_reasoning;
                true
            }
            KeyCode::Char('x') if ctrl => {
                if let Some(id) = self.open_thread().map(|t| t.id.clone()) {
                    self.turn(id, "close", String::new());
                }
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

fn status_word(status: &str) -> &'static str {
    match status {
        "queued" => "queued",
        "starting" => "starting",
        "idle" => "idle",
        "running" => "working",
        "waiting_approval" => "needs approval",
        "closed" => "closed",
        "failed" => "failed",
        _ => "unknown",
    }
}

fn short_key(id: &RecordId) -> String {
    id.key_string().chars().take(8).collect()
}
