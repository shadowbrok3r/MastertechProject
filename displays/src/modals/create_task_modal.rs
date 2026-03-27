use database::{schema::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, LiveTaskPayload, Priority, Status, TaskNotePayload, TaskCreationResult, TicketData, User, prestashop::OrderType},DATABASE};
use crate::{get_current_user_from_auth, get_toast_sender, ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, PlatformSpawner, Spawner, ToastMessage};
use eframe::egui::{Align, Button, Color32, ComboBox, Frame, RichText, Spinner, Stroke, TextEdit, Ui, Vec2, Widget, vec2};
use database::schema::utilities::{get_prestashop_payload, create_full_task_payload};
use super::{tabs::{display_ticket_page, display_computer_page}, task_modal::ModalAction};
use database::schema::{Datetime, RecordId};
use chrono::{Datelike, NaiveDate, Utc};
use egui_extras::DatePickerButton;
use crossbeam::channel::{Sender, Receiver};
use std::collections::BTreeSet;
use log::{error, info};
use serde::{Deserialize, Serialize};

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
    user: User,
    /// Flag indicating task creation is in progress
    pub creating_task: bool,
    /// Channel for receiving task creation result
    #[serde(skip)]
    pub creation_result_tx: Option<Sender<TaskCreationResult>>,
    #[serde(skip)]
    pub creation_result_rx: Option<Receiver<TaskCreationResult>>,
    /// Text to copy to clipboard after successful creation
    pub clipboard_text: Option<String>,
}

