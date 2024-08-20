use database::{schema::{prestashop_schema::{Address, Customer, CustomerMessage, CustomerThread, Employee, Order, Prestashop, PrestashopPayload}, CustomerData, CustomerId, Priority, Record, Status, TaskNotePayload, TaskPayload, TicketData, TicketId, User, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE}, DATABASE};
use displays::ui_tools::autocomplete::AutoCompleteTextEdit;
use eframe::egui::{Align, Button, Color32, ComboBox, FontId, Layout, Margin, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use super::{task_modal::{display_task_page, ModalAction}, ModalState};
// use displays::ui_tools::autocomplete::AutoCompleteTextEdit;
use egui_extras::{DatePickerButton, Size, StripBuilder};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use crate::utilities::{get_data::get_user_from_email, DisplayModal, ModalTypes};
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::vec2;
use std::collections::{BTreeSet, HashMap};
use wasm_bindgen_futures::spawn_local;
use surrealdb::sql::Thing;
use serde::Serialize;
use log::info;

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal{
    pub title: String,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,  
    pub store_users: Option<Vec<User>>,

    pub task_name: String,
    pub task_priority: Priority,
    pub due_date: NaiveDate,
    pub description: String,
    pub assignee: String,
    #[serde(skip)]
    pub tur: Tur,
    #[serde(skip)]
    pub state: ModalState
}

#[derive(Debug, Clone)]
pub struct Tur{
    pub prestashop_api_rx: Receiver<PrestashopPayload>,
    pub prestashop_api_tx: Sender<PrestashopPayload>, 
    pub data: PrestashopPayload,
    pub ticket_data: TicketData,
    pub task_data: TaskPayload,
    pub customer_data: CustomerData,
    pub task_notes: Vec<TaskNotePayload>,
    pub store_users: Option<Vec<User>>,
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
            tur: Tur::default(),
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
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction>{
        let mut response: Option<ModalAction> = None;
        let avail_size = Vec2::new(680.,500.);
        
        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(Align::Center))
            .size(Size::exact(30.0))
            .size(Size::exact(10.0))
            .size(Size::relative(0.8))
            .vertical(|mut strip| 
        {
            strip
                .strip(|strip| 
            {
                strip
                    .size(Size::exact(avail_size.x/3.0))
                    .size(Size::remainder())
                    .size(Size::exact(avail_size.x/3.0))
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .horizontal( |mut strip| 
                {
                    strip.empty();
                    strip.cell(|ui|
                    {
                        ui.horizontal_top(|ui|
                        {
                            let mut main_page = false;
                            let mut import_task_page = false;
                            match current_page_state{
                                ModalAction::TicketInfoPage => {main_page = true},
                                ModalAction::ImportTask => {import_task_page = true},
                                _ => {main_page = true},
                            };

                            ui.add_space(90.0);

                            if ui.selectable_label(main_page, RichText::new("🖹").heading()).clicked(){
                                response = Some(ModalAction::TicketInfoPage);
                            };
                            if ui.selectable_label(import_task_page, RichText::new("🖥").heading()).clicked(){
                                response = Some(ModalAction::ImportTask);
                            };
                        });
                    });
                    strip.empty();
                });
            });
            strip.empty();
            strip.strip(|strip| 
            {
                strip
                    .size(Size::exact(avail_size.x))
                    .horizontal( |mut strip| 
                {
                    strip.strip(|s| 
                    {
                        s
                            .size(Size::remainder())
                            .size(Size::exact(avail_size.x / 2.0))
                            .size(Size::remainder())
                            .cell_layout(Layout::top_down(Align::Center))
                            .cell_layout(Layout::top_down(Align::Center))
                            .cell_layout(Layout::top_down(Align::Center))
                            .horizontal(|mut s|
                        {
                            s.empty();
                            s.cell(|ui| {
                                ui.style_mut().override_font_id = Some(FontId::proportional(13.0));
                                match current_page_state{
                                    ModalAction::TicketInfoPage => self.create_task(ui, avail_size),
                                    ModalAction::ImportTask => display_task_page(ui, &mut self.tur.task_data, avail_size),
                                    _ => self.create_task(ui, avail_size)
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
    pub fn create_task(&mut self, ui: &mut Ui, avail_size: Vec2) {
        StripBuilder::new(ui)
            .size(Size::exact(avail_size.y / 4.0))
            .size(Size::exact(115.0))
            .size(Size::exact(avail_size.y / 4.0 - 20.0))
            .vertical(|mut strip| 
        {
            strip.cell(|ui|  self.tur.tur_sheet(ui));

            strip.strip(|s|{
                s.size(Size::exact(70.0))
                    .size(Size::exact(35.0))
                    .size(Size::exact(150.0))
                    .vertical(|mut s|
                {
                    s.cell(|ui|{
                        TextEdit::singleline(&mut self.task_name)
                            .hint_text("Task Name")
                            .margin(Margin::symmetric(6.0, 4.0))
                            .desired_width(200.0)
                            .ui(ui);

                        let mut inputs = BTreeSet::new();
                        
                        if let Some(users) = &mut self.store_users{
                            for user in users.iter(){
                                let parsed = user.email.split_once("@").unwrap_or(("","")).0;
                                inputs.insert(parsed.to_string());
                            }
                            let _result = AutoCompleteTextEdit::new(&mut self.assignee, inputs.clone())
                                .highlight_matches(true)
                                .max_suggestions(3)
                                .set_text_edit_properties(move |text_edit| 
                            {
                                text_edit
                                    .hint_text("Assignee")
                                    .desired_width(200.0)
                                    .font(FontId::proportional(12.0))
                                    .frame(true)
                            }).ui(ui);
                        }
                    });
                    
                    s.cell(|ui| {
                        ui.horizontal_top(|ui| {
                            ui.add_space(80.0);

                            ui.scope(|ui| 
                            {
                                ui.style_mut().spacing.combo_width = 70.0;
                                ComboBox::new("PriorityComboBox", "")
                                    .selected_text(RichText::new(format!("{}", &self.task_priority.as_str())))
                                    .show_ui(ui, |ui| 
                                {
                                    for priority in Priority::VALUES{
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
                                let so = self.tur.ticket_data.service_number.clone();
                                let service_number = if !so.is_empty() {
                                    Some(so)
                                } else { None };

                                let notes: Vec<TaskNotePayload> = Vec::new();
                                let mut task_payload = TaskPayload{
                                    task_name: self.task_name.clone(),
                                    task_description: self.description.clone(),
                                    due_date: y,
                                    priority: self.task_priority.clone(),
                                    task_note: notes,
                                    completed: false,
                                    status: Status::Todo,
                                    service_number,
                                    ..Default::default()
                                };

                                let assignee = self.assignee.clone();

                                spawn_local(async move{
                                    let email = format!("{assignee}@pclaptops.com");
                                    match get_user_from_email(email).await {
                                        Ok(user) => {
                                            if let Some(usr) = user {
                                                task_payload.assignee = usr.id;
                                                task_payload.everest_initials = usr.everest_initials;
                                            }
                                            let _: Vec<Record> = DATABASE
                                                .create(TASK_TABLE)
                                                .content(task_payload)
                                                .await
                                                .unwrap();
                                        },
                                        Err(e) => info!("Error getting user: {e:?}"),
                                    }
                                }); 
                            }
                        });
                    });
                });
            });
            strip.empty();
        });
    }
}

impl Default for Tur {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded::<PrestashopPayload>();
        Self {
            prestashop_api_tx: tx,
            prestashop_api_rx: rx,
            ticket_data: TicketData::default(),
            task_data: TaskPayload::default(),
            customer_data: CustomerData::default(),
            task_notes: Vec::new(),
            store_users: None,
            data: PrestashopPayload::default(),
        }
    }
}

impl Tur {
    pub fn set_store_users(&mut self, users: Option<Vec<User>>) -> &mut Self {
        self.store_users = users;
        self
    }

    pub fn tur_sheet(&mut self, ui: &mut Ui) {
        // ui.horizontal_top(|ui| {     
            let check = !self.ticket_data.service_number.is_empty();
            let stroke = Stroke::new(1.0, Color32::from_rgb(191, 33, 101));
            let txt_color = Color32::from_rgb(255, 204, 255);
            let txt = RichText::new("Pull Order").color(txt_color);
            let button_size = Vec2::new(120.0, 25.0);
            let button = Button::new(txt).stroke(stroke).min_size(button_size);

            if ui.add_enabled(check, button).clicked() {
                let service_num = self.ticket_data.service_number.clone();
                self.presta_api();
                self.ticket_data = TicketData::default();
                self.task_data = TaskPayload::default();
                self.customer_data = CustomerData::default();
                // self.task_notes = Vec::new::<Vec<TaskNotePayload>>();
                self.ticket_data.service_number = service_num;
            }

            ui.add_space(15.0);
            ui.set_width(ui.available_width()/3.0);
            ui.shrink_width_to_current();

            TextEdit::singleline(&mut self.ticket_data.service_number)
                .hint_text("Service #  ")
                .char_limit(8)
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(vec2( 120.0,14.0))
                .ui(ui);
            
            if let Ok(data) = self.prestashop_api_rx.try_recv(){
                self.data = data.clone();
                let customer = &mut self.customer_data;
                let ticket = &mut self.ticket_data;
                let _task = &mut self.task_data;
                let task_notes = &mut self.task_notes;
                
                let service_details = data.order.associations.order_service;
                let mut services: Vec<TicketId> = Vec::new();
    
                let sales_rep = data.sales_rep.unwrap_or_default();
                let split_rep = data.split_rep.unwrap_or_default();
                let email = sales_rep.email.split_once("@").clone().unwrap_or(("!! Getting Tech !!", "")).0.to_string();
                let email_split_rep = split_rep.email.split_once("@").clone().unwrap_or(("!! Getting Salesman !!", "")).0.to_string();
    
                for msg in data.customer_messages{
                    task_notes.push(TaskNotePayload{
                        everest_initials: msg.id_employee,
                        note: msg.message,
                        ..Default::default()
                    })
                }
    
                customer.id = data.customer.id;
                customer.cust_code = data.customer.cust_code;
                customer.email = data.customer.email;
                customer.name = data.customer.name.clone();
                customer.phone_number = data.customer.phone_number;
                ticket.salesman = email_split_rep;
                ticket.tech = email;
                ticket.customer = customer.id.clone();
    
                ticket.id = Some(TicketId(Thing::from((TICKET_TABLE.to_string(), ticket.service_number.clone()))));
    
                if let Some(ticket_id) = &ticket.id {
                    services.push(ticket_id.clone());
                }
    
                if let Some(service) = service_details{
                    if service.len() == 1{
                        let svc = service.get(0);
                        if let Some(service) = svc {
                            ticket.checkin_notes = service.check_in_notes.clone();
                        }
                    }else{
                        info!("Theres a couple.... {:?}", service);
                    }
                }
            }
        // });
    }

    pub fn presta_api(&mut self){ 
        let input = self.ticket_data.service_number.clone();
        let tx = self.prestashop_api_tx.clone();
        if !input.is_empty() {
            spawn_local(async move {
                let api_call = Prestashop::default();
                let mut query = HashMap::new();

                query.insert("filter[id_order]", input.as_str());
                query.insert("output_format", "JSON");
                
                let customer_threads: Vec<CustomerThread> = api_call.request_resources(
                    "customer_threads",
                    query.clone()
                ).await.unwrap_or_default();

                let mut customer_messages: Vec<CustomerMessage> = Vec::new();

                if !customer_threads.is_empty(){
                    for thread in customer_threads.iter(){
                        for msg in thread.associations.customer_messages.iter(){
                            customer_messages.push(
                                api_call.request_subresources_by_id(
                                    "customer_messages", 
                                    "customer_message",
                                    msg.id.as_str()
                                ).await.unwrap_or_default()
                            );
                        }
                    }
                }

                let order: Order = api_call.request_subresources_by_id(
                    "orders", 
                    "order", 
                    &input
                ).await.unwrap_or_default();

                if order.id_customer.is_empty(){
                    info!("Order is likely gonna fuKKKK");
                }

                info!("order: {order:#?}");

                let sales_rep: Option<Employee> = if !order.id_employee_sales_rep.contains("0"){
                    let employee: Employee = api_call.request_subresources_by_id(
                        "employees", 
                        "employee", 
                        &order.id_employee_sales_rep
                    ).await.unwrap_or_default();

                    info!("employee: {employee:#?}");
                    Some(employee)
                }else{
                    None
                };
                let split_rep: Option<Employee> = if !order.id_employee_split_rep.contains("0"){
                    let employee_2: Employee = api_call.request_subresources_by_id(
                        "employees", 
                        "employee", 
                        &order.id_employee_split_rep
                    ).await.unwrap_or_default();

                    info!("employee: {sales_rep:#?}");
                    Some(employee_2)
                }else{
                    None
                };


                let cust: Customer = api_call.request_subresources_by_id(
                    "customers", 
                    "customer", 
                    &order.id_customer
                ).await.unwrap_or_default();

                // info!("customer: {customer:#?}");

                let address: Address = api_call.request_subresources_by_id(
                    "addresses", 
                    "address", 
                    &order.id_address_invoice
                ).await.unwrap_or_default();

                // let notes: CustomerThread = api_call.request_subresources_by_id(
                //     "customer_threads", 
                //     "customer_thread", 
                //     &order.id_address_delivery
                // ).await.unwrap();

                info!("address: {address:#?}");
                // 2059728
                let customer = CustomerData{
                    id: Some(CustomerId(Thing::from((CUSTOMER_TABLE.to_string(), order.id_customer.clone())))),
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
                    customer_messages
                };

                match tx.try_send(presta_payload){
                    Ok(_) => drop(tx),
                    Err(err) => info!("Error: {err:?}"),
                };
            });
        }
    }
}