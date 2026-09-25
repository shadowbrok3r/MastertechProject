//! Codex agent sessions: the threads the admin-agent broker runs, their live
//! transcripts from `agent_event`, and a composer that queues `agent_turn` rows.
//!
//! Everything comes from SurrealDB, so this instance sees the same sessions as
//! every other one and never needs a socket to the agent host. LIVE SELECT
//! streams push thread and transcript changes; slow snapshot polls fill the
//! gap a dropped stream leaves.

mod transcript;

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use database::live_data::{listen_data_filtered, Action};
use database::schema::{AgentEvent, AgentThread, AgentTurn, QueuedTurn, RecordId, RecordIdExt, TurnImage};
use eframe::egui::{self, Align, Id, Layout, RichText, ScrollArea, TextEdit, Ui, vec2};
use futures::future::AbortHandle;
use web_time::Instant;

use crate::ui_tools::agent_chat::{self, Composer, ComposerAction, QueueAction, Rename, RenameOutcome};
use crate::ui_tools::framed_controls::selectable_card;
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};

pub use crate::ui_tools::agent_chat::status_chip;
pub(crate) use transcript::chat_line;
pub use transcript::transcript_ui;

/// Snapshot polls behind the live streams.
const THREADS_POLL: Duration = Duration::from_secs(30);
const EVENTS_POLL: Duration = Duration::from_secs(10);
/// Queue reads while a turn runs or messages wait, and otherwise.
const QUEUE_POLL_BUSY: Duration = Duration::from_secs(3);
const QUEUE_POLL_IDLE: Duration = Duration::from_secs(15);
/// Pause before a dropped stream is reopened.
const STREAM_RETRY: Duration = Duration::from_secs(3);
/// Repaint cadence that drains pushed rows while streams are open.
const TICK: Duration = Duration::from_millis(400);
const EVENT_PAGE: usize = 400;
/// Tallest the message box grows before it scrolls.
const COMPOSER_TEXT_MAX: f32 = 140.0;

enum Msg {
    Threads(Result<Vec<AgentThread>, String>),
    Events(RecordId, Result<Vec<AgentEvent>, String>),
    Waiting(RecordId, Result<Vec<QueuedTurn>, String>),
    Turn(Result<(), String>),
    TakenBack(Result<Option<AgentTurn>, String>),
    ThreadStreamEnded(u64, Option<String>),
    EventStreamEnded(u64, Option<String>),
}

/// One abortable LIVE SELECT and when to reopen it after it drops.
#[derive(Default)]
struct LiveStream {
    generation: u64,
    abort: Option<AbortHandle>,
    retry_at: Option<Instant>,
}

impl LiveStream {
    fn stop(&mut self) {
        if let Some(handle) = self.abort.take() {
            handle.abort();
        }
        self.retry_at = None;
    }

    fn due(&self) -> bool {
        self.abort.is_none() && self.retry_at.is_none_or(|t| Instant::now() >= t)
    }

    /// Marks the stream closed; `false` when the notice belongs to a superseded generation.
    fn ended(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.abort = None;
        self.retry_at = Some(Instant::now() + STREAM_RETRY);
        true
    }
}

pub struct AgentSessions {
    threads: Vec<AgentThread>,
    selected: Option<RecordId>,
    events: Vec<AgentEvent>,
    last_seq: i64,
    include_closed: bool,
    show_reasoning: bool,
    filter: String,
    composer: String,
    attachments: Composer,
    /// The selected thread's queue turns that have not gone out.
    waiting: Vec<QueuedTurn>,
    renaming: Option<Rename>,
    status: String,
    loading_threads: bool,
    loading_events: bool,
    loading_waiting: bool,
    last_threads_poll: Option<Instant>,
    last_events_poll: Option<Instant>,
    last_waiting_poll: Option<Instant>,
    thread_stream: LiveStream,
    event_stream: LiveStream,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    thread_live_tx: Sender<(Action, AgentThread)>,
    thread_live_rx: Receiver<(Action, AgentThread)>,
    event_live_tx: Sender<(Action, AgentEvent)>,
    event_live_rx: Receiver<(Action, AgentEvent)>,
}

