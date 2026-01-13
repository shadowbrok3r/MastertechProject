use eframe::egui::{Align, Align2, Area, Button, Color32, ComboBox, Direction, FontId, Frame, Id, Layout, Margin, Order, RichText, ScrollArea, Spinner, TextEdit, TopBottomPanel, Ui, UiBuilder, Vec2, Widget};
use crate::{chats::ChatView, get_current_user_from_auth, get_database_users, ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, Interaction, PlatformSpawner, Spawner};
use database::schema::{utilities::{delete_task, get_prestashop_payload, PhoneNumberFormatter}, CarboniteResponse, ComputerData, CustomerData, LiveTaskPayload, RecordId, RecordIdExt, Store, TaskHistory, TaskNotePayload, TicketData, User, COMPUTER_TABLE};
use database::schema::prestashop::{Prestashop, Customer, Address, OrderState};
use database::schema::prestashop::xml::{modify_xml, remove_xml_tag};
use database::schema::prestashop_schema::PrestashopPayload;
use database::schema::helper_traits::parse_email_user;
use reqwest::{header::{ACCEPT, CONTENT_TYPE}, Client};
use crossbeam::channel::{Receiver, Sender};
use rfd::{AsyncFileDialog, FileHandle};
use egui_extras::{Size, StripBuilder};
use std::collections::BTreeSet;
use serde_json::Value;
use serde::Serialize;
use std::sync::Arc;
use bytes::Bytes;
use core::f32;
use log::info;

use super::tabs::{display_computer_page_with_search, display_history_page, display_job_builder_page, display_software_page, display_ticket_page, ComputerSearchData};

#[cfg(target_arch="wasm32")]
use std::sync::Mutex;

#[cfg(not(target_arch="wasm32"))]
use tokio::sync::Mutex;


// use super::ModalState;

#[derive(Serialize, Clone, Debug)]
pub struct TaskModal {
    pub task: LiveTaskPayload,
    pub service_ticket: Option<TicketData>,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub title: String,
    pub current_page_state: ModalAction,
    #[serde(skip)]
    pub chat_view: ChatView,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub spo: SpecialPartOrder,
    store_users: Vec<User>,
    user: User,
    #[serde(skip)]
    pub service_ticket_tx: Sender<TicketData>,
    #[serde(skip)]
    pub service_ticket_rx: Receiver<TicketData>,
    #[serde(skip)]
    pub customer_tx: Sender<CustomerData>,
    #[serde(skip)]
    pub customer_rx: Receiver<CustomerData>,
    #[serde(skip)]
    pub computer_tx: Sender<ComputerData>,
    #[serde(skip)]
    pub computer_rx: Receiver<ComputerData>,
    #[serde(skip)]
    pub initial_notes_tx: Sender<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub initial_notes_rx: Receiver<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub task_history_tx: Sender<Vec<TaskHistory>>,
    #[serde(skip)]
    pub task_history_rx: Receiver<Vec<TaskHistory>>,
    /// Cached task history records
    pub task_history: Vec<TaskHistory>,
    /// SEB check channels
    #[serde(skip)]
    pub seb_tx: Sender<Vec<CarboniteResponse>>,
    #[serde(skip)]
    pub seb_rx: Receiver<Vec<CarboniteResponse>>,
    /// SEB check in progress flag
    pub seb_checking: bool,
    /// Prestashop order data for resync functionality
    pub prestashop_data: Option<PrestashopPayload>,
    /// Channel for receiving resynced prestashop data
    #[serde(skip)]
    pub resync_tx: Sender<PrestashopPayload>,
    #[serde(skip)]
    pub resync_rx: Receiver<PrestashopPayload>,
    /// Flag indicating resync is in progress
    pub resyncing: bool,
    
    // Customer change modal state
    pub customer_modal_open: bool,
    pub customer_search_query: String,
    pub customer_search_type: CustomerSearchType,
    pub customer_search_results: Vec<(Customer, Address)>,
    pub customer_searching: bool,
    #[serde(skip)]
    pub customer_search_tx: Sender<Vec<(Customer, Address)>>,
    #[serde(skip)]
    pub customer_search_rx: Receiver<Vec<(Customer, Address)>>,
    
    // Computer selection state
    pub customer_computers: Vec<ComputerData>,
    pub computer_search_query: String,
    pub computer_search_inputs: BTreeSet<String>,
    #[serde(skip)]
    pub computers_tx: Sender<Vec<ComputerData>>,
    #[serde(skip)]
    pub computers_rx: Receiver<Vec<ComputerData>>,
}

