use crate::{channel_manager::ChannelManager, chats::ChatView, egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, DataTable, Renderer, RowViewer, UiAction}, Spawner};
use eframe::egui::{Button, CentralPanel, CollapsingHeader, Color32, ComboBox, Hyperlink, Id, KeyboardShortcut, Label, Layout, RichText, ScrollArea, Separator, SidePanel, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::{helper_traits::{parse_email_user, EmployeeHelper, TaskNotePayloadHelper}, prestashop_schema::{self, Employee, PrestashopPayload}, utilities::{create_full_task_payload, get_prestashop_payload, get_task_notes_from_db_with_service_number}, ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, User, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use crate::{app_state::SharedContext, PlatformSpawner};
use crossbeam::channel::{Receiver, Sender};
use serde::Serialize;
use std::collections::HashMap;
use itertools::Itertools;
use egui_extras::Column;
use surrealdb::RecordId;
use log::info;

const BASE_URL: &str = "https://pclaptops.mojo11.com/pcladmin/index.php?controller=AdminOrders&vieworder=&id_order=";
impl SharedContext {
    pub fn task_table_viewer(&mut self, ui: &mut Ui) {
        self.task_audit_table.show(ui, self.current_user.clone());
    }
}

/// Every logic is defined in `Viewer`
#[derive(Serialize)]
pub struct TaskRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    pub selected: Option<PrestashopPayload>,
    order_data: PrestashopPayload,
    open_hotkeys: bool,
    pub chat_view: ChatView,
    #[serde(skip)]
    notes_channel: (Sender<Vec<TaskNotePayload>>, Receiver<Vec<TaskNotePayload>>),
    #[serde(skip)]
    tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>)
}

impl Default for TaskRowViewer {
    fn default() -> Self {
        let notes_channel = <Vec<TaskNotePayload>>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        Self {
            notes_channel,
            tur_channel,
            filter: Default::default(),
            row_protection: Default::default(),
            hotkeys: Default::default(),
            selected: Default::default(),
            open_hotkeys: Default::default(),
            chat_view: Default::default(),
            order_data: PrestashopPayload::default()
        }
    }
}
pub struct TaskAuditViewer {
    audit_selection: TaskAudit,
    order_channel: (Sender<prestashop_schema::PrestashopPayload>, Receiver<prestashop_schema::PrestashopPayload>),
    pub services_viewer: TaskRowViewer,
    loading: bool,
    index: HashMap<String, i32>,
    time: Option<web_time::Instant>,
    pub service_map: HashMap<String, DataTable<PrestashopPayload>>,
}

impl TaskAuditViewer {
    pub fn new() -> Self {
        let order_channel = <prestashop_schema::PrestashopPayload>::create_unbounded_channel();
        Self {
            audit_selection: TaskAudit::default(),
            services_viewer: TaskRowViewer::default(),
            order_channel,
            loading: false,
            index: HashMap::new(),
            service_map: HashMap::new(),
            time: None
        }
    }

