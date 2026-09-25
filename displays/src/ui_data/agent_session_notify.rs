//! Toasts the technician who asked for an agent session when its turn ends, fails or waits on them.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::live_data::{listen_data_filtered, Action};
use database::schema::user::UserAuthorization;
use database::schema::{AgentEvent, AgentThread, Datetime, RecordId, RecordIdExt, User};
use eframe::egui::{self, Align, Context, Frame, Layout, Margin, Response, RichText, Sense, Ui, Vec2};
use futures::future::AbortHandle;
use serde_json::Value;
use web_time::Instant;

use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts};
use crate::ui_tools::{do_not_disturb, icons, theme};
use crate::{PlatformSpawner, Spawner};

/// `ToastKind::Custom` discriminant for agent-session toasts.
pub const AGENT_TOAST_KIND: u32 = 0x7A5C_0002;

const SNAPSHOT_EVERY: Duration = Duration::from_secs(30);
const SNAPSHOT_LIMIT: usize = 50;
const STREAM_RETRY: Duration = Duration::from_secs(3);
/// How recently a view must have drawn a thread for it to count as on screen.
const IN_VIEW_FOR: Duration = Duration::from_millis(1500);
const DETAIL_CHARS: usize = 160;
const TOAST_WIDTH: f32 = 340.0;

/// What a change to one of their sessions means for the technician.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nudge {
    /// A turn ended and the agent is waiting for the technician.
    Replied,
    /// The session, or its last turn, ended in an error.
    Failed,
    /// The agent is blocked on an approval or a question.
    NeedsYou,
}

impl Nudge {
    fn title(self) -> &'static str {
        match self {
            Self::Replied => "Agent replied",
            Self::Failed => "Agent hit an error",
            Self::NeedsYou => "Agent needs a decision",
        }
    }

    fn mark(self, ui: &Ui) -> (&'static str, egui::Color32) {
        match self {
            Self::Replied => (icons::CHAT, theme::success(ui)),
            Self::Failed => (icons::STATUS_ERR, theme::error(ui)),
            Self::NeedsYou => (icons::LOCK, theme::warn(ui)),
        }
    }
}

/// The last recorded state of a thread.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    status: String,
    error: Option<String>,
    updated_at: Option<Datetime>,
}

impl Seen {
    fn of(thread: &AgentThread) -> Self {
        Self {
            status: thread.status.clone(),
            error: thread.error.clone(),
            updated_at: thread.updated_at,
        }
    }
}

/// The nudge the move from `prev` to `next` warrants.
fn nudge_for(prev: &Seen, next: &AgentThread) -> Option<Nudge> {
    let fresh_error =
        next.error.as_deref().is_some_and(|e| !e.trim().is_empty()) && next.error != prev.error;
    match (prev.status.as_str(), next.status.as_str()) {
        (from, to) if from == to => (to == "idle" && fresh_error).then_some(Nudge::Failed),
        (_, "failed") => Some(Nudge::Failed),
        (_, "waiting_approval") => Some(Nudge::NeedsYou),
        ("running" | "waiting_approval", "idle") if fresh_error => Some(Nudge::Failed),
        ("running" | "waiting_approval", "idle") => Some(Nudge::Replied),
        _ => None,
    }
}

/// Last known states of the technician's threads.
#[derive(Default)]
struct Tracker {
    seen: HashMap<RecordId, Seen>,
}

impl Tracker {
    /// Records `row`; returns its nudge, none for a first sighting or a row older than the recorded one.
    fn observe(&mut self, row: &AgentThread) -> Option<Nudge> {
        let next = Seen::of(row);
        let nudge = match self.seen.get(&row.id) {
            None => None,
            Some(prev) if next.updated_at < prev.updated_at => return None,
            Some(prev) => nudge_for(prev, row),
        };
        self.seen.insert(row.id.clone(), next);
        nudge
    }

    fn forget(&mut self, thread: &RecordId) {
        self.seen.remove(thread);
    }
}

/// The signed-in technician, as far as the notifier needs them.
#[derive(Debug, Clone)]
struct Viewer {
    id: RecordId,
    root: bool,
    store: Option<String>,
}

impl Viewer {
    fn of(user: &User) -> Self {
        Self {
            id: user.get_id(),
            root: user.get_authorization() == UserAuthorization::Root,
            store: serde_json::to_value(user)
                .ok()
                .and_then(|v| v.get("store").and_then(Value::as_str).map(str::to_string)),
        }
    }

