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
/// Interval between reads of a followed assist request.
const FOLLOW_POLL: std::time::Duration = std::time::Duration::from_secs(2);
/// How long a followed request keeps its chat's composer locked.
const FOLLOW_LOCK: std::time::Duration = std::time::Duration::from_secs(90);
/// How long a followed request is read before its chat gives up on it.
const FOLLOW_LIMIT: std::time::Duration = std::time::Duration::from_secs(600);

/// What a followed assist request reports to the chat waiting on it.
#[derive(Debug, Clone, PartialEq)]
enum FollowEvent {
    /// The composer lock ran out; the request is still followed.
    Slow,
    /// The broker linked this session; `echoed` when the chat already shows the tech's message.
    Opened { key: String, echoed: bool },
    /// The request failed or was never linked.
    Failed(String),
}

/// A placeholder chat waiting for the agent session of one assist request.
pub struct PendingSession {
    local: String,
    tx: Sender<(String, FollowEvent)>,
}

impl PendingSession {
    /// Shows `message` in the waiting chat and stops waiting.
    pub fn fail(&self, message: String) {
        let _ = self.tx.send((self.local.clone(), FollowEvent::Failed(message)));
    }

    /// Follows `request` until the broker links its session, then opens it in the waiting chat.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    pub async fn follow(self, request: RecordId) {
        let event = match await_session(&request, &self.local, &self.tx).await {
            Ok(thread) => FollowEvent::Opened { key: thread.key_string(), echoed: false },
            Err(message) => FollowEvent::Failed(message),
        };
        let _ = self.tx.send((self.local, event));
    }
}

/// Reads `request` until the broker links its session, reporting `Slow` once `FOLLOW_LOCK` passes.
#[cfg(any(target_arch = "wasm32", feature = "tokio"))]
async fn await_session(
    request: &RecordId,
    local: &str,
    tx: &Sender<(String, FollowEvent)>,
) -> Result<RecordId, String> {
    use database::schema::AssistRequest;

    let mut waited = std::time::Duration::ZERO;
    let mut slow = false;
    loop {
        if let Ok(Some(req)) = AssistRequest::get(request).await {
            if let Some(thread) = req.agent_thread {
                return Ok(thread);
            }
            if req.status == "failed" {
                return Err(format!(
                    "the agent host could not open a session: {}",
                    req.dispatch_error.unwrap_or_else(|| "unknown error".into())
                ));
            }
        }
        if waited >= FOLLOW_LIMIT {
            // Withdraws the request; one already claimed opens if it is linked by now.
            if let Ok(false) = AssistRequest::withdraw(request).await
                && let Ok(Some(req)) = AssistRequest::get(request).await
                && let Some(thread) = req.agent_thread
            {
                return Ok(thread);
            }
            return Err(format!(
                "no agent session opened within {} minutes; is admin-agent running?",
                FOLLOW_LIMIT.as_secs() / 60
            ));
        }
        if !slow && waited >= FOLLOW_LOCK {
            slow = true;
            let _ = tx.send((local.to_string(), FollowEvent::Slow));
        }
        database::sleep_compat(FOLLOW_POLL).await;
        waited += FOLLOW_POLL;
    }
}

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
    /// Threads showing a local echo of the tech's messages, whose user rows the poller skips.
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
    /// When set, a new chat's first message opens its own agent session instead of joining the machine's live one.
    #[serde(skip)]
    pub fresh_sessions: bool,
    /// Local chats whose composer is locked while their request opens a session.
    #[serde(skip)]
    following: std::collections::HashSet<String>,
    /// Local chats still waiting for their request's session after the composer lock ran out.
    #[serde(skip)]
    lingering: std::collections::HashSet<String>,
    /// Messages sent from a lingering chat, queued on its session once it opens.
    #[serde(skip)]
    held: HashMap<String, Vec<(String, Vec<TurnImage>)>>,
    /// Progress of followed requests, by the local chat waiting on each.
    #[serde(skip)]
    follow_tx: Sender<(String, FollowEvent)>,
    #[serde(skip)]
    follow_rx: Receiver<(String, FollowEvent)>,
    /// Local chats re-keyed onto an agent session, so late messages reach the session.
    #[serde(skip)]
    rekeyed: HashMap<String, String>,
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
        let (follow_tx, follow_rx) = crossbeam::channel::unbounded::<(String, FollowEvent)>();
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
            fresh_sessions: false,
            following: std::collections::HashSet::new(),
            lingering: std::collections::HashSet::new(),
            held: HashMap::new(),
            follow_tx,
            follow_rx,
            rekeyed: HashMap::new(),
        }
    }
}

