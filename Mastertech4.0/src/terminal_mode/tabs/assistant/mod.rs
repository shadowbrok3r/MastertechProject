//! Terminal-mode view of this machine's Codex agent session. The broker in
//! admin-agent talks to codex; this page reads `agent_event`, queues
//! `agent_turn` rows and decides `agent_approval` rows, like the desktop tab.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    AgentApproval, AgentDecideOutcome, AgentEvent, AgentThread, AgentTurn, ApprovalAudience, ApprovalViewer,
    AssistRequest, RecordId, RecordIdExt,
};
use displays::{PlatformSpawner, Spawner};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Alignment, Constraint, Layout, Position, Rect},
    prelude::Backend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
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
/// How long a first Ctrl+L waits for the second that closes the session.
const CONFIRM_WINDOW: Duration = Duration::from_secs(3);
/// How long a status note stays in the footer.
const NOTE_SHOWN: Duration = Duration::from_secs(10);
const START_HINT: &str = "Type a message and press Enter to start a new session";
const NOT_PERMITTED: &str = "only this session's technician or a Root user can decide";
const NOT_YOURS: &str = "this session belongs to another technician; type a message to start your own";
const REMOTE_REFUSED: &str = "a remote viewer cannot decide, send, stop or close here";
const OTHERS_SESSION_TITLE: &str = "Start your own session (this one is another technician's)";

