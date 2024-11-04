use crate::{
    app_state::MtechServerContext,
    utilities::ai::tool_call::{
        assistant_call_with_response_ai_tools, call_with_response, call_with_response_ai_tools,
    },
};
use async_openai_wasm::types::ChatChoice;
use crossbeam::channel::{Receiver, Sender};
use displays::markdown_editor::viewer;
use eframe::egui::{
    epaint::Shadow, Align, Button, Color32, Direction, Frame, Key, Layout, Margin, Rect, RichText,
    Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, Ui, Vec2, Widget,
};
use log::error;
use wasm_bindgen_futures::spawn_local;

pub struct AiPlayground {
    pub input: String,
    pub history: Vec<Message>,
    pub command_tx: Sender<Vec<ChatChoice>>,
    pub command_rx: Receiver<Vec<ChatChoice>>,
}

struct Message {
    note: String,
    sender: String,
}

impl Default for AiPlayground {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded::<Vec<ChatChoice>>();
        Self {
            input: String::new(),
            history: Vec::new(),
            command_tx: tx,
            command_rx: rx,
        }
    }
}

impl MtechServerContext {
    pub fn ai_playground(&mut self, ui: &mut Ui) {
        self.ai_playground.display(ui);
    }
}

impl AiPlayground {
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

                        for item in self.history.iter_mut() {
                            let is_message_from_myself =
                                if item.sender.eq("You") { true } else { false };

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

                                            let from = RichText::new(&item.sender)
                                                .strong()
                                                .monospace()
                                                .color(Color32::LIGHT_BLUE);

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
                                                            ui.ctx().copy_text(item.note.clone());
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
                                                            ui.ctx().copy_text(item.note.clone());
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
                                                        viewer::easy_mark(ui, &item.note);
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
                    });
            },
        );

        ui.vertical_centered_justified(|ui| {
            let text_edit = TextEdit::singleline(&mut self.input)
                .hint_text("Ask GPT to summarize a service order")
                .ui(ui);
            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            if text_edit.lost_focus() && key_press {
                text_edit.request_focus();
                let input = self.input.clone();
                let tx = self.command_tx.clone();
                self.history.push(Message {
                    note: input.clone(),
                    sender: "You".to_string(),
                });
                spawn_local(async move {
                    let res = assistant_call_with_response_ai_tools(input.as_str()).await;

                    match res {
                        Ok(chat_choices) => tx.try_send(chat_choices).unwrap(),
                        Err(e) => error!("Error sending {e:?}"),
                    }
                });
            }
        });

        if let Ok(chat_choices) = self.command_rx.try_recv() {
            for x in chat_choices {
                self.history.push(Message {
                    note: x.message.content.unwrap_or_default(),
                    sender: "GPT".to_string(),
                });
            }
        }
    }
}