impl EnhancedAiPlayground {
    /// Returns and clears the "close panel" request raised by the top-bar ✕.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Selects an empty local chat: the open one when blank, else another blank one, else a new one.
    pub fn start_new_session(&mut self) {
        if self.is_blank_chat(&self.selected_thread) {
            return;
        }
        match self.threads.keys().find(|id| self.is_blank_chat(id)).cloned() {
            Some(blank) => self.select_thread(blank),
            None => self.create_new_chat_thread(),
        }
    }

    /// True for a local chat with nothing typed, attached or sent, and no session behind it.
    fn is_blank_chat(&self, id: &str) -> bool {
        self.threads
            .get(id)
            .is_some_and(|t| t.messages.is_empty() && t.input.trim().is_empty())
            && self.composers.get(id).is_none_or(|c| c.attachments.is_empty())
            && !self.agent_threads.contains(id)
            && !self.following.contains(id)
            && !self.lingering.contains(id)
    }

    /// Selects a placeholder chat titled `label` that opens the session its assist request gets.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    pub fn follow_assist_request(&mut self, label: String) -> PendingSession {
        let blanks: Vec<String> = self.threads.keys().filter(|id| self.is_blank_chat(id)).cloned().collect();
        for blank in blanks {
            self.threads.remove(&blank);
            self.composers.remove(&blank);
            self.chat_title.remove(&blank);
            self.thread_engine.remove(&blank);
        }
        let local = uuid::Uuid::new_v4().to_string();
        let notice = ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: local.clone(),
            ts: crate::tabs::ai_playground::now_ts(),
            from: SentFrom::Assistant,
            content: ChatMessageType::Text(
                "Asked the agent host to open a session for this computer\u{2026}".to_string(),
            ),
        };
        self.threads.insert(
            local.clone(),
            ChatThread { id: local.clone(), messages: vec![notice], images: Vec::new(), input: String::new() },
        );
        self.chat_title.insert(local.clone(), label);
        self.thread_engine.insert(local.clone(), "Codex agent".to_string());
        self.following.insert(local.clone());
        self.select_thread(local.clone());
        PendingSession { local, tx: self.follow_tx.clone() }
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
                    if agent_chat::approve_all_chip(ui, &row) {
                        let prompt = database::schema::agent_turn::APPROVALS_PROMPT.to_string();
                        self.ask_agent(&row.id, "approvals", prompt, Vec::new());
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
        let busy = self
            .open_row
            .as_ref()
            .filter(|r| r.id.key_string() == tid)
            .is_some_and(AgentThread::is_busy);
        let enabled = self.composer_enabled(&tid);
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
            enabled,
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

    /// Whether the composer of `tid` takes input: its session is open and it is not waiting for one.
    fn composer_enabled(&self, tid: &str) -> bool {
        let row = self.open_row.as_ref().filter(|r| r.id.key_string() == tid);
        row.is_none_or(AgentThread::is_open) && !self.following.contains(tid)
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

    /// Adds threads loaded from the database, selecting the newest when no known thread is selected.
    fn merge_loaded(&mut self, loaded: Vec<LoadedThread>) {
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
    }

    fn handle_enhanced_ai_events(&mut self, ui: &mut Ui) {
        while let Ok(loaded) = self.load_rx.try_recv() {
            self.merge_loaded(loaded);
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
        while let Ok((local, event)) = self.follow_rx.try_recv() {
            self.apply_follow(&local, event);
        }
        if !self.following.is_empty() || !self.lingering.is_empty() {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
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
            self.apply_response(response);
        }
    }

    /// Adds one delivered message to its thread, or to the session its local chat became.
    fn apply_response(&mut self, response: ChatMessage) {
        let id = response.id.clone();
        let tid = self
            .rekeyed
            .get(&response.thread_id)
            .cloned()
            .unwrap_or_else(|| response.thread_id.clone());
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

    /// Applies what a followed request reported to the chat waiting on it.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn apply_follow(&mut self, local: &str, event: FollowEvent) {
        match event {
            FollowEvent::Slow => {
                if self.following.remove(local) {
                    self.lingering.insert(local.to_string());
                    self.notify(
                        local,
                        ChatMessageType::Text(
                            "The agent host has not opened the session yet. It opens here when it does, \
                             and messages you send meanwhile go to it."
                                .into(),
                        ),
                    );
                }
            }
            FollowEvent::Failed(message) => {
                let held = self.held.remove(local).map_or(0, |h| h.len());
                if self.stop_waiting(local) {
                    self.notify(local, ChatMessageType::Error(message));
                    let unsent = match held {
                        0 => None,
                        1 => Some("The message typed while waiting was not sent.".to_string()),
                        n => Some(format!("The {n} messages typed while waiting were not sent.")),
                    };
                    if let Some(unsent) = unsent {
                        self.notify(local, ChatMessageType::Error(unsent));
                    }
                }
            }
            FollowEvent::Opened { key, echoed } => {
                let held = self.held.remove(local).unwrap_or_default();
                if !self.stop_waiting(local) {
                    return;
                }
                if echoed {
                    self.adopt_agent_thread(local, key.clone());
                } else {
                    self.apply_opened(local, key.clone());
                }
                if !held.is_empty() {
                    self.queue_held(&key, held);
                }
            }
        }
    }

    /// Queues messages held for `key` while it opened, in the order they were sent.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn queue_held(&mut self, key: &str, held: Vec<(String, Vec<TurnImage>)>) {
        self.last_state_poll = None;
        let tx = self.response_tx.clone();
        let thread = RecordId::new("agent_thread", key);
        let tid = key.to_string();
        PlatformSpawner::spawn(async move {
            for (text, images) in held {
                if let Err(e) = AgentTurn::ask_with(&thread, "queue", &text, &images).await {
                    let _ = tx.try_send(ChatMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        thread_id: tid.clone(),
                        ts: crate::tabs::ai_playground::now_ts(),
                        from: SentFrom::Assistant,
                        content: ChatMessageType::Error(format!("could not queue the message: {e}")),
                    });
                }
            }
        });
    }

    /// Stops waiting on `local`; false when nothing was waiting on it.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn stop_waiting(&mut self, local: &str) -> bool {
        let locked = self.following.remove(local);
        let lingering = self.lingering.remove(local);
        locked || lingering
    }

    /// Appends an assistant line to a chat that still exists.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn notify(&mut self, tid: &str, content: ChatMessageType) {
        if let Some(thread) = self.threads.get_mut(tid) {
            thread.messages.push(ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                thread_id: tid.to_string(),
                ts: crate::tabs::ai_playground::now_ts(),
                from: SentFrom::Assistant,
                content,
            });
        }
    }

    /// Moves a local chat's messages, title, engine, attachments and echo flag onto `key`.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn rekey(&mut self, local: &str, key: &str) {
        if let Some(mut moved) = self.threads.remove(local) {
            moved.id = key.to_string();
            match self.threads.get_mut(key) {
                Some(existing) => existing.messages.extend(moved.messages),
                None => {
                    self.threads.insert(key.to_string(), moved);
                }
            }
        }
        if let Some(engine) = self.thread_engine.remove(local) {
            self.thread_engine.insert(key.to_string(), engine);
        }
        if let Some(title) = self.chat_title.remove(local) {
            self.chat_title.entry(key.to_string()).or_insert(title);
        }
        if let Some(composer) = self.composers.remove(local) {
            self.composers.insert(key.to_string(), composer);
        }
        if self.hydrated.remove(local) {
            self.hydrated.insert(key.to_string());
        }
        self.agent_threads.remove(local);
        self.rekeyed.insert(local.to_string(), key.to_string());
    }

    /// Re-keys a local thread onto the agent session it became, keeping what was typed.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn adopt_agent_thread(&mut self, local: &str, key: String) {
        if local == key {
            return;
        }
        self.rekey(local, &key);
        self.agent_threads.insert(key.clone());
        self.hydrated.insert(key.clone());
        if self.selected_thread == local {
            self.select_thread(key);
        }
        self.last_agent_poll = None;
    }

    /// Moves a placeholder chat onto the session its request opened, backfilling both sides.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn apply_opened(&mut self, local: &str, key: String) {
        let selected = self.selected_thread == local;
        self.rekey(local, &key);
        self.threads.entry(key.clone()).or_insert_with(|| ChatThread {
            id: key.clone(),
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        });
        self.agent_threads.insert(key.clone());
        if selected {
            self.select_thread(key);
            self.last_agent_poll = None;
        }
    }

    /// Sends one message as a `start`, `queue` or `steer` turn, files the request that opens its session, or holds it while its chat waits for one.
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

        if self.lingering.contains(&thread_id) {
            self.held.entry(thread_id).or_default().push((text, images));
            return;
        }
        self.last_state_poll = None;
        let session = self.agent_threads.contains(&thread_id)
            || self
                .agent_index
                .iter()
                .chain(&self.open_row)
                .any(|t| t.id.key_string() == thread_id);
        let fresh = self.fresh_sessions && !session;
        if fresh {
            self.following.insert(thread_id.clone());
        }
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
        let follow_tx = self.follow_tx.clone();
        let tid = thread_id.clone();
        PlatformSpawner::spawn(async move {
            let say = |content: ChatMessageType| ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                thread_id: tid.clone(),
                ts: crate::tabs::ai_playground::now_ts(),
                from: SentFrom::Assistant,
                content,
            };
            // Reports a failure; a chat waiting on a fresh request also stops waiting.
            let fail = |message: String| {
                if fresh {
                    let _ = follow_tx.send((tid.clone(), FollowEvent::Failed(message)));
                } else {
                    let _ = tx.try_send(say(ChatMessageType::Error(message)));
                }
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
                fail("Sign in to chat with the agent.".into());
                return;
            };
            let general = database::schema::is_general(&cs);
            if !general {
                if let Some(block) = database::schema::ConnectedClient::diagnosis_block(&cs).await {
                    fail(format!("Not dispatched — {cs}: {block}."));
                    return;
                }
            }
            // A fresh chat always files a request; otherwise a live session takes the message as a turn.
            let live = if fresh { Ok(None) } else { AgentThread::active_for_connection(&cs).await };
            match live {
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
                        fresh,
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
                            if fresh {
                                match await_session(&request, &tid, &follow_tx).await {
                                    Ok(thread) => {
                                        if !whole
                                            && let Err(e) =
                                                AgentTurn::ask_with(&thread, "queue", &text, &images).await
                                        {
                                            let _ = tx.try_send(say(ChatMessageType::Error(format!(
                                                "could not queue the message: {e}"
                                            ))));
                                        }
                                        let opened = FollowEvent::Opened { key: thread.key_string(), echoed: true };
                                        let _ = follow_tx.send((tid.clone(), opened));
                                    }
                                    Err(message) => fail(message),
                                }
                                return;
                            }
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
                        Err(e) => fail(format!("could not request a diagnosis: {e}")),
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

    /// The open thread, the message ids it already shows, and whether its user rows are read.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    fn reply_poll_plan(&self) -> (String, std::collections::HashSet<String>, bool) {
        let thread = self.selected_thread.clone();
        let seen = self
            .threads
            .get(&thread)
            .map(|t| t.messages.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default();
        // Threads with a local echo skip the tech's database rows.
        let hydrate = !self.hydrated.contains(&thread);
        (thread, seen, hydrate)
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

        let (thread, seen, hydrate) = self.reply_poll_plan();
        let tx = self.response_tx.clone();
        let flag_tx = self.agent_flag_tx.clone();
        PlatformSpawner::spawn(async move {
            use database::schema::{AgentEvent, RecordId};
            let rows = AgentEvent::recent(&RecordId::new("agent_thread", thread.as_str()), 300)
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
        let thread_id = self.echo_message(&text, staged);

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

    /// Echoes a message and its pictures into the open thread, which then skips the tech's database rows.
    fn echo_message(&mut self, text: &str, staged: Vec<String>) -> String {
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
            .try_send(echo(ChatMessageType::Text(text.to_string())));
        for name in staged {
            let _ = self
                .response_tx
                .try_send(echo(ChatMessageType::Image((name, bytes::Bytes::new()))));
        }
        self.hydrated.insert(thread_id.clone());
        thread_id
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

    fn note(thread: &str, text: &str) -> ChatMessage {
        ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread.into(),
            ts: 1_790_000_000,
            from: SentFrom::Assistant,
            content: ChatMessageType::Text(text.into()),
        }
    }

    fn legacy(id: &str) -> LoadedThread {
        LoadedThread { id: id.into(), title: "Old chat".into(), messages: vec![note(id, "old reply")] }
    }

    #[test]
    fn start_new_session_reuses_an_empty_local_chat() {
        let mut chat = EnhancedAiPlayground::default();
        chat.start_new_session();
        let first = chat.selected_thread.clone();
        chat.start_new_session();
        assert_eq!(chat.selected_thread, first);
        assert_eq!(chat.threads.len(), 1);

        chat.threads.get_mut(&first).expect("chat").messages.push(note(&first, "hi"));
        chat.start_new_session();
        let second = chat.selected_thread.clone();
        assert_ne!(second, first);

        chat.agent_threads.insert(second.clone());
        chat.start_new_session();
        assert_ne!(chat.selected_thread, second);
        assert_eq!(chat.threads.len(), 3);
    }

    #[test]
    fn merge_loaded_keeps_a_fresh_session_selected() {
        let mut chat = EnhancedAiPlayground::default();
        chat.start_new_session();
        let fresh = chat.selected_thread.clone();
        chat.merge_loaded(vec![legacy("legacy-1")]);
        assert_eq!(chat.selected_thread, fresh);
        assert!(chat.threads.contains_key("legacy-1"));
    }

    #[test]
    fn merge_loaded_selects_the_newest_chat_when_nothing_is_selected() {
        let mut chat = EnhancedAiPlayground::default();
        chat.merge_loaded(vec![legacy("newest"), legacy("older")]);
        assert_eq!(chat.selected_thread, "newest");
    }

    #[test]
    fn a_blank_session_is_not_reused_while_it_waits_for_a_request() {
        let mut chat = EnhancedAiPlayground::default();
        chat.start_new_session();
        let blank = chat.selected_thread.clone();
        chat.following.insert(blank.clone());
        assert!(!chat.composer_enabled(&blank));
        chat.start_new_session();
        assert_ne!(chat.selected_thread, blank);
    }

    #[test]
    fn start_new_session_selects_a_blank_chat_left_behind() {
        let mut chat = EnhancedAiPlayground::default();
        chat.start_new_session();
        let blank = chat.selected_thread.clone();
        chat.threads.insert(
            "old".into(),
            ChatThread { id: "old".into(), messages: vec![note("old", "reply")], images: Vec::new(), input: String::new() },
        );
        chat.select_thread("old".into());

        chat.start_new_session();
        assert_eq!(chat.selected_thread, blank);
        assert_eq!(chat.threads.len(), 2);
    }

    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    mod follow {
        use super::*;

        const LABEL: &str = "#2155467 PC-1";
        const KEY: &str = "k7x2";

        fn followed() -> (EnhancedAiPlayground, String) {
            let mut chat = EnhancedAiPlayground::default();
            let local = chat.follow_assist_request(LABEL.into()).local;
            (chat, local)
        }

        fn opened(key: &str) -> FollowEvent {
            FollowEvent::Opened { key: key.into(), echoed: false }
        }

        #[test]
        fn a_followed_request_selects_a_locked_placeholder() {
            let (chat, local) = followed();
            assert_eq!(chat.selected_thread, local);
            assert_eq!(chat.chat_title.get(&local).map(String::as_str), Some(LABEL));
            assert!(chat.following.contains(&local));
            assert!(!chat.composer_enabled(&local));
            assert_eq!(chat.threads[&local].messages.len(), 1);
            assert!(chat.response_rx.try_recv().is_err());
        }

        #[test]
        fn a_followed_request_replaces_a_blank_chat_instead_of_stacking() {
            let mut chat = EnhancedAiPlayground::default();
            chat.start_new_session();
            let blank = chat.selected_thread.clone();
            let local = chat.follow_assist_request(LABEL.into()).local;
            assert!(!chat.threads.contains_key(&blank));
            assert_eq!(chat.threads.len(), 1);
            assert_eq!(chat.selected_thread, local);
        }

        #[test]
        fn opened_request_replaces_its_placeholder() {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, opened(KEY));
            assert_eq!(chat.selected_thread, KEY);
            assert!(chat.agent_threads.contains(KEY));
            assert!(!chat.hydrated.contains(KEY));
            assert!(!chat.threads.contains_key(&local));
            assert_eq!(chat.threads[KEY].messages.len(), 1, "the notice moves with the chat");
            assert_eq!(chat.chat_title.get(KEY).map(String::as_str), Some(LABEL));
            assert!(chat.following.is_empty());
            assert!(chat.composer_enabled(KEY));
        }

        #[test]
        fn opened_request_leaves_a_moved_selection_alone() {
            let (mut chat, local) = followed();
            chat.composers.entry(local.clone()).or_default();
            chat.threads.insert(
                KEY.into(),
                ChatThread { id: KEY.into(), messages: vec![note(KEY, "earlier")], images: Vec::new(), input: String::new() },
            );
            chat.start_new_session();
            let moved_to = chat.selected_thread.clone();
            assert_ne!(moved_to, local);

            chat.apply_follow(&local, opened(KEY));
            assert_eq!(chat.selected_thread, moved_to);
            assert!(chat.agent_threads.contains(KEY));
            assert_eq!(chat.threads[KEY].messages.len(), 2, "the earlier transcript is kept");
            assert!(chat.composers.contains_key(KEY));
            assert!(!chat.composers.contains_key(&local));
        }

        #[test]
        fn failed_request_stops_following() {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, FollowEvent::Failed("admin-agent is down".into()));
            assert!(chat.following.is_empty());
            assert_eq!(chat.selected_thread, local);
            assert!(chat.composer_enabled(&local));
            let last = chat.threads[&local].messages.last().expect("error line");
            assert_eq!(last.content, ChatMessageType::Error("admin-agent is down".into()));
        }

        #[test]
        fn pending_session_failure_reaches_its_placeholder() {
            let mut chat = EnhancedAiPlayground::default();
            let pending = chat.follow_assist_request(LABEL.into());
            let local = pending.local.clone();
            pending.fail("could not file the request".into());
            let (to, event) = chat.follow_rx.try_recv().expect("event");
            assert_eq!(to, local);
            chat.apply_follow(&to, event);
            assert!(chat.composer_enabled(&local));
        }

        #[test]
        fn a_slow_request_unlocks_the_composer_and_still_opens() {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, FollowEvent::Slow);
            assert!(chat.composer_enabled(&local));
            assert!(chat.lingering.contains(&local));
            assert_eq!(chat.threads[&local].messages.len(), 2);
            chat.start_new_session();
            assert_ne!(chat.selected_thread, local, "a waiting chat is never reused");

            chat.apply_follow(&local, opened(KEY));
            assert!(chat.lingering.is_empty());
            assert!(chat.threads.contains_key(KEY));
        }

        #[test]
        fn an_event_for_a_chat_that_stopped_waiting_is_ignored() {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, FollowEvent::Failed("gone".into()));
            chat.apply_follow(&local, opened(KEY));
            assert!(chat.threads.contains_key(&local));
            assert!(!chat.threads.contains_key(KEY));
        }

        #[test]
        fn an_echoed_chat_is_adopted_with_its_own_rows_skipped() {
            let mut chat = EnhancedAiPlayground::default();
            chat.start_new_session();
            let local = chat.selected_thread.clone();
            chat.following.insert(local.clone());
            chat.apply_follow(&local, FollowEvent::Opened { key: KEY.into(), echoed: true });
            assert_eq!(chat.selected_thread, KEY);
            assert!(chat.hydrated.contains(KEY));
        }

        #[test]
        fn a_late_message_for_a_rekeyed_chat_reaches_its_session() {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, opened(KEY));
            chat.apply_response(ChatMessage {
                content: ChatMessageType::Error("late".into()),
                ..note(&local, "")
            });
            assert!(!chat.threads.contains_key(&local), "no phantom chat");
            assert_eq!(chat.threads[KEY].messages.len(), 2);
        }

        #[test]
        fn an_opened_session_keeps_reading_user_rows_until_the_tech_types() {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, opened(KEY));
            for _ in 0..2 {
                let (thread, seen, hydrate) = chat.reply_poll_plan();
                assert_eq!(thread, KEY);
                assert!(!seen.is_empty());
                assert!(hydrate, "an empty poll must not stop the opener from rendering");
            }
            chat.echo_message("what did you find?", Vec::new());
            assert!(!chat.reply_poll_plan().2);
        }

        #[test]
        fn a_followed_request_clears_every_blank_chat() {
            let mut chat = EnhancedAiPlayground::default();
            chat.start_new_session();
            chat.threads.insert(
                "old".into(),
                ChatThread { id: "old".into(), messages: vec![note("old", "reply")], images: Vec::new(), input: String::new() },
            );
            chat.create_new_chat_thread();
            assert_eq!(chat.threads.len(), 3);

            let local = chat.follow_assist_request(LABEL.into()).local;
            let mut kept: Vec<&str> = chat.threads.keys().map(String::as_str).collect();
            kept.sort();
            let mut want = vec!["old", local.as_str()];
            want.sort();
            assert_eq!(kept, want);
        }

        /// A followed chat whose composer lock ran out, with one message sent from it.
        fn lingering_with_a_message() -> (EnhancedAiPlayground, String) {
            let (mut chat, local) = followed();
            chat.apply_follow(&local, FollowEvent::Slow);
            chat.send_chat_message("start", "is the fan spinning?".into(), Vec::new(), Vec::new());
            (chat, local)
        }

        #[test]
        fn a_message_from_a_lingering_chat_is_held_for_its_own_session() {
            let (chat, local) = lingering_with_a_message();
            assert_eq!(chat.held[&local].len(), 1);
            assert_eq!(chat.held[&local][0].0, "is the fan spinning?");
            assert!(chat.lingering.contains(&local), "the chat still waits on its request");
            assert!(!chat.following.contains(&local));
            assert!(!chat.agent_threads.contains(&local));
        }

        #[cfg(feature = "tokio")]
        #[tokio::test]
        async fn held_messages_leave_when_the_session_opens_and_its_echo_is_kept() {
            let (mut chat, local) = lingering_with_a_message();
            chat.apply_follow(&local, opened(KEY));
            assert!(chat.held.is_empty());
            assert_eq!(chat.selected_thread, KEY);
            assert!(chat.agent_threads.contains(KEY));
            assert!(chat.hydrated.contains(KEY), "the local echo would be duplicated by its row");
            assert!(!chat.hydrated.contains(&local));
        }

        #[test]
        fn a_failed_request_reports_its_held_messages_as_unsent() {
            let (mut chat, local) = lingering_with_a_message();
            chat.apply_follow(&local, FollowEvent::Failed("admin-agent is down".into()));
            assert!(chat.held.is_empty());
            assert!(chat.lingering.is_empty());
            let tail: Vec<&ChatMessageType> =
                chat.threads[&local].messages.iter().rev().take(2).map(|m| &m.content).collect();
            assert_eq!(
                tail,
                vec![
                    &ChatMessageType::Error("The message typed while waiting was not sent.".into()),
                    &ChatMessageType::Error("admin-agent is down".into()),
                ]
            );
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
