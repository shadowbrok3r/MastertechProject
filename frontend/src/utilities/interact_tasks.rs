use chrono::{DateTime, NaiveDate, Utc, Datelike};
use egui::{Align, Button, Color32, ComboBox, Id, Response, RichText, Stroke, TextEdit, Ui, Widget};

use database::{schema::{Priority, User, Status, TaskPayload}, Database};
use egui_extras::DatePickerButton;
use log::info;

use crate::utilities::Updatable;

use super::Interaction;



impl Interaction for TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let text_edit = TextEdit::singleline(&mut self.task_name).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);
        if text_edit.changed(){
            self.update_task_name(self.task_name.clone(), database);
        }
        Some(text_edit)
    }

    fn interact_task_description(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        ui.add_space(10.0);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));
        // ui.style_mut().visuals.widgets.inactive.fg = Color32::BLACK;
        if let Some(description) = &self.task_description{
            let res = ui.label(
                egui::RichText::new(description).color(Color32::WHITE)
            );
            None
        }else{
            let mut task_description = "No task description";
            let text_edit = TextEdit::multiline(&mut task_description)
                .desired_rows(7)
                .desired_width(ui.available_width())
                .horizontal_align(egui::Align::Center)
                .ui(ui);

            if text_edit.changed(){
                info!("task_description changed: {:?}// {:?}", self.id, self.task_name);
            }
            Some(text_edit)
        }
        
    }

    fn interact_recommendations(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let mut recommendations = "These are test checkin notes";
        let text_edit = TextEdit::multiline(&mut recommendations)
            .desired_rows(4)
            .desired_width(ui.available_width())
            .horizontal_align(egui::Align::Center)
            .show(ui);

        if text_edit.response.changed(){
            info!("recommendations changed: {:?}// {:?}", self.id, self.task_name);
        }
        Some(text_edit.response)
    }

    fn interact_due_date(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let mut due_date = self.due_date.parse::<DateTime<Utc>>().unwrap().date_naive();
        
        let id = self.id.clone().unwrap().0.id.to_string();
        let date_picker = DatePickerButton::new(&mut due_date)
            .format("%m/%d/%y")
            .id_source(id.as_str())
            .show_icon(false)
            .ui(ui);

        if date_picker.changed(){
            // Combine the NaiveDate with a default time to create a DateTime<Utc>
            let date_time = NaiveDate::from_ymd_opt(due_date.year(), due_date.month(), due_date.day())
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Utc)
                .unwrap();
            let rfc3339_date = date_time.to_rfc3339();
            let date = due_date.clone().to_string();
            self.update_due_date(rfc3339_date.clone(), database);
            info!("date_widget changed: {:?}// {:?} ", self.task_name,  date);
        }
        None
    }

    fn interact_completed(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        if self.completed{
            let stroke = Stroke::new(2.0, Color32::DARK_GREEN);
            let button = Button::new("✔️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            let res = ui.add_sized(ui.available_size(), button);
            if res.clicked(){
                info!("marked incomplete: {:?}// {:?}", self.id, self.task_name);
            }
            Some(res)
        }else{
            let stroke = Stroke::new(2.0, Color32::from_rgba_premultiplied(200, 20, 200, 50));
            let button = Button::new("✖️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            let res = ui.add_sized(ui.available_size(), button);
            if res.clicked(){
                info!("marked completed: {:?}// {:?}", self.id, self.task_name);
                self.update_completed(!self.completed, database);
            }
            Some(res)
        }
    }

    fn interact_status(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let mut current_status = self.status.clone();
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(RichText::new(format!("{:?}", &current_status.as_str())))
            .width(ui.available_width())
            .height(ui.available_height())
            .show_ui(ui, |ui| 
        {
            for mut status in Status::VALUES{
                let status_change = ui.selectable_value(&mut current_status.clone(), self.status.clone(), status.as_str());
                if status_change.clicked(){
                    // info!("assignee changed?: {:?}// {:?} // {:?}", self.id, self.task_name, everest_initials);
                    self.update_status(status.clone(), database.clone());
                }
            }
        });
        Some(combo_box.response)
    }

    fn interact_dep(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        if let Some(ref mut dep) = self.dep {
            ui.label("Store:");
            let dep = ui.text_edit_singleline(dep);
            Some(dep)
        } else {
            ui.label("No department specified.");
            None
        }
        
    }

    fn interact_priority(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let mut current_priority = self.priority.clone();
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
        .selected_text(format!("{:?}", &current_priority.as_str()))
        .width(ui.available_width())
        .height(ui.available_height())
        .show_ui(ui, |ui| 
        {
            for mut priority in Priority::VALUES{
                let priority_change = ui.selectable_value(&mut current_priority.clone(), self.priority.clone(), priority.as_str());
                if priority_change.clicked(){
                    // info!("assignee changed?: {:?}// {:?} // {:?}", self.id, self.task_name, everest_initials);
                    self.update_priority(Some(priority.clone()), database.clone());
                }
            }
        });
        Some(combo_box.response)
    }

    fn interact_assignee_initials(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<Response> {
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(&self.everest_initials)
            .width(ui.available_width())
            .height(ui.available_height()/ 2.0)
            .show_ui(ui, |ui| 
        {
            for user in *&store_users{
                let assignee_selection = ui.selectable_value(&mut self.everest_initials, user.everest_initials.to_owned(), &user.everest_initials);
                if assignee_selection.clicked(){
                    info!("assignee changed?: {:?}// {:?} // {:?}", self.id, self.task_name, user.everest_initials.clone());
                    self.update_assignee_initials(user.everest_initials.clone(), database.clone());
                }
            }
        });
        Some(combo_box.response)
    }
}