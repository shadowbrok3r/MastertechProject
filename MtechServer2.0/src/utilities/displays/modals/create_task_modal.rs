use super::{
    task_modal::{display_ticket_page, ModalAction},
    ModalState,
};
use crate::utilities::{get_data::get_user_from_email, DisplayModal, ModalTypes};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use crossbeam::channel::Sender;
use database::{
    schema::{
        prestashop_schema::{
            Address, Customer, CustomerMessage, CustomerThread, Employee, Order, Prestashop,
            PrestashopPayload,
        }, utilities::{query_id, query_user_from_email}, ComputerData, CustomerData, LiveTaskPayload, Priority, Status, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE
    },
    DATABASE,
};
use displays::ui_tools::autocomplete::AutoCompleteTextEdit;
use eframe::egui::vec2;
use eframe::egui::{
    Align, Button, Color32, ComboBox, FontId, Layout, Margin, RichText, Stroke, TextEdit, Ui, Vec2,
    Widget,
};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use log::{error, info, warn};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use surrealdb::RecordId;
use wasm_bindgen_futures::spawn_local;

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal {
    pub title: String,
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
    #[serde(skip)]
    pub state: ModalState,
}

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
            state: ModalState::default(),
            due_date: Utc::now().date_naive(),
            store_users,
            prestashop_api_tx: Some(prestashop_api_tx),
            tur: Tur::default(),
            ..Default::default()
        }
    }

    pub fn update_tur_info(&mut self, tur: Tur) {
        self.tur = tur;
    }
}

impl ModalTypes for CreateTaskModal {
    fn modal_state(&mut self) -> &mut ModalState {
        &mut self.state
    }
    fn title(mut self, title: String) -> Self {
        self.modal_state().title = Some(title);
        self
    }
}

