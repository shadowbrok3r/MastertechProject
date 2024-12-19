use database::{schema::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, LiveTaskPayload, Priority, Status, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User, TASK_TABLE},DATABASE};
use eframe::egui::{vec2, Align, Button, Color32, ComboBox, FontId, Layout, Margin, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use database::schema::{get_data::get_user_from_email, utilities::{get_prestashop_payload, create_full_task_payload}};
use crate::{ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, PlatformSpawner, Spawner};
use super::task_modal::{display_ticket_page, ModalAction};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use crossbeam::channel::Sender;
use std::collections::BTreeSet;
use log::{error, info, warn};
use surrealdb::RecordId;
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
    pub due_date: NaiveDate,
    pub description: String,
    pub assignee: String,
    pub tur: Tur,
    #[serde(skip)]
    pub prestashop_api_tx: Option<Sender<PrestashopPayload>>,
}


// TODO This is an ugly implementation
#[derive(Debug, Clone, Default, Serialize)]
pub struct Tur {
    pub data: PrestashopPayload,
    pub ticket_data: TicketPayload,
    pub task_data: TaskPayload,
    pub customer_data: CustomerData,
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
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            due_date: Utc::now().date_naive(),
            store_users,
            prestashop_api_tx: Some(prestashop_api_tx),
            tur: Tur::default(),
            current_page_state: ModalAction::TicketInfoPage,
            ..Default::default()
        }
    }

    pub fn update_tur_info(&mut self, tur: Tur) {
        self.tur = tur;
    }
}

// impl ModalTypes for CreateTaskModal {
//     fn modal_state(&mut self) -> &mut ModalState {
//         &mut self.state
//     }
//     fn title(mut self, title: String) -> Self {
//         self.modal_state().title = Some(title);
//         self
//     }
// }

