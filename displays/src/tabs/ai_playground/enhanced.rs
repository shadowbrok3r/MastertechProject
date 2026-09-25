use eframe::egui::{
    Align, CentralPanel, Frame, Id, Layout, Margin, Popup, PopupCloseBehavior, RichText, ScrollArea, Ui,
};
use crate::{
    tabs::ai_playground::{ChatMessage, ChatMessageType, ChatThread, SentFrom, TOOL_PREFIX},
    ui_tools::agent_chat::{self, Composer, ComposerAction, QueueAction, Rename, RenameOutcome},
    ui_tools::chat_bubble::{self, ChatKind, ChatRow, ChatStyle},
    ui_tools::icons,
    PlatformSpawner, Spawner,
};

use std::collections::HashMap;
use chrono::{DateTime, Local, Utc};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{AgentThread, AgentTurn, QueuedTurn, RecordId, RecordIdExt, TurnImage};
use serde::Serialize;

/// Smallest outer height of the prompt box.
const INPUT_MIN_HEIGHT: f32 = 52.0;
const INPUT_PANEL_MARGIN: i8 = 6;
/// Longest header summary in characters; the header also truncates to its width.
const SUMMARY_CHARS: usize = 160;
/// Prefix on assistant lines that carry a broker notice rather than a reply.
const NOTICE_PREFIX: &str = icons::INFO;
/// Longest first message an `assist_request` carries whole.
const REQUEST_NOTE_MAX: usize = 500;
/// First message of a session whose real first message goes in as a queued turn.
const OPENER: &str = "Open this session. My request follows as the next message.";

/// The open agent chat's session row and queue, read together.
struct AgentState {
    thread: String,
    row: Option<AgentThread>,
    waiting: Vec<QueuedTurn>,
}

/// A chat thread loaded from the database, delivered to the UI thread.
struct LoadedThread {
    id: String,
    title: String,
    messages: Vec<ChatMessage>,
}

/// Chat with the Codex agent through the admin-agent broker: every thread is an
/// agent session, scoped to the focused machine when there is one and to the
/// technician's records otherwise.
#[derive(Serialize)]
pub struct EnhancedAiPlayground {
    pub selected_thread: String,
    pub chat_title: HashMap<String, String>,
    pub edit_title: bool,
    pub threads: HashMap<String, ChatThread>,
    #[serde(skip)]
    pub response_tx: Sender<ChatMessage>,
    #[serde(skip)]
    pub response_rx: Receiver<ChatMessage>,
    pub save_chats: bool,
    pub image_id: String,
    pub open_modal: bool,
    /// Connection string of the connected client the admin console is focused on; typed input goes to its session.
    #[serde(skip)]
    pub focused_client: Option<String>,
    /// When true, hides the close ✕ and uses self-diagnosis empty-state copy.
    #[serde(skip)]
    pub self_diagnosis: bool,
    /// Threads known to be agent sessions, so their input is never treated as a fresh chat.
    #[serde(skip)]
    pub agent_threads: std::collections::HashSet<String>,
    #[serde(skip)]
    last_agent_poll: Option<web_time::Instant>,
    /// Threads the reply poller found transcript rows for.
    #[serde(skip)]
    agent_flag_tx: Sender<String>,
    #[serde(skip)]
    agent_flag_rx: Receiver<String>,
    /// Agent sessions this user may open: their own, or every one for root.
    #[serde(skip)]
    agent_index: Vec<database::schema::AgentThread>,
    #[serde(skip)]
    agent_index_tx: Sender<Vec<database::schema::AgentThread>>,
    #[serde(skip)]
    agent_index_rx: Receiver<Vec<database::schema::AgentThread>>,
    #[serde(skip)]
    last_index_poll: Option<web_time::Instant>,
    /// Threads already backfilled from the database, so a thread opened from the
    /// index renders both sides once without duplicating the author's own echo.
    #[serde(skip)]
    hydrated: std::collections::HashSet<String>,
    /// Local threads re-keyed onto the agent session they turned out to be.
    #[serde(skip)]
    agent_switch_tx: Sender<(String, String)>,
    #[serde(skip)]
    agent_switch_rx: Receiver<(String, String)>,
    /// Service number the conversation is about, when the host knows one; joins
    /// the transcript to a service order.
    #[serde(skip)]
    pub service_number: Option<String>,
    /// Per-thread label of the engine that answered it, shown in the top bar.
    #[serde(skip)]
    thread_engine: HashMap<String, String>,
    /// Set when the panel's close button is clicked; the host reads + clears it.
    #[serde(skip)]
    close_requested: bool,
    /// One-time load guard for pulling persisted threads from the database.
    #[serde(skip)]
    loaded: bool,
    #[serde(skip)]
    load_tx: Sender<Vec<LoadedThread>>,
    #[serde(skip)]
    load_rx: Receiver<Vec<LoadedThread>>,
    /// Attachments waiting in each thread's composer.
    #[serde(skip)]
    composers: HashMap<String, Composer>,
    /// The open agent chat's session row, re-read with its queue.
    #[serde(skip)]
    open_row: Option<AgentThread>,
    /// The open agent chat's queue turns that have not gone out.
    #[serde(skip)]
    waiting: Vec<QueuedTurn>,
    #[serde(skip)]
    last_state_poll: Option<web_time::Instant>,
    #[serde(skip)]
    state_tx: Sender<AgentState>,
    #[serde(skip)]
    state_rx: Receiver<AgentState>,
    /// Queue turns taken back for editing, by the thread they belong to.
    #[serde(skip)]
    taken_back_tx: Sender<(String, AgentTurn)>,
    #[serde(skip)]
    taken_back_rx: Receiver<(String, AgentTurn)>,
    /// A chat title being edited in the top bar.
    #[serde(skip)]
    renaming: Option<Rename>,
}

impl Default for EnhancedAiPlayground {
    fn default() -> Self {
        let (response_tx, response_rx) = crossbeam::channel::unbounded::<ChatMessage>();
        let (load_tx, load_rx) = crossbeam::channel::unbounded::<Vec<LoadedThread>>();
        let (agent_flag_tx, agent_flag_rx) = crossbeam::channel::unbounded::<String>();
        let (agent_index_tx, agent_index_rx) =
            crossbeam::channel::unbounded::<Vec<database::schema::AgentThread>>();
        let (agent_switch_tx, agent_switch_rx) = crossbeam::channel::unbounded::<(String, String)>();
        let (state_tx, state_rx) = crossbeam::channel::unbounded::<AgentState>();
        let (taken_back_tx, taken_back_rx) = crossbeam::channel::unbounded::<(String, AgentTurn)>();
        Self {
            selected_thread: String::new(),
            chat_title: HashMap::new(),
            edit_title: false,
            threads: HashMap::new(),
            response_tx,
            response_rx,
            save_chats: false,
            image_id: String::new(),
            open_modal: false,
            focused_client: None,
            self_diagnosis: false,
            agent_threads: std::collections::HashSet::new(),
            last_agent_poll: None,
            agent_flag_tx,
            agent_flag_rx,
            agent_index: Vec::new(),
            agent_index_tx,
            agent_index_rx,
            last_index_poll: None,
            hydrated: std::collections::HashSet::new(),
            agent_switch_tx,
            agent_switch_rx,
            service_number: None,
            thread_engine: HashMap::new(),
            close_requested: false,
            loaded: false,
            load_tx,
            load_rx,
            composers: HashMap::new(),
            open_row: None,
            waiting: Vec::new(),
            last_state_poll: None,
            state_tx,
            state_rx,
            taken_back_tx,
            taken_back_rx,
            renaming: None,
        }
    }
}