impl Default for AgentSessions {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        let (thread_live_tx, thread_live_rx) = crossbeam::channel::unbounded();
        let (event_live_tx, event_live_rx) = crossbeam::channel::unbounded();
        Self {
            threads: Vec::new(),
            selected: None,
            events: Vec::new(),
            last_seq: 0,
            include_closed: false,
            show_reasoning: false,
            filter: String::new(),
            composer: String::new(),
            attachments: Composer::default(),
            waiting: Vec::new(),
            renaming: None,
            status: String::new(),
            loading_threads: false,
            loading_events: false,
            loading_waiting: false,
            last_threads_poll: None,
            last_events_poll: None,
            last_waiting_poll: None,
            thread_stream: LiveStream::default(),
            event_stream: LiveStream::default(),
            tx,
            rx,
            thread_live_tx,
            thread_live_rx,
            event_live_tx,
            event_live_rx,
        }
    }
}

impl AgentSessions {
    /// Opens a thread from elsewhere in the app (the viewport, a notification).
    pub fn select(&mut self, thread: RecordId) {
        if self.selected.as_ref() != Some(&thread) {
            self.selected = Some(thread);
            self.events.clear();
            self.waiting.clear();
            self.last_seq = 0;
            self.last_events_poll = None;
            self.last_waiting_poll = None;
            self.event_stream.stop();
        }
    }

    /// Selects `thread`, listing closed sessions too when it is no longer open.
    pub fn open(&mut self, thread: RecordId, is_open: bool) {
        if !is_open {
            self.include_closed = true;
        }
        self.last_threads_poll = None;
        self.select(thread);
    }

    fn start_thread_stream(&mut self) {
        self.thread_stream.stop();
        self.thread_stream.generation += 1;
        let generation = self.thread_stream.generation;
        let live_tx = self.thread_live_tx.clone();
        let msg_tx = self.tx.clone();
        let (fut, handle) = futures::future::abortable(async move {
            let res = listen_data_filtered::<AgentThread>(
                live_tx,
                "LIVE SELECT * FROM agent_thread".to_string(),
                Vec::new(),
                None,
            )
            .await;
            let _ = msg_tx.send(Msg::ThreadStreamEnded(generation, res.err().map(|e| e.to_string())));
        });
        self.thread_stream.abort = Some(handle);
        PlatformSpawner::spawn(async move {
            let _ = fut.await;
        });
    }

