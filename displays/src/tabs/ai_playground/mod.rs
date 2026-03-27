use eframe::egui::{
    epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, FontId, Frame, Id, Image, ImageSource, Key, KeyboardShortcut, Layout, Margin, Modifiers, Rect, RichText, ScrollArea, Sense, Shape, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget
};
use crate::{ai::{oa_client::new_oa_client, tool_call::{assistant_call_with_response_ai_tools, get_or_retrieve_thread}}, app_state::SharedContext, markdown_editor::viewer, openai::Threads, PlatformSpawner, Spawner};
use egui_extras::syntax_highlighting::{code_view_ui, CodeTheme};
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use bytes::Bytes;
use log::info;
use core::str;

pub mod enhanced;

pub struct AiPlayground {
    pub selected_thread: String,
    chat_title: HashMap<String, String>,
    edit_title: bool,
    threads: HashMap<String, ChatThread>,

    response_tx: Sender<ChatMessage>,
    response_rx: Receiver<ChatMessage>,
    /// Save AI chats to local storage // SurrealDB for persistence
    pub save_chats: bool,
    image_id: String,
    open_modal: bool,
}

pub type ImageType = (String, Bytes);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    pub images: Vec<ImageType>,
    pub input: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    pub ts: i32,
    pub from: SentFrom,
    pub content: ChatMessageType
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SentFrom {
    #[default]
    Me,
    Gpt
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatMessageType {
    Text(String),
    FileId(String),
    Code(String),
    Image(ImageType),
    Error(String),
    Done
}

impl Default for ChatMessageType {
    fn default() -> Self {
        ChatMessageType::Text(String::new())
    }
}

impl Default for AiPlayground {
    fn default() -> Self {
        let (
            response_tx, 
            response_rx
        ) = crossbeam::channel::unbounded::<ChatMessage>();

        Self {
            selected_thread: String::new(),
            threads: HashMap::new(),
            chat_title: HashMap::new(),
            edit_title: false,
            response_tx, response_rx,
            save_chats: false,
            image_id: String::new(),
            open_modal: false,
        }
    }
}

impl SharedContext {
    pub fn ai_playground(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("GPT")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_height(50.)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let title = if let Some(title) = self
                        .ai_playground
                        .chat_title
                        .get_mut(&self.ai_playground.selected_thread) 
                    { 
                        title 
                    } else { 
                        &mut self.ai_playground.selected_thread 
                    };

                    ui.add_space(ui.available_width()/2.5);
                    if !self.ai_playground.edit_title {
                        ui.heading(self.ai_playground.selected_thread.clone());
                        ui.add_space(10.);
                        if Button::new(
                                RichText::new("🖊")
                                .heading()
                            )
                            .min_size(Vec2::new(10., 8.))
                            .ui(ui)
                            .clicked() 
                        {
                            self.ai_playground.edit_title = true;
                        }
                    } else {
                        let edit = TextEdit::singleline(title)
                        .margin(Margin::same(5))
                        .font(FontId::proportional(12.))
                        .ui(ui);
                        // request keyboard focus somehow..
                        ui.add_space(10.);
                        let done = Button::new(
                                RichText::new("✔")
                                .heading()
                            )
                            .min_size(Vec2::new(10., 8.))
                            .ui(ui);

                        if edit.lost_focus() ||  done.clicked() {
                            info!("self.ai_playground.chat_title: {:?}", self.ai_playground.chat_title);
                            // self.ai_playground.chat_title.get(&selected_thread).insert(&title.clone());
                            self.ai_playground.edit_title = false;
                        }
                    }
                });
            });

        eframe::egui::Panel::left("ChatHistoryPanel")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_width(175.)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let mut selected_thread = self.ai_playground.selected_thread.clone();
                    if !self.ai_playground.threads.is_empty() {
                        for (thread_id, _) in self.ai_playground.threads.iter() {
                            let title = if let Some(title) = self
                                .ai_playground
                                .chat_title
                                .get_key_value(thread_id) 
                            { 
                                title 
                            } else { 
                                (&selected_thread, &selected_thread)
                            };

                            let selected_thread_res = ui.selectable_label(
                                selected_thread.eq(&thread_id.clone()), 
                                RichText::new(title.1)
                            );
                        
                            if selected_thread_res.clicked() {
                                info!(
                                    "Selected: {selected_thread:?}\nthread_id: {thread_id:?}\nBool: {:?}", 
                                    selected_thread.eq(&thread_id.clone())
                                );
                                selected_thread = thread_id.clone();
                            }
                        }
                    } else {

                    }

                    ui.add_space(10.);
                    let new_chat = Button::new("New ➕")
                        .corner_radius(eframe::egui::CornerRadius::same(25))
                        .min_size(Vec2::new(120., 24.))
                        .stroke(Stroke::new(0.8, Color32::from_rgb(150, 12, 150)))
                        .ui(ui);

                    if new_chat.clicked() {

                        let tx = self.ai_thread_channel.0.clone();
                        PlatformSpawner::spawn(async move {
                            // -- Initialize AI Client
                            let oa_client = new_oa_client().unwrap();
                            // let assistant_client = oa_client.assistants();
                            let asst_thread = Threads::new(&oa_client);
                            let thread = get_or_retrieve_thread(asst_thread, None).await.unwrap();
                            let _ = tx.try_send(thread);
                        });
                    }
                });
            });

        eframe::egui::Panel::bottom("ChatInputPanel")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_height(75.)
            .show_inside(ui, |ui| {
                self.ai_playground.chat(ui);
            });

        CentralPanel::default()
            .frame(Frame::dark_canvas(ui.style()))
            .show_inside(ui, |ui| {
                self.ai_playground.display(ui);
            });

    }
}

