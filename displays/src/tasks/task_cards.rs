use eframe::egui::{Button, CollapsingHeader, Widget, Vec2, Color32, Frame, Margin, RichText, Ui};
use database::schema::{TaskPayload, User};
use crossbeam::channel::Sender;
use chrono::{DateTime, Utc};
use log::info;

use crate::{Displayable, Interaction, PlatformSpawner, Spawner, TaskUiActions, Updatable};

impl Displayable for TaskPayload {
    fn display_cards(&mut self, ui: &mut Ui, store_users: &Vec<User>, tx: Sender<TaskUiActions>) {
        let style = ui.style().clone();
        
        let mut frame = Frame::default()
            .fill(style.visuals.extreme_bg_color) // (Color32::from_rgb(14, 14, 18))
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(5))
            .corner_radius(eframe::egui::CornerRadius::same(15))
            .begin(ui);

        {
            let ui = &mut frame.content_ui;
            let available = ui.available_width();
            const CARD_MARGIN: f32 = 8.0; // match the Frame::inner_margin you’re using
            ui.set_width(available - CARD_MARGIN * 2.0);
            
            
            // ui.set_max_height(300.);
            ui.set_min_height(67.0);

            ui.horizontal(|ui| {
                let _ = self.interact_task_name(ui);

                ui.style_mut().spacing.button_padding.x = 6.0;
                ui.style_mut().spacing.button_padding.y = 4.0;

                let mut count = 0;
                if !self.task_note.is_empty() {
                    count = self.task_note.len();
                }

                let txt = if count > 0 {
                    RichText::new(format!("{} 💬", count)).color(style.visuals.warn_fg_color)
                } else {
                    RichText::new("  💬").color(Color32::WHITE)
                };

                if Button::new(txt)
                    .min_size(Vec2::new(25.0, 20.0))
                    .ui(ui)
                    .on_disabled_hover_text("Open Task Notes")
                    .clicked()
                {
                    let _ = tx.try_send(
                        TaskUiActions::OpenChatModal((
                            self.id.clone(),
                            self.task_note.clone(),
                            self.service_number.clone()
                        )),
                    );
                }

                let button = Button::new("⮫")
                    .min_size(Vec2::new(25.0, 20.0))
                    .ui(ui)
                    .on_hover_text("Open Task Modal");
                    
                if button.clicked() {
                    let _ = tx.try_send(TaskUiActions::OpenTaskModal(self.to_owned()));
                }
                if button.secondary_clicked() {
                    info!("Secondary clicked, opening viewport");
                    let _ = tx.try_send(TaskUiActions::OpenViewport(self.to_owned()));
                }

                let complete_response = self.interact_completed(ui);
                if complete_response.has_focus()
                    || complete_response.changed()
                    || complete_response.clicked()
                {
                    info!("Marked Task Complete / Incomplete ");
                    let task = self.clone();
                    if task.completed {
                        PlatformSpawner::spawn(async move {
                            let update = task.update_completed(false).await;
                            info!("update_completed: {update:?}");
                        });
                    } else {
                        PlatformSpawner::spawn(async move {
                            let update = task.update_completed(true).await;
                            info!("update_completed: {update:?}");
                        });
                    }
                }
            });

            ui.separator();

            ui.horizontal(|ui: &mut Ui| {
                ui.push_id(format!("Assignee {}", self.id.key().to_string().clone()), |ui| {
                    let _ = self.interact_assignee_initials(ui, store_users);
                });

                ui.add_space(50.);
                
                ui.push_id(format!("Priority {}", self.id.key().to_string().clone()), |ui| {
                    let _ = self.interact_priority(ui);
                });

                ui.add_space(50.);

                ui.push_id(format!("Status {}", self.id.key().to_string().clone()), |ui| {
                    let _ = self.interact_status(ui);
                });

                ui.add_space(50.);

                let _ = self.interact_due_date(ui);
            });

            ui.separator();

            ui.horizontal(|ui| {
                let task_descrip_header = ui.make_persistent_id(format!("task_description {:?}",self.id.clone()));
                let task_descrip_head = CollapsingHeader::new("Task Description").id_salt(task_descrip_header);
                task_descrip_head.show_unindented(ui, |ui| {
                    let _ = self.interact_task_description(ui);
                });
            });
            
        }


        let response = frame.allocate_space(ui);
        if response.hovered() {
            frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
            frame.frame.shadow = style.visuals.window_shadow;
        } else {
            frame.frame.stroke = style.visuals.widgets.open.bg_stroke;
        }
        frame.paint(ui);
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
