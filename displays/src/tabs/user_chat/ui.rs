use super::UserChat;
use crate::{
    PlatformSpawner, Spawner,
    chats::{
        BubbleAction, DELETE_TIP, bubble_actions, bubble_corners, detail_row, sender_chip, sent_at,
    },
    markdown_editor::viewer::easy_mark,
    ui_tools::{
        controls::{
            self, BarKind, ButtonKind, IconBox, IconButton, Metrics, TextArea, TextButton,
            TextField, Tokens,
        },
        icons::svg,
        list_row::{Lead, ListRow},
        theme,
    },
};
use database::schema::{ChatAction, ChatMessageType, ChatThread, RecordIdExt, UserMessage};
use eframe::egui::{
    Align, CentralPanel, Direction, Frame, Image, ImageSource, Layout, Margin, Popup,
    PopupCloseBehavior, RectAlign, Response, ScrollArea, Stroke, Ui, Vec2, Widget,
};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use rfd::AsyncFileDialog;
use std::{borrow::Cow, sync::Arc};

/// Width of the chat list.
const LIST_W: f32 = 120.0;
/// Width of the title field while renaming.
const TITLE_FIELD_W: f32 = 240.0;
/// Width of a sender's details popup.
const SENDER_POPUP_W: f32 = 300.0;
/// Hover warning on deleting someone else's message.
const ADMIN_DELETE_TIP: &str = "DOUBLE CLICK to delete";

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
                .map_or("Select a chat to get started".to_string(), |u| {
                    u.get_username().to_string()
                })
        } else {
            "Select a chat to get started".to_string()
        };

        let m = Metrics::get(ui.ctx());
        eframe::egui::Panel::top(self.chat_title.clone())
            .frame(Frame::side_top_panel(&ui.global_style()).inner_margin(Margin::ZERO))
            .exact_size(m.header_h)
            .show(ui, |ui| {
                controls::bar(ui, BarKind::Header, |ui| {
                    if !self.edit_title {
                        let title = self.chat_title.clone();
                        let rename = TextButton::new(&title)
                            .icon(svg::CHAT_CIRCLE_DOTS)
                            .kind(ButtonKind::Ghost)
                            .show(ui)
                            .on_hover_text("Rename this chat");
                        if rename.clicked() {
                            self.edit_title = true;
                        }
                    } else {
                        let edit = TextField::new(&mut self.chat_title)
                            .icon(svg::PENCIL_SIMPLE)
                            .width(TITLE_FIELD_W)
                            .show(ui);

                        if edit.lost_focus() {
                            log::info!("self.chat_title: {:?}", self.chat_title);
                            self.edit_title = false;
                        }
                    }
                });
            });

        eframe::egui::Panel::left("ChatHistoryPanel")
            .exact_size(LIST_W)
            .show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.add_space(m.gap.y);
                    controls::section_label(ui, "Chats");
                    let selected_thread = &mut self.selected_thread.clone();
                    if self.threads.is_empty() {
                        controls::empty_state(ui, svg::CHAT_CIRCLE_DOTS, "No chats yet", "", &[]);
                    } else {
                        for thread in self.threads.iter() {
                            let uid = self.current_user.get_id();
                            let title = thread
                                .thread_users
                                .iter()
                                .filter(|u| **u != uid)
                                .map(|u| {
                                    self.store_users
                                        .iter()
                                        .find(|user| user.get_id() == *u)
                                        .map_or(u.key_string(), |user| {
                                            user.get_username().to_string()
                                        })
                                })
                                .collect::<Vec<String>>()
                                .join(", ");

                            let selected_thread_res = ListRow::new(&title)
                                .lead(Lead::Icon(svg::CHAT_CIRCLE_DOTS, None))
                                .selected(
                                    selected_thread.as_ref().is_some_and(|t| t.id == thread.id),
                                )
                                .show(ui);

                            if selected_thread_res.clicked() {
                                *selected_thread = Some(thread.clone());
                                let thread_id = thread.id.clone();
                                let thread_tx = self.thread_tx.clone();
                                PlatformSpawner::spawn(async move {
                                    if let Ok(Some(thread)) =
                                        ChatThread::get_thread_from_id(thread_id).await
                                    {
                                        let _ = thread_tx.try_send(thread);
                                    }
                                });
                            }
                        }
                    }

                    ui.add_space(m.gap.y);
                    controls::hairline(ui);
                    ui.add_space(m.gap.y);
                    controls::section_label(ui, "People");

                    for user in self.store_users.iter() {
                        let row = ListRow::new(user.get_username())
                            .lead(Lead::Icon(svg::USER, None))
                            .show(ui)
                            .on_hover_text("Open a chat with this person");
                        if row.clicked() {
                            let tx = self.chat_action_tx.clone();
                            let _ = tx.try_send(ChatAction::SelectThread(user.get_id()));
                        }
                    }
                });
            });

        eframe::egui::Panel::bottom("ChatInputPanel")
            .frame(Frame::default().inner_margin(Margin::same(m.gap.x as i8)))
            .default_size(150.)
            .max_size(300.)
            .show(ui, |ui| self.chat_input(ui));

        CentralPanel::default()
            .frame(Frame::dark_canvas(ui.style()))
            .show(ui, |ui| self.display_thread(ui));
    }

    pub fn display_thread(&mut self, ui: &mut Ui) {
        // Extract thread and messages outside the closure
        let threads = self.thread_messages.clone();
        let messages = self
            .selected_thread
            .as_ref()
            .and_then(|thread| threads.get(&thread.id));
        let selected = self.selected_thread.is_some();

        // messages.sort_by_key(|message| message.created_at.clone() );
        ui.allocate_ui(
            Vec2::new(ui.available_width(), ui.available_height()),
            |ui| {
                ScrollArea::vertical()
                    .animated(true)
                    .max_width(ui.available_height())
                    .max_width(f32::INFINITY)
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let max_msg_width = ui.available_width() / 2.5;
                        match messages {
                            Some(messages) if !messages.is_empty() => {
                                for message in messages.iter() {
                                    self.display_message(ui, message, max_msg_width);
                                }
                            }
                            _ if selected => {
                                controls::empty_state(
                                    ui,
                                    svg::CHAT_TEXT,
                                    "No messages yet",
                                    "Send the first one below.",
                                    &[],
                                );
                            }
                            _ => {
                                controls::empty_state(
                                    ui,
                                    svg::CHAT_CIRCLE_DOTS,
                                    "No chat selected",
                                    "Pick a chat or a person to start.",
                                    &[],
                                );
                            }
                        }
                    });
            },
        );
    }

    fn chat_input(&mut self, ui: &mut Ui) {
        if self.selected_thread.is_some() {
            let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
            ui.horizontal_centered(|ui| {
                let text_edit = TextArea::new(&mut self.input)
                    .hint("Message")
                    .width(ui.available_width() / 1.1)
                    .show(ui);

                ui.add_space(m.gap.x);

                ui.vertical_centered(|ui| {
                    #[cfg(not(any(target_os = "ios", target_os = "android")))]
                    if controls::icon_button(ui, svg::PAPERCLIP, IconBox::Lg, false, "Attach files")
                        .clicked()
                    {
                        let tx = self.chat_action_tx.clone();
                        PlatformSpawner::spawn(async move {
                            let dialog = AsyncFileDialog::new().pick_files().await;
                            if let Some(files) = dialog {
                                let _ = tx.try_send(ChatAction::UploadedFiles(files));
                            }
                        });
                    }

                    ui.add_space(m.gap.y);

                    let send = IconButton::new(svg::PAPER_PLANE_RIGHT, IconBox::Lg)
                        .tint(t.primary)
                        .tip("Send (or Ctrl + Shift)")
                        .show(ui);
                    if send.clicked() {
                        let _ = self.chat_action_tx.try_send(ChatAction::SubmitMessage(
                            ChatMessageType::Text(self.input.clone()),
                        ));
                        self.input.clear();
                        text_edit.request_focus();
                    }
                });
            });
        }
    }

    fn display_message(&mut self, ui: &mut Ui, item: &UserMessage, max_msg_width: f32) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        let is_message_from_myself = item.user.eq(&self.current_user.get_id());

        let usr = self
            .store_users
            .iter()
            .find(|u| u.get_id() == item.user)
            .cloned()
            .unwrap_or_default();

        let layout = if is_message_from_myself {
            Layout::top_down(Align::Max)
        } else {
            Layout::top_down(Align::Min)
        };

        let msg_color = if is_message_from_myself {
            t.primary_soft
        } else {
            t.raised
        };

        ui.with_layout(layout, |ui| {
            ui.set_max_width(max_msg_width);
            let outer_margin = Margin {
                left: 1,
                right: 1,
                top: 4,
                bottom: 1,
            };
            let inner_margin = Margin {
                left: 0,
                right: 0,
                top: 4,
                bottom: 0,
            };
            let rnding = bubble_corners(t.radius_md, is_message_from_myself);

            let outer_f = &mut Frame::new()
                .corner_radius(rnding)
                .inner_margin(inner_margin)
                .outer_margin(outer_margin)
                .fill(msg_color)
                .show(ui, |ui| {
                    let mut frame = Frame::NONE
                        .corner_radius(rnding)
                        .inner_margin(inner_margin)
                        .outer_margin(Margin {
                            left: 0,
                            right: 0,
                            top: 4,
                            bottom: 1,
                        })
                        .fill(msg_color)
                        .begin(ui);

                    frame.frame.stroke = Stroke::new(1.0, t.card_border);

                    let ui = &mut frame.content_ui;
                    ui.set_width(max_msg_width);

                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        let id = item.id.clone();
                        let popup_key = format!("{:?}", item.id);
                        let editing = self.allow_edit.contains(&id.key_string());
                        let mut action = None;
                        ui.horizontal(|ui| {
                            ui.add_space(m.gap.x / 2.0);
                            if is_message_from_myself {
                                action = bubble_actions(
                                    ui,
                                    Some(DELETE_TIP),
                                    "Copy Task Note",
                                    Some(editing),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(m.gap.x / 2.0);
                                    let from_btn = sender_chip(
                                        ui,
                                        self.current_user.get_username(),
                                        &popup_key,
                                    );
                                    if from_btn.clicked() {
                                        self.open = !self.open;
                                    }
                                    popup_widget(&from_btn, item);
                                    sent_at(ui, &item.created_at);
                                });
                            } else {
                                let from_btn = sender_chip(ui, usr.get_username(), &popup_key);
                                if from_btn.clicked() {
                                    self.open = !self.open;
                                }
                                popup_widget(&from_btn, item);
                                sent_at(ui, &item.created_at);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(m.gap.x / 2.0);
                                    let admin = self.current_user.is_admin();
                                    action = ui
                                        .horizontal(|ui| {
                                            bubble_actions(
                                                ui,
                                                admin.then_some(ADMIN_DELETE_TIP),
                                                "Copy Task Note",
                                                admin.then_some(editing),
                                            )
                                        })
                                        .inner;
                                });
                            }
                        });
                        match action {
                            Some(BubbleAction::Delete) => {
                                let _ = self
                                    .chat_action_tx
                                    .try_send(ChatAction::DeleteMessage(item.id.clone()));
                            }
                            Some(BubbleAction::Copy) => {
                                if let ChatMessageType::Text(txt) = &item.content {
                                    ui.ctx().copy_text(txt.clone());
                                }
                            }
                            Some(BubbleAction::Edit) => {
                                let _ = self.chat_action_tx.try_send(ChatAction::Edit(id.clone()));
                            }
                            Some(BubbleAction::Save) => {
                                let _ = self
                                    .chat_action_tx
                                    .try_send(ChatAction::SaveNote(item.clone()));
                            }
                            Some(BubbleAction::Cancel) => {
                                let _ = self
                                    .chat_action_tx
                                    .try_send(ChatAction::CancelEdit(id.clone()));
                            }
                            None => {}
                        }

                        Frame::new()
                            .fill(theme::bg_surface(ui))
                            .stroke(Stroke::new(1.0, t.field_border))
                            .outer_margin(Margin {
                                top: 3,
                                ..Default::default()
                            })
                            .inner_margin(Margin::same(m.gap.x as i8))
                            .corner_radius(rnding)
                            .show(ui, |ui| {
                                ui.with_layout(
                                    Layout::from_main_dir_and_cross_align(
                                        Direction::TopDown,
                                        Align::Center,
                                    ),
                                    |ui| {
                                        ui.set_width(ui.available_width());
                                        if self.allow_edit.contains(&item.id.key_string()) {
                                            if let Some(msg) =
                                                self.edit_text.get_mut(&item.id.key_string())
                                                && let ChatMessageType::Text(note) =
                                                    &mut msg.content
                                            {
                                                TextArea::new(note).width(max_msg_width).show(ui);
                                            }
                                        } else {
                                            match &item.content {
                                                ChatMessageType::Text(msg) => easy_mark(ui, msg),
                                                ChatMessageType::Image((file_id, bytes)) => {
                                                    let shown = self.image_id.eq(file_id);
                                                    let icon = if shown {
                                                        svg::CARET_UP
                                                    } else {
                                                        svg::IMAGE
                                                    };
                                                    if controls::icon_text_button(ui, icon, file_id)
                                                        .clicked()
                                                    {
                                                        let _ = self.chat_action_tx.try_send(
                                                            ChatAction::OpenImage(file_id.clone()),
                                                        );
                                                    }

                                                    let image_source = ImageSource::Bytes {
                                                        uri: Cow::from(format!(
                                                            "bytes://{file_id}"
                                                        )),
                                                        bytes: eframe::egui::load::Bytes::Shared(
                                                            Arc::from(bytes.to_vec()),
                                                        ),
                                                    };

                                                    if shown {
                                                        Image::new(image_source)
                                                            .show_loading_spinner(true)
                                                            .fit_to_original_size(0.8)
                                                            .max_size(Vec2::new(800., 700.))
                                                            .ui(ui);
                                                    }
                                                }
                                            }
                                        }
                                    },
                                );
                            });
                    });

                    frame
                })
                .inner;

            let response = outer_f.allocate_space(ui);
            if response.hovered() {
                outer_f.frame.fill = t.hover;
                outer_f.frame.stroke = Stroke::new(1.0, t.primary_border);
                outer_f.frame.shadow = ui.visuals().window_shadow;
            }
            outer_f.paint(ui);
        });
    }
}

/// The sender's ids in a popup under `btn_response`.
pub fn popup_widget(btn_response: &Response, item: &UserMessage) {
    Popup::menu(btn_response)
        .width(btn_response.rect.width().min(SENDER_POPUP_W))
        .align(RectAlign::BOTTOM)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(SENDER_POPUP_W);
            detail_row(
                ui,
                svg::HASH,
                "ID",
                &format!("user_message:{}", item.id.key_string()),
            );
            detail_row(
                ui,
                svg::CHAT_CIRCLE_DOTS,
                "Task ID",
                &format!("chat_thread:{}", item.thread_id.key_string()),
            );
            detail_row(
                ui,
                svg::USER,
                "User ID",
                &format!("User:{}", item.user.key_string()),
            );
        });
}