#[derive(Debug, Default, PartialEq, Clone, Serialize)]
pub enum CustomerSearchType {
    #[default]
    Email,
    Phone,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub enum ModalAction {
    #[default]
    TicketInfoPage,
    PartOrderPage,
    SoftwareInfoPage,
    ComputerInfoPage,
    JobBuilderPage,
    TaskNotePage,
    TaskHistoryPage,
    ImportTask,
    Close,
    // TaskPage,
    None,
}

impl TaskModal {
    pub fn new(chat_view: ChatView, task: LiveTaskPayload) -> Self {
        let (service_ticket_tx, service_ticket_rx) = crossbeam::channel::unbounded();
        let (customer_tx, customer_rx) = crossbeam::channel::unbounded();
        let (computer_tx, computer_rx) = crossbeam::channel::unbounded();
        let (initial_notes_tx, initial_notes_rx) = crossbeam::channel::unbounded();
        let (task_history_tx, task_history_rx) = crossbeam::channel::unbounded();
        let (seb_tx, seb_rx) = crossbeam::channel::unbounded();
        let (resync_tx, resync_rx) = crossbeam::channel::unbounded();
        let (customer_search_tx, customer_search_rx) = crossbeam::channel::unbounded();
        let (computers_tx, computers_rx) = crossbeam::channel::unbounded();
        let comp_tx = computer_tx.clone();
        let cust_tx = customer_tx.clone();
        let svc_tx = service_ticket_tx.clone();
        let notes_tx = initial_notes_tx.clone();
        let history_tx = task_history_tx.clone();
        let computers_tx_clone = computers_tx.clone();
        let id = task.id.clone();
        let service_number = task.service_number.clone();
        PlatformSpawner::spawn(async move {
            let mut customer_id: Option<String> = None;
            
            match TicketData::get_associated_ticket(id.clone()).await {
                Ok(ticket) => { let _ = svc_tx.try_send(ticket); },
                Err(e) => log::error!("Error getting ticket data: {e:?}"),
            }
            match ComputerData::get_associated_computer(id.clone()).await {
                Ok(computer) => { let _ = comp_tx.try_send(computer); },
                Err(e) => log::error!("Error getting computer data: {e:?}"),
            }
            match CustomerData::get_associated_customer(id.clone()).await {
                Ok(customer) => { 
                    customer_id = Some(customer.cust_code.clone());
                    let _ = cust_tx.try_send(customer); 
                },
                Err(e) => log::error!("Error getting customer data: {e:?}"),
            }
            match TaskNotePayload::get_db_notes_from_task_id(id.clone()).await {
                Ok(notes) => { let _ = notes_tx.try_send(notes); },
                Err(e) => log::error!("Error getting notes from task ID: {e:?}"),
            }
            // Fetch task history
            match TaskHistory::get_history_for_task(id.clone()).await {
                Ok(history) => { let _ = history_tx.try_send(history); },
                Err(e) => log::error!("Error getting task history: {e:?}"),
            }

            if let Some(service_number) = service_number {
                match TaskNotePayload::get_prestashop_notes_from_service(&service_number, Some(id.clone())).await {
                    Ok(notes) => { let _ = notes_tx.try_send(notes); },
                    Err(e) => log::error!("Error getting notes from task ID: {e:?}"),
                }
            }
            
            // Fetch customer's computers for computer search
            if let Some(cust_id) = customer_id {
                match ComputerData::get_computers_by_customer_id(cust_id).await {
                    Ok(computers) => { let _ = computers_tx_clone.try_send(computers); },
                    Err(e) => log::error!("Error getting customer computers: {e:?}"),
                }
            }
        });

        Self {
            title: task.task_name.clone(),
            current_page_state: ModalAction::TicketInfoPage,
            task,
            service_ticket: None,
            customer: None,
            computer: None,
            service_ticket_tx, service_ticket_rx,
            customer_tx, customer_rx,
            computer_tx, computer_rx,
            initial_notes_tx, initial_notes_rx,
            task_history_tx, task_history_rx,
            task_history: Vec::new(),
            seb_tx, seb_rx,
            seb_checking: false,
            prestashop_data: None,
            resync_tx, resync_rx,
            resyncing: false,
            // Customer change modal
            customer_modal_open: false,
            customer_search_query: String::new(),
            customer_search_type: CustomerSearchType::default(),
            customer_search_results: Vec::new(),
            customer_searching: false,
            customer_search_tx, customer_search_rx,
            // Computer selection
            customer_computers: Vec::new(),
            computer_search_query: String::new(),
            computer_search_inputs: BTreeSet::new(),
            computers_tx, computers_rx,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            chat_view,
            spo: SpecialPartOrder::default(),
            store_users: get_database_users(),
            user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            }
        }
    }

