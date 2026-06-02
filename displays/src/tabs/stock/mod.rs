use eframe::egui::{Align2, Area, Button, CentralPanel, Color32, ComboBox, Frame, Hyperlink, Id, Key, Link, Order, Panel, RichText, ScrollArea, Spinner, TextEdit, Ui, Widget, scroll_area};
use crate::tabs::stock::store_inventory_viewer::{ExtraInventoryData, StockQuantityData, StockQuantityViewer};
use crate::tabs::stock::everest_lookup::{EverestItemRow, EverestItemViewer, EverestLookupResult, EverestOrder, OdooSerialHistory, lookup_everest_order, fetch_serial_movement, order_to_rows, order_totals, EverestRow, EverestCustomerSearchResult, EverestCustomerOrdersResult, search_everest_customers, fetch_customer_orders, lookup_everest_order_by_docnum, row_str, row_customer_name, row_cust_code, row_doc_no};
use crate::tabs::stock::inventory_audit::{
    format_date_long, format_date_short, list_audits, load_audit, lookup_serials_in_odoo,
    mark_found, render_history_windows, save_audit, AuditSerialRow, HistoryWindow,
    InventoryAuditMeta, InventoryView,
};
use crate::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use crate::{PlatformSpawner, Spawner, TaskUiActions, get_current_user_from_auth};
use database::schema::{RecordId, Store, prestashop::{Customer, Address, xml::{modify_xml, remove_xml_tag}}};
use database::xidax_order_url;
use egui_data_table::Renderer;
use log::info;
use std::collections::HashMap;

pub mod everest_lookup;
pub mod inventory_audit;
pub mod row_viewer;
pub mod stock_operations;
pub mod store_inventory_viewer;

/// Which "Import Serials" sub-action the user picked from the menu.
#[derive(Copy, Clone, Debug)]
enum ImportKick {
    Csv,
    Paste,
}

/// Open a file picker and parse the chosen file as one serial per line.
/// Returns `None` if the user cancelled. Skips blank lines and a header
/// row if the first line is non-alphanumeric or looks like the word
/// "serial".
#[cfg(not(target_arch = "wasm32"))]
fn pick_csv_serials() -> Option<Vec<String>> {
    let path = rfd::FileDialog::new()
        .add_filter("CSV / Text", &["csv", "txt"])
        .set_title("Choose a list of serials to import")
        .pick_file()?;
    match std::fs::read_to_string(&path) {
        Ok(contents) => Some(parse_serial_list(&contents)),
        Err(e) => {
            log::error!("Failed to read serial list {:?}: {e:?}", path);
            None
        }
    }
}

/// Parse a newline-separated serial list. Trims, strips a single optional
/// CSV-style first column wrapping comma (so "ABC123,..." works), filters
/// blanks, and drops a header row that looks like "serial" / "Serial Number".
fn parse_serial_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let mut line = raw.trim().trim_matches(['"', '\'']).to_string();
        if let Some(pos) = line.find(',') {
            line.truncate(pos);
            line = line.trim().to_string();
        }
        if line.is_empty() {
            continue;
        }
        if idx == 0 {
            let lower = line.to_ascii_lowercase();
            if lower == "serial" || lower.starts_with("serial number") || lower.starts_with("s/n") {
                continue;
            }
        }
        out.push(line);
    }
    out
}

pub use row_viewer::*;
pub use stock_operations::*;

// #[derive(Default)]
pub struct StockTable {
    stock_selection: StockSelection,
    inventory_serials_table: egui_data_table::DataTable<SerialsData>,
    inventory_serials_viewer: SerialsViewer,
    stock_quantity_viewer: StockQuantityViewer,
    stock_quantity_table: egui_data_table::DataTable<StockQuantityData>,
    // Cost breakdown
    cost_breakdown_viewer: CostBreakdownViewer,
    cost_breakdown_table: egui_data_table::DataTable<CostBreakdownData>,
    cost_order_id: String,
    cost_loading: bool,
    cost_summary: Option<CostBreakdownSummary>,
    // Systems In-Store
    systems_in_store_viewer: SystemInStoreViewer,
    systems_in_store_table: egui_data_table::DataTable<SystemInStoreData>,
    systems_order_id: String,
    systems_loading: bool,
    systems_first_load: bool,
    is_admin: bool,
    first_run: bool,
    pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    pub extra_stock_channel: (Sender<Vec<ExtraInventoryData>>, Receiver<Vec<ExtraInventoryData>>),
    pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    pub cost_channel: (Sender<Vec<CostBreakdownData>>, Receiver<Vec<CostBreakdownData>>),
    pub cost_summary_channel: (Sender<CostBreakdownSummary>, Receiver<CostBreakdownSummary>),
    pub systems_channel: (Sender<Vec<SystemInStoreData>>, Receiver<Vec<SystemInStoreData>>),
    pub systems_add_channel: (Sender<SystemInStoreData>, Receiver<SystemInStoreData>),
    pub systems_task_channel: (Sender<SystemInStoreData>, Receiver<SystemInStoreData>),
    store_selection: u64,
    // Everest lookup state
    everest_serial_input: String,
    everest_loading: bool,
    everest_history_loading: bool,
    everest_order: Option<EverestOrder>,
    everest_error: Option<String>,
    everest_selected_serial: Option<String>,
    everest_history: Option<OdooSerialHistory>,
    everest_items_table: egui_data_table::DataTable<EverestItemRow>,
    everest_items_viewer: EverestItemViewer,
    pub everest_order_channel: (Sender<EverestLookupResult>, Receiver<EverestLookupResult>),
    pub everest_history_channel: (Sender<OdooSerialHistory>, Receiver<OdooSerialHistory>),
    pub everest_serial_click_channel: (Sender<String>, Receiver<String>),
    // Everest customer search + breadcrumb navigation
    everest_view: EverestView,
    everest_crumbs: Vec<EverestCrumb>,
    everest_nav: Option<EverestNav>,
    everest_order_intent: EverestOrderIntent,
    everest_customer_query: String,
    everest_customer_search_type: CustomerSearchType,
    everest_search_loading: bool,
    everest_orders_loading: bool,
    everest_search_result: Option<EverestCustomerSearchResult>,
    everest_results_shown: usize,
    everest_current_cust: String,
    everest_orders_by_cust: HashMap<String, Vec<EverestRow>>,
    everest_order_by_doc: HashMap<String, EverestOrder>,
    pub everest_search_channel: (Sender<EverestCustomerSearchResult>, Receiver<EverestCustomerSearchResult>),
    pub everest_orders_channel: (Sender<EverestCustomerOrdersResult>, Receiver<EverestCustomerOrdersResult>),
    // Customer change modal state
    pub customer_change_channel: (Sender<CustomerChangeRequest>, Receiver<CustomerChangeRequest>),
    pub customer_search_results_channel: (Sender<Vec<(Customer, Address)>>, Receiver<Vec<(Customer, Address)>>),
    customer_modal_open: bool,
    customer_modal_order_id: String,
    customer_modal_current_name: String,
    customer_search_query: String,
    customer_search_type: CustomerSearchType,
    customer_search_results: Vec<(Customer, Address)>,
    customer_searching: bool,
    // Shared serial-history scan input used from Company Stock / Store Inventory headers.
    serial_history_input: String,
    // ---- Inventory audit state ----
    /// Whether the Store Inventory table is showing Live data or a saved audit.
    inventory_view: InventoryView,
    /// Audits available in the current store's combobox.
    audit_list: Vec<InventoryAuditMeta>,
    /// Right-side paste panel for the "Paste Serials" import path.
    import_panel_open: bool,
    import_textarea: String,
    /// Spinner toggle for the Odoo lookup phase of an import.
    import_in_progress: bool,
    /// When `true`, a separate keyboard-locked input is shown next to the
    /// serial-history input. Each Enter looks the serial up in the loaded
    /// audit and flips its `found` flag.
    scan_mode_active: bool,
    scan_input: String,
    /// Transient banner after a scan ("✓ MARKED" or "✗ NOT IN AUDIT").
    scan_feedback: Option<(String, Color32)>,
    /// Floating Odoo-history Windows. One per clicked serial.
    history_windows: Vec<HistoryWindow>,
    /// Raw Odoo timestamps the user has clicked in the right-side history
    /// panel. Cells in this set render with the time appended; others
    /// render as MM/DD/YYYY only.
    expanded_history_dates: std::collections::HashSet<String>,
    /// Cache of item-code → (std_price, list_price) populated from the
    /// Company Stock pull. Reused both to fill the Std/List columns on
    /// the live Store Inventory view, and to seed the same columns when
    /// importing a new audit.
    extra_stock_prices: HashMap<String, (f64, f64)>,
    pub audit_list_channel: (Sender<Vec<InventoryAuditMeta>>, Receiver<Vec<InventoryAuditMeta>>),
    pub audit_lookup_channel: (Sender<Vec<AuditSerialRow>>, Receiver<Vec<AuditSerialRow>>),
    pub audit_save_channel: (Sender<(InventoryAuditMeta, Vec<AuditSerialRow>)>, Receiver<(InventoryAuditMeta, Vec<AuditSerialRow>)>),
    pub audit_load_channel: (Sender<(InventoryAuditMeta, Vec<AuditSerialRow>)>, Receiver<(InventoryAuditMeta, Vec<AuditSerialRow>)>),
    pub serial_window_channel: (Sender<String>, Receiver<String>),
    pub history_result_channel: (Sender<OdooSerialHistory>, Receiver<OdooSerialHistory>),
    pub found_toggle_channel: (Sender<(RecordId, String, bool)>, Receiver<(RecordId, String, bool)>),
    csv_import_channel: (Sender<Vec<String>>, Receiver<Vec<String>>),
}

#[derive(Default, PartialEq, Clone)]
pub enum CustomerSearchType {
    #[default]
    Email,
    Phone,
}

/// Which screen the Everest tab is currently showing.
#[derive(Default, PartialEq, Clone, Debug)]
enum EverestView {
    #[default]
    Empty,
    Results,
    CustomerOrders,
    OrderDetail,
}

/// One entry in the Everest navigation trail. Each crumb carries enough to
/// restore its view from cache without re-fetching.
#[derive(Clone, Debug)]
enum EverestCrumb {
    Results,
    Customer { cust_code: String, label: String },
    Order { doc_no: String, label: String },
}

/// A navigation action collected during rendering and applied afterwards
/// (avoids borrowing `self` while iterating cached data).
#[derive(Clone, Debug)]
enum EverestNav {
    Crumb(usize),
    OpenCustomer { cust_code: String, label: String },
    OpenOrder { doc_no: String, label: String },
    OpenSerial(String),
    LoadMore,
}

