use eframe::egui::{
    text::LayoutJob, Align, Button, CentralPanel, CollapsingHeader, Color32, FontId, Frame, Key,
    KeyboardShortcut, Layout, Margin, Modifiers, Popup, PopupCloseBehavior, RichText, ScrollArea,
    TextEdit, TextFormat, Ui,
};
use crate::{
    tabs::ai_playground::{ChatMessage, ChatMessageType, ChatThread, SentFrom},
    ui_tools::icons,
    PlatformSpawner, Spawner,
};

use std::collections::HashMap;
use crossbeam::channel::{Receiver, Sender};
use database::schema::RecordIdExt;
use serde::Serialize;

/// A chat thread loaded from the database, delivered to the UI thread.
struct LoadedThread {
    id: String,
    title: String,
    messages: Vec<ChatMessage>,
}

/// Streaming chat against the user's OpenAI-compatible MCP endpoint, with
/// collapsible reasoning ("thinking") and an optional Mastertech tool-calling loop.
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
    /// Expose the Mastertech MCP tools to the model (native only).
    pub use_mcp_tools: bool,
    /// Connection string of the connected client the admin console is focused on; seeds Claude Code diagnostics.
    #[serde(skip)]
    pub focused_client: Option<String>,
    /// When true, hides the close ✕ and the external Claude Code button and uses self-diagnosis empty-state copy.
    #[serde(skip)]
    pub self_diagnosis: bool,
    /// Multi-turn Claude Code session (subscription auth, :9004 MCP).
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    pub claude: crate::ai::claude_code::ClaudeCodeSession,
    /// Threads whose turns run on the Codex agent broker. Input in one of
    /// these goes to the agent, never to the OpenAI-compatible endpoint.
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
    /// Service number the conversation is about, when the host knows one; joins
    /// the transcript to a service order.
    #[serde(skip)]
    pub service_number: Option<String>,
    /// Thread the Claude Code session is bound to; input in that thread resumes it.
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    claude_thread: Option<String>,
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
}

impl Default for EnhancedAiPlayground {
    fn default() -> Self {
        let (response_tx, response_rx) = crossbeam::channel::unbounded::<ChatMessage>();
        let (load_tx, load_rx) = crossbeam::channel::unbounded::<Vec<LoadedThread>>();
        let (agent_flag_tx, agent_flag_rx) = crossbeam::channel::unbounded::<String>();
        let (agent_index_tx, agent_index_rx) =
            crossbeam::channel::unbounded::<Vec<database::schema::AgentThread>>();
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
            use_mcp_tools: true,
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
            service_number: None,
            #[cfg(not(target_arch = "wasm32"))]
            claude: crate::ai::claude_code::ClaudeCodeSession::new(),
            #[cfg(not(target_arch = "wasm32"))]
            claude_thread: None,
            thread_engine: HashMap::new(),
            close_requested: false,
            loaded: false,
            load_tx,
            load_rx,
        }
    }
}

