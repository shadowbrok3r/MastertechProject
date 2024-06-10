use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use database::{schema::{Priority, Record, Status, TaskPayload, User, TASK_TABLE}, Database};
use egui::{Align, Button, Color32, ComboBox, Direction, FontId, Layout, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use log::info;
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

use crate::utilities::{DisplayModal, ModalTypes};

use super::{modals::ModalState, task_modal::ModalAction};

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
            due_date: NaiveDate::default(),
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
        ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
        
        let mut _response: Option<ModalAction> = None;
        StripBuilder::new(ui)
        .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
        .size(Size::exact(20.0))
        .size(Size::exact(200.0))
        .size(Size::exact(50.0))
        .vertical(|mut s| {
            s.empty();
            s.strip(|s| 
            {
                s
                    .cell_layout(Layout::centered_and_justified(Direction::TopDown))
                    .size(Size::exact(30.0))
                    .size(Size::exact(200.0))
                    .size(Size::exact(30.0))
                    .horizontal(|mut s| 
                {
                    s.empty();
                    s.cell(|ui| 
                    {
                        ui.horizontal_top(|ui| 
                        { 
                            ui.style_mut().override_font_id = Some(FontId::proportional(15.0));
                            TextEdit::singleline(&mut self.task_name)
                                .hint_text("Task Name")
                                .desired_width(130.0)
                                .ui(ui);
                        
                            ComboBox::new("PriorityComboBox", "")
                                .selected_text(RichText::new(format!("{}", &self.task_priority.as_str())))
                                .show_ui(ui, |ui| 
                            {
                                for mut priority in Priority::VALUES{
                                    ui.selectable_value(&mut self.task_priority, priority.to_owned(), priority.as_str());
                                }
                            });

                            DatePickerButton::new(&mut self.due_date)
                                .calendar(true)
                                .calendar_week(false)
                                .combo_boxes(true)
                                .format("%m/%d/%y")
                                .ui(ui);
                        });
                        // let x = DateTime

                        TextEdit::multiline(&mut self.description)
                            .hint_text("Task Description")
                            .desired_rows(3)
                            .code_editor()
                            .desired_width(200.0)
                            .ui(ui);

                        ui.horizontal_top(|ui| {

                            if let Some(users) = &mut self.store_users{
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

                            if Button::new("Submit")
                                .min_size(Vec2::new(120.0, 20.0))
                                .fill(Color32::from_rgb(30, 30, 35))
                                .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)))
                                .ui(ui)
                                .clicked()
                            {
                                let db = self.database.clone();
                                let time = NaiveTime::from_hms_milli_opt(0,0,0,0).unwrap();
                                let date = NaiveDateTime::new(self.due_date, time);
                                let y = date.and_utc().to_rfc3339();
                                let task_payload = TaskPayload{
                                    task_name: self.task_name.clone(),
                                    everest_initials: self.assignee.as_ref().unwrap().everest_initials.clone(),
                                    task_description: Some(self.description.clone()),
                                    assignee: Some(self.assignee.as_ref().unwrap().id.clone()),
                                    due_date: y,
                                    priority: self.task_priority.clone(),
                                    task_note: None,
                                    completed: false,
                                    status: Status::Todo,
                                    dep: Some(format!("{:?}", self.assignee.as_ref().unwrap().store)),
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
                        });
                    });
                    s.empty()
                });
            });
            s.empty();
        });
        None
    }

}
