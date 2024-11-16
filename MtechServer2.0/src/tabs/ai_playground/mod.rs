use crate::{
    app_state::MtechServerContext,
    utilities::ai::{oa_client::new_oa_client, tool_call::{assistant_call_with_response_ai_tools, get_or_retrieve_thread}}
};
use async_openai_wasm::Threads;
use eframe::egui::{
    epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Key, Layout, Margin, Rect, RichText, 
    Rounding, ScrollArea, Sense, Shape, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget
};

use crossbeam::channel::{Receiver, Sender};
use egui_extras::syntax_highlighting::{code_view_ui, CodeTheme};
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;
use displays::markdown_editor::viewer;
use bytes::Bytes;
use core::str;
use std::collections::HashMap;

pub struct AiPlayground {
    selected_thread: String,
    threads: HashMap<String, ChatThread>,

    response_tx: Sender<ChatMessage>,
    response_rx: Receiver<ChatMessage>
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
            response_tx, response_rx,
        }
    }
}

impl MtechServerContext {
    pub fn ai_playground(&mut self, ui: &mut Ui) {
        SidePanel::left("ChatHistoryPanel")
            .default_width(50.)
            .frame(Frame::dark_canvas(ui.style()))
            .show_inside(ui, |ui| {
                let mut selected_thread = self.ai_playground.selected_thread.clone();
                if !self.ai_playground.threads.is_empty() {
                    for (thread_id, _) in self.ai_playground.threads.iter() {
                        let selected_thread_res = ui.selectable_label(
                            selected_thread.eq(&thread_id.clone()), 
                            RichText::new(thread_id.clone())
                        );

                        if selected_thread_res.clicked() {
                            selected_thread = thread_id.clone();
                        }
                    }
                } else {
                    let new_chat = Button::new("New chat +").ui(ui);
                    if new_chat.clicked() {
                        let tx = self.ai_thread_channel.0.clone();
                        spawn_local(async move {
                            // -- Initialize AI Client
                            let oa_client = new_oa_client().unwrap();
                            // let assistant_client = oa_client.assistants();
                            let asst_thread = Threads::new(&oa_client);
                            let thread = get_or_retrieve_thread(asst_thread, None).await.unwrap();
                            let _ = tx.try_send(thread);
                        });
                    }
                }


            });

        TopBottomPanel::bottom("ChatInputPanel")
            .frame(Frame::dark_canvas(ui.style()))
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
            Vec2::new(ui.available_width(), ui.available_height() - 20.0),
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
                        for (thread_id, thread) in self.threads.iter() {
                            for message in thread.messages.iter() {
                                if message.thread_id.eq(thread_id) {
                                    self.render_message(ui, message, max_msg_width, fixed_height, min_width);
                                }
                            }
                        }
                    });
            },
        );

        self.handle_events();
    }

    fn chat(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
            ui.vertical_centered_justified(|ui| {
                let text_edit = TextEdit::multiline(&mut thread.input)
                    .hint_text("Ask GPT to summarize a service order")
                    .ui(ui);

                let key_press = ui.input(|i| i.key_pressed(Key::Enter));

                if text_edit.lost_focus() && key_press {

                    text_edit.request_focus();
                    let response_tx = self.response_tx.clone();
                    let input = thread.input.clone();
                    thread.input.clear();

                    // let current_thread = &self.selected_thread;
                    let id = if !thread.id.is_empty() {
                        Some(thread.id.clone())
                    } else { None };

                    spawn_local(async move {
                        
                        let res = assistant_call_with_response_ai_tools(
                            input.as_str(), 
                            id, 
                            response_tx.clone()
                        ).await;

                        log::info!("Res: {res:?}");
                    });
                }
            });
        }
    }

    fn handle_events(&mut self) {
        // Append characters to the current streaming message
        while let Ok(response) = self.response_rx.try_recv() {
            gloo_console::log!(format!("Response: {response:?}"));
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
                ChatMessageType::Text(ref msg) => {
                    // Update or add the message in the thread
                    if let Some(existing_message) = current_thread.messages.iter_mut().find(|m| m.id == response.id) {
                        // Append new text to the existing message
                        if let ChatMessageType::Text(existing_content) = &mut existing_message.content {
                            existing_content.push_str(msg);
                        }
                    } else {
                        // Add the message if it's not already in the thread
                        current_thread.messages.push(response);
                    }
                }
                ChatMessageType::Image(_) | ChatMessageType::Code(_) | ChatMessageType::FileId(_) => {
                    // Directly add these types of messages
                    current_thread.messages.push(response);
                }
                ChatMessageType::Error(ref e) => {
                    log::info!("Error in response: {}", e);
                    current_thread.messages.push(response);
                }
                ChatMessageType::Done => {
                    // Finalize the message, no further action needed
                }
            }
        
        }
    }

    fn render_message(
        &self,
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

            let rounding = 8.0;
            let margin = 8.0;

            // ui.set_min_width(min_width);
            let rnding = Rounding {
                ne: if is_message_from_myself {
                    0.0
                } else {
                    rounding
                },
                nw: if is_message_from_myself {
                    rounding
                } else {
                    0.0
                },
                se: rounding,
                sw: rounding,
            };

            let response = Frame::none()
                .rounding(rnding)
                .inner_margin(margin)
                .outer_margin(margin)
                .fill(msg_color)
                .show(ui, |ui| {
                    ui.set_min_height(fixed_height); // Set the fixed height for the message box
                    ui.set_min_width(min_width / 2.5);
                    // Use a vertical layout to stack the name and message content
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        let mut shadow = Shadow::default();
                        shadow.blur = 3.0;
                        shadow.spread = 3.0;
                        shadow.color = Color32::from_rgb(40, 36, 40);

                        let mut b_panel_marg = Margin::default();
                        b_panel_marg.top = 3.0;

                        let color = Color32::from_rgb(10, 10, 12);

                        let note_frame = Frame::none()
                            .fill(color)
                            .shadow(shadow)
                            .stroke(
                                ui.style().visuals.widgets.inactive.bg_stroke,
                            )
                            .outer_margin(b_panel_marg)
                            .inner_margin(Margin::symmetric(6.0, 10.0))
                            .rounding(rnding);

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
                                    .rounding(Rounding::same(f32::INFINITY))
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
                                    .rounding(Rounding::same(f32::INFINITY))
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
                                    match &item.content {
                                        ChatMessageType::Text(msg) => {
                                            viewer::easy_mark(ui, msg);
                                        },
                                        ChatMessageType::Code(code) => {
                                            let language = "python";
                                            let theme = CodeTheme::from_memory(ui.ctx(), ui.style());
                                            code_view_ui(ui, &theme, code, language);
                                        },
                                        ChatMessageType::Image((_, img)) => {
                                            if let Ok(img) = str::from_utf8(img) {
                                                ui.image(img);
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

            let points = if !is_message_from_myself {
                let top = response.rect.left_top() + Vec2::splat(margin);
                let arrow_rect = Rect::from_two_pos(
                    top,
                    top + Vec2::new(-rounding, rounding),
                );

                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_top(),
                    arrow_rect.right_bottom(),
                ]
            } else {
                let top =
                    response.rect.right_top() + Vec2::new(-margin, margin);
                let arrow_rect = Rect::from_two_pos(
                    top,
                    top + Vec2::new(rounding, rounding),
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