impl EnhancedAiPlayground {
    /// Returns and clears the "close panel" request raised by the top-bar ✕.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Opens a fresh thread that asks the agent for a first look at the focused machine.
    pub fn start_agent_diagnosis(&mut self, connection_string: Option<String>) {
        let thread_id = uuid::Uuid::new_v4().to_string();
        self.select_thread(thread_id.clone());
        self.threads.insert(
            thread_id.clone(),
            ChatThread { id: thread_id.clone(), messages: Vec::new(), images: Vec::new(), input: String::new() },
        );
        let label = match &connection_string {
            Some(cs) => format!("Diagnose {cs}"),
            None => "What can you look up for me?".to_string(),
        };
        let _ = self.response_tx.try_send(ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread_id.clone(),
            ts: crate::tabs::ai_playground::now_ts(),
            from: SentFrom::Me,
            content: ChatMessageType::Text(label),
        });
        #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
        {
            let prompt = match &connection_string {
                Some(_) => "Diagnose that client. Pull its prior history and run an initial triage \
                     using the Mastertech tools."
                    .to_string(),
                None => "In two lines, say what you can look up and do from here.".to_string(),
            };
            self.thread_engine.insert(thread_id.clone(), "Codex agent".to_string());
            self.send_to_agent(thread_id, prompt, Vec::new(), "start", connection_string);
        }
        #[cfg(not(any(target_arch = "wasm32", feature = "tokio")))]
        {
            let _ = connection_string;
        }
    }

    pub fn enhanced_ai_playground(&mut self, ui: &mut Ui) {
        self.ensure_loaded();
        let rail = ui.max_rect();

        eframe::egui::Panel::top("enhanced_ai_topbar")
            .frame(Frame::default().inner_margin(Margin::symmetric(6, 2)))
            .exact_size(28.)
            .show_separator_line(false)
            .show(ui, |ui| self.show_chat_topbar(ui));

        if let Some(row) = self
            .open_row
            .clone()
            .filter(|r| r.id.key_string() == self.selected_thread)
        {
            eframe::egui::Panel::top("enhanced_ai_context")
                .frame(Frame::default().inner_margin(Margin::symmetric(8, 1)))
                .show_separator_line(false)
                .show(ui, |ui| {
                    if agent_chat::context_bar(ui, &row) {
                        self.ask_agent(&row.id, "compact", String::new(), Vec::new());
                    }
                });
        }

        // Sized from last frame's composer, from the minimum up to half the chat.
        let height_id = ui.id().with("enhanced_ai_input_height");
        let max_height = (ui.available_height() * 0.5).max(INPUT_MIN_HEIGHT);
        let height = ui
            .memory(|m| m.data.get_temp::<f32>(height_id))
            .unwrap_or(INPUT_MIN_HEIGHT)
            .clamp(INPUT_MIN_HEIGHT, max_height);
        eframe::egui::Panel::bottom("enhanced_ai_input")
            .frame(Frame::default().inner_margin(Margin::same(INPUT_PANEL_MARGIN)))
            .exact_size(height)
            .show(ui, |ui| {
                let used = self.show_chat_input(ui, max_height);
                let wanted = (used + 2.0 * f32::from(INPUT_PANEL_MARGIN))
                    .clamp(INPUT_MIN_HEIGHT, max_height);
                if (wanted - height).abs() > 0.5 {
                    ui.memory_mut(|m| m.data.insert_temp(height_id, wanted));
                    ui.ctx().request_repaint();
                }
            });

        CentralPanel::default()
            .frame(Frame::central_panel(ui.style()).inner_margin(Margin::same(10)))
            .show(ui, |ui| self.show_chat_content(ui));

        if self.threads.contains_key(&self.selected_thread) {
            let id = composer_id(&self.selected_thread);
            self.composers
                .entry(self.selected_thread.clone())
                .or_default()
                .drop_zone(ui, id, rail);
        }
        self.handle_enhanced_ai_events(ui);
    }

    fn thread_title(&self, id: &str) -> String {
        let session = self
            .open_row
            .iter()
            .chain(&self.agent_index)
            .find(|t| {
                t.id.key_string() == id && t.title.as_deref().is_some_and(|t| !t.trim().is_empty())
            })
            .map(AgentThread::label);
        session
            .or_else(|| self.chat_title.get(id).cloned())
            .unwrap_or_else(|| {
                self.threads
                    .get(id)
                    .and_then(|t| {
                        t.messages.iter().find_map(|m| match &m.content {
                            ChatMessageType::Text(s) if matches!(m.from, SentFrom::Me) => {
                                Some(short_title(s))
                            }
                            _ => None,
                        })
                    })
                    .unwrap_or_else(|| "New chat".to_string())
            })
    }

    fn current_thread_title(&self) -> String {
        if self.threads.contains_key(&self.selected_thread) {
            self.thread_title(&self.selected_thread)
        } else {
            "Threads".to_string()
        }
    }

    /// Top bar: the threads dropdown and New chat on the left; the session's status and close on the right.
    fn show_chat_topbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if self.renaming.as_ref().is_some_and(|r| r.key == self.selected_thread) {
                self.rename_field(ui);
                return;
            }
            // ── Threads dropdown (opens on hover, stays open over the popup) ──
            let label = format!("{}  {}  {}", icons::CHAT, self.current_thread_title(), icons::CHEV_OPEN);
            let resp = ui.button(RichText::new(label));
            if self.threads.contains_key(&self.selected_thread) {
                resp.context_menu(|ui| {
                    if ui.button(format!("{} Rename", icons::EDIT)).clicked() {
                        self.start_rename(self.selected_thread.clone());
                        ui.close();
                    }
                });
            }
            // Stay open while the pointer is over the button OR the popup
            // (compared against last frame's popup rect, so crossing the gap
            // between them doesn't snap it shut).
            let rect_id = ui.make_persistent_id("threads_dropdown_rect");
            let last_rect = ui.memory(|m| m.data.get_temp::<eframe::egui::Rect>(rect_id));
            let pointer = ui.ctx().pointer_hover_pos();
            let over_popup = match (last_rect, pointer) {
                (Some(r), Some(p)) => r.expand(8.0).contains(p),
                _ => false,
            };
            let open = resp.hovered() || over_popup;

            let mut picked: Option<String> = None;
            let mut rename: Option<String> = None;
            let popup = Popup::from_response(&resp)
                .open(open)
                .gap(2.0)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_min_width(220.);
                    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
                    let agent_index = self.index_with_open_row();
                    #[cfg(not(any(target_arch = "wasm32", feature = "tokio")))]
                    let agent_index: Vec<AgentThread> = Vec::new();
                    if self.threads.is_empty() && agent_index.is_empty() {
                        ui.label(RichText::new("No chats yet").weak());
                        return;
                    }
                    let selected = self.selected_thread.clone();
                    let mut ids: Vec<String> = self.threads.keys().cloned().collect();
                    ids.sort();
                    ScrollArea::vertical().max_height(320.).show(ui, |ui| {
                        for id in ids {
                            let title = self.thread_title(&id);
                            let row = ui.selectable_label(selected == id, RichText::new(format!("{}  {title}", icons::CHAT)));
                            if row.clicked() {
                                picked = Some(id.clone());
                            }
                            row.context_menu(|ui| {
                                if ui.button(format!("{} Rename", icons::EDIT)).clicked() {
                                    rename = Some(id.clone());
                                    ui.close();
                                }
                            });
                        }
                        if agent_index.is_empty() {
                            return;
                        }
                        ui.separator();
                        ui.label(RichText::new("Agent sessions").weak().small());
                        for t in &agent_index {
                            let who = t.requested_by.as_deref().unwrap_or("unattributed");
                            let key = t.id.key_string();
                            let (icon, color, _) = agent_chat::status_chip(ui, &t.status);
                            let row = ui
                                .horizontal(|ui| {
                                    if agent_chat::is_active(t) {
                                        ui.add(eframe::egui::Spinner::new().size(12.0).color(color));
                                    } else {
                                        ui.label(RichText::new(icon).color(color));
                                    }
                                    let line = format!("{}  ({})", t.label(), agent_chat::status_words(t));
                                    ui.selectable_label(selected == key, RichText::new(line))
                                })
                                .inner
                                .on_hover_text(format!("{who}\n{}", t.connection_string));
                            if row.clicked() {
                                picked = Some(key.clone());
                            }
                            row.context_menu(|ui| {
                                if ui.button(format!("{} Rename", icons::EDIT)).clicked() {
                                    rename = Some(key.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
                });
            let stored = popup.map(|r| r.response.rect).unwrap_or(eframe::egui::Rect::NOTHING);
            ui.memory_mut(|m| m.data.insert_temp(rect_id, stored));
            if let Some(id) = rename {
                picked = Some(id.clone());
                self.start_rename(id);
            }
            if let Some(id) = picked {
                #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
                if !self.threads.contains_key(&id) {
                    // Only an agent conversation can be picked without local
                    // state; opening it backfills the transcript.
                    self.open_agent_thread(id.clone());
                }
                self.select_thread(id);
            }

            if ui.button(RichText::new(icons::PLUS)).on_hover_text("New chat").clicked() {
                self.create_new_chat_thread();
            }

            // ── Right side: close · diagnose · status or engine ──
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.self_diagnosis
                    && ui.button(RichText::new(icons::CLOSE)).on_hover_text("Close chat").clicked()
                {
                    self.close_requested = true;
                }
                #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
                if self.focused_client.is_some()
                    && ui
                        .button(RichText::new(icons::ROBOT))
                        .on_hover_text("Ask the agent for a first look at the focused machine")
                        .clicked()
                {
                    let cs = self.focused_client.clone();
                    self.start_agent_diagnosis(cs);
                }
                if let Some(row) = self.open_row.as_ref().filter(|r| r.id.key_string() == self.selected_thread) {
                    agent_chat::status_badge(ui, row);
                    return;
                }
                let engine = self
                    .thread_engine
                    .get(&self.selected_thread)
                    .cloned()
                    .unwrap_or_else(|| "Codex agent".to_string());
                ui.add(eframe::egui::Label::new(RichText::new(engine).weak().small()).truncate()).on_hover_text(
                    "Every thread runs on the Codex agent (Qwen on the shop pool) through the admin-agent broker.",
                );
            });
        });
    }

    /// The agent index with the open session's fresher row in place of its listed one.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn index_with_open_row(&self) -> Vec<AgentThread> {
        let mut index = self.agent_index.clone();
        if let Some(row) = &self.open_row
            && let Some(listed) = index.iter_mut().find(|t| t.id == row.id)
        {
            *listed = row.clone();
        }
        index
    }

    fn select_thread(&mut self, id: String) {
        if self.selected_thread != id {
            self.open_row = None;
            self.waiting.clear();
            self.last_state_poll = None;
        }
        self.selected_thread = id;
    }

    fn start_rename(&mut self, key: String) {
        let current = self.thread_title(&key);
        self.renaming = Some(Rename::new(key, &current));
    }

    /// The title field in the top bar; saving renames the session through the broker, or the local chat.
    fn rename_field(&mut self, ui: &mut Ui) {
        let width = (ui.available_width() - 40.0).clamp(80.0, 280.0);
        let Some(edit) = self.renaming.as_mut() else {
            return;
        };
        match edit.show(ui, width) {
            RenameOutcome::Editing => {}
            RenameOutcome::Cancel => self.renaming = None,
            RenameOutcome::Save(key, title) => {
                self.renaming = None;
                self.chat_title.insert(key.clone(), title.clone());
                if let Some(t) = self
                    .agent_index
                    .iter_mut()
                    .find(|t| t.id.key_string() == key)
                {
                    t.title = Some(title.clone());
                }
                if let Some(t) = self.open_row.as_mut().filter(|t| t.id.key_string() == key) {
                    t.title = Some(title.clone());
                }
                if self.agent_threads.contains(&key) {
                    self.ask_agent(
                        &RecordId::new("agent_thread", key.as_str()),
                        "rename",
                        title,
                        Vec::new(),
                    );
                } else {
                    self.save_thread(&key);
                }
            }
        }
    }

    /// Draws the queue strip and the composer; returns the height they took.
    fn show_chat_input(&mut self, ui: &mut Ui, max_height: f32) -> f32 {
        let tid = self.selected_thread.clone();
        if !self.threads.contains_key(&tid) {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(format!("Start a new chat with  {}  above.", icons::PLUS)).weak());
            });
            return INPUT_MIN_HEIGHT;
        }
        let top = ui.cursor().top();
        if let Some(action) = agent_chat::queue_strip(ui, &self.waiting) {
            self.apply_queue_action(&tid, action);
        }
        let row = self.open_row.as_ref().filter(|r| r.id.key_string() == tid);
        let busy = row.is_some_and(AgentThread::is_busy);
        let open = row.is_none_or(AgentThread::is_open);
        let text_max = (max_height - 72.0).max(40.0);
        let composer = self.composers.entry(tid.clone()).or_default();
        let Some(thread) = self.threads.get_mut(&tid) else {
            return INPUT_MIN_HEIGHT;
        };
        let action = composer.show(
            ui,
            composer_id(&tid),
            &mut thread.input,
            busy,
            open,
            text_max,
        );
        match action {
            Some(ComposerAction::Send {
                kind,
                text,
                images,
                staged,
            }) => self.send_chat_message(kind, text, images, staged),
            Some(ComposerAction::Stop) if self.agent_threads.contains(&tid) => self.ask_agent(
                &RecordId::new("agent_thread", tid.as_str()),
                "interrupt",
                String::new(),
                Vec::new(),
            ),
            Some(ComposerAction::Stop) | None => {}
        }
        ui.min_rect().bottom() - top
    }

    fn show_chat_content(&mut self, ui: &mut Ui) {
        if self.agent_threads.contains(&self.selected_thread) {
            let thread = database::schema::RecordId::new("agent_thread", self.selected_thread.as_str());
            crate::ui_data::agent_session_notify::mark_in_view(&thread);
        }
        let messages = self
            .threads
            .get(&self.selected_thread)
            .map(|t| t.messages.clone())
            .unwrap_or_default();

        if messages.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(120.);
                ui.label(RichText::new(format!("{}", icons::CHAT)).size(40.).weak());
                if self.self_diagnosis {
                    ui.heading(RichText::new("Diagnose this computer").strong());
                    ui.label(
                        RichText::new("Ask about the PC Mastertech is running on. The Codex agent inspects it with the Mastertech tools; anything that would run a command here waits for a technician's approval.")
                            .weak(),
                    );
                } else {
                    ui.heading(RichText::new("Mastertech Assistant").strong());
                    ui.label(RichText::new("Ask a question to get started.").weak());
                }
            });
            return;
        }

        let style = ChatStyle::from_ui(ui);
        let scope = Id::new(("ai_chat_rows", self.selected_thread.as_str()));
        let now = Local::now();
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| chat_rows(ui, &style, scope, &now, &messages));
    }

    /// Kicks off a one-time load of the user's persisted chat threads.
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let tx = self.load_tx.clone();
        PlatformSpawner::spawn(async move {
            if let Ok(serde_json::Value::Array(rows)) = database::schema::User::load_ai_chat_threads().await {
                let loaded: Vec<LoadedThread> = rows
                    .into_iter()
                    .filter_map(|r| {
                        let id = r.get("thread_id")?.as_str()?.to_string();
                        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        let messages = r
                            .get("messages")
                            .cloned()
                            .and_then(|m| serde_json::from_value::<Vec<ChatMessage>>(m).ok())
                            .unwrap_or_default();
                        Some(LoadedThread { id, title, messages })
                    })
                    .collect();
                let _ = tx.send(loaded);
            }
        });
    }

    fn thread_entry(&mut self, tid: &str) -> &mut ChatThread {
        self.threads.entry(tid.to_string()).or_insert_with(|| ChatThread {
            id: tid.to_string(),
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        })
    }

    fn upsert_stream(&mut self, tid: &str, id: &str, ts: i64, from: SentFrom, chunk: String, reasoning: bool) {
        let thread = self.thread_entry(tid);
        if let Some(m) = thread.messages.iter_mut().find(|m| m.id == id) {
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
            thread.messages.push(ChatMessage { id: id.to_string(), thread_id: tid.to_string(), ts, from, content });
        }
    }

    /// Persists one thread to the database (fire-and-forget).
    fn save_thread(&mut self, tid: &str) {
        if let Some(thread) = self.threads.get(tid) {
            // Streaming opens a Reasoning/Text block before any token arrives;
            // blocks that never received one would persist as empty noise.
            let keep: Vec<&ChatMessage> = thread
                .messages
                .iter()
                .filter(|m| match &m.content {
                    ChatMessageType::Text(t)
                    | ChatMessageType::Reasoning(t)
                    | ChatMessageType::Code(t) => !t.trim().is_empty(),
                    ChatMessageType::Done => false,
                    _ => true,
                })
                .collect();
            let messages = serde_json::to_value(&keep).unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
            let title = self.thread_title(tid);
            let id = tid.to_string();
            PlatformSpawner::spawn(async move {
                if let Err(e) = database::schema::User::save_ai_chat_thread(&id, &title, messages).await {
                    log::error!("save_ai_chat_thread: {e:?}");
                }
            });
        }
    }

    fn handle_enhanced_ai_events(&mut self, ui: &mut Ui) {
        // Merge any threads loaded from the database.
        while let Ok(loaded) = self.load_rx.try_recv() {
            let first = loaded.first().map(|l| l.id.clone());
            for lt in loaded {
                self.chat_title.insert(lt.id.clone(), lt.title);
                self.threads.entry(lt.id.clone()).or_insert_with(|| ChatThread {
                    id: lt.id.clone(),
                    messages: lt.messages,
                    images: Vec::new(),
                    input: String::new(),
                });
            }
            if !self.threads.contains_key(&self.selected_thread) {
                if let Some(f) = first {
                    self.selected_thread = f;
                }
            }
            ui.ctx().request_repaint();
        }

        #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
        while let Ok(thread) = self.agent_flag_rx.try_recv() {
            self.agent_threads.insert(thread);
        }
        #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
        while let Ok((local, key)) = self.agent_switch_rx.try_recv() {
            self.adopt_agent_thread(&local, key);
        }
        #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
        {
            self.poll_agent_index(ui);
            self.poll_agent_replies(ui);
            self.poll_agent_state(ui);
        }
        while let Ok(state) = self.state_rx.try_recv() {
            if state.thread == self.selected_thread {
                self.open_row = state.row;
                self.waiting = state.waiting;
            }
        }
        while let Ok((thread, turn)) = self.taken_back_rx.try_recv() {
            let ctx = ui.ctx().clone();
            if let (Some(composer), Some(chat)) = (
                self.composers.get_mut(&thread),
                self.threads.get_mut(&thread),
            ) {
                composer.restore(&ctx, &mut chat.input, turn);
            }
            self.last_state_poll = None;
        }

        while let Ok(response) = self.response_rx.try_recv() {
            ui.ctx().request_repaint();
            let id = response.id.clone();
            let tid = response.thread_id.clone();
            let ts = response.ts;
            let from = response.from.clone();
            match response.content {
                ChatMessageType::Text(chunk) => self.upsert_stream(&tid, &id, ts, from, chunk, false),
                ChatMessageType::Reasoning(chunk) => self.upsert_stream(&tid, &id, ts, from, chunk, true),
                // Turn finished — persist the full thread.
                ChatMessageType::Done => self.save_thread(&tid),
                other => {
                    self.thread_entry(&tid).messages.push(ChatMessage { id, thread_id: tid.clone(), ts, from, content: other });
                }
            }
        }
    }

    /// Re-keys a local thread onto the agent session it became, keeping what was typed.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn adopt_agent_thread(&mut self, local: &str, key: String) {
        if local == key {
            return;
        }
        if let Some(mut moved) = self.threads.remove(local) {
            moved.id = key.clone();
            match self.threads.get_mut(&key) {
                Some(existing) => existing.messages.extend(moved.messages),
                None => {
                    self.threads.insert(key.clone(), moved);
                }
            }
        }
        if let Some(engine) = self.thread_engine.remove(local) {
            self.thread_engine.insert(key.clone(), engine);
        }
        if let Some(title) = self.chat_title.remove(local) {
            self.chat_title.insert(key.clone(), title);
        }
        if let Some(composer) = self.composers.remove(local) {
            self.composers.insert(key.clone(), composer);
        }
        self.agent_threads.remove(local);
        self.agent_threads.insert(key.clone());
        self.hydrated.insert(key.clone());
        if self.selected_thread == local {
            self.select_thread(key);
        }
        self.last_agent_poll = None;
    }

    /// Sends one message as a `start`, `queue` or `steer` turn, or files the request that opens its session.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn send_to_agent(
        &mut self,
        thread_id: String,
        text: String,
        images: Vec<TurnImage>,
        kind: &'static str,
        connection_string: Option<String>,
    ) {
        use database::schema::AssistRequest;

        self.agent_threads.insert(thread_id.clone());
        self.last_state_poll = None;
        let session = self
            .agent_index
            .iter()
            .chain(&self.open_row)
            .any(|t| t.id.key_string() == thread_id);
        let target = connection_string.or_else(|| self.focused_client.clone());
        let user = crate::get_current_user_from_auth();
        let tech = user.as_ref().map(|u| u.get_email().to_string());
        let store = user
            .as_ref()
            .and_then(|u| serde_json::to_value(u).ok())
            .and_then(|v| v.get("store").and_then(serde_json::Value::as_str).map(str::to_string));
        let service_number = self.service_number.clone();
        let tx = self.response_tx.clone();
        let switch_tx = self.agent_switch_tx.clone();
        let tid = thread_id.clone();
        PlatformSpawner::spawn(async move {
            let say = |content: ChatMessageType| ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                thread_id: tid.clone(),
                ts: crate::tabs::ai_playground::now_ts(),
                from: SentFrom::Assistant,
                content,
            };
            if session {
                if let Err(e) = AgentTurn::ask_with(
                    &RecordId::new("agent_thread", tid.as_str()),
                    kind,
                    &text,
                    &images,
                )
                .await
                {
                    let _ = tx.try_send(say(ChatMessageType::Error(format!(
                        "could not queue the message: {e}"
                    ))));
                }
                return;
            }
            // No machine in scope: the technician's standing records-only session.
            let target = target.or_else(|| tech.as_deref().map(database::schema::general_connection));
            let Some(cs) = target else {
                let _ = tx.try_send(say(ChatMessageType::Error("Sign in to chat with the agent.".into())));
                return;
            };
            let general = database::schema::is_general(&cs);
            if !general {
                if let Some(block) = database::schema::ConnectedClient::diagnosis_block(&cs).await {
                    let _ = tx.try_send(say(ChatMessageType::Error(format!("Not dispatched — {cs}: {block}."))));
                    return;
                }
            }
            // A machine with a live session takes the message as a turn; otherwise a
            // request opens one. Either way this thread becomes that session.
            match AgentThread::active_for_connection(&cs).await {
                Ok(Some(thread)) => {
                    let kind = if thread.is_busy() && kind == "start" {
                        "queue"
                    } else {
                        kind
                    };
                    match AgentTurn::ask_with(&thread.id, kind, &text, &images).await {
                        Ok(_) => {
                            let _ = switch_tx.try_send((tid.clone(), thread.id.key_string()));
                        }
                        Err(e) => {
                            let _ = tx.try_send(say(ChatMessageType::Error(format!(
                                "could not queue the message: {e}"
                            ))));
                        }
                    }
                }
                _ => {
                    // A long message or pictures follow the opener as a queued turn.
                    let whole = images.is_empty() && text.chars().count() <= REQUEST_NOTE_MAX;
                    let note = if whole { text.as_str() } else { OPENER };
                    match AssistRequest::create_from_chat(
                        &cs,
                        tech.as_deref(),
                        store.as_deref(),
                        service_number.as_deref(),
                        note,
                    )
                    .await
                    {
                        Ok(request) => {
                            let what = if general {
                                "your records session".to_string()
                            } else {
                                format!("a session for {cs}")
                            };
                            let _ = tx.try_send(say(ChatMessageType::Text(format!(
                                "Asked the agent host to open {what}\u{2026}"
                            ))));
                            for _ in 0..45 {
                                database::sleep_compat(std::time::Duration::from_secs(2)).await;
                                if let Ok(Some(req)) = AssistRequest::get(&request).await {
                                    if let Some(thread) = req.agent_thread {
                                        if !whole
                                            && let Err(e) = AgentTurn::ask_with(
                                                &thread, "queue", &text, &images,
                                            )
                                            .await
                                        {
                                            let _ = tx.try_send(say(ChatMessageType::Error(
                                                format!("could not queue the message: {e}"),
                                            )));
                                        }
                                        let _ =
                                            switch_tx.try_send((tid.clone(), thread.key_string()));
                                        return;
                                    }
                                    if req.status == "failed" {
                                        let _ = tx.try_send(say(ChatMessageType::Error(format!(
                                            "the agent host could not open a session: {}",
                                            req.dispatch_error
                                                .unwrap_or_else(|| "unknown error".into())
                                        ))));
                                        return;
                                    }
                                }
                            }
                            let _ = tx.try_send(say(ChatMessageType::Error(
                                "no agent session opened within 90 seconds; is admin-agent running?".into(),
                            )));
                        }
                        Err(e) => {
                            let _ = tx.try_send(say(ChatMessageType::Error(format!(
                                "could not request a diagnosis: {e}"
                            ))));
                        }
                    }
                }
            }
        });
    }

    /// Writes one turn row for a session, reporting a failure in its chat.
    fn ask_agent(
        &mut self,
        thread: &RecordId,
        kind: &'static str,
        text: String,
        images: Vec<TurnImage>,
    ) {
        self.last_state_poll = None;
        let tx = self.response_tx.clone();
        let (thread, tid) = (thread.clone(), thread.key_string());
        PlatformSpawner::spawn(async move {
            if let Err(e) = AgentTurn::ask_with(&thread, kind, &text, &images).await {
                let _ = tx.try_send(ChatMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    thread_id: tid,
                    ts: crate::tabs::ai_playground::now_ts(),
                    from: SentFrom::Assistant,
                    content: ChatMessageType::Error(format!(
                        "could not send the {kind} request: {e}"
                    )),
                });
            }
        });
    }

    /// Resumes the held queue, removes a queued message, or takes one back into the composer.
    fn apply_queue_action(&mut self, tid: &str, action: QueueAction) {
        let thread = RecordId::new("agent_thread", tid);
        match action {
            QueueAction::Resume => self.ask_agent(&thread, "queue", String::new(), Vec::new()),
            QueueAction::Remove(id) => {
                self.waiting.retain(|w| w.id != id);
                PlatformSpawner::spawn(async move {
                    if let Err(e) = AgentTurn::cancel(&id).await {
                        log::warn!("could not remove a queued message: {e}");
                    }
                });
            }
            QueueAction::Edit(id) => {
                self.waiting.retain(|w| w.id != id);
                let tx = self.taken_back_tx.clone();
                let tid = tid.to_string();
                PlatformSpawner::spawn(async move {
                    match AgentTurn::take_back(&id).await {
                        Ok(Some(turn)) => {
                            let _ = tx.send((tid, turn));
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("could not take a queued message back: {e}"),
                    }
                });
            }
        }
        self.last_state_poll = None;
    }

    /// Re-reads the open agent chat's session row and its queue.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn poll_agent_state(&mut self, ui: &Ui) {
        use std::time::Duration;
        const EVERY: Duration = Duration::from_secs(2);
        if !self.agent_threads.contains(&self.selected_thread) {
            return;
        }
        let now = web_time::Instant::now();
        if self
            .last_state_poll
            .is_some_and(|t| now.duration_since(t) < EVERY)
        {
            return;
        }
        self.last_state_poll = Some(now);
        ui.ctx().request_repaint_after(EVERY);
        let tx = self.state_tx.clone();
        let thread = self.selected_thread.clone();
        PlatformSpawner::spawn(async move {
            let id = RecordId::new("agent_thread", thread.as_str());
            let (Ok(row), Ok(waiting)) =
                (AgentThread::get(&id).await, AgentTurn::waiting(&id).await)
            else {
                return;
            };
            let _ = tx.send(AgentState {
                thread,
                row,
                waiting,
            });
        });
    }

    /// Refreshes the list of agent conversations this user may open. A
    /// technician sees only their own; root sees every one, which is the only
    /// way to answer a conversation the tech who opened it has gone home on.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn poll_agent_index(&mut self, ui: &Ui) {
        use std::time::Duration;
        const EVERY: Duration = Duration::from_secs(15);

        while let Ok(index) = self.agent_index_rx.try_recv() {
            self.agent_index = index;
        }
        let now = web_time::Instant::now();
        if self.last_index_poll.is_some_and(|t| now.duration_since(t) < EVERY) {
            return;
        }
        self.last_index_poll = Some(now);
        ui.ctx().request_repaint_after(EVERY);

        let tx = self.agent_index_tx.clone();
        PlatformSpawner::spawn(async move {
            use database::schema::{AgentThread, User, UserAuthorization};
            // Scope is derived here, not passed in: a stale cached flag would
            // widen what a technician can read.
            let me = User::get_current_user_from_auth().await.ok().flatten();
            let root = me
                .as_ref()
                .is_some_and(|u| u.get_authorization() == UserAuthorization::Root);
            let scope = if root { None } else { me.as_ref().map(|u| u.get_email().to_string()) };
            // A signed-out client scopes to nobody rather than to everybody.
            if !root && scope.is_none() {
                let _ = tx.try_send(Vec::new());
                return;
            }
            match AgentThread::list_recent(200, true).await {
                Ok(threads) => {
                    let index: Vec<AgentThread> = threads
                        .into_iter()
                        .filter(|t| scope.as_deref().is_none_or(|me| t.requested_by.as_deref() == Some(me)))
                        .collect();
                    let _ = tx.try_send(index);
                }
                Err(e) => log::warn!("poll_agent_index: {e}"),
            }
        });
    }

    /// Opens a conversation from the index, backfilling both sides on first view.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn open_agent_thread(&mut self, thread: String) {
        self.agent_threads.insert(thread.clone());
        self.threads.entry(thread.clone()).or_insert_with(|| ChatThread {
            id: thread.clone(),
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        });
        self.select_thread(thread);
        // Force the next reply poll rather than waiting out the interval.
        self.last_agent_poll = None;
    }

    /// Pulls agent replies for the open thread. Messages carry their row id, so
    /// the thread's own contents are the dedupe set and no extra state is kept.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn poll_agent_replies(&mut self, ui: &Ui) {
        use std::time::Duration;
        let gap = if self.agent_threads.contains(&self.selected_thread) { 2 } else { 8 };
        let now = web_time::Instant::now();
        if self.last_agent_poll.is_some_and(|t| now.duration_since(t) < Duration::from_secs(gap)) {
            return;
        }
        self.last_agent_poll = Some(now);
        ui.ctx().request_repaint_after(Duration::from_secs(gap));

        let thread = self.selected_thread.clone();
        let seen: std::collections::HashSet<String> = self
            .threads
            .get(&thread)
            .map(|t| t.messages.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default();
        let tx = self.response_tx.clone();
        let flag_tx = self.agent_flag_tx.clone();
        // A thread with nothing rendered yet is being read for the first time, so
        // the tech's own side is backfilled too. Afterwards it is skipped, or the
        // author's local echo would be duplicated by its database copy.
        let hydrate = seen.is_empty() && !self.hydrated.contains(&thread);
        if hydrate {
            self.hydrated.insert(thread.clone());
        }
        PlatformSpawner::spawn(async move {
            use database::schema::{AgentEvent, RecordId};
            let rows = AgentEvent::history(&RecordId::new("agent_thread", thread.as_str()), 0, 300)
                .await
                .unwrap_or_default();
            if !rows.is_empty() {
                let _ = flag_tx.try_send(thread.clone());
            }
            for row in rows {
                // A streaming row changes under one id; it lands once complete.
                if !row.done {
                    continue;
                }
                let id = row.id.key_string();
                if seen.contains(&id) {
                    continue;
                }
                let (from, content) = match row.kind.as_str() {
                    "agent" => (SentFrom::Assistant, ChatMessageType::Text(row.text.clone())),
                    "error" => (SentFrom::Assistant, ChatMessageType::Error(row.text.clone())),
                    "tool_call" | "command" => (
                        SentFrom::Assistant,
                        ChatMessageType::Text(format!(
                            "{TOOL_PREFIX}{}",
                            crate::tabs::agent_sessions::chat_line(&row).unwrap_or_default()
                        )),
                    ),
                    "approval" => (
                        SentFrom::Assistant,
                        ChatMessageType::Text(format!("{} {}", icons::LOCK, row.text)),
                    ),
                    "other" if !row.text.trim().is_empty() => (
                        SentFrom::Assistant,
                        ChatMessageType::Text(format!("{NOTICE_PREFIX} {}", row.text)),
                    ),
                    "user" if hydrate => (SentFrom::Me, ChatMessageType::Text(row.text.clone())),
                    _ => continue,
                };
                let ts = row
                    .created_at
                    .map(|at| DateTime::<Utc>::from(at).timestamp())
                    .unwrap_or_else(crate::tabs::ai_playground::now_ts);
                let pictures = if from == SentFrom::Me {
                    agent_chat::attach::image_names(row.item.as_ref())
                } else {
                    Vec::new()
                };
                let _ = tx.try_send(ChatMessage {
                    id: id.clone(),
                    thread_id: thread.clone(),
                    ts,
                    from,
                    content,
                });
                for (n, name) in pictures.into_iter().enumerate() {
                    let content = ChatMessageType::Image((name, bytes::Bytes::new()));
                    let _ = tx.try_send(ChatMessage {
                        id: format!("{id}:image{n}"),
                        thread_id: thread.clone(),
                        ts,
                        from: SentFrom::Me,
                        content,
                    });
                }
            }
        });
    }

    fn create_new_chat_thread(&mut self) {
        let thread_id = uuid::Uuid::new_v4().to_string();
        self.select_thread(thread_id.clone());
        self.threads.insert(thread_id.clone(), ChatThread {
            id: thread_id,
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        });
    }

    /// Echoes a composed message and its pictures into the open thread and sends it to the agent.
    fn send_chat_message(
        &mut self,
        kind: &'static str,
        text: String,
        images: Vec<TurnImage>,
        staged: Vec<String>,
    ) {
        if !self.threads.contains_key(&self.selected_thread) {
            self.create_new_chat_thread();
        }
        let thread_id = self.selected_thread.clone();
        let ts = crate::tabs::ai_playground::now_ts();
        let echo = |content: ChatMessageType| ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread_id.clone(),
            ts,
            from: SentFrom::Me,
            content,
        };
        let _ = self
            .response_tx
            .try_send(echo(ChatMessageType::Text(text.clone())));
        for name in staged {
            let _ = self
                .response_tx
                .try_send(echo(ChatMessageType::Image((name, bytes::Bytes::new()))));
        }

        // Every message goes to the agent: a session thread continues, a focused
        // machine gets its session, anything else the technician's records session.
        #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
        {
            if !self.agent_threads.contains(&thread_id) {
                self.thread_engine.insert(thread_id.clone(), "Codex agent".to_string());
            }
            self.send_to_agent(thread_id, text, images, kind, None);
        }
        #[cfg(not(any(target_arch = "wasm32", feature = "tokio")))]
        {
            let _ = (text, images, kind, thread_id);
        }
    }
}

