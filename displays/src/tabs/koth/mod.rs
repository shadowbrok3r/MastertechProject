use database::schema::{helper_traits::EmployeeHelper, prestashop::{generate_orders_report, get_order_payments, Employee, Order, OrderPayment, OrderState, PayPeriod, OrderType}, Store, User};
use eframe::egui::{CentralPanel, Color32, Frame, Ui, Widget, scroll_area};
use crate::{get_current_user_from_auth, PlatformSpawner, Spawner};
use crate::ui_tools::controls::{self, BarKind, IconLabel, Metrics};
use crate::ui_tools::icons::svg::{self, Icon};
use crate::ui_tools::{glass_card, theme};
use egui_data_table::{DataTable, Renderer};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use chrono::NaiveDateTime;
use itertools::Itertools;

pub mod data;
pub mod row_viewer;
pub mod row_viewer_all;
pub mod codec;

use data::KothTableData;
use row_viewer::KothRowViewer;
use row_viewer_all::AllEmployeesRowViewer;
use crate::tabs::koth::data::AllEmployeesTableData;

pub struct Koth {
    response_tx: Sender<Vec<Order>>,
    response_rx: Receiver<Vec<Order>>,
    employee_tx: Sender<Vec<Employee>>,
    employee_rx: Receiver<Vec<Employee>>,
    order_payment_tx: Sender<OrderPayment>,
    order_payment_rx: Receiver<OrderPayment>,
    orders: HashMap<String, Vec<Order>>,
    payments: HashMap<String, Vec<OrderPayment>>,
    // Order ids whose payments have been requested.
    payment_requests: HashSet<String>,
    employees: Vec<Employee>,
    order_state: OrderState,
    koth_selection: KothSelection,
    pay_period: PayPeriod,
    user: User,
    total: f64,
    total_w_tax: f64,
    total_spiffs: f64,
    pulling_all_orders: bool,
    // egui_data_table for per-employee orders view
    koth_table: DataTable<KothTableData>,
    koth_viewer: KothRowViewer,
    // egui_data_table for all employees summary view
    all_table: DataTable<AllEmployeesTableData>,
    all_viewer: AllEmployeesRowViewer,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum KothSelection {
    #[default]
    Me,
    AllEmployees
}

impl KothSelection {
    fn as_str(&self) -> &'static str {
        match self {
            KothSelection::Me => "Me",
            KothSelection::AllEmployees => "All Employees",
        }
    }
}

impl Default for Koth {
    fn default() -> Self {
        let (response_tx, response_rx) = crossbeam::channel::unbounded();
        let (employee_tx, employee_rx) = crossbeam::channel::unbounded();
        let (order_payment_tx, order_payment_rx) = crossbeam::channel::unbounded();
        
        Self {
            response_tx, response_rx,
            employee_tx, employee_rx,
            order_payment_tx, order_payment_rx,
            employees: Vec::new(),
            orders: Default::default(),
            payments: Default::default(),
            payment_requests: HashSet::new(),
            order_state: Default::default(),
            pay_period: Default::default(),
            koth_selection: KothSelection::default(),
            user: if let Some(usr) = get_current_user_from_auth() {
                usr.clone()
            } else {
                User::default()
            },
            total: 0.0,
            total_w_tax: 0.0,
            total_spiffs: 0.0,
            pulling_all_orders: false,
            koth_table: DataTable::new(),
            koth_viewer: KothRowViewer::default(),
            all_table: DataTable::new(),
            all_viewer: AllEmployeesRowViewer::default(),
        }
    }
}

/// A summary figure: an icon and the text, both in `color`.
fn stat(ui: &mut Ui, icon: Icon, text: &str, color: Color32) {
    IconLabel::new(icon, text).tone(color).show(ui);
}