    /// Whether the approval modal lists this thread's decisions for the viewer.
    fn sees_approvals_of(&self, thread: &AgentThread) -> bool {
        self.root
            || thread.assignee.as_ref() == Some(&self.id)
            || (self.store.is_some() && thread.store == self.store)
    }
}

/// What the toast for one thread shows.
#[derive(Debug, Clone)]
struct Notice {
    nudge: Nudge,
    thread: RecordId,
    is_open: bool,
    subject: String,
    detail: Option<String>,
}

impl Notice {
    fn new(nudge: Nudge, thread: &AgentThread, detail: Option<String>) -> Self {
        Self {
            nudge,
            thread: thread.id.clone(),
            is_open: thread.is_open(),
            subject: thread.label(),
            detail,
        }
    }
}

/// A session a toast asked the Agent Sessions tab to show.
#[derive(Debug, Clone)]
pub struct OpenRequest {
    pub thread: RecordId,
    /// False once the session has closed or failed.
    pub is_open: bool,
}

/// State the views and the toast renderer share with the notifier.
#[derive(Default)]
struct Board {
    drawn: HashMap<String, Instant>,
    notices: HashMap<String, Notice>,
    open: Option<OpenRequest>,
}

static BOARD: LazyLock<Mutex<Board>> = LazyLock::new(Mutex::default);

fn with_board<R>(f: impl FnOnce(&mut Board) -> R) -> Option<R> {
    BOARD.lock().ok().map(|mut board| f(&mut board))
}

/// Records that a view drew `thread` this frame.
pub fn mark_in_view(thread: &RecordId) {
    let key = thread.key_string();
    with_board(|b| b.drawn.insert(key, Instant::now()));
}

/// The session a toast's Open button asked for, handed out once.
pub fn take_open_request() -> Option<OpenRequest> {
    with_board(|b| b.open.take()).flatten()
}

/// True while any viewport of the app has focus.
fn app_focused(ctx: &Context) -> bool {
    ctx.input(|i| i.raw.focused || i.raw.viewports.values().any(|v| v.focused == Some(true)))
}

/// Whether the technician is looking at the thread `key` right now.
fn in_view(ctx: &Context, key: &str) -> bool {
    app_focused(ctx)
        && with_board(|b| b.drawn.get(key).is_some_and(|t| t.elapsed() < IN_VIEW_FOR)).unwrap_or(false)
}

/// Flags the app to a technician looking elsewhere: a taskbar flash, or a count in the tab title.
fn request_attention(ctx: &Context, unseen: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = unseen;
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational),
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        page_title::badge(unseen);
    }
}

fn clear_attention() {
    #[cfg(target_arch = "wasm32")]
    page_title::clear();
}

#[cfg(target_arch = "wasm32")]
mod page_title {
    use std::sync::Mutex;

    /// The page title before the first badge.
    static BASE: Mutex<Option<String>> = Mutex::new(None);

    /// Prefixes the page title with `count`.
    pub fn badge(count: usize) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Ok(mut base) = BASE.lock() else { return };
        let base = base.get_or_insert_with(|| document.title());
        document.set_title(&format!("({count}) {base}"));
    }

    /// Restores the page title.
    pub fn clear() {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        if let Some(title) = BASE.lock().ok().and_then(|mut base| base.take()) {
            document.set_title(&title);
        }
    }
}

/// `text` as one plain line of at most `DETAIL_CHARS` characters.
fn summary(text: &str) -> String {
    let flat = text
        .lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .replace("**", "")
        .replace('`', "");
    if flat.chars().count() <= DETAIL_CHARS {
        return flat;
    }
    let kept: String = flat.chars().take(DETAIL_CHARS - 1).collect();
    format!("{}\u{2026}", kept.trim_end())
}

enum Msg {
    Snapshot(u64, Result<Vec<AgentThread>, String>),
    StreamEnded(u64),
    Ready(u64, Notice),
}