    fn show(&mut self, ui: &mut Ui, current_user: Option<User>) {
        SidePanel::right(Id::new("Task Audit Side Panel"))
            .default_width(280.)
            .max_width(900.)
            .resizable(true)
            .show_separator_line(true)
            .show_inside(ui, |ui| 
        {
            let service = self.services_viewer.selected.clone();

            let header = if let Some(service) = &service {
                &format!("{} - {}", service.customer.name, service.order.id)
            } else { "Task Details" };

            if let Some(order) = self.services_viewer.selected.clone() {
                ui.vertical_centered_justified(|ui| {
                    let service = self.services_viewer.selected.clone();

                    let header = if let Some(service) = &service {
                        &format!("{} - {}", service.customer.name, service.order.id)
                    } else { "Task Details" };
                    ui.add_space(5.);
                    ui.heading(header.to_uppercase());
                    Separator::default().horizontal().shrink(ui.available_width()/2.5).ui(ui);
                    ui.add_space(5.0);


                    ScrollArea::vertical()
                    .auto_shrink(true)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                if ui.button(RichText::new("Create Task").code().color(ui.style().visuals.error_fg_color)).clicked() {
                                    let tx = self.services_viewer.tur_channel.0.clone();
                                    let order_num = order.order.id.clone();
                                    PlatformSpawner::spawn(async move {
                                        let presta_order = TaskRowViewer::get_prestashop_order(order_num).await.unwrap_or_default();
                                        let _ = tx.try_send(presta_order);
                                    });
                                }
                                ui.add_space(10.);
                            });
                        });

                        ui.add_space(5.0);
                        ui.separator();
                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label("Status");

                            let status = match order.order.current_state.as_str() {
                                "30" => "In Repair",
                                "40" => "Done Shelf",
                                "4" => "Shipped",
                                "29" => "Check-in Shelf",
                                "239" => "Accepted by Odoo",
                                _ => ""
                            };

                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                ui.label(status);
                                ui.add_space(10.);
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label("Sales Rep");
                            let sales_rep = order.sales_rep.unwrap_or_default();
                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                ui.label(format!("{} {}", sales_rep.firstname, sales_rep.lastname));
                                ui.add_space(10.);
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label("Split Rep");
                            let split_rep = order.split_rep.unwrap_or_default();

                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                ui.label(format!("{} {}", split_rep.firstname, split_rep.lastname));
                                ui.add_space(10.);
                            });
                        });
                    });

                    // ui.add_space(ui.available_height());
                });
                ui.with_layout(Layout::bottom_up(eframe::egui::Align::Min), |ui| {
                    CollapsingHeader::new(
                        RichText::new("Order Notes")
                            .color(ui.style().visuals.error_fg_color)
                            .monospace()
                        )
                        .default_open(true)
                        .id_salt(format!("Order Notes - {}", header))
                        .show_unindented(ui, |ui| 
                    {
                        self.services_viewer.chat_view.ui(ui);
                    });
                });
            } else {
                ui.vertical_centered_justified(|ui| {

                    ui.add_space(5.);
                    ui.heading(header.to_uppercase());
                    Separator::default().horizontal().shrink(ui.available_width()/2.5).ui(ui);
                    ui.add_space(5.0);

                });
            }
        });

        TopBottomPanel::top("Task Audit Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.services_viewer.filter)
                    .hint_text(" Search for SO# / Customer")
                    .ui(ui);

                ui.add_space(10.);

                if Button::new(" Refresh ").ui(ui).clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let selection = selected.clone().as_str();

                    let start_idx = self
                        .index
                        .entry(selection.clone())
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&selection) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx);
                }
                ui.add_space(10.);
                if Button::new(" Load +10 ").ui(ui).clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let selection = selected.clone().as_str();

                    let start_idx = self
                        .index
                        .entry(selection.clone())
                        .and_modify(|i| *i+=10)
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&selection) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx);
                }
                ui.add_space(10.);
                let label = if self.services_viewer.open_hotkeys {
                    " Hide Hotkeys "
                } else {
                    " Show Hotkeys "
                };
                if Button::new(label).ui(ui).clicked() {
                    self.services_viewer.open_hotkeys = !self.services_viewer.open_hotkeys;
                }
            });
        });

        TopBottomPanel::bottom(Id::new("Task Audit Hot Keys"))
            .max_height(240.)
            .show_animated_inside(ui, self.services_viewer.open_hotkeys, |ui| 
        {
            ui.vertical_centered(|ui| ui.heading("Hotkeys"));
            ui.vertical_centered_justified(|ui| ui.separator());

            ui.horizontal_wrapped(|ui| {
                ui.style_mut().spacing.item_spacing.y = 5.0;
                ui.add_space(2.);
                let mut count = 0;
                for (k, a) in &self.services_viewer.hotkeys {
                    Button::new(format!("{a:?}"))
                        .min_size(Vec2::new(280., 25.))
                        .shortcut_text(
                            RichText::new(ui.ctx().format_shortcut(k))
                            .code()
                            .color(ui.style().visuals.warn_fg_color)
                        )
                        .ui(ui);
                    
                    count += 1;
                    if count % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            ui.horizontal(|ui| {
                ui.add_space(10.);

                let selected_text = self.audit_selection.to_owned().as_str();
                let selected = &mut self.audit_selection;
                let current_selection = selected.clone();

                ComboBox::new("Store_Selection", "")
                    .selected_text(selected_text.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(selected, TaskAudit::AllServices, " All Services ");
                        ui.selectable_value(selected, TaskAudit::CheckinShelf, " Check-in Shelf ");
                        ui.selectable_value(selected, TaskAudit::MyInRepair, " My In Repair ");
                        ui.selectable_value(selected, TaskAudit::InRepair, " In Repair ");
                        ui.selectable_value(selected, TaskAudit::DoneShelf, " Done Shelf ");
                        ui.selectable_value(selected, TaskAudit::MyServices, " My Services ");
                    })
                    .response;

                if current_selection != *selected {
                    self.loading = true;
                    let order_tx = self.order_channel.0.clone();
                    let selection = selected.clone().as_str();
                    let start_idx = self.index.entry(selection.clone()).or_insert(0).clone();
                    let svcs = if let Some(k) = self.service_map.get_mut(&selection) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    info!("Services from cache: {:?}", svcs.clone());
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user, order_tx, svcs, start_idx);
                }
            
                if self.loading {
                    ui.ctx().request_repaint();
                    ui.add_space(10.);
                    Spinner::new().color(ui.style().visuals.error_fg_color).ui(ui);
                }
                
                if let Some(time) = self.time.clone() {
                    if time.elapsed() > web_time::Duration::from_secs(5) {
                        self.loading = false;
                    }
                }
            });
            ui.add_space(5.);

            if let Some(table) = self.service_map.get_mut(&self.audit_selection.clone().as_str()) {
                Renderer::new(table, &mut self.services_viewer).ui(ui);
            }
        });  
    }

    pub fn get_services(
        selected: TaskAudit, 
        current_user: Option<User>, 
        order_tx: Sender<prestashop_schema::PrestashopPayload>, 
        current_orders: Vec<String>,
        start_idx: i32
    ) {
        let time = web_time::Instant::now();
        let usr = current_user.clone().unwrap_or_default();
        let id = usr.id_prestashop.unwrap_or_default();
        let mut employee = Employee::default();
        employee.id = format!("{id}");
        employee.id_store = usr.id_store.unwrap_or_default();
        match selected {
            TaskAudit::CheckinShelf => {
                PlatformSpawner::spawn(async move {
                    // Fetch services within the range
                    let orders = employee
                        .get_services_by_status("29", start_idx, start_idx+30)
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = employee.to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e)
                    };
                });
            },
            TaskAudit::MyInRepair => {
                PlatformSpawner::spawn(async move {
                    // Fetch services within the range            
                    let orders = employee
                        .get_my_services_in_repair()
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = employee.to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e)
                    };
                });
            },
            TaskAudit::InRepair => {
                PlatformSpawner::spawn(async move {
                    // Fetch services within the range
                    let orders = employee
                        .get_services_by_status("30", start_idx, start_idx+30)
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = employee.to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::info!("Error getting inrepair services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::info!("Error getting in repair shelf services: {:?}", e)
                    };
                });
            },
            TaskAudit::DoneShelf => {
                PlatformSpawner::spawn(async move {
                    // Fetch services within the range
                    let orders = employee
                        .get_services_by_status("40", start_idx, start_idx+30)
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = employee.to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::info!("Error with get_services_by_status 40: : {:?}", e)
                    };
                });
            },
            TaskAudit::AllServices => {
                PlatformSpawner::spawn(async move {
                    // Fetch services within the range
                    let orders = employee
                        .get_all_my_services()
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = employee.to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::info!("Error with get_all_services_in_my_store: {:?}", e)
                    };
                });
            },
            TaskAudit::MyServices => {
                PlatformSpawner::spawn(async move {
                    // Fetch services within the range
                    let orders = employee
                        .get_all_my_services()
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = employee.to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::info!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::info!("Error with get_my_services_in_repair: {:?}", e)
                    };
                });
            },
        }
        let elapsed = time.elapsed();
        info!("Time elapsed: {elapsed:?}");
    }

    pub fn receive(&mut self, current_user: User, store_users: Vec<User>, _frame: &mut eframe::Frame) {
        if let Ok(order) = self.order_channel.1.try_recv() {
            self.loading = true;
            let key = self.audit_selection.clone().as_str();

            self
                .service_map
                .entry(key.clone())
                .or_insert(DataTable::default());

            
            if let Some(k) = self.service_map.get_mut(&key) {
                if !k.iter().contains(&order) {
                    info!("Order: {order:?}");
                    k.push(order);
                }
            }


            // if let self.time.el {
                // self.loading = false;
                // if let Some(storage) = frame.storage_mut() {
                //     match serde_json::to_string(&self.service_map) {
                //         Ok(service_map) => storage.set_string("service_data", service_map),
                //         Err(e) => info!("error converting service_data to string: {e:?}"),
                //     }
                // }
            // }
        }
    
        if let Ok(notes) = self.services_viewer.notes_channel.1.try_recv() {
            info!("Got notes: {notes:?}");
            if self.services_viewer.selected.is_some() {
                info!("Creating chat view");
                self.services_viewer.chat_view = ChatView::new(notes, current_user, store_users, None, None);
            }
        }

        if let Ok(order_data) = self.services_viewer.tur_channel.1.try_recv() {
            info!("Got order_data: {order_data:?}");
            // if self.services_viewer.selected.is_some() {
            //     self.services_viewer.chat_view = ChatView::new(order_data, current_user, store_users);
            // }
        }

    }
}

