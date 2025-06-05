use eframe::egui::{Button, CentralPanel, Color32, ComboBox, FontId, Grid, Hyperlink, Id, RichText, ScrollArea, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::{prestashop::{generate_orders_report, get_order_payments, Order, OrderPayment, OrderState, PayPeriod}, User};
use crate::{get_current_user_from_auth, modals::tabs::return_colors, PlatformSpawner, Spawner};
use crate::tabs::task_audit::row_viewer::BASE_URL;
use crossbeam::channel::{Receiver, Sender};
use chrono::NaiveDateTime;
use itertools::Itertools;
use std::f32;

pub struct Koth {
    response_tx: Sender<Vec<Order>>,
    response_rx: Receiver<Vec<Order>>,
    order_payment_tx: Sender<OrderPayment>,
    order_payment_rx: Receiver<OrderPayment>,
    orders: Vec<Order>,
    order_state: OrderState,
    pay_period: PayPeriod,
    user: User,
    total: f64,
    total_w_tax: f64,
    payments: Vec<OrderPayment>,
    pulling_all_orders: bool,
}

impl Default for Koth {
    fn default() -> Self {
        let (response_tx, response_rx) = crossbeam::channel::unbounded();
        let (order_payment_tx, order_payment_rx) = crossbeam::channel::unbounded();
        Self {
            response_tx, response_rx,
            order_payment_tx, order_payment_rx,
            payments: Vec::new(),
            orders: Default::default(),
            order_state: Default::default(),
            pay_period: Default::default(),
            user: if let Some(usr) = get_current_user_from_auth() {
                usr.clone()
            } else {
                User::default()
            },
            total: 0.0,
            total_w_tax: 0.0,
            pulling_all_orders: false,
        }
    }
}

impl Koth {
    pub fn ui(&mut self, ui: &mut Ui) {
        TopBottomPanel::top("KothTopPanel")
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
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

                if Button::new("Pull KOTH").ui(ui).clicked() {
                    self.pulling_all_orders = false;
                    self.total = 0.0;
                    self.total_w_tax = 0.0;
                    self.orders = vec![];
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
                    self.orders = vec![];
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
                    Grid::new(Id::new("Orders Grid"))
                    .spacing(Vec2::new(2., 4.))
                    .max_col_width(ui.available_width() / 9.)
                    .min_col_width(ui.available_width() / 9.)
                    .with_row_color(|num, style| return_colors(num, style))
                    .show(ui, |ui| {

                        let date = match self.order_state {
                            OrderState::AcceptedByOdoo if !self.pulling_all_orders => "Delivery Date",
                            _ => "Date Updated"
                        };

                        ui.style_mut().override_font_id = Some(FontId::proportional(15.));
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("#").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("ID").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new(date).underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Order State").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Product").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Payment Type").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Warranty").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Total Paid").underline());
                        ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Total Without Tax").underline());
                        ui.end_row();

                        let total_laptops = self
                            .orders
                            .iter()
                            .filter(|o| o.id_order_type != "2")
                            .flat_map(|o| o.associations.order_rows.iter())
                            .filter(|a| a.product_reference.to_lowercase().starts_with("lap/"))
                            .count();

                        let total_desktops = self
                            .orders
                            .iter()
                            .filter(|o| o.id_order_type != "2")
                            .flat_map(|o| o.associations.order_rows.iter())
                            .filter_map(|o| {
                                if !o.product_reference.to_lowercase().starts_with("lap/") 
                                    && o.product_reference.to_lowercase().starts_with("case/")
                                    && !o.product_reference.to_lowercase().starts_with("case/15")
                                    && !o.product_reference.to_lowercase().starts_with("case/17")
                                {
                                    Some(o.product_reference.clone())
                                } else { 
                                    None 
                                }
                            })
                            .count();

                        
                        let total_financed = self
                        .orders
                        .iter()
                        .filter(|o| {
                            self.payments
                            .iter()
                            .any(|p| p.payment_method == "Financing Payment" && p.id_order == o.id)
                        })
                        .map(|o| o.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0))
                        .sum::<f64>();
                    
                        let ar_financing_ratio = if total_financed > 0.0 && self.total > 0.0 { self.total / total_financed } else { 0.0 };
                        let total_sales = total_desktops + total_laptops;
                        let total_warranties = &mut 0;
                        let total_orders = self.orders.iter().count();

                        for (i, order) in self.orders.iter().enumerate() {
                            let order_id = order.id.clone();
                            let state = OrderState::state_from_id_str(&order.current_state);
                            let delivery_date = match state {
                                OrderState::AcceptedByOdoo => order.delivery_date.clone(),
                                _ => order.date_upd.clone()
                            };
                            let total_paid_num: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);

                            let total_paid = if total_paid_num == 0.0 {
                                "$ 0.0".to_string()
                            } else {
                                format!("$ {:.2}", total_paid_num)
                            };
                            let total_paid_tax_excl_num: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                            let total_paid_tax_excl = if total_paid_tax_excl_num == 0.0 {
                                "$ 0.0".to_string()
                            } else {
                                format!("$ {:.2}", total_paid_tax_excl_num)
                            };

                            let computer: String = order.associations.order_rows
                            .iter()
                            .filter_map(|o| {
                                if o.product_reference.to_lowercase().starts_with("lap") 
                                    || o.product_reference.to_lowercase().starts_with("case/")
                                    && !o.product_reference.to_lowercase().starts_with("case/15")
                                    && ! o.product_reference.to_lowercase().starts_with("case/17")
                                {
                                    Some(o.product_reference.clone())
                                } else { 
                                    None 
                                }
                            })
                            .next()
                            .unwrap_or_else(|| {
                                // Fall back to the first product_reference if no matches
                                order
                                    .associations
                                    .order_rows
                                    .first()
                                    .map(|o| o.product_reference.clone())
                                    .unwrap_or_default()
                            });

                            let warranty = order.associations.order_rows
                            .iter()
                            .filter_map(|o| {
                                if o.product_reference.to_lowercase().starts_with("wty/")
                                {
                                    Some(o.product_reference.clone())
                                } else { 
                                    None 
                                }
                            })
                            .next()
                            .unwrap_or_else(|| "-".to_string());

                            if warranty.as_str() != "-" {
                                *total_warranties += 1;
                            }

                            let payment = self
                            .payments
                            .iter()
                            .find(|p| p.id_order == order.id)
                            .map(|p| p.payment_method.clone())
                            .unwrap_or("-".to_string());

                            ui.label(i.to_string());
                            Hyperlink::from_label_and_url(
                                RichText::new(order_id.clone()).underline().strong().color(Color32::LIGHT_RED), 
                                format!("{BASE_URL}{}", order_id)
                            )
                            .open_in_new_tab(true)
                            .ui(ui);
                            ui.label(
                                NaiveDateTime::parse_from_str(&delivery_date, "%Y-%m-%d %H:%M:%S")
                                .map(|dt| dt.format("%m / %d / %Y").to_string())
                                .unwrap_or_else(|_| String::new())
                            );
                            ui.label(OrderState::from_id_str(&order.current_state).to_string());
                            ui.label(computer);
                            ui.label(payment);
                            ui.label(warranty);
                            ui.label(total_paid);
                            ui.label(total_paid_tax_excl);
                            ui.end_row();
                        }
                        
                        for _ in 1..9 { ui.label(""); }
                        ui.end_row();

                        ui.label(format!("Sales: {total_sales} / Orders: {total_orders}"));
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label(format!("Laptops: {total_laptops} / Desktops: {total_desktops}"));
                        ui.colored_label(ui.style().visuals.error_fg_color, format!("Finance ratio: {ar_financing_ratio:.2}%"));
                        ui.colored_label(ui.style().visuals.error_fg_color, format!("WTY's: {total_warranties} out of {total_sales} sales"));
                        ui.colored_label(ui.style().visuals.warn_fg_color, format!("$ {:.2}", self.total_w_tax));
                        ui.colored_label(ui.style().visuals.warn_fg_color, format!("REVENUE: $ {:.2}", self.total));
                        ui.end_row();
                    });
                });
            });
        });
    }

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