    fn receive(&mut self) {
        if let Ok(service_ticket) = self.service_ticket_rx.try_recv() {
            log::info!("Received ticket");
            self.service_ticket = Some(service_ticket);
        }
        
        if let Ok(mut customer) = self.customer_rx.try_recv() {
            log::info!("Received customer");
            let mut formatter = PhoneNumberFormatter::default();
            let phone_num = customer.phone_number.to_string();
            customer.phone_number = formatter.format_phone_number(&phone_num).unwrap_or(phone_num);
            self.customer = Some(customer);
        }
        
        if let Ok(computer) = self.computer_rx.try_recv() {
            log::info!("Received computer");
            self.computer = Some(computer);
        }

        if let Ok(notes) = self.initial_notes_rx.try_recv() {
            log::info!("Received notes: {}", notes.len());
            self.chat_view.set_notes(notes);
        }
        
        if let Ok(history) = self.task_history_rx.try_recv() {
            log::info!("Received task history: {} records", history.len());
            self.task_history = history;
        }
        
        // Handle SEB check results
        if let Ok(seb_results) = self.seb_rx.try_recv() {
            log::info!("Received SEB results: {} records", seb_results.len());
            self.seb_checking = false;
            
            // Update computer's seb_info if we got results
            if !seb_results.is_empty() {
                if let Some(computer) = self.computer.as_mut() {
                    // Convert CarboniteResponse to LocalSebData
                    if let Some(first_result) = seb_results.first() {
                        use database::schema::computer::seb::{LocalSebData, ExtendedSeb};
                        let seb_info = LocalSebData {
                            InstalledDeviceId: first_result.device_id.clone(),
                            InstallInstanceId: String::new(),
                            HasIssues: String::new(),
                            InstallationStage: first_result.state.clone(),
                            ReasonCode: String::new(),
                            ActivationCode: String::new(),
                            InstallVersion: String::new(),
                            MachineName: first_result.device_name.clone(),
                            ExtendedSeb: Some(ExtendedSeb {
                                email: first_result.email.clone(),
                                phone: first_result.phone.clone(),
                                userid: first_result.userid.clone(),
                                device_name: first_result.device_name.clone(),
                                device_id: first_result.device_id.clone(),
                                state: first_result.state.clone(),
                                usage_gb: first_result.usage_gb.clone(),
                                date_device_created: first_result.date_device_created.clone(),
                                activated: String::new(),
                                activation_code: String::new(),
                                last_complete_backup: String::new(),
                                last_client_status_update: String::new(),
                                id_recurly_account: String::new(),
                                date_last_scan: String::new(),
                                date_email_sent: String::new(),
                                date_canceled_account: String::new(),
                                date_deleted_account: String::new(),
                                current_period_ends_at: String::new(),
                                date_modified: String::new(),
                                date_created: String::new(),
                            }),
                        };
                        computer.seb_info = Some(seb_info);
                        log::info!("Updated computer with SEB info for device: {}", first_result.device_name);
                    }
                }
            }
        }
        
        // Handle resync results
        if let Ok(prestashop_data) = self.resync_rx.try_recv() {
            log::info!("Received resynced prestashop data for order: {}", prestashop_data.order.id);
            self.resyncing = false;
            
            // Update ticket data with new prestashop info
            if let Some(ticket) = self.service_ticket.as_mut() {
                let sales_rep = prestashop_data.sales_rep.clone().unwrap_or_default();
                let split_rep = prestashop_data.split_rep.clone().unwrap_or_default();
                let email = parse_email_user(&sales_rep.email).to_string();
                let email_split_rep = parse_email_user(&split_rep.email).to_string();
                
                ticket.salesman = email_split_rep;
                ticket.sales_rep = email.clone();
                ticket.tech = email.clone();
                ticket.terms = prestashop_data.order.payment.clone();
                ticket.ticket_total = prestashop_data.order.total_products_wt.clone();
                ticket.doc_alias = prestashop_data.order.order_type.clone();
                
                if let Some(service) = prestashop_data.order.associations.order_service.first() {
                    ticket.checkin_notes = service.check_in_notes.clone();
                }
            }
            
            // Update customer data
            if let Some(customer) = self.customer.as_mut() {
                customer.id = prestashop_data.customer.id.clone();
                customer.cust_code = prestashop_data.customer.cust_code.clone();
                customer.email = prestashop_data.customer.email.clone();
                customer.name = prestashop_data.customer.name.clone();
                customer.phone_number = prestashop_data.customer.phone_number.clone();
            }
            
            self.prestashop_data = Some(prestashop_data);
        }
        
        // Handle customer search results
        if let Ok(results) = self.customer_search_rx.try_recv() {
            self.customer_searching = false;
            self.customer_search_results = results;
        }
        
        // Handle customer computers list
        if let Ok(computers) = self.computers_rx.try_recv() {
            log::info!("Received {} computers for customer", computers.len());
            self.customer_computers = computers.clone();
            
            // Build the search inputs from computer CPU/hostname
            self.computer_search_inputs.clear();
            for comp in computers {
                // Add CPU + hostname as search option
                let search_str = if !comp.hostname.is_empty() && !comp.cpu.is_empty() {
                    format!("{} - {}", comp.hostname, comp.cpu)
                } else if !comp.hostname.is_empty() {
                    comp.hostname.clone()
                } else if !comp.cpu.is_empty() {
                    comp.cpu.clone()
                } else {
                    comp.id.key_string()
                };
                self.computer_search_inputs.insert(search_str);
            }
        }
    }
    
