use chrono::{DateTime, NaiveDate, Utc, Datelike};
use database::schema::{Priority, Status, UserId};
use egui::{Align, Button, ComboBox, Direction, FontId, Layout, RichText, TextEdit, Ui, Vec2, Widget};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use log::info;
use serde::Serialize;

use crate::utilities::{DisplayModal, ModalTypes};

use super::{modals::ModalState, task_modal::ModalAction};

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal{
    pub title: String,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,  

    pub task_name: String,
    pub task_status: Status,
    pub task_priority: Priority,
    pub due_date: NaiveDate,
    pub description: String,
    pub assignee: Option<UserId>,

    pub state: ModalState
}

impl CreateTaskModal{
    /// Create a new modal with the given title.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            due_date: NaiveDate::default(),
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
                            TextEdit::singleline(&mut self.task_name)
                                .hint_text("Task Name")
                                .desired_width(130.0)
                                .ui(ui);
                            
                            ComboBox::new("StatusComboBox", "")
                                .selected_text(RichText::new(format!("{}", &self.task_status.as_str())))
                                // .width(ui.available_width())
                                .show_ui(ui, |ui| 
                            {
                                for mut status in Status::VALUES{
                                    let status_change = ui.selectable_value(&mut self.task_status, status.to_owned(), status.as_str());
                                    if status_change.clicked(){
                                        
                                    }
                                }
                            });

                            ComboBox::new("PriorityComboBox", "")
                                .selected_text(RichText::new(format!("{}", &self.task_priority.as_str())))
                                .show_ui(ui, |ui| 
                            {
                                for mut priority in Priority::VALUES{
                                    let priority_change = ui.selectable_value(&mut self.task_priority, priority.to_owned(), priority.as_str());
                                    if priority_change.clicked(){
                                        // info!("assignee changed?: {:?}// {:?} // {:?}", self.id, self.task_name, everest_initials);
                                        // self.update_priority(Some(priority.clone()), database.clone());
                                    }
                                }
                            });
                            // let mut due_date = self.due_date.parse::<DateTime<Utc>>().unwrap().date_naive();
                            // let id = self.id.clone().unwrap().0.id.to_string();
                            {
                                ui.style_mut().override_font_id = Some(FontId::proportional(6.0));
                                ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
                                let date_picker = DatePickerButton::new(&mut self.due_date)
                                    .calendar(true)
                                    .calendar_week(false)
                                    .combo_boxes(true)
                                    .format("%m/%d/%y")
                                    .ui(ui);
                                if date_picker.changed(){
                                    // Combine the NaiveDate with a default time to create a DateTime<Utc>
                                    // let date_time = NaiveDate::from_ymd_opt(due_date.year(), due_date.month(), due_date.day())
                                    //     .unwrap()
                                    //     .and_hms_opt(0, 0, 0)
                                    //     .unwrap()
                                    //     .and_local_timezone(Utc)
                                    //     .unwrap();
                                    // let rfc3339_date = date_time.to_rfc3339();
                                    // let date = due_date.clone().to_string();
                                    // // self.update_due_date(rfc3339_date.clone(), database);
                                    // info!("date_widget changed: {:?}// {:?} ", self.task_name,  date);
                                }
                            }
                    



                        });

                        TextEdit::multiline(&mut self.description)
                            .hint_text("Task Description")
                            .desired_rows(6)
                            .code_editor()
                            .desired_width(200.0)
                            .ui(ui);

                        ui.horizontal(|ui| {
                            if Button::new("Create Task")
                                .min_size(Vec2::new(100.0, 16.0))
                                .ui(ui)
                                .clicked()
                            {
                                
                            }
                        });
                    });
                    s.empty();
                });
            });
            s.empty();
        });
        None
    }

}
