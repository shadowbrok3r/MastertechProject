use eframe::egui::{Align, Button, CentralPanel, Color32, Direction, Frame, Image, ImageSource, Layout, Margin, Popup, PopupCloseBehavior, RectAlign, Response, RichText, ScrollArea, SidePanel, Stroke, Style, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::{ChatAction, ChatMessageType, ChatThread, RecordIdExt, UserMessage};
use crate::{markdown_editor::viewer::easy_mark, PlatformSpawner, Spawner};
use std::{borrow::Cow, sync::Arc};
use rfd::AsyncFileDialog;
use super::UserChat;

impl UserChat {
    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive(ui);
        if self.first_run {
            self.first_run();
        }

        self.chat_title = if let Some(thread) = &self.selected_thread {
            let usr = &thread.user_created;
            self.store_users
                .iter()
                .find(|user| user.get_id() == *usr)
                .map_or("Select a chat to get started".to_string(), |u| u.get_username().to_string())
        } else {
            "Select a chat to get started".to_string()
        };

        eframe::egui::Panel::top(self.chat_title.clone())
            .frame(Frame::default().inner_margin(Margin::same(4)))
            .exact_height(28.)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if !self.edit_title {
                        let t = self.chat_title.clone();
                        if Button::new(RichText::new(format!("{t} 🖊")).heading())
                            .min_size(Vec2::new(10., 8.))
                            .ui(ui)
                            .clicked() 
                        {
                            self.edit_title = true;
                        }
                    } else {
                        let edit = TextEdit::singleline(&mut self.chat_title)
                        .margin(Margin::same(3))
                        .ui(ui);

                        if edit.lost_focus() {
                            log::info!("self.chat_title: {:?}", self.chat_title);
                            // self.chat_title.get(&selected_thread).insert(&title.clone());
                            self.edit_title = false;
                        }
                    }
                });
            });

        eframe::egui::Panel::left("ChatHistoryPanel")
            .exact_width(120.)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.add_space(10.);
                    let selected_thread = &mut self.selected_thread.clone();
                    if self.threads.is_empty() {
                        ui.label("No chats yet");
                    } else {
                        for thread in self.threads.iter() {
                            let uid = self.current_user.get_id();
                            let title = thread.thread_users
                                    .iter()
                                    .filter(|u| **u != uid)
                                    .map(|u| {
                                        self.store_users
                                            .iter()
                                            .find(|user| user.get_id() == *u)
                                            .map_or(u.key_string(), |user| user.get_username().to_string())
                                    })
                                    .collect::<Vec<String>>()
                                    .join(", ");

                            let selected_thread_res = ui.selectable_label(
                                selected_thread.as_ref().map_or(false, |t| t.id == thread.id),
                                RichText::new(title),
                            );

                            if selected_thread_res.clicked() {
                                *selected_thread = Some(thread.clone());
                                let thread_id = thread.id.clone();
                                let thread_tx = self.thread_tx.clone();
                                PlatformSpawner::spawn(async move {
                                    if let Ok(Some(thread)) = ChatThread::get_thread_from_id(thread_id).await {
                                        let _ = thread_tx.try_send(thread);
                                    }
                                });
                            }
                        }
                    }

                    ui.add_space(10.);
                    ui.separator();
                    ui.add_space(10.);

                    for user in self.store_users.iter() {
                        if Button::new(user.get_username())
                            .min_size(Vec2::new(120., 24.))
                            .ui(ui)
                            .clicked()
                        {
                            let tx = self.chat_action_tx.clone();
                            let _ = tx.try_send(ChatAction::SelectThread(user.get_id()));
                        }
                        ui.add_space(10.);
                    }
                });
            });

        eframe::egui::Panel::bottom("ChatInputPanel")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .default_height(150.)
            .max_height(300.)
            .show_inside(ui, |ui| self.chat_input(ui) );

        CentralPanel::default()
            .frame(Frame::dark_canvas(ui.style()))
            .show_inside(ui, |ui| self.display_thread(ui) );

    }
    
    pub fn display_thread(&mut self, ui: &mut Ui) {
        // Extract thread and messages outside the closure
        let threads = self.thread_messages.clone();
        let messages = self
            .selected_thread
            .as_ref()
            .and_then(|thread| threads.get(&thread.id));
        
        // messages.sort_by_key(|message| message.created_at.clone() );
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
                        if let Some(messages) = messages {
                            for message in messages.iter() {
                                self.display_message(ui, message, max_msg_width);
                            }
                        }
                    });
            },
        );
    }

    fn chat_input(&mut self, ui: &mut Ui) {
        if self.selected_thread.is_some() {
            ui.horizontal_centered(|ui| {
                let text_edit = TextEdit::multiline(&mut self.input)
                    .desired_width(ui.available_width()/1.1)
                    .margin(Margin::same(8))
                    .ui(ui);

                ui.add_space(10.);

                ui.vertical_centered(|ui| {
                    if Button::new(RichText::new(" 🖻 ").heading())
                    .min_size(Vec2::new(ui.available_width(), 25.))
                    .stroke(Stroke::new(0.8, Color32::from_rgb(150, 12, 150)))
                    .ui(ui)
                    .clicked() {
                        let tx = self.chat_action_tx.clone();
                        PlatformSpawner::spawn(async move {
                            let dialog = AsyncFileDialog::new().pick_files().await;
                            if let Some(files) = dialog {
                                let _ = tx.try_send(ChatAction::UploadedFiles(files));
                            }
                        });
                    }

                    ui.add_space(10.);

                    if Button::new(RichText::new(" ⮫ ").heading())
                        .min_size(Vec2::new(ui.available_width(), 25.))
                        .stroke(Stroke::new(0.8, Color32::from_rgb(150, 12, 150)))
                        .ui(ui)
                        .on_hover_text(RichText::new("(Or CTRL + Shift to submit)"))
                        .clicked() {
                            let _ = self.chat_action_tx.try_send(ChatAction::SubmitMessage(ChatMessageType::Text(self.input.clone())));
                            self.input.clear();
                            text_edit.request_focus();
                    }
                });
            });
        }
    }

    fn display_message(
        &mut self,
        ui: &mut Ui,
        item: &UserMessage,
        max_msg_width: f32,
    ) {
        let is_message_from_myself = if item
            .user
            .eq(&self.current_user.get_id()) { true } else { false };

        let usr = self.store_users.iter().find(|u| u.get_id() == item.user).cloned().unwrap_or_default();

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
            let rounding = 8.;
            let outer_margin = Margin { left: 1, right: 1, top: 4, bottom: 1 };
            let inner_margin = Margin { left: 0, right: 0, top: 4, bottom: 0 };

            let rnding = eframe::egui::CornerRadius {
                ne: if is_message_from_myself { 0 } else { rounding as u8 },
                nw: if is_message_from_myself { rounding as u8 } else { 0 },
                se: rounding as u8,
                sw: rounding as u8,
            };

            let style = ui.style().clone();

            let outer_f = &mut Frame::new()
            .corner_radius(rnding)
            .inner_margin(inner_margin)
            .outer_margin(outer_margin)
            .fill(msg_color)
            .show(ui, |ui| { // NOTE FRAME SCOPED UI
                let mut frame = Frame::NONE
                    .corner_radius(rnding)
                    .inner_margin(inner_margin)
                    .outer_margin(Margin { left: 0, right: 0, top: 4, bottom: 1 })
                    .fill(msg_color)
                    .begin(ui);

                frame.frame.stroke = style.visuals.widgets.open.bg_stroke;

                let ui = &mut frame.content_ui;
                ui.set_width(max_msg_width);

                // Use a vertical layout to stack the name and message content
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    if is_message_from_myself {
                        ui.horizontal(|ui| {
                            let btn_txt_color = ui.style().visuals.error_fg_color;
                            let username_txt_color = ui.style().visuals.hyperlink_color;
                            let from = RichText::new(self.current_user.get_username())
                                .strong()
                                .monospace()
                                .color(username_txt_color);
                            
                            ui.add_space(5.);
                            if  Button::new(RichText::new("🗙").color(btn_txt_color))
                            .min_size(Vec2::new(15., 14.))
                            .ui(ui)
                            .on_hover_text(
                                RichText::new("WARNING, this will delete the note from prestashop AND Master-tech.app\nIf this is what you want, DOUBLE CLICK to delete")
                                    .strong()
                                    .color(Color32::LIGHT_RED)
                            )
                            .double_clicked() {
                                let _ = self.chat_action_tx.try_send(ChatAction::DeleteMessage(item.id.clone()));
                            }

                            if Button::new(RichText::new("🗐").color(btn_txt_color))
                            .ui(ui)
                            .on_hover_text(RichText::new("Copy Task Note"))
                            .clicked() {
                                if let ChatMessageType::Text(txt) = &item.content {
                                    ui.ctx().copy_text(txt.clone());
                                }
                            }
                            
                            let id = &item.id;

                            if self.allow_edit.contains(&id.key_string()) {
                                if Button::new(RichText::new("Save").color(btn_txt_color))
                                .ui(ui)
                                .clicked() {
                                    let _ = self.chat_action_tx.try_send(ChatAction::SaveNote(item.clone()));
                                }

                                if Button::new(RichText::new("Cancel").color(Color32::LIGHT_RED))
                                .ui(ui)
                                .clicked() {
                                    let _ = self.chat_action_tx.try_send(ChatAction::CancelEdit(id.clone()));
                                }
                            } else {
                                    if Button::new(RichText::new("🖊").color(btn_txt_color)).ui(ui)
                                    .on_hover_text(RichText::new("Edit Task Note\nWARNING: This will modify the note in Prestashop AND Master-tech.app"))
                                    .clicked() {
                                        let _ = self.chat_action_tx.try_send(ChatAction::Edit(id.clone()));
                                    }
                            }

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui|{
                                ui.add_space(2.);
                                let from_btn = Button::new(from)
                                    .fill(Color32::from_rgb(7, 7, 9))
                                    .min_size(Vec2::new(30., 20.))
                                    .ui(ui);

                                if from_btn.clicked(){
                                    Popup::open_id(ui.ctx(), format!("sub_menu-from-{:?}", item.id).into());
                                    self.open = !self.open;
                                }

                                popup_widget(from_btn, style.clone(), &item);

                                ui.add_space(5.);

                                ui.label(RichText::new(item.created_at.format("%m/%d @ %I:%M%p").to_string()).weak());
                            });
                        });
                    } else {
                        ui.horizontal(|ui| {
                            let btn_txt_color = ui.style().visuals.error_fg_color;
                            let username_txt_color = ui.style().visuals.hyperlink_color;
                            let from = RichText::new(usr.get_username())
                                .strong()
                                .monospace()
                                .color(username_txt_color);
                            // ui.set_max_width(max_msg_width);
                            let id = &item.id;
                            ui.add_space(2.);
                            let from_btn = Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 20.)).ui(ui);
                            if from_btn.clicked(){
                                Popup::open_id(ui.ctx(), format!("sub_menu-from-{:?}", item.id).into());
                                self.open = !self.open;
                            }

                            popup_widget( from_btn, style.clone(), &item);

                            ui.add_space(5.);
                        
                            ui.label(RichText::new(item.created_at.format("%m/%d @ %I:%M%p").to_string()).weak());

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(5.);
                                if self.current_user.is_admin() {
                                    if self.allow_edit.contains(&id.key_string()) {
                                        if Button::new(RichText::new("Cancel").color(Color32::LIGHT_RED))
                                        .ui(ui)
                                        .clicked() {
                                            let _ = self.chat_action_tx.try_send(ChatAction::CancelEdit(id.clone()));
                                        }
                                        if Button::new(RichText::new("Save").color(btn_txt_color))
                                        .ui(ui)
                                        .clicked() {
                                            let _ = self.chat_action_tx.try_send(ChatAction::SaveNote(item.clone()));
                                        }
                                    } else {
                                        if Button::new(RichText::new("🖊").color(btn_txt_color)).ui(ui)
                                        .on_hover_text(RichText::new("Edit Task Note\nWARNING: This will modify the note in Prestashop AND Master-tech.app"))
                                        .clicked() {
                                            let _ = self.chat_action_tx.try_send(ChatAction::Edit(id.clone()));
                                        }
                                    }
                                }
                                
                                if Button::new(RichText::new("🗐").color(btn_txt_color))
                                .ui(ui)
                                .clicked(){
                                    if let ChatMessageType::Text(txt) = &item.content {
                                        ui.ctx().copy_text(txt.clone());
                                    }
                                }
                                
                                if self.current_user.is_admin() {
                                    if  Button::new(RichText::new("🗙").color(btn_txt_color))
                                    .ui(ui)
                                    .on_hover_text(
                                        RichText::new("DOUBLE CLICK to delete")
                                            .strong()
                                            .color(Color32::LIGHT_RED)
                                    )
                                    .double_clicked() {
                                        let _ = self.chat_action_tx.try_send(ChatAction::DeleteMessage(item.id.clone()));
                                    }
                                }
                            });
                        });
                    }

                    Frame::new() // Frame for the actual note text itself // or for modifying the note
                        .fill(Color32::from_rgb(10,10,12))
                        .stroke(style.visuals.widgets.inactive.bg_stroke)
                        .outer_margin(Margin {
                            top: 3,
                            ..Default::default()
                        })
                        .inner_margin(Margin::symmetric(6, 10))
                        .corner_radius(rnding)
                        .show(ui, |ui| 
                    {
                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                            Direction::TopDown,
                            Align::Center,
                        ), |ui| {
                            ui.set_width(ui.available_width());
                            if self.allow_edit.contains(&item.id.key_string()) {
                                if let Some(msg) = self.edit_text.get_mut(&item.id.key_string()){
                                        if let ChatMessageType::Text(note) = &mut msg.content {
                                        TextEdit::multiline(note)
                                            .margin(Margin::symmetric(10, 3))
                                            .desired_width(max_msg_width)
                                            .show(ui);
                                    }
                                }
                            } else {
                                match &item.content {
                                    ChatMessageType::Text(msg) => easy_mark(ui, &msg),
                                    ChatMessageType::Image((file_id, bytes)) => {
                                        let txt = if self.image_id.eq(file_id) { "⏶" } else { "⏷" };
                                        if Button::new(RichText::new(format!("{txt} {file_id}")).strong()).ui(ui).clicked() {
                                            let _ = self.chat_action_tx.try_send(ChatAction::OpenImage(file_id.clone()));
                                        }

                                        let image_source = ImageSource::Bytes {
                                            uri: Cow::from(format!("bytes://{file_id}")),
                                            bytes: eframe::egui::load::Bytes::Shared(Arc::from(bytes.to_vec())),
                                        };
                                        
                                        if self.image_id.eq(file_id) {
                                            Image::new(image_source)
                                                .show_loading_spinner(true)
                                                .fit_to_original_size(0.8)
                                                .max_size(Vec2::new(800., 700.))
                                                .ui(ui);
                                        }
                                    }
                                }
                            }
                        });
                    });
                });

                frame
            }).inner;

            
            let response = outer_f.allocate_space(ui); //.allocate_space(ui);
            // log::info!("Message left edge: {:?}", response.rect.left());
            if response.hovered() {
                // style.visuals.widgets.inactive.bg_fill
                outer_f.frame.fill =  style.visuals.widgets.inactive.bg_fill + Color32::from_rgb(1, 1, 4);
                outer_f.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
                outer_f.frame.shadow = style.visuals.window_shadow;
            }
            outer_f.paint(ui);
        });
    }

}


pub fn popup_widget(btn_response: Response, style: Arc<Style>, item: &UserMessage) {
    Popup::menu(&btn_response)
    .width(btn_response.rect.width().min(300.0))
    .align(RectAlign::BOTTOM)
    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
    // .open_bool(open)
    .show(|ui| {
        ui.vertical_centered_justified(|ui| {
            ui.set_width(300.0);
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("user_message:{}", item.id.key_string()));
                });
            });
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "Task ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("chat_thread:{}", item.thread_id.key_string()));
                });
            });
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "User ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("User:{}", item.user.key_string()));
                });
            });
        });
    });
}


