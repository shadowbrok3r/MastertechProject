use database::{schema::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, Priority, Status, TaskNotePayload, TaskPayload, TicketPayload, User},DATABASE};
use crate::{get_current_user_from_auth, ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, PlatformSpawner, Spawner};
use eframe::egui::{vec2, Align, Button, Color32, ComboBox, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use database::schema::utilities::{get_prestashop_payload, create_full_task_payload};
use super::{tabs::{display_ticket_page, display_computer_page}, task_modal::ModalAction};
use surrealdb::{sql::Datetime, RecordId};
use chrono::{Datelike, NaiveDate, Utc};
use egui_extras::DatePickerButton;
use crossbeam::channel::Sender;
use std::collections::BTreeSet;
use log::{error, info};
use serde::Serialize;

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal {
    pub title: String,
    pub current_page_state: ModalAction,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,
    pub store_users: Vec<User>,

    pub task_name: String,
    pub task_priority: Priority,
    pub due_date: Datetime,
    pub description: String,
    pub assignee: String,
    pub tur: Tur,
    #[serde(skip)]
    pub prestashop_api_tx: Option<Sender<PrestashopPayload>>,
    user: User
}

// TODO This is an ugly implementation
#[derive(Debug, Clone, Default, Serialize)]
pub struct Tur {
    pub data: PrestashopPayload,
    pub ticket_data: TicketPayload,
    pub task_data: TaskPayload,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    pub store_users: Vec<User>,
}

impl CreateTaskModal {
    /// Create a new modal with the given title.
    pub fn new(
        title: &str,
        store_users: Vec<User>,
        prestashop_api_tx: Sender<PrestashopPayload>,
    ) -> Self {
        Self {
            title: title.to_owned(),
            min_width: Some(500.0),
            min_height: Some(500.0),
            default_height: Some(500.0),
            full_span_content: false,
            due_date: Utc::now().into(),
            store_users,
            prestashop_api_tx: Some(prestashop_api_tx),
            tur: Tur::default(),
            current_page_state: ModalAction::TicketInfoPage,
            user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            },
            ..Default::default()
        }
    }

    pub fn update_tur_info(&mut self, tur: Tur) {
        self.tur = tur;
        let name = self.tur.customer_data.name.clone();
        let service_num = self.tur.ticket_data.service_number.clone();
        if !service_num.is_empty() && !name.is_empty()
        {
            self.task_name = format!(
                "{} - {}",
                self.tur.customer_data.name,
                self.tur.ticket_data.service_number
            );
        }
    }
}

impl DisplayModal for CreateTaskModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction> {
        let avail_size = Vec2::new(500.0, 500.0);
        ui.set_min_size(avail_size);
        ui.set_max_size(avail_size);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {

                ui.add_space(225.0);

                if ui
                    .selectable_label(self.current_page_state == ModalAction::TicketInfoPage, RichText::new("🖹").heading())
                    .clicked()
                {
                    self.current_page_state = ModalAction::TicketInfoPage;
                };

                if ui
                    .selectable_label(
                        self.current_page_state == ModalAction::ComputerInfoPage,
                        RichText::new("🖹").heading(),
                    )
                    .clicked()
                {
                    self.current_page_state = ModalAction::ComputerInfoPage;
                };    
                if ui
                    .selectable_label(
                        self.current_page_state == ModalAction::ImportTask,
                        RichText::new("🖥").heading(),
                    )
                    .clicked()
                {
                    self.current_page_state = ModalAction::ImportTask;
                };
            });

            ui.add_space(20.);

            ui.horizontal_centered(|ui| {
                match self.current_page_state {
                    ModalAction::TicketInfoPage => {
                        if let Some(tx) = self.prestashop_api_tx.clone()
                        {
                            ui.add_space(10.0);
                            let action = self
                            .create_task(
                                ui, 
                                tx.clone()
                            );
                            if let ModalAction::Close = action {
                                action_handler(ModalAction::Close);
                            }
                        }
                    }
                    ModalAction::ImportTask => {
                        let store_users = self.store_users.clone();

                        display_ticket_page(
                            ui,
                            &mut self.tur.task_data,
                            avail_size,
                            &store_users,
                            self.user.clone()
                        );
                    },
                    ModalAction::ComputerInfoPage => display_computer_page(ui, &mut self.tur.task_data, avail_size),
                    _ => {}
                };
            });
        });

        if self.current_page_state == ModalAction::Close {
            action_handler(ModalAction::Close);
        }
        Some(self.current_page_state.clone())
    }
}