impl DisplayModal for CreateTaskModal {
    fn display(&mut self, ui: &mut Ui) -> Option<ModalAction> {
        let avail_size = Vec2::new(680., 580.);

        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(Align::Center))
            .size(Size::exact(30.0))
            .size(Size::exact(10.0))
            .size(Size::relative(0.9))
            .vertical(|mut strip| {
                strip.strip(|strip| {
                    strip
                        .size(Size::exact(avail_size.x / 3.0))
                        .size(Size::remainder())
                        .size(Size::exact(avail_size.x / 3.0))
                        .cell_layout(Layout::top_down_justified(Align::Center))
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .cell_layout(Layout::top_down_justified(Align::Center))
                        .horizontal(|mut strip| {
                            strip.empty();
                            strip.cell(|ui| {
                                ui.horizontal_top(|ui| {

                                    ui.add_space(90.0);

                                    if ui
                                        .selectable_label(self.current_page_state == ModalAction::TicketInfoPage, RichText::new("🖹").heading())
                                        .clicked()
                                    {
                                        self.current_page_state = ModalAction::TicketInfoPage;
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
                            });
                            strip.empty();
                        });
                });
                strip.empty();
                strip.strip(|strip| {
                    strip
                        .size(Size::exact(avail_size.x))
                        .horizontal(|mut strip| {
                            strip.strip(|s| {
                                let size = if let ModalAction::ImportTask = self.current_page_state {
                                    Size::exact(avail_size.x - 15.0)
                                } else {
                                    Size::exact(avail_size.x / 2.0)
                                };

                                s.size(Size::remainder())
                                    .size(size)
                                    .size(Size::remainder())
                                    .cell_layout(Layout::top_down(Align::Center))
                                    .cell_layout(Layout::top_down(Align::Center))
                                    .cell_layout(Layout::top_down(Align::Center))
                                    .horizontal(|mut s| {
                                        s.empty();
                                        s.cell(|ui| {
                                            // ui.style_mut().override_font_id =
                                            //     Some(FontId::proportional(13.0));
                                            match self.current_page_state {
                                                ModalAction::TicketInfoPage => {
                                                    if let Some(tx) = self.prestashop_api_tx.clone()
                                                    {
                                                        ui.add_space(50.0);
                                                        if let ModalAction::Close = self
                                                            .create_task(ui, avail_size, tx.clone())
                                                        {
                                                            self.current_page_state = ModalAction::Close
                                                        }
                                                    }
                                                }
                                                ModalAction::ImportTask => {
                                                    ui.set_width(660.0);
                                                    display_ticket_page(
                                                        ui,
                                                        &mut self.tur.task_data,
                                                        avail_size,
                                                    );
                                                }
                                                _ => {
                                                    if let Some(tx) = self.prestashop_api_tx.clone()
                                                    {
                                                        if let ModalAction::Close = self
                                                            .create_task(ui, avail_size, tx.clone())
                                                        {
                                                            self.current_page_state = ModalAction::Close
                                                        }
                                                    }
                                                }
                                            };
                                        });
                                        s.empty();
                                    });
                            });
                        });
                });
            });

        Some(self.current_page_state.clone())
    }
}

impl CreateTaskModal {
    pub fn create_task(
        &mut self,
        ui: &mut Ui,
        avail_size: Vec2,
        prestashop_api_tx: Sender<PrestashopPayload>,
    ) -> ModalAction {
        StripBuilder::new(ui)
            .size(Size::exact(avail_size.y / 4.0))
            .size(Size::exact(115.0))
            .size(Size::exact(avail_size.y / 4.0 - 20.0))
            .vertical(|mut strip| {
                strip.cell(|ui| self.tur.tur_sheet(ui, prestashop_api_tx.clone()));

                strip.strip(|s| {
                    s.size(Size::exact(70.0))
                        .size(Size::exact(35.0))
                        .size(Size::exact(150.0))
                        .vertical(|mut s| {
                            s.cell(|ui| {
                                let service_num = self.tur.ticket_data.service_number.clone();

                                let edit = TextEdit::singleline(&mut self.task_name)
                                    .hint_text("Task Name")
                                    .margin(Margin::symmetric(6.0, 4.0))
                                    .desired_width(200.0)
                                    .ui(ui);

                                let name = self.tur.customer_data.name.clone();
                                if !service_num.is_empty() && edit.lost_focus() && !name.is_empty()
                                {
                                    self.task_name = format!(
                                        "{} - {}",
                                        self.tur.customer_data.name,
                                        self.tur.ticket_data.service_number
                                    );
                                }

                                ui.add_space(15.0);
                                let mut inputs = BTreeSet::new();
                                for user in self.store_users.iter_mut() {
                                    let parsed = user.email.split_once("@").unwrap_or(("", "")).0;
                                    inputs.insert(parsed.to_string());
                                }
                                let _result =
                                    AutoCompleteTextEdit::new(&mut self.assignee, inputs.clone())
                                        .highlight_matches(true)
                                        .max_suggestions(3)
                                        .set_text_edit_properties(move |text_edit| {
                                            text_edit
                                                .hint_text("Assignee")
                                                .desired_width(200.0)
                                                .font(FontId::proportional(12.0))
                                                .frame(true)
                                        })
                                        .ui(ui);
                            });

                            s.cell(|ui| {
                                ui.horizontal_top(|ui| {
                                    ui.add_space(80.0);

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

                                    DatePickerButton::new(&mut self.due_date)
                                        .calendar_week(false)
                                        .format("%m/%d/%y")
                                        .show_icon(true)
                                        .ui(ui);
                                });
                            });
                            s.cell(|ui| {
                                ui.vertical_centered(|ui| {
                                    TextEdit::multiline(&mut self.description)
                                        .hint_text("Task Description")
                                        .margin(Margin::symmetric(6.0, 4.0))
                                        .desired_rows(6)
                                        .code_editor()
                                        .desired_width(200.0)
                                        .ui(ui);

                                    ui.add_space(15.0);
                                    let btn = Button::new("Submit")
                                        .min_size(Vec2::new(130.0, 30.0))
                                        .fill(Color32::from_rgb(30, 30, 35))
                                        .stroke(Stroke::new(2.0, Color32::from_rgb(30, 3, 28)));
                                    let pulling_ticket = self.tur.ticket_data.service_number.len() == 7;
                                    let check = !self.task_name.is_empty() && !self.description.is_empty() && !self.assignee.is_empty();

                                    let enabled = if (pulling_ticket && check) || (check) { true } else { false };

                                    if ui.add_enabled(enabled, btn).clicked()
                                    {
                                        info!("ASSIGNEE: {:?}", self.assignee.clone());
                                        let time =
                                            NaiveTime::from_hms_milli_opt(0, 0, 0, 0).unwrap();
                                        let date = NaiveDateTime::new(self.due_date, time);
                                        let y = date.and_utc().to_rfc3339();
                                        let so = self.tur.ticket_data.service_number.clone();
                                        let service_number =
                                            if !so.is_empty() { Some(so) } else { None };

                                        let assignee = self.assignee.clone();
                                        let mut payload = self.tur.clone();                                        
                                        payload.task_data.priority = self.task_priority.clone();
                                        payload.task_data.due_date = y.clone();
                                        payload.task_data.completed = false;
                                        payload.task_data.status = Status::Todo;
                                        payload.task_data.task_name = self.task_name.clone();
                                        payload.task_data.task_description = self.description.clone();
                                        
                                        let live_task_payload = LiveTaskPayload {
                                            task_name: self.task_name.clone(),
                                            task_description: self.description.clone(),
                                            due_date: y.clone(),
                                            priority: self.task_priority.clone(),
                                            completed: false,
                                            status: Status::Todo,
                                            service_number: service_number.clone(),
                                            service_ticket: Some(self.tur.ticket_data.id.clone()),
                                            ..Default::default()
                                        };
                                        
                                        warn!("--> SELF.TUR: {:#?}\n--> LIVE TASK PAYLOAD: {:#?}\n--> TASK PAYLOAD: {:#?}", 
                                            payload.clone(), 
                                            live_task_payload.clone(),
                                            payload.task_data.clone()
                                        );

                                        PlatformSpawner::spawn(async move {
                                            if !payload.ticket_data.service_number.is_empty() {
                                                warn!("Submitting Ticket\n=====> PRE CONVERTED: {:?}\n\n", payload.ticket_data.clone());
                                                let mut ticket_data: TicketData = payload.ticket_data.into();
                                                warn!("=====> POST CONVERTED: {:?}\n\n", ticket_data.clone());

                                                if ticket_data.salesman.is_empty() {
                                                    info!("Salesman was empty, assigning current user");
                                                    ticket_data.salesman = assignee.clone();
                                                    info!("TicketData.Salesman: {:?}\nAssignee: {:?}", 
                                                        ticket_data.salesman.clone(), 
                                                        assignee.clone()
                                                    );
                                                } 

                                                info!("TicketData: {:?}", ticket_data.clone());

                                                info!("Attaching Customer with Ticket: {:?}", &payload.customer_data.name);
                                                match create_full_task_payload(
                                                    ticket_data,
                                                    payload.customer_data.clone(),
                                                    ComputerData::default(),
                                                    live_task_payload.clone(),
                                                    payload.task_notes,
                                                    false,
                                                )
                                                .await
                                                {
                                                    Ok(_) => info!("Created Records"),
                                                    Err(e) => error!("Error sending payload: {e:?}")
                                                }
                                            } else {
                                                info!("Creating Regular Task");
                                                let email = format!("{assignee}@pclaptops.com");

                                                match get_user_from_email(email).await {
                                                    Ok(user) => {
                                                        if let Some(usr) = user {

                                                            payload.task_data.assignee = usr.id;
                                                            payload.task_data.everest_initials =
                                                                usr.everest_initials;

                                                            let query: Result<surrealdb::Response, surrealdb::Error> = DATABASE
                                                                .query("CREATE task CONTENT $content")
                                                                .bind(("content", payload.task_data))
                                                                .await;

                                                            match query {
                                                                Ok(mut res) => {
                                                                    let result: Option<RecordId> = res.take(0).unwrap_or_default();
                                                                    info!("Result: {result:?}");
                                                                },
                                                                Err(e) => error!("Error creating task: {e:?}")
                                                            }
                                                                
                                                        }
                                                    }
                                                    Err(e) => error!("Error getting user: {e:?}"),
                                                }
                                            }
                                        });
                                        self.current_page_state = ModalAction::Close;
                                    }
                                });
                            });
                        });
                });
                strip.empty();
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
        ui.set_width(ui.available_width() / 3.0);
        ui.shrink_width_to_current();

        TextEdit::singleline(&mut self.ticket_data.service_number)
            .hint_text("Service #  ")
            .char_limit(8)
            .vertical_align(Align::Center)
            .margin(vec2(4.0, 4.0))
            .min_size(vec2(120.0, 14.0))
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