    /// Resync order data from Prestashop
    fn resync_from_prestashop(&mut self) {
        if let Some(service_number) = self.task.service_number.clone() {
            self.resyncing = true;
            let resync_tx = self.resync_tx.clone();
            PlatformSpawner::spawn(async move {
                match get_prestashop_payload(&service_number).await {
                    Ok(data) => {
                        let _ = resync_tx.try_send(data);
                    }
                    Err(e) => {
                        log::error!("Error resyncing from prestashop: {:?}", e);
                    }
                }
            });
        }
    }
    
    /// Display customer change modal
    pub fn show_customer_modal(&mut self, ui: &mut Ui) {
        if !self.customer_modal_open {
            return;
        }

        // Dim background
        let screen_rect = ui.ctx().screen_rect();
        ui.painter().rect_filled(
            screen_rect,
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 180),
        );

        Area::new(Id::new("task_customer_change_modal"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .order(Order::Foreground)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style())
                    .fill(Color32::from_rgb(30, 30, 35))
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        ui.set_min_width(400.0);
                        ui.set_min_height(300.0);
                        
                        ui.vertical(|ui| {
                            // Header
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new("Change Customer").color(Color32::WHITE));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("✕").clicked() {
                                        self.customer_modal_open = false;
                                    }
                                });
                            });
                            
                            ui.separator();
                            
                            // Show current customer info
                            if let Some(customer) = &self.customer {
                                ui.label(format!("Current Customer: [{}] {}", customer.cust_code, customer.name));
                            }
                            if let Some(service_number) = &self.task.service_number {
                                ui.label(format!("Order: {}", service_number));
                            }
                            
                            ui.add_space(10.0);
                            
                            // Search type selector
                            ui.horizontal(|ui| {
                                ui.label("Search by:");
                                ui.selectable_value(&mut self.customer_search_type, CustomerSearchType::Email, "Email");
                                ui.selectable_value(&mut self.customer_search_type, CustomerSearchType::Phone, "Phone");
                            });
                            
                            ui.add_space(5.0);
                            
                            // Search input
                            ui.horizontal(|ui| {
                                let hint = match self.customer_search_type {
                                    CustomerSearchType::Email => "Enter email address...",
                                    CustomerSearchType::Phone => "Enter phone number...",
                                };
                                
                                let response = TextEdit::singleline(&mut self.customer_search_query)
                                    .hint_text(hint)
                                    .desired_width(250.0)
                                    .ui(ui);
                                
                                let can_search = !self.customer_search_query.is_empty() && !self.customer_searching;
                                if ui.add_enabled(can_search, Button::new("Search")).clicked() || 
                                   (response.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) && can_search) 
                                {
                                    self.customer_searching = true;
                                    let query = self.customer_search_query.clone();
                                    let search_type = self.customer_search_type.clone();
                                    let tx = self.customer_search_tx.clone();
                                    
                                    PlatformSpawner::spawn(async move {
                                        let results = match search_type {
                                            CustomerSearchType::Email => Customer::find_customer_by_email(&query).await,
                                            CustomerSearchType::Phone => Customer::find_customer_by_phone(&query).await,
                                        };
                                        
                                        match results {
                                            Ok(customers) => { let _ = tx.try_send(customers); },
                                            Err(e) => log::error!("Customer search error: {:?}", e),
                                        }
                                    });
                                }
                                
                                if self.customer_searching {
                                    Spinner::new().size(16.0).ui(ui);
                                }
                            });
                            
                            ui.add_space(10.0);
                            
                            // Search results
                            if !self.customer_search_results.is_empty() {
                                ui.label(RichText::new(format!("Found {} customers:", self.customer_search_results.len()))
                                    .color(Color32::LIGHT_BLUE));
                                
                                ScrollArea::vertical()
                                    .max_height(200.0)
                                    .show(ui, |ui| {
                                        for (customer, address) in self.customer_search_results.iter() {
                                            let name = format!("{} {}", customer.firstname, customer.lastname);
                                            let addr_info = if !address.address1.is_empty() {
                                                format!("{}, {}", address.address1, address.city)
                                            } else {
                                                "No address".to_string()
                                            };
                                            
                                            ui.group(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.vertical(|ui| {
                                                        ui.label(RichText::new(format!("[{}] {}", customer.id, name)).strong());
                                                        ui.label(RichText::new(&customer.email).small().color(Color32::GRAY));
                                                        ui.label(RichText::new(&addr_info).small().color(Color32::GRAY));
                                                    });
                                                    
                                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                        if ui.button("Select").clicked() {
                                                            // Update order with new customer
                                                            if let Some(service_number) = self.task.service_number.clone() {
                                                                let order_id = service_number.clone();
                                                                let customer_id = customer.id.clone();
                                                                let address_id = address.id.clone();
                                                                let customer_name = name.clone();
                                                                
                                                                log::info!("Updating order {} with customer {} ({}), address {}", 
                                                                          order_id, customer_name, customer_id, address_id);
                                                                
                                                                // Update local customer data
                                                                if let Some(cust) = self.customer.as_mut() {
                                                                    cust.cust_code = customer_id.clone();
                                                                    cust.name = customer_name.clone();
                                                                    cust.email = customer.email.clone();
                                                                    cust.phone_number = address.phone.clone();
                                                                }
                                                                
                                                                // Async update to Prestashop
                                                                PlatformSpawner::spawn(async move {
                                                                    Self::update_order_customer(&order_id, &customer_id, &address_id).await;
                                                                });
                                                            }
                                                            
                                                            self.customer_modal_open = false;
                                                        }
                                                    });
                                                });
                                            });
                                        }
                                    });
                            } else if !self.customer_searching && !self.customer_search_query.is_empty() {
                                ui.label(RichText::new("No customers found.").color(Color32::GRAY));
                            }
                            
                            ui.add_space(10.0);
                            
                            // Cancel button
                            ui.horizontal(|ui| {
                                if ui.button("Cancel").clicked() {
                                    self.customer_modal_open = false;
                                }
                            });
                        });
                    });
            });
    }
    
    /// Update order customer and address in Prestashop
    async fn update_order_customer(order_id: &str, customer_id: &str, address_id: &str) {
        let api = Prestashop::default();
        
        // Get the current order XML
        match api.request_raw_resource_by_id("orders", order_id).await {
            Ok(xml) => {
                // Update id_customer
                match modify_xml(&xml, "id_customer", customer_id) {
                    Ok(xml_with_customer) => {
                        // Update id_address_invoice
                        match modify_xml(&xml_with_customer, "id_address_invoice", address_id) {
                            Ok(xml_with_address) => {
                                // Remove problematic tags
                                match remove_xml_tag(&xml_with_address, "tax_exempt") {
                                    Ok(final_xml) => {
                                        match api.modify_prestashop_order(&final_xml).await {
                                            Ok(_) => log::info!("Successfully updated order {} with customer {}", order_id, customer_id),
                                            Err(e) => log::error!("Error modifying prestashop order: {:?}", e),
                                        }
                                    }
                                    Err(e) => log::error!("Error removing tax_exempt tag: {:?}", e),
                                }
                            }
                            Err(e) => log::error!("Error modifying address in XML: {:?}", e),
                        }
                    }
                    Err(e) => log::error!("Error modifying customer in XML: {:?}", e),
                }
            }
            Err(e) => log::error!("Error getting order XML: {:?}", e),
        }
    }

}