impl TaskRowViewer {
    async fn get_order_notes(service_number: String) -> anyhow::Result<Vec<TaskNotePayload>, anyhow::Error> {
        let existing_notes = get_task_notes_from_db_with_service_number(service_number.clone()).await?;
        if !existing_notes.is_empty() {
            info!("We already have notes");
            Ok(existing_notes)
        } else {
            let mut note = TaskNotePayload::default();
            let notes = note.get_notes_from_service_number(&service_number).await?;
            info!("notes: {notes:?}");
            Ok(notes)
        }
    }

    async fn get_prestashop_order(service_number: String) -> anyhow::Result<PrestashopPayload, anyhow::Error> {
        info!("Did not have a task, creating");
        let value = get_prestashop_payload(&service_number).await?;
        let mut customer = CustomerData::default();
        let mut ticket = TicketPayload::default();
        let mut task: TaskPayload = TaskPayload::default();
        let mut task_notes = Vec::new();

        let service_details = value.order.associations.order_service.clone();
        let mut services: Vec<RecordId> = Vec::new();

        let sales_rep = value.sales_rep.clone().unwrap_or_default();
        let split_rep = value.split_rep.clone().unwrap_or_default();
        let email = parse_email_user(&sales_rep.email);
        let email_split_rep = parse_email_user(&split_rep.email);

        customer.id = value.customer.id.clone();
        customer.cust_code = value.customer.cust_code.clone();
        customer.email = value.customer.email.clone();
        customer.name = value.customer.name.clone();
        customer.phone_number = value.customer.phone_number.clone();
        ticket.salesman = email_split_rep.to_string();
        ticket.sales_rep = email.to_string();
        ticket.tech = email.to_string();
        info!(
            "Salesman: {:?}\nTech: {:?}",
            ticket.salesman.clone(),
            ticket.tech.clone()
        );
        ticket.customer = Some(customer.clone());
        ticket.checkin_rep = email.to_string();
        ticket.terms = value.order.payment.clone();
        ticket.ticket_total = value.order.total_products_wt.clone();
        ticket.doc_alias = value.order.order_type.clone();
        ticket.service_number = value.order.id.clone();
        ticket.id = RecordId::from((
            TICKET_TABLE.to_string(),
            ticket.service_number.clone(),
        ));
        task.id = RecordId::from((
            TASK_TABLE.to_string(),
            ticket.service_number.clone(),
        ));

        for msg in value.customer_messages.iter() {
            task_notes.push(TaskNotePayload {
                everest_initials: msg.id_employee.clone(),
                note: msg.message.clone(),
                id: RecordId::from((TASK_NOTE_TABLE, msg.id.clone())),
                task_id: Some(task.id.clone()),
                // created_at: msg.date_add.clone(),
                id_customer_thread: Some(msg.id_customer_thread.clone()),
                id_customer_message: Some(msg.id.clone()),
                id_employee: Some(msg.id_employee.clone()),
                ..Default::default()
            })
        }
        task.task_note = task_notes.clone();
        // Get the current time in UTC
        let now = Utc::now();

        // Format the date in the desired format
        let formatted_date = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        
        task.due_date = formatted_date;
        services.push(ticket.id.clone());
        let mut computer_data = ComputerData::default();
        if !service_details.is_empty() {
            if service_details.len() == 1 {
                let svc = service_details.get(0);
                if let Some(service) = svc {
                    ticket.checkin_notes = service.check_in_notes.clone();
                    computer_data.device_name = Some(service.device_name.clone());
                    computer_data.device_mfg = Some(service.device_mfg.clone());
                    computer_data.device_model = Some(service.device_model.clone());
                    computer_data.device_serial = Some(service.device_serial.clone());
                }
            } else {
                info!("Theres a couple.... {:?}", service_details);
            }
        }

        task.service_ticket = Some(ticket.clone());

        task.task_name = format!(
            "{} - {}",
            &customer.name,
            ticket.service_number.clone()
        );

        create_full_task_payload(
            ticket.into(), 
            customer, 
            computer_data, 
            task.clone().into(), 
            task.clone().task_note, 
            false
        ).await?;

        Ok(value)
    }
}

