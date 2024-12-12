use std::collections::HashMap;

use crate::{channel_manager::ChannelManager, egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, DataTable, Renderer, RowViewer, UiAction}, Spawner};
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{Button, CentralPanel, ComboBox, KeyboardShortcut, Spinner, TextEdit, TopBottomPanel, Ui, Widget};
use database::schema::{helper_traits::EmployeeHelper, prestashop_schema::{self, Employee, PrestashopPayload}, User};
use egui_extras::Column;
use itertools::Itertools;
use crate::{app_state::SharedContext, PlatformSpawner};
use serde::Serialize;

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
    order_channel: (Sender<Vec<prestashop_schema::PrestashopPayload>>, Receiver<Vec<prestashop_schema::PrestashopPayload>>),
    my_orders_table: DataTable<PrestashopOrderData>,
    my_orders_viewer: TaskRowViewer,
    loading: bool,
    index: i32,
    services: HashMap<String, Vec<PrestashopPayload>>
}

impl TaskAuditViewer {
    pub fn new() -> Self {
        let order_channel = <Vec<prestashop_schema::PrestashopPayload>>::create_unbounded_channel();
        Self {
            audit_selection: TaskAudit::default(),
            my_orders_table: DataTable::default(),
            my_orders_viewer: TaskRowViewer::default(),
            order_channel,
            loading: false,
            index: 0,
            services: HashMap::new()
        }
    }

    fn show(&mut self, ui: &mut Ui, current_user: Option<User>) {
        TopBottomPanel::top("Task Audit Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    TextEdit::singleline(&mut self.my_orders_viewer.filter)
                        .hint_text("Search for SO# / Customer")
                        .ui(ui);

                    ui.add_space(10.);

                    if Button::new("Refresh").ui(ui).clicked() {
                        let order_tx = self.order_channel.0.clone();
                        let selected = self.audit_selection.clone();
                        Self::get_services(selected, current_user.clone(), order_tx, self.index);
                    }
                    ui.add_space(10.);
                    if Button::new("Load +10").ui(ui).clicked() {
                        let order_tx = self.order_channel.0.clone();
                        let selected = self.audit_selection.clone();
                        self.index += 10;
                        Self::get_services(selected, current_user.clone(), order_tx, self.index);
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
                        ui.selectable_value(selected, TaskAudit::AllServices, "All Services");
                        ui.selectable_value(selected, TaskAudit::CheckinShelf, "Check-in Shelf");
                        ui.selectable_value(selected, TaskAudit::MyInRepair, "My In Repair");
                        ui.selectable_value(selected, TaskAudit::InRepair, "In Repair");
                        ui.selectable_value(selected, TaskAudit::DoneShelf, "Done Shelf");
                        ui.selectable_value(selected, TaskAudit::MyServices, "My Services");
                    })
                    .response;

                if current_selection != *selected {
                    self.loading = true;
                    let order_tx = self.order_channel.0.clone();
                    Self::get_services(selected.clone(), current_user, order_tx, self.index);
                }
            
                if self.loading {
                    ui.add_space(10.);
                    Spinner::new().color(ui.style().visuals.error_fg_color).ui(ui);
                }
            });

            Renderer::new(&mut self.my_orders_table, &mut self.my_orders_viewer).ui(ui);
        });  
    }

    pub fn get_services(
        selected: TaskAudit, 
        current_user: Option<User>, 
        order_tx: Sender<Vec<prestashop_schema::PrestashopPayload>>, 
        index: i32
    ) {
        let usr = current_user.clone().unwrap_or_default();
        let id = usr.id_prestashop.unwrap_or_default();
        let mut employee = Employee::default();
        employee.id = format!("{id}");
        employee.id_store = usr.id_store.unwrap_or_default();
        let idx = index.clone();
        match selected {
            TaskAudit::CheckinShelf => {
                PlatformSpawner::spawn(async move {
                    let services = employee.get_services_by_status("29", idx).await;
                    match services {
                        Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                        Err(e) => log::info!("Error getting my services: {e:?}"),
                    }
                });
            },
            TaskAudit::MyInRepair => {
                PlatformSpawner::spawn(async move {
                    let services = employee.get_my_services(idx).await;
                    match services {
                        Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                        Err(e) => log::info!("Error getting my services: {e:?}"),
                    }
                });
            },
            TaskAudit::InRepair => {
                PlatformSpawner::spawn(async move {
                    let services = employee.get_services_by_status("30", idx).await;
                    match services {
                        Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                        Err(e) => log::info!("Error getting my services: {e:?}"),
                    }
                });
            },
            TaskAudit::DoneShelf => {
                PlatformSpawner::spawn(async move {
                    let services = employee.get_services_by_status("40", idx).await;
                    match services {
                        Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                        Err(e) => log::info!("Error getting my services: {e:?}"),
                    }
                });
            },
            TaskAudit::AllServices => {
                PlatformSpawner::spawn(async move {
                    let services = employee.get_all_services_in_my_store(idx).await;
                    match services {
                        Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                        Err(e) => log::info!("Error getting my services: {e:?}"),
                    }
                });
            },
            TaskAudit::MyServices => {
                PlatformSpawner::spawn(async move {
                    let services = employee.get_my_services(idx).await;
                    match services {
                        Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                        Err(e) => log::info!("Error getting my services: {e:?}"),
                    }
                });
            },
        }
    }

    pub fn receive(&mut self) {
        if let  Ok(orders) = self.order_channel.1.try_recv() {
            // let key = self.audit_selection.as_str();
            // self.services.entry(key).or_insert(orders.clone()).it;
            // self.services
            //     .iter_mut()
            //     .map(|(k, v)| 
            // {

            // });
            let data: Vec<PrestashopOrderData> = orders
                .iter()
                .map(|order_data| {
                    PrestashopOrderData(
                        order_data.order.id.clone(),
                        order_data.customer.name.clone(),
                        order_data.order.date_add.clone(),
                        order_data.order.associations.order_rows.iter().find_or_first(
                            |f|
                            f.product_name ==  f.product_name
                        ).cloned().unwrap_or_default().product_name,
                        order_data.order.id_store.clone()
                    )
                })
                .collect();

            self.my_orders_table.extend(data);
            self.loading = false;
        }
    }

}

#[derive(PartialEq, Debug, Clone, Default)]
pub enum TaskAudit {
    CheckinShelf,
    #[default]
    MyInRepair,
    InRepair,
    DoneShelf,
    AllServices,
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
#[derive(Default, Serialize, Clone)]
pub struct PrestashopOrderData(pub String, pub String, pub String, pub String, pub String);

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
            _ => unreachable!(),
        }

        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> PrestashopOrderData {
        PrestashopOrderData("".to_string(), "".to_string(),"".to_string(),"".to_string(),"".to_string())
    }
}


impl RowViewer<PrestashopOrderData> for TaskRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<PrestashopOrderData>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        5
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Needs Call"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &PrestashopOrderData) -> bool {
        row.0.contains(&self.filter) || row.1.contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &PrestashopOrderData, column: usize) {
        let _ = match column {
            0 => ui.horizontal_centered(|ui| ui.colored_label(ui.style().visuals.warn_fg_color , format!(" {}", row.0.clone()))),
            1 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.1.clone()))),
            2 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.2.clone()))),
            3 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.3.clone()))),
            4 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.4.clone()))),
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
            4 => col_config.resizable(true).at_least(50.).at_most(50.),
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
            _ => row_l.0.cmp(&row_r.0)
        }
    }

    fn new_empty_row(&mut self) -> PrestashopOrderData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        PrestashopOrderData(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
    }
}
