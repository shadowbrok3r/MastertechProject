use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use database::{schema::{Priority, Record, Status, TaskPayload, User, TASK_TABLE}, Database};
use egui::{Align, Button, Color32, ComboBox, Direction, FontId, Layout, Margin, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use log::info;
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

use crate::utilities::{DisplayModal, ModalTypes};

use super::{task_modal::ModalAction, ModalState};


#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal{
    pub title: String,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,  
    #[serde(skip)]
    pub database: Option<Database>,
    pub store_users: Option<Vec<User>>,

    pub task_name: String,
    pub task_priority: Priority,
    pub due_date: NaiveDate,
    pub description: String,
    pub assignee: Option<User>,
    #[serde(skip)]
    pub state: ModalState
}

impl CreateTaskModal{
    /// Create a new modal with the given title.
    pub fn new(title: &str, database: Option<Database>, store_users: Option<Vec<User>>) -> Self {
        Self {
            title: title.to_owned(),
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            due_date: Utc::now().date_naive(),
            database,
            store_users,
            ..Default::default()
        }
    }
}

impl ModalTypes for CreateTaskModal{
    fn modal_state(&mut self) -> &mut ModalState {
        &mut self.state
    }
    fn title(mut self, title: String) -> Self {
        self.modal_state().title = Some(title);
        self
    }
}


impl DisplayModal for CreateTaskModal {
    fn display(&mut self, ui: &mut Ui, _current_state: ModalAction) -> Option<ModalAction>{
        let mut _response: Option<ModalAction> = None;

        ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center), |ui| {
            ui.style_mut().override_font_id = Some(FontId::proportional(15.0));

            ui.add_space(50.0);
            let combo_center_width = ui.available_width() / 2.98;

            TextEdit::singleline(&mut self.task_name)
                .hint_text("Task Name")
                .margin(Margin::symmetric(6.0, 4.0))
                .desired_width(200.0)
                .ui(ui);

            ui.add_space(10.0);

            ui.horizontal_top(|ui| {
                ui.add_space(combo_center_width);
                if let Some(users) = &mut self.store_users{
                    ui.style_mut().spacing.combo_width = 50.0;
                    ComboBox::new("AssigneeComboBox", "")
                        .selected_text(self.assignee.as_ref().unwrap_or(users.get(0).as_ref().unwrap()).everest_initials.clone())
                        .show_ui(ui, |ui| 
                    {
                        for user in users.iter_mut(){
                            let initials = user.everest_initials.clone();
                            let x = ui.selectable_value(&mut self.assignee, Some(user.to_owned()), &initials.clone());
                            if x.changed(){
                                info!("x changed: {:?}", self.assignee);
                            }
                        }
                    });
                }
                ui.scope(|ui| {
                    ui.style_mut().spacing.combo_width = 70.0;
                    ComboBox::new("PriorityComboBox", "")
                        .selected_text(RichText::new(format!("{}", &self.task_priority.as_str())))
                        .show_ui(ui, |ui| 
                    {
                        for mut priority in Priority::VALUES{
                            ui.selectable_value(&mut self.task_priority, priority.to_owned(), priority.as_str());
                        }
                    });
                });

                DatePickerButton::new(&mut self.due_date)
                    .calendar_week(false)
                    .format("%m/%d/%y")
                    .show_icon(true)
                    .ui(ui);
            });
            
            ui.add_space(10.0);

            TextEdit::multiline(&mut self.description)
                .hint_text("Task Description")
                .margin(Margin::symmetric(6.0, 4.0))
                .desired_rows(6)
                .code_editor()
                .desired_width(200.0)
                .ui(ui);

            ui.add_space(10.0);
            if Button::new("Submit")
                .min_size(Vec2::new(120.0, 30.0))
                .fill(Color32::from_rgb(30, 30, 35))
                .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)))
                .ui(ui)
                .clicked()
            {
                let db = self.database.clone();
                let time = NaiveTime::from_hms_milli_opt(0,0,0,0).unwrap();
                let date = NaiveDateTime::new(self.due_date, time);
                let y = date.and_utc().to_rfc3339();
                let usr = self.assignee
                    .as_ref()
                    .unwrap_or(
                        self.store_users.clone().unwrap_or(Vec::new())
                        .get(0)
                        .as_ref()
                        .unwrap()
                )
                .clone();

                let task_payload = TaskPayload{
                    task_name: self.task_name.clone(),
                    everest_initials: usr.everest_initials,
                    task_description: Some(self.description.clone()),
                    assignee: Some(usr.id),
                    due_date: y,
                    priority: self.task_priority.clone(),
                    task_note: None,
                    completed: false,
                    status: Status::Todo,
                    dep: Some(format!("{:?}", usr.store)),
                    ..Default::default()
                };

                spawn_local(async move{
                    if let Some(db) = db{
                        let _: Vec<Record> = db
                        .database
                        .create(TASK_TABLE)
                        .content(task_payload)
                        .await
                        .unwrap();
                    }
            
                });
            }
            ui.add_space(ui.available_width() / 3.0);
        });
        None
    }

}