impl DisplayModal for TaskModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction> {
        self.receive();
        let avail_size = Vec2::new(700.0, 700.0);
        let max_space = Vec2::new(715.0, 700.0);
        ui.set_min_size(max_space);
        ui.set_max_size(max_space);
        ui.style_mut().override_font_id = Some(FontId::proportional(13.0));

        TopBottomPanel::top(format!("Top panel header {}", self.task.id.key_string())).exact_height(28.).show_inside(ui, |ui| {

            ui.columns(3, |ui| {
                ui[0].with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let delete_btn = Button::new(
                        RichText::new("Delete Task").color(Color32::LIGHT_RED),
                    )
                    .min_size([70., 22.0].into())
                    .ui(ui)
                    .on_hover_text("Double Click To Delete Task");

                    if delete_btn.double_clicked() {
                        let task_id = self.task.id.clone();
                        PlatformSpawner::spawn(async move {
                            match delete_task(task_id).await {
                                Ok(_) => info!("Deleted task"),
                                Err(e) => log::error!("Error: {e:?}"),
                            }
                        });
                        self.current_page_state = ModalAction::Close;
                    }
                    
                    // Resync from Prestashop button
                    if self.task.service_number.is_some() {
                        let can_resync = !self.resyncing;
                        if ui.add_enabled(can_resync, Button::new(RichText::new("🔄").heading()).min_size([22., 22.].into()))
                            .on_hover_text("Resync Order from Prestashop")
                            .clicked() 
                        {
                            self.resync_from_prestashop();
                        }
                        if self.resyncing {
                            Spinner::new().size(16.0).ui(ui);
                        }
                    }
                });

                ui[1].vertical_centered(|ui| {
                    ui.horizontal_top(|ui| {
                        ui.add_space(55.);
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::TicketInfoPage,
                            RichText::new("🖹").heading()
                        ))
                        .on_hover_text("Ticket Info")
                        .clicked() {
                            self.current_page_state = ModalAction::TicketInfoPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::ComputerInfoPage,
                            RichText::new("🖥").heading()
                        ))
                        .on_hover_text("Computer Info")
                        .clicked() {
                            self.current_page_state = ModalAction::ComputerInfoPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::SoftwareInfoPage,
                            RichText::new("💾").heading()
                        ))
                        .on_hover_text("Software Info")
                        .clicked() {
                            self.current_page_state = ModalAction::SoftwareInfoPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::TaskHistoryPage,
                            RichText::new("📜").heading()
                        ))
                        .on_hover_text("Task History")
                        .clicked() {
                            self.current_page_state = ModalAction::TaskHistoryPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::TaskNotePage,
                            RichText::new("💬").heading()
                        ))
                        .on_hover_text("Task Notes")
                        .clicked() {
                            self.current_page_state = ModalAction::TaskNotePage;
                        }
                    });
                });

                ui[2].with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.push_id(
                        format!("Completed {}", self.task.completed),
                        |ui| self.task.interact_completed(ui),
                    );
                    ui.add_space(6.0);
                    ui.colored_label(Color32::LIGHT_RED, "Completed:");
                });
            });
        });

        ui.add_space(10.);
        ui.scope_builder(
        UiBuilder::new()
        .layout(Layout::from_main_dir_and_cross_align(Direction::LeftToRight, Align::Max)), 
        |ui| {
            let is_sizing_pass = ui.is_sizing_pass();
            let available_width = 715.;
            let total_content_width = match self.current_page_state {
                ModalAction::TicketInfoPage => 670.,
                ModalAction::SoftwareInfoPage => 670.,
                ModalAction::ComputerInfoPage => 670.,
                ModalAction::JobBuilderPage => 670.,
                ModalAction::TaskNotePage => 715.,
                ModalAction::TaskHistoryPage => 670.,
                _ => 715.
            };

            // In the rendering pass, add spacing to center the content
            if !is_sizing_pass && available_width > total_content_width {
                let padding = (available_width - total_content_width) / 2.0;
                ui.add_space(padding);
            }

            let store_users = self.store_users.clone();
            match self.current_page_state {
                ModalAction::TicketInfoPage   => display_ticket_page(
                    ui, 
                    &mut self.task, 
                    self.service_ticket.as_mut(), 
                    self.customer.as_mut(),
                    self.computer.as_mut(),
                    avail_size, 
                    &store_users, 
                    self.user.clone(),
                    Some(self.seb_tx.clone()),
                    &mut self.seb_checking,
                    Some(&mut self.customer_modal_open),
                ),
                ModalAction::ComputerInfoPage => {
                    let search_data = ComputerSearchData {
                        search_query: &mut self.computer_search_query,
                        search_inputs: &self.computer_search_inputs,
                        customer_computers: &self.customer_computers,
                        selected_computer: &mut None,
                    };
                    display_computer_page_with_search(
                        ui, 
                        self.service_ticket.as_mut(), 
                        self.computer.as_mut(), 
                        avail_size,
                        Some(search_data),
                    );
                },
                ModalAction::SoftwareInfoPage => display_software_page(ui, self.computer.as_mut().unwrap_or(&mut ComputerData::default()), avail_size),
                ModalAction::JobBuilderPage   => display_job_builder_page(ui),
                ModalAction::TaskNotePage     => self.chat_view.ui(ui),
                ModalAction::TaskHistoryPage  => display_history_page(ui, &self.task_history, avail_size),
                // ModalAction::TaskPage         => display_task_page(ui, &mut self.task, avail_size),
                _ => {}
            }

            let id = format!("{:?} Sizing ID", self.task.id);
            // Use egui memory to track if we've already done the sizing pass
            let sizing_pass_done = ui.memory(|mem| mem.data.get_temp::<bool>(Id::new(&id)).unwrap_or(false));

            if is_sizing_pass && !sizing_pass_done {
                ui.ctx().request_discard("Centering ComboBox sizing pass");
                ui.ctx().request_repaint();
                ui.memory_mut(|mem| mem.data.insert_temp(Id::new(&id), true));
            }

            // Reset the flag if the UI is repainted for other reasons (e.g., window resize)
            if !is_sizing_pass && sizing_pass_done {
                ui.memory_mut(|mem| mem.data.insert_temp(Id::new(&id), false));
            }
        });

        // Show customer change modal if open
        self.show_customer_modal(ui);
        
        if self.current_page_state == ModalAction::Close {
            action_handler(ModalAction::Close);
        }
        Some(self.current_page_state.clone())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecialPartOrder {
    customer_name: String,              //  "kathleen Hoffmon",
    customer_phone_number: String,      //  "801-888-8888",
    application_comment: String,        //  "These are some notes",
    system_order_number: String,        //  "123456",
    id_location: String,                //  "Riverdale",
    request_type: String,               //  "Any",
    shipping_method: String,            //  "2 - 2-3 Day Express",
    part_manufacturer: Manufacturer,    //  "PC Laptops",
    manufacturer_model_number: String,  //  "12345Test",
    manufacturer_serial_number: String, //  "123456789",
    manufacturer_part_number: String,   //  "324657687",
    part_color: String,                 //  "N/A",
    part_description: String,           //  "Test",
    part_lcd_toggle: bool,              //  "0"
    spo_status: SpoStatus,
    #[serde(skip)]
    files: Arc<Mutex<Option<Vec<FileHandle>>>>,
}

