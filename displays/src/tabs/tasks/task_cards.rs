use eframe::egui::{Button, CollapsingHeader, Widget, Vec2, Color32, Frame, Margin, RichText, Ui};
use database::schema::{LiveTaskPayload, RecordIdExt, TaskNotePayload, User};
use crossbeam::channel::Sender;
use chrono::{DateTime, Utc};
use log::info;

use crate::{Displayable, Interaction, PlatformSpawner, Spawner, TaskUiActions};

impl Displayable for LiveTaskPayload {
    fn display_cards(
        &mut self, 
        ui: &mut Ui, 
        user: &User, 
        store_users: &Vec<User>, 
        notes: Vec<TaskNotePayload>, 
        tx: Sender<TaskUiActions>
    ) {
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
                if !notes.is_empty() {
                    count = notes.len();
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
                            notes.clone(),
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
                    PlatformSpawner::spawn(async move {
                        let update = task.update_completed(!task.completed).await;
                        info!("update_completed: {update:?}");
                    });
                }
            });

            ui.separator();

            ui.horizontal(|ui: &mut Ui| {
                ui.push_id(format!("Assignee {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_assignee(ui, store_users, user);
                });

                ui.add_space(22.);
                
                ui.push_id(format!("Priority {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_priority(ui);
                });

                ui.add_space(22.);

                ui.push_id(format!("Status {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_status(user, ui);
                });

                ui.add_space(22.);

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

pub fn date_colors(ui: &mut Ui, due_date: DateTime<Utc>, _complete: bool) -> Color32 {
    let current_date = Utc::now().date_naive();
    let due_date_naive = due_date.date_naive();
    // 3 days in seconds
    let three_days_secs = 3 * 24 * 60 * 60;
    
    let current_secs = current_date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    let due_secs = due_date_naive.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();

    if due_secs < current_secs {
        // Overdue - red
        ui.style().visuals.error_fg_color
    } else if due_secs <= current_secs + three_days_secs {
        // Today to 3 days from now - warning color (orange/yellow)
        Color32::from_rgb(217, 255, 0)
    } else {
        // Beyond 3 days - green
        Color32::from_rgb(11,244,192)
    }
}