/// What to do with the breadcrumb stack when an order-detail load resolves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum EverestOrderIntent {
    /// Fresh top-level lookup: reset the trail to just this order.
    Reset,
    /// Drill-down (clicked serial): append an order crumb on arrival.
    PushOnArrival,
    /// Crumb was already pushed at the click site (clicked order number).
    AlreadyPushed,
}

#[derive(Default, PartialEq)]
pub enum StockSelection {
    #[default]
    CompanyStock,
    StoreInventory,
    CostBreakdown,
    SystemsInStore,
    Everest,
}

impl StockSelection {
    fn as_str(&self) -> &str {
        match self {
            StockSelection::CompanyStock => "Company Stock",
            StockSelection::StoreInventory => "Store Inventory",
            StockSelection::CostBreakdown => "Cost Breakdown",
            StockSelection::SystemsInStore => "Systems In-Store",
            StockSelection::Everest => "Everest",
        }
    }
}

impl Default for StockTable {
    fn default() -> Self {
        let stock_channel = <Vec<RawStockData>>::create_unbounded_channel();
        let serial_channel = <SerialData>::create_unbounded_channel();
        let extra_stock_channel = <Vec<ExtraInventoryData>>::create_unbounded_channel();
        let cost_channel = <Vec<CostBreakdownData>>::create_unbounded_channel();
        let cost_summary_channel = <CostBreakdownSummary>::create_unbounded_channel();
        let systems_channel = <Vec<SystemInStoreData>>::create_unbounded_channel();
        let systems_add_channel = <SystemInStoreData>::create_unbounded_channel();
        let systems_task_channel = <SystemInStoreData>::create_unbounded_channel();
        let customer_change_channel: (Sender<CustomerChangeRequest>, Receiver<CustomerChangeRequest>) = crossbeam::channel::unbounded();
        let customer_search_results_channel: (Sender<Vec<(Customer, Address)>>, Receiver<Vec<(Customer, Address)>>) = crossbeam::channel::unbounded();

        let everest_order_channel: (Sender<EverestLookupResult>, Receiver<EverestLookupResult>) = crossbeam::channel::unbounded();
        let everest_history_channel: (Sender<OdooSerialHistory>, Receiver<OdooSerialHistory>) = crossbeam::channel::unbounded();
        let everest_serial_click_channel: (Sender<String>, Receiver<String>) = crossbeam::channel::unbounded();
        let everest_search_channel: (Sender<EverestCustomerSearchResult>, Receiver<EverestCustomerSearchResult>) = crossbeam::channel::unbounded();
        let everest_orders_channel: (Sender<EverestCustomerOrdersResult>, Receiver<EverestCustomerOrdersResult>) = crossbeam::channel::unbounded();

        let audit_list_channel: (Sender<Vec<InventoryAuditMeta>>, Receiver<Vec<InventoryAuditMeta>>) = crossbeam::channel::unbounded();
        let audit_lookup_channel: (Sender<Vec<AuditSerialRow>>, Receiver<Vec<AuditSerialRow>>) = crossbeam::channel::unbounded();
        let audit_save_channel: (Sender<(InventoryAuditMeta, Vec<AuditSerialRow>)>, Receiver<(InventoryAuditMeta, Vec<AuditSerialRow>)>) = crossbeam::channel::unbounded();
        let audit_load_channel: (Sender<(InventoryAuditMeta, Vec<AuditSerialRow>)>, Receiver<(InventoryAuditMeta, Vec<AuditSerialRow>)>) = crossbeam::channel::unbounded();
        let serial_window_channel: (Sender<String>, Receiver<String>) = crossbeam::channel::unbounded();
        let history_result_channel: (Sender<OdooSerialHistory>, Receiver<OdooSerialHistory>) = crossbeam::channel::unbounded();
        let found_toggle_channel: (Sender<(RecordId, String, bool)>, Receiver<(RecordId, String, bool)>) = crossbeam::channel::unbounded();
        let csv_import_channel: (Sender<Vec<String>>, Receiver<Vec<String>>) = crossbeam::channel::unbounded();

        let mut inventory_serials_viewer = SerialsViewer::default();
        inventory_serials_viewer.stock_tx = Some(serial_channel.0.clone());
        inventory_serials_viewer.serial_click_tx = Some(serial_window_channel.0.clone());
        inventory_serials_viewer.found_toggle_tx = Some(found_toggle_channel.0.clone());

        let systems_in_store_viewer = SystemInStoreViewer::new(
            systems_task_channel.0.clone(),
            customer_change_channel.0.clone(),
        );

        let mut everest_items_viewer = EverestItemViewer::default();
        everest_items_viewer.serial_click_tx = Some(everest_serial_click_channel.0.clone());

        // Check if current user is admin
        let is_admin = get_current_user_from_auth()
            .map(|user| user.is_admin() | user.is_manager())
            .unwrap_or(false);

        Self { 
            stock_selection: Default::default(), 
            inventory_serials_table: egui_data_table::DataTable::<SerialsData>::default(),
            inventory_serials_viewer,
            stock_quantity_viewer: StockQuantityViewer::default(),
            stock_quantity_table: egui_data_table::DataTable::<StockQuantityData>::default(),
            cost_breakdown_viewer: CostBreakdownViewer::default(),
            cost_breakdown_table: egui_data_table::DataTable::<CostBreakdownData>::default(),
            cost_order_id: String::new(),
            cost_loading: false,
            cost_summary: None,
            systems_in_store_viewer,
            systems_in_store_table: egui_data_table::DataTable::<SystemInStoreData>::default(),
            systems_order_id: String::new(),
            systems_loading: false,
            systems_first_load: true,
            is_admin,
            first_run: true, 
            serial_channel, 
            extra_stock_channel, 
            stock_channel,
            cost_channel,
            cost_summary_channel,
            systems_channel,
            systems_add_channel,
            systems_task_channel,
            store_selection: Store::RIV.into_odoo_store_id() as u64,
            // Customer change modal
            customer_change_channel,
            customer_search_results_channel,
            customer_modal_open: false,
            customer_modal_order_id: String::new(),
            customer_modal_current_name: String::new(),
            customer_search_query: String::new(),
            customer_search_type: CustomerSearchType::default(),
            customer_search_results: Vec::new(),
            customer_searching: false,
            // Everest
            everest_serial_input: String::new(),
            everest_loading: false,
            everest_history_loading: false,
            everest_order: None,
            everest_error: None,
            everest_selected_serial: None,
            everest_history: None,
            everest_items_table: egui_data_table::DataTable::<EverestItemRow>::default(),
            everest_items_viewer,
            everest_order_channel,
            everest_history_channel,
            everest_serial_click_channel,
            everest_view: EverestView::Empty,
            everest_crumbs: Vec::new(),
            everest_nav: None,
            everest_order_intent: EverestOrderIntent::Reset,
            everest_customer_query: String::new(),
            everest_customer_search_type: CustomerSearchType::Phone,
            everest_search_loading: false,
            everest_orders_loading: false,
            everest_search_result: None,
            everest_results_shown: 20,
            everest_current_cust: String::new(),
            everest_orders_by_cust: HashMap::new(),
            everest_order_by_doc: HashMap::new(),
            everest_search_channel,
            everest_orders_channel,
            serial_history_input: String::new(),
            // ---- Inventory audit ----
            inventory_view: InventoryView::default(),
            audit_list: Vec::new(),
            import_panel_open: false,
            import_textarea: String::new(),
            import_in_progress: false,
            scan_mode_active: false,
            scan_input: String::new(),
            scan_feedback: None,
            history_windows: Vec::new(),
            expanded_history_dates: std::collections::HashSet::new(),
            extra_stock_prices: HashMap::new(),
            audit_list_channel,
            audit_lookup_channel,
            audit_save_channel,
            audit_load_channel,
            serial_window_channel,
            history_result_channel,
            found_toggle_channel,
            csv_import_channel,
        }
    }
}

