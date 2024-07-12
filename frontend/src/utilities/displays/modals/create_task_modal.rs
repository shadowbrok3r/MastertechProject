use std::collections::BTreeSet;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use database::{schema::{CustomerData, Priority, Record, Status, TaskNotePayload, TaskPayload, TicketData, User, TASK_TABLE}, DATABASE};
use eframe::egui::{Align, Button, Color32, ComboBox, Direction, FontId, Layout, Margin, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use eframe::egui::{vec2, Grid, ScrollArea};
use egui_extras::DatePickerButton;
use log::info;
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

use crate::utilities::{ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, ModalTypes};

use super::{task_modal::ModalAction, ModalState};


#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal{
    pub title: String,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,  
    pub store_users: Option<Vec<User>>,

    pub ticket_data: TicketData,
    pub task_data: TaskPayload,
    pub customer_data: CustomerData,
    pub task_notes: TaskNotePayload,

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
    pub fn new(title: &str, store_users: Option<Vec<User>>) -> Self {
        Self {
            title: title.to_owned(),
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            due_date: Utc::now().date_naive(),
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
            // self.tur_sheet(ui);
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
                    task_description: self.description.clone(),
                    assignee: usr.id,
                    due_date: y,
                    priority: self.task_priority.clone(),
                    task_note: None,
                    completed: false,
                    status: Status::Todo,
                    dep: format!("{:?}", usr.store),
                    ..Default::default()
                };

                spawn_local(async move{
                        let _: Vec<Record> = DATABASE
                        .create(TASK_TABLE)
                        .content(task_payload)
                        .await
                        .unwrap();
                });
            }
            ui.add_space(ui.available_width() / 3.0);
        });
        None
    }

}

impl CreateTaskModal {
    pub fn tur_sheet(&mut self, ui: &mut Ui) {
        let check = !self.ticket_data.service_number.is_empty();
        if ui.add_enabled(
            check, 
            Button::new( 
                RichText::new("Get PrestaShop Order")
                .color(Color32::from_rgb(255, 204, 255)) 
            )
            .stroke(
                Stroke::new(1.0, Color32::from_rgb(191, 33, 101))
            ).min_size(
                Vec2::new(145.0, 25.0)
            )
        ).clicked() {
            // let service_num = self.ticket_data.service_number.clone();
            // self.presta_api();
            // self.ticket_data = TicketData::default();
            // self.task_data = LiveTaskPayload::default();
            // self.customer_data = CustomerData::default();
            // self.task_notes = Vec::new();
            // self.ticket_data.service_number = service_num;
        }
        Grid::new("ticket_info_grid")
            .spacing(vec2(4.0, 7.0))
            .min_col_width( 135.0+3.0)
            .max_col_width( 135.0 + 8.0)
            .num_columns(2)
            .show(ui, |ui| 
        {
                                /*     ROW 1     */
            TextEdit::singleline(&mut self.ticket_data.service_number)
                .hint_text("Service #  ")
                .char_limit(8)
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(vec2( 135.0+2.0,14.0))
                .ui(ui);

            TextEdit::singleline(&mut self.customer_data.name)
                .hint_text("Customer Name  ")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(vec2( 135.0+2.0,14.0))
                .ui(ui);

            ui.end_row();

                                /*     ROW 2     */
            TextEdit::singleline(&mut self.customer_data.phone_number)
                .hint_text("Phone Number 1")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(vec2( 135.0+2.0,14.0))
                .ui(ui);

            TextEdit::singleline(&mut self.customer_data.phone_number_2)
                .hint_text("Phone Number 2")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(vec2( 135.0+2.0,14.0))
                .ui(ui);
            
            ui.end_row();

                                /*     ROW 3     */
            let mut inputs = BTreeSet::new();
            if let Some(users) = &self.store_users{

                for user in users.iter(){
                    let parsed = user.email.split_once("@").unwrap_or(("","")).0;
                    inputs.insert(parsed.to_string());
                }
                // let size = vec2(  135.0 + 2.0, 14.0 );
                let _result = AutoCompleteTextEdit::new(&mut self.ticket_data.salesman, inputs.clone())
                    .highlight_matches(true)
                    .max_suggestions(3)
                    .set_text_edit_properties(move |text_edit| 
                {
                    text_edit
                        .hint_text("Assignee")
                        // .min_size(size)
                        .font(FontId::proportional(12.0))
                        .frame(true)
                        // .horizontal_align(egui::Align::Center)
                })
                .ui(ui);

                let _result = AutoCompleteTextEdit::new(&mut self.ticket_data.tech, inputs.clone())
                    .highlight_matches(true)
                    .max_suggestions(3)
                    .set_text_edit_properties(move |text_edit| 
                {
                    text_edit
                        .hint_text("Tech")
                        // .min_size(size)
                        .font(FontId::proportional(12.0))
                        .frame(true)
                        // .horizontal_align(egui::Align::Center)
                })
                .ui(ui);

            } else {

                TextEdit::singleline(&mut self.ticket_data.salesman)
                    .hint_text("Assignee")
                    .vertical_align(Align::Center)
                    .margin(vec2(4.0, 4.0))
                    // .min_size(vec2( 135.0+2.0,14.0))
                    .ui(ui);
                
                TextEdit::singleline(&mut self.ticket_data.tech)
                    .hint_text("Tech")
                    .vertical_align(Align::Center)
                    .margin(vec2(4.0, 4.0))
                    // .min_size(vec2( 135.0+2.0,14.0))
                    .ui(ui);
            }
            
            ui.end_row();
        }); // grid

        let width = ui.available_width() / 2.0;
        let check = !self.ticket_data.service_number.is_empty()
            && !self.customer_data.name.is_empty()
            && !self.customer_data.phone_number.is_empty()
            && !self.ticket_data.salesman.is_empty()
            && !self.ticket_data.tech.is_empty();
        if ui
        .add_enabled(
            check,
            Button::new(RichText::new("Submit TUR").color(Color32::from_rgb(255, 204, 255)))
            .min_size(Vec2::new(width, 20.0))
            .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
        )
        .clicked()
        {  
            // self.submit_tur();
        }

        let check = !self.ticket_data.service_number.is_empty()
            && !self.customer_data.name.is_empty()
            && !self.customer_data.phone_number.is_empty()
            && !self.ticket_data.tech.is_empty();
        if ui
            .add_enabled(check, 
                Button::new( RichText::new("Master-Tech.app"))
                .min_size(Vec2::new(width, 20.0)))
            .clicked()
        {  
        // self.submit_tur_mastertech(); 
        }

        ScrollArea::new([false, true])
        .id_source("checkin_notes_scroll")
        .show(ui, |_ui|{
            let _ = TextEdit::multiline(&mut self.ticket_data.checkin_notes)
            .hint_text(RichText::new("Checkin Notes").weak())
            .font(FontId::proportional(15.0))
            .desired_rows(4);
        });
        ScrollArea::new([false, true])
        .id_source("recomendations_scroll")
        .show(ui, |_ui|{
            let _ = TextEdit::multiline(&mut self.task_data.task_description)
            .hint_text(RichText::new("Recommendations").weak())
            .font(FontId::proportional(15.0))
            .desired_rows(4);
        });
    }

}