impl Default for SpecialPartOrder {
    fn default() -> Self {
        Self {
            customer_name: String::new(),
            customer_phone_number: String::new(),
            application_comment: String::new(),
            system_order_number: String::new(),
            id_location: "1".to_string(),
            request_type: String::new(),
            shipping_method: "2 - 2-3 Day Express".to_string(),
            part_manufacturer: Manufacturer::Pclaptops,
            manufacturer_model_number: String::new(),
            manufacturer_serial_number: String::new(),
            manufacturer_part_number: String::new(),
            part_color: "N/A".to_string(),
            part_description: String::new(),
            part_lcd_toggle: false,
            spo_status: SpoStatus::AwaitingQuote,
            files: Arc::new(Mutex::new(None)),
        }
    }
}
#[derive(PartialEq, Default, Debug, Serialize, Clone)]
pub enum SpoStatus {
    #[default]
    AwaitingQuote,
    QuoteFullfilled,
    OrderPendingDM,
}

#[derive(PartialEq, Default, Debug, Serialize, Clone)]
pub enum Manufacturer {
    #[default]
    Pclaptops,
    Other,
}

impl Manufacturer {
    pub fn as_str(&mut self) -> &str {
        match self {
            Manufacturer::Pclaptops => "PC Laptops",
            Manufacturer::Other => "Other",
        }
    }
}