// TODO This is an ugly implementation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tur {
    pub data: PrestashopPayload,
    pub ticket_data: TicketData,
    pub task_data: LiveTaskPayload,
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
        let (creation_result_tx, creation_result_rx) = crossbeam::channel::unbounded();
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
            creating_task: false,
            creation_result_tx: Some(creation_result_tx),
            creation_result_rx: Some(creation_result_rx),
            clipboard_text: None,
            ..Default::default()
        }
    }

    pub fn update_tur_info(&mut self, tur: Tur) {
        self.tur = tur;
        let name = self.tur.customer_data.name.clone();
        let service_num = self.tur.ticket_data.service_number.clone();
        let computer = &mut self.tur.computer_data;
        if !computer.device_mfg.is_none() || !computer.device_serial.is_none() || !computer.cpu.is_empty() || !computer.gpu.is_empty() || !computer.ram.is_empty() {
            self.tur.ticket_data.computer = Some(computer.id.clone());
        }

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
        // Check for creation result
        if let Some(rx) = &self.creation_result_rx {
            if let Ok(result) = rx.try_recv() {
                self.creating_task = false;
                let toast_tx = get_toast_sender();
                match result {
                    TaskCreationResult::Created { service_number } => {
                        let _ = toast_tx.try_send(ToastMessage::Success(
                            format!("Task created for service #{service_number}")
                        ));
                        // Copy description to clipboard on success
                        if let Some(text) = self.clipboard_text.take() {
                            ui.ctx().copy_text(text);
                        }
                        self.current_page_state = ModalAction::Close;
                    },
                    TaskCreationResult::AlreadyExists { service_number } => {
                        let _ = toast_tx.try_send(ToastMessage::Warning(
                            format!("Task already exists for service #{service_number}")
                        ));
                        // Still copy to clipboard and close since task exists
                        if let Some(text) = self.clipboard_text.take() {
                            ui.ctx().copy_text(text);
                        }
                        self.current_page_state = ModalAction::Close;
                    },
                    TaskCreationResult::Updated { service_number } => {
                        let _ = toast_tx.try_send(ToastMessage::Info(
                            format!("Task updated for service #{service_number}")
                        ));
                        // Copy to clipboard on update
                        if let Some(text) = self.clipboard_text.take() {
                            ui.ctx().copy_text(text);
                        }
                        self.current_page_state = ModalAction::Close;
                    },
                    TaskCreationResult::Error { message } => {
                        let _ = toast_tx.try_send(ToastMessage::Error(
                            format!("Error creating task: {message}")
                        ));
                        // Don't close on error - let user try again
                        self.clipboard_text = None;
                    },
                }
            }
        }
        
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
                        let tur = &mut self.tur;
                        let mut seb_checking = false; // Not used in create task modal
                        display_ticket_page(
                            ui,
                            &mut tur.task_data,
                            Some(&mut tur.ticket_data),
                            Some(&mut tur.customer_data),
                            Some(&mut tur.computer_data),
                            avail_size,
                            &store_users,
                            self.user.clone(),
                            None, // No SEB channel for create task
                            &mut seb_checking,
                            None, // No customer modal for create task
                        );
                    },
                    ModalAction::ComputerInfoPage => {
                        let tur = &mut self.tur;
                        display_computer_page(
                            ui, 
                            Some(&mut tur.ticket_data), 
                            Some(&mut tur.computer_data), 
                            avail_size
                        )
                    },
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
            .set_text_edit_properties(|text_edit| {
                text_edit
                    .hint_text("Assignee")
                    .desired_width(200.0)
                    .desired_rows(1)
                    .margin(vec2(10., 3.))
                    .frame(Frame::NONE)
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

                let mut due_date = crate::to_jiff_date(&self.due_date);
                let date_picker = DatePickerButton::new(&mut due_date)
                    .calendar_week(false)
                    .format("%m/%d/%y")
                    .show_icon(true)
                    .ui(ui);

                if date_picker.changed() {
                    self.due_date = crate::apply_jiff_date(&self.due_date, &due_date).into();
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
            let enabled = enabled && !self.creating_task;

            // Show spinner when creating task
            if self.creating_task {
                Spinner::new().size(20.0).ui(ui);
                ui.label(RichText::new("Creating...").color(Color32::LIGHT_BLUE));
            }

            if ui.add_enabled(enabled, btn).clicked() {
                info!("ASSIGNEE: {:?}", self.assignee.clone());
                let assignee = self.assignee.clone();
                let mut payload = self.tur.clone();                   
                payload.task_data.priority = self.task_priority.clone();
                payload.task_data.created_at = Utc::now().into();
                payload.task_data.due_date = self.due_date.clone();
                payload.task_data.completed = false;
                
                // Set status to QC for non-service orders (sales orders, R2R, BSD, RCI)
                let order_type = OrderType::from_id_str(&payload.data.order.id_order_type);
                if order_type != OrderType::ServiceOrder {
                    payload.task_data.status = Status::Qc;
                    info!("Order type is {:?}, setting task status to QC", order_type);
                } else {
                    payload.task_data.status = Status::Todo;
                }
                
                payload.task_data.task_name = self.task_name.clone();
                payload.task_data.task_description = self.description.clone();
                payload.task_data.service_number = Some(payload.ticket_data.service_number.clone());
                
                // Store the description to copy to clipboard after success
                self.clipboard_text = Some(self.description.clone());
                self.creating_task = true;
                
                let usr = &mut User::default();
                for user in self.store_users.iter() {
                    if assignee == user.get_username() {
                        log::info!("Got {:?} from assignee: {assignee:?}", user.get_name());
                        *usr = user.clone();
                    }
                }

                let task = payload.task_data.clone();
                let result_tx = self.creation_result_tx.clone();
                let service_number = payload.ticket_data.service_number.clone();
                
                // Check if we have a service number but haven't pulled the order yet
                // (customer name will be empty if not pulled)
                let needs_pull = !service_number.is_empty() && payload.customer_data.name.is_empty();
                
                PlatformSpawner::spawn(async move {
                    let mut payload = payload;
                    
                    // If service number is entered but data wasn't pulled, pull it now
                    if needs_pull {
                        info!("Service number entered but order not pulled, fetching now: {}", service_number);
                        match get_prestashop_payload(&service_number).await {
                            Ok(presta_data) => {
                                info!("Successfully fetched prestashop data for order {}", service_number);
                                // Update payload with fetched data
                                payload.customer_data.name = presta_data.customer.name.clone();
                                payload.customer_data.email = presta_data.customer.email.clone();
                                payload.customer_data.phone_number = presta_data.customer.phone_number.clone();
                                payload.customer_data.cust_code = presta_data.customer.cust_code.clone();
                                payload.customer_data.id = presta_data.customer.id.clone();
                                
                                // Update ticket data
                                let sales_rep = presta_data.sales_rep.clone().unwrap_or_default();
                                let split_rep = presta_data.split_rep.clone().unwrap_or_default();
                                let email = database::schema::helper_traits::parse_email_user(&sales_rep.email).to_string();
                                let email_split_rep = database::schema::helper_traits::parse_email_user(&split_rep.email).to_string();
                                
                                payload.ticket_data.salesman = email_split_rep;
                                payload.ticket_data.sales_rep = email.clone();
                                payload.ticket_data.tech = email.clone();
                                payload.ticket_data.customer = payload.customer_data.id.clone();
                                payload.ticket_data.checkin_rep = email;
                                payload.ticket_data.terms = presta_data.order.payment.clone();
                                payload.ticket_data.ticket_total = presta_data.order.total_products_wt.clone();
                                payload.ticket_data.doc_alias = presta_data.order.order_type.clone();
                                
                                if let Some(service) = presta_data.order.associations.order_service.first() {
                                    payload.ticket_data.checkin_notes = service.check_in_notes.clone();
                                }
                                
                                // Copy task notes
                                for msg in presta_data.task_notes.iter() {
                                    payload.task_notes.push(TaskNotePayload {
                                        task_id: Some(payload.task_data.id.clone()),
                                        ..msg.clone()
                                    });
                                }
                                
                                // Extract computer data using the Order's methods
                                let model = presta_data.order.extract_model();
                                if !model.is_empty() {
                                    payload.computer_data.device_model = Some(model);
                                }
                                
                                // Extract all specs including device serial and mfg
                                let specs = presta_data.order.extract_specs().await;
                                if !specs.cpu.is_empty() {
                                    payload.computer_data.cpu = specs.cpu;
                                }
                                if !specs.gpu.is_empty() {
                                    payload.computer_data.gpu = specs.gpu;
                                }
                                if !specs.ram.is_empty() {
                                    payload.computer_data.ram = specs.ram;
                                }
                                if !specs.device_serial.is_empty() {
                                    payload.computer_data.device_serial = Some(specs.device_serial);
                                }
                                if !specs.device_mfg.is_empty() {
                                    payload.computer_data.device_mfg = Some(specs.device_mfg);
                                }
                                
                                // Also update task status based on order type
                                let order_type = OrderType::from_id_str(&presta_data.order.id_order_type);
                                if order_type != OrderType::ServiceOrder {
                                    payload.task_data.status = Status::Qc;
                                    info!("Auto-pulled order type is {:?}, setting task status to QC", order_type);
                                }
                            }
                            Err(e) => {
                                error!("Failed to fetch prestashop data: {:?}", e);
                                // Continue anyway with what we have
                            }
                        }
                    }
                    
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
                            payload.computer_data.clone(),
                            task.into(),
                            payload.task_notes,
                            true,
                        ).await;
                        info!("create_task_result: {create_task_result:?}");

                        // Send result through channel
                        if let Some(tx) = result_tx {
                            let _ = tx.try_send(create_task_result);
                        }

                    } else {
                        info!("Creating Regular Task");
                        match User::query_user_from_email(assignee).await {
                            Ok(user) => {
                                payload.task_data.assignee = user.get_id();

                                log::info!("Payload: {payload:?}");
                                let query: Result<_, surrealdb::Error> = DATABASE
                                    .query("CREATE task CONTENT $content")
                                    .bind(("content", payload.task_data))
                                    .await;

                                match query {
                                    Ok(mut res) => {
                                        let _: Option<database::schema::Record> = res.take(0).unwrap_or_default();
                                        if let Some(tx) = result_tx {
                                            let _ = tx.try_send(TaskCreationResult::Created { 
                                                service_number: "regular_task".to_string() 
                                            });
                                        }
                                    },
                                    Err(e) => {
                                        error!("Error creating task: {e:?}");
                                        if let Some(tx) = result_tx {
                                            let _ = tx.try_send(TaskCreationResult::Error { 
                                                message: format!("{e}") 
                                            });
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Error getting user: {e:?}");
                                if let Some(tx) = result_tx {
                                    let _ = tx.try_send(TaskCreationResult::Error { 
                                        message: format!("{e}") 
                                    });
                                }
                            },
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
        let check = !self.ticket_data.service_number.is_empty();
        let stroke = Stroke::new(1.0, Color32::from_rgb(191, 33, 101));
        let txt_color = Color32::from_rgb(255, 204, 255);
        let txt = RichText::new("Pull Order").color(txt_color);
        let button_size = Vec2::new(70.0, 25.0);
        let button = Button::new(txt).stroke(stroke).min_size(button_size);
        
        ui.horizontal_top(|ui| {
            ui.add_space(ui.available_width() / 3.5);
            if ui.add_enabled(check, button).clicked() {
                let service_num = self.ticket_data.service_number.clone();
                self.ticket_data = TicketData::default();
                self.task_data = LiveTaskPayload::default();
                self.customer_data = CustomerData::default();
                self.ticket_data.service_number = service_num;
                // self.task_notes = Vec::new::<Vec<TaskNotePayload>>();
                Self::presta_api(prestashop_api_tx, self.ticket_data.service_number.clone());
            }
        
            let upload = Button::new("Upload TUR")
                .min_size(Vec2::new(70., 25.))
                .stroke(Stroke::new(1., ui.style().visuals.warn_fg_color))
                .ui(ui);
            
            if upload.clicked() {
                // let tx = tx.clone();
                PlatformSpawner::spawn(async move {
                    if let Some(_file) = rfd::AsyncFileDialog::new()
                        .set_file_name("tur.json")
                        .pick_file()
                        .await
                    {
                        // match serde_json::from_slice::<Tur>(&file.read().await) {
                        //     Ok(tur) => { let _ = tx.try_send(tur); },
                        //     Err(e) => log::error!("Error converting bytes to Theme: {e:?}"),
                        // }
                    }
                });
            }
        });

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

