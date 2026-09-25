//! Terminal-mode view of this machine's Codex agent session. The broker in
//! admin-agent talks to codex; this page reads `agent_event`, queues
//! `agent_turn` rows and decides `agent_approval` rows, like the desktop tab.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    AgentApproval, AgentDecideOutcome, AgentEvent, AgentThread, AgentTurn, AssistRequest, RecordId, RecordIdExt,
};
use displays::{PlatformSpawner, Spawner};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Alignment, Constraint, Layout, Position, Rect},
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

mod json;
mod markdown;
mod syntax;
mod transcript;
mod wrap;

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

/// A folding row header on screen: its view row, event key and open state.
struct Hit {
    y: u16,
    key: String,
    open: bool,
}

/// Line `line` of the row keyed `key`, held on view row `at`.
#[derive(Clone)]
struct Anchor {
    key: String,
    line: usize,
    at: usize,
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
    /// Unfolds every tool, shell, file-change and approval row.
    expand: bool,
    transcript: transcript::Transcript,
    /// Open state of rows toggled by a click, by event key.
    toggled: RefCell<HashMap<String, bool>>,
    hits: RefCell<Vec<Hit>>,
    /// The first row line on screen.
    top: RefCell<Option<Anchor>>,
    /// The line to hold in place on the next redraw.
    anchor: RefCell<Option<Anchor>>,
    /// `scroll_back` as the last redraw left it.
    last_back: Cell<usize>,
    view: Cell<Rect>,
    page: Cell<usize>,
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
            expand: false,
            transcript: transcript::Transcript::default(),
            toggled: RefCell::new(HashMap::new()),
            hits: RefCell::new(Vec::new()),
            top: RefCell::new(None),
            anchor: RefCell::new(None),
            last_back: Cell::new(0),
            view: Cell::new(Rect::default()),
            page: Cell::new(5),
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
                        self.transcript.clear();
                        self.toggled.borrow_mut().clear();
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
                            Some(existing) if *existing != row => {
                                self.transcript.forget(&row.id);
                                *existing = row;
                            }
                            Some(_) => {}
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
        let text = self.input.get_text().join("\n").trim().to_string();
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

    /// The decision panel's lines within `budget` rows, cutting the arguments before the keys.
    fn approval_lines(req: &AgentApproval, width: usize, busy: bool, budget: usize) -> Vec<Line<'static>> {
        let warn = Style::default().fg(THEME.warning);
        let muted = Style::default().fg(THEME.text_muted);
        let strong = Style::default().fg(THEME.text).add_modifier(Modifier::BOLD);
        let secs = req.secs_remaining();
        let expires = format!("expires in {}m {:02}s", secs / 60, secs % 60);
        let mut lines = wrap::words(&[(strong, req.summary.as_str())], width, &[], &[]);
        let mut body = Vec::new();
        let mut keys = Vec::new();
        if req.kind == "question" {
            let questions = req.questions.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(q) = questions.first() {
                for (i, opt) in q.get("options").and_then(Value::as_array).into_iter().flatten().enumerate().take(9) {
                    let label = opt.get("label").and_then(Value::as_str).unwrap_or("");
                    let desc = opt.get("description").and_then(Value::as_str).unwrap_or("");
                    let text = if desc.is_empty() { format!("{}) {label}", i + 1) } else { format!("{}) {label} \u{2014} {desc}", i + 1) };
                    body.push(Line::from(Span::styled(wrap::clip(&text, width), Style::default().fg(THEME.text))));
                }
            }
            keys.push(Line::from(Span::styled(
                wrap::clip(&format!("digit picks an option \u{00b7} or type an answer and press Enter \u{00b7} Esc later \u{00b7} {expires}"), width),
                muted,
            )));
        } else {
            if let Some(args) = req.arguments.as_ref().filter(|a| !a.is_null()) {
                body = json::lines(args, width);
            }
            let session = if req.may_approve_for_session() { "  [s] approve for session" } else { "" };
            keys.push(Line::from(Span::styled(
                wrap::clip(&format!("[y] approve{session}  [n] decline  [Ctrl+N] decline with the typed note  [x] stop agent  [Esc] later"), width),
                warn,
            )));
            keys.push(Line::from(Span::styled(expires, muted)));
        }
        if busy {
            keys.push(Line::from(Span::styled("sending\u{2026}", muted)));
        }
        let room = budget.saturating_sub(lines.len() + keys.len());
        if body.len() > room {
            let keep = room.saturating_sub(1);
            let hidden = body.len() - keep;
            body.truncate(keep);
            if room > 0 {
                body.push(Line::from(Span::styled(format!("{} {hidden} more lines", glyphs::ELLIPSIS), muted)));
            }
        }
        lines.extend(body);
        lines.extend(keys);
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

    /// Drops click toggles after a fold mode changes, keeping the top row in place when scrolled back.
    fn refold(&self) {
        self.toggled.borrow_mut().clear();
        if self.scroll_back.get() > 0 {
            *self.anchor.borrow_mut() = self.top.borrow().clone();
        }
    }

