use eframe::egui::{Button, CentralPanel, ComboBox, FontId, Grid, Id, RichText, ScrollArea, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::{helper_traits::EmployeeHelper, prestashop::{generate_orders_report, get_order_payments, Employee, Order, OrderPayment, OrderState, PayPeriod}, Store, User};
use crate::{get_current_user_from_auth, modals::tabs::return_colors, PlatformSpawner, Spawner};
use crossbeam::channel::{Receiver, Sender};
use chrono::NaiveDateTime;
use itertools::Itertools;
use std::{collections::HashMap, f32};
use egui_data_table::{DataTable, Renderer};

pub mod data;
pub mod row_viewer;
use data::KothTableData;
use row_viewer::KothRowViewer;

pub struct Koth {
    response_tx: Sender<Vec<Order>>,
    response_rx: Receiver<Vec<Order>>,
    employee_tx: Sender<Vec<Employee>>,
    employee_rx: Receiver<Vec<Employee>>,
    order_payment_tx: Sender<OrderPayment>,
    order_payment_rx: Receiver<OrderPayment>,
    orders: HashMap<String, Vec<Order>>,
    payments: HashMap<String, Vec<OrderPayment>>,
    employees: Vec<Employee>,
    order_state: OrderState,
    koth_selection: KothSelection,
    pay_period: PayPeriod,
    user: User,
    total: f64,
    total_w_tax: f64,
    pulling_all_orders: bool,
    // egui_data_table for per-employee orders view
    koth_table: DataTable<KothTableData>,
    koth_viewer: KothRowViewer,
}

#[derive(Default, PartialEq)]
enum KothSelection {
    #[default]
    Me,
    AllEmployees
}

impl KothSelection {
    fn as_str(&self) -> &str {
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
            pulling_all_orders: false,
            koth_table: DataTable::new(),
            koth_viewer: KothRowViewer::default(),
        }
    }
}

