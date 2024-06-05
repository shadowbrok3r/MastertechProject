use std::iter::repeat;
use std::sync::Arc;
use std::time::Duration;
use std::usize;
use database::schema::{TaskNotePayload, User};
use eframe::emath::Vec2;
use egui::{
    Align, Frame, Label, Layout, Rect, RichText, Rounding, ScrollArea, Shape, Stroke, TextEdit, Ui, Widget
};
use egui_animation::animate_continuous;
use egui_inbox::UiInbox;
use egui_infinite_scroll::InfiniteScroll;
use log::info;
use wasm_bindgen_futures::spawn_local;

use crate::utilities::ModalTypes;

use super::modals::ModalState;

#[derive(Debug, Default)]
pub struct HistoryLoader {
    pub history: Vec<ChatMessage>,
    pub messages: Vec<ChatMessage>,
}

impl HistoryLoader {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: &User) -> Self {
        let history: Vec<_> = messages.clone()
            .into_iter()
            .map(|chat_message|
                ChatMessage {
                    note: chat_message.note,
                    from: if chat_message.everest_initials == current_user.everest_initials {
                        None
                    } else {
                        Some(chat_message.everest_initials)
                    },
                })
            .rev()
            .collect();

        // Repeat the history 5 times to make it longer.
        let history = repeat(history)
            .take(5)
            .flat_map(|history| history.clone())
            .collect();

            let messages = messages
            .into_iter()
            .map(|chat_message| 
                ChatMessage {
                    note: chat_message.note,
                    from: if chat_message.everest_initials == current_user.everest_initials {
                        None
                    } else {
                        Some(chat_message.everest_initials)
                    },
                }
            )
            .collect();
        Self {
            history,
            messages
        }
    }

    pub async fn load(&self, page: Option<usize>) -> (Vec<ChatMessage>, Option<usize>) {
        let page = page.unwrap_or(0);
        let page_size = 10;
        let start = page * page_size;
        let end = usize::min(start + page_size, self.history.len());

        let has_more = end < self.history.len();

        let messages = self.history[start..end].iter().cloned().rev().collect();

        (messages, if has_more { Some(page + 1) } else { None })
    }
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub note: String,
    pub from: Option<String>,
}

#[derive(Default, Debug)]
pub struct ChatModal{
    pub state: ModalState,
    pub title: String,
    pub messages: InfiniteScroll<ChatMessage, usize>,
    pub inbox: UiInbox<ChatMessage>,
    pub history_loader: Arc<HistoryLoader>,
    pub shown: bool,
    pub msgs_received: usize,  
}

impl ModalTypes for ChatModal{
    fn modal_state(&mut self) -> &mut ModalState {
        &mut self.state
    }
    fn title(mut self, title: String) -> Self {
        self.modal_state().title = Some(title);
        self
    }
}

impl ChatModal {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: &User) -> Self {
        let history_loader = Arc::new(HistoryLoader::new(messages, current_user));

        let inbox = UiInbox::new();

        let history_loader_clone = history_loader.clone();

