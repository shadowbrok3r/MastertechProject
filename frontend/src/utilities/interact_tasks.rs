use core::future::Future;

use chrono::{DateTime, NaiveDate, Utc};
use egui::{epaint::text, Align, Button, Color32, ComboBox, Id, Response, RichText, Stroke, TextEdit, Ui, Widget};

use database::{schema::{ModifyTask, Priority, Status, Store, TaskPayload}, Database};
use egui_extras::DatePickerButton;
use log::info;
use surrealdb::{opt::RecordId, sql::Value};
use wasm_bindgen_futures::spawn_local;

use super::Interaction;



impl Interaction for TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui) -> Option<Response> {
        let text_edit = TextEdit::singleline(&mut self.task_name).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);
        if text_edit.changed(){
            info!("task_name changed: {:?}// {:?}", self.id, self.task_name);
        }
        Some(text_edit)
    }

    fn interact_task_description(&mut self, ui: &mut Ui) -> Option<Response> {
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

    fn interact_recommendations(&mut self, ui: &mut Ui) -> Option<Response>{
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

    fn interact_due_date(&mut self, ui: &mut Ui) -> Option<Response> {
        let datetime: DateTime<Utc> = self.due_date.parse().expect("Failed to parse date");
        let mut formatted: NaiveDate = datetime.date_naive(); // format("%m/%d/%y");
        let id = self.id.clone().unwrap().0.id.to_string();
        let date_picker = DatePickerButton::new(&mut formatted)
            .format("%m/%d/%y")
            .id_source(id.as_str())
            .show_icon(false)
            .ui(ui);

        if date_picker.clicked(){
            info!("date_widget changed: {:?}// {:?} // {:?}", self.id, self.task_name, self.due_date);
        }
        None
    }

    fn interact_completed(&mut self, ui: &mut Ui) -> Option<Response> {
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
            }
            Some(res)
        }
    }

    fn interact_status(&mut self, ui: &mut Ui) -> Option<Response> {
        // let mut status = Status::Todo;
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(
                RichText::new(
                    format!("{:?}", &self.status)
                )
                
            )
            .width(ui.available_width())
            .height(ui.available_height())
            .show_ui(ui, |ui| 
        {
            ui.selectable_value(&mut self.status, Status::Todo, "Todo");
            ui.selectable_value(&mut self.status, Status::InRepair, "In Repair");
            ui.selectable_value(&mut self.status, Status::Complete, "Complete");
        });
        if combo_box.response.lost_focus(){
            info!("self.status changed?: {:?}// {:?} // {:?}", self.id, self.task_name, self.status);
        }
        Some(combo_box.response)
    }

    fn interact_dep(&mut self, ui: &mut Ui) -> Option<Response> {
        if let Some(ref mut dep) = self.dep {
            ui.label("Store:");
            let dep = ui.text_edit_singleline(dep);
            Some(dep)
        } else {
            ui.label("No department specified.");
            None
        }
        
    }

    fn interact_priority(&mut self, ui: &mut Ui) -> Option<Response> {
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
        .selected_text(format!("{:?}", &self.priority))
        .width(ui.available_width())
        .height(ui.available_height())
        .show_ui(ui, |ui| 
        {
            ui.selectable_value(&mut self.priority, Priority::Normal, "Normal");
            ui.selectable_value(&mut self.priority, Priority::Rfs, "Rfs");
            ui.selectable_value(&mut self.priority, Priority::Qc, "Qc");
            ui.selectable_value(&mut self.priority, Priority::Express, "Express");
            ui.selectable_value(&mut self.priority, Priority::CustomerFire, "CustomerFire");
        });
        if combo_box.response.lost_focus(){
            info!("self.status changed?: {:?}// {:?} // {:?}", self.id, self.task_name, &self.status);
        }
        Some(combo_box.response)
    }

    fn interact_assignee_initials(&mut self, ui: &mut Ui) -> Option<Response> {
        if let Some(assignee_initials) = &mut self.assignee_initials{
            let new_assignee = String::new();
            let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
                .selected_text(assignee_initials.clone())
                .width(ui.available_width())
                .height(ui.available_height()/ 2.0)
                .show_ui(ui, |ui| 
            {
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
            
            });
            if combo_box.response.lost_focus(){
                info!("self.status changed?: {:?}// {:?} // {:?}", self.id, self.task_name, assignee_initials);
            }
            Some(combo_box.response)
        }else{
            None
        }
        
    }

    // Add more interact methods for other fields if necessary...
}