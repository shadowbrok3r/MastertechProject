use eframe::egui::{
    Align, Button, CentralPanel, CollapsingHeader, Color32, Frame, Key, KeyboardShortcut, Layout,
    Margin, Modifiers, Popup, PopupCloseBehavior, RichText, ScrollArea, TextEdit, Ui,
};
use crate::{
    markdown_editor::viewer,
    tabs::ai_playground::{ChatMessage, ChatMessageType, ChatThread, SentFrom},
    ui_tools::icons,
    PlatformSpawner, Spawner,
};

use std::collections::HashMap;
use crossbeam::channel::{Receiver, Sender};
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

    pub fn enhanced_ai_playground(&mut self, ui: &mut Ui) {
        self.ensure_loaded();

        eframe::egui::Panel::top("enhanced_ai_topbar")
            .frame(Frame::default().inner_margin(Margin::symmetric(6, 2)))
            .exact_size(28.)
            .show_separator_line(false)
            .show_inside(ui, |ui| self.show_chat_topbar(ui));

        eframe::egui::Panel::bottom("enhanced_ai_input")
            .frame(Frame::default().inner_margin(Margin::same(6)))
            .exact_size(92.)
            .show_inside(ui, |ui| self.show_chat_input(ui));

        CentralPanel::default()
            .frame(Frame::central_panel(ui.style()).inner_margin(Margin::same(10)))
            .show_inside(ui, |ui| self.show_chat_content(ui));

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
                    if self.threads.is_empty() {
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
                    });
                });
            let stored = popup.map(|r| r.response.rect).unwrap_or(eframe::egui::Rect::NOTHING);
            ui.memory_mut(|m| m.data.insert_temp(rect_id, stored));
            if let Some(id) = picked {
                self.selected_thread = id;
            }

            if ui.button(RichText::new(icons::PLUS)).on_hover_text("New chat").clicked() {
                self.create_new_chat_thread();
            }

            // ── Right side: close · tools · model ──
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(RichText::new(icons::CLOSE)).on_hover_text("Close chat").clicked() {
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
                let model = crate::ai::effective_model(crate::ai::gpts::MODEL);
                ui.label(RichText::new(model).weak().small());
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
                ui.heading(RichText::new("Mastertech Assistant").strong());
                ui.label(RichText::new("Ask a question to get started.").weak());
            });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for message in &messages {
                    self.render_chat_message(ui, message);
                    ui.add_space(6.);
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
                        viewer::easy_mark(ui, reasoning);
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
                        viewer::easy_mark(ui, text);
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

    fn upsert_stream(&mut self, tid: &str, id: &str, ts: i32, from: SentFrom, chunk: String, reasoning: bool) {
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
            let messages = serde_json::to_value(&thread.messages).unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
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
            ts: 0,
            from: SentFrom::Me,
            content: ChatMessageType::Text(input.clone()),
        });

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