impl StockTable {
    pub fn ui(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("StockTopPanel")
            .exact_size(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ComboBox::new("Stock Selection", "")
                    .selected_text(self.stock_selection.as_str())
                    .show_ui(ui, |ui| {
                        let selected = &mut self.stock_selection;
                        ui.selectable_value(
                            selected, 
                            StockSelection::CompanyStock,
                            StockSelection::CompanyStock.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            StockSelection::StoreInventory,
                            StockSelection::StoreInventory.as_str()
                        );
                        // Only show Cost Breakdown option for admins
                        if self.is_admin {
                            ui.selectable_value(
                                selected,
                                StockSelection::CostBreakdown,
                                StockSelection::CostBreakdown.as_str()
                            );
                            ui.selectable_value(
                                selected,
                                StockSelection::SystemsInStore,
                                StockSelection::SystemsInStore.as_str()
                            );
                        }
                        ui.selectable_value(
                            selected,
                            StockSelection::Everest,
                            StockSelection::Everest.as_str()
                        );
                    });

                    ui.add_space(10.);

                    match self.stock_selection {
                        StockSelection::CompanyStock => {
                            TextEdit::singleline(&mut self.stock_quantity_viewer.filter)
                                .hint_text("Search for Item Code")
                                .ui(ui);

                            ui.add_space(10.);

                            if Button::new("Refresh").ui(ui).clicked() {
                                let stock_tx = self.extra_stock_channel.0.clone();
                                PlatformSpawner::spawn(async move {
                                    let stock = get_extra_stock_info(stock_tx.clone()).await;
                                    info!("Stock call: {stock:?}");
                                });
                            }
                            ui.add_space(10.);
                            ui.separator();
                            ui.add_space(10.);
                            self.serial_history_scan_input(ui);
                        },
                        StockSelection::StoreInventory => {
                            TextEdit::singleline(&mut self.inventory_serials_viewer.filter)
                                .hint_text("Search for Item Code or S/N")
                                .ui(ui);
        
                            ui.add_space(10.);
        
                            let selected = &mut self.store_selection;
                            let current = selected.clone();
                            let selected_text = Store::from_odoo_store_id(&selected.to_string()).as_str().to_string();
        
                            ComboBox::new("Store_Selection", "")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected, Store::RIV.into_odoo_store_id() as u64, Store::RIV.as_str());
                                    ui.selectable_value(selected, Store::LTN.into_odoo_store_id() as u64, Store::LTN.as_str());
                                    ui.selectable_value(selected, Store::MUR.into_odoo_store_id() as u64, Store::MUR.as_str());
                                    ui.selectable_value(selected, Store::ORE.into_odoo_store_id() as u64, Store::ORE.as_str());
                                    ui.selectable_value(selected, Store::SAN.into_odoo_store_id() as u64, Store::SAN.as_str());
                                });
        
                            if *selected != current {
                                let stock_tx = self.stock_channel.0.clone();
                                let store_selection = self.store_selection;
                                PlatformSpawner::spawn(async move {
                                    info!("Store: {:?}", store_selection);
                                    let stock = get_stock(stock_tx.clone(), store_selection).await;
                                    info!("Stock call: {stock:?}");
                                });
                                // Re-pull the audit list for the newly selected store.
                                let audit_tx = self.audit_list_channel.0.clone();
                                let store_id = Store::from_odoo_store_id(&store_selection.to_string()).into_odoo_store_id();
                                PlatformSpawner::spawn(async move {
                                    if let Err(e) = list_audits(store_id, audit_tx).await {
                                        log::error!("list_audits error: {e:?}");
                                    }
                                });
                                self.inventory_view = InventoryView::Live;
                                self.inventory_serials_viewer.audit_id = None;
                                self.scan_mode_active = false;
                            }
                            ui.add_space(10.);

                            // ---- Inventory Source combobox ----
                            let current_label = match &self.inventory_view {
                                InventoryView::Live => "Live Inventory".to_string(),
                                InventoryView::Audit(id) => self
                                    .audit_list
                                    .iter()
                                    .find(|m| m.id == *id)
                                    .map(|m| m.label.clone())
                                    .unwrap_or_else(|| "Audit".to_string()),
                            };
                            let mut pick: Option<Option<RecordId>> = None;
                            ComboBox::new("Audit_Source", "")
                                .selected_text(current_label)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(matches!(self.inventory_view, InventoryView::Live), "Live Inventory")
                                        .clicked()
                                    {
                                        pick = Some(None);
                                    }
                                    if self.audit_list.is_empty() {
                                        ui.label(RichText::new("(no saved audits)").color(Color32::GRAY));
                                    } else {
                                        for meta in self.audit_list.iter() {
                                            let selected = matches!(&self.inventory_view, InventoryView::Audit(id) if id == &meta.id);
                                            if ui.selectable_label(selected, &meta.label).clicked() {
                                                pick = Some(Some(meta.id.clone()));
                                            }
                                        }
                                    }
                                });
                            if let Some(choice) = pick {
                                match choice {
                                    None => {
                                        // Switch back to Live: re-pull stock.
                                        self.inventory_view = InventoryView::Live;
                                        self.inventory_serials_viewer.audit_id = None;
                                        self.scan_mode_active = false;
                                        let stock_tx = self.stock_channel.0.clone();
                                        let store_selection = self.store_selection;
                                        PlatformSpawner::spawn(async move {
                                            let _ = get_stock(stock_tx, store_selection).await;
                                        });
                                    }
                                    Some(id) => {
                                        let tx = self.audit_load_channel.0.clone();
                                        let load_id = id.clone();
                                        PlatformSpawner::spawn(async move {
                                            if let Err(e) = load_audit(load_id, tx).await {
                                                log::error!("load_audit error: {e:?}");
                                            }
                                        });
                                    }
                                }
                            }

                            ui.add_space(10.);

                            if Button::new("Refresh").ui(ui).clicked() {
                                let stock_tx = self.stock_channel.0.clone();
                                let store_selection = self.store_selection;
                                PlatformSpawner::spawn(async move {
                                    let stock = get_stock(stock_tx.clone(), store_selection).await;
                                    info!("Stock call: {stock:?}");
                                });
                            }
                            ui.add_space(10.);

                            if Button::new("Refresh S/N Info").ui(ui).clicked() {
                                let tx = self.serial_channel.0.clone();
                                let data_table = self.inventory_serials_table.iter();
                                let sns = data_table.map(|r| r.4.clone()).collect::<Vec<String>>();
                                PlatformSpawner::spawn(async move {
                                    let _res = find_attached_serials(sns, tx.clone()).await;
                                    if let Err(e) = _res {
                                        log::error!("S/N Info call error: {e:?}");
                                    } else {
                                        log::info!("S/N Info call ran ok");
                                    }
                                });
                            }
                            ui.add_space(10.);

                            // ---- Import Serials menu ----
                            let mut start_import = None;
                            eframe::egui::containers::menu::MenuButton::new("Import Serials").ui(ui, |ui| {
                                if ui.button("From CSV…").clicked() {
                                    start_import = Some(ImportKick::Csv);
                                    ui.close();
                                }
                                if ui.button("Paste List").clicked() {
                                    start_import = Some(ImportKick::Paste);
                                    ui.close();
                                }
                            });
                            if let Some(kind) = start_import {
                                match kind {
                                    ImportKick::Csv => {
                                        #[cfg(not(target_arch = "wasm32"))]
                                        if let Some(serials) = pick_csv_serials() {
                                            self.kick_off_import(serials);
                                        }
                                        #[cfg(target_arch = "wasm32")]
                                        {
                                            let tx = self.csv_import_channel.0.clone();
                                            PlatformSpawner::spawn(async move {
                                                if let Some(file) = rfd::AsyncFileDialog::new()
                                                    .add_filter("CSV / Text", &["csv", "txt"])
                                                    .set_title("Choose a list of serials to import")
                                                    .pick_file()
                                                    .await
                                                {
                                                    let contents = file.read().await;
                                                    let text = String::from_utf8_lossy(&contents);
                                                    let _ = tx.send(parse_serial_list(&text));
                                                }
                                            });
                                        }
                                    }
                                    ImportKick::Paste => {
                                        self.import_panel_open = true;
                                    }
                                }
                            }

                            ui.add_space(10.);

                            // ---- Start / Done Scanning toggle ----
                            let in_audit = matches!(self.inventory_view, InventoryView::Audit(_));
                            if in_audit {
                                let btn_label = if self.scan_mode_active { "Done" } else { "Start Scanning" };
                                if Button::new(btn_label).ui(ui).clicked() {
                                    self.scan_mode_active = !self.scan_mode_active;
                                    self.scan_input.clear();
                                    self.scan_feedback = None;
                                }
                            }
                            if self.import_in_progress {
                                Spinner::new().size(16.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);
                            ui.separator();
                            ui.add_space(10.);
                            self.serial_history_scan_input(ui);

                            // ---- Focus-locked scan input ----
                            if self.scan_mode_active && in_audit {
                                self.audit_scan_input(ui);
                            }
                            if let Some((msg, color)) = &self.scan_feedback {
                                ui.colored_label(*color, msg);
                            }
                        },
                        StockSelection::CostBreakdown => {
                            TextEdit::singleline(&mut self.cost_order_id)
                                .desired_width(200.)
                                .hint_text("Enter Order ID")
                                .ui(ui);

                            ui.add_space(10.);

                            let can_search = !self.cost_order_id.is_empty() && !self.cost_loading;
                            if ui.add_enabled(can_search, Button::new("Search")).clicked() {
                                let cost_tx = self.cost_channel.0.clone();
                                let summary_tx = self.cost_summary_channel.0.clone();
                                let order_id = self.cost_order_id.clone();
                                self.cost_loading = true;
                                self.cost_summary = None;
                                self.cost_breakdown_viewer.clear_selection();
                                PlatformSpawner::spawn(async move {
                                    let _ = get_cost_breakdown(order_id, cost_tx, summary_tx).await;
                                });
                            }

                            if self.cost_loading {
                                ui.add_space(5.);
                                Spinner::new().size(18.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);

                            TextEdit::singleline(&mut self.cost_breakdown_viewer.filter)
                                .desired_width(250.)
                                .hint_text("Filter results")
                                .ui(ui);
                        },
                        StockSelection::SystemsInStore => {
                            // Store selection for Systems In-Store
                            let selected = &mut self.store_selection;
                            let current = *selected;
                            
                            let selected_text = Store::from_presta_store_id(&selected.to_string());
                            
                            ComboBox::new("Systems_Store_Selection", "")
                                .selected_text(selected_text.as_str())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected, Store::RIV.into_store_id() as u64, Store::RIV.as_str());
                                    ui.selectable_value(selected, Store::LTN.into_store_id() as u64, Store::LTN.as_str());
                                    ui.selectable_value(selected, Store::MUR.into_store_id() as u64, Store::MUR.as_str());
                                    ui.selectable_value(selected, Store::ORE.into_store_id() as u64, Store::ORE.as_str());
                                    ui.selectable_value(selected, Store::SAN.into_store_id() as u64, Store::SAN.as_str());
                                });
                            
                            // Trigger refresh when store changes
                            if *selected != current {
                                self.systems_first_load = true;
                            }
                            
                            ui.add_space(10.);

                            if Button::new("Refresh").ui(ui).clicked() || self.systems_first_load {
                                self.systems_first_load = false;
                                self.systems_loading = true;
                                let systems_tx = self.systems_channel.0.clone();
                                // store_selection is shared with the Store-Inventory view, which
                                // writes Odoo ids; normalize to a PrestaShop id so first-load and
                                // subsequent refreshes always hit the right backend identifier.
                                let store_id = Store::from_any_store_id(&self.store_selection.to_string())
                                    .into_store_id() as u64;
                                PlatformSpawner::spawn(async move {
                                    let _ = get_systems_in_store(store_id, systems_tx).await;
                                });
                            }
                            
                            ui.add_space(10.);
                            ui.separator();
                            ui.add_space(10.);
                            
                            // Manual order add
                            TextEdit::singleline(&mut self.systems_order_id)
                                .desired_width(120.)
                                .hint_text("Add Order ID")
                                .ui(ui);

                            ui.add_space(5.);

                            let can_add = !self.systems_order_id.is_empty() && !self.systems_loading;
                            if ui.add_enabled(can_add, Button::new("Add")).clicked() {
                                let systems_tx = self.systems_add_channel.0.clone();
                                let order_id = self.systems_order_id.clone();
                                self.systems_order_id.clear();
                                PlatformSpawner::spawn(async move {
                                    let _ = add_order_to_systems(order_id, systems_tx).await;
                                });
                            }

                            if self.systems_loading {
                                ui.add_space(5.);
                                Spinner::new().size(18.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);

                            TextEdit::singleline(&mut self.systems_in_store_viewer.filter)
                                .desired_width(200.)
                                .hint_text("Filter systems")
                                .ui(ui);
                        },
                        StockSelection::Everest => {
                            let response = TextEdit::singleline(&mut self.everest_serial_input)
                                .desired_width(220.)
                                .hint_text("Scan / enter MFG serial")
                                .ui(ui);

                            ui.add_space(8.);

                            let can_lookup = !self.everest_serial_input.trim().is_empty() && !self.everest_loading;
                            let enter_submit = response.lost_focus()
                                && ui.input(|i| i.key_pressed(Key::Enter))
                                && can_lookup;
                            if ui.add_enabled(can_lookup, Button::new("Lookup")).clicked() || enter_submit {
                                let serial = self.everest_serial_input.trim().to_string();
                                self.everest_loading = true;
                                self.everest_error = None;
                                self.everest_order = None;
                                self.everest_selected_serial = None;
                                self.everest_history = None;
                                self.everest_items_table.replace(Vec::new());
                                self.everest_view = EverestView::OrderDetail;
                                self.everest_order_intent = EverestOrderIntent::Reset;
                                self.everest_crumbs.clear();
                                let tx = self.everest_order_channel.0.clone();
                                PlatformSpawner::spawn(async move {
                                    if let Err(e) = lookup_everest_order(serial, tx).await {
                                        log::error!("Everest lookup error: {e:?}");
                                    }
                                });
                            }

                            if self.everest_loading {
                                ui.add_space(6.);
                                Spinner::new().size(18.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);
                            ui.separator();
                            ui.add_space(10.);

                            // ---- Customer search (phone / email) ----
                            ui.selectable_value(&mut self.everest_customer_search_type, CustomerSearchType::Phone, "Phone");
                            ui.selectable_value(&mut self.everest_customer_search_type, CustomerSearchType::Email, "Email");

                            let cust_resp = TextEdit::singleline(&mut self.everest_customer_query)
                                .desired_width(200.)
                                .hint_text(match self.everest_customer_search_type {
                                    CustomerSearchType::Phone => "Customer phone",
                                    CustomerSearchType::Email => "Customer email",
                                })
                                .ui(ui);

                            let can_search = !self.everest_customer_query.trim().is_empty() && !self.everest_search_loading;
                            let search_submit = cust_resp.lost_focus()
                                && ui.input(|i| i.key_pressed(Key::Enter))
                                && can_search;
                            if ui.add_enabled(can_search, Button::new("Search")).clicked() || search_submit {
                                let query = self.everest_customer_query.trim().to_string();
                                let by_email = matches!(self.everest_customer_search_type, CustomerSearchType::Email);
                                self.everest_search_loading = true;
                                self.everest_error = None;
                                self.everest_search_result = None;
                                self.everest_view = EverestView::Results;
                                let tx = self.everest_search_channel.0.clone();
                                PlatformSpawner::spawn(async move {
                                    if let Err(e) = search_everest_customers(query, by_email, tx).await {
                                        log::error!("Everest customer search error: {e:?}");
                                    }
                                });
                            }

                            if self.everest_search_loading {
                                Spinner::new().size(16.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);

                            TextEdit::singleline(&mut self.everest_items_viewer.filter)
                                .desired_width(200.)
                                .hint_text("Filter rows")
                                .ui(ui);

                            if let Some(err) = &self.everest_error {
                                ui.add_space(10.);
                                ui.colored_label(ui.global_style().visuals.error_fg_color, err);
                            }
                        },
                    }
                });
            });

        // Bottom panel for Cost Breakdown summary
        if self.stock_selection == StockSelection::CostBreakdown {
            // Calculate selection sums
            let selected_products = &self.cost_breakdown_viewer.selected_products;
            let (selected_unit_price_sum, selected_cost_sum): (f64, f64) = if !selected_products.is_empty() {
                self.cost_breakdown_table
                    .iter()
                    .filter(|row| selected_products.contains(&format!("{}:{}", row.0, row.1)))
                    .fold((0.0, 0.0), |(price_acc, cost_acc), row| {
                        // row.4 = unit_price, row.5 = cost, row.3 = quantity
                        (price_acc + (row.4 * row.3), cost_acc + (row.5 * row.3))
                    })
            } else {
                (0.0, 0.0)
            };
            let selection_count = selected_products.len();
            
            if let Some(ref summary) = self.cost_summary {
                eframe::egui::Panel::bottom("CostBreakdownBottom")
                    .exact_size(30.)
                    .show_inside(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 25.0;
                            Hyperlink::from_label_and_url(
                                RichText::new(self.cost_order_id.clone())
                                    .underline()
                                    .strong()
                                    .color(ui.global_style().visuals.error_fg_color),
                                xidax_order_url(&self.cost_order_id),
                            )
                            .open_in_new_tab(true)
                            .ui(ui);

                            ui.colored_label(Color32::LIGHT_BLUE, format!("Customer: {}", summary.customer_name));
                            
                            ui.label(format!("Order Total: ${:.2}", summary.order_total));
                            
                            ui.colored_label(
                                Color32::from_rgb(200, 100, 100),
                                format!("Cost: ${:.2}", summary.total_cost)
                            );
                            
                            let profit_color = if summary.profit >= 0.0 {
                                Color32::LIGHT_GREEN
                            } else {
                                ui.global_style().visuals.error_fg_color
                            };
                            ui.colored_label(
                                profit_color,
                                format!("Gross Profit: ${:.2}", summary.profit)
                            );
                            
                            // Show selection sum if any items are selected
                            if selection_count > 0 {
                                ui.separator();
                                ui.colored_label(
                                    Color32::GOLD,
                                    format!("Selected ({}):", selection_count)
                                );
                                ui.label(format!("Price: ${:.2}", selected_unit_price_sum));
                                ui.colored_label(
                                    Color32::from_rgb(200, 100, 100),
                                    format!("Cost: ${:.2}", selected_cost_sum)
                                );
                                let sel_profit = selected_unit_price_sum - selected_cost_sum;
                                let sel_profit_color = if sel_profit >= 0.0 {
                                    Color32::LIGHT_GREEN
                                } else {
                                    ui.global_style().visuals.error_fg_color
                                };
                                ui.colored_label(sel_profit_color, format!("Profit: ${:.2}", sel_profit));
                            }
                        });
                    });
            }
        }

        // Floating Odoo-history windows (Store Inventory clicks land here).
        // Rendered against the root context so they aren't clipped to the
        // central panel.
        render_history_windows(ui.ctx(), &mut self.history_windows);

        CentralPanel::default().show_inside(ui, |ui| {
            // Right-side paste-list import panel (Store Inventory only).
            if self.stock_selection == StockSelection::StoreInventory && self.import_panel_open {
                Panel::right("inventory_import_panel")
                    .resizable(true)
                    .default_size(420.)
                    .min_size(320.)
                    .show_inside(ui, |ui| {
                        self.render_import_panel(ui);
                    });
            }

            // Shared right-side history panel for Company Stock / Store Inventory tabs.
            // (The Everest tab manages its own panels inside show_everest.)
            let show_shared_history = matches!(
                self.stock_selection,
                StockSelection::CompanyStock | StockSelection::StoreInventory
            ) && (self.everest_history.is_some() || self.everest_history_loading);
            if show_shared_history {
                Panel::right("shared_serial_history_panel")
                    .resizable(true)
                    .default_size(360.)
                    .min_size(280.)
                    .show_inside(ui, |ui| {
                        self.render_serial_history_panel(ui);
                    });
            }

            match self.stock_selection {
                StockSelection::CompanyStock => {
                    if self.stock_quantity_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.label("Pulling Company Stock Information..");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else {   
                        Renderer::new(
                            &mut self.stock_quantity_table,
                            &mut self.stock_quantity_viewer,
                        )
                        .with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                },
                StockSelection::StoreInventory => {
                    if self.inventory_serials_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.label("Pulling Store Stock Information..");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else {
                        Renderer::new(
                            &mut self.inventory_serials_table, 
                            &mut self.inventory_serials_viewer
                        ).with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                },
                StockSelection::CostBreakdown => {
                    if self.cost_loading {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("Fetching order cost breakdown...");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else if self.cost_breakdown_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("Enter an Order ID above and click Search to view cost breakdown.");
                        });
                    } else {
                        Renderer::new(
                            &mut self.cost_breakdown_table, 
                            &mut self.cost_breakdown_viewer
                        ).with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                },
                StockSelection::SystemsInStore => {
                    if self.systems_loading {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("Fetching systems in-store...");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else if self.systems_in_store_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("No systems found. Select a store and click Refresh to load systems.");
                        });
                    } else {
                        Renderer::new(
                            &mut self.systems_in_store_table,
                            &mut self.systems_in_store_viewer
                        ).with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }

                    // Show customer change modal if open
                    self.show_customer_modal(ui);
                },
                StockSelection::Everest => self.show_everest(ui),
            }
        });
    }

    /// Kick off an audit import (the Odoo lookup phase). The save-and-swap
    /// happens later when `audit_save_channel` fires.
    fn kick_off_import(&mut self, serials: Vec<String>) {
        if serials.is_empty() {
            return;
        }
        self.import_in_progress = true;
        self.scan_feedback = None;
        let tx = self.audit_lookup_channel.0.clone();
        let prices = self.extra_stock_prices.clone();
        PlatformSpawner::spawn(async move {
            if let Err(e) = lookup_serials_in_odoo(serials, prices, tx).await {
                log::error!("lookup_serials_in_odoo error: {e:?}");
            }
        });
    }

    /// Focus-locked scan input rendered while `scan_mode_active`. The
    /// barcode scanner's post-serial Enter normally makes a
    /// `TextEdit::singleline` surrender focus; we explicitly re-grab it
    /// **only when the widget isn't already focused**. Calling
    /// `request_focus` unconditionally every frame fights TextEdit's
    /// internal surrender_focus on Enter, which is why Enter previously
    /// "did nothing" — the response never reported `lost_focus` and our
    /// submit branch never fired.
    fn audit_scan_input(&mut self, ui: &mut Ui) {
        let id = Id::new("inventory_scan_input");
        let response = TextEdit::singleline(&mut self.scan_input)
            .id(id)
            .desired_width(220.)
            .hint_text("Scan serial → Enter")
            .ui(ui);

        let submitted = response.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter));
        if submitted && !self.scan_input.trim().is_empty() {
            let serial = self.scan_input.trim().to_string();
            self.scan_input.clear();
            self.handle_scan_submit(&serial);
        }

        // Re-grab focus *only* if we lost it (first-frame of scan mode,
        // or right after the Enter-triggered surrender). Don't pre-empt
        // a focused widget — that's what broke the Enter detection.
        if !response.has_focus() {
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// Look up `serial` in the currently displayed audit table, flip the
    /// found flag if present, and persist via the channel→`mark_found`
    /// pipeline. Updates `scan_feedback` so the operator sees what
    /// happened.
    fn handle_scan_submit(&mut self, serial: &str) {
        let audit_id = match &self.inventory_view {
            InventoryView::Audit(id) => id.clone(),
            _ => return,
        };
        let mut found_match = false;
        let mut already_found = false;
        let mut data = self.inventory_serials_table.take();
        for row in data.iter_mut() {
            if row.4.eq_ignore_ascii_case(serial) {
                found_match = true;
                if row.7 {
                    already_found = true;
                } else {
                    row.7 = true;
                }
                break;
            }
        }
        self.inventory_serials_table.replace(data);
        if found_match {
            if already_found {
                self.scan_feedback = Some((format!("• {serial}: already marked"), Color32::GRAY));
            } else {
                self.scan_feedback = Some((format!("✓ {serial}: marked"), Color32::LIGHT_GREEN));
                let tx = self.found_toggle_channel.0.clone();
                let _ = tx.try_send((audit_id, serial.to_string(), true));
            }
        } else {
            self.scan_feedback = Some((
                format!("✗ {serial}: not in this audit"),
                Color32::from_rgb(220, 120, 120),
            ));
        }
    }

    /// Compact serial-history scan input shown in the Company Stock / Store Inventory
    /// headers. Submitting fires the same Odoo lookup as the Everest tab and opens
    /// the right-side history panel.
    fn serial_history_scan_input(&mut self, ui: &mut Ui) {
        let response = TextEdit::singleline(&mut self.serial_history_input)
            .desired_width(180.)
            .hint_text("S/N -> Odoo history")
            .ui(ui);

        let can_lookup = !self.serial_history_input.trim().is_empty() && !self.everest_history_loading;
        let enter_submit = response.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && can_lookup;
        if ui.add_enabled(can_lookup, Button::new("Lookup")).clicked() || enter_submit {
            let serial = self.serial_history_input.trim().to_string();
            self.everest_selected_serial = Some(serial.clone());
            self.everest_history = None;
            self.everest_history_loading = true;
            let tx = self.everest_history_channel.0.clone();
            PlatformSpawner::spawn(async move {
                if let Err(e) = fetch_serial_movement(serial, tx).await {
                    log::error!("Odoo serial history error: {e:?}");
                }
            });
        }

        if self.everest_history_loading {
            Spinner::new().size(16.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
        }
    }

    fn show_everest(&mut self, ui: &mut Ui) {
        let in_detail = self.everest_view == EverestView::OrderDetail;

        // Bottom: order totals summary (only on the order-detail view).
        if let Some(order) = self.everest_order.as_ref().filter(|_| in_detail) {
            let totals = order_totals(order);
            let kit_codes: Vec<String> = order
                .items
                .iter()
                .filter_map(|i| i.kit_code.clone())
                .filter(|s| !s.is_empty())
                .collect();
            let kit_summary = {
                let mut uniq: Vec<String> = Vec::new();
                for k in kit_codes.iter() {
                    if !uniq.contains(k) { uniq.push(k.clone()); }
                }
                uniq
            };

            Panel::bottom("EverestBottom").show_inside(ui, |ui| {
                ui.add_space(4.);
                ui.columns(5, |cols| {
                    cols[0].label(format!("Items: {}", order.items.len()));

                    cols[1].colored_label(
                        Color32::from_rgb(200, 100, 100),
                        format!("Cost: $ {:.2}", totals.cost),
                    );

                    cols[2].colored_label(
                        cols[2].style().visuals.warn_fg_color,
                        format!("Revenue: $ {:.2}", totals.revenue),
                    );

                    let profit_color = if totals.profit >= 0.0 {
                        Color32::LIGHT_GREEN
                    } else {
                        cols[3].style().visuals.error_fg_color
                    };
                    cols[3].colored_label(
                        profit_color,
                        format!("Profit: $ {:.2} ({:.1}%)", totals.profit, totals.margin_pct()),
                    );

                    if kit_summary.is_empty() {
                        cols[4].label("");
                    } else {
                        cols[4].colored_label(
                            Color32::from_rgb(255, 180, 80),
                            format!("Kit: {}", kit_summary.join(", ")),
                        );
                    }
                });
                ui.add_space(4.);
            });
        }

        // Right: serial-history side panel, only when populated (detail only).
        if in_detail && (self.everest_history.is_some() || self.everest_history_loading) {
            Panel::right("everest_serial_history_panel")
                .resizable(true)
                .default_size(360.)
                .min_size(280.)
                .show_inside(ui, |ui| {
                    self.render_serial_history_panel(ui);
                });
        }

        // Left: customer + addresses, only when an order is loaded (detail only).
        if in_detail && self.everest_order.is_some() {
            Panel::left("everest_customer_panel")
                .resizable(true)
                .default_size(300.)
                .min_size(220.)
                .show_inside(ui, |ui| {
                    self.render_customer_panel(ui);
                });
        }

        // Center: breadcrumb trail above the active view's content.
        CentralPanel::default().show_inside(ui, |ui| {
            self.render_everest_breadcrumb(ui);

            match self.everest_view {
                EverestView::Results => { self.render_everest_results(ui); return; }
                EverestView::CustomerOrders => { self.render_everest_orders(ui); return; }
                EverestView::Empty => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.);
                        ui.label("Scan a serial number, or search for a customer by phone / email.");
                    });
                    return;
                }
                EverestView::OrderDetail => {}
            }

            if self.everest_loading && self.everest_order.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.);
                    ui.label("Looking up Everest order...");
                    Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                });
            } else if self.everest_items_table.len() < 1 {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.);
                    ui.label("Scan a serial number to look up the matching Everest order.");
                });
            } else {
                if let Some(order) = &self.everest_order {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.0;

                        // Invoice number — the headline value, no "DOC #" prefix.
                        ui.label(
                            RichText::new(&order.header.doc_no)
                                .strong()
                                .size(18.0)
                                .color(Color32::from_rgb(255, 200, 80)),
                        );

                        if !order.header.doc_alias.is_empty() {
                            ui.separator();
                            ui.colored_label(Color32::LIGHT_BLUE, &order.header.doc_alias);
                        }

                        if let Some(date) = order.header.order_date.as_ref() {
                            let trimmed = date.split_whitespace().next().unwrap_or(date);
                            ui.separator();
                            ui.label(RichText::new(trimmed).color(Color32::LIGHT_GRAY));
                        }

                        if !order.header.dep.is_empty() {
                            ui.separator();
                            ui.colored_label(Color32::LIGHT_GREEN, &order.header.dep);
                        }

                        if !order.header.sales_rep.is_empty() {
                            ui.separator();
                            ui.label(format!("Rep  {}", order.header.sales_rep));
                        }

                        if !order.header.terms.is_empty() {
                            ui.separator();
                            ui.label(RichText::new(&order.header.terms).color(Color32::LIGHT_GRAY));
                        }
                    });
                    ui.add_space(4.0);
                    ui.separator();
                }
                Renderer::new(&mut self.everest_items_table, &mut self.everest_items_viewer)
                    .with_style_modify(|s| {
                        s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                        s.single_click_edit_mode = true;
                        s.auto_shrink = [false, false].into();
                    })
                    .ui(ui);
            }
        });

        // Apply any navigation collected during rendering (after all borrows
        // of `self` from the panels/closures above are released).
        if let Some(nav) = self.everest_nav.take() {
            self.apply_everest_nav(nav);
        }
    }

    /// Breadcrumb trail for the Everest tab. Clicking an earlier crumb
    /// restores that view from cache (no re-fetch).
    fn render_everest_breadcrumb(&mut self, ui: &mut Ui) {
        if self.everest_crumbs.is_empty() {
            return;
        }
        let crumbs = self.everest_crumbs.clone();
        let last = crumbs.len().saturating_sub(1);
        ui.horizontal(|ui| {
            for (i, crumb) in crumbs.iter().enumerate() {
                if i > 0 {
                    ui.label(RichText::new("›").color(Color32::GRAY));
                }
                let label = match crumb {
                    EverestCrumb::Results => "Search Results".to_string(),
                    EverestCrumb::Customer { label, .. } => label.clone(),
                    EverestCrumb::Order { label, .. } => format!("Order {label}"),
                };
                if i == last {
                    ui.label(RichText::new(label).strong().color(Color32::from_rgb(255, 200, 80)));
                } else if ui.link(RichText::new(label).color(Color32::LIGHT_BLUE)).clicked() {
                    self.everest_nav = Some(EverestNav::Crumb(i));
                }
            }
        });
        ui.separator();
    }

    /// Customer search-results table (paged 20 at a time).
    fn render_everest_results(&mut self, ui: &mut Ui) {
        if self.everest_search_loading {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label("Searching Everest customers...");
                Spinner::new().size(40.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
            });
            return;
        }
        let Some(res) = self.everest_search_result.as_ref() else {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label("No search run yet.");
            });
            return;
        };
        let total = res.customers.len();
        if total == 0 {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label(RichText::new("No customers found.").color(Color32::GRAY));
            });
            return;
        }
        let shown = self.everest_results_shown.min(total);
        let rows: Vec<EverestRow> = res.customers[..shown].to_vec();

        ui.label(
            RichText::new(format!("Showing {shown} of {total} customers"))
                .color(Color32::LIGHT_BLUE),
        );
        ui.add_space(4.);

        ScrollArea::vertical().show(ui, |ui| {
            use egui_extras::{Column as TblCol, TableBuilder};
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(TblCol::remainder().at_least(180.))
                .column(TblCol::auto().at_least(90.))
                .column(TblCol::auto().at_least(120.))
                .column(TblCol::remainder().at_least(160.))
                .column(TblCol::auto().at_least(70.))
                .header(20., |mut h| {
                    h.col(|ui| { ui.strong("Name"); });
                    h.col(|ui| { ui.strong("Cust Code"); });
                    h.col(|ui| { ui.strong("Phone"); });
                    h.col(|ui| { ui.strong("Email"); });
                    h.col(|ui| { ui.strong("Invoices"); });
                })
                .body(|mut body| {
                    for row in rows.iter() {
                        let name = row_customer_name(row);
                        let code = row_cust_code(row);
                        let phone = row_str(row, &["TEL1", "TEL2", "MOBILE_PHONE", "PHONE"]);
                        let email = row_str(row, &["EMAIL"]);
                        let invoices = row_str(row, &["total_documents", "NUM_INV"]);
                        body.row(22., |mut r| {
                            r.col(|ui| {
                                if Link::new(RichText::new(&name).color(Color32::LIGHT_BLUE)).ui(ui).clicked() {
                                    self.everest_nav = Some(EverestNav::OpenCustomer {
                                        cust_code: code.clone(),
                                        label: name.clone(),
                                    });
                                }
                            });
                            r.col(|ui| { ui.label(&code); });
                            r.col(|ui| { ui.label(&phone); });
                            r.col(|ui| { ui.label(&email); });
                            r.col(|ui| { ui.label(&invoices); });
                        });
                    }
                });
        });

        if shown < total {
            ui.add_space(6.);
            if Button::new(format!("Load +20  ({} remaining)", total - shown)).ui(ui).clicked() {
                self.everest_nav = Some(EverestNav::LoadMore);
            }
        }
    }

    /// All orders on the selected customer's account.
    fn render_everest_orders(&mut self, ui: &mut Ui) {
        if self.everest_orders_loading {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label("Loading customer orders...");
                Spinner::new().size(40.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
            });
            return;
        }
        let cust = self.everest_current_cust.clone();
        let orders = self.everest_orders_by_cust.get(&cust).cloned().unwrap_or_default();
        if orders.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label(RichText::new("No orders found for this customer.").color(Color32::GRAY));
            });
            return;
        }

        ui.label(RichText::new(format!("{} orders", orders.len())).color(Color32::LIGHT_BLUE));
        ui.add_space(4.);

        ScrollArea::vertical().show(ui, |ui| {
            use egui_extras::{Column as TblCol, TableBuilder};
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(TblCol::auto().at_least(110.))
                .column(TblCol::remainder().at_least(140.))
                .column(TblCol::auto().at_least(100.))
                .column(TblCol::auto().at_least(70.))
                .column(TblCol::auto().at_least(90.))
                .column(TblCol::auto().at_least(100.))
                .header(20., |mut h| {
                    h.col(|ui| { ui.strong("Order #"); });
                    h.col(|ui| { ui.strong("Type"); });
                    h.col(|ui| { ui.strong("Date"); });
                    h.col(|ui| { ui.strong("Dept"); });
                    h.col(|ui| { ui.strong("Sales Rep"); });
                    h.col(|ui| { ui.strong("Amount"); });
                })
                .body(|mut body| {
                    for row in orders.iter() {
                        let doc = row_doc_no(row);
                        let alias = row_str(row, &["DOC_ALIAS"]);
                        let date = row_str(row, &["ORDER_DATE", "DATE", "DOC_DATE"]);
                        let date = date.split_whitespace().next().unwrap_or(&date).to_string();
                        let dep = row_str(row, &["DEP", "DEP_CODE", "DEPARTMENT"]);
                        let rep = row_str(row, &["SALES_REP", "REP"]);
                        let amount = row_str(row, &["INV_AMOUNT", "AMOUNT", "TOTAL"]);
                        body.row(22., |mut r| {
                            r.col(|ui| {
                                if doc.is_empty() {
                                    ui.label(RichText::new("—").color(Color32::GRAY));
                                } else if Link::new(RichText::new(&doc).color(Color32::from_rgb(42, 195, 222))).ui(ui).clicked() {
                                    self.everest_nav = Some(EverestNav::OpenOrder {
                                        doc_no: doc.clone(),
                                        label: doc.clone(),
                                    });
                                }
                            });
                            r.col(|ui| { ui.label(RichText::new(&alias).color(Color32::LIGHT_GRAY)); });
                            r.col(|ui| { ui.label(&date); });
                            r.col(|ui| { ui.label(&dep); });
                            r.col(|ui| { ui.label(&rep); });
                            r.col(|ui| {
                                if amount.is_empty() {
                                    ui.label("");
                                } else {
                                    ui.label(RichText::new(format!("$ {amount}")).color(Color32::LIGHT_GREEN));
                                }
                            });
                        });
                    }
                });
        });
    }

    /// Apply a collected Everest navigation action.
    fn apply_everest_nav(&mut self, nav: EverestNav) {
        match nav {
            EverestNav::LoadMore => {
                self.everest_results_shown += 20;
            }
            EverestNav::Crumb(i) => self.everest_goto_crumb(i),
            EverestNav::OpenCustomer { cust_code, label } => {
                self.everest_current_cust = cust_code.clone();
                self.everest_view = EverestView::CustomerOrders;
                self.everest_crumbs.push(EverestCrumb::Customer {
                    cust_code: cust_code.clone(),
                    label,
                });
                if !self.everest_orders_by_cust.contains_key(&cust_code) {
                    self.everest_orders_loading = true;
                    let tx = self.everest_orders_channel.0.clone();
                    PlatformSpawner::spawn(async move {
                        if let Err(e) = fetch_customer_orders(cust_code, tx).await {
                            log::error!("Everest customer orders error: {e:?}");
                        }
                    });
                }
            }
            EverestNav::OpenOrder { doc_no, label } => {
                self.everest_crumbs.push(EverestCrumb::Order {
                    doc_no: doc_no.clone(),
                    label,
                });
                self.everest_view = EverestView::OrderDetail;
                if let Some(order) = self.everest_order_by_doc.get(&doc_no).cloned() {
                    self.everest_items_table.replace(order_to_rows(&order));
                    self.everest_order = Some(order);
                } else {
                    self.everest_loading = true;
                    self.everest_error = None;
                    self.everest_order_intent = EverestOrderIntent::AlreadyPushed;
                    let tx = self.everest_order_channel.0.clone();
                    PlatformSpawner::spawn(async move {
                        if let Err(e) = lookup_everest_order_by_docnum(doc_no, tx).await {
                            log::error!("Everest order-by-docnum error: {e:?}");
                        }
                    });
                }
            }
            EverestNav::OpenSerial(serial) => {
                self.everest_loading = true;
                self.everest_error = None;
                self.everest_order_intent = EverestOrderIntent::PushOnArrival;
                let tx = self.everest_order_channel.0.clone();
                PlatformSpawner::spawn(async move {
                    if let Err(e) = lookup_everest_order(serial, tx).await {
                        log::error!("Everest serial lookup error: {e:?}");
                    }
                });
            }
        }
    }

    /// Jump back to crumb `i`, truncating the trail and restoring that view
    /// entirely from cached data.
    fn everest_goto_crumb(&mut self, i: usize) {
        if i >= self.everest_crumbs.len() {
            return;
        }
        self.everest_crumbs.truncate(i + 1);
        match self.everest_crumbs[i].clone() {
            EverestCrumb::Results => {
                self.everest_view = EverestView::Results;
            }
            EverestCrumb::Customer { cust_code, .. } => {
                self.everest_current_cust = cust_code;
                self.everest_view = EverestView::CustomerOrders;
            }
            EverestCrumb::Order { doc_no, .. } => {
                if let Some(order) = self.everest_order_by_doc.get(&doc_no).cloned() {
                    self.everest_items_table.replace(order_to_rows(&order));
                    self.everest_order = Some(order);
                }
                self.everest_view = EverestView::OrderDetail;
            }
        }
    }

    /// Right-side panel where the user pastes one serial per line. The
    /// Import button kicks off the same Odoo lookup → save_audit flow
    /// the CSV path uses.
    fn render_import_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Import Serials");
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if ui.small_button("✕").on_hover_text("Close").clicked() {
                        self.import_panel_open = false;
                    }
                },
            );
        });
        ui.label(
            RichText::new(
                "Paste one serial per line. Looks each one up in Odoo and saves a new audit for the selected store.",
            )
            .color(Color32::GRAY),
        );
        ui.add_space(6.);
        ScrollArea::vertical()
            .max_height(ui.available_height() - 90.)
            .show(ui, |ui| {
                TextEdit::multiline(&mut self.import_textarea)
                    .desired_rows(20)
                    .desired_width(f32::INFINITY)
                    .font(eframe::egui::TextStyle::Monospace)
                    .ui(ui);
            });
        ui.add_space(8.);

        let parsed = parse_serial_list(&self.import_textarea);
        let count = parsed.len();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{count} serials parsed"))
                    .color(if count == 0 { Color32::GRAY } else { Color32::LIGHT_GREEN }),
            );
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    let enabled = count > 0 && !self.import_in_progress;
                    if ui.add_enabled(enabled, Button::new("Import")).clicked() {
                        self.kick_off_import(parsed);
                    }
                    if ui.button("Clear").clicked() {
                        self.import_textarea.clear();
                    }
                },
            );
        });
    }

    fn render_customer_panel(&mut self, ui: &mut Ui) {
        let Some(order) = &self.everest_order else { return; };
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Customer");
            ui.separator();

            let display_name = {
                let h = &order.header;
                let fl = format!("{} {}", h.first_name.trim(), h.last_name.trim());
                if !fl.trim().is_empty() { fl.trim().to_string() }
                else if !h.name.is_empty() { h.name.clone() }
                else if !h.acct_name.is_empty() { h.acct_name.clone() }
                else { "Unknown".to_string() }
            };
            ui.label(RichText::new(&display_name).strong().color(Color32::LIGHT_BLUE));
            if !order.header.cust_code.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Code").color(Color32::GRAY));
                    ui.label(
                        RichText::new(&order.header.cust_code)
                            .strong()
                            .color(Color32::from_rgb(255, 200, 80)),
                    );
                });
            }
            if !order.header.email.is_empty() {
                ui.label(format!("✉ {}", order.header.email));
            }
            if !order.header.tel1.is_empty() {
                ui.label(format!("📞 {}", order.header.tel1));
            }
            if !order.header.tel2.is_empty() && order.header.tel2 != order.header.tel1 {
                ui.label(format!("📞 {}", order.header.tel2));
            }

            ui.add_space(8.);
            if !order.customer.num_inv.is_empty() {
                ui.label(
                    RichText::new(format!("Invoices: {}", order.customer.num_inv))
                        .color(Color32::GRAY),
                );
            }
            if !order.customer.inv_life.is_empty() {
                ui.label(
                    RichText::new(format!("Lifetime: $ {}", order.customer.inv_life))
                        .color(Color32::GRAY),
                );
            }

            ui.add_space(12.);
            ui.heading("Addresses");
            ui.separator();
            if order.addresses.is_empty() {
                ui.label(RichText::new("No addresses on file.").color(Color32::GRAY));
            } else {
                for (idx, addr) in order.addresses.iter().enumerate() {
                    ui.group(|ui| {
                        if !addr.name.is_empty() {
                            ui.label(RichText::new(&addr.name).strong());
                        } else {
                            ui.label(RichText::new(format!("Address {}", idx + 1)).strong());
                        }
                        if !addr.full_address.is_empty() {
                            ui.label(RichText::new(&addr.full_address).color(Color32::LIGHT_GRAY));
                        } else {
                            if !addr.street_address.is_empty() { ui.label(&addr.street_address); }
                            let line2 = [&addr.city, &addr.state, &addr.zip]
                                .iter().filter(|s| !s.is_empty()).map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
                            if !line2.is_empty() { ui.label(line2); }
                            if !addr.country.is_empty() { ui.label(&addr.country); }
                        }
                        if !addr.tel1.is_empty() { ui.label(format!("📞 {}", addr.tel1)); }
                        if !addr.mobile_phone.is_empty() { ui.label(format!("📱 {}", addr.mobile_phone)); }
                        if !addr.email.is_empty() { ui.label(format!("✉ {}", addr.email)); }
                    });
                    ui.add_space(4.);
                }
            }
        });
    }

    fn render_serial_history_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Serial History");
            if ui.small_button(crate::ui_tools::icons::CLOSE).on_hover_text("Close").clicked() {
                self.everest_history = None;
                self.everest_selected_serial = None;
            }
        });
        if let Some(serial) = &self.everest_selected_serial {
            ui.label(RichText::new(serial).color(Color32::from_rgb(42, 195, 222)));
        }
        if self.everest_history_loading {
            ui.add_space(10.);
            ui.horizontal(|ui| {
                Spinner::new().size(16.).ui(ui);
                ui.label("Fetching Odoo movements...");
            });
            return;
        }

        let Some(history) = &self.everest_history else { return; };

        if let Some(p) = &history.product_name {
            if !p.is_empty() {
                ui.label(RichText::new(p).color(Color32::GRAY));
            }
        }
        if let Some(err) = &history.error {
            ui.colored_label(ui.global_style().visuals.error_fg_color, err);
            return;
        }
        ui.separator();
        if history.moves.is_empty() {
            ui.label(RichText::new("No movements found.").color(Color32::GRAY));
            return;
        }

        ScrollArea::vertical().show(ui, |ui| {
            use egui_extras::{TableBuilder, Column as TblCol};
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(TblCol::auto().at_least(95.))
                .column(TblCol::auto().at_least(75.))
                .column(TblCol::auto().at_least(110.))
                .column(TblCol::auto().at_least(110.))
                .column(TblCol::remainder().at_least(120.))
                .column(TblCol::auto().at_least(40.))
                .header(20., |mut h| {
                    h.col(|ui| { ui.strong("Date"); });
                    h.col(|ui| { ui.strong("State"); });
                    h.col(|ui| { ui.strong("From"); });
                    h.col(|ui| { ui.strong("To"); });
                    h.col(|ui| { ui.strong("Reference"); });
                    h.col(|ui| { ui.strong("Qty"); });
                })
                .body(|mut body| {
                    for m in history.moves.iter() {
                        // Trimmed raw timestamp ("YYYY-MM-DD HH:MM:SS")
                        // is the stable key for the expanded-dates set
                        // and the input to the formatters.
                        let raw_date = {
                            let s = m.date.clone().unwrap_or_default();
                            s.split('.').next().unwrap_or(&s).to_string()
                        };
                        body.row(20., |mut row| {
                            row.col(|ui| {
                                let expanded = self.expanded_history_dates.contains(&raw_date);
                                let display = if expanded {
                                    format_date_long(&raw_date)
                                } else {
                                    format_date_short(&raw_date)
                                };
                                let res = Link::new(display)
                                    .ui(ui)
                                    .on_hover_text("Click to toggle time");
                                if res.clicked() {
                                    if expanded {
                                        self.expanded_history_dates.remove(&raw_date);
                                    } else {
                                        self.expanded_history_dates.insert(raw_date.clone());
                                    }
                                }
                            });
                            row.col(|ui| {
                                let state = m.state.clone().unwrap_or_default();
                                let color = match state.as_str() {
                                    "done" => Color32::LIGHT_GREEN,
                                    "cancel" => Color32::from_rgb(200, 100, 100),
                                    "draft" => Color32::GRAY,
                                    _ => Color32::LIGHT_GRAY,
                                };
                                ui.label(RichText::new(state).color(color));
                            });
                            row.col(|ui| {
                                ui.label(m.location_name());
                            });
                            row.col(|ui| {
                                ui.label(m.dest_name());
                            });
                            row.col(|ui| {
                                let refn = m.reference_str();
                                let refn = if refn.is_empty() { m.picking_name() } else { refn };
                                ui.label(refn);
                            });
                            row.col(|ui| {
                                ui.label(format!("{:.0}", m.qty_done.unwrap_or(0.)));
                            });
                        });
                    }
                });
        });
    }

    pub fn first_run(&mut self) {
        if self.first_run {
            self.first_run = false;
            let stock_tx = self.extra_stock_channel.0.clone();
            PlatformSpawner::spawn(async move {
                let stock = get_extra_stock_info(stock_tx.clone()).await;
                log::info!("Stock call: {stock:?}");
            });

            let stock_tx = self.stock_channel.0.clone();
            let store_selection = self.store_selection;
            PlatformSpawner::spawn(async move {
                let stock = get_stock(stock_tx.clone(), store_selection).await;
                log::info!("Stock call: {stock:?}");
            });

            // Seed the audit-source combobox so it's populated by the
            // time the user opens the Store Inventory tab.
            let audit_tx = self.audit_list_channel.0.clone();
            let store_id = Store::from_odoo_store_id(&store_selection.to_string()).into_odoo_store_id();
            PlatformSpawner::spawn(async move {
                if let Err(e) = list_audits(store_id, audit_tx).await {
                    log::error!("list_audits error (first_run): {e:?}");
                }
            });

            self.is_admin = get_current_user_from_auth()
                .map(|user| if user.get_username().is_empty() {
                    log::info!("User is empty");
                    false
                } else {
                    user.is_admin() | user.is_manager()
                })
                .unwrap_or(false);
        }
    }

    pub fn receive(&mut self, ui_actions_tx: Sender<TaskUiActions>) {
        if let Ok(stock_data) = self.stock_channel.1.try_recv() {
            // A live stock pull always switches the Store Inventory view
            // back to Live and drops any audit-row mode flags.
            self.inventory_view = InventoryView::Live;
            self.inventory_serials_viewer.audit_id = None;

            let data: Vec<SerialsData> = stock_data
                .iter()
                .map(|stock_data| {
                    let item_code = stock_data.product_id.clone().1.clone();
                    // Cache is keyed by bracket-only item code; the live
                    // pull's product_id.1 is the full Odoo display name,
                    // so strip everything after the closing bracket
                    // before looking up.
                    let cache_key = item_code_only(&item_code);
                    let (std_price, list_price) = self
                        .extra_stock_prices
                        .get(&cache_key)
                        .copied()
                        .unwrap_or((0.0, 0.0));
                    SerialsData(
                        item_code,
                        std_price,
                        list_price,
                        0,
                        stock_data.lot_id.clone().1.parse::<String>().unwrap(),
                        "S/N Info ⮫".to_string(),
                        Store::from_odoo_store_id(&stock_data.location_id.0.to_string()).as_str().to_string(),
                        false,
                    )
                })
                .collect();

            let tx = self.serial_channel.0.clone();

            let sns = data.iter().map(|r| r.4.clone()).collect::<Vec<String>>();

            PlatformSpawner::spawn(async move {
                let _res = find_attached_serials(sns, tx.clone()).await;
            });

            self.inventory_serials_table.replace(data);
            self.recompute_qty_rollup();
        }

        if let Ok(serial_data) = self.serial_channel.1.try_recv() {
            log::debug!("Serial Data: {:?}", serial_data);
            let mut data_table = self.inventory_serials_table.take();
            for data in data_table.iter_mut() {
                for serial_info in serial_data.result.iter() {
                    if data.4 == serial_info.name {
                        match serial_info.clone().bs_prest_ref {
                            BoolOrString::Bool(_) => {
                                data.5 = "Not Attached".to_string();
                                data.7 = false;
                            }
                            BoolOrString::String(order_num) => {
                                if !order_num.is_empty() {
                                    data.5 = order_num;
                                    data.7 = true;
                                } else {
                                    data.5 = "Not Attached".to_string();
                                    data.7 = false;
                                }
                            }
                        };
                    }
                }
            }
            self.inventory_serials_table.replace(data_table);
        }

        if let Ok(stock_inf) = self.extra_stock_channel.1.try_recv() {
            log::debug!("Serial Data: {:?}", stock_inf);

            // Rebuild the price cache so future audit imports + Live
            // refreshes can fan std/list prices into Store Inventory rows
            // without an extra round-trip.
            self.extra_stock_prices.clear();
            for d in stock_inf.iter() {
                let key = item_code_only(&d.display_name);
                self.extra_stock_prices
                    .insert(key, (d.standard_price, d.list_price));
            }

            // Backfill Std/List Price columns on any rows the Store
            // Inventory view already shows (Live or audit).
            let mut live = self.inventory_serials_table.take();
            for row in live.iter_mut() {
                let key = item_code_only(&row.0);
                if let Some((std_price, list_price)) = self.extra_stock_prices.get(&key) {
                    row.1 = *std_price;
                    row.2 = *list_price;
                }
            }
            self.inventory_serials_table.replace(live);
            self.recompute_qty_rollup();

            let data: Vec<StockQuantityData> = stock_inf
                .iter()
                .map(|stock_data| {
                    StockQuantityData(
                        stock_data.display_name.clone(),
                        stock_data.qty_available.clone(),
                        stock_data.virtual_available.clone(),
                        stock_data.standard_price.clone(),
                        stock_data.list_price.clone(),
                    )
                })
                .collect();
            self.stock_quantity_table.replace(data);
        }

        // Handle cost breakdown data
        if let Ok(cost_data) = self.cost_channel.1.try_recv() {
            log::info!("Received cost breakdown data: {} items", cost_data.len());
            self.cost_loading = false;
            self.cost_breakdown_table.replace(cost_data);
        }

        // Handle cost breakdown summary
        if let Ok(summary) = self.cost_summary_channel.1.try_recv() {
            log::info!("Received cost summary: customer={}, total=${:.2}, cost=${:.2}, profit=${:.2}", 
                      summary.customer_name, summary.order_total, summary.total_cost, summary.profit);
            self.cost_summary = Some(summary);
        }

        // Handle systems in-store data
        if let Ok(systems_data) = self.systems_channel.1.try_recv() {
            log::info!("Received systems in-store data: {} systems", systems_data.len());
            self.systems_loading = false;
            self.systems_in_store_table.replace(systems_data);
        }

        // Handle single system add
        if let Ok(system_data) = self.systems_add_channel.1.try_recv() {
            log::info!("Adding system to table: {}", system_data.order_id);
            let mut data = self.systems_in_store_table.take();
            // Only add if not already in the table
            if !data.iter().any(|s| s.order_id == system_data.order_id) {
                data.push(system_data);
            }
            self.systems_in_store_table.replace(data);
        }

        // Handle single system task creation
        if let Ok(system_data) = self.systems_task_channel.1.try_recv() {
            log::info!("Creating task for system: {}", system_data.order_id);
            // Send the system data to the create task modal via TaskUiActions
            let _ = ui_actions_tx.try_send(TaskUiActions::OpenCreateTaskModalFromSystem(system_data));
        }

        // Handle customer change request (open modal)
        if let Ok(request) = self.customer_change_channel.1.try_recv() {
            log::info!("Opening customer change modal for order: {}", request.order_id);
            self.customer_modal_open = true;
            self.customer_modal_order_id = request.order_id;
            self.customer_modal_current_name = request.customer_name;
            self.customer_search_query.clear();
            self.customer_search_results.clear();
        }

        // Handle customer search results
        if let Ok(results) = self.customer_search_results_channel.1.try_recv() {
            log::info!("Received customer search results: {} customers", results.len());
            self.customer_search_results = results;
            self.customer_searching = false;
        }

        // Everest: order lookup result
        if let Ok(result) = self.everest_order_channel.1.try_recv() {
            self.everest_loading = false;
            self.everest_error = result.error;
            if let Some(order) = result.order {
                let doc = order.header.doc_no.clone();
                let rows = order_to_rows(&order);
                log::info!(
                    "Everest order loaded: DOC {} with {} item rows",
                    doc, rows.len()
                );
                self.everest_items_table.replace(rows);
                self.everest_order_by_doc.insert(doc.clone(), order.clone());
                self.everest_order = Some(order);
                self.everest_view = EverestView::OrderDetail;
                let label = doc.clone();
                match self.everest_order_intent {
                    EverestOrderIntent::Reset => {
                        self.everest_crumbs = vec![EverestCrumb::Order { doc_no: doc, label }];
                    }
                    EverestOrderIntent::PushOnArrival => {
                        self.everest_crumbs.push(EverestCrumb::Order { doc_no: doc, label });
                    }
                    EverestOrderIntent::AlreadyPushed => {}
                }
                self.everest_order_intent = EverestOrderIntent::Reset;
            } else if self.everest_order_intent == EverestOrderIntent::Reset {
                self.everest_order = None;
                self.everest_items_table.replace(Vec::new());
            }
        }

        // Everest: user clicked an MFG serial cell -> look the serial up in
        // Everest (serial → DOCNUM → order), NOT Odoo movement history.
        if let Ok(serial) = self.everest_serial_click_channel.1.try_recv() {
            self.apply_everest_nav(EverestNav::OpenSerial(serial));
        }

        // Everest: customer search results arrived.
        if let Ok(res) = self.everest_search_channel.1.try_recv() {
            self.everest_search_loading = false;
            self.everest_results_shown = 20;
            for (code, ords) in res.prefetched_orders.iter() {
                self.everest_orders_by_cust.insert(code.clone(), ords.clone());
            }
            if res.error.is_some() {
                self.everest_error = res.error.clone();
            }
            self.everest_search_result = Some(res);
            self.everest_view = EverestView::Results;
            self.everest_crumbs = vec![EverestCrumb::Results];
        }

        // Everest: a customer's orders arrived.
        if let Ok(res) = self.everest_orders_channel.1.try_recv() {
            self.everest_orders_loading = false;
            if let Some(err) = &res.error {
                self.everest_error = Some(err.clone());
            }
            self.everest_orders_by_cust.insert(res.cust_code, res.orders);
        }

        // Everest: Odoo movement history arrived
        if let Ok(history) = self.everest_history_channel.1.try_recv() {
            self.everest_history_loading = false;
            self.everest_history = Some(history);
        }

        // ---- Inventory audit channel handling ----

        // List of audits for the currently selected store arrived.
        if let Ok(metas) = self.audit_list_channel.1.try_recv() {
            self.audit_list = metas;
        }

        if let Ok(serials) = self.csv_import_channel.1.try_recv() {
            self.kick_off_import(serials);
        }

        // Raw Odoo lookup finished: turn around and persist as an audit.
        if let Ok(rows) = self.audit_lookup_channel.1.try_recv() {
            self.import_in_progress = false;
            let store = Store::from_odoo_store_id(&self.store_selection.to_string());
            let user_id = get_current_user_from_auth().map(|u| u.get_id());
            let tx = self.audit_save_channel.0.clone();
            PlatformSpawner::spawn(async move {
                if let Err(e) = save_audit(store, user_id, rows, tx).await {
                    log::error!("save_audit error: {e:?}");
                }
            });
        }

        // Audit persisted: swap the view to it and refresh the listing.
        if let Ok((meta, rows)) = self.audit_save_channel.1.try_recv() {
            self.inventory_view = InventoryView::Audit(meta.id.clone());
            self.audit_list.insert(0, meta.clone());
            self.import_panel_open = false;
            self.import_textarea.clear();
            self.apply_audit_rows(meta.id, rows);
        }

        // User selected a different audit from the combobox.
        if let Ok((meta, rows)) = self.audit_load_channel.1.try_recv() {
            self.inventory_view = InventoryView::Audit(meta.id.clone());
            self.apply_audit_rows(meta.id, rows);
        }

        // Clicked serial in the Store Inventory Serial Number column.
        while let Ok(serial) = self.serial_window_channel.1.try_recv() {
            // Don't open duplicates — focus-style behavior is up to egui.
            if self.history_windows.iter().any(|w| w.serial == serial) {
                continue;
            }
            self.history_windows.push(HistoryWindow::loading(serial.clone()));
            let tx = self.history_result_channel.0.clone();
            PlatformSpawner::spawn(async move {
                if let Err(e) = fetch_serial_movement(serial, tx).await {
                    log::error!("Odoo serial history (window) error: {e:?}");
                }
            });
        }

        // Movement history result for one of the floating windows.
        while let Ok(history) = self.history_result_channel.1.try_recv() {
            let serial = history.serial.clone();
            if let Some(win) = self
                .history_windows
                .iter_mut()
                .find(|w| w.serial == serial)
            {
                win.populate_from_history(history);
            }
        }

        // Found-flag toggled on an audit row: persist to SurrealDB.
        while let Ok((audit_id, serial, found)) = self.found_toggle_channel.1.try_recv() {
            PlatformSpawner::spawn(async move {
                if let Err(e) = mark_found(audit_id, serial, found).await {
                    log::error!("mark_found error: {e:?}");
                }
            });
        }
    }

    /// Recompute the `qty_by_item` rollup on the SerialsViewer from the
    /// current contents of `inventory_serials_table`. Cheap (single
    /// linear scan); call after any `.replace()` on the table.
    fn recompute_qty_rollup(&mut self) {
        let mut by_item: HashMap<String, u32> = HashMap::new();
        for row in self.inventory_serials_table.iter() {
            *by_item.entry(row.0.clone()).or_insert(0) += 1;
        }
        self.inventory_serials_viewer.qty_by_item = by_item;
    }

    /// Swap the Store Inventory table to render the given audit's rows.
    fn apply_audit_rows(&mut self, audit_id: RecordId, rows: Vec<AuditSerialRow>) {
        let data: Vec<SerialsData> = rows
            .into_iter()
            .map(|r| {
                let item_code = r.item_code.unwrap_or_default();
                let std_price = r.std_price.unwrap_or_else(|| {
                    self.extra_stock_prices
                        .get(&item_code)
                        .map(|p| p.0)
                        .unwrap_or(0.0)
                });
                let list_price = r.list_price.unwrap_or_else(|| {
                    self.extra_stock_prices
                        .get(&item_code)
                        .map(|p| p.1)
                        .unwrap_or(0.0)
                });
                let order_col = r
                    .last_reference
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "S/N Info ⮫".to_string());
                SerialsData(
                    item_code,
                    std_price,
                    list_price,
                    0,
                    r.serial,
                    order_col,
                    r.last_location.unwrap_or_default(),
                    r.found,
                )
            })
            .collect();
        self.inventory_serials_table.replace(data);
        self.inventory_serials_viewer.audit_id = Some(audit_id);
        self.recompute_qty_rollup();
    }

    /// Show customer change modal
    pub fn show_customer_modal(&mut self, ui: &mut Ui) {
        if !self.customer_modal_open {
            return;
        }

        // Dim background
        let screen_rect = ui.ctx().content_rect();
        ui.painter().rect_filled(
            screen_rect,
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 180),
        );

        Area::new(Id::new("customer_change_modal"))
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
                                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                    if ui.button("✕").clicked() {
                                        self.customer_modal_open = false;
                                    }
                                });
                            });
                            
                            ui.separator();
                            
                            ui.label(format!("Order: {}", self.customer_modal_order_id));
                            ui.label(format!("Current Customer: {}", self.customer_modal_current_name));
                            
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
                                    let tx = self.customer_search_results_channel.0.clone();
                                    
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
                                
                                eframe::egui::ScrollArea::vertical()
                                    .max_width(200.0)
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
                                                        // Show customer ID and name
                                                        ui.label(RichText::new(format!("[{}] {}", customer.id, name)).strong());
                                                        ui.label(RichText::new(&customer.email).small().color(Color32::GRAY));
                                                        ui.label(RichText::new(&addr_info).small().color(Color32::GRAY));
                                                    });
                                                    
                                                    ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                                        if ui.button("Select").clicked() {
                                                            // Update the order with new customer
                                                            let order_id = self.customer_modal_order_id.clone();
                                                            let customer_id = customer.id.clone();
                                                            let address_id = address.id.clone();
                                                            let customer_name = name.clone();
                                                            
                                                            log::info!("Updating order {} with customer {} ({}), address {}", 
                                                                      order_id, customer_name, customer_id, address_id);
                                                            
                                                            // Update the table immediately
                                                            let mut data = self.systems_in_store_table.take();
                                                            for system in data.iter_mut() {
                                                                if system.order_id == order_id {
                                                                    system.customer_id = customer_id.clone();
                                                                    system.customer_name = customer_name.clone();
                                                                }
                                                            }
                                                            self.systems_in_store_table.replace(data);
                                                            
                                                            // Async update to Prestashop
                                                            PlatformSpawner::spawn(async move {
                                                                update_order_customer(&order_id, &customer_id, &address_id).await;
                                                            });
                                                            
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
}

