use chrono::{DateTime, NaiveDate, Utc};
use egui::{Align, Button, Color32, ComboBox, Id, RichText, Stroke, TextEdit, Ui, Widget};

use database::schema::{ModifyTask, Priority, Status, TaskPayload};
use egui_extras::DatePickerButton;



pub trait Displayable{
    fn display_task_cards(&mut self, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Updatable {
    fn update_completed(&mut self, completed: bool);
    fn update_due_date(&mut self, due_date: String);
    fn update_assignee_initials(&mut self, initials: String);
    fn update_task_name(&mut self, name: String);
    fn update_status(&mut self, status: Status);
    fn update_dep(&mut self, dep: String);
    fn update_priority(&mut self, priority: Option<Priority>);
    fn update_task_description(&mut self, description: Option<String>);
}


impl Updatable for TaskPayload {
    fn update_completed(&mut self, completed: bool) {
        self.completed = completed;
    }

    fn update_due_date(&mut self, due_date: String) {
        self.due_date = due_date;
    }

    fn update_assignee_initials(&mut self, initials: String) {
        self.assignee_initials = Some(initials);
    }

    fn update_task_name(&mut self, name: String) {
        self.task_name = name;
    }

    fn update_status(&mut self, status: Status) {
        self.status = status;
    }

    fn update_dep(&mut self, dep: String) {
        self.dep = Some(dep);
    }

    fn update_priority(&mut self, priority: Option<Priority>) {
        self.priority = priority;
    }

    fn update_task_description(&mut self, description: Option<String>) {
        self.task_description = description;
    }
}

// Usage:
fn update_task_payload(task_payload: &mut TaskPayload) {
    task_payload.update_completed(true);
    task_payload.update_due_date("2024-06-01".to_string());
    task_payload.update_assignee_initials("JD".to_string());
    task_payload.update_task_name("New Task Name".to_string());
    task_payload.update_status(Status::InRepair);
    task_payload.update_dep("IT".to_string());
    task_payload.update_priority(Some(Priority::Normal));
    task_payload.update_task_description(Some("Updated task description.".to_string()));
}


pub trait Interaction{
    fn interact_task_name(&mut self, ui: &mut Ui);
    fn interact_task_description(&mut self, ui: &mut Ui);
    fn interact_recommendations(&mut self, ui: &mut Ui);
    fn interact_due_date(&mut self, ui: &mut Ui);
    fn interact_completed(&mut self, ui: &mut Ui);
    fn interact_status(&mut self, ui: &mut Ui);
    fn _interact_dep(&mut self, ui: &mut Ui);
    fn interact_priority(&mut self, ui: &mut Ui);
    fn interact_assignee_initials(&mut self, ui: &mut Ui);
}


impl Interaction for TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui) {
        TextEdit::singleline(&mut self.task_name).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);
    }

    fn interact_task_description(&mut self, ui: &mut Ui) {
        ui.add_space(10.0);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));
        // ui.style_mut().visuals.widgets.inactive.fg = Color32::BLACK;
        if let Some(description) = &self.task_description{
            ui.label(
                egui::RichText::new(description).color(Color32::WHITE)
            );
        }else{
            let mut task_description = "No task description";
            TextEdit::multiline(&mut task_description)
                .desired_rows(7)
                .desired_width(ui.available_width())
                .horizontal_align(egui::Align::Center)
                .ui(ui);
        }
    }

    fn interact_recommendations(&mut self, ui: &mut Ui){
        let mut recommendations = "These are test checkin notes";
        TextEdit::multiline(&mut recommendations)
            .desired_rows(4)
            .desired_width(ui.available_width())
            .horizontal_align(egui::Align::Center)
            .show(ui);
    }

    fn interact_due_date(&mut self, ui: &mut Ui) {
        let datetime: DateTime<Utc> = self.due_date.parse().expect("Failed to parse date");
        let mut formatted: NaiveDate = datetime.date_naive(); // format("%m/%d/%y");
        let id = self.id.clone().unwrap().0.id.to_string();
        let date_picker = DatePickerButton::new(&mut formatted)
            .format("%m/%d/%y")
            .id_source(id.as_str())
            .show_icon(false);

        ui.add_sized(ui.available_size(), date_picker);
    }

    fn interact_completed(&mut self, ui: &mut Ui) {
        if self.completed{
            let stroke = Stroke::new(2.0, Color32::GREEN);
            let button = Button::new("✔️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            ui.add_sized(ui.available_size(), button);
        }else{
            let stroke = Stroke::new(2.0, Color32::from_rgb(200, 20, 200));
            let button = Button::new("✖️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            ui.add_sized(ui.available_size(), button);
        }
    }

    fn interact_status(&mut self, ui: &mut Ui) {
        let mut status = Status::Todo;
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(
                RichText::new(
                    format!("{:?}", &self.status)
                )
                
            )
            .width(ui.available_width())
            .height(ui.available_height())
            .show_ui(ui, |ui| 
        {
            ui.selectable_value(&mut status, Status::Todo, "Todo");
            ui.selectable_value(&mut status, Status::InRepair, "In Repair");
            ui.selectable_value(&mut status, Status::Complete, "Complete");
        });
    }

    fn _interact_dep(&mut self, ui: &mut Ui) {
        if let Some(ref mut dep) = self.dep {
            ui.label("Department:");
            ui.text_edit_singleline(dep);
        } else {
            ui.label("No department specified.");
        }
    }

    fn interact_priority(&mut self, ui: &mut Ui) {
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
        .selected_text(format!("{:?}", &self.priority.clone().unwrap_or_default()))
        .width(ui.available_width())
        .height(ui.available_height())
        .show_ui(ui, |ui| 
        {
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Normal, "Normal");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Rfs, "Rfs");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Qc, "Qc");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Express, "Express");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::CustomerFire, "CustomerFire");
        });
    }

    fn interact_assignee_initials(&mut self, ui: &mut Ui) {
        if let Some(assignee_initials) = &mut self.assignee_initials{
            let new_assignee = String::new();
            ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
                .selected_text(assignee_initials.clone())
                .width(ui.available_width())
                .height(ui.available_height()/ 2.0)
                .show_ui(ui, |ui| 
            {
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
            
            });
        }
    }

    // Add more interact methods for other fields if necessary...
}