    /// Folds or unfolds the row whose header is at `(x, y)`; false when no header is there.
    fn toggle_at(&self, x: u16, y: u16) -> bool {
        let view = self.view.get();
        if !view.contains(Position::new(x, y)) {
            return false;
        }
        let hits = self.hits.borrow();
        let Some(hit) = hits.iter().find(|h| h.y == y) else { return false };
        self.toggled.borrow_mut().insert(hit.key.clone(), !hit.open);
        *self.anchor.borrow_mut() = Some(Anchor { key: hit.key.clone(), line: 0, at: usize::from(y - view.y) });
        true
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
        let approval_budget = (area.height / 2).saturating_sub(2) as usize;
        let approval_lines =
            approval.as_ref().map(|r| Self::approval_lines(r, inner_w.max(8), busy, approval_budget));
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

        let inner = block.inner(rows[0]);
        let inner_h = inner.height as usize;
        let opts = transcript::Options {
            width: inner_w,
            show_reasoning: self.show_reasoning,
            expand: self.expand,
            spinner,
        };
        let (body, tail) = if self.events.is_empty() {
            (Vec::new(), self.empty_lines())
        } else {
            (self.transcript.rows(&self.events, &opts, &self.toggled.borrow()), Vec::new())
        };
        let blank = Line::default();
        let slots = transcript::flatten(&body, &tail, &blank);
        let total = slots.len();
        let asked = self.scroll_back.get();
        let mut back = asked.min(total.saturating_sub(inner_h));
        let held = if asked > 0 && asked == self.last_back.get() { self.top.borrow().clone() } else { None };
        let anchor = self.anchor.borrow_mut().take().or(held);
        if let Some(a) = anchor
            && let Some(i) = slots
                .iter()
                .position(|s| s.is(&a.key, a.line))
                .or_else(|| slots.iter().position(|s| s.is(&a.key, 0)))
        {
            back = anchored_back(total, inner_h, i, a.at);
        }
        self.scroll_back.set(back);
        self.last_back.set(back);
        let end = total - back;
        let start = end.saturating_sub(inner_h);
        let mut hits = Vec::new();
        let mut top = None;
        for (i, slot) in slots[start..end].iter().enumerate() {
            let Some(row) = slot.row else { continue };
            if top.is_none() {
                top = Some(Anchor { key: row.key.clone(), line: slot.index, at: i });
            }
            if let Some(open) = row.fold.filter(|_| slot.is_head()) {
                hits.push(Hit { y: inner.y + i as u16, key: row.key.clone(), open });
            }
        }
        *self.hits.borrow_mut() = hits;
        *self.top.borrow_mut() = top;
        self.view.set(inner);
        self.page.set(inner_h.saturating_sub(2).max(1));
        let view: Vec<Line> = slots[start..end].iter().map(|s| s.line.clone()).collect();
        let block = if back > 0 {
            block.title_bottom(Line::from(format!(" {} {back} more below \u{00b7} PgDn ", glyphs::SCROLL_DOWN)).right_aligned())
        } else {
            block
        };
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

        let expand = if self.expand { "Ctrl+O collapse tools" } else { "Ctrl+O expand tools" };
        let mut footer = format!(
            "Enter send  \u{00b7}  Alt+Enter newline  \u{00b7}  {expand}  \u{00b7}  Ctrl+T thinking  \u{00b7}  PgUp/PgDn scroll  \u{00b7}  Ctrl+X close session",
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
        f.render_widget(Paragraph::new(wrap::clip(&footer, hints.width as usize)).style(muted), hints);
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
                self.scroll_back.set(self.scroll_back.get().saturating_add(self.page.get()));
                true
            }
            KeyCode::PageDown => {
                self.scroll_back.set(self.scroll_back.get().saturating_sub(self.page.get()));
                true
            }
            KeyCode::Char('o') if ctrl => {
                self.expand = !self.expand;
                self.refold();
                true
            }
            KeyCode::Char('t') if ctrl => {
                self.show_reasoning = !self.show_reasoning;
                self.refold();
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
            MouseEventKind::Down(MouseButton::Left) if self.toggle_at(mouse_event.column, mouse_event.row) => {}
            _ => ButtonType::handle_mouse_event(&self.input, mouse_event),
        }
    }
}

/// Lines scrolled back from the end that put line `index` on view row `at` of a `height`-row view over `total` lines.
fn anchored_back(total: usize, height: usize, index: usize, at: usize) -> usize {
    total.saturating_sub(index.saturating_sub(at) + height)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchored_line_stays_on_its_view_row() {
        assert_eq!(anchored_back(100, 10, 50, 3), 43);
        assert_eq!(anchored_back(100, 10, 95, 0), 0);
        assert_eq!(anchored_back(8, 10, 2, 5), 0);
        assert_eq!(anchored_back(100, 10, 2, 5), 90);
    }
}