impl DisplayModal for CreateTaskModal {
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction> {
        let mut response: Option<ModalAction> = None;
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
                                    let mut main_page = false;
                                    let mut import_task_page = false;
                                    match current_page_state {
                                        ModalAction::TicketInfoPage => main_page = true,
                                        ModalAction::ImportTask => import_task_page = true,
                                        _ => main_page = true,
                                    };

                                    ui.add_space(90.0);

                                    if ui
                                        .selectable_label(main_page, RichText::new("🖹").heading())
                                        .clicked()
                                    {
                                        response = Some(ModalAction::TicketInfoPage);
                                    };
                                    if ui
                                        .selectable_label(
                                            import_task_page,
                                            RichText::new("🖥").heading(),
                                        )
                                        .clicked()
                                    {
                                        response = Some(ModalAction::ImportTask);
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
                                let size = if let ModalAction::ImportTask = current_page_state {
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
                                            match current_page_state {
                                                ModalAction::TicketInfoPage => {
                                                    if let Some(tx) = self.prestashop_api_tx.clone()
                                                    {
                                                        ui.add_space(50.0);
                                                        self.create_task(
                                                            ui,
                                                            avail_size,
                                                            tx.clone(),
                                                        );
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
                                                            response = Some(ModalAction::Close)
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

        response
    }
}

impl CreateTaskModal {
    pub fn create_task(
        &mut self,
        ui: &mut Ui,
        avail_size: Vec2,
        prestashop_api_tx: Sender<PrestashopPayload>,
    ) -> ModalAction {
        let mut action = ModalAction::None;

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
                                            service_ticket: self.tur.ticket_data.id.clone(),
                                            ..Default::default()
                                        };
                                        
                                        warn!("--> SELF.TUR: {:#?}\n--> LIVE TASK PAYLOAD: {:#?}\n--> TASK PAYLOAD: {:#?}", 
                                            payload.clone(), 
                                            live_task_payload.clone(),
                                            payload.task_data.clone()
                                        );

                                        spawn_local(async move {
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
                                                match send_payload(
                                                    ticket_data,
                                                    payload.customer_data.clone(),
                                                    ComputerData::default(),
                                                    live_task_payload.clone(),
                                                    payload.task_notes,
                                                    false,
                                                )
                                                .await
                                                {
                                                    Ok(records) => {
                                                        info!("Created Records: {records:?}")
                                                    }
                                                    Err(e) => {
                                                        error!("Error sending payload: {e:?}")
                                                    }
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

                                                            match DATABASE
                                                                .create::<Option<RecordId>>(TASK_TABLE)
                                                                .content(payload.task_data)
                                                                .await {
                                                                    Ok(created_task) => info!("Created Task: {created_task:?}"),
                                                                    Err(e) => error!("Error creating task: {e:?}")
                                                                }
                                                                
                                                        }
                                                    }
                                                    Err(e) => error!("Error getting user: {e:?}"),
                                                }
                                            }
                                        });
                                        action = ModalAction::Close;
                                    }
                                });
                            });
                        });
                });
                strip.empty();
            });

        action
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
            self.presta_api(prestashop_api_tx);
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

    pub fn presta_api(&mut self, prestashop_api_tx: Sender<PrestashopPayload>) {
        let input = self.ticket_data.service_number.clone();
        let tx = prestashop_api_tx.clone();
        if !input.is_empty() {
            spawn_local(async move {
                let api_call = Prestashop::default();
                let mut query = HashMap::new();

                query.insert("filter[id_order]", input.as_str());
                query.insert("output_format", "JSON");

                let customer_threads: Vec<CustomerThread> = api_call
                    .request_resources_wasm("customer_threads", query.clone())
                    .await
                    .unwrap_or_default();

                let mut customer_messages: Vec<CustomerMessage> = Vec::new();

                if !customer_threads.is_empty() {
                    for thread in customer_threads.iter() {
                        for msg in thread.associations.customer_messages.iter() {
                            customer_messages.push(
                                api_call
                                    .request_subresources_by_id_wasm(
                                        "customer_messages",
                                        "customer_message",
                                        msg.id.as_str(),
                                    )
                                    .await
                                    .unwrap_or_default(),
                            );
                        }
                    }
                }

                let order: Order = api_call
                    .request_subresources_by_id_wasm("orders", "order", &input)
                    .await
                    .unwrap_or_default();

                if order.id_customer.is_empty() {
                    info!("Order is likely gonna fuKKKK");
                }

                info!("order: {order:#?}");

                let sales_rep: Option<Employee> = if !order.id_employee_sales_rep.contains("0") && !order.id_employee_sales_rep.is_empty() {
                    let employee: Employee = api_call
                        .request_subresources_by_id_wasm(
                            "employees",
                            "employee",
                            &order.id_employee_sales_rep,
                        )
                        .await
                        .unwrap_or_default();

                    info!("employee: {employee:#?}");


                    // let my_returns = employee.get_my_return_for_services().await;
                    // error!("RETURN FOR SERVICES: {:?}", my_returns);
                    Some(employee)
                } else {
                    None
                };

                let split_rep: Option<Employee> = if !order.id_employee_split_rep.contains("0") && !order.id_employee_split_rep.is_empty() {
                    let employee_2: Employee = api_call
                        .request_subresources_by_id_wasm(
                            "employees",
                            "employee",
                            &order.id_employee_split_rep,
                        )
                        .await
                        .unwrap_or_default();

                    info!("employee: {sales_rep:#?}");
                    Some(employee_2)
                } else {
                    None
                };

                let cust: Customer = api_call
                    .request_subresources_by_id_wasm("customers", "customer", &order.id_customer)
                    .await
                    .unwrap_or_default();

                // info!("customer: {customer:#?}");

                let address: Address = api_call
                    .request_subresources_by_id_wasm(
                        "addresses",
                        "address",
                        &order.id_address_invoice,
                    )
                    .await
                    .unwrap_or_default();


                info!("address: {address:#?}");
                let customer = CustomerData {
                    id: Some(RecordId::from((
                        CUSTOMER_TABLE.to_string(),
                        order.id_customer.clone(),
                    ))),
                    cust_code: order.id_customer.clone(),
                    name: format!("{} {}", &cust.firstname, &cust.lastname),
                    phone_number: address.phone.clone().to_string(),
                    // phone_number_2: address.phone_mobile.clone().unwrap_or(0).to_string(),
                    email: cust.email,
                    ..Default::default()
                };

                let presta_payload = PrestashopPayload {
                    customer,
                    order,
                    sales_rep,
                    split_rep,
                    address,
                    customer_threads,
                    customer_messages,
                };

                match tx.try_send(presta_payload) {
                    Ok(_) => {
                        info!("SENT PRESTASHOP DATA");
                        drop(tx);
                    }
                    Err(err) => error!("Error: {err:?}"),
                };
            });
        }
    }
}