impl Koth {
    pub fn ui(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        eframe::egui::Panel::top("KothTopPanel")
        .frame(Frame::NONE)
        .exact_size(m.bar_h)
        .show(ui, |ui| {
            controls::bar(ui, BarKind::Tool, |ui| {
                // Search box should drive the active table's viewer
                let filter_ref = match self.koth_selection {
                    KothSelection::Me => &mut self.koth_viewer.filter,
                    KothSelection::AllEmployees => &mut self.all_viewer.filter,
                };

                controls::search_field(ui, filter_ref, "Search", 180.);

                let all_employees = match self.koth_selection {
                    KothSelection::Me => false,
                    KothSelection::AllEmployees => true,
                };

                if !all_employees {
                    // generate_orders_report
                    let states = [
                        OrderState::AcceptedByOdoo,
                        OrderState::Shipped,
                        OrderState::DeliveredToStore,
                        OrderState::DoneShelf,
                        OrderState::OrderPlaced,
                        OrderState::PrePulled,
                        OrderState::ReadyToBuild,
                        OrderState::QcAndBurnin,
                        OrderState::ShipToStore,
                        OrderState::Returned,
                    ];
                    let options: Vec<(OrderState, &str)> =
                        states.iter().map(|s| (s.clone(), s.as_str())).collect();
                    controls::dropdown(ui, "Koth OrderState", &mut self.order_state, &options, 160.);
                }

                controls::icon_label(ui, svg::CALENDAR_DOTS, "Pay Period");
                let periods = [PayPeriod::Current, PayPeriod::Last];
                let options: Vec<(PayPeriod, &str)> =
                    periods.iter().map(|p| (p.clone(), p.as_str())).collect();
                controls::dropdown(ui, "Koth PayPeriod", &mut self.pay_period, &options, 120.);

                let selections = [KothSelection::Me, KothSelection::AllEmployees]
                    .map(|s| (s, s.as_str()));
                controls::dropdown(ui, "Koth Selection", &mut self.koth_selection, &selections, 140.);

                if !all_employees {
                    let pull_label = format!("Pull Orders in '{}'", self.order_state.as_str());
                    if controls::icon_text_button(ui, svg::DOWNLOAD_SIMPLE, &pull_label).clicked() {
                        self.pulling_all_orders = false;
                        self.total = 0.0;
                        self.total_w_tax = 0.0;
                        self.total_spiffs = 0.0;
                        self.orders.clear();
                        self.payments.clear();
                        self.payment_requests.clear();
                        self.koth_table.clear();
                        self.all_table.clear();
                        let pay_period = self.pay_period.clone();
                        let state = self.order_state.clone();
                        let id = if let Some(id) = self.user.get_employee_id() {
                            id
                        } else { 
                            self.user = if let Some(usr) = get_current_user_from_auth() {
                                usr.clone()
                            } else {
                                User::default()
                            };
                            self.user.get_employee_id().unwrap_or(0)
                        };

                        let id_employee = id.to_string().clone();
                        let tx = self.response_tx.clone();
                        PlatformSpawner::spawn(async move {
                            if id != 0 {
                                let res = generate_orders_report(pay_period, &state.to_id().to_string(), &id_employee).await;
                                log::info!("Result: {res:?}");
                                match res {
                                    Ok(orders) => {let _ = tx.try_send(orders);},
                                    Err(e) => log::error!("Error getting orders for koth: {e:?}"),
                                }
                            }
                        });
                    }
                }

                if controls::icon_text_button(ui, svg::CLOUD_ARROW_DOWN, "Pull ALL orders").clicked() {
                    self.pulling_all_orders = true;
                    self.total = 0.0;
                    self.total_w_tax = 0.0;
                    self.total_spiffs = 0.0;
                    self.orders.clear();
                    self.payments.clear();
                    self.payment_requests.clear();
                    self.employees.clear();
                    self.koth_table.clear();
                    self.all_table.clear();

                    match self.koth_selection {
                        KothSelection::Me => {
                            let pay_period = self.pay_period.clone();
                            let id = if let Some(id) = self.user.get_employee_id() {
                                id
                            } else { 
                                self.user = if let Some(usr) = get_current_user_from_auth() {
                                    usr.clone()
                                } else {
                                    User::default()
                                };
                                self.user.get_employee_id().unwrap_or(0)
                            };

                            let id_employee = id.to_string().clone();
                            let tx = self.response_tx.clone();
                            PlatformSpawner::spawn(async move {
                                if id != 0 {
                                    for state in OrderState::VALUES.iter() {
                                        let period = pay_period.clone();
                                        if *state != OrderState::Returned {
                                            let state_id = state.to_id().to_string();
                                            let res = generate_orders_report(period, &state_id, &id_employee).await;
                                            log::info!("Result: {res:?}");
                                            match res {
                                                Ok(orders) => {let _ = tx.try_send(orders);},
                                                Err(e) => log::error!("Error getting orders for koth: {e:?}"),
                                            }
                                        }
                                    }
                                }
                            });
                        },
                        KothSelection::AllEmployees => {
                            self.employees.clear();
                            let emp_tx = self.employee_tx.clone();
                            PlatformSpawner::spawn(async move {
                                for store in Store::RETAIL {
                                    match Employee::get_employees_in_store(&store.into_store_id().to_string()).await {
                                        Ok(employees) => { let _ = emp_tx.try_send(employees); },
                                        Err(e) => log::error!("Error getting employee id's: {e:?}"),
                                    }
                                }
                            });
                        },
                    }
                }
            });
        });

        if let KothSelection::Me = self.koth_selection {
            eframe::egui::Panel::bottom("KothBottom")
            .show(ui, |ui| {
                ui.columns(9, |ui| {
                    // Summary row below the table (unchanged logic)
                    let my_emp_id = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
                    let my_orders = self
                        .orders
                        .iter()
                        .flat_map(|(_, orders)| orders)
                        .filter(|order|
                            order.id_employee_sales_rep == my_emp_id
                            || order.id_employee_split_rep == my_emp_id
                        )
                        .collect::<Vec<&Order>>();
                    
                    let my_payments = self
                        .payments
                        .iter()
                        .filter(|(id, _)| **id == my_emp_id)
                        .flat_map(|(_, payments)| payments)
                        .collect::<Vec<&OrderPayment>>();

                    let total_laptops = my_orders
                        .iter()
                        .filter(|o| OrderType::from_id_str(&o.id_order_type) != OrderType::ServiceOrder)
                        .flat_map(|o| o.associations.order_rows.iter())
                        .filter(|a| a.product_reference.to_lowercase().starts_with("lap/"))
                        .count();

                    let total_desktops = my_orders
                        .iter()
                        .filter(|o| OrderType::from_id_str(&o.id_order_type) != OrderType::ServiceOrder)
                        .flat_map(|o| o.associations.order_rows.iter())
                        .filter_map(|o| {
                            if !o.product_reference.to_lowercase().starts_with("lap/") 
                                && (
                                    o.product_reference.to_lowercase().starts_with("case/")
                                    || o.product_reference.to_lowercase().starts_with("bsd/")
                                    || o.product_reference.to_lowercase().starts_with("rci/")
                                    || o.product_reference.to_lowercase().starts_with("r2r/")
                                    || o.product_reference.to_lowercase().starts_with("rtr/")
                                )
                                && !o.product_reference.to_lowercase().starts_with("case/15")
                                && !o.product_reference.to_lowercase().starts_with("case/17")
                            {
                                Some(o.product_reference.clone())
                            } else { 
                                None 
                            }
                        })
                        .count();
                    
                    let total_financed = my_orders
                        .iter()
                        .filter(|o| {
                            my_payments
                                .iter()
                                .any(|p| p.payment_method == "Financing Payment" && p.id_order == o.id)
                        })
                        .map(|o| o.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0))
                        .sum::<f64>();

                    let ar_financing_ratio = if self.total > 0.0 { (total_financed / self.total) * 100.0 } else { 0.0 };
                    
                    let total_sales = total_desktops + total_laptops;
                    
                    // Count my actual orders, not entries in the map
                    let total_orders = self.orders.get(&my_emp_id).map(|v| v.len()).unwrap_or(0);
                    let (text, error, warn) = (theme::text(&ui[0]), theme::error(&ui[0]), theme::warn(&ui[0]));
                    stat(&mut ui[0], svg::CHART_BAR, &format!("Sales: {total_sales} / Orders: {total_orders}"), text);
                    stat(&mut ui[1], svg::LAPTOP, &format!("Laptops: {total_laptops} / Desktops: {total_desktops}"), text);
                    ui[2].label("");
                    ui[3].label("");
                    stat(&mut ui[4], svg::CREDIT_CARD, &format!("Finance ratio: {ar_financing_ratio:.2}%"), error);
                    let warranties = my_orders.iter().filter(|order| {
                        order.associations.order_rows.iter().any(|o| o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0"))
                    }).count();
                    stat(&mut ui[5], svg::SHIELD_CHECK, &format!("WTY's: {warranties} out of {total_sales} sales"), error);
                    stat(&mut ui[6], svg::RECEIPT, &format!("Total W/Tax: $ {:.2}", self.total_w_tax), warn);
                    stat(&mut ui[7], svg::CURRENCY_DOLLAR, &format!("REVENUE: $ {:.2}", self.total), warn);
                    stat(&mut ui[8], svg::COINS, &format!("Spiffs: $ {:.2}", self.total_spiffs), text);
                });
            });
        }

        CentralPanel::default()
        .show(ui, |ui| {
            glass_card::group(ui, |ui| {
                match self.koth_selection {
                    KothSelection::Me => {
                        // Keep the date column label in sync with selection
                        let date_label = match (self.order_state.clone(), self.pulling_all_orders) {
                            (OrderState::AcceptedByOdoo, false) => "Delivery Date",
                            _ => "Date Updated",
                        };
                        self.koth_viewer.date_label = date_label.to_string();

                        Renderer::new(&mut self.koth_table, &mut self.koth_viewer)
                        .with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                    KothSelection::AllEmployees => {
                        // Show the summary table with the specific 7 columns
                        Renderer::new(&mut self.all_table, &mut self.all_viewer)
                        .with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                }
            });
        });
    }

    fn rebuild_koth_rows(&mut self) {
        let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
        let my_orders = self.orders.get(&uid).cloned().unwrap_or_default();
        let my_payments = self.payments.get(&uid).cloned().unwrap_or_default();

        let mut rows: Vec<KothTableData> = Vec::with_capacity(my_orders.len());
        let mut spiff_sum: f64 = 0.0;
        let mut total_sum_tax_excl: f64 = 0.0;
        let mut total_sum_tax_incl: f64 = 0.0;
        for order in my_orders.iter() {
            let state = OrderState::state_from_id_str(&order.current_state);
            let date_str = match state { OrderState::AcceptedByOdoo => order.delivery_date.clone(), _ => order.date_upd.clone() };
            // Attribute revenue via payments recorded for this employee for this order
            let order_total_paid: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);
            let order_total_paid_tax_excl: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
            let attributed_paid: f64 = my_payments
                .iter()
                .filter(|p| p.id_order == order.id)
                .map(|p| p.amount.parse::<f64>().unwrap_or(0.0))
                .sum();
            // If we don't have any payments attributed, skip this order from Me view
            if attributed_paid <= 0.0 || order_total_paid <= 0.0 { continue; }
            let share_ratio: f64 = (attributed_paid / order_total_paid).clamp(0.0, 1.0);
            let total_paid_num: f64 = order_total_paid * share_ratio;
            let total_paid_tax_excl_num: f64 = order_total_paid_tax_excl * share_ratio;
            // Calculate spiffs for this order
            let mut spiffs_total: f64 = 0.0;
            let mut has_system_product = false;
            let mut cps_units: i32 = 0; // sw/cps (not plat) qty-based
            let mut has_sas: bool = false;
            let mut has_wrav: bool = false;

            for o in order.associations.order_rows.iter() {
                let r = o.product_reference.to_lowercase();
                let qty: i32 = o.product_quantity.parse::<i32>().unwrap_or(1);

                // Track if this order contains a system (laptop/desktop) product
                if r.starts_with("lap/") || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17")) {
                    has_system_product = true;
                }

                // Track SAS/WRAV presence for CPS pairing rule
                if r.starts_with("sw/sas") { has_sas = true; }
                if r.starts_with("sw/wrav") { has_wrav = true; }

                // CPS $10 for sw/cps (not cps-plat). Qty-based.
                if r.starts_with("sw/cps") && !r.starts_with("sw/cps-plat") {
                    cps_units += qty;
                }

                // CPS Plat $25
                if r.starts_with("sw/cps-plat") {
                    spiffs_total += 25.0 * qty as f64;
                }

                // SEB/Year $15
                if r == "seb/year" {
                    spiffs_total += 15.0 * qty as f64;
                }

                // Parts with $2 spiff
                if r.starts_with("mon/")
                    || r.starts_with("kb/")
                    || r.starts_with("mou/")
                    || r.contains("/dock/")
                    || r == "dvdrw/usb"
                    || r.starts_with("case/15")
                    || r.starts_with("case/17")
                    || r.starts_with("spkr/")
                    || r.starts_with("belk/")
                {
                    spiffs_total += 2.0 * qty as f64;
                }

                // Warranty spiffs
                if r.starts_with("wty/") {
                    let ru = r.to_uppercase();
                    // RTR warranty spiffs (R2R)
                    if ru.contains("/R2R/2YR") { spiffs_total += 3.0 * qty as f64; }
                    else if ru.contains("/R2R/3YR") { spiffs_total += 6.0 * qty as f64; }
                    else if ru.contains("/R2R/4YR") { spiffs_total += 9.0 * qty as f64; }
                    else if ru.contains("/R2R/5YR") { spiffs_total += 12.0 * qty as f64; }
                    else if ru.contains("/R2R/LIFE") { spiffs_total += 15.0 * qty as f64; }

                    // System warranty spiffs (LAP/DSK CUST)
                    if ru.contains("/LAP/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
                    else if ru.contains("/LAP/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
                    else if ru.contains("/LAP/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }

                    if ru.contains("/DSK/CUST/3YR") { spiffs_total += 3.0 * qty as f64; }
                    else if ru.contains("/DSK/CUST/4YR") { spiffs_total += 6.0 * qty as f64; }
                    else if ru.contains("/DSK/CUST/5YR") { spiffs_total += 9.0 * qty as f64; }
                    else if ru.contains("/DSK/CUST/LIFE") { spiffs_total += 12.0 * qty as f64; }

                    // BSD warranty spiffs (only when order type is BSD)
                    if matches!(OrderType::from_id_str(&order.id_order_type), OrderType::Bsd) {
                        if ru.contains("/BSD/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
                        else if ru.contains("/BSD/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
                        else if ru.contains("/BSD/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }
                        else if ru.contains("/BSD/CUST/5YR") { spiffs_total += 12.0 * qty as f64; }
                    }
                }
            }

            // Apply CPS from sw/cps items
            if cps_units > 0 {
                spiffs_total += 10.0 * cps_units as f64;
            } else if has_sas && has_wrav {
                // or SAS+WRAV pairing counts as a single CPS $10 (once per order)
                spiffs_total += 10.0;
            }

            // Base system spiff by order type (only if a system product is present)
            if has_system_product {
                match OrderType::from_id_str(&order.id_order_type) {
                    OrderType::ReadyToRoll => spiffs_total += 5.0,
                    OrderType::Rci => spiffs_total += 5.0,
                    OrderType::Bsd => spiffs_total += 25.0,
                    // Unknown pays no spiff.
                    OrderType::SalesOrder | OrderType::ServiceOrder | OrderType::RepairOrder | OrderType::Unknown => {}
                }
            }

            let product: String = order.associations.order_rows
                .iter()
                .filter_map(|o| {
                    if o.product_reference.to_lowercase().starts_with("lap")
                        || o.product_reference.to_lowercase().starts_with("case/")
                        || o.product_reference.to_lowercase().starts_with("bsd/")
                        || o.product_reference.to_lowercase().starts_with("rci/")
                        || o.product_reference.to_lowercase().starts_with("r2r/")
                        || o.product_reference.to_lowercase().starts_with("rtr/")
                        && !( 
                            o.product_reference.to_lowercase().starts_with("case/15") 
                            && o.product_reference.to_lowercase().starts_with("case/17") 
                        )
                    { Some(o.product_reference.clone()) } else { None }
                })
                .next()
                .unwrap_or_else(|| {
                    order.associations.order_rows.first().map(|o| o.product_reference.clone()).unwrap_or_default()
                });

            let warranty = order.associations.order_rows
                .iter()
                .filter_map(|o| {
                    if o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0")
                    { Some(o.product_reference.clone()) } else { None }
                })
                .next()
                .unwrap_or_else(|| "-".to_string());

            let payment = my_payments
                .iter()
                .find(|p| p.id_order == order.id)
                .map(|p| p.payment_method.clone())
                .unwrap_or("-".to_string());

            // Format display date as "MM / DD / YYYY"
            let display_date = NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.format("%m / %d / %Y").to_string())
                .unwrap_or_else(|_| String::new());

            // Attribute spiffs by the same share ratio so split orders show half spiffs per rep
            spiff_sum += spiffs_total * share_ratio;
            total_sum_tax_incl += total_paid_num;
            total_sum_tax_excl += total_paid_tax_excl_num;
            rows.push(KothTableData {
                order_id: order.id.clone(),
                date: display_date,
                order_state: OrderState::from_id_str(&order.current_state).to_string(),
                product,
                payment,
                warranty,
                total_paid: total_paid_num,
                total_without_tax: total_paid_tax_excl_num,
                spiffs: spiffs_total * share_ratio,
            });
        }
        self.koth_table.replace(rows);
        self.total_spiffs = spiff_sum;
        self.total = total_sum_tax_excl;
        self.total_w_tax = total_sum_tax_incl;
    }

    fn rebuild_koth_rows_all(&mut self) {
        // Flatten all orders across employees and deduplicate by order id
        let mut seen: HashSet<&str> = HashSet::new();
        let mut rows: Vec<KothTableData> = Vec::new();
        let mut spiff_sum: f64 = 0.0;

        for order in self
            .orders
            .values()
            .flat_map(|v| v.iter())
        {
            if order.id.is_empty() { continue; }
            if !seen.insert(order.id.as_str()) { continue; }

            let state = OrderState::state_from_id_str(&order.current_state);
            let date_str = match state { OrderState::AcceptedByOdoo => order.delivery_date.clone(), _ => order.date_upd.clone() };
            let total_paid_num: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);
            let total_paid_tax_excl_num: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);

            // Calculate spiffs for this order (same logic as rebuild_koth_rows)
            let mut spiffs_total: f64 = 0.0;
            let mut has_system_product = false;
            let mut cps_units: i32 = 0;
            let mut has_sas: bool = false;
            let mut has_wrav: bool = false;

            for o in order.associations.order_rows.iter() {
                let r = o.product_reference.to_lowercase();
                let qty: i32 = o.product_quantity.parse::<i32>().unwrap_or(1);

                if r.starts_with("lap/") || (r.starts_with("case/") || r.starts_with("bsd/") && (!r.starts_with("case/15") && !r.starts_with("case/17"))) {
                    has_system_product = true;
                }

                if r.starts_with("sw/sas") { has_sas = true; }
                if r.starts_with("sw/wrav") { has_wrav = true; }
                if r.starts_with("sw/cps") && !r.starts_with("sw/cps-plat") { cps_units += qty; }
                if r.starts_with("sw/cps-plat") { spiffs_total += 25.0 * qty as f64; }
                if r == "seb/year" { spiffs_total += 15.0 * qty as f64; }

                if r.starts_with("mon/")
                    || r.starts_with("kb/")
                    || r.starts_with("mou/")
                    || r.contains("/dock/")
                    || r == "dvdrw/usb"
                    || r.starts_with("case/15")
                    || r.starts_with("case/17")
                    || r.starts_with("spkr/")
                    || r.starts_with("belk/")
                { spiffs_total += 2.0 * qty as f64; }

                if r.starts_with("wty/") {
                    let ru = r.to_uppercase();
                    if ru.contains("/R2R/2YR") { spiffs_total += 3.0 * qty as f64; }
                    else if ru.contains("/R2R/3YR") { spiffs_total += 6.0 * qty as f64; }
                    else if ru.contains("/R2R/4YR") { spiffs_total += 9.0 * qty as f64; }
                    else if ru.contains("/R2R/5YR") { spiffs_total += 12.0 * qty as f64; }
                    else if ru.contains("/R2R/LIFE") { spiffs_total += 15.0 * qty as f64; }

                    if ru.contains("/LAP/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
                    else if ru.contains("/LAP/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
                    else if ru.contains("/LAP/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }

                    if ru.contains("/DSK/CUST/3YR") { spiffs_total += 3.0 * qty as f64; }
                    else if ru.contains("/DSK/CUST/4YR") { spiffs_total += 6.0 * qty as f64; }
                    else if ru.contains("/DSK/CUST/5YR") { spiffs_total += 9.0 * qty as f64; }
                    else if ru.contains("/DSK/CUST/LIFE") { spiffs_total += 12.0 * qty as f64; }

                    if matches!(OrderType::from_id_str(&order.id_order_type), OrderType::Bsd) {
                        if ru.contains("/BSD/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
                        else if ru.contains("/BSD/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
                        else if ru.contains("/BSD/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }
                        else if ru.contains("/BSD/CUST/5YR") { spiffs_total += 12.0 * qty as f64; }
                    }
                }
            }

            if cps_units > 0 { spiffs_total += 10.0 * cps_units as f64; }
            else if has_sas && has_wrav { spiffs_total += 10.0; }

            if has_system_product {
                match OrderType::from_id_str(&order.id_order_type) {
                    OrderType::ReadyToRoll => spiffs_total += 5.0,
                    OrderType::Rci => spiffs_total += 5.0,
                    OrderType::Bsd => spiffs_total += 25.0,
                    // Unknown pays no spiff.
                    OrderType::SalesOrder | OrderType::ServiceOrder | OrderType::RepairOrder | OrderType::Unknown => {}
                }
            }

            let product: String = order.associations.order_rows
                .iter()
                .filter_map(|o| {
                    if o.product_reference.to_lowercase().starts_with("lap")
                        || o.product_reference.to_lowercase().starts_with("case/")
                        || o.product_reference.to_lowercase().starts_with("bsd/")
                        || o.product_reference.to_lowercase().starts_with("rci/")
                        || o.product_reference.to_lowercase().starts_with("r2r/")
                        || o.product_reference.to_lowercase().starts_with("rtr/")
                        && !o.product_reference.to_lowercase().starts_with("case/15")
                        && !o.product_reference.to_lowercase().starts_with("case/17")
                    { Some(o.product_reference.clone()) } else { None }
                })
                .next()
                .unwrap_or_else(|| {
                    order.associations.order_rows.first().map(|o| o.product_reference.clone()).unwrap_or_default()
                });

            let payment = "-".to_string(); // In All view, payment per-employee is ambiguous; omit for now
            let warranty = order.associations.order_rows
                .iter()
                .filter_map(|o| {
                    if o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0")
                    { Some(o.product_reference.clone()) } else { None }
                })
                .next()
                .unwrap_or_else(|| "-".to_string());

            let display_date = NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.format("%m / %d / %Y").to_string())
                .unwrap_or_else(|_| String::new());

            spiff_sum += spiffs_total;
            rows.push(KothTableData {
                order_id: order.id.clone(),
                date: display_date,
                order_state: OrderState::from_id_str(&order.current_state).to_string(),
                product,
                payment,
                warranty,
                total_paid: total_paid_num,
                total_without_tax: total_paid_tax_excl_num,
                spiffs: spiffs_total,
            });
        }

        self.koth_table.replace(rows);
        self.total_spiffs = spiff_sum;
    }

    // Build summary rows for All Employees data table (7 columns)
    fn rebuild_all_employees_rows(&mut self) {
        // Collect and compute metrics per employee using payment-attributed shares (same as Me)
        let mut rows: Vec<AllEmployeesTableData> = Vec::new();

        for employee in self.employees.iter() {
            let emp_id = &employee.id;

            // Orders cached for this employee
            let orders: Vec<&Order> = self
                .orders
                .get(emp_id)
                .map(|v| v.iter().collect())
                .unwrap_or_else(|| Vec::new());

            if orders.is_empty() { continue; }

            // Payments for this employee (used for attribution and finance ratio)
            let emp_payments: Vec<&OrderPayment> = self
                .payments
                .iter()
                .filter(|(id, _)| **id == *emp_id)
                .flat_map(|(_, payments)| payments.iter())
                .collect();

            let total_laptops = orders
                .iter()
                .filter(|o| OrderType::from_id_str(&o.id_order_type) != OrderType::ServiceOrder)
                .flat_map(|o| o.associations.order_rows.iter())
                .filter(|a| a.product_reference.to_lowercase().starts_with("lap/"))
                .count();

            let total_desktops = orders
                .iter()
                .filter(|o| OrderType::from_id_str(&o.id_order_type) != OrderType::ServiceOrder)
                .flat_map(|o| o.associations.order_rows.iter())
                .filter_map(|o| {
                    if !o.product_reference.to_lowercase().starts_with("lap/")
                        && (
                            o.product_reference.to_lowercase().starts_with("case/")
                            || o.product_reference.to_lowercase().starts_with("bsd/")
                            || o.product_reference.to_lowercase().starts_with("rci/")
                            || o.product_reference.to_lowercase().starts_with("r2r/")
                            || o.product_reference.to_lowercase().starts_with("rtr/")
                        )
                        && !o.product_reference.to_lowercase().starts_with("case/15")
                        && !o.product_reference.to_lowercase().starts_with("case/17")
                    {
                        Some(o.product_reference.clone())
                    } else {
                        None
                    }
                })
                .count();

            let total_sales = total_desktops + total_laptops;

            let mut total_financed: f64 = 0.0;
            let mut total_revenue: f64 = 0.0;
            let mut total_warranties: usize = 0;
            let mut total_spiffs: f64 = 0.0;

            for order in orders.iter() {
                // Attribute share by this employee's payments on this order
                let order_total_paid = order.total_paid.parse::<f64>().unwrap_or(0.0);
                let order_tax_excl = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                if order_total_paid <= 0.0 { continue; }
                let attributed_paid: f64 = emp_payments
                    .iter()
                    .filter(|p| p.id_order == order.id)
                    .map(|p| p.amount.parse::<f64>().unwrap_or(0.0))
                    .sum();
                if attributed_paid <= 0.0 { continue; }
                let share_ratio = (attributed_paid / order_total_paid).clamp(0.0, 1.0);
                let tax_excl = order_tax_excl * share_ratio;
                total_revenue += tax_excl;

                // warranty present?
                let warranty = order.associations.order_rows
                    .iter()
                    .any(|o| o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0"));
                if warranty { total_warranties += 1; }

                // compute spiffs (reuse same logic as in rebuild_koth_rows)
                let mut spiffs_total: f64 = 0.0;
                let mut has_system_product = false;
                let mut cps_units: i32 = 0;
                let mut has_sas: bool = false;
                let mut has_wrav: bool = false;

                for o in order.associations.order_rows.iter() {
                    let r = o.product_reference.to_lowercase();
                    let qty: i32 = o.product_quantity.parse::<i32>().unwrap_or(1);

                    if r.starts_with("lap/") || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17")) {
                        has_system_product = true;
                    }

                    if r.starts_with("sw/sas") { has_sas = true; }
                    if r.starts_with("sw/wrav") { has_wrav = true; }
                    if r.starts_with("sw/cps") && !r.starts_with("sw/cps-plat") { cps_units += qty; }
                    if r.starts_with("sw/cps-plat") { spiffs_total += 25.0 * qty as f64; }
                    if r == "seb/year" { spiffs_total += 15.0 * qty as f64; }

                    if r.starts_with("mon/")
                        || r.starts_with("kb/")
                        || r.starts_with("mou/")
                        || r.contains("/dock/")
                        || r == "dvdrw/usb"
                        || r.starts_with("case/15")
                        || r.starts_with("case/17")
                        || r.starts_with("spkr/")
                        || r.starts_with("belk/")
                    { spiffs_total += 2.0 * qty as f64; }

                    if r.starts_with("wty/") {
                        let ru = r.to_uppercase();
                        if ru.contains("/R2R/2YR") { spiffs_total += 3.0 * qty as f64; }
                        else if ru.contains("/R2R/3YR") { spiffs_total += 6.0 * qty as f64; }
                        else if ru.contains("/R2R/4YR") { spiffs_total += 9.0 * qty as f64; }
                        else if ru.contains("/R2R/5YR") { spiffs_total += 12.0 * qty as f64; }
                        else if ru.contains("/R2R/LIFE") { spiffs_total += 15.0 * qty as f64; }

                        if ru.contains("/LAP/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
                        else if ru.contains("/LAP/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
                        else if ru.contains("/LAP/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }

                        if ru.contains("/DSK/CUST/3YR") { spiffs_total += 3.0 * qty as f64; }
                        else if ru.contains("/DSK/CUST/4YR") { spiffs_total += 6.0 * qty as f64; }
                        else if ru.contains("/DSK/CUST/5YR") { spiffs_total += 9.0 * qty as f64; }
                        else if ru.contains("/DSK/CUST/LIFE") { spiffs_total += 12.0 * qty as f64; }

                        if matches!(OrderType::from_id_str(&order.id_order_type), OrderType::Bsd) {
                            if ru.contains("/BSD/CUST/2YR") { spiffs_total += 3.0 * qty as f64; }
                            else if ru.contains("/BSD/CUST/3YR") { spiffs_total += 6.0 * qty as f64; }
                            else if ru.contains("/BSD/CUST/4YR") { spiffs_total += 9.0 * qty as f64; }
                            else if ru.contains("/BSD/CUST/5YR") { spiffs_total += 12.0 * qty as f64; }
                        }
                    }
                }

                if cps_units > 0 { spiffs_total += 10.0 * cps_units as f64; }
                else if has_sas && has_wrav { spiffs_total += 10.0; }

                if has_system_product {
                    match OrderType::from_id_str(&order.id_order_type) {
                        OrderType::ReadyToRoll => spiffs_total += 5.0,
                        OrderType::Rci => spiffs_total += 5.0,
                        OrderType::Bsd => spiffs_total += 25.0,
                        // Unknown pays no spiff.
                        OrderType::SalesOrder | OrderType::ServiceOrder | OrderType::RepairOrder | OrderType::Unknown => {}
                    }
                }

                total_spiffs += spiffs_total * share_ratio;

                // Finance numerator: only count attributed share for orders with financing payment by this employee
                let has_financing = emp_payments
                    .iter()
                    .any(|p| p.payment_method == "Financing Payment" && p.id_order == order.id);
                if has_financing { total_financed += tax_excl; }
            }

            // Finance ratio: out of total revenue (tax excl), how much was financed
            // Use the same scale the "Me" summary uses (x100 to match observed data)
            let finance_ratio = if total_revenue > 0.0 { (total_financed / total_revenue) * 100.0 } else { 0.0 };

            // Total orders = count of attributed orders (share > 0)
            let total_orders = self
                .payments
                .get(emp_id)
                .map(|ps| {
                    ps.iter().map(|p| p.id_order.as_str()).unique().count()
                })
                .unwrap_or(0);

            rows.push(AllEmployeesTableData {
                employee_id: emp_id.clone(),
                employee_name: format!("{} {}", employee.firstname, employee.lastname),
                total_sales,
                total_orders,
                laptops: total_laptops,
                desktops: total_desktops,
                finance_ratio,
                warranties: total_warranties,
                revenue: total_revenue,
                spiffs: total_spiffs,
            });
        }

        // Sort by revenue desc to match previous behavior
        rows.sort_by(|a, b| b.revenue.partial_cmp(&a.revenue).unwrap_or(std::cmp::Ordering::Equal));
        self.all_table.replace(rows);
    }

    pub fn receive(&mut self, ctx: &eframe::egui::Context) {
        let mut changed = false;
        while let Ok(orders) = self.response_rx.try_recv() {
            changed = true;
            match self.koth_selection {
                KothSelection::Me => {
                    let sort = |a: &Order, b: &Order| {
                        let a_total: f64 = a.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                        let b_total: f64 = b.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                        b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)
                    };

                    let new_orders: Vec<Order> = orders
                        .clone()
                        .iter()
                        .filter(|o| !o.id.is_empty())
                        .sorted_by(|a, b| sort(a, b))
                        .cloned()
                        .collect();

                    // Request payments for each order; totals are computed from payments in rebuild
                    for order in new_orders.iter() {
                        if !self.payment_requests.insert(order.id.clone()) { continue; }
                        let tx = self.order_payment_tx.clone();
                        let order = order.clone();
                        PlatformSpawner::spawn(async move {
                            match get_order_payments(&order.id).await {
                                Ok(payments) => { 
                                    for payment in payments {
                                        let _ = tx.try_send(payment);
                                    }
                                },
                                Err(e) => log::error!("Error getting payment detials: {e:?}"),
                            }
                        });
                    }

                    let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
                    
                    if self.pulling_all_orders {
                        // Append only truly new orders (by id) and keep list sorted
                        let entry = self
                            .orders
                            .entry(uid.clone())
                            .or_insert_with(Vec::new);

                        for o in new_orders.into_iter() {
                            if !entry.iter().any(|e| e.id == o.id) { // dedup by order id
                                entry.push(o);
                            }
                        }
                        entry.sort_by(sort);
                    } else {
                        // Replace with latest snapshot
                        self.orders.insert(uid.clone(), new_orders.clone());
                    }

                    // Rebuild table rows for current user
                    self.rebuild_koth_rows();
                },
                KothSelection::AllEmployees => {
                    self.total = 0.0;
                    self.total_w_tax = 0.0;
                    let sort = |a: &Order, b: &Order| {
                        let a_total: f64 = a.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                        let b_total: f64 = b.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                        b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)
                    };

                    let new_orders: Vec<Order> = orders
                        .clone()
                        .iter()
                        .filter(|o| !o.id.is_empty())
                        .sorted_by(|a, b| sort(a, b))
                        .cloned()
                        .collect();

                    // Process each order
                    for order in new_orders.iter() {
                        if !self.payment_requests.insert(order.id.clone()) { continue; }
                        let tx = self.order_payment_tx.clone();
                        let order = order.clone();
                        PlatformSpawner::spawn(async move {
                            match get_order_payments(&order.id).await {
                                Ok(payments) => {
                                    for payment in payments {
                                        let _ = tx.try_send(payment);
                                    }
                                },
                                Err(e) => log::error!("Error getting payment details: {e:?}"),
                            }
                        });
                    }

                    // Store orders for each employee based on their involvement
                    for emp in self.employees.iter() {
                        let uid = emp.id.clone();
                        let emp_orders: Vec<Order> = new_orders
                            .iter()
                            .filter(|o| o.id_employee_sales_rep == uid || o.id_employee_split_rep == uid)
                            .cloned()
                            .collect();

                        if self.pulling_all_orders {
                            let orders = self.orders.entry(uid.clone()).or_default();
                            for order in emp_orders {
                                if !orders.iter().any(|o| o.id == order.id) {
                                    orders.push(order);
                                }
                            }
                            orders.sort_by(sort);
                        } else {
                            self.orders.insert(uid.clone(), emp_orders);
                        }
                    }
                    
                    self.rebuild_koth_rows_all();
                    self.rebuild_all_employees_rows();
                }
            }
        }
        
        let mut got_payment = false;
        while let Ok(payment) = self.order_payment_rx.try_recv() {
            got_payment = true;
            changed = true;
            match self.koth_selection {
                KothSelection::Me => {
                    let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
                    // Find the order to determine split
                    let maybe_order = self
                        .orders
                        .get(&uid)
                        .and_then(|os| os.iter().find(|o| o.id == payment.id_order));

                    if let Some(order) = maybe_order {
                        // Skip zero or invalid payments
                        let amt = payment.amount.parse::<f64>().unwrap_or(0.0); 
                        if amt > 0.0 {
                            let mut p = payment.clone();
                            // If this is a true split (two different reps), halve the payment before storing
                            let is_true_split = !order.id_employee_split_rep.trim().is_empty()
                                && order.id_employee_sales_rep != order.id_employee_split_rep
                                && (order.id_employee_sales_rep == uid || order.id_employee_split_rep == uid)
                                && order.id_employee_split_rep != "0".to_string();
                            if is_true_split {
                                p.amount = format!("{}", amt / 2.0);
                            }
                            push_payment(&mut self.payments, uid, p);
                        }
                    } else {
                        // Fallback: if we can't find the order, just store as-is if positive
                        let amt = payment.amount.parse::<f64>().unwrap_or(0.0);
                        if amt > 0.0 {
                            push_payment(&mut self.payments, uid, payment.clone());
                        }
                    }
                },
                KothSelection::AllEmployees => {
                    let order_id = payment.id_order.clone();
                    // Get the actual order to determine split and recipients
                    let order_opt = self
                        .orders
                        .values()
                        .flat_map(|os| os.iter())
                        .find(|o| o.id == order_id);

                    let base_amt = payment.amount.parse::<f64>().unwrap_or(0.0);
                    if base_amt <= 0.0 { continue; }

                    if let Some(order) = order_opt {
                        let sales = order.id_employee_sales_rep.clone();
                        let split_rep = order.id_employee_split_rep.clone();
                        let is_true_split = !split_rep.trim().is_empty()
                            && split_rep != "0"
                            && sales != split_rep;

                        // Build recipients: always sales rep; include split rep only on true split
                        let mut recipients: Vec<String> = vec![];
                        if !sales.is_empty() && sales != "0" { recipients.push(sales); }
                        if is_true_split { recipients.push(split_rep); }
                        recipients = recipients.into_iter().unique().collect();

                        for emp_id in recipients {
                            let mut p = payment.clone();
                            if is_true_split { p.amount = format!("{}", base_amt / 2.0); }
                            push_payment(&mut self.payments, emp_id, p);
                        }
                    } else {
                        // Fallback: previous behavior using discovered employees, halving if two distinct employees involved
                        let relevant_employees: Vec<String> = self
                            .orders
                            .iter()
                            .filter(|(_, orders)| orders.iter().any(|o| o.id == order_id))
                            .filter_map(|(emp_id, orders)| {
                                orders
                                    .iter()
                                    .find(|o| {
                                        o.id == order_id && (o.id_employee_sales_rep == *emp_id || o.id_employee_split_rep == *emp_id)
                                    })
                                    .map(|_| emp_id.clone())
                            })
                            .collect();
                        let uniq: Vec<String> = relevant_employees.into_iter().unique().collect();
                        let split = if uniq.len() >= 2 { 2.0 } else { 1.0 };
                        for emp_id in uniq {
                            let mut p = payment.clone();
                            if split > 1.0 { p.amount = format!("{}", base_amt / split); }
                            push_payment(&mut self.payments, emp_id, p);
                        }
                    }
                }
            }
        }
        if got_payment {
            match self.koth_selection {
                KothSelection::Me => self.rebuild_koth_rows(),
                KothSelection::AllEmployees => {
                    self.rebuild_koth_rows_all();
                    self.rebuild_all_employees_rows();
                }
            }
        }

        while let Ok(mut employees) = self.employee_rx.try_recv() {
            changed = true;
            let pay_period = self.pay_period.clone();
            let tx = self.response_tx.clone();
            let emps = employees.clone();
            PlatformSpawner::spawn(async move {
                for employee in emps.iter() {
                    let id_employee = &employee.id;
                    if id_employee != "0" {
                        // for state in OrderState::VALUES.iter() { }
                        let period = pay_period.clone();
                        // if *state != OrderState::Returned { }
                        // let state_id = state.to_id().to_string();
                        let res = generate_orders_report(
                            period, 
                            &OrderState::AcceptedByOdoo.to_id().to_string(), 
                            &id_employee
                        ).await;
                        
                        log::info!("Result: {res:?}");
                        match res {
                            Ok(orders) => { let _ = tx.try_send(orders); },
                            Err(e) => log::error!("Error getting orders for koth: {e:?}"),
                        }      
                    }
                }
            });
            
            self.employees.append(&mut employees);
            self.rebuild_koth_rows_all();
            self.rebuild_all_employees_rows();
        }

        if changed {
            ctx.request_repaint();
        }
    }

}

/// Adds `payment` to `emp_id`'s payments unless one with the same id is already there.
fn push_payment(payments: &mut HashMap<String, Vec<OrderPayment>>, emp_id: String, payment: OrderPayment) {
    let entry = payments.entry(emp_id).or_default();
    if !entry.iter().any(|p| p.id == payment.id) {
        entry.push(payment);
    }
}