impl AiPlayground {
    pub fn set_threads(&mut self, threads: HashMap<String, ChatThread>) {
        self.threads = threads;
    }

    pub fn get_threads(&mut self) -> HashMap<String, ChatThread> {
        self.threads.clone()
    }

    pub fn display(&mut self, ui: &mut Ui) {
        ui.allocate_ui(
            Vec2::new(ui.available_width(), ui.available_height()),
            |ui| {
                ScrollArea::vertical()
                    .animated(true)
                    .max_height(ui.available_height())
                    .max_width(f32::INFINITY)
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let max_msg_width = ui.available_width() / 2.5;
                        let fixed_height = 50.0;
                        let min_width = 200.0;

                        // Render all finalized messages
                        let threads = self.threads.clone();
                        for (thread_id, thread) in threads.iter() {
                            if thread.messages.is_empty() {
                                // let query = [("limit", "1")]; // Limit the list responses to 1 message
                                // let _response: async_openai_wasm::types::ListMessagesResponse = oa_client
                                //     .files()
                                //     .list(&query)
                                //     .await?
                                //     .data;
                            }
                            for message in thread.messages.iter() {
                                if message.thread_id.eq(thread_id) {
                                    self.render_message(ui, message, max_msg_width, fixed_height, min_width);
                                }
                            }
                        }
                    });
            },
        );

        self.handle_events(ui);
    }

    fn chat(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
            ui.horizontal_centered(|ui| {

                let add_media = Button::new(RichText::new("🖻").heading())
                    .corner_radius(eframe::egui::CornerRadius::same(25))
                    .min_size(Vec2::new(60., ui.available_height()/1.5))
                    .stroke(Stroke::new(0.8, Color32::from_rgb(150, 12, 150)))
                    .ui(ui);
                    // .on_hover_text(RichText::new("(Or CTRL + Shift to submit)"));

                if add_media.clicked() {
                    // Self::submit_input(thread, self.response_tx.clone());
                }

                ui.add_space(10.);

                let text_edit = TextEdit::multiline(&mut thread.input)
                    .desired_width(ui.available_width()/1.1)
                    .hint_text("Ask GPT to summarize a service order")
                    .margin(Margin::same(8))
                    .return_key(Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter)))
                    .ui(ui);

                let key_press = ui
                    .input(|i| 
                        i.key_pressed(Key::Enter)
                    );

                if text_edit.lost_focus() && key_press {
                    text_edit.request_focus();
                    Self::submit_input(thread, self.response_tx.clone());
                }

                ui.add_space(10.);

                let submit = Button::new(RichText::new("⮫").heading())
                    .corner_radius(eframe::egui::CornerRadius::same(25))
                    .min_size(Vec2::new(60., ui.available_height()/1.5))
                    .stroke(Stroke::new(0.8, Color32::from_rgb(150, 12, 150)))
                    .ui(ui)
                    .on_hover_text(RichText::new("(Or CTRL + Shift to submit)"));

                if submit.clicked() {
                    Self::submit_input(thread, self.response_tx.clone());
                }
            });
        }
    }

    fn submit_input(thread: &mut ChatThread, response_tx: Sender<ChatMessage>) {
        
        let input = thread.input.clone();
        thread.input.clear();

        // let current_thread = &self.selected_thread;
        let id = if !thread.id.is_empty() {
            Some(thread.id.clone())
        } else { None };

        PlatformSpawner::spawn(async move {
            
            let res = assistant_call_with_response_ai_tools(
                input.as_str(), 
                id, 
                response_tx.clone()
            ).await;

            log::info!("Res: {res:?}");
        });
    }

    fn handle_events(&mut self, ui: &mut Ui) {
        // Append characters to the current streaming message
        while let Ok(response) = self.response_rx.try_recv() {
            ui.ctx().request_repaint();
                    // Ensure the thread exists
            let current_thread = self
                .threads
                .entry(response.thread_id.clone())
                .or_insert_with(|| ChatThread {
                    id: response.thread_id.clone(),
                    messages: Vec::new(),
                    images: Vec::new(),
                    input: String::new(),
                }
            );

            match response.content {
                ChatMessageType::Text(ref msg) | ChatMessageType::Code(ref msg) => {
                    info!("msg ID: {}", response.id.clone());
                    // Update or add the message in the thread
                    if let Some(existing_message) = current_thread.messages.iter_mut().find(|m| m.id == response.id) {
                        info!("Got existing_message: {}", response.id.clone());
                        // Append new text to the existing message
                        if let ChatMessageType::Text(existing_content) = &mut existing_message.content {
                            log::info!("Got msg of type Text: {msg}");
                            existing_content.push_str(msg);
                        }
                    } else {
                        log::info!("We did NOT have an existing message. Pushing response: {:?}", response);
                        // Add the message if it's not already in the thread
                        current_thread.messages.push(response);
                    }
                }
                ChatMessageType::Image((_, ref img)) => {
                    info!("{img:?}");
                    // Directly add these types of messages
                    current_thread.messages.push(response);
                }
                ChatMessageType::FileId(_)  => {
                    current_thread.messages.push(response);
                }
                ChatMessageType::Error(ref e) => {
                    log::error!("Error in response: {}", e);
                    current_thread.messages.push(response);
                }
                ChatMessageType::Done => {
                    self.save_chats = true;
                    // Finalize the message, no further action needed
                }
            }
        
        }
    }

    fn render_message(
        &mut self,
        ui: &mut Ui,
        item: &ChatMessage,
        max_msg_width: f32,
        fixed_height: f32,
        min_width: f32,
    ) {
        let is_message_from_myself =
            if item.from.eq(&SentFrom::Me) { true } else { false };

        // Messages from the user are right-aligned.
        let layout = if is_message_from_myself {
            Layout::top_down(Align::Max)
        } else {
            Layout::top_down(Align::Min)
        };

        let msg_color = if is_message_from_myself {
            ui.style().visuals.widgets.inactive.bg_fill
        } else {
            ui.style().visuals.widgets.active.weak_bg_fill
        };

        ui.with_layout(layout, |ui| {
            ui.set_max_width(max_msg_width);

            let rounding = 8;
            let margin = 8;

            // ui.set_min_width(min_width);
            let rnding = eframe::egui::CornerRadius {
                ne: if is_message_from_myself {
                    0
                } else {
                    rounding
                },
                nw: if is_message_from_myself {
                    rounding
                } else {
                    0
                },
                se: rounding,
                sw: rounding,
            };

            let response = Frame::new()
                .corner_radius(rnding)
                .inner_margin(margin)
                .outer_margin(margin)
                .fill(msg_color)
                .show(ui, |ui| {
                    ui.set_min_height(fixed_height); // Set the fixed height for the message box
                    ui.set_min_width(min_width / 2.5);
                    // Use a vertical layout to stack the name and message content
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        let mut shadow = Shadow::default();
                        shadow.blur = 3;
                        shadow.spread = 3;
                        shadow.color = Color32::from_rgb(40, 36, 40);

                        let mut b_panel_marg = Margin::default();
                        b_panel_marg.top = 3;

                        let color = Color32::from_rgb(10, 10, 12);

                        let note_frame = Frame::new()
                            .fill(color)
                            .shadow(shadow)
                            .stroke(
                                ui.style().visuals.widgets.inactive.bg_stroke,
                            )
                            .outer_margin(b_panel_marg)
                            .inner_margin(Margin::symmetric(6, 10))
                            .corner_radius(rnding);

                        let from = match item.from {
                            SentFrom::Me => {
                                RichText::new("You")
                                    .strong()
                                    .monospace()
                                    .color(Color32::LIGHT_BLUE)
                            },
                            SentFrom::Gpt => {
                                RichText::new("GPT")
                                    .strong()
                                    .monospace()
                                    .color(Color32::LIGHT_BLUE)
                            }
                        };

                        if is_message_from_myself {
                            ui.with_layout(
                                Layout::from_main_dir_and_cross_align(
                                    Direction::RightToLeft,
                                    Align::Min,
                                ),
                                |ui| {
                                    ui.add_space(8.0);
                                    Button::new(from)
                                        .fill(Color32::TRANSPARENT)
                                        .min_size(Vec2::new(30.0, 20.0))
                                        .sense(Sense::hover())
                                        .ui(ui);

                                    ui.add_space(20.0);

                                    let copy_btn = Button::new(
                                        RichText::new("🗐")
                                            .weak()
                                            .color(Color32::LIGHT_RED),
                                    )
                                    .corner_radius(eframe::egui::CornerRadius::same(255))
                                    .small()
                                    .min_size(Vec2::new(30.0, 14.0))
                                    .ui(ui);

                                    if copy_btn.clicked() {
                                        if let 
                                            ChatMessageType::Code(txt) 
                                            | ChatMessageType::Text(txt) 
                                            = &item.content
                                        {
                                            ui.ctx().copy_text(txt.clone());
                                        }
                                    }
                                },
                            );
                        } else {
                            ui.with_layout(
                                Layout::from_main_dir_and_cross_align(
                                    Direction::LeftToRight,
                                    Align::Min,
                                ),
                                |ui| {
                                    ui.add_space(8.0);
                                    Button::new(from)
                                        .fill(Color32::TRANSPARENT)
                                        .min_size(Vec2::new(30.0, 20.0))
                                        .sense(Sense::hover())
                                        .ui(ui);
                                    ui.add_space(35.0);
                                    let copy_btn = Button::new(
                                        RichText::new("🗐")
                                            .small()
                                            .weak()
                                            .color(Color32::LIGHT_RED),
                                    )
                                    .corner_radius(eframe::egui::CornerRadius::same(255))
                                    .small()
                                    .min_size(Vec2::new(30.0, 14.0))
                                    .ui(ui);

                                    if copy_btn.clicked() {
                                        if let 
                                            ChatMessageType::Code(txt) 
                                            | ChatMessageType::Text(txt) 
                                            = &item.content
                                        {
                                            ui.ctx().copy_text(txt.clone());
                                        }
                                    }
                                },
                            );
                        }
                        note_frame.show(ui, |ui| {
                            ui.with_layout(
                                Layout::from_main_dir_and_cross_align(
                                    Direction::TopDown,
                                    Align::Center,
                                ),
                                |ui| {
                                    ui.set_width(ui.available_width());
                                    // info!("Got msg: {item:?}"));
                                    match &item.content {
                                        ChatMessageType::Text(msg) 
                                        | ChatMessageType::FileId(msg) 
                                        | ChatMessageType::Error(msg) => viewer::easy_mark(ui, msg),
                                        ChatMessageType::Code(code) => {
                                            info!("Got code: {code:?}");
                                            let language = "python";
                                            let theme = CodeTheme::from_memory(ui.ctx(), ui.style());
                                            code_view_ui(ui, &theme, code, language);
                                        },
                                        ChatMessageType::Image((file_id, bytes)) => {
                                            // info!("Got an img: {bytes:?}"));
                                            // Convert `bytes::Bytes` into `Arc<[u8]>` required by `egui::load::Bytes`
                                            let egui_bytes: eframe::egui::load::Bytes = eframe::egui::load::Bytes::Shared(Arc::from(bytes.to_vec()));
                                            
                                            let unique_uri = format!("bytes://{file_id}.png");
                                            let uri = Cow::from(unique_uri);

                                            let image_source = ImageSource::Bytes {
                                                uri,
                                                bytes: egui_bytes,
                                            };
                                            
                                            let modal = eframe::egui::Modal::new(
                                                Id::new(format!("{file_id}"))
                                            ).show(ui.ctx(), |ui| {
                                                if self.image_id.eq(file_id) {
                                                    Image::new(image_source)
                                                        .show_loading_spinner(true)
                                                        .fit_to_original_size(0.8)
                                                        .max_size(Vec2::new(800., 700.))
                                                        .ui(ui);
                                                    info!("Available size: {:?}", ui.available_size());
                                                }
                                            });
                                            // .button(ui, "Close")

                                            if Button::new(
                                                RichText::new(
                                                    format!("Image {}", file_id)
                                                )
                                                .color(
                                                    Color32::from_rgb(155, 12, 165)
                                                )
                                                .strong()
                                            )
                                            .ui(ui)
                                            .clicked() {
                                                self.image_id = file_id.to_string();
                                                self.open_modal = true;
                                            // if Image::new(image_source.clone()).show_loading_spinner(true).max_size(ui.available_size()/2.).fit_to_original_size(0.8).ui(ui).clicked(){
                                            }
                                            
                                            if modal.backdrop_response.clicked() {
                                                self.open_modal = false;
                                            }
                                        },
                                        _ => {}
                                    }
                                },
                            );
                        });
                    });
                })
                .response;
            
            let r = rounding as f32;

            let points = if !is_message_from_myself {
                let top = response.rect.left_top() + Vec2::splat(margin as f32);
                
                let arrow_rect = Rect::from_two_pos(
                    top,
                    top + Vec2::new(-r, r),
                );

                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_top(),
                    arrow_rect.right_bottom(),
                ]
            } else {
                let top =
                    response.rect.right_top() + Vec2::new(-r, r);
                let arrow_rect = Rect::from_two_pos(
                    top,
                    top + Vec2::new(r, r),
                );

                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_top(),
                    arrow_rect.left_bottom(),
                ]
            };

            ui.painter().add(Shape::convex_polygon(
                points,
                msg_color,
                Stroke::NONE,
            ));
        });
    }
}