enum Msg {
    Thread(Result<Option<AgentThread>, String>),
    Events(RecordId, Result<Vec<AgentEvent>, String>),
    Approvals(RecordId, Result<Vec<AgentApproval>, String>),
    Turn(&'static str, Result<(), String>),
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

/// A new session the technician asked for with Ctrl+L: the thread it replaces and when.
struct Fresh {
    replaces: Option<RecordId>,
    at: Instant,
}

/// What Ctrl+L does.
#[derive(Debug, PartialEq)]
enum NewSession {
    /// A request for a session is already in flight.
    Requesting,
    /// The session being replaced has not closed yet.
    Closing,
    /// First press over an open session.
    Confirm,
    /// Close this session and clear the view.
    Close(RecordId),
    /// Nothing is open; clear the view.
    Clear,
}

fn new_session_step(requesting: bool, closing: bool, open: Option<&RecordId>, confirmed: bool) -> NewSession {
    match open {
        _ if requesting => NewSession::Requesting,
        _ if closing => NewSession::Closing,
        Some(id) if confirmed => NewSession::Close(id.clone()),
        Some(_) => NewSession::Confirm,
        None => NewSession::Clear,
    }
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
    /// Who decides from this panel; kept while the user lock is busy.
    viewer: Option<ApprovalViewer>,
    last_viewer_read: Option<Instant>,
    /// The key being handled came from a remote viewer.
    remote_input: bool,
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
    fresh: Option<Fresh>,
    /// When Ctrl+L was first pressed over an open session.
    armed_at: Option<Instant>,
    note: String,
    note_at: Option<Instant>,
    requested_at: Option<Instant>,
    /// The message that asked for the new session.
    requested_text: String,
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
        Self::for_connection(crate::filesystem::get_client_hash().connection_string)
    }

    fn for_connection(connection_string: String) -> Self {
        let input = InputField::new("Message the agent", WidgetId("AssistantInput".to_string()));
        input.set_state(ButtonState::Active);
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
            viewer: None,
            last_viewer_read: None,
            remote_input: false,
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
            fresh: None,
            armed_at: None,
            note: String::new(),
            note_at: None,
            requested_at: None,
            requested_text: String::new(),
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

    /// True while the latest thread is the one a new session replaces.
    fn replacing(&self) -> bool {
        self.fresh
            .as_ref()
            .is_some_and(|f| f.replaces.as_ref() == self.thread.as_ref().map(|t| &t.id))
    }

    /// True while the thread a new session replaces is still open.
    fn closing(&self) -> bool {
        self.replacing() && self.open_thread().is_some()
    }

    /// True when the viewer is the open session's technician or an active Root.
    fn may_steer(&self) -> bool {
        self.open_thread()
            .is_some_and(|t| self.viewer.as_ref().is_some_and(|v| v.may_steer(t.assignee.as_ref())))
    }

    /// True while an open session is showing that the viewer may not steer.
    fn watching_others(&self) -> bool {
        self.open_thread().is_some() && !self.replacing() && !self.may_steer()
    }

    /// True when the next message starts a new session.
    fn starts_session(&self) -> bool {
        self.open_thread().is_none() || self.replacing() || !self.may_steer()
    }

    /// Handles a key, refusing decisions and session actions when it came from a remote viewer.
    pub fn handle_key_from(&mut self, key: KeyEvent, remote: bool) -> bool {
        self.remote_input = remote;
        let consumed = self.handle_key_event(key);
        self.remote_input = false;
        consumed
    }

    fn set_note(&mut self, note: impl Into<String>) {
        self.note = note.into();
        self.note_at = Some(Instant::now());
    }

    /// The decision shown right now: the oldest pending one the viewer may decide that is neither snoozed nor lapsed.
    fn active_approval(&self) -> Option<AgentApproval> {
        if self.replacing() {
            return None;
        }
        let now = Instant::now();
        self.approvals
            .iter()
            .find(|r| {
                r.secs_remaining() > 0
                    && r.audience_for(self.viewer.as_ref()) != ApprovalAudience::Hidden
                    && !self.snoozed.get(&r.id).is_some_and(|until| *until > now)
            })
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
        if self.viewer.is_none() {
            return;
        }
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
                        self.fresh = None;
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
                    self.set_note(e);
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
                    self.set_note(e);
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
                    self.set_note(e);
                }
                Msg::Turn(kind, Ok(())) => self.set_note(match kind {
                    "close" => "closing the session",
                    "interrupt" => "asked the agent to stop",
                    _ => "sent",
                }),
                Msg::Turn(kind, Err(e)) => {
                    if kind == "close" {
                        self.fresh = None;
                    }
                    let what = match kind {
                        "close" => "close",
                        "interrupt" => "stop",
                        _ => "send",
                    };
                    self.set_note(format!("{what} failed: {e}"));
                }
                Msg::Requested(Ok(())) => self.set_note("asked for the agent; waiting for the host to open the session"),
                Msg::Requested(Err(e)) => {
                    self.requested_at = None;
                    if self.input.get_raw_text().is_empty() {
                        self.input.set_text(&self.requested_text);
                    }
                    self.set_note(format!("request failed: {e}"));
                }
                Msg::Decided(id, outcome) => {
                    self.in_flight.remove(&id);
                    self.set_note(match outcome {
                        Ok(AgentDecideOutcome::Recorded) => "decision recorded".into(),
                        Ok(AgentDecideOutcome::AlreadyResolved(held)) => format!("already {held} elsewhere"),
                        Ok(AgentDecideOutcome::Missing) => {
                            "that request was withdrawn, or you are not permitted to decide it".into()
                        }
                        Ok(AgentDecideOutcome::NotPermitted) => NOT_PERMITTED.into(),
                        Err(e) => format!("decision failed: {e}"),
                    });
                    self.last_approval_poll = None;
                }
            }
        }
    }

    /// Schedules the periodic reads; called once per frame.
    fn tick(&mut self) {
        self.drain();
        self.frame.set(self.frame.get().wrapping_add(1));
        if self.last_viewer_read.is_none_or(|t| t.elapsed() >= APPROVAL_POLL)
            && let Some(viewer) = ApprovalViewer::signed_in()
        {
            self.viewer = viewer;
            self.last_viewer_read = Some(Instant::now());
        }
        if self.closing() && self.fresh.as_ref().is_some_and(|f| f.at.elapsed() >= REQUEST_WAIT) {
            self.fresh = None;
            self.set_note("the session did not close; check that admin-agent is running");
        }
        let waiting = self.requested_at.is_some() || self.closing();
        let thread_every = if waiting { THREAD_POLL_WAITING } else { THREAD_POLL };
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
            let _ = tx.send(Msg::Turn(kind, r));
        });
    }

    /// Files an assist request for this machine; the broker opens the session, a new one when another technician's is open.
    fn request_session(&mut self, text: String) {
        let fresh = self.open_thread().is_some();
        let cs = self.connection_string.clone();
        let user = displays::get_current_user_from_auth();
        let tech = user.as_ref().map(|u| u.get_email().to_string());
        let store = user
            .as_ref()
            .and_then(|u| serde_json::to_value(u).ok())
            .and_then(|v| v.get("store").and_then(Value::as_str).map(str::to_string));
        self.requested_at = Some(Instant::now());
        self.requested_text = text.clone();
        self.set_note("asking for the agent\u{2026}");
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AssistRequest::create_from_chat(&cs, tech.as_deref(), store.as_deref(), None, &text, fresh)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Requested(r));
        });
    }

    fn decide(&mut self, id: RecordId, status: &'static str, note: Option<String>, answers: Option<Value>) {
        let permitted = self
            .approvals
            .iter()
            .find(|r| r.id == id)
            .is_some_and(|r| r.audience_for(self.viewer.as_ref()) != ApprovalAudience::Hidden);
        if !permitted {
            self.set_note(NOT_PERMITTED);
            return;
        }
        if !self.in_flight.insert(id.clone()) {
            return;
        }
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentApproval::decide(&id, status, note, answers).await.map_err(|e| e.to_string());
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
        if self.closing() {
            self.set_note("the previous session is still closing; press Enter again in a moment");
            return;
        }
        match self.open_thread().map(|t| (t.id.clone(), t.status.clone())) {
            Some((id, status)) if self.may_steer() => {
                // A message during a turn steers it; otherwise it starts the next one.
                let kind = if matches!(status.as_str(), "running" | "waiting_approval") { "steer" } else { "start" };
                self.input.set_text("");
                self.scroll_back.set(0);
                self.turn(id, kind, text);
            }
            _ => {
                if self.requested_at.is_some_and(|t| t.elapsed() < REQUEST_WAIT) {
                    self.set_note("still waiting for the host to open the session");
                    return;
                }
                self.input.set_text("");
                self.scroll_back.set(0);
                self.request_session(text);
            }
        }
    }

    /// Ctrl+L: closes the open session on a second press within [`CONFIRM_WINDOW`], then clears the view for a new one.
    fn new_session(&mut self) {
        if self.watching_others() {
            self.set_note(NOT_YOURS);
            return;
        }
        let requesting = self.requested_at.is_some_and(|t| t.elapsed() < REQUEST_WAIT);
        let armed = self.armed_at.is_some_and(|t| t.elapsed() < CONFIRM_WINDOW);
        let open = self.open_thread().map(|t| t.id.clone());
        match new_session_step(requesting, self.closing(), open.as_ref(), armed) {
            NewSession::Requesting => self.set_note("a new session is already being opened"),
            NewSession::Closing => self.set_note("the previous session is still closing"),
            NewSession::Confirm => {
                self.armed_at = Some(Instant::now());
                self.set_note("press Ctrl+L again to close this session and start a new one");
            }
            NewSession::Close(id) => {
                self.turn(id, "close", String::new());
                self.last_thread_poll = None;
                self.clear_for_new_session();
            }
            NewSession::Clear => {
                self.set_note("new session: type a message and press Enter");
                self.clear_for_new_session();
            }
        }
    }

    fn clear_for_new_session(&mut self) {
        self.armed_at = None;
        self.requested_at = None;
        self.fresh = Some(Fresh { replaces: self.thread.as_ref().map(|t| t.id.clone()), at: Instant::now() });
        self.scroll_back.set(0);
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

    /// Lines under the transcript: a request in flight, a closing or ended session, or the first-run text.
    fn tail_lines(&self, width: usize, spinner: &str) -> Vec<Line<'static>> {
        let muted = Style::default().fg(THEME.text_muted);
        let error = Style::default().fg(THEME.error);
        let say = |text: &str, style: Style| wrap::words(&[(style, text)], width, &[], &[]);
        let start = || say(START_HINT, Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD));
        let mut out = Vec::new();
        if let Some(at) = self.requested_at {
            if at.elapsed() >= REQUEST_WAIT {
                out.extend(say("No session opened after two minutes; check that admin-agent is running.", error));
                out.extend(start());
            } else {
                out.push(transcript::divider("New session", width));
                out.extend(transcript::draft(&self.requested_text, width));
                out.push(Line::default());
                let waiting = format!("{spinner} Opening a new session; the agent host picks it up within a few seconds.");
                out.extend(say(&waiting, muted));
            }
            return out;
        }
        if self.closing() {
            out.extend(say(&format!("{spinner} Closing the previous session{}", glyphs::ELLIPSIS), muted));
            out.extend(start());
            return out;
        }
        if self.replacing() {
            out.extend(start());
            return out;
        }
        match &self.thread {
            Some(t) if !t.is_open() => {
                let caption = if t.status == "failed" { "Session failed" } else { "Session closed" };
                out.push(transcript::divider(caption, width));
                if let Some(e) = t.error.as_deref().filter(|e| !e.trim().is_empty()) {
                    out.extend(say(e, error));
                }
                out.extend(start());
            }
            Some(t) if self.events.is_empty() => {
                let starting = format!(
                    "Session {} {} {} \u{2014} the agent is starting up.",
                    short_key(&t.id),
                    glyphs::DOT,
                    status_word(&t.status)
                );
                out.extend(say(&starting, muted));
            }
            Some(_) => {}
            None => {
                out.extend(say("This machine has no agent session yet.", muted));
                out.extend(say(
                    "The Codex agent on the admin host opens one for your first message. Anything that would run a command here waits for your approval in this pane.",
                    muted,
                ));
                out.extend(start());
            }
        }
        out
    }

    /// Key hints that fit in `width` cells, after the new-session hint when the next message starts one.
    fn footer_line(&self, width: usize, running: bool) -> Line<'static> {
        let key = Style::default().fg(THEME.text).add_modifier(Modifier::BOLD);
        let action = Style::default().fg(THEME.text_muted);
        let starts = self.starts_session() && self.requested_at.is_none();
        let mut spans = Vec::new();
        if starts {
            spans.push(Span::styled(START_HINT, Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD)));
        }
        let mut hints = Vec::new();
        if !starts {
            hints.push(("Enter", "send"));
        }
        if running && !self.replacing() && self.may_steer() {
            hints.push(("Esc", "stop"));
        }
        hints.push(("Ctrl+O", if self.expand { "collapse tools" } else { "expand tools" }));
        hints.push(("Ctrl+T", if self.show_reasoning { "hide thinking" } else { "show thinking" }));
        if !self.watching_others() {
            hints.push(("Ctrl+L", "new session"));
        }
        if self.open_thread().is_some() && !self.replacing() && self.may_steer() {
            hints.push(("Ctrl+X", "close session"));
        }
        hints.push(("PgUp/PgDn", "scroll"));
        hints.push(("Alt+Enter", "newline"));
        let mut used = wrap::spans_width(&spans);
        for (k, a) in hints {
            let sep = if spans.is_empty() { 0 } else { 3 };
            let w = sep + wrap::width(k) + 1 + wrap::width(a);
            if used + w > width {
                break;
            }
            if sep > 0 {
                spans.push(Span::styled(format!(" {} ", glyphs::DOT), action));
            }
            spans.push(Span::styled(k, key));
            spans.push(Span::styled(format!(" {a}"), action));
            used += w;
        }
        Line::from(spans)
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
        let word = if self.requested_at.is_some() {
            "opening a new session".to_string()
        } else if self.closing() {
            "closing".to_string()
        } else if self.replacing() {
            "new session".to_string()
        } else {
            match &self.thread {
                Some(t) => format!("{} {} {}", status_word(&t.status), glyphs::DOT, short_key(&t.id)),
                None => "no session".to_string(),
            }
        };
        let title = if running || self.requested_at.is_some() || self.closing() {
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
        let tail = self.tail_lines(inner_w.max(8), spinner);
        let body = if self.replacing() {
            Vec::new()
        } else {
            self.transcript.rows(&self.events, &opts, &self.toggled.borrow())
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

        let input_title = if approval.as_ref().is_some_and(|r| r.kind == "question") {
            "Answer the agent"
        } else if self.watching_others() {
            OTHERS_SESSION_TITLE
        } else if self.starts_session() {
            "Start a new session"
        } else if running {
            "Message the agent while it works"
        } else {
            "Message the agent"
        };
        let (_, title_color, _, _) = self.input.colors();
        self.input.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::styled(input_title, Style::default().fg(title_color))),
        );
        f.render_widget(&self.input, rows[2]);

        let muted = Style::default().fg(THEME.text_muted).bg(THEME.bg);
        let mut right = Vec::new();
        if let Some(note) = self.note_at.filter(|t| t.elapsed() < NOTE_SHOWN).map(|_| self.note.as_str()) {
            let note = wrap::clip(&wrap::one_line(note), (rows[3].width / 2) as usize);
            right.push(Span::styled(note, Style::default().fg(THEME.accent_soft)));
        }
        if let Some(context) = self.thread.as_ref().and_then(AgentThread::context_usage) {
            if !right.is_empty() {
                right.push(Span::styled(format!(" {} ", glyphs::DOT), muted));
            }
            right.push(Span::styled(context, muted));
        }
        let right_w = if right.is_empty() { 0 } else { wrap::spans_width(&right) as u16 + 1 };
        let [hints, usage] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_w)]).areas(rows[3]);
        f.render_widget(Paragraph::new(self.footer_line(hints.width as usize, running)).style(muted), hints);
        f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right).style(muted), usage);
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if !self.remote_input
            && let Some(req) = self.active_approval()
            && self.approval_hotkey(&req, &key)
        {
            return true;
        }
        let session_key = match key.code {
            KeyCode::Enter => !alt,
            KeyCode::Esc => true,
            KeyCode::Char('x' | 'l' | 'n') => ctrl,
            _ => false,
        };
        if self.remote_input && session_key {
            self.set_note(REMOTE_REFUSED);
            return true;
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
                } else if self.running() && self.may_steer() {
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
                if self.watching_others() {
                    self.set_note(NOT_YOURS);
                } else if let Some(id) = self.open_thread().map(|t| t.id.clone()) {
                    self.turn(id, "close", String::new());
                }
                true
            }
            KeyCode::Char('l') if ctrl => {
                self.new_session();
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

    fn thread(key: &str, status: &str) -> AgentThread {
        AgentThread {
            id: RecordId::new("agent_thread", key),
            status: status.into(),
            connection_string: "PC-1:abc".into(),
            hostname: None,
            service_number: None,
            store: None,
            requested_by: None,
            assignee: Some(RecordId::new("user", "tech")),
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
            tokens_used: None,
            tokens_window: None,
            activity: None,
            last_seq: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    fn event(thread: &AgentThread) -> AgentEvent {
        AgentEvent {
            id: RecordId::new("agent_event", "e1"),
            thread: thread.id.clone(),
            seq: 1,
            turn_id: None,
            item_id: None,
            kind: "agent".into(),
            text: "hello".into(),
            done: true,
            item: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn tab(thread: Option<AgentThread>) -> AssistantTab<'static> {
        let mut tab = AssistantTab::for_connection("PC-1:abc".into());
        if let Some(t) = &thread {
            tab.events.push(event(t));
        }
        tab.thread = thread;
        tab.viewer = viewer("tech", false);
        tab
    }

    fn text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn footer(tab: &AssistantTab<'_>) -> String {
        text(&[tab.footer_line(400, tab.running())])
    }

    #[test]
    fn an_anchored_line_stays_on_its_view_row() {
        assert_eq!(anchored_back(100, 10, 50, 3), 43);
        assert_eq!(anchored_back(100, 10, 95, 0), 0);
        assert_eq!(anchored_back(8, 10, 2, 5), 0);
        assert_eq!(anchored_back(100, 10, 2, 5), 90);
    }

    #[test]
    fn ctrl_l_confirms_before_closing_and_waits_while_busy() {
        let id = RecordId::new("agent_thread", "t1");
        assert_eq!(new_session_step(false, false, Some(&id), false), NewSession::Confirm);
        assert_eq!(new_session_step(false, false, Some(&id), true), NewSession::Close(id.clone()));
        assert_eq!(new_session_step(false, false, None, false), NewSession::Clear);
        assert_eq!(new_session_step(true, false, Some(&id), true), NewSession::Requesting);
        assert_eq!(new_session_step(false, true, Some(&id), true), NewSession::Closing);
    }

    #[test]
    fn an_open_session_sends_and_lists_its_keys() {
        let tab = tab(Some(thread("t1", "idle")));
        assert!(!tab.starts_session());
        let keys = footer(&tab);
        for hint in ["Enter send", "Ctrl+O expand tools", "Ctrl+T show thinking", "Ctrl+L new session", "Ctrl+X close session"] {
            assert!(keys.contains(hint), "{keys}");
        }
        assert!(!keys.contains(START_HINT));
        assert!(tab.tail_lines(80, "*").is_empty());
        let busy = self::tab(Some(thread("t1", "running")));
        assert!(footer(&busy).contains("Esc stop"));
    }

    #[test]
    fn a_closed_or_failed_session_says_how_to_start_a_new_one() {
        let closed = tab(Some(thread("t1", "closed")));
        assert!(closed.starts_session());
        assert!(footer(&closed).starts_with(START_HINT));
        assert!(footer(&closed).contains("Ctrl+L new session"));
        let tail = text(&closed.tail_lines(80, "*"));
        assert!(tail.contains(" Session closed "), "{tail}");
        assert!(tail.contains(START_HINT), "{tail}");

        let mut failed = thread("t2", "failed");
        failed.error = Some("codex exited".into());
        let tail = text(&tab(Some(failed)).tail_lines(80, "*"));
        assert!(tail.contains(" Session failed ") && tail.contains("codex exited"), "{tail}");

        let none = tab(None);
        assert!(text(&none.tail_lines(80, "*")).contains(START_HINT));
        assert!(footer(&none).starts_with(START_HINT));
    }

    #[test]
    fn a_new_session_hides_the_old_one_until_it_has_closed() {
        let mut tab = tab(Some(thread("t1", "idle")));
        tab.fresh = Some(Fresh { replaces: tab.thread.as_ref().map(|t| t.id.clone()), at: Instant::now() });
        assert!(tab.replacing() && tab.closing() && tab.starts_session());
        assert!(tab.active_approval().is_none());
        let tail = text(&tab.tail_lines(80, "*"));
        assert!(tail.contains("Closing the previous session") && tail.contains(START_HINT), "{tail}");
        assert!(!footer(&tab).contains("Ctrl+X"));

        tab.thread = Some(thread("t1", "closed"));
        assert!(tab.replacing() && !tab.closing());
        assert_eq!(text(&tab.tail_lines(80, "*")), START_HINT);
    }

    fn approval(thread: &AgentThread, owner: Option<&str>) -> AgentApproval {
        AgentApproval {
            id: RecordId::new("agent_approval", "a1"),
            thread: thread.id.clone(),
            kind: "tool_call".into(),
            method: String::new(),
            codex_request_id: String::new(),
            summary: "run desktop_click on PC-1".into(),
            server: None,
            tool: Some("desktop_click".into()),
            arguments: None,
            params: None,
            questions: None,
            answers: None,
            response_sent: None,
            status: "pending".into(),
            assignee: owner.map(|k| RecordId::new("user", k)),
            connection_string: Some("PC-1:abc".into()),
            store: Some("MUR".into()),
            requested_at: None,
            expires_at: database::schema::Datetime::from_timestamp(database::schema::Datetime::now().timestamp() + 600, 0),
            decided_at: None,
            sent_to_codex_at: None,
            decided_by: None,
            deny_note: None,
        }
    }

    fn viewer(key: &str, root: bool) -> Option<ApprovalViewer> {
        Some(ApprovalViewer { id: RecordId::new("user", key), root })
    }

    #[test]
    fn only_the_owner_or_an_active_root_gets_the_decision_panel() {
        let t = thread("t1", "waiting_approval");
        let mut tab = tab(Some(t.clone()));
        tab.approvals.push(approval(&t, Some("tech")));
        tab.viewer = None;
        assert!(tab.active_approval().is_none(), "nobody signed in");
        tab.viewer = viewer("mate", false);
        assert!(tab.active_approval().is_none(), "another technician");
        tab.viewer = viewer("gone", false);
        assert!(tab.active_approval().is_none(), "an inactive Root reads as root = false");
        tab.viewer = viewer("tech", false);
        assert!(tab.active_approval().is_some(), "the owner");
        tab.viewer = viewer("boss", true);
        assert!(tab.active_approval().is_some(), "an active Root");
    }

    #[test]
    fn a_non_owner_cannot_decide_by_hotkey() {
        let t = thread("t1", "waiting_approval");
        let mut tab = tab(Some(t.clone()));
        let req = approval(&t, Some("tech"));
        tab.approvals.push(req.clone());
        tab.viewer = viewer("mate", false);
        tab.decide(req.id.clone(), "accepted", None, None);
        assert!(tab.in_flight.is_empty());
        assert_eq!(tab.note, NOT_PERMITTED);
    }

    #[test]
    fn a_remote_key_never_decides_sends_or_closes() {
        let t = thread("t1", "waiting_approval");
        let mut tab = tab(Some(t.clone()));
        tab.approvals.push(approval(&t, Some("tech")));
        for key in [KeyCode::Char('y'), KeyCode::Char('s')] {
            tab.handle_key_from(KeyEvent::new(key, KeyModifiers::NONE), true);
        }
        assert!(tab.in_flight.is_empty(), "no decision went out");
        tab.input.set_text("");
        tab.input.set_text("run it");
        for (code, mods) in [
            (KeyCode::Enter, KeyModifiers::NONE),
            (KeyCode::Esc, KeyModifiers::NONE),
            (KeyCode::Char('x'), KeyModifiers::CONTROL),
            (KeyCode::Char('l'), KeyModifiers::CONTROL),
            (KeyCode::Char('n'), KeyModifiers::CONTROL),
        ] {
            tab.note.clear();
            assert!(tab.handle_key_from(KeyEvent::new(code, mods), true));
            assert_eq!(tab.note, REMOTE_REFUSED, "{code:?}");
        }
        assert_eq!(tab.input.get_raw_text(), "run it", "the message stays unsent");
        assert!(tab.snoozed.is_empty() && tab.fresh.is_none() && tab.armed_at.is_none());
        assert!(!tab.remote_input, "the flag covers one key only");
    }

    #[test]
    fn another_technicians_session_is_watched_not_steered() {
        let mut tab = tab(Some(thread("t1", "running")));
        tab.viewer = viewer("mate", false);
        assert!(!tab.may_steer() && tab.watching_others() && tab.starts_session());
        let keys = footer(&tab);
        assert!(keys.starts_with(START_HINT), "{keys}");
        for hidden in ["Esc stop", "Ctrl+X", "Ctrl+L", "Enter send"] {
            assert!(!keys.contains(hidden), "{keys}");
        }
        tab.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        assert_eq!(tab.note, NOT_YOURS);
        tab.note.clear();
        tab.handle_key_event(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
        assert_eq!(tab.note, NOT_YOURS);
        assert!(tab.fresh.is_none());

        tab.viewer = viewer("boss", true);
        assert!(tab.may_steer() && !tab.starts_session());
        assert!(footer(&tab).contains("Ctrl+X close session"));
    }

    #[test]
    fn an_unowned_request_reaches_root_only() {
        let t = thread("t1", "waiting_approval");
        let mut tab = tab(Some(t.clone()));
        tab.approvals.push(approval(&t, None));
        tab.viewer = viewer("tech", false);
        assert!(tab.active_approval().is_none());
        tab.viewer = viewer("boss", true);
        assert!(tab.active_approval().is_some());
    }

    #[test]
    fn a_requested_session_echoes_the_message_while_it_opens() {
        let mut tab = tab(Some(thread("t1", "closed")));
        tab.requested_at = Some(Instant::now());
        tab.requested_text = "fans are **loud**".into();
        let tail = text(&tab.tail_lines(80, "*"));
        assert!(tail.contains(" New session "), "{tail}");
        assert!(tail.contains("You") && tail.contains("fans are loud"), "{tail}");
        assert!(tail.contains("Opening a new session"), "{tail}");
        assert!(!footer(&tab).contains(START_HINT));
    }
}