impl EnhancedAiPlayground {
    /// Returns and clears the "close panel" request raised by the top-bar ✕.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Start a Claude Code (subscription) session in a fresh thread, seeded with the focused
    /// connected client. Later input in that thread resumes the same session.
    pub fn start_claude_diagnosis(&mut self, connection_string: Option<String>) {
        let thread_id = uuid::Uuid::new_v4().to_string();
        self.selected_thread = thread_id.clone();
        self.threads.insert(
            thread_id.clone(),
            ChatThread { id: thread_id.clone(), messages: Vec::new(), images: Vec::new(), input: String::new() },
        );
        let label = match &connection_string {
            Some(cs) => format!("Diagnose {cs} with Claude Code"),
            None => "Diagnose with Claude Code".to_string(),
        };
        let _ = self.response_tx.try_send(ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread_id.clone(),
            ts: crate::tabs::ai_playground::now_ts(),
            from: SentFrom::Me,
            content: ChatMessageType::Text(label),
        });
        #[cfg(not(target_arch = "wasm32"))]
        {
            let prompt = match &connection_string {
                Some(_) => "Diagnose that client. Pull its prior history and run an initial triage \
                     using the Mastertech tools."
                    .to_string(),
                None => "Run an initial diagnostic of this machine using the Mastertech tools.".to_string(),
            };
            // Broker route: a named machine goes to the Codex agent, whose session
            // every Mastertech instance can follow; the local host stays on Claude Code.
            #[cfg(feature = "tokio")]
            {
                if connection_string.is_some() {
                    self.thread_engine
                        .insert(thread_id.clone(), "Codex agent".to_string());
                    self.send_to_agent(thread_id.clone(), prompt, connection_string);
                    return;
                }
            }
            let model = std::env::var("CC_MODEL").unwrap_or_else(|_| "default model".into());
            self.thread_engine
                .insert(thread_id.clone(), format!("Claude Code (local) \u{00B7} {model}"));
            self.claude.reset();
            self.claude_thread = Some(thread_id.clone());
            // Same opt-out gate as the agent route. The session is Arc-backed, so
            // the turn still runs against the one the tab holds.
            let session = self.claude.clone();
            let tx = self.response_tx.clone();
            PlatformSpawner::spawn(async move {
                if let Some(cs) = connection_string.as_deref() {
                    if let Some(block) =
                        database::schema::ConnectedClient::diagnosis_block(cs).await
                    {
                        let _ = tx.try_send(ChatMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            thread_id: thread_id.clone(),
                            ts: crate::tabs::ai_playground::now_ts(),
                            from: SentFrom::Assistant,
                            content: ChatMessageType::Error(format!(
                                "Not dispatched — {cs}: {block}."
                            )),
                        });
                        return;
                    }
                }
                session.send(prompt, connection_string, thread_id, tx);
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = connection_string;
        }
    }

    pub fn enhanced_ai_playground(&mut self, ui: &mut Ui) {
        self.ensure_loaded();

        // Keep frames coming while Claude streams so the drain below runs without input.
        #[cfg(not(target_arch = "wasm32"))]
        if self.claude.is_busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }

        eframe::egui::Panel::top("enhanced_ai_topbar")
            .frame(Frame::default().inner_margin(Margin::symmetric(6, 2)))
            .exact_size(28.)
            .show_separator_line(false)
            .show(ui, |ui| self.show_chat_topbar(ui));

        eframe::egui::Panel::bottom("enhanced_ai_input")
            .frame(Frame::default().inner_margin(Margin::same(6)))
            .exact_size(92.)
            .show(ui, |ui| self.show_chat_input(ui));

        CentralPanel::default()
            .frame(Frame::central_panel(ui.style()).inner_margin(Margin::same(10)))
            .show(ui, |ui| self.show_chat_content(ui));