/// The composer id of a chat thread.
fn composer_id(thread: &str) -> Id {
    Id::new(("enhanced_ai_composer", thread))
}

fn short_title(s: &str) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= 28 {
        t
    } else {
        format!("{}…", t.chars().take(28).collect::<String>())
    }
}

/// True for assistant tool-activity lines emitted with `TOOL_PREFIX`.
fn is_tool_line(message: &ChatMessage) -> bool {
    match (&message.from, &message.content) {
        (SentFrom::Assistant, ChatMessageType::Text(t)) => t.starts_with(TOOL_PREFIX),
        _ => false,
    }
}

/// The staged file name of a picture message.
fn picture_name(message: &ChatMessage) -> Option<&str> {
    match &message.content {
        ChatMessageType::Image((name, _)) => Some(name.as_str()),
        _ => None,
    }
}

/// Draws a thread's messages, folding tool-line runs into one row and trailing pictures into their message.
fn chat_rows(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    now: &DateTime<Local>,
    messages: &[ChatMessage],
) {
    let mut i = 0;
    while i < messages.len() {
        if is_tool_line(&messages[i]) {
            let start = i;
            while i < messages.len() && is_tool_line(&messages[i]) {
                i += 1;
            }
            tool_group(ui, style, scope, now, &messages[start..i]);
        } else {
            let mut end = i + 1;
            while end < messages.len() && picture_name(&messages[end]).is_some() {
                end += 1;
            }
            let pictures: Vec<String> = messages[i + 1..end]
                .iter()
                .filter_map(picture_name)
                .map(str::to_string)
                .collect();
            chat_message(ui, style, scope, now, &messages[i], &pictures);
            i = end;
        }
    }
}