/// Follows the signed-in technician's agent sessions and toasts their replies, failures and waits.
pub struct AgentSessionNotifier {
    viewer: Option<Viewer>,
    tracker: Tracker,
    /// Bumped on every sign-in change; results of an earlier viewer are dropped.
    viewer_gen: u64,
    /// Bumped on every stream start; end notices of earlier streams are dropped.
    stream_gen: u64,
    stream: Option<AbortHandle>,
    /// The shared live-query epoch the stream was opened under.
    stream_epoch: Option<u64>,
    stream_retry_at: Option<Instant>,
    live_rx: Option<Receiver<(Action, AgentThread)>>,
    last_snapshot: Option<Instant>,
    /// Notices raised while the app had no focus.
    unseen: usize,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Default for AgentSessionNotifier {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            viewer: None,
            tracker: Tracker::default(),
            viewer_gen: 0,
            stream_gen: 0,
            stream: None,
            stream_epoch: None,
            stream_retry_at: None,
            live_rx: None,
            last_snapshot: None,
            unseen: 0,
            tx,
            rx,
        }
    }
}

impl AgentSessionNotifier {
    /// Follows the signed-in user's sessions and queues toasts; reads nothing while `live_epoch` is `None`.
    pub fn tick(&mut self, ctx: &Context, user: Option<&User>, live_epoch: Option<u64>, toasts: &mut Toasts) {
        if user.map(User::get_id).as_ref() != self.viewer.as_ref().map(|v| &v.id) {
            self.reset(user.map(Viewer::of));
        }
        let Some(viewer) = self.viewer.clone() else { return };

        if self.stream.is_some() && self.stream_epoch != live_epoch {
            self.stop_stream();
        }

        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Snapshot(generation, rows) if generation == self.viewer_gen => match rows {
                    Ok(rows) => {
                        for row in &rows {
                            self.observe(ctx, &viewer, row, toasts);
                        }
                    }
                    Err(e) => log::debug!("agent session snapshot failed: {e}"),
                },
                Msg::StreamEnded(generation) if generation == self.stream_gen => {
                    self.stop_stream();
                    self.stream_retry_at = Some(Instant::now() + STREAM_RETRY);
                }
                Msg::Ready(generation, notice) if generation == self.viewer_gen => self.show(ctx, notice, toasts),
                _ => {}
            }
        }

        let live: Vec<(Action, AgentThread)> =
            self.live_rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for (action, row) in live {
            match action {
                Action::Delete => self.tracker.forget(&row.id),
                Action::Create | Action::Update => self.observe(ctx, &viewer, &row, toasts),
            }
        }

        if self.unseen > 0 && app_focused(ctx) {
            self.unseen = 0;
            clear_attention();
        }
        let Some(epoch) = live_epoch else { return };
        if self.stream.is_none() && self.stream_retry_at.is_none_or(|t| Instant::now() >= t) {
            self.start_stream(epoch);
        }
        if self.last_snapshot.is_none_or(|t| t.elapsed() >= SNAPSHOT_EVERY) {
            self.request_snapshot();
        }
    }

    fn reset(&mut self, viewer: Option<Viewer>) {
        self.stop_stream();
        self.tracker = Tracker::default();
        self.viewer = viewer;
        self.viewer_gen += 1;
        self.stream_retry_at = None;
        self.last_snapshot = None;
        self.unseen = 0;
        clear_attention();
        with_board(|b| {
            b.notices.clear();
            b.open = None;
        });
    }

    fn stop_stream(&mut self) {
        if let Some(handle) = self.stream.take() {
            handle.abort();
        }
        self.live_rx = None;
        self.stream_epoch = None;
    }

    fn start_stream(&mut self, live_epoch: u64) {
        self.stop_stream();
        self.stream_gen += 1;
        let generation = self.stream_gen;
        let (live_tx, live_rx) = unbounded();
        let tx = self.tx.clone();
        let (fut, handle) = futures::future::abortable(async move {
            let query = AgentThread::live_query_for_signed_in_tech();
            if let Err(e) = listen_data_filtered::<AgentThread>(live_tx, query, Vec::new(), None).await {
                log::debug!("agent session stream ended: {e}");
            }
            let _ = tx.try_send(Msg::StreamEnded(generation));
        });
        PlatformSpawner::spawn(async move {
            let _ = fut.await;
        });
        self.stream = Some(handle);
        self.stream_epoch = Some(live_epoch);
        self.stream_retry_at = None;
        self.live_rx = Some(live_rx);
        self.last_snapshot = None;
    }

    fn request_snapshot(&mut self) {
        self.last_snapshot = Some(Instant::now());
        let tx = self.tx.clone();
        let generation = self.viewer_gen;
        PlatformSpawner::spawn(async move {
            let rows = AgentThread::list_for_signed_in_tech(SNAPSHOT_LIMIT)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.try_send(Msg::Snapshot(generation, rows));
        });
    }

    fn observe(&mut self, ctx: &Context, viewer: &Viewer, row: &AgentThread, toasts: &mut Toasts) {
        let Some(nudge) = self.tracker.observe(row) else { return };
        if in_view(ctx, &row.id.key_string()) {
            return;
        }
        match nudge {
            Nudge::NeedsYou if viewer.sees_approvals_of(row) => self.attention(ctx),
            Nudge::NeedsYou => {
                let detail = Some("It is waiting for a technician's decision.".to_string());
                self.show(ctx, Notice::new(nudge, row, detail), toasts);
            }
            Nudge::Failed => {
                let detail = row.error.as_deref().map(summary).filter(|s| !s.is_empty());
                self.show(ctx, Notice::new(nudge, row, detail), toasts);
            }
            Nudge::Replied => self.fetch_reply(Notice::new(nudge, row, None)),
        }
    }

    /// Fills `notice` with the agent's newest message, then queues it for display.
    fn fetch_reply(&self, mut notice: Notice) {
        let tx = self.tx.clone();
        let generation = self.viewer_gen;
        PlatformSpawner::spawn(async move {
            match AgentEvent::latest_of_kind(&notice.thread, "agent").await {
                Ok(reply) => notice.detail = reply.map(|e| summary(&e.text)).filter(|s| !s.is_empty()),
                Err(e) => log::debug!("agent reply fetch failed: {e}"),
            }
            let _ = tx.try_send(Msg::Ready(generation, notice));
        });
    }

    fn show(&mut self, ctx: &Context, notice: Notice, toasts: &mut Toasts) {
        let key = notice.thread.key_string();
        if in_view(ctx, &key) {
            return;
        }
        with_board(|b| b.notices.insert(key.clone(), notice));
        toasts.add(Toast {
            kind: ToastKind::Custom(AGENT_TOAST_KIND),
            text: format!("agent session {key}").into(),
            options: ToastOptions::default().show_icon(false).show_progress(false),
            payload: Some(key),
            ..Default::default()
        });
        self.attention(ctx);
    }

    fn attention(&mut self, ctx: &Context) {
        if do_not_disturb::is_enabled() || app_focused(ctx) {
            return;
        }
        self.unseen += 1;
        request_attention(ctx, self.unseen);
    }
}

