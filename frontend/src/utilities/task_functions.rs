use core::future::Future;

use chrono::{DateTime, NaiveDate, Utc};
use egui::{epaint::text, Align, Button, Color32, ComboBox, Id, Response, RichText, Stroke, TextEdit, Ui, Widget};

use database::{schema::{ModifyTask, Priority, Status, Store, TaskPayload}, Database};
use egui_extras::DatePickerButton;
use log::info;
use surrealdb::{opt::RecordId, sql::Value};
use wasm_bindgen_futures::spawn_local;

pub trait Displayable{
    fn display_task_cards(&mut self, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Updatable {
    fn update_completed(&mut self, completed: bool, db: Database);
    fn update_due_date(&mut self, due_date: String, db: Database);
    fn update_assignee_initials(&mut self, initials: String, db: Database);
    fn update_task_name(&mut self, name: String, db: Database);
    fn update_status(&mut self, status: Status, db: Database);
    fn update_dep(&mut self, store: Store, db: Database);
    fn update_priority(&mut self, priority: Option<Priority>, db: Database);
    fn update_task_description(&mut self, description: Option<String>, db: Database);
}

pub trait Interaction{
    fn interact_task_name(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_task_description(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_recommendations(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_due_date(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_completed(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_status(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_dep(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_priority(&mut self, ui: &mut Ui) -> Option<Response>;
    fn interact_assignee_initials(&mut self, ui: &mut Ui) -> Option<Response>;
}

impl Updatable for TaskPayload {
    fn update_completed(&mut self, completed: bool, db: Database) {
        // self.completed = completed;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET completed={completed}, status='{:?}' WHERE id={id}",
                Status::Complete
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_due_date(&mut self, due_date: String, db: Database) {
        // self.due_date = due_date;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET due_date={} WHERE id={id}", due_date
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_assignee_initials(&mut self, initials: String, db: Database) {
        // self.assignee_initials = Some(initials);
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET assignee={} WHERE id={id}", initials
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_task_name(&mut self, name: String, db: Database) {
        // self.task_name = name;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET task_name={name} WHERE id={id}", 
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_status(&mut self, status: Status, db: Database) {
        // self.status = status;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let mut query = String::new();
            match status{
                Status::Todo => {
                    query = format!(
                        "UPDATE task SET status={:?} WHERE id={id}",
                        Status::Todo
                    );
                },
                Status::InRepair => {
                    query = format!(
                        "UPDATE task SET status={:?} WHERE id={id}",
                        Status::InRepair
                    );
                },
                Status::Complete => {
                    query = format!(
                        "UPDATE task SET status={:?} WHERE id={id}",
                        Status::Complete
                    );
                },
            }

            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_dep(&mut self, dep: Store, db: Database) {
        // self.dep = Some(dep);
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET dep={:?} WHERE id={id}", dep
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_priority(&mut self, priority: Option<Priority>, db: Database) {
        // self.priority = priority;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET priority={:?} WHERE id={id}", priority.unwrap()
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();


                info!("Updated task: {update_task:#?}");
        })
    }

    fn update_task_description(&mut self, description: Option<String>, db: Database) {
        // self.task_description = description;
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "UPDATE task SET description={} WHERE id={id}", description.unwrap()
            );
            let update_task: Vec<Value> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0)
                .unwrap();
        })
    }
}

impl Interaction for TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui) -> Option<Response> {
        let text_edit = TextEdit::singleline(&mut self.task_name).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);
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
        Some(text_edit.response)
    }

    fn interact_due_date(&mut self, ui: &mut Ui) -> Option<Response> {
        let datetime: DateTime<Utc> = self.due_date.parse().expect("Failed to parse date");
        let mut formatted: NaiveDate = datetime.date_naive(); // format("%m/%d/%y");
        let id = self.id.clone().unwrap().0.id.to_string();
        let date_picker = DatePickerButton::new(&mut formatted)
            .format("%m/%d/%y")
            .id_source(id.as_str())
            .show_icon(false);

        ui.add_sized(ui.available_size(), date_picker);
        None
    }

    fn interact_completed(&mut self, ui: &mut Ui) -> Option<Response> {
        if self.completed{
            let stroke = Stroke::new(2.0, Color32::DARK_GREEN);
            let button = Button::new("✔️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            let res = ui.add_sized(ui.available_size(), button);
            Some(res)
        }else{
            let stroke = Stroke::new(2.0, Color32::from_rgba_premultiplied(200, 20, 200, 50));
            let button = Button::new("✖️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            let res = ui.add_sized(ui.available_size(), button);
            Some(res)
        }
    }

    fn interact_status(&mut self, ui: &mut Ui) -> Option<Response> {
        let mut status = Status::Todo;
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
            ui.selectable_value(&mut status, Status::Todo, "Todo");
            ui.selectable_value(&mut status, Status::InRepair, "In Repair");
            ui.selectable_value(&mut status, Status::Complete, "Complete");
        });
        Some(combo_box.response)
    }

    fn interact_dep(&mut self, ui: &mut Ui) -> Option<Response> {
        if let Some(ref mut dep) = self.dep {
            ui.label("Department:");
            let dep = ui.text_edit_singleline(dep);
            Some(dep)
        } else {
            ui.label("No department specified.");
            None
        }
        
    }

    fn interact_priority(&mut self, ui: &mut Ui) -> Option<Response> {
        let combo_box = ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
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
            Some(combo_box.response)
        }else{
            None
        }
        
    }

    // Add more interact methods for other fields if necessary...
}