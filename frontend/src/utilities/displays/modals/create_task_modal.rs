use std::collections::{BTreeSet, HashMap};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use crossbeam::channel::{Receiver, Sender};
use database::{schema::{prestashop_schema::{Address, Customer, CustomerMessage, CustomerThread, Employee, Order, PrestashopPayload, SubResource}, ComputerId, CustomerData, CustomerId, LiveTaskPayload, Priority, Record, Status, TaskNotePayload, TaskPayload, TicketData, TicketId, User, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE}, DATABASE};
use eframe::egui::{Align, Button, Color32, ComboBox, Direction, FontId, Layout, Margin, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use eframe::egui::{vec2, Grid, ScrollArea};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use log::info;
use reqwest::{header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE}, Client};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use surrealdb::sql::Thing;
use wasm_bindgen_futures::spawn_local;

use crate::utilities::{ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, ModalTypes};

use super::{task_modal::ModalAction, ModalState};

const AUTH_TOKEN: &str = "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06";

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
    pub assignee: Option<User>,
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
        let avail_size = Vec2::new(500.0,400.0);
        
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
                    .size(Size::remainder())
                    .horizontal( |mut strip| 
                {
                    strip.cell(|ui|{
                        ui.horizontal(|ui|{
                            
                            let mut main_page = false;
                            let mut import_task_page = false;
                            match current_page_state{
                                ModalAction::TicketInfoPage => {main_page = true},
                                ModalAction::ImportTask => {import_task_page = true},
                                _ => {main_page = true},
                            };

                            ui.add_space(200.0);

                            if ui.selectable_label(main_page, RichText::new("🖹").heading()).clicked(){
                                response = Some(ModalAction::TicketInfoPage);
                            };
                            if ui.selectable_label(import_task_page, RichText::new("🖥").heading()).clicked(){
                                response = Some(ModalAction::ImportTask);
                            };
                        });
                    });
                    
                });
            });
            strip.empty();
            strip
                .strip(|strip| 
            {
                strip
                    .size(Size::exact(avail_size.y))
                    .horizontal( |mut strip| 
                {
                    strip
                        .strip(|s| 
                    {
                        s
                            .size(Size::exact(15.0))
                            .size(Size::exact(avail_size.x))
                            .size(Size::exact(15.0))
                            .cell_layout(Layout::top_down(Align::Center))
                            .cell_layout(Layout::top_down(Align::Center))
                            .cell_layout(Layout::top_down(Align::Center))
                            .vertical(|mut s|
                        {
                            s.empty();
                            s.cell(|ui| {
                                ui.horizontal_centered(|ui| {
                                    ui.style_mut().override_font_id = Some(FontId::proportional(13.0));
                                    match current_page_state{
                                        ModalAction::TicketInfoPage => self.create_task(ui),
                                        ModalAction::ImportTask => ui.vertical_centered(|ui| self.tur.set_store_users(self.store_users.clone()).tur_sheet(ui)).inner,
                                        _ => self.create_task(ui)
                                    };
                                });
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
    pub fn create_task(&mut self, ui: &mut Ui) {
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
        let check = !self.ticket_data.service_number.is_empty();
        let stroke = Stroke::new(1.0, Color32::from_rgb(191, 33, 101));
        let txt_color = Color32::from_rgb(255, 204, 255);
        let txt = RichText::new("Get PrestaShop Order").color(txt_color);
        let button_size = Vec2::new(145.0, 25.0);
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
        ui.horizontal(|ui| ui.add_space(250.0));
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
            
        let button2 = Button::new(RichText::new("Submit TUR").color(txt_color)).min_size(Vec2::new(width, 20.0)).stroke(stroke);

        if ui.add_enabled(check,button2).clicked() {  
            // self.submit_tur();
        }

        let check = !self.ticket_data.service_number.is_empty()
            && !self.customer_data.name.is_empty()
            && !self.customer_data.phone_number.is_empty()
            && !self.ticket_data.tech.is_empty();

        let button3 = Button::new( RichText::new("Master-Tech.app")).min_size(Vec2::new(width, 20.0));

        if ui.add_enabled(check, button3).clicked() {  
        // self.submit_tur_mastertech(); 
        }

        ScrollArea::new([false, true])
        .id_source("checkin_notes_scroll")
        .show(ui, |ui|{
            let _ = TextEdit::multiline(&mut self.ticket_data.checkin_notes)
            .hint_text(RichText::new("Checkin Notes").weak())
            .font(FontId::proportional(15.0))
            .desired_rows(4).ui(ui);
        });
        ScrollArea::new([false, true])
        .id_source("recomendations_scroll")
        .show(ui, |ui|{
            let _ = TextEdit::multiline(&mut self.task_data.task_description)
            .hint_text(RichText::new("Recommendations").weak())
            .font(FontId::proportional(15.0))
            .desired_rows(4).ui(ui);
        });

        ScrollArea::new([false, true])
        .id_source("data_scroll")
        .show(ui, |ui|{
            TextEdit::multiline(&mut format!("{:?}", self.data))
            .ui(ui);
        });


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

            // self.output_text += &serde_json::to_string_pretty(&ticket).unwrap_or("".to_string());
            // self.output_text += &serde_json::to_string_pretty(&customer).unwrap_or("".to_string());
            // self.output_text += &serde_json::to_string_pretty(&computer).unwrap_or("".to_string());
        }

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
pub struct Prestashop<'a>{
    client: Client,
    /// [field1,field2 …] or 'full'
    display: &'a str,
    /// &schema=synopsis for tests
    schema: Option<&'a str>, 
    /** 
    * [1|5]	    OR operator: list of possible values
    * [1,10]    Interval operator: define interval of possible values
    * [John]	Literal value (not case sensitive)
    * [Jo]%	    Begin operator: fields begins with the value (not case sensitive)
    * %[hn]	    End operator: fields ends with the value (not case sensitive)
    * %[oh]%	Contains operator: fields contains the value (not case sensitive)
    */ 
    filter: Option<&'a str>,
    /// number, or starting index (limit from number to the index)
    limit: Option<(i32, i32)>,
    // data_channel: PrestaDataChannel
}

impl <'a> Default for Prestashop<'a>{
    fn default() -> Self {
        Self {
            client: Client::new(),
            schema: None,
            display: "full",
            filter: None,
            limit: None,
        }
    }
}

impl <'a>Prestashop<'a> {
    pub fn new<T: for <> Deserialize<'a> + std::fmt::Debug + SubResource>(
        client: Client, display: &'a str, filter: Option<&'a str>, limit: Option<(i32, i32)>, schema: Option<&'a str>,
    ) -> Self { Self { client, display, filter, limit, schema } }

    pub fn query_args(&self, resource_name: &str, url_params: HashMap<&str, &str>) -> String {
        let base_url = format!("https://pclaptops.mojo11.com/api/{}", resource_name);
        
        let mut query_params = vec![];

        // Adding `display` parameter
        if !self.display.is_empty() {
            query_params.push(format!("display={}", self.display));
        }

        // Adding `schema` parameter if present
        if let Some(ref schema) = self.schema {
            query_params.push(format!("schema={}", schema));
        }

        // Adding `filter` parameter if present
        if let Some(ref filter) = self.filter {
            query_params.push(format!("filter[{}]={}", resource_name, filter));
        }

        // Adding `limit` parameter if present
        if let Some((start, end)) = self.limit {
            query_params.push(format!("limit={},{}", start, end));
        }

        // Adding other URL parameters
        for (key, value) in url_params {
            query_params.push(format!("{}={}", key, value));
        }

        // Constructing the final URL
        let query_string = if !query_params.is_empty() {
            format!("?{}", query_params.join("&"))
        } else {
            String::new()
        };

        format!("{}{}", base_url, query_string)
    }

    pub async fn request_subresources_by_id<T>(
        &self, 
        resource: &str, 
        name: &str, 
        id: &str
    ) 
        -> anyhow::Result<T, anyhow::Error>
            where T: for <'de>Deserialize<'de> + std::fmt::Debug
    {
        let url = format!("https://pclaptops.mojo11.com/api/{resource}/{id}?output_format=JSON");
        let response: Value = self.client 
            .get(url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .json()
            .await?;

        info!("query:{url}\nresponse: {:#?}", response);

        let x: T = from_value(response[name].clone())?;
        info!("x: {x:#?}");
        Ok(x)
    }

    pub async fn request_resources<T>(
        &self, 
        resource_name: &str,
        url_params: HashMap<&str, &str>
    ) 
        -> anyhow::Result<Vec<T>, anyhow::Error>
            where T: for <'de>Deserialize<'de> + std::fmt::Debug
    {
        info!(
            "resource_name: {resource_name:#?}, {url_params:#?}\nURL: {:#?}", 
            self.query_args(resource_name, url_params.clone())
        );
        
        let response: Value = self.client.get(self.query_args(resource_name, url_params))
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .json()
            .await?;
        
        info!("response: {:#?}", response);
        let x: Vec<T> = from_value(response[resource_name].clone())?;
        info!("x: {x:#?}");
        
        Ok(x)
    }
}