/// Local clock time of a message's unix-seconds stamp.
fn message_time(ts: i64, now: &DateTime<Local>) -> Option<String> {
    let at = DateTime::<Utc>::from_timestamp(ts, 0).filter(|_| ts > 0)?;
    Some(chat_bubble::local_clock(at, now))
}

fn chat_message(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    now: &DateTime<Local>,
    message: &ChatMessage,
    pictures: &[String],
) {
    let time = message_time(message.ts, now);
    let key = message.id.as_str();
    match &message.content {
        ChatMessageType::Reasoning(text) => {
            if text.trim().is_empty() {
                return;
            }
            ChatRow::new(ChatKind::Reasoning, key, "Thinking")
                .time(time)
                .copy(text)
                .summary(chat_bubble::summary_line(text, SUMMARY_CHARS))
                .show(ui, style, scope, |ui, id| {
                    chat_bubble::markdown(ui, style, text, style.text, id)
                });
        }
        ChatMessageType::Error(text) => {
            ChatRow::new(ChatKind::Error, key, "Error")
                .time(time)
                .copy(text)
                .show(ui, style, scope, |ui, id| {
                    chat_bubble::markdown(ui, style, text, style.error, id)
                });
        }
        ChatMessageType::Text(text)
        | ChatMessageType::Code(text)
        | ChatMessageType::FileId(text) => {
            let approval = match message.from {
                SentFrom::Assistant => text.strip_prefix(icons::LOCK).map(str::trim_start),
                SentFrom::Me => None,
            };
            if let Some(text) = approval {
                ChatRow::new(ChatKind::Approval, key, "Approval")
                    .time(time)
                    .copy(text)
                    .summary(chat_bubble::summary_line(text, SUMMARY_CHARS))
                    .show(ui, style, scope, |ui, id| {
                        chat_bubble::markdown(ui, style, text, style.text, id)
                    });
                return;
            }
            if let Some(notice) = text
                .strip_prefix(NOTICE_PREFIX)
                .filter(|_| message.from == SentFrom::Assistant)
            {
                chat_bubble::notice(ui, style, notice, time.as_deref());
                return;
            }
            match message.from {
                SentFrom::Me => {
                    ChatRow::new(ChatKind::User, key, "You")
                        .time(time)
                        .copy(text)
                        .has_body(!text.trim().is_empty() || !pictures.is_empty())
                        .show(ui, style, scope, |ui, id| {
                            agent_chat::user_body(ui, style, text, pictures, id)
                        });
                }
                SentFrom::Assistant => {
                    ChatRow::new(ChatKind::Agent, key, "Assistant")
                        .time(time)
                        .copy(text)
                        .has_body(!text.trim().is_empty())
                        .show(ui, style, scope, |ui, id| {
                            chat_bubble::markdown(ui, style, text, style.text, id)
                        });
                }
            }
        }
        ChatMessageType::Image((name, _)) => {
            let names: Vec<String> = std::iter::once(name.clone())
                .chain(pictures.iter().cloned())
                .collect();
            agent_chat::sent_images(ui, &names);
        }
        ChatMessageType::Done => {}
    }
}