    fn start_event_stream(&mut self) {
        self.event_stream.stop();
        let Some(thread) = self.selected.clone() else { return };
        let key = thread.key_string();
        // The key is inlined, so only the alphabet SurrealDB generates is accepted.
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
            self.event_stream.retry_at = Some(Instant::now() + Duration::from_secs(3600));
            return;
        }
        self.event_stream.generation += 1;
        let generation = self.event_stream.generation;
        let query = format!("LIVE SELECT * FROM agent_event WHERE thread = agent_thread:`{key}`");
        let live_tx = self.event_live_tx.clone();
        let msg_tx = self.tx.clone();
        let (fut, handle) = futures::future::abortable(async move {
            let res = listen_data_filtered::<AgentEvent>(live_tx, query, Vec::new(), None).await;
            let _ = msg_tx.send(Msg::EventStreamEnded(generation, res.err().map(|e| e.to_string())));
        });
        self.event_stream.abort = Some(handle);
        PlatformSpawner::spawn(async move {
            let _ = fut.await;
        });
    }

    fn poll_threads(&mut self) {
        if self.loading_threads {
            return;
        }
        self.loading_threads = true;
        self.last_threads_poll = Some(Instant::now());
        let tx = self.tx.clone();
        let include_closed = self.include_closed;
        PlatformSpawner::spawn(async move {
            let r = AgentThread::list_recent(150, include_closed).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Threads(r));
        });
    }

    fn poll_events(&mut self) {
        let Some(thread) = self.selected.clone() else { return };
        if self.loading_events {
            return;
        }
        self.loading_events = true;
        self.last_events_poll = Some(Instant::now());
        let tx = self.tx.clone();
        let after = self.last_seq;
        PlatformSpawner::spawn(async move {
            let r = AgentEvent::since(&thread, after, EVENT_PAGE).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Events(thread, r));
        });
    }

    fn poll_waiting(&mut self) {
        let Some(thread) = self.selected.clone() else { return };
        if self.loading_waiting {
            return;
        }
        self.loading_waiting = true;
        self.last_waiting_poll = Some(Instant::now());
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentTurn::waiting(&thread).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Waiting(thread, r));
        });
    }

    fn waiting_poll_due(&self) -> bool {
        let busy = self.selected_thread().is_some_and(AgentThread::is_busy) || !self.waiting.is_empty();
        let every = if busy { QUEUE_POLL_BUSY } else { QUEUE_POLL_IDLE };
        self.last_waiting_poll.is_none_or(|t| t.elapsed() >= every)
    }

    fn merge_event(&mut self, row: AgentEvent) {
        self.last_seq = self.last_seq.max(row.seq);
        match self.events.iter_mut().find(|e| e.id == row.id) {
            Some(existing) => *existing = row,
            None => self.events.push(row),
        }
        self.events.sort_by_key(|e| e.seq);
    }

    fn merge_thread(&mut self, row: AgentThread) {
        let keep = self.include_closed || row.is_open();
        match self.threads.iter().position(|t| t.id == row.id) {
            Some(i) if keep => self.threads[i] = row,
            Some(i) => {
                self.threads.remove(i);
            }
            None if keep => self.threads.insert(0, row),
            None => {}
        }
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok((action, row)) = self.thread_live_rx.try_recv() {
            match action {
                Action::Delete => self.threads.retain(|t| t.id != row.id),
                Action::Create | Action::Update => self.merge_thread(row),
            }
            self.status = format!("{} sessions", self.threads.len());
        }
        while let Ok((action, row)) = self.event_live_rx.try_recv() {
            if self.selected.as_ref() != Some(&row.thread) {
                continue;
            }
            match action {
                Action::Delete => self.events.retain(|e| e.id != row.id),
                Action::Create | Action::Update => self.merge_event(row),
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Threads(Ok(threads)) => {
                    self.loading_threads = false;
                    self.status = format!("{} sessions", threads.len());
                    if self.selected.is_none() {
                        if let Some(first) = threads.first() {
                            self.select(first.id.clone());
                        }
                    }
                    self.threads = threads;
                }
                Msg::Threads(Err(e)) => {
                    self.loading_threads = false;
                    self.status = e;
                }
                Msg::Events(thread, Ok(rows)) => {
                    self.loading_events = false;
                    if self.selected.as_ref() != Some(&thread) {
                        continue;
                    }
                    for row in rows {
                        self.merge_event(row);
                    }
                }
                Msg::Events(_, Err(e)) => {
                    self.loading_events = false;
                    self.status = e;
                }
                Msg::Waiting(thread, result) => {
                    self.loading_waiting = false;
                    match result {
                        Ok(rows) if self.selected.as_ref() == Some(&thread) => self.waiting = rows,
                        Ok(_) => {}
                        Err(e) => log::debug!("agent queue poll failed: {e}"),
                    }
                }
                Msg::Turn(Ok(())) => {
                    self.status = "sent".into();
                    self.last_waiting_poll = None;
                }
                Msg::Turn(Err(e)) => self.status = format!("send failed: {e}"),
                Msg::TakenBack(Ok(Some(turn))) => {
                    self.attachments.restore(ctx, &mut self.composer, turn);
                    self.last_waiting_poll = None;
                }
                Msg::TakenBack(Ok(None)) => {
                    self.status = "that message already went out".into();
                    self.last_waiting_poll = None;
                }
                Msg::TakenBack(Err(e)) => self.status = format!("could not take the message back: {e}"),
                Msg::ThreadStreamEnded(generation, error) => {
                    if self.thread_stream.ended(generation) {
                        self.last_threads_poll = None;
                        if let Some(e) = error {
                            self.status = format!("session stream dropped, reconnecting: {e}");
                        }
                    }
                }
                Msg::EventStreamEnded(generation, error) => {
                    if self.event_stream.ended(generation) {
                        self.last_events_poll = None;
                        if let Some(e) = error {
                            self.status = format!("transcript stream dropped, reconnecting: {e}");
                        }
                    }
                }
            }
        }
    }

    /// Writes one turn row for `thread`.
    fn ask(&mut self, thread: RecordId, kind: &'static str, text: String, images: Vec<TurnImage>) {
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let r = AgentTurn::ask_with(&thread, kind, &text, &images).await.map(|_| ()).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Turn(r));
        });
    }

    fn send_turn(&mut self, kind: &'static str) {
        let Some(thread) = self.selected.clone() else { return };
        self.ask(thread, kind, String::new(), Vec::new());
    }

    fn apply_queue_action(&mut self, action: QueueAction) {
        let Some(thread) = self.selected.clone() else { return };
        let tx = self.tx.clone();
        match action {
            QueueAction::Resume => self.ask(thread, "queue", String::new(), Vec::new()),
            QueueAction::Remove(id) => {
                self.waiting.retain(|w| w.id != id);
                PlatformSpawner::spawn(async move {
                    let r = AgentTurn::cancel(&id).await.map(|_| ()).map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Turn(r));
                });
            }
            QueueAction::Edit(id) => {
                self.waiting.retain(|w| w.id != id);
                PlatformSpawner::spawn(async move {
                    let r = AgentTurn::take_back(&id).await.map_err(|e| e.to_string());
                    let _ = tx.send(Msg::TakenBack(r));
                });
            }
        }
    }

    fn selected_thread(&self) -> Option<&AgentThread> {
        let id = self.selected.as_ref()?;
        self.threads.iter().find(|t| &t.id == id)
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.drain(ui.ctx());
        if self.thread_stream.due() {
            self.start_thread_stream();
        }
        if self.selected.is_some() && self.event_stream.due() {
            self.start_event_stream();
        }
        if self.last_threads_poll.is_none_or(|t| t.elapsed() >= THREADS_POLL) {
            self.poll_threads();
        }
        if self.selected.is_some() && self.last_events_poll.is_none_or(|t| t.elapsed() >= EVENTS_POLL) {
            self.poll_events();
        }
        if self.selected.is_some() && self.waiting_poll_due() {
            self.poll_waiting();
        }
        ui.ctx().request_repaint_after(TICK);

        ui.horizontal(|ui| {
            if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
                self.last_threads_poll = None;
                self.last_events_poll = None;
                self.last_waiting_poll = None;
            }
            if ui.checkbox(&mut self.include_closed, "Show closed").changed() {
                self.last_threads_poll = None;
            }
            ui.checkbox(&mut self.show_reasoning, "Show thinking");
            ui.label("Search:");
            ui.add(TextEdit::singleline(&mut self.filter).desired_width(200.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&self.status).weak());
            });
        });
        ui.separator();

        let list_w = (ui.available_width() * 0.28).clamp(220.0, 360.0);
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(list_w, height), Layout::top_down(Align::Min), |ui| {
                self.list_ui(ui);
            });
            ui.separator();
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::top_down(Align::Min),
                |ui| self.thread_ui(ui),
            );
        });
    }

    fn list_ui(&mut self, ui: &mut Ui) {
        let needle = self.filter.trim().to_lowercase();
        let rows: Vec<AgentThread> = self
            .threads
            .iter()
            .filter(|t| {
                needle.is_empty()
                    || t.label().to_lowercase().contains(&needle)
                    || t.connection_string.to_lowercase().contains(&needle)
                    || t.requested_by.as_deref().unwrap_or("").to_lowercase().contains(&needle)
                    || t.status.contains(&needle)
            })
            .cloned()
            .collect();
        if rows.is_empty() {
            ui.label(RichText::new("No agent sessions yet.").weak());
            return;
        }
        let mut picked = None;
        let mut rename_to = None;
        ScrollArea::vertical().id_salt("agent_thread_list").show(ui, |ui| {
            for t in &rows {
                let key = t.id.key_string();
                if let Some(edit) = self.renaming.as_mut().filter(|r| r.key == key) {
                    match edit.show(ui, ui.available_width()) {
                        RenameOutcome::Editing => {}
                        RenameOutcome::Save(_, title) => rename_to = Some((t.id.clone(), title)),
                        RenameOutcome::Cancel => self.renaming = None,
                    }
                    continue;
                }
                let selected = self.selected.as_ref() == Some(&t.id);
                let card = selectable_card(ui, &key, selected, |ui| {
                    ui.horizontal(|ui| {
                        if agent_chat::is_active(t) {
                            ui.add(egui::Spinner::new().size(12.0));
                        }
                        ui.add(egui::Label::new(RichText::new(t.label()).strong()).truncate());
                    });
                    ui.horizontal(|ui| {
                        agent_chat::status_badge(ui, t);
                        if let Some(who) = &t.requested_by {
                            ui.add(egui::Label::new(RichText::new(who).small().weak()).truncate());
                        }
                    });
                });
                if card.response.clicked() {
                    picked = Some(t.id.clone());
                }
                card.response.context_menu(|ui| {
                    if ui.button(format!("{} Rename", icons::EDIT)).clicked() {
                        self.renaming = Some(Rename::new(key.clone(), &t.label()));
                        ui.close();
                    }
                });
            }
        });
        if let Some(id) = picked {
            self.select(id);
        }
        if let Some((id, title)) = rename_to {
            self.renaming = None;
            self.rename(id, title);
        }
    }

    /// Asks the broker to store `title`; the list shows it at once.
    fn rename(&mut self, thread: RecordId, title: String) {
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread) {
            t.title = Some(title.clone());
        }
        self.ask(thread, "rename", title, Vec::new());
    }

    fn thread_ui(&mut self, ui: &mut Ui) {
        let Some(thread) = self.selected_thread().cloned() else {
            ui.label(RichText::new("Select a session.").weak());
            return;
        };
        crate::ui_data::agent_session_notify::mark_in_view(&thread.id);
        let pane = ui.available_rect_before_wrap();
        ui.horizontal_wrapped(|ui| {
            agent_chat::status_badge(ui, &thread);
            ui.label(RichText::new(format!("\u{00b7} {}", thread.label())).strong());
            if ui
                .small_button(icons::EDIT)
                .on_hover_text("Rename this session")
                .clicked()
            {
                self.renaming = Some(Rename::new(thread.id.key_string(), &thread.label()));
            }
            ui.label(RichText::new(format!("\u{00b7} {}", thread.connection_string)).weak());
            if let Some(m) = &thread.model {
                ui.label(RichText::new(format!("\u{00b7} {m}")).weak());
            }
            if let Some(by) = &thread.requested_by {
                ui.label(RichText::new(format!("\u{00b7} asked by {by}")).weak());
            }
        });
        if agent_chat::context_bar(ui, &thread) {
            self.send_turn("compact");
        }
        if let Some(err) = thread.error.as_deref().filter(|e| !e.is_empty()) {
            ui.label(RichText::new(format!("{} {err}", icons::STATUS_ERR)).color(theme::error(ui)).small());
        }
        ui.separator();

        let queue_h = if self.waiting.is_empty() { 0.0 } else { 26.0 + 22.0 * self.waiting.len().min(4) as f32 };
        let attach_h = if self.attachments.attachments.is_empty() { 0.0 } else { 70.0 };
        let composer_h = 110.0 + queue_h + attach_h;
        let body_h = (ui.available_height() - composer_h).max(120.0);
        let show_reasoning = self.show_reasoning;
        let salt = thread.id.key_string();
        ScrollArea::vertical()
            .id_salt(("agent_transcript", &salt))
            .stick_to_bottom(true)
            .max_height(body_h)
            .show(ui, |ui| {
                if self.events.is_empty() {
                    ui.label(RichText::new("Nothing yet — the agent is starting up.").weak());
                }
                transcript_ui(ui, &salt, &self.events, show_reasoning);
            });

        ui.separator();
        if let Some(action) = agent_chat::queue_strip(ui, &self.waiting) {
            self.apply_queue_action(action);
        }
        let open = thread.is_open();
        let composer_id = Id::new(("agent_sessions_composer", &salt));
        let action = self.attachments.show(ui, composer_id, &mut self.composer, thread.is_busy(), open, COMPOSER_TEXT_MAX);
        match action {
            Some(ComposerAction::Send { kind, text, images, .. }) => self.ask(thread.id.clone(), kind, text, images),
            Some(ComposerAction::Stop) => self.send_turn("interrupt"),
            None => {}
        }
        ui.add_enabled_ui(open, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button(RichText::new(format!("{} Close session", icons::CLOSE)).color(theme::error(ui)))
                    .clicked()
                {
                    self.send_turn("close");
                }
            });
        });
        if open {
            self.attachments.drop_zone(ui, composer_id, pane);
        }
    }
}
