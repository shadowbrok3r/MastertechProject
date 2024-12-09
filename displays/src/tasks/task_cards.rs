use chrono::{DateTime, Utc};
use crossbeam::channel::Sender;
use database::schema::{TaskPayload, User};
use eframe::egui::Vec2;
use eframe::egui::{Align, Button, CollapsingHeader, Direction, Widget};
use eframe::egui::{Color32, Frame, Layout, Margin, Rounding};
use eframe::egui::{RichText, Ui};
use egui_extras::{Size, StripBuilder};
use log::info;

use crate::{Displayable, Interaction, TaskUiActions, Updatable};

impl Displayable for TaskPayload {
    fn display_cards(&mut self, ui: &mut Ui, store_users: &Vec<User>, tx: Sender<TaskUiActions>) {
        let style = ui.style().clone();

        Frame::default()
            .fill(style.visuals.extreme_bg_color) // (Color32::from_rgb(14, 14, 18))
            .inner_margin(Margin::same(8.0))
            .outer_margin(Margin::same(5.0))
            .rounding(Rounding::same(15.0))
            .stroke(style.visuals.window_stroke)
            .show(ui, |ui| {
                ui.set_max_height(300.0);
                ui.set_min_height(67.0);
                ui.set_width(400.0);

                StripBuilder::new(ui)
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .size(Size::exact(15.0)) // Task Header
                    .size(Size::exact(4.0))
                    .size(Size::exact(15.0)) // Task Footer
                    .size(Size::exact(4.0))
                    .size(Size::initial(20.0).at_most(200.0)) // Task Body
                    .vertical(|mut strip| {
                        strip.strip(|strip| {
                            strip
                                .cell_layout(Layout::left_to_right(Align::Min))
                                .cell_layout(Layout::left_to_right(Align::Center))
                                .cell_layout(Layout::left_to_right(Align::Max))
                                .size(Size::remainder())
                                .size(Size::relative(0.1))
                                .size(Size::relative(0.1))
                                .horizontal(|mut s| {
                                    s.cell(|ui| {
                                        ui.with_layout(
                                            Layout::centered_and_justified(Direction::TopDown),
                                            |ui| {
                                                let response = self.interact_task_name(ui);
                                                if response.has_focus() && response.changed() {
                                                    info!("task_name changed");
                                                    let _ = tx.try_send(TaskUiActions::Editing(
                                                        self.id.clone(),
                                                    ));
                                                } else if response.lost_focus() {
                                                    info!("task_name lost_focus");
                                                    let _ =
                                                        tx.try_send(TaskUiActions::CommitChanges(
                                                            self.id.clone(),
                                                        ));
                                                }
                                            },
                                        );
                                    });
                                    s.cell(|ui| {
                                        ui.with_layout(
                                            Layout::centered_and_justified(Direction::TopDown),
                                            |ui| {
                                                let button = Button::new("⮫")
                                                    .small()
                                                    .min_size(Vec2::new(25.0, 20.0))
                                                    .ui(ui);
                                                    
                                                if button.clicked() {
                                                    let _ = tx.try_send(TaskUiActions::OpenTaskModal(self.to_owned()));
                                                }
                                                if button.secondary_clicked() {
                                                    info!("Secondary clicked, opening viewport");
                                                    let _ = tx.try_send(TaskUiActions::OpenViewport(self.to_owned()));
                                                }
                                            },
                                        );
                                    });
                                    s.cell(|ui| {
                                        ui.with_layout(
                                            Layout::centered_and_justified(Direction::TopDown),
                                            |ui| {
                                                let response = self.interact_completed(ui);
                                                if response.has_focus()
                                                    || response.changed()
                                                    || response.clicked()
                                                {
                                                    info!("Marked Task Complete / Incomplete ");
                                                    if self.completed {
                                                        self.update_completed(false);
                                                    } else {
                                                        self.update_completed(true);
                                                    }
                                                    let _ =
                                                        tx.try_send(TaskUiActions::CommitChanges(
                                                            self.id.clone(),
                                                        ));
                                                }
                                            },
                                        );
                                    });
                                });
                        });

                        strip.empty();

                        strip.strip(|strip| {
                            strip
                                .cell_layout(Layout::left_to_right(Align::Center))
                                .cell_layout(Layout::left_to_right(Align::Center))
                                .cell_layout(Layout::left_to_right(Align::Center))
                                .cell_layout(Layout::left_to_right(Align::Max))
                                .cell_layout(Layout::left_to_right(Align::Max))
                                .size(Size::exact(70.))
                                .size(Size::exact(90.))
                                .size(Size::exact(90.))
                                .size(Size::exact(80.))
                                .size(Size::exact(50.))
                                .horizontal(|mut s| {
                                    s.cell(|ui| {
                                        let response =
                                            self.interact_assignee_initials(ui, store_users);
                                        if response.secondary_clicked() {
                                            let _ = tx.try_send(TaskUiActions::OpenTaskModal(
                                                self.to_owned(),
                                            ));
                                        }
                                        if response.has_focus()
                                            || response.changed()
                                            || response.clicked()
                                        {
                                            info!("assignee initials changed");
                                            let _ = tx.try_send(TaskUiActions::Editing(
                                                self.id.clone(),
                                            ));
                                        } else if response.lost_focus() {
                                            info!("assignee initials lost_focus");
                                            let _ = tx.try_send(TaskUiActions::CommitChanges(
                                                self.id.clone(),
                                            ));
                                        }
                                    });
                                    s.cell(|ui| {
                                        let response = self.interact_priority(ui);
                                        if response.has_focus()
                                            || response.changed()
                                            || response.clicked()
                                        {
                                            info!("interact_priority changed");
                                            let _ = tx.try_send(TaskUiActions::Editing(
                                                self.id.clone(),
                                            ));
                                        } else if response.lost_focus() {
                                            info!("interact_priority lost focus");
                                            let _ = tx.try_send(TaskUiActions::CommitChanges(
                                                self.id.clone(),
                                            ));
                                            // let _ = tx.try_send(Some(TaskUiActions::CommitChanges(self.id.clone()))
                                        }
                                    });
                                    s.cell(|ui| {
                                        let response = self.interact_status(ui);

                                        if response.changed() {
                                            info!("interact_status changed");
                                            let _ = tx.try_send(TaskUiActions::Editing(
                                                self.id.clone(),
                                            ));
                                        } else if response.lost_focus() {
                                            info!("interact_status lost focus");
                                            let _ = tx.try_send(TaskUiActions::CommitChanges(
                                                self.id.clone(),
                                            ));
                                            // let _ = tx.try_send(Some(TaskUiActions::CommitChanges(self.id.clone()))
                                        }
                                    });

                                    s.cell(|ui| {
                                        ui.with_layout(
                                            Layout::centered_and_justified(Direction::TopDown),
                                            |ui| {
                                                let response = self.interact_due_date(ui);
                                                if response.changed() {
                                                    info!("interact_due_date changed");
                                                    let _ = tx.try_send(TaskUiActions::Editing(
                                                        self.id.clone(),
                                                    ));
                                                }
                                                if response.lost_focus() {
                                                    info!("interact_due_date lost focus");
                                                    let _ =
                                                        tx.try_send(TaskUiActions::CommitChanges(
                                                            self.id.clone(),
                                                        ));
                                                    // let _ = tx.try_send(Some(TaskUiActions::CommitChanges(self.id.clone()))
                                                }
                                            },
                                        );
                                    });
                                    s.cell(|ui| {
                                        ui.with_layout(
                                            Layout::centered_and_justified(Direction::TopDown),
                                            |ui| {
                                                let mut count = 0;
                                                if !self.task_note.is_empty() {
                                                    count = self.task_note.len();
                                                }
                                                ui.style_mut().spacing.button_padding.x = 6.0;
                                                ui.style_mut().spacing.button_padding.y = 6.0;
                                                let txt = if count > 0 {
                                                    RichText::new(format!("{} 💬", count))
                                                        .color(style.visuals.window_stroke.color)
                                                } else {
                                                    RichText::new("💬").color(Color32::WHITE)
                                                };
                                                if Button::new(txt)
                                                    .small()
                                                    .min_size(Vec2::new(25.0, 20.0))
                                                    .ui(ui)
                                                    .clicked()
                                                {
                                                    let _ = tx.try_send(
                                                        TaskUiActions::OpenChatModal((
                                                            self.id.clone(),
                                                            self.task_note.clone(),
                                                        )),
                                                    );
                                                }
                                            },
                                        );
                                    });
                                });
                        });
                        strip.empty();
                        strip.strip(|strip| {
                            strip
                                .cell_layout(Layout::top_down(Align::Center))
                                .size(Size::remainder())
                                .horizontal(|mut s| {
                                    s.cell(|ui| {
                                        let task_descrip_header = ui.make_persistent_id(format!(
                                            "task_description {:?}",
                                            self.id.clone()
                                        ));
                                        let task_descrip_head =
                                            CollapsingHeader::new("Task Description")
                                                .id_salt(task_descrip_header);
                                        task_descrip_head.show_unindented(ui, |ui| {
                                            let response = self.interact_task_description(ui);
                                            if response.changed() {
                                                let _ = tx.try_send(TaskUiActions::Editing(
                                                    self.id.clone(),
                                                ));
                                            }
                                            if response.lost_focus() {
                                                let _ = tx.try_send(TaskUiActions::CommitChanges(
                                                    self.id.clone(),
                                                ));
                                            }
                                        });
                                    });
                                });
                        });
                    });
            });
    }
}

pub fn date_colors(date: String, _complete: bool) -> Color32 {
    let due_date = DateTime::parse_from_rfc3339(&date)
        .expect("Invalid date format")
        .with_timezone(&Utc);

    let current_date = Utc::now().date_naive();
    let mut overdue: Option<String> = None;
    let mut due_today: Option<String> = None;
    let mut due_tomorrow: Option<String> = None;
    if due_date.date_naive() == current_date.pred_opt().unwrap() {
        overdue = Some(date.clone());
    } else if due_date.date_naive() == current_date {
        due_today = Some(date.clone());
    } else if due_date.date_naive() == current_date.succ_opt().unwrap() {
        due_tomorrow = Some(date.clone());
    }
    if let Some(_) = overdue {
        Color32::from_rgb(199, 30, 60)
    }
    // Pink
    else if let Some(_) = due_today {
        Color32::from_rgb(240, 200, 108)
    }
    // Orange
    else if let Some(_) = due_tomorrow {
        Color32::from_rgb(79, 232, 125)
    }
    // Green
    else {
        Color32::from_rgb(199, 48, 103)
    } // Pink
}
