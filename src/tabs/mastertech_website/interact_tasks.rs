use chrono::{DateTime, NaiveDate, Utc, Datelike};
use eframe::egui::{Align, Button, Color32, ComboBox, Id, Response, RichText, Stroke, TextEdit, Ui, Widget};

use crate::{database::{database::Database, schema::{Priority, Status, TaskPayload, User}}, utilities::Interaction};
use egui_extras::DatePickerButton;
use log::info;

use crate::utilities::Updatable;

use super::task_cards::date_colors;

impl Interaction for TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(110));
        let text_edit = TextEdit::singleline(&mut self.task_name).desired_width(ui.available_width() - 10.0).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);
        if text_edit.changed(){
            self.update_task_name(self.task_name.clone(), database);
        }
        Some(text_edit)
    }

    fn interact_checkin_notes(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);

        
        if let Some(service_ticket) = &mut self.service_ticket{ // std::mem::take()

            let text_edit = TextEdit::multiline(&mut service_ticket.checkin_notes)
                .desired_rows(5)
                .desired_width(ui.available_width())
                .horizontal_align(Align::Center)
                .ui(ui);
            if text_edit.changed() {
                let notes = service_ticket.checkin_notes.clone();
                self.update_checkin_notes(Some(notes), database.clone());
                info!("checkin_notes changed: {:?}// {:?}", self.id, self.task_name);
            }
        }else{
            TextEdit::multiline(&mut "No checkin notes")
                .desired_rows(5)
                .desired_width(ui.available_width())
                .horizontal_align(Align::Center)
                .ui(ui);
        }


        None
    }

    fn interact_task_description(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));

        let text_edit = TextEdit::multiline(&mut self.task_description)
            .desired_rows(6)
            .desired_width(ui.available_width())
            .horizontal_align(Align::Center)
            .ui(ui);

        if text_edit.changed() {
            self.update_task_description(self.task_description.clone(), database.clone());
        }
        None
    }

    fn interact_due_date(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let frame_color = date_colors(self.due_date.clone(), self.completed);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(0.5, frame_color);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(0.5, frame_color);
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
            let hover_txt = "✔";
            let color_complete = Color32::LIGHT_GREEN;
            // let color_incomplete = Color32::LIGHT_RED;

            let stroke = Stroke::new(1.0, color_complete);
            let button = Button::new(hover_txt).stroke(stroke).small();
            let res = ui.add_sized(ui.available_size(), button);
           
            // if res.hovered(){
            //     res.stroke(Stroke::new(2.0, color_incomplete));
            // }
            if res.clicked(){
                self.update_completed(false, database);
            }
            Some(res)
        }else{
            let hover_txt = "✖";
            // let color_complete = Color32::LIGHT_GREEN;
            let color_incomplete = Color32::LIGHT_RED;

            let stroke = Stroke::new(1.0, color_incomplete);
            let button = Button::new(hover_txt).stroke(stroke).small();
            let res = ui.add_sized(ui.available_size(), button);
            // if res.hovered(){
            //     button.stroke(Stroke::new(2.0, color_complete));
            // }
            if res.clicked(){
                self.update_completed(true, database);
            }
            Some(res)
        }
    }

    fn interact_status(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(RichText::new(format!("{}", &self.status.as_str())))
            .width(ui.available_width())
            .height(ui.available_height())
            .show_ui(ui, |ui| 
        {
            for mut status in Status::VALUES{
                let status_change = ui.selectable_value(&mut self.status, status.to_owned(), status.as_str());
                if status_change.clicked(){
                    // info!("assignee changed?: {:?}// {:?} // {:?}", self.id, self.task_name, everest_initials);
                    self.update_status(status.clone(), database.clone());
                }
            }
        });
        Some(combo_box.response)
    }

    fn interact_priority(&mut self, ui: &mut Ui, database: Database) -> Option<Response> {
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(RichText::new(format!("{}", &self.priority.as_str())))
            .width(ui.available_width() - 2.0)
            .height(ui.available_height() - 2.0)
            .show_ui(ui, |ui| 
        {
            for mut priority in Priority::VALUES{
                let priority_change = ui.selectable_value(&mut self.priority, priority.to_owned(), priority.as_str());
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
            .selected_text(RichText::new(&self.everest_initials).small())
            .width(ui.available_width() / 1.3)
            .height(ui.available_height() - 2.0)
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
    
    fn interact_dep(&mut self, ui: &mut Ui, _database: Database) -> Option<Response> {
        if let Some(ref mut dep) = self.dep {
            ui.label("Store:");
            let dep = ui.text_edit_singleline(dep);
            Some(dep)
        } else {
            ui.label("No department specified.");
            None
        }
        
    }
}