use eframe::egui::{Button, CentralPanel, Color32, ComboBox, FontId, Grid, Hyperlink, Id, RichText, ScrollArea, TopBottomPanel, Ui, Vec2, Widget};
use database::{schema::{prestashop::{generate_orders_report, Order, OrderState, PayPeriod}, User}};
use itertools::Itertools;
use crate::{get_current_user_from_auth, modals::tabs::return_colors, PlatformSpawner, Spawner};
use crate::tabs::task_audit::row_viewer::BASE_URL;
use crossbeam::channel::{Receiver, Sender};
use std::f32;

pub struct Koth {
    response_tx: Sender<Vec<Order>>,
    response_rx: Receiver<Vec<Order>>,
    orders: Vec<Order>,
    order_state: OrderState,
    pay_period: PayPeriod,
    user: User,
    total: f64,
    total_w_tax: f64
}

impl Default for Koth {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            response_tx: tx,
            response_rx: rx,
            orders: Default::default(),
            order_state: Default::default(),
            pay_period: Default::default(),
            user: if let Some(usr) = get_current_user_from_auth() {
                usr.clone()
            } else {
                User::default()
            },
            total: 0.0,
            total_w_tax: 0.0
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
                            .max_col_width(ui.available_width() / 5.)
                            .min_col_width(ui.available_width() / 5.)
                            .with_row_color(|num, style| return_colors(num, style))
                            .num_columns(5)
                            .show(ui, |ui| {
                                    let date = match self.order_state {
                                        OrderState::AcceptedByOdoo => "Delivery Date",
                                        _ => "Date Updated"
                                    };
                                    ui.style_mut().override_font_id = Some(FontId::proportional(15.));
                                    ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("ID").underline());
                                    ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Product").underline());
                                    ui.colored_label(ui.style().visuals.error_fg_color, RichText::new(date).underline());
                                    ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Total Paid").underline());
                                    ui.colored_label(ui.style().visuals.error_fg_color, RichText::new("Total Without Tax").underline());
                                    ui.end_row();
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.end_row();
                                    for order in self.orders.iter() {
                                        let order_id = order.id.clone();
                                        let delivery_date = match self.order_state {
                                            OrderState::AcceptedByOdoo => order.delivery_date.clone(),
                                            _ => order.date_upd.clone()
                                        };
                                        let total_paid_num: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);
                                        let total_paid = if total_paid_num == 0.0 {
                                            order.total_paid.clone()
                                        } else {
                                            format!("${:.2}", total_paid_num)
                                        };
                                        let total_paid_tax_excl_num: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                                        let total_paid_tax_excl = if total_paid_tax_excl_num == 0.0 {
                                            order.total_paid_tax_excl.clone()
                                        } else {
                                            format!("${:.2}", total_paid_tax_excl_num)
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

                                        // log::info!("Comp: {computer:?}");
                                        
                                        Hyperlink::from_label_and_url(
                                            RichText::new(order_id.clone()).underline().strong().color(Color32::LIGHT_RED), 
                                            format!("{BASE_URL}{}", order_id)
                                        )
                                        .open_in_new_tab(true)
                                        .ui(ui);
                                        ui.label(computer);
                                        ui.label(delivery_date);
                                        ui.label(total_paid);
                                        ui.label(total_paid_tax_excl);
                                        ui.end_row();
                                    }
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.end_row();
                                    ui.colored_label(ui.style().visuals.error_fg_color, "Totals");
                                    ui.label("");
                                    ui.label("");
                                    ui.colored_label(ui.style().visuals.warn_fg_color, format!("${:.2}", self.total_w_tax));
                                    ui.colored_label(ui.style().visuals.warn_fg_color, format!("Revenue -> ${:.2}", self.total));
                                    ui.end_row();
                                });
                        });
                    });
                });
    }

    pub fn receive(&mut self) {
        if let Ok(orders) = self.response_rx.try_recv() {
        self.orders = orders
            .clone()
            .iter()
            .filter(|o| **o != Order::default())
            .sorted_by(|a, b| {
                let a_total: f64 = a.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                let b_total: f64 = b.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
            .collect();
            // Process each order
            for order in orders {
                let total_paid_tax_excl: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                let total_paid_tax: f64 = order.total_paid.parse::<f64>().unwrap_or(0.0);

                if total_paid_tax_excl > 0.0 {
                    self.total += total_paid_tax_excl;
                }
                if total_paid_tax > 0.0 {
                    self.total_w_tax += total_paid_tax;
                }
            }
        }
    }
}

