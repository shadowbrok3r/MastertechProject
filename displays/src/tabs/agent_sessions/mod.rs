//! Codex agent sessions: the threads the admin-agent broker runs, their live
//! transcripts from `agent_event`, and a composer that queues `agent_turn` rows.
//!
//! Everything comes from SurrealDB, so this instance sees the same sessions as
//! every other one and never needs a socket to the agent host. LIVE SELECT
//! streams push thread and transcript changes; slow snapshot polls fill the
//! gap a dropped stream leaves.

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use database::live_data::{listen_data_filtered, Action};
use database::schema::{AgentEvent, AgentThread, AgentTurn, RecordId, RecordIdExt};
use eframe::egui::{self, Align, Layout, RichText, ScrollArea, TextEdit, Ui, vec2};
use futures::future::AbortHandle;
use serde_json::Value;
use web_time::Instant;

use crate::markdown_editor::chat_markdown;
use crate::ui_tools::{hex_json, icons, theme};
use crate::{PlatformSpawner, Spawner};

/// Snapshot polls behind the live streams.
const THREADS_POLL: Duration = Duration::from_secs(30);
const EVENTS_POLL: Duration = Duration::from_secs(10);
/// Pause before a dropped stream is reopened.
const STREAM_RETRY: Duration = Duration::from_secs(3);
/// Repaint cadence that drains pushed rows while streams are open.
const TICK: Duration = Duration::from_millis(400);
const EVENT_PAGE: usize = 400;

enum Msg {
    Threads(Result<Vec<AgentThread>, String>),
    Events(RecordId, Result<Vec<AgentEvent>, String>),
    Turn(Result<(), String>),
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
    status: String,
    loading_threads: bool,
    loading_events: bool,
    last_threads_poll: Option<Instant>,
    last_events_poll: Option<Instant>,
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
            status: String::new(),
            loading_threads: false,
            loading_events: false,
            last_threads_poll: None,
            last_events_poll: None,
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
            self.last_seq = 0;
            self.last_events_poll = None;
            self.event_stream.stop();
        }
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

    fn drain(&mut self) {
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
                Msg::Turn(Ok(())) => self.status = "sent".into(),
                Msg::Turn(Err(e)) => self.status = format!("send failed: {e}"),
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

    fn send_turn(&mut self, kind: &str) {
        let Some(thread) = self.selected.clone() else { return };
        let text = self.composer.trim().to_string();
        if matches!(kind, "start" | "steer") && text.is_empty() {
            return;
        }
        if matches!(kind, "start" | "steer") {
            self.composer.clear();
        }
        let tx = self.tx.clone();
        let kind = kind.to_string();
        PlatformSpawner::spawn(async move {
            let r = AgentTurn::ask(&thread, &kind, &text).await.map(|_| ()).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Turn(r));
        });
    }

    fn selected_thread(&self) -> Option<&AgentThread> {
        let id = self.selected.as_ref()?;
        self.threads.iter().find(|t| &t.id == id)
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.drain();
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
        ui.ctx().request_repaint_after(TICK);

        ui.horizontal(|ui| {
            if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
                self.last_threads_poll = None;
                self.last_events_poll = None;
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
        let rows: Vec<(RecordId, String, String, String, egui::Color32)> = self
            .threads
            .iter()
            .filter(|t| {
                needle.is_empty()
                    || t.label().to_lowercase().contains(&needle)
                    || t.connection_string.to_lowercase().contains(&needle)
                    || t.requested_by.as_deref().unwrap_or("").to_lowercase().contains(&needle)
                    || t.status.contains(&needle)
            })
            .map(|t| {
                let (icon, color, word) = status_chip(ui, &t.status);
                let who = t.requested_by.clone().unwrap_or_default();
                (t.id.clone(), t.label(), format!("{icon} {word}"), who, color)
            })
            .collect();
        if rows.is_empty() {
            ui.label(RichText::new("No agent sessions yet.").weak());
            return;
        }
        ScrollArea::vertical().id_salt("agent_thread_list").show(ui, |ui| {
            for (id, label, chip, who, color) in rows {
                let selected = self.selected.as_ref() == Some(&id);
                let text = format!("{label}\n{chip}  {who}");
                let resp = ui.selectable_label(selected, RichText::new(text).color(color));
                if resp.clicked() {
                    self.select(id.clone());
                }
            }
        });
    }

    fn thread_ui(&mut self, ui: &mut Ui) {
        let Some(thread) = self.selected_thread().cloned() else {
            ui.label(RichText::new("Select a session.").weak());
            return;
        };
        let (icon, color, word) = status_chip(ui, &thread.status);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{icon} {word}")).color(color).strong());
            ui.label(RichText::new(format!("· {}", thread.label())).strong());
            ui.label(RichText::new(format!("· {}", thread.connection_string)).weak());
            if let Some(m) = &thread.model {
                ui.label(RichText::new(format!("· {m}")).weak());
            }
            if let Some(context) = thread.context_usage() {
                ui.label(RichText::new(format!("· {context}")).weak());
            }
            if let Some(by) = &thread.requested_by {
                ui.label(RichText::new(format!("· asked by {by}")).weak());
            }
        });
        if let Some(err) = thread.error.as_deref().filter(|e| !e.is_empty()) {
            ui.label(RichText::new(format!("{} {err}", icons::STATUS_ERR)).color(theme::error(ui)).small());
        }
        ui.separator();

        let composer_h = 96.0;
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
        let open = thread.is_open();
        let running = matches!(thread.status.as_str(), "running" | "waiting_approval");
        ui.add_enabled_ui(open, |ui| {
            let resp = ui.add(
                TextEdit::multiline(&mut self.composer)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text(if running {
                        "Tell the agent something while it works (Nudge), or queue a message for its next turn (Send)"
                    } else {
                        "Message the agent…"
                    }),
            );
            let enter = resp.has_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
            ui.horizontal(|ui| {
                if ui.button(format!("{} Send", icons::CHAT)).clicked() || enter {
                    self.send_turn("start");
                }
                ui.add_enabled_ui(running, |ui| {
                    if ui.button(format!("{} Nudge", icons::ARROW_RIGHT)).clicked() {
                        self.send_turn("steer");
                    }
                    if ui
                        .button(RichText::new(format!("{} Stop", icons::STOP)).color(theme::warn(ui)))
                        .clicked()
                    {
                        self.send_turn("interrupt");
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(RichText::new(format!("{} Close session", icons::CLOSE)).color(theme::error(ui)))
                        .clicked()
                    {
                        self.send_turn("close");
                    }
                });
            });
        });
    }
}