impl Koth {
    pub fn ui(&mut self, ui: &mut Ui) {
        TopBottomPanel::top("KothTopPanel")
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                TextEdit::singleline(&mut self.koth_viewer.filter)
                    .desired_width(150.)
                    .hint_text(" Search")
                    .ui(ui);
                // generate_orders_report
                ComboBox::new("Koth OrderState", "")
                    .selected_text(self.order_state.as_str())
                    .show_ui(ui, |ui| {
                        let selected = &mut self.order_state;
                        ui.selectable_value(
                            selected, 
                            OrderState::AcceptedByOdoo,
                            OrderState::AcceptedByOdoo.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::Shipped,
                            OrderState::Shipped.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::DeliveredToStore,
                            OrderState::DeliveredToStore.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::DoneShelf,
                            OrderState::DoneShelf.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::OrderPlaced,
                            OrderState::OrderPlaced.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::PrePulled,
                            OrderState::PrePulled.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::ReadyToBuild,
                            OrderState::ReadyToBuild.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::QcAndBurnin,
                            OrderState::QcAndBurnin.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::ShipToStore,
                            OrderState::ShipToStore.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            OrderState::Returned,
                            OrderState::Returned.as_str()
                        );
                    });

                ComboBox::new("Koth PayPeriod", "")
                    .selected_text(self.pay_period.as_str())
                    .show_ui(ui, |ui| {
                        let selected = &mut self.pay_period;
                        ui.selectable_value(
                            selected, 
                            PayPeriod::Current,
                            PayPeriod::Current.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            PayPeriod::Last,
                            PayPeriod::Last.as_str()
                        );
                    });

                ComboBox::new("Koth Selection", "")
                    .selected_text(self.koth_selection.as_str())
                    .show_ui(ui, |ui| {
                        let selected = &mut self.koth_selection;
                        ui.selectable_value(
                            selected, 
                            KothSelection::Me,
                            KothSelection::Me.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            KothSelection::AllEmployees,
                            KothSelection::AllEmployees.as_str()
                        );
                    });

                if Button::new("Pull KOTH").ui(ui).clicked() {
                    self.pulling_all_orders = false;
                    self.total = 0.0;
                    self.total_w_tax = 0.0;
                    self.orders = HashMap::new();
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
                            let res = generate_orders_report(pay_period, &state.id().to_string(), &id_employee).await;
                            log::info!("Result: {res:?}");
                            match res {
                                Ok(orders) => {let _ = tx.try_send(orders);},
                                Err(e) => log::error!("Error getting orders for koth: {e:?}"),
                            }
                        }
                    });
                }

                if Button::new("Pull ALL orders").ui(ui).clicked() {
                    self.pulling_all_orders = true;
                    self.total = 0.0;
                    self.total_w_tax = 0.0;
                    self.orders = HashMap::new();
                    
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
                                            let state_id = state.id().to_string();
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
                                for store in Store::VALUES {
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

        CentralPanel::default()
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(RichText::new("Orders").font(FontId::proportional(17.)));
                ui.separator();
            });
            ui.add_space(5.);
            ScrollArea::vertical()
            .max_height(f32::INFINITY)
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.group(|ui| {
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
                                    s.auto_shrink = [false, false].into();
                                    s.single_click_edit_mode = false;
                                })
                                .ui(ui);

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
                                .filter(|o| o.id_order_type != "2")
                                .flat_map(|o| o.associations.order_rows.iter())
                                .filter(|a| a.product_reference.to_lowercase().starts_with("lap/"))
                                .count();
                            let total_desktops = my_orders
                                .iter()
                                .filter(|o| o.id_order_type != "2")
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
                            let total_orders = self.orders.iter().count();

                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(format!("Sales: {total_sales} / Orders: {total_orders}"));
                                ui.label("");
                                ui.label("");
                                ui.label("");
                                ui.label(format!("Laptops: {total_laptops} / Desktops: {total_desktops}"));
                                ui.colored_label(ui.style().visuals.error_fg_color, format!("Finance ratio: {ar_financing_ratio:.2}%"));
                                ui.colored_label(ui.style().visuals.error_fg_color, format!("WTY's: {} out of {total_sales} sales", {
                                    // recompute warranties
                                    my_orders.iter().filter(|order| {
                                        order.associations.order_rows.iter().any(|o| o.product_reference.to_lowercase().starts_with("wty/") && !o.product_price.starts_with("0.0"))
                                    }).count()
                                }));
                                ui.colored_label(ui.style().visuals.warn_fg_color, format!("$ {:.2}", self.total_w_tax));
                                ui.colored_label(ui.style().visuals.warn_fg_color, format!("REVENUE: $ {:.2}", self.total));
                            });
                        }
                        KothSelection::AllEmployees => self.all_employees_grid(ui),
                    }
                });
            });
        });
    }

    pub fn current_employee_grid(&mut self, ui: &mut Ui) {
    // No-op: replaced by egui_data_table in `ui()` for the "Me" view.
    }

    pub fn all_employees_grid(&mut self, ui: &mut Ui) {
        Grid::new(Id::new("All Employees Orders Grid"))
        .spacing(Vec2::new(2., 4.))
        .max_col_width(ui.available_width() / 6.)
        .min_col_width(ui.available_width() / 6.)
        .with_row_color(|num, style| return_colors(num, style))
        .show(ui, |ui| {
            ui.style_mut().override_font_id = Some(FontId::proportional(15.));
            ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Employee Name").underline());
            ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Total Sales / Total Orders").underline());
            ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Laptops / Desktops").underline());
            ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Finance ratio").underline());
            ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Warranty Ratio").underline());
            ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Revenue $").underline());
            ui.end_row();

            // Collect employee data with metrics
            let mut employee_data: Vec<(&Employee, usize, usize, usize, f64, usize, f64)> = self
                .employees
                .iter()
                .map(|employee| {
                    let emp_id = &employee.id;

                    let orders = self
                        .orders
                        .iter()
                        .flat_map(|(_, orders)| orders)
                        .filter(|order|
                            order.id_employee_sales_rep == *emp_id
                            || order.id_employee_split_rep == *emp_id
                        )
                        .collect::<Vec<&Order>>();

                    let emp_payments = self
                        .payments
                        .iter()
                        .filter(|(id, _)| **id == *emp_id)
                        .flat_map(|(_, payments)| payments)
                        .cloned()
                        .collect::<Vec<OrderPayment>>();

                    let total_laptops = orders
                        .iter()
                        .filter(|o| o.id_order_type != "2")
                        .flat_map(|o| o.associations.order_rows.iter())
                        .filter(|a| a.product_reference.to_lowercase().starts_with("lap/"))
                        .count();

                    let total_desktops = orders
                        .iter()
                        .filter(|o| o.id_order_type != "2")
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

                    let total_financed = orders
                        .iter()
                        .filter(|o| {
                            emp_payments
                                .iter()
                                .any(|p| p.payment_method == "Financing Payment" && p.id_order == o.id)
                        })
                        .map(|o| o.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0))
                        .sum::<f64>();

                    let mut total = 0.0;
                    let mut total_warranties = 0;
                    // let total_orders = orders.iter().count();

                    for order in orders {
                        let total_paid_tax_excl = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                        if total_paid_tax_excl > 0.0 {
                            total += total_paid_tax_excl;
                        }

                        let warranty = order.associations.order_rows
                            .iter()
                            .filter_map(|o| {
                                if o.product_reference.to_lowercase().starts_with("wty/")
                                    && !o.product_price.starts_with("0.0")
                                {
                                    Some(o.product_reference.clone())
                                } else {
                                    None
                                }
                            })
                            .next()
                            .unwrap_or_else(|| "-".to_string());

                        if warranty.as_str() != "-" {
                            total_warranties += 1;
                        }
                    }

                    let ar_financing_ratio = if total > 0.0 { (total / total_financed) * 100.0 } else { 0.0 };

                    (
                        employee,
                        total_sales,
                        total_laptops,
                        total_desktops,
                        ar_financing_ratio,
                        total_warranties,
                        total,
                    )
                })
                .filter(|(_, total_sales, _, _, _, _, _)| *total_sales > 0) // Filter out zero sales
                .collect();

            // Sort by revenue (total) in descending order
            employee_data.sort_by(|a, b| {
                b.6.partial_cmp(&a.6).unwrap_or(std::cmp::Ordering::Equal)
            });

            // Render sorted employees
            for (employee, total_sales, total_laptops, total_desktops, ar_financing_ratio, total_warranties, total) in employee_data {
                ui.label(format!("{} {}", employee.firstname, employee.lastname));
                ui.label(format!(
                    "{total_sales} / {}", 
                    self
                    .orders
                    .iter()
                    .filter(|(id, _)| id == &&employee.id)
                    .map(|(_, orders)| orders.len())
                    .sum::<usize>()
                ));
                ui.label(format!("{} / {}", total_laptops.to_string(), total_desktops.to_string()));
                ui.label(format!("{:.2}%", ar_financing_ratio));
                ui.label(format!("{total_warranties} / {total_sales}"));
                ui.label(format!("$ {:.2}", total));
                ui.end_row();
            }
        });
    }

    pub fn receive(&mut self) {
        if let Ok(orders) = self.response_rx.try_recv() {
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

                    // Process each order
                    for order in new_orders.iter() {
                        let total_paid_tax_excl: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                        let total_paid_tax: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);

                        if total_paid_tax_excl > 0.0 {
                            self.total += total_paid_tax_excl;
                        }
                        if total_paid_tax > 0.0 {
                            self.total_w_tax += total_paid_tax;
                        }

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
                    self.rebuild_koth_rows_for_me();
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
                            if let Some(orders) = self.orders.get_mut(&uid) {
                                orders.extend(emp_orders);
                                orders.sort_by(sort);
                            } else {
                                self.orders.insert(uid.clone(), emp_orders);
                            }
                        } else {
                            self.orders.insert(uid.clone(), emp_orders);
                        }
                    }
                }
            }
        }

        if let Ok(payment) = self.order_payment_rx.try_recv() {
            match self.koth_selection {
                KothSelection::Me => {
                    let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
                    self.payments.entry(uid).or_insert_with(Vec::new).push(payment.clone());
                    // Update table to reflect payment method column
                    self.rebuild_koth_rows_for_me();
                },
                KothSelection::AllEmployees => { // PROBLEM - LOOK INTO MULTIPLE PAYMENTS ON SAME ORDER. Am i accounting for this?
                    let order_id = payment.id_order.clone();
                    // Find employees associated with this order
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

                    for emp_id in relevant_employees {
                        self.payments
                            .entry(emp_id)
                            .or_insert_with(Vec::new)
                            .push(payment.clone());
                    }
                }
            }
        }
    
        if let Ok(mut employees) = self.employee_rx.try_recv() {
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
                        // let state_id = state.id().to_string();
                        let res = generate_orders_report(
                            period, 
                            &OrderState::AcceptedByOdoo.id().to_string(), 
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
        }
    }
}

