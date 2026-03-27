
use eframe::egui::{Align, Button, Color32, ComboBox, FontId, Id, Margin, Response, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use database::schema::{LiveTaskPayload, Priority, RecordIdExt, Status, User};
use crate::{Interaction, PlatformSpawner, Spawner, apply_jiff_date, to_jiff_date};
use chrono::{Datelike, NaiveDate, Utc};
use egui_extras::DatePickerButton;
use log::info;

use super::task_cards::date_colors;

impl Interaction for LiveTaskPayload {
    fn interact_service_number(&mut self, ui: &mut Ui) -> Response {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12, 12, 14);
        ui.style_mut().override_font_id = Some(FontId::proportional(12.0));
        let mut default = String::new();
        let service_number = self.service_number.as_mut().unwrap_or(&mut default);
        let text_edit = TextEdit::singleline(service_number)
            .desired_width(325.)
            .margin(Margin::symmetric(6, 3))
            .horizontal_align(Align::Min)
            .vertical_align(Align::Center)
            .ui(ui);

        if text_edit.lost_focus() {
            let svc = service_number.clone();
            let task = self.clone(); 
            PlatformSpawner::spawn(async move { 
                let update = task.update_service_number(svc.clone()).await;
                info!("Update: {update:?}"); 
            });
        }

        text_edit
    }

    fn interact_task_name(&mut self, ui: &mut Ui) -> Response {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12, 12, 14);
        ui.style_mut().override_font_id = Some(FontId::proportional(12.0));
        // ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(0.5, Color32::from_additive_luminance(110));
        let text_edit = TextEdit::singleline(&mut self.task_name)
            .desired_width(320.)
            .margin(Margin::symmetric(6, 3))
            .horizontal_align(Align::Min)
            .vertical_align(Align::Center)
            .ui(ui);

        if text_edit.lost_focus() {
            let task = self.clone(); 
            PlatformSpawner::spawn(async move { 
                let update = task.update_task_name(task.task_name.clone()).await;
                info!("Update: {update:?}"); 
            });
        }

        text_edit
    }

    fn interact_task_description(&mut self, ui: &mut Ui) -> Response {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12, 12, 14);
        // ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));

        let text_edit = TextEdit::multiline(&mut self.task_description)
            .desired_rows(6)
            .margin(Margin::symmetric(6, 3))
            .desired_width(445.)
            .horizontal_align(Align::Center)
            .ui(ui);

        if text_edit.lost_focus() {
            let task = self.clone(); 
            PlatformSpawner::spawn(async move { 
                let update = task.update_task_description().await;
                info!("Update: {update:?}"); 
            });
        }

        text_edit
    }

    fn interact_due_date(&mut self, ui: &mut Ui) -> Response {
        let frame_color = date_colors(ui, self.due_date.clone().into(), self.completed);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(0.5, frame_color);
        let id = self.id.key_string();
        let mut due_date = to_jiff_date(&self.due_date);
        let date_picker = DatePickerButton::new(&mut due_date)
            .format("%m/%d")
            .id_salt(id.as_str())
            .show_icon(false)
            .ui(ui);

        if date_picker.changed() {
            self.due_date = apply_jiff_date(&self.due_date, &due_date).into();

            let task = self.clone(); 
            info!("new date: {due_date:?}"); 
            PlatformSpawner::spawn(async move { 
                let update = task.update_due_date().await;
                info!("Update: {update:?}"); 
            });
            info!("date_widget changed: {:?}// {:?} ", self.task_name, due_date);
        }

        date_picker
    }

    fn interact_completed(&mut self, ui: &mut Ui) -> Response {
        if self.completed {
            let hover_txt = "✔";
            let color_complete = Color32::from_rgba_premultiplied(51, 255, 189, 200);
            let stroke = Stroke::new(0.7, color_complete);
            return Button::new(hover_txt)
                .stroke(stroke)
                .min_size(Vec2::new(25.0, 20.0))
                .ui(ui);
        } else {
            let hover_txt = "✖";
            let color_incomplete = Color32::from_rgba_premultiplied(255, 51, 153, 200);
            let stroke = Stroke::new(0.7, color_incomplete);
            return Button::new(hover_txt)
                .stroke(stroke)
                .min_size(Vec2::new(25.0, 20.0))
                .ui(ui);
        }
    }

    fn interact_status(&mut self, user: &User, ui: &mut Ui) -> Response {
        ComboBox::new(Id::new(&self.id.key_string()), "")
            .selected_text(RichText::new(format!("{}", &self.status.as_str())))
            .width(80.)
            .height(150.)
            .show_ui(ui, |ui| {
                let statuses = if self.assignee == user.get_id() {
                    user.get_statuses()
                } else {
                    Status::VALUES.to_vec().iter().filter(|s| !s.as_str().is_empty()).cloned().collect()
                };
                for status in statuses {
                    if ui.selectable_value(
                        &mut self.status, 
                        status.to_owned(), 
                        status.as_str()
                    ).clicked() {
                        let task = self.clone(); 
                        PlatformSpawner::spawn(async move { 
                            let update = task.update_status(status.clone()).await;
                            info!("Update: {update:?}"); 
                        });
                    }
                }
            })
            .response
    }

    fn interact_priority(&mut self, ui: &mut Ui) -> Response {
        ui.spacing_mut().combo_height = 300.;
        
        let is_web = cfg!(target_arch = "wasm32");
        if is_web {
            ui.ctx().options_mut(|o| o.input_options.line_scroll_speed = 20.0);
        } else {
            ui.ctx().options_mut(|o| o.input_options.line_scroll_speed = 50.0);
        };
        
        ComboBox::new(Id::new(&self.id.key_string()), "")
            .selected_text(RichText::new(format!("{}", &self.priority.as_str())))
            .width(80.)
            .height(150.)
            .show_ui(ui, |ui| {
                for priority in Priority::VALUES {
                    let priority_change = ui.selectable_value(
                        &mut self.priority,
                        priority.to_owned(),
                        priority.as_str(),
                    );
                    if priority_change.clicked() {
                        let task = self.clone(); 
                        PlatformSpawner::spawn(async move { 
                            let update = task.update_priority(Some(priority.clone())).await;
                            info!("Update: {update:?}"); 
                        });
                    }
                }
            })
            .response
    }

    fn interact_assignee(&mut self, ui: &mut Ui, store_users: &Vec<User>, current_user: &User) -> Response {
        // 1. Figure out what to show in the ComboBox when nothing is open:
        let current_name = store_users
            .iter()
            .filter(|u| u.is_active())
            .find(|u| u.get_id() == self.assignee)
            .map(|u| u.get_username().to_owned())
            .unwrap_or_else(|| "Unassigned".to_string());

        // 2) Build & sort the list
        let my_store = current_user.get_store();
        let mut sorted_users: Vec<&User> = store_users
            .iter()
            .filter(|u| u.is_active())
            .collect();

        sorted_users.sort_by_key(|u| {
            (
                // same‑store? (false=first, true=later)
                u.get_store() != my_store,
                // then by username (case‑insensitive)
                u.get_username().to_lowercase(),
            )
        });

        ComboBox::from_id_salt(Id::new(&self.id.key_string()))
            .selected_text(current_name)
            .width(100.)
            .height(150.)
            .show_ui(ui, |ui| {
                for user in sorted_users {
                    let assignee_selection = ui.selectable_value(
                    &mut self.assignee,       // current_value: &mut RecordId
                    user.get_id(),    // selected_value: RecordId
                    user.get_username(),      // text: &str or String
                );
                    if assignee_selection.clicked() {
                        let task = self.clone(); 
                        let new_assignee = user.get_id().clone();
                        PlatformSpawner::spawn(async move { 
                            let update = task.update_assignee(new_assignee).await;
                            info!("Update: {update:?}"); 
                        });
                    }
                }
            })
            .response
    }
}