impl CreateTaskModal {
    pub fn create_task(
        &mut self,
        ui: &mut Ui,
        prestashop_api_tx: Sender<PrestashopPayload>,
    ) -> ModalAction {
        ui.vertical_centered(|ui| {
            let mut lost_focus = false;
            self.tur.tur_sheet(ui, prestashop_api_tx.clone());

            ui.add_space(15.0);

            let _ = TextEdit::singleline(&mut self.task_name)
                .hint_text("Task Name")
                .margin(vec2(10., 3.))
                .desired_width(200.0)
                .ui(ui);

            ui.add_space(15.0);
            let mut inputs = BTreeSet::new();

            for user in self.store_users.iter() {
                inputs.insert(user.get_username().to_string());
            }

            let r = AutoCompleteTextEdit::new(
                &mut self.assignee, 
                inputs.clone()
            )
            .highlight_matches(true)
            .max_suggestions(3)
            .set_text_edit_properties(move |text_edit| {
                text_edit
                    .hint_text("Assignee")
                    .desired_width(200.0)
                    .desired_rows(1)
                    .margin(vec2(10., 3.))
                    .frame(true)
            })
            .ui(ui);
        
            if r.lost_focus() {
                lost_focus = true;
            }
        
            ui.add_space(15.0);

            ui.horizontal_top(|ui|{
                ui.add_space(150.);
                ui.scope(|ui| {
                    ui.style_mut().spacing.combo_width = 70.0;
                    ComboBox::new("PriorityComboBox", "")
                        .selected_text(RichText::new(format!(
                            "{}",
                            &self.task_priority.as_str()
                        )))
                        .show_ui(ui, |ui| {
                            for priority in Priority::VALUES {
                                ui.selectable_value(
                                    &mut self.task_priority,
                                    priority.to_owned(),
                                    priority.as_str(),
                                );
                            }
                        });
                });

                let mut due_date = self.due_date.date_naive();
                let date_picker = DatePickerButton::new(&mut due_date)
                    .calendar_week(false)
                    .format("%m/%d/%y")
                    .show_icon(true)
                    .ui(ui);

                if date_picker.changed() {
                    // Combine the NaiveDate with a default time to create a DateTime<Utc>
                    let date_time = NaiveDate::from_ymd_opt(
                        due_date.year(), 
                        due_date.month(), 
                        due_date.day()
                    )
                    .unwrap_or_default()
                    .and_hms_opt(0, 0, 0)
                    .unwrap_or_default()
                    .and_local_timezone(Utc)
                    .unwrap();
                
                    self.due_date = date_time.clone().into();
                }
            });

            ui.add_space(15.0);

            let r = TextEdit::multiline(&mut self.description)
                .hint_text("Task Description")
                .margin(vec2(10., 3.))
                .desired_rows(10)
                .code_editor()
                .desired_width(350.0)
                .ui(ui);

            if lost_focus {
                r.request_focus();
            }

            ui.add_space(15.0);
            let btn = Button::new("Submit")
                .min_size(Vec2::new(130.0, 30.0))
                .fill(Color32::from_rgb(30, 30, 35))
                .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)));
            let pulling_ticket = !self.tur.ticket_data.service_number.is_empty();
            let check = !self.task_name.is_empty() && !self.description.is_empty() && !self.assignee.is_empty();

            let enabled = if (pulling_ticket && check) || (check) { true } else { false };

