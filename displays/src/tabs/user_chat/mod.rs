use database::schema::User;
use eframe::egui::{
    epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, FontId, Frame, Id, Image, ImageSource, Key, KeyboardShortcut, Layout, Margin, Modifiers, Rect, RichText, ScrollArea, Sense, Shape, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget
};
use surrealdb::{sql::Datetime, RecordId};
use std::{borrow::Cow, sync::Arc};
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use bytes::Bytes;
use log::info;
use core::str;

use crate::{get_current_user_from_auth, get_database_users, markdown_editor::viewer::easy_mark};


#[derive(Debug, Clone, Serialize)]
pub struct UserChat {
    chat_title: String,
    pub selected_thread: Option<RecordId>,
    edit_title: bool,
    threads: Vec<ChatThread>,
    current_user: User,
    store_users: Vec<User>,
    #[serde(skip)]
    chat_action_tx: Sender<ChatAction>,
    #[serde(skip)]
    chat_action_rx: Receiver<ChatAction>,
    image_id: String,
    open_modal: bool,
    first_run:  bool
}

pub type ImageType = (String, Bytes);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: RecordId,
    pub messages: Vec<UserMessage>,
    pub images: Vec<ImageType>,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserMessage {
    pub id: RecordId,
    pub thread_id: RecordId,
    pub created_at: Datetime,
    pub from: User,
    pub content: ChatMessageType
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatMessageType {
    Text(String),
    Image(ImageType),
}

enum ChatAction {
    NewChat(RecordId),
    ArchiveChat(RecordId),
    RemoveChat(RecordId)
}

impl Default for ChatMessageType {
    fn default() -> Self {
        Self::Text(String::new())
    }
}
impl Default for UserChat {
    fn default() -> Self {
        let (chat_action_tx, chat_action_rx) = crossbeam::channel::unbounded();
        Self {
            chat_title: String::new(),
            selected_thread: None,
            threads: Vec::new(),
            edit_title: false,
            chat_action_tx, chat_action_rx,
            image_id: String::new(),
            open_modal: false,
            current_user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            },
            store_users: vec![],
            first_run: true
        }
    }
}