pub async fn send_payload(
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
) -> anyhow::Result<Option<RecordId>, anyhow::Error> {
    info!("Send_Payload");
    let queried_salesman = query_user_from_email(ticket_data.salesman.clone()).await?;
    // let _queried_tech = query_user_from_email(ticket_data.tech.clone()).await?;

    let task_id = task_data.id.clone();
    let ticket_id = ticket_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();

    task_data.task_name = format!(
        "{} - {}",
        &customer_data.name,
        ticket_data.service_number.clone()
    );
    task_data.service_ticket = ticket_id.clone();
    task_data.service_number = Some(ticket_data.service_number.clone());
    task_data.priority = Priority::Normal;
    task_data.everest_initials = queried_salesman.everest_initials;
    task_data.assignee = queried_salesman.id;

    info!("Customer: {:?}", customer_data);
    if let Some(cust) = query_id(CUSTOMER_TABLE.to_string(), customer_id).await? {
        let update_cust_record: Vec<RecordId> = DATABASE
            .update(cust.key().to_string())
            .content(customer_data.clone())
            .await?;
        info!("Customer updated: {update_cust_record:?}");
        if send_specs {
            if let Some(computer_record) = query_id(COMPUTER_TABLE.to_string(), computer_id).await?
            {
                let create_computer_record: Vec<RecordId> = DATABASE
                    .update(computer_record.key().to_string())
                    .content(computer_data)
                    .await?;
                info!("create_computer_record: {create_computer_record:?}");
            } else {
                let create_computer_record: Option<RecordId> = DATABASE
                    .create(COMPUTER_TABLE)
                    .content(computer_data)
                    .await?;
                info!("create_computer_record: {create_computer_record:?}");
            }
        }
        info!("Ticket: {:?}", ticket_data);

        if let Some(ticket) = query_id(TICKET_TABLE.to_string(), ticket_id).await? {
            let service_ticket_record: Vec<RecordId> =
                DATABASE.update(ticket.key().to_string()).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        } else {
            let service_ticket_record: Option<RecordId> =
                DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }
    } else {
        match DATABASE
            .create::<Option<RecordId>>(CUSTOMER_TABLE)
            .content(customer_data.clone())
            .await
        {
            Ok(create_cust_record) => info!("Created RecordId: {create_cust_record:?}"),
            Err(e) => error!("Error with create_cust_record: {e:?}"),
        }
        match DATABASE
            .create::<Option<RecordId>>(COMPUTER_TABLE)
            .content(computer_data)
            .await
        {
            Ok(create_computer_record) => info!("Created RecordId: {create_computer_record:?}"),
            Err(e) => error!("Error with create_computer_record: {e:?}"),
        }
        match DATABASE
            .create::<Option<RecordId>>(TICKET_TABLE)
            .content(ticket_data)
            .await
        {
            Ok(create_ticket_record) => info!("Created RecordId: {create_ticket_record:?}"),
            Err(e) => error!("Error with create_ticket_record: {e:?}"),
        }
    }

    info!("Task: {:?}", task_data);

    let create_task_record: Option<RecordId> = DATABASE.create(TASK_TABLE).content(task_data).await?;
    info!("create_task_record: {create_task_record:?}");

    if task_notes.len() > 0 {
        info!("Task Notes: {:?}", task_notes);
        let mut note_ids = Vec::new();

        for mut note in task_notes {
            note.task_id = task_id.clone();
            let create_task_note_record: Option<RecordId> =
                DATABASE.create(TASK_NOTE_TABLE).content(note).await?;
            info!("create_task_note_record: {:?}", create_task_note_record);
            if let Some(note_record) = create_task_note_record {
                note_ids.push(note_record.key().to_string().clone());
            }
        }

        if let Some(ref record) = create_task_record {
            let update_task: Option<RecordId> = DATABASE
                .query("UPDATE $task SET task_note += $notes")
                .bind(("task", record.key().to_string().clone()))
                .bind(("notes", note_ids))
                .await?
                .take(0)?;

            info!("Update_task with notes: {update_task:?}");
        }
    }

    Ok(create_task_record)
}
