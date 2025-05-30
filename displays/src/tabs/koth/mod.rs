use eframe::egui::{Button, CentralPanel, ComboBox, FontId, Grid, Hyperlink, Id, ScrollArea, TopBottomPanel, Ui, Vec2, Widget};
use database::{schema::{prestashop::{generate_orders_report, Order, OrderState, PayPeriod}, User}};
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
    total: f64
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
            total: 0.0
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
                        ui.heading("Orders");
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
                            .max_col_width(ui.available_width() / 4.1)
                            .min_col_width(ui.available_width() / 4.1)
                            .with_row_color(|num, style| return_colors(num, style))
                            .num_columns(4)
                            .show(ui, |ui| {
                                    ui.style_mut().override_font_id = Some(FontId::proportional(15.));
                                    ui.colored_label(ui.style().visuals.error_fg_color, "ID");
                                    ui.colored_label(ui.style().visuals.error_fg_color, "Delivery Date");
                                    ui.colored_label(ui.style().visuals.error_fg_color, "Total Paid");
                                    ui.colored_label(ui.style().visuals.error_fg_color, "Total Without Tax");
                                    ui.end_row();
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.end_row();
                                    for order in self.orders.iter() {
                                        let order_id = order.id.clone();
                                        let delivery_date = order.delivery_date.clone();
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

                                        Hyperlink::from_label_and_url(
                                            order_id.clone(), 
                                            format!("{BASE_URL}{}", order_id)
                                        )
                                        .open_in_new_tab(true)
                                        .ui(ui);
                                        ui.label(delivery_date);
                                        ui.label(total_paid);
                                        ui.label(total_paid_tax_excl);
                                        ui.end_row();
                                    }
                                    ui.label("");
                                    ui.label("");
                                    ui.colored_label(ui.style().visuals.error_fg_color, "Total: ");
                                    ui.colored_label(ui.style().visuals.warn_fg_color, format!("${:.2}", self.total));
                                    ui.end_row();
                                });
                        });
                    });
                });
    }

    pub fn receive(&mut self) {
        if let Ok(orders) = self.response_rx.try_recv() {
            self.orders = orders.clone().iter().filter(|o| **o != Order::default()).cloned().collect();
            // Process each order
            for order in orders {
                let total_paid_tax_excl: f64 = order.total_paid_tax_excl.parse::<f64>().unwrap_or(0.0);
                
                if total_paid_tax_excl > 0.0 {
                    self.total += total_paid_tax_excl;
                }
            }
        }
    }
}

