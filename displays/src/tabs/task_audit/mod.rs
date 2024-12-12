use crate::{channel_manager::ChannelManager, egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, DataTable, Renderer, RowViewer, UiAction}, Spawner};
use eframe::egui::{Button, CentralPanel, ComboBox, KeyboardShortcut, Spinner, TextEdit, TopBottomPanel, Ui, Widget};
use database::schema::{helper_traits::EmployeeHelper, prestashop_schema::{self, Employee}, User};
use log::info;
use crate::{app_state::SharedContext, PlatformSpawner};
use crossbeam::channel::{Receiver, Sender};
use std::collections::HashMap;
use itertools::Itertools;
use egui_extras::Column;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://pclaptops.mojo11.com/pcladmin/index.php?controller=AdminOrders&vieworder=&id_order=";
impl SharedContext {
    pub fn task_table_viewer(&mut self, ui: &mut Ui) {
        self.task_audit_table.show(ui, self.current_user.clone());
    }
}

/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct TaskRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
}

pub struct TaskAuditViewer {
    audit_selection: TaskAudit,
    order_channel: (Sender<prestashop_schema::PrestashopPayload>, Receiver<prestashop_schema::PrestashopPayload>),
    services_viewer: TaskRowViewer,
    loading: bool,
    index: HashMap<String, i32>,
    counter: i32,
    pub service_map: HashMap<String, DataTable<PrestashopOrderData>>,
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
            counter: 0,
            service_map: HashMap::new()
        }
    }

    fn show(&mut self, ui: &mut Ui, current_user: Option<User>) {
        TopBottomPanel::top("Task Audit Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
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
                            k.iter().map(|k| k.0.clone()).collect::<Vec<String>>()
                        } else {
                            Vec::new()
                        };
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
                            k.iter().map(|k| k.0.clone()).collect::<Vec<String>>()
                        } else {
                            Vec::new()
                        };
                        Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx);
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
                        k.iter().map(|k| k.0.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    info!("Services from cache: {:?}", svcs.clone());
                    Self::get_services(selected.clone(), current_user, order_tx, svcs, start_idx);
                }
            
                if self.loading {
                    ui.ctx().request_repaint();
                    ui.add_space(10.);
                    Spinner::new().color(ui.style().visuals.error_fg_color).ui(ui);
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
                        .get_services_by_status("29", start_idx, start_idx+10)
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
                        .get_my_services(start_idx, start_idx+10)
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
                        .get_services_by_status("30", start_idx, start_idx+10)
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
                        .get_services_by_status("40", start_idx, start_idx+10)
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
                        .get_all_services_in_my_store(start_idx, start_idx+10)
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
                        .get_my_services(start_idx, start_idx+10)
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
                        Err(e) => log::info!("Error with get_my_services: {:?}", e)
                    };
                });
            },
        }
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame) {
        if let  Ok(order) = self.order_channel.1.try_recv() {
            self.counter += 1;
            self.loading = true;

            let key = self.audit_selection.clone().as_str();
            let sales_rep = order
                .sales_rep
                .clone()
                .unwrap_or_default()
                .clone();

            let email = sales_rep
                .email
                .split_once('@')
                .unwrap_or_default()
                .clone();

            let split_rep = order
                .split_rep
                .clone()
                .unwrap_or_default()
                .clone();

            let split_rep_email = split_rep
                .email
                .split_once('@')
                .unwrap_or_default()
                .clone();
            
            let status = match order.order.current_state.as_str() {
                "30" => "In Repair",
                "40" => "Done Shelf",
                "4" => "Shipped",
                "29" => "Check-in Shelf",
                "239" => "Accepted by Odoo",
                _ => ""
            };

            let final_data = PrestashopOrderData(
                order.order.id.clone(),
                order.customer.name.clone(),
                order.order.date_add.clone(),
                status.to_string(),
                // order.order.associations.order_rows.iter().find_or_first(
                //     |f|
                //     f.product_name ==  f.product_name
                // ).cloned().unwrap_or_default().product_name,
                email.0.to_string(),
                split_rep_email.0.to_string(),
                "".to_string()
            );

            self
                .service_map
                .entry(key.clone())
                .or_insert(DataTable::default());

            
            if let Some(k) = self.service_map.get_mut(&key) {
                if !k.iter().contains(&final_data) {
                    k.push(final_data);
                }
            }


            if self.counter == 10 {
                self.counter = 0;
                self.loading = false;
                if let Some(storage) = frame.storage_mut() {
                    match serde_json::to_string(&self.service_map) {
                        Ok(service_map) => storage.set_string("service_data", service_map),
                        Err(e) => info!("error converting service_data to string: {e:?}"),
                    }
                }
            }
            log::warn!("idx: {:?}\ncounter: {}", self.index, self.counter);
        }
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
#[derive(Default, Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct PrestashopOrderData(pub String, pub String, pub String, pub String, pub String, pub String, pub String);

impl RowViewer<PrestashopOrderData> for TaskRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<PrestashopOrderData>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        7
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Sales Rep", "Split Rep", "Needs Call"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &PrestashopOrderData) -> bool {
        row.0.contains(&self.filter) 
        || row.1.to_lowercase().contains(&self.filter)
        || row.4.to_lowercase().contains(&self.filter)
        || row.5.to_lowercase().contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &PrestashopOrderData, column: usize) {
        let _ = match column {
            0 => ui.horizontal_centered(|ui| ui.hyperlink_to(format!(" {}", row.0.clone()), format!("{BASE_URL}{}", row.0.clone()))),
            1 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.1.clone()))),
            2 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.2.clone()))),
            3 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.3.clone()))),
            4 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.4.clone()))),
            5 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.5.clone()))),
            6 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.6.clone()))),
            _ => unreachable!(),
        };
    }

    fn column_render_config(&mut self, column: usize) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(150.).at_most(150.),
            3 => col_config.resizable(true).at_least(250.).at_most(320.),
            4 => col_config.resizable(true).at_least(130.).at_most(150.),
            5 => col_config.resizable(true).at_least(130.).at_most(150.),
            6 => col_config.resizable(true).at_least(50.).at_most(50.),
            _ => col_config,
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut PrestashopOrderData,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        let res = match column {
            0 => ui.label(row.0.clone()),
            1 => ui.label(row.1.clone()),
            2 => ui.label(row.2.clone()),
            3 => ui.label(row.3.clone()),
            4 => ui.label(row.4.clone()),
            5 => ui.label(row.5.clone()),
            6 => ui.label(row.6.clone()),
            _ => unreachable!(),
        };
        Some(res)
    }

    fn set_cell_value(
        &mut self,
        src: &PrestashopOrderData,
        dst: &mut PrestashopOrderData,
        column: usize,
    ) {
        match column {
            0 => dst.0 = src.0.clone(),
            1 => dst.1 = src.1.clone(),
            2 => dst.2 = src.2.clone(),
            3 => dst.3 = src.3.clone(),
            4 => dst.4 = src.4.clone(),
            5 => dst.5 = src.5.clone(),
            6 => dst.6 = src.6.clone(),
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &PrestashopOrderData,
        row_r: &PrestashopOrderData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l.1.cmp(&row_r.1),
            2 => row_l.2.cmp(&row_r.2),
            3 => row_l.3.cmp(&row_r.3),
            4 => row_l.4.cmp(&row_r.4),
            5 => row_l.5.cmp(&row_r.5),
            6 => row_l.6.cmp(&row_r.6),
            _ => row_l.0.cmp(&row_r.0)
        }
    }

    fn new_empty_row(&mut self) -> PrestashopOrderData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        PrestashopOrderData::default()
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<PrestashopOrderData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &PrestashopOrderData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.0),
            1 => dst.push_str(&src_row.1),
            2 => dst.push_str(&src_row.2),
            3 => dst.push_str(&src_row.3),
            4 => dst.push_str(&src_row.4),
            5 => dst.push_str(&src_row.5),
            6 => dst.push_str(&src_row.6),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut PrestashopOrderData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.0.replace_range(.., src_data),
            1 => dst_row.1 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.2 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.3 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst_row.4 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            5 => dst_row.5 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            6 => dst_row.6 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }

        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> PrestashopOrderData {
        PrestashopOrderData::default()
    }
}