#[derive(PartialEq, Debug, Clone, Default)]
pub enum TaskAudit {
    #[default]
    AllServices,
    CheckinShelf,
    MyInRepair,
    InRepair,
    DoneShelf,
    MyServices
}

impl TaskAudit {
    fn as_str(self) -> String {
        match self {
            TaskAudit::CheckinShelf => "Check-in Shelf".to_string(),
            TaskAudit::MyInRepair => "My In Repair".to_string(),
            TaskAudit::InRepair => "In Repair".to_string(),
            TaskAudit::DoneShelf => "Done Shelf".to_string(),
            TaskAudit::AllServices => "All Services".to_string(),
            TaskAudit::MyServices => "My Services".to_string()
        }
    }
}

// Don't need to implement any trait on row data itself.
// #[derive(Default, Serialize, Clone, Deserialize, PartialEq, Debug)]
// pub struct PrestashopPayload(pub String, pub String, pub String, pub String, pub String, pub String, pub String);

impl RowViewer<PrestashopPayload> for TaskRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<PrestashopPayload>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        10
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Sales Rep", "Split Rep", "Checkin Notes", "Device", "Model", "Needs Call"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &PrestashopPayload) -> bool {
        row.order.id.contains(&self.filter) 
        || row.customer.name.to_lowercase().contains(&self.filter)
        || row.sales_rep.clone().unwrap_or_default().firstname.to_lowercase().contains(&self.filter)
        || row.sales_rep.clone().unwrap_or_default().lastname.to_lowercase().contains(&self.filter)
        || row.split_rep.clone().unwrap_or_default().firstname.to_lowercase().contains(&self.filter)
        || row.split_rep.clone().unwrap_or_default().lastname.to_lowercase().contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &PrestashopPayload, column: usize) {
        let _ = match column {
            0 => ui.horizontal_centered(|ui| ui.colored_label(ui.style().visuals.warn_fg_color, format!(" {}", row.order.id.clone()))).inner,
            1 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.customer.name.clone()))).inner,
            2 => ui.horizontal_centered(|ui| {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(&row.order.date_add, "%Y-%m-%d %H:%M:%S")
                    .expect("Failed to parse datetime");

                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);

                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format(" %m/%d/%Y").to_string();
                let split1 = formatted_date.split_once('/').unwrap_or_default();
                let split2 = split1.1.split_once('/').unwrap_or_default();
                ui.horizontal_centered(|ui| {
                    ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}/", split1.0));
                    ui.colored_label(ui.style().visuals.error_fg_color, format!("{}/", split2.0));
                    ui.colored_label(ui.style().visuals.warn_fg_color, split2.1)
                }).inner
            }).inner,
            3 => ui.horizontal_centered(|ui| {
                let status = match row.order.current_state.as_str() {
                    "30" => "In Repair",
                    "40" => "Done Shelf",
                    "4" => "Shipped",
                    "29" => "Check-in Shelf",
                    "239" => "Accepted by Odoo",
                    _ => ""
                };

                ui.label(format!(" {status}"))
            }).inner,
            4 => ui.horizontal_centered(|ui| {
                let emp = row.sales_rep.clone().unwrap_or_default();
                let split = parse_email_user(&emp.email);
                ui.label(format!(" {split}"))
            }).inner,
            5 => ui.horizontal_centered(|ui| {
                let emp = row.split_rep.clone().unwrap_or_default();
                let split = parse_email_user(&emp.email);
                ui.label(format!(" {split}"))
            }).inner,
            6 => ui.horizontal_centered(|ui| Label::new(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone())).wrap().ui(ui)).inner,
            7 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg))).inner,
            8 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model))).inner,
            9 => ui.vertical_centered(|ui| {
                ui.checkbox(&mut false, "")
                // row.customer_messages.iter().map(|c| {
                //     let date = convert_date_string(&c.date_add);
                    
                // })
            }).inner,
            _ => unreachable!(),
        };
    }

    fn column_render_config(&mut self, column: usize) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(90.).at_most(100.),
            3 => col_config.resizable(true).at_least(130.).at_most(130.),
            4 => col_config.resizable(true).at_least(100.).at_most(150.),
            5 => col_config.resizable(true).at_least(100.).at_most(150.),
            6 => col_config.resizable(true).at_least(80.),
            7 => col_config.resizable(true).at_least(100.).at_most(150.),
            8 => col_config.resizable(true).at_least(100.).at_most(150.),
            9 => col_config.resizable(false).at_least(80.).at_most(80.),
            _ => col_config,
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut PrestashopPayload,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        match column {
            0 => Some(
                Hyperlink::from_label_and_url(
                    format!(" {}", row.order.id.clone()), 
                    format!("{BASE_URL}{}", row.order.id.clone())
                )
                .open_in_new_tab(true)
                .ui(ui)
            ),
            _ => None,
        }
    }

    fn on_cell_view_response(
        &mut self,
        row: &PrestashopPayload,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<PrestashopPayload>> {
        match column {
            0 | 1 => {
                if resp.clicked() {
                    self.chat_view.messages.clear();
                    self.selected = Some(row.clone());
                    let notes_tx = self.notes_channel.0.clone();
                    let service_number = row.order.id.clone();
                    PlatformSpawner::spawn(async move {
                        match Self::get_order_notes(service_number).await {
                            Ok(notes) => notes_tx.try_send(notes).unwrap(),
                            Err(e) => info!("Error {e:?}"),
                        };
                    });
                }
            },
            _ => {}
        }
    
        resp
            .clone()
            .on_hover_and_drag_cursor(eframe::egui::CursorIcon::Crosshair)
            .dnd_release_payload::<String>()
            .map(|_| Box::new(PrestashopPayload::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &PrestashopPayload,
        dst: &mut PrestashopPayload,
        column: usize,
    ) {
        match column {
            0 => dst.order.id = src.order.id.clone(),
            1 => dst.customer.name = src.customer.name.clone(),
            2 => dst.order.date_add = src.order.date_add.clone(),
            3 => dst.order.current_state = src.order.current_state.clone(),
            4 => dst.sales_rep = src.sales_rep.clone(),
            5 => dst.split_rep = src.split_rep.clone(),
            6 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone()
            7 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().device_mfg.clone()
            8 => dst.sales_rep = src.sales_rep.clone(),
            9 => dst.sales_rep = src.sales_rep.clone(),
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &PrestashopPayload,
        row_r: &PrestashopPayload,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.order.id.cmp(&row_r.order.id),
            1 => row_l.customer.name.cmp(&row_r.customer.name),
            2 => row_l.order.date_add.cmp(&row_r.order.date_add),
            3 => row_l.order.current_state.cmp(&row_r.order.current_state),
            4 => {
                let emp = row_l.sales_rep.clone().unwrap_or_default();
                let name = format!("{} {}", emp.firstname, emp.lastname);
                let emp1 = row_r.sales_rep.clone().unwrap_or_default();
                let name1 = format!("{} {}", emp1.firstname, emp1.lastname);
                name.cmp(&name1)
            },
            5 => {
                let emp = row_l.split_rep.clone().unwrap_or_default();
                let name = format!("{} {}", emp.firstname, emp.lastname);
                let emp1 = row_r.split_rep.clone().unwrap_or_default();
                let name1 = format!("{} {}", emp1.firstname, emp1.lastname);
                name.cmp(&name1)
            },
            6 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes),
            7 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg),
            8 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model),
            _ => row_l.sales_rep.clone().unwrap_or_default().lastname.cmp(&row_r.sales_rep.clone().unwrap_or_default().lastname)
        }
    }

    fn new_empty_row(&mut self) -> PrestashopPayload {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        PrestashopPayload::default()
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<PrestashopPayload> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &PrestashopPayload, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.order.id),
            1 => dst.push_str(&src_row.customer.name),
            2 => {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(
                    &src_row.order.date_add,
                    "%Y-%m-%d %H:%M:%S"
                )
                .expect("Failed to parse datetime");
                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);
                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format("%m/%d/%Y").to_string();
                dst.push_str(&formatted_date);
            },
            3 => dst.push_str(&src_row.order.current_state),
            4 => {
                let emp = src_row.sales_rep.clone().unwrap_or_default();
                info!("Employee: {emp:?}");
                let name = format!("{} {}", emp.firstname, emp.lastname);
                dst.push_str(&name);
            },
            5 => {
                let emp = src_row.split_rep.clone().unwrap_or_default();
                info!("Employee: {emp:?}");
                let name = format!("{} {}", emp.firstname, emp.lastname);
                dst.push_str(&name);
            },
            6 => dst.push_str(&src_row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes),
            7 => dst.push_str(&src_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg),
            8 => dst.push_str(&src_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model),
            9 => dst.push_str("False"),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut PrestashopPayload,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.order.id.replace_range(.., src_data),
            1 => dst_row.customer.name = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.order.date_add = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.order.current_state = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => {
                let dst = &mut dst_row.sales_rep.clone().unwrap_or_default().firstname;
                *dst = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?
            },
            5 => {
                let dst = &mut dst_row.split_rep.clone().unwrap_or_default().firstname;
                *dst = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?
            },
            6 => dst_row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            7 => dst_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            8 => dst_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            9 => {},
            _ => unreachable!(),
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> PrestashopPayload {
        PrestashopPayload::default()
    }
}