/// Draws an agent-session toast; registered on the shared [`Toasts`] under [`AGENT_TOAST_KIND`].
pub fn toast_contents(ui: &mut Ui, toast: &mut Toast) -> Response {
    let key = toast.payload.clone().unwrap_or_default();
    let notice = with_board(|b| b.notices.get(&key).cloned()).flatten();
    let Some(notice) = notice.filter(|_| !in_view(ui.ctx(), &key)) else {
        with_board(|b| b.notices.remove(&key));
        toast.close();
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    };

    let (glyph, color) = notice.nudge.mark(ui);
    let mut open = false;
    let mut dismiss = false;
    let response = Frame::window(ui.style())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(TOAST_WIDTH);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{glyph} {}", notice.nudge.title())).strong().color(color));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    dismiss = ui.small_button(icons::CLOSE).on_hover_text("Dismiss").clicked();
                });
            });
            ui.label(RichText::new(&notice.subject).small().color(theme::weak_text(ui)));
            if let Some(detail) = &notice.detail {
                ui.add_space(2.0);
                ui.label(detail);
            }
            ui.add_space(4.0);
            open = ui.button(format!("{} Open session", icons::ARROW_RIGHT)).clicked();
        })
        .response;

    if open {
        with_board(|b| {
            b.open = Some(OpenRequest { thread: notice.thread.clone(), is_open: notice.is_open })
        });
    }
    if open || dismiss {
        with_board(|b| b.notices.remove(&key));
        toast.close();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(status: &str, error: Option<&str>, updated_secs: i64) -> AgentThread {
        AgentThread {
            id: RecordId::new("agent_thread", "t1"),
            status: status.to_string(),
            connection_string: "PC-1:abc".to_string(),
            hostname: Some("PC-1".to_string()),
            service_number: Some("2155113".to_string()),
            store: Some("MUR".to_string()),
            requested_by: Some("tech@example.com".to_string()),
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
            error: error.map(str::to_string),
            broker_node: None,
            allow_box_shell: false,
            tokens_used: None,
            tokens_window: None,
            last_seq: None,
            activity: None,
            created_at: None,
            updated_at: Datetime::from_timestamp(1_790_000_000 + updated_secs, 0),
            last_event_at: None,
            closed_at: None,
        }
    }

    fn step(prev: &str, prev_error: Option<&str>, next: &str, next_error: Option<&str>) -> Option<Nudge> {
        nudge_for(&Seen::of(&thread(prev, prev_error, 0)), &thread(next, next_error, 1))
    }

    #[test]
    fn a_finished_turn_is_a_reply() {
        assert_eq!(step("running", None, "idle", None), Some(Nudge::Replied));
        assert_eq!(step("waiting_approval", None, "idle", None), Some(Nudge::Replied));
    }

    #[test]
    fn a_turn_ending_on_a_new_error_is_a_failure() {
        assert_eq!(step("running", None, "idle", Some("429 Too Many Requests")), Some(Nudge::Failed));
        assert_eq!(step("idle", None, "idle", Some("429 Too Many Requests")), Some(Nudge::Failed));
    }

    #[test]
    fn an_error_left_from_an_earlier_turn_is_not_a_failure() {
        assert_eq!(step("running", Some("old"), "idle", Some("old")), Some(Nudge::Replied));
        assert_eq!(step("idle", Some("old"), "idle", Some("old")), None);
    }

    #[test]
    fn a_failed_session_is_a_failure_once() {
        assert_eq!(step("starting", None, "failed", Some("codex daemon: refused")), Some(Nudge::Failed));
        assert_eq!(step("failed", Some("x"), "failed", Some("x")), None);
    }

    #[test]
    fn waiting_on_a_decision_needs_the_tech() {
        assert_eq!(step("running", None, "waiting_approval", None), Some(Nudge::NeedsYou));
        assert_eq!(step("waiting_approval", None, "running", None), None);
    }

    #[test]
    fn other_moves_are_quiet() {
        assert_eq!(step("starting", None, "idle", None), None);
        assert_eq!(step("queued", None, "starting", None), None);
        assert_eq!(step("idle", None, "running", None), None);
        assert_eq!(step("running", None, "closed", None), None);
        assert_eq!(step("running", None, "running", None), None);
    }

    #[test]
    fn the_first_sighting_of_a_thread_is_quiet() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.observe(&thread("idle", None, 0)), None);
        assert_eq!(tracker.observe(&thread("failed", Some("x"), 1)), Some(Nudge::Failed));
        let mut fresh = Tracker::default();
        assert_eq!(fresh.observe(&thread("failed", Some("x"), 0)), None);
    }

    #[test]
    fn an_older_row_neither_nudges_nor_rewinds_the_state() {
        let mut tracker = Tracker::default();
        tracker.observe(&thread("running", None, 5));
        assert_eq!(tracker.observe(&thread("idle", None, 3)), None);
        assert_eq!(tracker.observe(&thread("idle", None, 6)), Some(Nudge::Replied));
        assert_eq!(tracker.observe(&thread("running", None, 4)), None);
        assert_eq!(tracker.observe(&thread("idle", None, 6)), None);
    }

    #[test]
    fn a_forgotten_thread_starts_over() {
        let mut tracker = Tracker::default();
        let row = thread("running", None, 0);
        tracker.observe(&row);
        tracker.forget(&row.id);
        assert_eq!(tracker.observe(&thread("idle", None, 1)), None);
    }

    #[test]
    fn the_approval_modal_covers_the_assignee_their_store_and_root() {
        let row = thread("waiting_approval", None, 0);
        let viewer = |id: &str, store: Option<&str>, root: bool| Viewer {
            id: RecordId::new("user", id),
            root,
            store: store.map(str::to_string),
        };
        assert!(viewer("tech", None, false).sees_approvals_of(&row));
        assert!(viewer("other", Some("MUR"), false).sees_approvals_of(&row));
        assert!(viewer("other", None, true).sees_approvals_of(&row));
        assert!(!viewer("other", Some("RIV"), false).sees_approvals_of(&row));
        assert!(!viewer("other", None, false).sees_approvals_of(&row));
    }

    #[test]
    fn summary_flattens_markdown_and_clips() {
        assert_eq!(summary("## Status\n\nKB5129195 is **installed**.\n- run `sfc`"), "Status KB5129195 is installed. - run sfc");
        let long = "word ".repeat(80);
        let clipped = summary(&long);
        assert_eq!(clipped.chars().count(), DETAIL_CHARS);
        assert!(clipped.ends_with('\u{2026}'));
    }
}