/// Consecutive tool lines as one collapsible row, with a nested row per call when there are several.
fn tool_group(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    now: &DateTime<Local>,
    group: &[ChatMessage],
) {
    let calls: Vec<(&ChatMessage, &str, ToolLine<'_>)> = group
        .iter()
        .filter_map(|m| match &m.content {
            ChatMessageType::Text(t) => Some((m, t.as_str(), ToolLine::parse(t))),
            _ => None,
        })
        .collect();
    let Some((first, _, _)) = calls.first() else {
        return;
    };
    let key = format!("tools:{}", first.id);
    let failed = calls.iter().filter(|(_, _, c)| c.failed()).count();
    let (label, summary) = match calls.as_slice() {
        [(_, _, only)] => (
            chat_bubble::tool_label(only.name).to_string(),
            only.summary(ui),
        ),
        _ => {
            let names: Vec<&str> = calls
                .iter()
                .map(|(_, _, c)| chat_bubble::tool_label(c.name))
                .collect();
            (
                format!("Tools ({})", calls.len()),
                chat_bubble::clip(&names.join(", "), SUMMARY_CHARS).into_owned(),
            )
        }
    };
    let mut row = ChatRow::new(ChatKind::Tool, &key, &label)
        .time(message_time(first.ts, now))
        .summary(summary)
        .default_open(failed > 0);
    if let [(_, _, only)] = calls.as_slice() {
        if only.failed() {
            row = row.badge("failed", style.error);
        } else if !only.status.is_empty() {
            row = row.badge(only.status, style.weak);
        }
    } else if failed > 0 {
        row = row.badge(format!("{failed} failed"), style.error);
    }
    let copied = row.show(ui, style, scope, |ui, id| match calls.as_slice() {
        [(_, _, only)] => only.body(ui, style, id),
        _ => {
            for (m, raw, call) in &calls {
                let mut sub =
                    ChatRow::new(ChatKind::Tool, &m.id, chat_bubble::tool_label(call.name))
                        .nested(true)
                        .copy(raw.trim_start_matches(TOOL_PREFIX))
                        .summary(call.summary(ui))
                        .default_open(call.failed())
                        .has_body(call.has_payload());
                if call.failed() {
                    sub = sub.badge("failed", style.error);
                } else if !call.status.is_empty() {
                    sub = sub.badge(call.status, style.weak);
                }
                sub.show(ui, style, id, |ui, sub_id| call.body(ui, style, sub_id));
            }
        }
    });
    if copied {
        let all: Vec<&str> = calls
            .iter()
            .map(|(_, raw, _)| raw.trim_start_matches(TOOL_PREFIX))
            .collect();
        ui.ctx().copy_text(all.join("\n\n"));
    }
}

/// A `» name (arguments) status` tool line with its result or error after the first newline.
#[derive(Debug, PartialEq, Eq)]
struct ToolLine<'a> {
    name: &'a str,
    args: &'a str,
    status: &'a str,
    detail: &'a str,
}