        ChatModal {
            messages: InfiniteScroll::new().start_loader(move |cursor, cb| {
                info!("Loading messages...");
                let history_loader = history_loader_clone.clone();
                spawn_local(async move {
                    let (messages, cursor) = history_loader.load(cursor).await;
                    cb(Ok((messages, cursor)));
                });
            }),
            inbox,
            history_loader,
            shown: false,
            msgs_received: 0,
            state: ModalState::default(),
            title: "Chats".to_string()
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        
        if !self.shown {
            self.shown = true;

            let tx = self.inbox.sender();
            self.history_loader
                .messages
                .iter()
                .for_each(|message| {
                    let tx = tx.clone();
                    let message = message.clone();
                    tx.send(message).ok();
                });
        }

        self.inbox.read(ui).for_each(|message| {
            info!("In chat modal: {:?}", message);
            self.messages.items.push(message);
            self.msgs_received += 1;
        });

        ScrollArea::vertical()
            .animated(false)
            .max_height(400.0)
            // .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                
                ui.set_width(ui.available_width());

                ui.vertical_centered(|ui| {
                    ui.set_visible(self.messages.top_loading_state().loading());
                    ui.spinner();
                });

                let max_msg_width = ui.available_width() - 40.0;
                let inner_margin = 8.0;
                let outer_margin = 8.0;

                self.messages.ui(ui, 5, |ui, _index, item| {
                    let is_message_from_myself = item.from.is_none();

                    // Messages from the user are right-aligned.
                    let layout = if is_message_from_myself {
                        Layout::top_down(Align::Max)
                    } else {
                        Layout::top_down(Align::Min)
                    };

                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(max_msg_width);

                        let mut measure = |text| {
                            let label = Label::new(text);
                            // We need to calculate the text width here to enable the typical
                            // chat bubble layout where the own bubbles are right-aligned and
                            // the text within is left-aligned.
                            let (_pos, galley, _response) = label
                                .layout_in_ui(&mut ui.child_ui(ui.max_rect(), *ui.layout()));
                            let rect = galley.rect;
                            // Calculate the width of the frame based on the width of
                            // the text and add 0.1 to account for floating point errors.
                            f32::min(
                                rect.width() + inner_margin * 2.0 + outer_margin * 2.0 + 0.1,
                                max_msg_width,
                            )
                        };

                        let content = RichText::new(&item.note);
                        let mut msg_width = measure(content.clone());
                        let name = if let Some(from) = &item.from {
                            let name = RichText::new(from).strong();
                            let width = measure(name.clone());
                            msg_width = f32::max(msg_width, width);
                            Some(name)
                        } else {
                            None
                        };

                        // Set the width of the ui to the width of the message.
                        ui.set_min_width(msg_width);

                        let msg_color = if is_message_from_myself {
                            ui.style().visuals.widgets.inactive.bg_fill
                        } else {
                            ui.style().visuals.extreme_bg_color
                        };

                        let rounding = 8.0;
                        let margin = 8.0;
                        let response = Frame::none()
                            .rounding(Rounding {
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
                            })
                            .inner_margin(margin)
                            .outer_margin(margin)
                            .fill(msg_color)
                            .show(ui, |ui| {
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    if let Some(from) = name {
                                        Label::new(from).ui(ui);
                                    }

                                    ui.label(&item.note);
                                });
                            })
                            .response;

                        let points = if !is_message_from_myself {
                            let top = response.rect.left_top() + Vec2::splat(margin);
                            let arrow_rect =
                                Rect::from_two_pos(top, top + Vec2::new(-rounding, rounding));

                            vec![
                                arrow_rect.left_top(),
                                arrow_rect.right_top(),
                                arrow_rect.right_bottom(),
                            ]
                        } else {
                            let top = response.rect.right_top() + Vec2::new(-margin, margin);
                            let arrow_rect =
                                Rect::from_two_pos(top, top + Vec2::new(rounding, rounding));

                            vec![
                                arrow_rect.left_top(),
                                arrow_rect.right_top(),
                                arrow_rect.left_bottom(),
                            ]
                        };

                        ui.painter()
                            .add(Shape::convex_polygon(points, msg_color, Stroke::NONE));

                        let _text_edit = TextEdit::multiline(&mut "Enter a message")
                        .desired_rows(5)
                        .desired_width(ui.available_width())
                        .horizontal_align(egui::Align::Center)
                        .ui(ui);
                    });
                });

                if self.msgs_received < self.history_loader.messages.len()
                    && !self.messages.initial_loading()
                {
                    Frame::none()
                        .rounding(8.0)
                        .inner_margin(8.0)
                        .outer_margin(8.0)
                        .fill(ui.style().visuals.faint_bg_color)
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                let mut dot = |offset| {
                                    let t = animate_continuous(
                                        ui,
                                        egui_animation::easing::sine_in_out,
                                        Duration::from_secs_f32(1.0),
                                        offset,
                                    );

                                    let res = ui.allocate_response(
                                        Vec2::splat(4.0),
                                        egui::Sense::hover(),
                                    );

                                    ui.painter().circle_filled(
                                        res.rect.center() + Vec2::Y * t * 4.0,
                                        res.rect.width() / 2.0,
                                        ui.style().visuals.text_color(),
                                    )
                                };

                                dot(0.0);
                                dot(0.3);
                                dot(8.6);
                            });
                        });
                }
            });

        ui.add_space(8.0);
    }
}
