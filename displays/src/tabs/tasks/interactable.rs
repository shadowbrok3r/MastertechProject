use crate::ui_data::pending_task_edits::stage_edit;
use crate::{Interaction, PlatformSpawner, Spawner, apply_jiff_date, to_jiff_date};
use database::schema::{
    LiveTaskPayload, Priority, RecordIdExt, Status, TaskField, User, assignable_users,
};
use eframe::egui::{
    Align2, Color32, ComboBox, FontId, Id, Margin, Response, RichText, Stroke, TextEdit, Ui, Widget,
};
use egui_extras::DatePickerButton;
use log::info;

use super::task_cards::date_colors;

impl Interaction for LiveTaskPayload {
    fn interact_service_number(&mut self, ui: &mut Ui) -> Response {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12, 12, 14);
        ui.style_mut().override_font_id = Some(FontId::monospace(12.0));
        let mut default = String::new();
        let service_number = self.service_number.as_mut().unwrap_or(&mut default);
        let text_edit = TextEdit::singleline(service_number)
            .desired_width(325.)
            .margin(Margin::symmetric(6, 3))
            .align(Align2::LEFT_CENTER)
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
        task_name_edit(self, ui, 320.0)
    }

    fn interact_task_description(&mut self, ui: &mut Ui) -> Response {
        task_description_edit(self, ui, 445.0)
    }

    fn interact_due_date(&mut self, ui: &mut Ui) -> Response {
        let frame_color = date_colors(ui, self.due_date.clone().into(), self.completed);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(0.5_f32, frame_color);
        let id = self.id.key_string();
        let mut due_date = to_jiff_date(&self.due_date);
        let date_picker = DatePickerButton::new(&mut due_date)
            .format("%m/%d")
            .id_salt(id.as_str())
            .show_icon(false)
            .ui(ui);

        if date_picker.changed() {
            let mut edited = self.clone();
            edited.due_date = apply_jiff_date(&self.due_date, &due_date).into();
            stage_edit(self, &edited, TaskField::DueDate);
            self.due_date = edited.due_date;
        }

        date_picker
    }

    fn interact_completed(&mut self, ui: &mut Ui) -> Response {
        let (response, requested) = crate::tabs::tasks::complete_button::complete_button(ui, self);
        if let Some(completed) = requested {
            let mut edited = self.clone();
            edited.completed = completed;
            edited.status = if completed {
                Status::Complete
            } else {
                Status::InRepair
            };
            stage_edit(self, &edited, TaskField::Completed);
        }
        response
    }

    fn interact_status(&mut self, user: &User, ui: &mut Ui) -> Response {
        status_combo(self, user, ui, 80.0)
    }

    fn interact_priority(&mut self, ui: &mut Ui) -> Response {
        priority_combo(self, ui, 80.0)
    }

    fn interact_assignee(
        &mut self,
        ui: &mut Ui,
        store_users: &Vec<User>,
        current_user: &User,
    ) -> Response {
        assignee_combo(self, ui, store_users, current_user, 100.0)
    }
}

/// Task-name editor at most `width` wide.
pub(crate) fn task_name_edit(task: &mut LiveTaskPayload, ui: &mut Ui, width: f32) -> Response {
    ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12, 12, 14);
    ui.style_mut().override_font_id = Some(FontId::monospace(12.0));

    // Focused edits type into a buffer so per-frame live refreshes can't wipe keystrokes.
    let buf_id = Id::unique(("task_name_edit", task.id.key_string()));
    let buffered: Option<String> = ui.data_mut(|d| d.get_temp(buf_id));
    let had_buffer = buffered.is_some();
    let mut text = buffered.unwrap_or_else(|| task.task_name.clone());

    let text_edit = TextEdit::singleline(&mut text)
        .desired_width(width)
        .margin(Margin::symmetric(6, 3))
        .align(Align2::LEFT_CENTER)
        .ui(ui);

    if text_edit.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf_id, text.clone()));
    } else if text_edit.lost_focus() || had_buffer {
        ui.data_mut(|d| d.remove_temp::<String>(buf_id));
        // Save only when the edit actually changed the value.
        if text != task.task_name {
            task.task_name = text;
            let task = task.clone();
            PlatformSpawner::spawn(async move {
                let update = task.update_task_name(task.task_name.clone()).await;
                info!("Update: {update:?}");
            });
        }
    }

    text_edit
}