impl<'a> ToolLine<'a> {
    /// Reads `name (args) status`, the older `name(args)`, and the legacy `name (args) → status`.
    fn parse(text: &'a str) -> Self {
        let (head, detail) = text.split_once('\n').unwrap_or((text, ""));
        let body = head.strip_prefix(TOOL_PREFIX).unwrap_or(head).trim();
        let (body, legacy_status) = match body.rsplit_once(" \u{2192} ") {
            Some((call, status)) => (call.trim(), Some(status.trim())),
            None => (body, None),
        };
        let detail = detail.trim();
        let Some(open) = body.find('(') else {
            return Self {
                name: body,
                args: "",
                status: legacy_status.unwrap_or(""),
                detail,
            };
        };
        let rest = &body[open + 1..];
        let (args, status) = match rest.rfind(')') {
            Some(close) => (rest[..close].trim(), rest[close + 1..].trim()),
            None => (rest.trim(), ""),
        };
        Self {
            name: body[..open].trim(),
            args,
            status: legacy_status.unwrap_or(status),
            detail,
        }
    }

    fn failed(&self) -> bool {
        let status = self.status.to_ascii_lowercase();
        ["fail", "error", "declin", "denied"]
            .iter()
            .any(|w| status.contains(w))
            || status
                .strip_prefix("exit ")
                .is_some_and(|code| code.trim() != "0")
    }