impl Koth {
    fn rebuild_koth_rows_for_me(&mut self) {
        let uid = self.user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
        let my_orders = self.orders.get(&uid).cloned().unwrap_or_default();
        let my_payments = self.payments.get(&uid).cloned().unwrap_or_default();

        let mut rows: Vec<KothTableData> = Vec::with_capacity(my_orders.len());
        for (i, order) in my_orders.iter().enumerate() {
            let state = OrderState::state_from_id_str(&order.current_state);
            let date_str = match state { OrderState::AcceptedByOdoo => order.delivery_date.clone(), _ => order.date_upd.clone() };
            let total_paid_num: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);
            let total_paid_tax_excl_num: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);

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

            rows.push(KothTableData {
                idx: i,
                order_id: order.id.clone(),
                date: display_date,
                order_state: OrderState::from_id_str(&order.current_state).to_string(),
                product,
                payment,
                warranty,
                total_paid: total_paid_num,
                total_without_tax: total_paid_tax_excl_num,
            });
        }
        self.koth_table.replace(rows);
    }
}



/*
use crate::{get_current_user_from_auth, tabs::koth::{data::KothTableData, row_viewer::KothRowViewer}, PlatformSpawner, Spawner};
use database::schema::{prestashop::{get_order_payments, Order, OrderPayment, OrderState, PayPeriod}, User};
use crossbeam::channel::{Receiver, Sender};
use egui_data_table::DataTable;
use itertools::Itertools;

pub mod row_viewer;
pub mod ui;
pub mod data;
pub mod codec;

pub struct Koth {
    koth_viewer: KothRowViewer,
    koth_table: DataTable<KothTableData>,
    order_state: OrderState,
    pay_period: PayPeriod,
    payments: Vec<OrderPayment>,
    pub response_tx: Sender<Vec<Order>>,
    pub response_rx: Receiver<Vec<Order>>,
    pub order_payment_tx: Sender<OrderPayment>,
    pub order_payment_rx: Receiver<OrderPayment>,
    pub orders: Vec<Order>,
    pub user: User,
    pub total: f64,
    pub total_w_tax: f64,
    pulling_all_orders: bool,
}

impl Default for Koth {
    fn default() -> Self {
        let (response_tx, response_rx) = crossbeam::channel::unbounded();
        let (order_payment_tx, order_payment_rx) = crossbeam::channel::unbounded();

        Self {
            koth_viewer: KothRowViewer::default(),
            koth_table: DataTable::new(),
            payments: Vec::new(),
            order_state: Default::default(),
            pay_period: Default::default(),
            pulling_all_orders: false,
            response_tx, response_rx,
            order_payment_tx, order_payment_rx,
            orders: Default::default(),
            user: if let Some(usr) = get_current_user_from_auth() {
                usr.clone()
            } else {
                User::default()
            },
            total: 0.0,
            total_w_tax: 0.0,
        }
    }
}

impl Koth {
    pub fn receive(&mut self) {
        if let Ok(orders) = self.response_rx.try_recv() {

            let sort = |a: &Order, b: &Order| {
                let a_total: f64 = a.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                let b_total: f64 = b.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)
            };

            let mut new_orders: Vec<Order> = orders
                .clone()
                .iter()
                .filter(|o| !o.id.is_empty())
                .sorted_by(|a, b| sort(a, b))
                .cloned()
                .collect();

            // Process each order
            for order in new_orders.iter() {
                let total_paid_tax_excl: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                let total_paid_tax: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);

                if total_paid_tax_excl > 0.0 {
                    self.total += total_paid_tax_excl;
                }
                if total_paid_tax > 0.0 {
                    self.total_w_tax += total_paid_tax;
                }

                let tx = self.order_payment_tx.clone();
                let order = order.clone();
                PlatformSpawner::spawn(async move {
                    match get_order_payments(&order.id).await {
                        Ok(payment) => { let _ = tx.try_send(payment); },
                        Err(e) => log::error!("Error getting payment detials: {e:?}"),
                    }
                });
            }

            if self.pulling_all_orders {
                self.orders.append(&mut new_orders);
                self.orders.sort_by(sort);
            } else {
                self.orders = new_orders;
            }
        }

        if let Ok(payment) = self.order_payment_rx.try_recv() {
            self.payments.push(payment);
        }
    }
}

*/