impl UserChat {
    pub fn ui(&mut self, ui: &mut Ui) {
        self.handle_events(ui);
        if self.first_run {
            if self.store_users.is_empty() {
                self.set_users();
            } else {
                self.first_run = false;
                log::warn!("Setting users");
            }
        }

        let username =  self.current_user.get_username().to_string();
        let title = if let Some(id) = &self.selected_thread {
            let uid = self.current_user.get_id();
            if id == &uid {
                username.clone()
            } else {
                "Select a chat to get started".to_string()
            }
        } else {
            "Select a chat to get started".to_string()
        };
        
        TopBottomPanel::top(title)
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_height(28.)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if !self.edit_title {
                        let t = self.chat_title.clone();
                        if Button::new(
                                RichText::new(format!("{t} 🖊"))
                                .heading()
                            )
                            .min_size(Vec2::new(10., 8.))
                            .ui(ui)
                            .clicked() 
                        {
                            self.edit_title = true;
                        }
                    } else {
                        let edit = TextEdit::singleline(&mut self.chat_title)
                        .margin(Margin::same(5))
                        .font(FontId::proportional(12.))
                        .ui(ui);

                        if edit.lost_focus() {
                            info!("self.chat_title: {:?}", self.chat_title);
                            // self.chat_title.get(&selected_thread).insert(&title.clone());
                            self.edit_title = false;
                        }
                    }
                });
            });

        SidePanel::left("ChatHistoryPanel")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_width(150.)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let mut selected_thread = self.selected_thread.clone();
                    if !self.threads.is_empty() {
                        for thread in self.threads.iter() {
                            let uid = self.current_user.get_id();
                            let title = if thread.id == uid {
                                self.current_user.get_username()
                            } else {
                                &thread.id.to_string()
                            };

                            let selected_thread_res = ui.selectable_label(
                                selected_thread.eq(&Some(thread.id.clone())), 
                                RichText::new(title)
                            );
                        
                            if selected_thread_res.clicked() {
                                selected_thread = Some(thread.id.clone());
                            }
                        }
                    } else {

                    }

                    
                    for user in self.store_users.iter() {
                        ui.add_space(10.);
                        if Button::new(user.get_username())
                            .min_size(Vec2::new(120., 24.))
                            .ui(ui)
                            .clicked() 
                        {
                            let tx = self.chat_action_tx.clone();
                            let _ = tx.try_send(ChatAction::NewChat(user.get_id()));
                        }
                    }
                });
            });

        TopBottomPanel::bottom("ChatInputPanel")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_height(150.)
            .show_inside(ui, |ui| {
                self.chat(ui);
            });

        CentralPanel::default()
            .frame(Frame::dark_canvas(ui.style()))
            .show_inside(ui, |ui| {
                self.display(ui);
            });

    }
    pub fn set_users(&mut self) {
        let me = self.current_user.clone();
       self.store_users = get_database_users().iter().filter(|u| u.get_store() == me.get_store()).cloned().collect::<Vec<User>>();
    }

    pub fn set_threads(&mut self, threads: Vec<ChatThread>) {
        self.threads = threads;
    }

    pub fn get_threads(&mut self) -> Vec<ChatThread> {
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
                        if !threads.is_empty() {
                            log::info!("Got a thread: {threads:?}");
                        }
                        for thread in threads.iter() {
                            if thread.messages.is_empty() {
                                // let query = [("limit", "1")]; // Limit the list responses to 1 message
                                // let _response: async_openai_wasm::types::ListMessagesResponse = oa_client
                                //     .files()
                                //     .list(&query)
                                //     .await?
                                //     .data;
                            }
                            for message in thread.messages.iter() {
                                if message.thread_id.eq(&thread.id) {
                                    self.render_message(ui, message, max_msg_width, fixed_height, min_width);
                                }
                            }
                        }
                    });
            },
        );
    }

    fn chat(&mut self, ui: &mut Ui) {
        let selected = self.selected_thread.clone();
        if let Some(thread) = self.threads.iter_mut().find(|t| Some(t.id.clone()) == selected) {
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
                    // Self::submit_input(thread, self.chat_action_tx.clone());
                }

                ui.add_space(10.);

                let submit = Button::new(RichText::new("⮫").heading())
                    .corner_radius(eframe::egui::CornerRadius::same(25))
                    .min_size(Vec2::new(60., ui.available_height()/1.5))
                    .stroke(Stroke::new(0.8, Color32::from_rgb(150, 12, 150)))
                    .ui(ui)
                    .on_hover_text(RichText::new("(Or CTRL + Shift to submit)"));

                if submit.clicked() {
                    // Self::submit_input(thread, self.chat_action_tx.clone());
                }
            });
        }
    }

    fn submit_input(thread: &mut ChatThread, response_tx: Sender<UserMessage>) {
        
        let input = thread.input.clone();
        thread.input.clear();


    }

    fn handle_events(&mut self, ui: &mut Ui) {
        // Append characters to the current streaming message
        if let Ok(response) = self.chat_action_rx.try_recv() {
            ui.ctx().request_repaint();

            // Ensure the thread exists
            let id = match response {
                ChatAction::NewChat(record_id) => record_id,
                ChatAction::ArchiveChat(record_id) => record_id,
                ChatAction::RemoveChat(record_id) => record_id,
            };

            self.selected_thread = Some(id.clone());

            self.threads
                .iter()
                .find(|t| t.id == id.clone())
                .get_or_insert(&ChatThread {
                    id: id.clone(),
                    messages: Vec::new(),
                    images: Vec::new(),
                    input: String::new(),
                });


            // match response.content {
            //     ChatMessageType::Text(ref msg) => {
            //         info!("msg ID: {}", response.id.clone());
            //         // Update or add the message in the thread
            //         if let Some(existing_message) = current_thread.messages.iter_mut().find(|m| m.id == id.clone()) {
            //             info!("Got existing_message: {}", response.id.clone());
            //             // Append new text to the existing message
            //             if let ChatMessageType::Text(existing_content) = &mut existing_message.content {
            //                 log::info!("Got msg of type Text: {msg}");
            //                 existing_content.push_str(msg);
            //             }
            //         } else {
            //             log::info!("We did NOT have an existing message. Pushing response: {:?}", response);
            //             // Add the message if it's not already in the thread
            //             current_thread.messages.push(response);
            //         }
            //     }
            //     ChatMessageType::Image((_, ref img)) => {
            //         info!("{img:?}");
            //         // Directly add these types of messages
            //         current_thread.messages.push(response);
            //     }
            // }
        
        }
    }

    fn render_message(
        &mut self,
        ui: &mut Ui,
        item: &UserMessage,
        max_msg_width: f32,
        fixed_height: f32,
        min_width: f32,
    ) {
        let is_message_from_myself = if item
            .from
            .get_id()
            .eq(&self.current_user.get_id()) { true } else { false };

        let from = match is_message_from_myself {
            true => RichText::new(self.current_user.get_username())
                .strong()
                .monospace()
                .color(Color32::LIGHT_BLUE),
            false => RichText::new(item.from.get_username())
                .strong()
                .monospace()
                .color(Color32::LIGHT_BLUE),
        };

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
                                        if let ChatMessageType::Text(txt) = &item.content {
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
                                        if let ChatMessageType::Text(txt) = &item.content {
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
                                        ChatMessageType::Text(msg) => easy_mark(ui, &msg),
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