impl SpoStatus {
    pub fn as_str(&mut self) -> &str {
        match self {
            SpoStatus::AwaitingQuote => "Awaiting Quote",
            SpoStatus::OrderPendingDM => "Pending DM",
            SpoStatus::QuoteFullfilled => "Quote Fullfilled",
        }
    }
}

impl SpecialPartOrder {
    pub fn set_customer(
        &mut self,
        customer_name: String,
        customer_phone_number: String,
        system_order_number: String,
    ) {
        self.customer_name = customer_name;
        self.customer_phone_number = customer_phone_number;
        self.system_order_number = system_order_number;
    }

    pub fn display_part_order_page(&mut self, ui: &mut Ui, avail_size: Vec2, location: Store) {
        StripBuilder::new(ui)
            .cell_layout(Layout::from_main_dir_and_cross_align(
                Direction::TopDown,
                Align::Center,
            ))
            .size(Size::exact(50.0))
            .size(Size::remainder())
            .size(Size::remainder())
            .vertical(|mut s| {
                s.empty();
                s.strip(|s| {
                    s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                        .size(Size::exact(avail_size.x / 3.2))
                        .size(Size::exact(200.0))
                        .horizontal(|mut s| {
                            s.empty();
                            s.cell(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.horizontal(|ui| {
                                        ComboBox::new("AwaitingQuoteCombo", "")
                                            .selected_text(self.spo_status.as_str())
                                            .width(50.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut self.spo_status,
                                                    SpoStatus::OrderPendingDM,
                                                    "Pending DM",
                                                );
                                                ui.selectable_value(
                                                    &mut self.spo_status,
                                                    SpoStatus::QuoteFullfilled,
                                                    "Quote Fullfilled",
                                                );
                                                ui.selectable_value(
                                                    &mut self.spo_status,
                                                    SpoStatus::AwaitingQuote,
                                                    "Awaiting Quote",
                                                );
                                            });
                                        ComboBox::new("ManufacturerCombo", "")
                                            .selected_text(self.part_manufacturer.as_str())
                                            .width(50.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut self.part_manufacturer,
                                                    Manufacturer::Pclaptops,
                                                    "PC Laptops",
                                                );
                                                ui.selectable_value(
                                                    &mut self.part_manufacturer,
                                                    Manufacturer::Other,
                                                    "Other",
                                                );
                                            });
                                    });

                                    ui.add_space(15.0);

                                    TextEdit::singleline(&mut self.manufacturer_model_number)
                                        .hint_text("MFG Model #".to_string())
                                        .margin(Margin::same(5))
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    TextEdit::singleline(&mut self.manufacturer_part_number)
                                        .hint_text("MFG P/N".to_string())
                                        .margin(Margin::same(5))
                                        .frame(true)
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    TextEdit::singleline(&mut self.part_description)
                                        .hint_text("Part Description".to_string())
                                        .margin(Margin::same(5))
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    TextEdit::multiline(&mut self.application_comment)
                                        .hint_text("Notes".to_string())
                                        .margin(Margin::same(5))
                                        .desired_rows(3)
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    // let mut task: Option<AsyncFileDialog> = None;

                                    ui.horizontal(|ui| {
                                        let toggle = ui.checkbox(&mut self.part_lcd_toggle, "LCD?");
                                        ui.add_space(ui.available_width() / 2.0);
                                        let file_upload =
                                            ui.selectable_label(false, "Upload Picture");

                                        if file_upload.clicked() {
                                            let data_clone = Arc::clone(&self.files);
                                            PlatformSpawner::spawn(async move {
                                                #[cfg(not(target_arch="wasm32"))]
                                                {
                                                    let mut data = data_clone.lock().await;
                                                    *data = AsyncFileDialog::new().pick_files().await;
                                                }

                                                #[cfg(target_arch="wasm32")]
                                                {
                                                    let mut data = data_clone.lock().unwrap();
                                                    *data = AsyncFileDialog::new().pick_files().await;
                                                }

                                            });
                                        };
                                        if toggle.clicked() {
                                            info!("self.part_lcd_toggle: {}", self.part_lcd_toggle);
                                        }
                                    });

                                    ui.add_space(15.0);

                                    ui.horizontal_top(|ui| {
                                        if Button::new("Submit")
                                            .min_size(Vec2::new(50.0, 20.0))
                                            .ui(ui)
                                            .clicked()
                                        {
                                            let location = match location {
                                                Store::RIV => "1".to_string(),
                                                Store::LTN => "2".to_string(),
                                                Store::MUR => "4".to_string(),
                                                Store::ORE => "8".to_string(),
                                                Store::SAN => "6".to_string(),
                                            };

                                            let spo = SpecialPartOrder {
                                                customer_name: self.customer_name.clone(),
                                                customer_phone_number: self
                                                    .customer_phone_number
                                                    .clone(),
                                                application_comment: self
                                                    .application_comment
                                                    .clone(),
                                                system_order_number: self
                                                    .system_order_number
                                                    .clone(),
                                                id_location: location,
                                                request_type: self.request_type.clone(),
                                                shipping_method: self.shipping_method.clone(),
                                                part_manufacturer: self.part_manufacturer.clone(),
                                                manufacturer_model_number: self
                                                    .manufacturer_model_number
                                                    .clone(),
                                                manufacturer_serial_number: self
                                                    .manufacturer_serial_number
                                                    .clone(),
                                                manufacturer_part_number: self
                                                    .manufacturer_part_number
                                                    .clone(),
                                                part_color: self.part_color.clone(),
                                                part_description: self.part_description.clone(),
                                                part_lcd_toggle: self.part_lcd_toggle.clone(),
                                                spo_status: self.spo_status.clone(),
                                                files: self.files.clone(),
                                            };

                                            let data_clone = Arc::clone(&self.files);

                                            PlatformSpawner::spawn(async move {
                                                let mut _bytes: Bytes = Bytes::new();
                                                let mut _file_name = String::new();
                                                #[cfg(not(target_arch="wasm32"))]
                                                {
                                                    let data = data_clone.lock().await;
                                                    if let Some(ref files) = *data {
                                                        for file_handle in files {
                                                            _file_name = file_handle.file_name();
                                                            _bytes = Bytes::copy_from_slice(
                                                                file_handle.read().await.as_slice(),
                                                            );
                                                            info!("file_name: {:?}", _file_name);
                                                        }
                                                    }
                                                }

                                                #[cfg(target_arch="wasm32")]
                                                {
                                                    let data = data_clone.lock().unwrap();
                                                    if let Some(ref files) = *data {
                                                        for file_handle in files {
                                                            _file_name = file_handle.file_name();
                                                            _bytes = Bytes::copy_from_slice(
                                                                file_handle.read().await.as_slice(),
                                                            );
                                                            info!("file_name: {:?}", _file_name);
                                                        }
                                                    }
                                                }

                                                let params: Value = serde_json::json!({
                                                    "user_email": "logan.lees@pclaptops.com",
                                                    "user_password": "Poolparty1",
                                                    "format_data": "text",
                                                    "action": "create",
                                                    "application": "customer_request_order",
                                                    "payload": spo,
                                                });

                                                let client = Client::new();
                                                client
                                                    .post(
                                                        "https://scaffold.pclaptops.com/api/index",
                                                    )
                                                    .header(CONTENT_TYPE, "application/json")
                                                    .header(ACCEPT, "application/json")
                                                    .json(&params)
                                                    .send()
                                                    .await
                                                    .unwrap();
                                            });
                                        }
                                    });
                                });
                            });
                        });
                });
                s.empty();
            });
    }
}

/*
 * 1 Riverdale [RIV]
 * 2 Layton [LTN]
 * 3 Salt Lake City [SLC]
 * 4 Murray [MUR]
 * 5 West Jordan [WJ]
 * 6 Sandy [SAN]
 * 7 American Fork [AF]
 * 8 Orem [ORE]
*/