/// Task-description editor at most `width` wide.
pub(crate) fn task_description_edit(
    task: &mut LiveTaskPayload,
    ui: &mut Ui,
    width: f32,
) -> Response {
    ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12, 12, 14);

    // Focused edits type into a buffer so per-frame live refreshes can't wipe keystrokes.
    let buf_id = Id::unique(("task_description_edit", task.id.key_string()));
    let buffered: Option<String> = ui.data_mut(|d| d.get_temp(buf_id));
    let had_buffer = buffered.is_some();
    let mut text = buffered.unwrap_or_else(|| task.task_description.clone());

    let text_edit = TextEdit::multiline(&mut text)
        .desired_rows(6)
        .margin(Margin::symmetric(6, 3))
        .desired_width(width)
        .align(Align2::CENTER_TOP)
        .ui(ui);

    if text_edit.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf_id, text.clone()));
    } else if text_edit.lost_focus() || had_buffer {
        ui.data_mut(|d| d.remove_temp::<String>(buf_id));
        // Save only when the edit actually changed the value.
        if text != task.task_description {
            task.task_description = text;
            let task = task.clone();
            PlatformSpawner::spawn(async move {
                let update = task.update_task_description().await;
                info!("Update: {update:?}");
            });
        }
    }

    text_edit
}

/// Status combo at least `width` wide.
pub(crate) fn status_combo(
    task: &mut LiveTaskPayload,
    user: &User,
    ui: &mut Ui,
    width: f32,
) -> Response {
    ComboBox::from_id_salt(Id::unique(task.id.key_string()))
        .selected_text(RichText::new(task.status.as_str()))
        .width(width)
        .height(150.)
        .show_ui(ui, |ui| {
            let statuses = if task.assignee == user.get_id() {
                user.get_statuses()
            } else {
                Status::VALUES
                    .to_vec()
                    .iter()
                    .filter(|s| !s.as_str().is_empty())
                    .cloned()
                    .collect()
            };
            for status in statuses {
                let previous = task.status.clone();
                if ui
                    .selectable_value(&mut task.status, status.to_owned(), status.as_str())
                    .clicked()
                {
                    let mut edited = task.clone();
                    edited.status = status.clone();
                    // Only Todo/In Repair/Complete imply a completion
                    // state; Sales, QC and custom statuses leave it alone.
                    let implied = match status {
                        Status::Complete => Some(true),
                        Status::Todo | Status::InRepair => Some(false),
                        _ => None,
                    };
                    task.status = previous;
                    stage_edit(task, &edited, TaskField::Status);
                    if let Some(completed) = implied
                        && completed != task.completed
                    {
                        edited.completed = completed;
                        stage_edit(task, &edited, TaskField::Completed);
                    }
                    task.status = edited.status;
                }
            }
        })
        .response
}

/// Priority combo at least `width` wide.
pub(crate) fn priority_combo(task: &mut LiveTaskPayload, ui: &mut Ui, width: f32) -> Response {
    ui.spacing_mut().combo_height = 300.;

    let is_web = cfg!(target_arch = "wasm32");
    if is_web {
        ui.ctx()
            .options_mut(|o| o.input_options.line_scroll_speed = 20.0);
    } else {
        ui.ctx()
            .options_mut(|o| o.input_options.line_scroll_speed = 50.0);
    };

    ComboBox::from_id_salt(Id::unique(task.id.key_string()))
        .selected_text(RichText::new(task.priority.as_str()))
        .width(width)
        .height(150.)
        .show_ui(ui, |ui| {
            for priority in Priority::VALUES {
                let previous = task.priority.clone();
                let priority_change =
                    ui.selectable_value(&mut task.priority, priority.to_owned(), priority.as_str());
                if priority_change.clicked() {
                    let mut edited = task.clone();
                    edited.priority = priority.clone();
                    task.priority = previous;
                    stage_edit(task, &edited, TaskField::Priority);
                    task.priority = edited.priority;
                }
            }
        })
        .response
}

/// Assignee combo at least `width` wide.
pub(crate) fn assignee_combo(
    task: &mut LiveTaskPayload,
    ui: &mut Ui,
    store_users: &[User],
    current_user: &User,
    width: f32,
) -> Response {
    let current_name = store_users
        .iter()
        .find(|u| u.get_id() == task.assignee)
        .map(|u| u.get_username().to_owned())
        .unwrap_or_else(|| "Unassigned".to_string());
    let my_store = current_user.get_store();

    ComboBox::from_id_salt(Id::unique(task.id.key_string()))
        .selected_text(current_name)
        .width(width)
        .height(150.)
        .show_ui(ui, |ui| {
            let roster = crate::get_database_users();
            for user in assignable_users(&roster, my_store) {
                let previous = task.assignee.clone();
                let assignee_selection = ui.selectable_value(
                    &mut task.assignee,  // current_value: &mut RecordId
                    user.get_id(),       // selected_value: RecordId
                    user.get_username(), // text: &str or String
                );
                if assignee_selection.clicked() {
                    let mut edited = task.clone();
                    edited.assignee = user.get_id().clone();
                    task.assignee = previous;
                    stage_edit(task, &edited, TaskField::Assignee);
                    task.assignee = edited.assignee;
                }
            }
        })
        .response
}