            if ui.add_enabled(enabled, btn).clicked() {
                // let service_num = self.ticket_data.service_number.clone();
                // Self::presta_api(prestashop_api_tx, self.ticket_data.service_number.clone());
                // self.ticket_data = TicketPayload::default();
                // self.task_data = TaskPayload::default();
                // self.customer_data = CustomerData::default();
                // // self.task_notes = Vec::new::<Vec<TaskNotePayload>>();
                // self.ticket_data.service_number = service_num;

                self.current_page_state = ModalAction::Close;
                info!("ASSIGNEE: {:?}\nSTATE: {:?}", self.assignee.clone(), self.current_page_state);
                let assignee = self.assignee.clone();
                let mut payload = self.tur.clone();                   
                payload.task_data.priority = self.task_priority.clone();
                payload.task_data.created_at = Utc::now().into();
                payload.task_data.due_date = self.due_date.clone();
                payload.task_data.completed = false;
                payload.task_data.status = Status::Todo;
                payload.task_data.task_name = self.task_name.clone();
                payload.task_data.task_description = self.description.clone();
                payload.task_data.service_number = Some(payload.ticket_data.service_number.clone());
                
                
                let mut usr = User::default();
                for user in self.store_users.iter() {
                    if assignee == user.get_username() {
                        log::info!("Got {:?} from assignee: {assignee:?}", user.get_name());
                        usr = user.clone();
                    }
                }

                payload.task_data.everest_initials = usr.get_initials().to_string();

                // let live_task_payload = LiveTaskPayload {
                //     task_name: self.task_name.clone(),
                //     task_description: self.description.clone(),
                //     due_date: date,
                //     priority: self.task_priority.clone(),
                //     completed: false,
                //     status: Status::Todo,
                //     service_number: service_number.clone(),
                //     service_ticket: if let Some(ticket) = &payload.task_data.service_ticket {
                //         Some(ticket.id.clone())
                //     } else {
                //         None
                //     },
                //     everest_initials: usr.everest_initials,
                //     assignee: usr.id,
                //     ..Default::default()
                // };

                let task = payload.task_data.clone();
                PlatformSpawner::spawn(async move {
                    if !payload.ticket_data.service_number.is_empty() {

                        if payload.ticket_data.salesman.is_empty() {
                            info!("Salesman was empty, assigning current user");
                            payload.ticket_data.salesman = assignee.clone();
                            info!("TicketData.Salesman: {:?}\nAssignee: {:?}", 
                                payload.ticket_data.salesman.clone(), 
                                assignee.clone()
                            );
                        }
                        
                        let create_task_result = create_full_task_payload(
                            payload.ticket_data.into(),
                            payload.customer_data.clone(),
                            ComputerData::default(),
                            task.into(),
                            payload.task_notes,
                            false,
                        ).await;
                        info!("create_task_result: {create_task_result:?}");

                    } else {
                        info!("Creating Regular Task");
                        match User::query_user_from_email(assignee).await {
                            Ok(user) => {
                                payload.task_data.assignee = user.get_id();
                                payload.task_data.everest_initials = user.get_initials().to_string();

                                log::info!("Payload: {payload:?}");
                                let query: Result<surrealdb::Response, surrealdb::Error> = DATABASE
                                    .query("CREATE task CONTENT $content")
                                    .bind(("content", payload.task_data))
                                    .await;

                                match query {
                                    Ok(mut res) => {
                                        let _: Option<RecordId> = res.take(0).unwrap_or_default();
                                    },
                                    Err(e) => error!("Error creating task: {e:?}")
                                }
                            }
                            Err(e) => error!("Error getting user: {e:?}"),
                        }
                    }
                });
            }
        });

        self.current_page_state.clone()
    }
}

impl Tur {
    pub fn set_store_users(&mut self, users: Vec<User>) -> &mut Self {
        self.store_users = users;
        self
    }

    pub fn tur_sheet(&mut self, ui: &mut Ui, prestashop_api_tx: Sender<PrestashopPayload>) {
        // ui.horizontal_top(|ui| {
        let check = !self.ticket_data.service_number.is_empty();
        let stroke = Stroke::new(1.0, Color32::from_rgb(191, 33, 101));
        let txt_color = Color32::from_rgb(255, 204, 255);
        let txt = RichText::new("Pull Order").color(txt_color);
        let button_size = Vec2::new(120.0, 25.0);
        let button = Button::new(txt).stroke(stroke).min_size(button_size);

        if ui.add_enabled(check, button).clicked() {
            let service_num = self.ticket_data.service_number.clone();
            Self::presta_api(prestashop_api_tx, self.ticket_data.service_number.clone());
            self.ticket_data = TicketPayload::default();
            self.task_data = TaskPayload::default();
            self.customer_data = CustomerData::default();
            // self.task_notes = Vec::new::<Vec<TaskNotePayload>>();
            self.ticket_data.service_number = service_num;
        }

        ui.add_space(15.0);


        TextEdit::singleline(&mut self.ticket_data.service_number)
            .hint_text(" Service #  ")
            .char_limit(8)
            .vertical_align(Align::Center)
            .margin(vec2(10., 3.))
            .desired_width(200.)
            .ui(ui);
    }

    pub fn presta_api(prestashop_api_tx: Sender<PrestashopPayload>, input: String) {
        let input = input.clone();
        let tx = prestashop_api_tx.clone();
        if !input.is_empty() {
            PlatformSpawner::spawn(async move {
                let _ = tx.try_send(
                    get_prestashop_payload(&input.clone()).await.unwrap_or_default()
                );
            });
        }
    }
}