/// Pull just the `[...]` bracket prefix off an Odoo display name. Items
/// without brackets fall through unchanged. Used so the price cache
/// (keyed on item code) lines up between the Company-Stock pull
/// (which sees full display names) and the Store-Inventory rows
/// (which already render bracket-only item codes).
fn item_code_only(display: &str) -> String {
    if let Some(end) = display.find(']') {
        display[..=end].to_string()
    } else {
        display.to_string()
    }
}

/// Update order customer and address in Prestashop
async fn update_order_customer(order_id: &str, customer_id: &str, address_id: &str) {
    use database::schema::prestashop::Prestashop;
    
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
                            // Remove tax_exempt tag (required for update)
                            match remove_xml_tag(&xml_with_address, "tax_exempt") {
                                Ok(final_xml) => {
                                    match api.modify_prestashop_order(&final_xml).await {
                                        Ok(_) => log::info!("Successfully updated order {} customer to {}", order_id, customer_id),
                                        Err(e) => log::error!("Failed to update order {}: {:?}", order_id, e),
                                    }
                                }
                                Err(e) => log::error!("Failed to remove tax_exempt from XML: {:?}", e),
                            }
                        }
                        Err(e) => log::error!("Failed to modify id_address_invoice in XML: {:?}", e),
                    }
                }
                Err(e) => log::error!("Failed to modify id_customer in XML: {:?}", e),
            }
        }
        Err(e) => log::error!("Failed to get order XML for {}: {:?}", order_id, e),
    }
}