    fn has_payload(&self) -> bool {
        !self.args.is_empty() || !self.detail.is_empty()
    }

    fn summary(&self, ui: &Ui) -> String {
        chat_bubble::payload_summary(ui.ctx(), self.args, SUMMARY_CHARS)
    }

    fn body(&self, ui: &mut Ui, style: &ChatStyle, id: Id) {
        if !self.args.is_empty() {
            chat_bubble::caption(ui, style, "Arguments");
            chat_bubble::payload(ui, style, self.args, style.text, id.with("args"));
        }
        if !self.detail.is_empty() {
            let (title, ink) = if self.failed() {
                ("Error", style.error)
            } else {
                ("Result", style.text)
            };
            chat_bubble::caption(ui, style, title);
            chat_bubble::payload(ui, style, self.detail, ink, id.with("detail"));
        }
        if !self.has_payload() {
            chat_bubble::caption(ui, style, "No arguments or result were recorded.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_lines_split_into_name_arguments_status_and_result() {
        let line =
            format!("{TOOL_PREFIX}get_client_info ({{\"a\":\"(x)\"}}) 1.2 s\n{{\"ok\":true}}\n");
        assert_eq!(
            ToolLine::parse(&line),
            ToolLine {
                name: "get_client_info",
                args: "{\"a\":\"(x)\"}",
                status: "1.2 s",
                detail: "{\"ok\":true}"
            }
        );
        let old = format!("{TOOL_PREFIX}query_surrealdb({{\"query\":\"SELECT 1\"}})");
        assert_eq!(
            ToolLine::parse(&old),
            ToolLine {
                name: "query_surrealdb",
                args: "{\"query\":\"SELECT 1\"}",
                status: "",
                detail: ""
            }
        );
        assert_eq!(ToolLine::parse("» bare").name, "bare");
    }

    #[test]
    fn legacy_arrow_status_lines_parse() {
        let line = format!("{TOOL_PREFIX}remote_channel_health ({{\"cs\": \"D:1\"}}) \u{2192} ok");
        let t = ToolLine::parse(&line);
        assert_eq!((t.name, t.args, t.status), ("remote_channel_health", "{\"cs\": \"D:1\"}", "ok"));
        let cut = format!("{TOOL_PREFIX}ToolSearch ({{\"query\": \"select:a,cre\u{2026}) \u{2192} ok");
        let t = ToolLine::parse(&cut);
        assert_eq!((t.name, t.args, t.status), ("ToolSearch", "{\"query\": \"select:a,cre\u{2026}", "ok"));
        let bare = format!("{TOOL_PREFIX}remote_egui_list_targets \u{2192} ok");
        let t = ToolLine::parse(&bare);
        assert_eq!((t.name, t.args, t.status), ("remote_egui_list_targets", "", "ok"));
    }

    #[test]
    fn failures_are_read_from_the_status_word() {
        let status = |s: &'static str| ToolLine {
            name: "t",
            args: "",
            status: s,
            detail: "",
        };
        for failed in ["failed", "error", "Declined", "exit 2"] {
            assert!(status(failed).failed(), "{failed}");
        }
        for ok in ["", "1.2 s", "exit 0", "ok"] {
            assert!(!status(ok).failed(), "{ok}");
        }
    }

    #[test]
    fn message_times_skip_a_missing_stamp() {
        let now = Local::now();
        assert_eq!(message_time(0, &now), None);
        assert!(message_time(1_790_000_000, &now).is_some());
    }

    #[test]
    fn every_message_kind_draws_open_and_closed_inside_the_viewport() {
        use eframe::egui::{Context, RawInput, Rect, pos2, vec2};
        let long = "y".repeat(3_000);
        let message = |id: &str, from: SentFrom, content: ChatMessageType| ChatMessage {
            id: id.into(),
            thread_id: "t".into(),
            ts: 1_790_000_000,
            from,
            content,
        };
        let tool = |id: &str, line: String| {
            message(
                id,
                SentFrom::Assistant,
                ChatMessageType::Text(format!("{TOOL_PREFIX}{line}")),
            )
        };
        let messages = vec![
            message(
                "a",
                SentFrom::Me,
                ChatMessageType::Text("Why is **PC-1** slow?".into()),
            ),
            message(
                "b",
                SentFrom::Assistant,
                ChatMessageType::Reasoning(format!("thinking {long}")),
            ),
            tool(
                "c",
                format!("get_client_info ({{\"k\":\"{long}\"}}) 1.2 s\n{{\"cpu\":\"{long}\"}}"),
            ),
            message(
                "d",
                SentFrom::Assistant,
                ChatMessageType::Text(format!("## Found\n```\n{long}\n```\n{long}")),
            ),
            tool("e", "query_surrealdb({\"q\":\"SELECT 1\"})".into()),
            tool("f", format!("run_script ({{}}) failed\nerror: {long}")),
            message(
                "g",
                SentFrom::Assistant,
                ChatMessageType::Text(format!(
                    "{} Waiting for a technician to approve: run x",
                    icons::LOCK
                )),
            ),
            message(
                "h",
                SentFrom::Assistant,
                ChatMessageType::Error("could not queue the message".into()),
            ),
            message(
                "i",
                SentFrom::Me,
                ChatMessageType::Text(format!("see the log\n\n**setup.log**\n```log\n{long}\n```")),
            ),
            message(
                "j",
                SentFrom::Me,
                ChatMessageType::Image(("shot-0a1b2c3d.png".into(), bytes::Bytes::new())),
            ),
            message(
                "k",
                SentFrom::Assistant,
                ChatMessageType::Text(format!("{NOTICE_PREFIX} Queue held with 1 waiting: {long}")),
            ),
        ];
        let ctx = Context::default();
        let scope = Id::new(("ai_chat_rows", "t"));
        let now = Local::now();
        let render = || {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 900.0))),
                ..Default::default()
            };
            let mut size = vec2(0.0, 0.0);
            let mut out = ctx.run_ui(input, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    chat_rows(ui, &ChatStyle::from_ui(ui), scope, &now, &messages);
                    size = ui.min_rect().size();
                });
            });
            out.textures_delta.clear();
            size
        };
        let closed = render();
        ctx.data_mut(|d| {
            for m in &messages {
                d.insert_temp(scope.with(m.id.as_str()).with("open"), true);
            }
            d.insert_temp(scope.with("tools:c").with("open"), true);
            let group = scope.with("tools:e");
            d.insert_temp(group.with("open"), true);
            for id in ["e", "f"] {
                d.insert_temp(group.with(id).with("open"), true);
            }
        });
        let open = render();
        let settled = render();
        for size in [closed, open, settled] {
            assert!(
                (300.0..=421.0).contains(&size.x),
                "content {} px wide in a 420 px viewport",
                size.x
            );
        }
        assert!(
            open.y > closed.y + 400.0,
            "opening every row grew the chat from {} to {} px",
            closed.y,
            open.y
        );
    }
}