        self.handle_enhanced_ai_events(ui);
    }

    fn thread_title(&self, id: &str) -> String {
        self.chat_title.get(id).cloned().unwrap_or_else(|| {
            self.threads
                .get(id)
                .and_then(|t| t.messages.iter().find_map(|m| match &m.content {
                    ChatMessageType::Text(s) if matches!(m.from, SentFrom::Me) => Some(short_title(s)),
                    _ => None,
                }))
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

    /// Compact top bar: hover-open threads dropdown + New chat on the left;
    /// model, tools toggle and close on the right.
    fn show_chat_topbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // ── Threads dropdown (opens on hover, stays open over the popup) ──
            let label = format!("{}  {}  {}", icons::CHAT, self.current_thread_title(), icons::CHEV_OPEN);
            let resp = ui.button(RichText::new(label));
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
            let popup = Popup::from_response(&resp)
                .open(open)
                .gap(2.0)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_min_width(220.);
                    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                    let agent_index = self.agent_index.clone();
                    #[cfg(not(all(not(target_arch = "wasm32"), feature = "tokio")))]
                    let agent_index: Vec<database::schema::AgentThread> = Vec::new();
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
                            if ui
                                .selectable_label(selected == id, RichText::new(format!("{}  {title}", icons::CHAT)))
                                .clicked()
                            {
                                picked = Some(id);
                            }
                        }
                        if agent_index.is_empty() {
                            return;
                        }
                        ui.separator();
                        ui.label(RichText::new("Agent sessions").weak().small());
                        for t in &agent_index {
                            // A session mid-turn or waiting on a decision is the one a tech is watching.
                            let mark = match t.status.as_str() {
                                "running" => icons::STATUS_ON,
                                "waiting_approval" => icons::LOCK,
                                _ => icons::ROBOT,
                            };
                            let who = t.requested_by.as_deref().unwrap_or("unattributed");
                            let key = t.id.key_string();
                            let line = format!("{mark}  {}  ({})", t.label(), t.status);
                            if ui
                                .selectable_label(selected == key, RichText::new(line))
                                .on_hover_text(format!("{who}\n{}", t.connection_string))
                                .clicked()
                            {
                                picked = Some(key);
                            }
                        }
                    });
                });
            let stored = popup.map(|r| r.response.rect).unwrap_or(eframe::egui::Rect::NOTHING);
            ui.memory_mut(|m| m.data.insert_temp(rect_id, stored));
            if let Some(id) = picked {
                #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                if !self.threads.contains_key(&id) {
                    // Only an agent conversation can be picked without local
                    // state; opening it backfills the transcript.
                    self.open_agent_thread(id.clone());
                }
                self.selected_thread = id;
            }

            if ui.button(RichText::new(icons::PLUS)).on_hover_text("New chat").clicked() {
                self.create_new_chat_thread();
            }

            // ── Right side: close · tools · model ──
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.self_diagnosis
                    && ui.button(RichText::new(icons::CLOSE)).on_hover_text("Close chat").clicked()
                {
                    self.close_requested = true;
                }
                #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
                if ui
                    .selectable_label(self.use_mcp_tools, RichText::new(icons::WRENCH))
                    .on_hover_text("Use Mastertech tools")
                    .clicked()
                {
                    self.use_mcp_tools = !self.use_mcp_tools;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if self.claude.is_busy() {
                        if ui
                            .button(RichText::new(icons::STOP).color(ui.visuals().error_fg_color))
                            .on_hover_text("Stop Claude Code")
                            .clicked()
                        {
                            self.claude.cancel();
                        }
                    } else if ui
                        .button(RichText::new(icons::ROBOT))
                        .on_hover_text("Diagnose with Claude Code (subscription)")
                        .clicked()
                    {
                        let cs = self.focused_client.clone();
                        self.start_claude_diagnosis(cs);
                    }
                }
                let engine = self.thread_engine.get(&self.selected_thread).cloned().unwrap_or_else(|| {
                    format!("OpenRouter \u{00B7} {}", crate::ai::effective_model(crate::ai::gpts::MODEL))
                });
                ui.label(RichText::new(engine).weak().small())
                    .on_hover_text("Which engine answers this thread. Claude Code runs locally; ZeroClaw runs on the agent host.");
            });
        });
    }

    fn show_chat_input(&mut self, ui: &mut Ui) {
        let mut send = false;
        if self.threads.contains_key(&self.selected_thread) {
            if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
                let row_h = ui.available_height();
                let send_w = 38.0;
                ui.horizontal(|ui| {
                    let resp = ui.add_sized(
                        [ui.available_width() - send_w - 6.0, row_h],
                        TextEdit::multiline(&mut thread.input)
                            .hint_text("Ask anything…  (Shift+Enter for newline)")
                            .return_key(Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let clicked = ui
                        .add_sized([send_w, row_h], Button::new(RichText::new(icons::UP).strong()))
                        .on_hover_text("Send")
                        .clicked();
                    if (clicked || enter) && !thread.input.trim().is_empty() {
                        send = true;
                    }
                });
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(format!("Start a new chat with  {}  above.", icons::PLUS)).weak());
            });
        }

        if send {
            self.send_chat_message();
        }
    }

    fn show_chat_content(&mut self, ui: &mut Ui) {
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
                        RichText::new("Ask about the PC Mastertech is running on — it inspects this machine with the Mastertech tools.")
                            .weak(),
                    );
                } else {
                    ui.heading(RichText::new("Mastertech Assistant").strong());
                    ui.label(RichText::new("Ask a question to get started.").weak());
                }
            });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                // Consecutive tool lines collapse into one block instead of one card each.
                let mut i = 0;
                while i < messages.len() {
                    if Self::is_tool_line(&messages[i]) {
                        let start = i;
                        while i < messages.len() && Self::is_tool_line(&messages[i]) {
                            i += 1;
                        }
                        self.render_tool_group(ui, &messages[start..i]);
                    } else {
                        self.render_chat_message(ui, &messages[i]);
                        i += 1;
                    }
                    ui.add_space(6.);
                }
            });
    }

    /// True for assistant tool-activity lines emitted with `TOOL_PREFIX`.
    fn is_tool_line(message: &ChatMessage) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let ChatMessageType::Text(t) = &message.content {
            return matches!(message.from, SentFrom::Assistant)
                && t.starts_with(crate::ai::claude_code::TOOL_PREFIX);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = message;
        false
    }

    fn render_tool_group(&self, ui: &mut Ui, group: &[ChatMessage]) {
        let salt = group.first().map(|m| m.id.as_str()).unwrap_or("tools");
        let plural = if group.len() == 1 { "" } else { "s" };
        let title = format!("{}  {} tool call{plural}", icons::WRENCH, group.len());
        Frame::group(ui.style()).fill(ui.visuals().extreme_bg_color).show(ui, |ui| {
            ui.set_width(ui.available_width());
            CollapsingHeader::new(RichText::new(title).small().weak())
                .id_salt(format!("tool-group-{salt}"))
                .default_open(true)
                .show(ui, |ui| {
                    for m in group {
                        if let ChatMessageType::Text(t) = &m.content {
                            self.render_tool_line(ui, &m.id, t);
                        }
                    }
                });
        });
    }

    /// Splits `» name ({json}) status` plus an optional result after the first
    /// newline, rendering both JSON payloads as collapsible trees.
    fn render_tool_line(&self, ui: &mut Ui, id: &str, text: &str) {
        let (text, result) = match text.split_once('\n') {
            Some((head, tail)) => (head, tail.trim()),
            None => (text, ""),
        };
        #[cfg(not(target_arch = "wasm32"))]
        let body = text.trim_start_matches(crate::ai::claude_code::TOOL_PREFIX).trim_start();
        #[cfg(target_arch = "wasm32")]
        let body = text.trim_start();

        let (name, rest) = match body.find(" (") {
            Some(i) => (&body[..i], &body[i + 1..]),
            None => (body, ""),
        };
        let (args, status) = match rest.rfind(')') {
            Some(i) => (rest[1..i].trim(), rest[i + 1..].trim()),
            None => ("", rest.trim()),
        };

        let mut header = LayoutJob::default();
        header.append(
            name,
            0.0,
            TextFormat {
                font_id: FontId::monospace(11.0),
                color: ui.visuals().strong_text_color(),
                ..Default::default()
            },
        );
        if !status.is_empty() {
            header.append(
                &format!("  {status}"),
                0.0,
                TextFormat {
                    font_id: FontId::proportional(11.0),
                    color: Color32::LIGHT_GREEN,
                    ..Default::default()
                },
            );
        }
        // Nothing to reveal when the call carried neither payload.
        if args.is_empty() && result.is_empty() {
            ui.label(header);
            return;
        }
        CollapsingHeader::new(header)
            .id_salt(format!("tool-call-{id}"))
            .default_open(false)
            .show(ui, |ui| {
                if !args.is_empty() {
                    Self::render_payload(ui, &format!("tool-args-{id}"), "arguments", args);
                }
                if !result.is_empty() {
                    Self::render_payload(ui, &format!("tool-res-{id}"), "result", result);
                }
            });
    }

    /// JSON payloads get a tree; anything else falls back to monospace text.
    fn render_payload(ui: &mut Ui, salt: &str, label: &str, raw: &str) {
        CollapsingHeader::new(RichText::new(label).small().weak())
            .id_salt(salt)
            .default_open(false)
            .show(ui, |ui| match serde_json::from_str::<serde_json::Value>(raw) {
                Ok(value) => crate::ui_tools::hex_json::json_tree(ui, salt, &value),
                Err(_) => {
                    ui.add(
                        eframe::egui::Label::new(RichText::new(raw).monospace().small())
                            .wrap_mode(eframe::egui::TextWrapMode::Extend),
                    );
                }
            });
    }

    fn render_chat_message(&self, ui: &mut Ui, message: &ChatMessage) {
        match &message.content {
            ChatMessageType::Reasoning(reasoning) => {
                if reasoning.trim().is_empty() {
                    return;
                }
                CollapsingHeader::new(RichText::new(format!("{}  Thinking", icons::LIGHTBULB)).weak())
                    .id_salt(format!("think-{}", message.id))
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.style_mut().visuals.override_text_color = Some(ui.visuals().weak_text_color());
                        crate::markdown_editor::chat_markdown::render(ui, reasoning);
                    });
            }
            ChatMessageType::Text(text)
            | ChatMessageType::Code(text)
            | ChatMessageType::Error(text)
            | ChatMessageType::FileId(text) => {
                let is_user = matches!(message.from, SentFrom::Me);
                let (glyph, name) = if is_user {
                    (icons::p::USER, "You")
                } else {
                    (icons::ROBOT, "Assistant")
                };
                let fill = if is_user {
                    ui.visuals().widgets.active.weak_bg_fill
                } else {
                    ui.visuals().faint_bg_color
                };
                Frame::group(ui.style()).fill(fill).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let color = if is_user { Color32::LIGHT_BLUE } else { Color32::LIGHT_GREEN };
                        ui.label(RichText::new(format!("{glyph}  {name}")).strong().color(color).small());
                    });
                    if matches!(message.content, ChatMessageType::Error(_)) {
                        ui.colored_label(ui.visuals().error_fg_color, text);
                    } else {
                        crate::markdown_editor::chat_markdown::render(ui, text);
                    }
                });
            }
            ChatMessageType::Image(_) | ChatMessageType::Done => {}
        }
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

        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        while let Ok(thread) = self.agent_flag_rx.try_recv() {
            self.agent_threads.insert(thread);
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        {
            self.poll_agent_index(ui);
            self.poll_agent_replies(ui);
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

    /// Queues one technician message for the agent: a turn on an open session, or a
    /// request that opens one for the target machine.
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    fn send_to_agent(
        &mut self,
        thread_id: String,
        text: String,
        connection_string: Option<String>,
    ) {
        use database::schema::{AgentThread, AgentTurn, AssistRequest, RecordId};

        self.agent_threads.insert(thread_id.clone());
        let session = self.agent_index.iter().any(|t| t.id.key_string() == thread_id);
        let target = connection_string.or_else(|| self.focused_client.clone());
        let user = crate::get_current_user_from_auth();
        let tech = user.as_ref().map(|u| u.get_email().to_string());
        let store = user
            .as_ref()
            .and_then(|u| serde_json::to_value(u).ok())
            .and_then(|v| v.get("store").and_then(serde_json::Value::as_str).map(str::to_string));
        let service_number = self.service_number.clone();
        let tx = self.response_tx.clone();
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
                if let Err(e) = AgentTurn::ask(&RecordId::new("agent_thread", tid.as_str()), "start", &text).await {
                    let _ = tx.try_send(say(ChatMessageType::Error(format!("could not queue the message: {e}"))));
                }
                return;
            }
            let Some(cs) = target else {
                let _ = tx.try_send(say(ChatMessageType::Error(
                    "Agent diagnosis needs a target machine: focus a client in the admin console first.".into(),
                )));
                return;
            };
            if let Some(block) = database::schema::ConnectedClient::diagnosis_block(&cs).await {
                let _ = tx.try_send(say(ChatMessageType::Error(format!("Not dispatched — {cs}: {block}."))));
                return;
            }
            // A machine with a live session takes the message as a turn; otherwise a request opens one.
            match AgentThread::active_for_connection(&cs).await {
                Ok(Some(thread)) => match AgentTurn::ask(&thread.id, "start", &text).await {
                    Ok(_) => {
                        let _ = tx.try_send(say(ChatMessageType::Text(format!(
                            "Sent to the live agent session for {cs}; open it under Agent sessions to follow along."
                        ))));
                    }
                    Err(e) => {
                        let _ = tx.try_send(say(ChatMessageType::Error(format!("could not queue the message: {e}"))));
                    }
                },
                _ => match AssistRequest::create_from_chat(&cs, tech.as_deref(), store.as_deref(), service_number.as_deref(), &text).await {
                    Ok(_) => {
                        let _ = tx.try_send(say(ChatMessageType::Text(format!(
                            "Diagnosis requested for {cs}. The agent session appears under Agent sessions within a minute."
                        ))));
                    }
                    Err(e) => {
                        let _ = tx.try_send(say(ChatMessageType::Error(format!("could not request a diagnosis: {e}"))));
                    }
                },
            }
        });
    }

    /// Refreshes the list of agent conversations this user may open. A
    /// technician sees only their own; root sees every one, which is the only
    /// way to answer a conversation the tech who opened it has gone home on.
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
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
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    fn open_agent_thread(&mut self, thread: String) {
        self.agent_threads.insert(thread.clone());
        self.threads.entry(thread.clone()).or_insert_with(|| ChatThread {
            id: thread.clone(),
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        });
        self.selected_thread = thread;
        // Force the next reply poll rather than waiting out the interval.
        self.last_agent_poll = None;
    }

    /// Pulls agent replies for the open thread. Messages carry their row id, so
    /// the thread's own contents are the dedupe set and no extra state is kept.
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
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
                            "{}{}",
                            crate::ai::claude_code::TOOL_PREFIX,
                            row.text.lines().next().unwrap_or_default()
                        )),
                    ),
                    "approval" => (SentFrom::Assistant, ChatMessageType::Text(format!("{} {}", icons::LOCK, row.text))),
                    "user" if hydrate => (SentFrom::Me, ChatMessageType::Text(row.text.clone())),
                    _ => continue,
                };
                let _ = tx.try_send(ChatMessage {
                    id,
                    thread_id: thread.clone(),
                    ts: crate::tabs::ai_playground::now_ts(),
                    from,
                    content,
                });
            }
        });
    }

    fn create_new_chat_thread(&mut self) {
        let thread_id = uuid::Uuid::new_v4().to_string();
        self.selected_thread = thread_id.clone();
        self.threads.insert(thread_id.clone(), ChatThread {
            id: thread_id,
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        });
    }

    fn send_chat_message(&mut self) {
        if !self.threads.contains_key(&self.selected_thread) {
            self.create_new_chat_thread();
        }

        let (input, thread_id) = match self.threads.get_mut(&self.selected_thread) {
            Some(thread) => {
                let input = thread.input.trim().to_string();
                if input.is_empty() {
                    return;
                }
                thread.input.clear();
                (input, thread.id.clone())
            }
            None => return,
        };

        // Echo the user's message into the thread.
        let _ = self.response_tx.try_send(ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread_id.clone(),
            ts: crate::tabs::ai_playground::now_ts(),
            from: SentFrom::Me,
            content: ChatMessageType::Text(input.clone()),
        });

        // Input in an agent thread continues that conversation. Without this the
        // turn would silently fall through to the chat endpoint under a top bar
        // still naming the agent.
        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        if self.agent_threads.contains(&thread_id) {
            self.send_to_agent(thread_id, input, None);
            return;
        }

        // Input in the Claude Code thread resumes that session instead of the OpenAI endpoint.
        #[cfg(not(target_arch = "wasm32"))]
        if self.claude_thread.as_deref() == Some(thread_id.as_str()) {
            self.claude.send(input, None, thread_id, self.response_tx.clone());
            return;
        }

        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        {
            use crate::{PlatformSpawner, Spawner};
            let prior = self
                .threads
                .get(&thread_id)
                .map(|t| crate::ai::mcp_chat::history_json_from_messages(&t.messages))
                .unwrap_or_default();
            let tx = self.response_tx.clone();
            let use_tools = self.use_mcp_tools;
            PlatformSpawner::spawn(async move {
                if let Err(e) = crate::ai::mcp_chat::stream_chat(input, prior, thread_id, use_tools, tx).await {
                    log::error!("stream_chat error: {e:?}");
                }
            });
        }
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "tokio")))]
        {
            let _ = (input, thread_id);
        }
    }
}

fn short_title(s: &str) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= 28 {
        t
    } else {
        format!("{}…", t.chars().take(28).collect::<String>())
    }
}