/// Icon, colour and word for a thread status.
pub fn status_chip(ui: &Ui, status: &str) -> (&'static str, egui::Color32, &'static str) {
    match status {
        "queued" => (icons::STATUS_QUEUED, theme::weak_text(ui), "Queued"),
        "starting" => (icons::STATUS_WAIT, theme::info(ui), "Starting"),
        "idle" => (icons::STATUS_READY, theme::success(ui), "Idle"),
        "running" => (icons::STATUS_ON, theme::info(ui), "Working"),
        "waiting_approval" => (icons::LOCK, theme::warn(ui), "Needs approval"),
        "closed" => (icons::STATUS_OFF, theme::weak_text(ui), "Closed"),
        "failed" => (icons::STATUS_ERR, theme::error(ui), "Failed"),
        _ => (icons::STATUS_DOT, theme::weak_text(ui), "Unknown"),
    }
}

fn item_str<'a>(item: &'a Option<Value>, key: &str) -> Option<&'a str> {
    item.as_ref()?.get(key)?.as_str()
}

/// Renders a transcript; shared with the bench-side progress window.
pub fn transcript_ui(ui: &mut Ui, salt: &str, events: &[AgentEvent], show_reasoning: bool) {
    for ev in events {
        let row_salt = format!("{salt}:{}", ev.seq);
        match ev.kind.as_str() {
            "turn_started" => {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("— {} —", ev.turn_id.clone().unwrap_or_else(|| "turn".into())))
                        .weak()
                        .small(),
                );
            }
            "turn_completed" => ui.add_space(4.0),
            "user" => {
                ui.add_space(4.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new("Technician").strong().small());
                    ui.label(&ev.text);
                });
            }
            "agent" => {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Agent", icons::ROBOT)).strong().small());
                    if !ev.done {
                        ui.spinner();
                    }
                });
                chat_markdown::render(ui, &ev.text);
            }
            "reasoning" => {
                if show_reasoning && !ev.text.trim().is_empty() {
                    egui::CollapsingHeader::new(RichText::new("thinking").weak().small())
                        .id_salt(&row_salt)
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label(RichText::new(&ev.text).weak());
                        });
                }
            }
            "tool_call" => {
                let tool = item_str(&ev.item, "tool").unwrap_or("tool").to_string();
                let failed = ev.item.as_ref().and_then(|i| i.get("error")).is_some_and(|e| !e.is_null());
                let title = if failed {
                    RichText::new(format!("{} {tool} failed", icons::STATUS_ERR)).color(theme::error(ui))
                } else if ev.done {
                    RichText::new(format!("{} {tool}", icons::WRENCH)).weak()
                } else {
                    RichText::new(format!("{} {tool} running…", icons::WRENCH)).color(theme::info(ui))
                };
                egui::CollapsingHeader::new(title)
                    .id_salt(&row_salt)
                    .default_open(failed)
                    .show(ui, |ui| {
                        if let Some(args) = ev.item.as_ref().and_then(|i| i.get("arguments")) {
                            ui.label(RichText::new("Arguments").strong().small());
                            hex_json::json_tree(ui, &format!("{row_salt}:args"), args);
                        }
                        if let Some(err) = ev.item.as_ref().and_then(|i| i.pointer("/error/message")).and_then(Value::as_str) {
                            ui.label(RichText::new(err).color(theme::error(ui)));
                        } else if let Some(result) = ev.item.as_ref().and_then(|i| i.get("result")).filter(|r| !r.is_null()) {
                            ui.label(RichText::new("Result").strong().small());
                            hex_json::json_tree(ui, &format!("{row_salt}:result"), result);
                        } else if !ev.text.is_empty() {
                            ui.label(RichText::new(&ev.text).monospace().small());
                        }
                    });
            }
            "command" => {
                egui::CollapsingHeader::new(RichText::new(format!("{} shell", icons::TERMINAL)).weak())
                    .id_salt(&row_salt)
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label(RichText::new(&ev.text).monospace().small());
                    });
            }
            "approval" => {
                ui.label(RichText::new(format!("{} {}", icons::LOCK, ev.text)).color(theme::warn(ui)));
            }
            "error" => {
                ui.label(RichText::new(format!("{} {}", icons::STATUS_ERR, ev.text)).color(theme::error(ui)));
            }
            _ => {
                if !ev.text.trim().is_empty() {
                    ui.label(RichText::new(&ev.text).weak().small());
                }
            }
        }
    }
}
