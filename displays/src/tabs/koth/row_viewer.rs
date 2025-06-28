
use crate::{get_current_user_from_auth, get_database_users, tabs::koth::data::{KothData, KothTableData}, Interaction};
use database::schema::{ComputerData, CustomerData, LiveTaskPayload, TicketPayload, User, COMPUTER_TABLE, CUSTOMER_TABLE};
use eframe::egui::{Color32, Layout, Response, RichText, TextEdit};
use egui_data_table::RowViewer;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use egui_extras::Column;
use surrealdb::RecordId;
use std::cmp::Ordering;


#[derive(serde::Serialize)]
pub struct KothRowViewer {
    pub koth_table_data: KothTableData,
    pub current_user: User,
    pub store_users: Vec<User>,
    pub filter: String,
}


impl Default for KothRowViewer {
    fn default() -> Self {
        Self {
            current_user: if let Some(usr) = get_current_user_from_auth() {
                usr.clone()
            } else {
                User::default()
            },
            koth_table_data: KothTableData::default(),
            filter: Default::default(),
            store_users: get_database_users(),
        }
    }
}

impl RowViewer<KothTableData> for KothRowViewer {
    // fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<KothData>> {
    //     Some(Codec)
    // }

    fn num_columns(&mut self) -> usize { 8 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        [ "ID", "Delivery Date", "Order State", "Product", "Payment Type", "Warranty", "Total Paid", "Total Without Tax" ][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [false, true, true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &KothData) -> bool {
        row.
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &KothData, column: usize) {

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


                        // Ticket count, how many services tech's are completing

                        let total_desktops = self
                            .orders
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
                                    || o.product_reference.to_lowercase().starts_with("bsd/")
                                    || o.product_reference.to_lowercase().starts_with("rci/")
                                    || o.product_reference.to_lowercase().starts_with("r2r/")
                                    || o.product_reference.to_lowercase().starts_with("rtr/")
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
                        
        let _ = match column {
            // 0 => ui.label(format!(" {}", task_payload.id.key().to_string())),
            1 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                ui.add_space(2.);
                ui.label(RichText::new(task_payload.task_name.trim()))
            }).inner,
            2 => {
                let user = self.store_users
                    .iter()
                    .find(|u| u.get_id() == task_payload.assignee)
                    .cloned()
                    .unwrap_or_default();
                ui.vertical_centered(|ui| ui.label(format!(" {}", user.get_username()))).inner
            },
            3 => {
                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(task_payload.due_date.clone().naive_local(), Utc);
                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format(" %m/%d/%Y").to_string();
                let split1 = formatted_date.split_once('/').unwrap_or_default();
                let split2 = split1.1.split_once('/').unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}/", split1.0));
                    ui.colored_label(Color32::from_rgb(3, 252, 194), format!("{}/", split2.0));
                    ui.colored_label(Color32::from_rgb(66, 69, 245), split2.1)
                }).inner
            },
            4 => ui.vertical_centered(|ui| ui.label(task_payload.service_number.clone().unwrap_or_default().trim())).inner,
            5 => ui.vertical_centered(|ui| ui.label(format!("{}", task_payload.priority.as_str().trim()))).inner,
            6 => ui.vertical_centered(|ui| ui.label(format!(" {}", task_payload.status.as_str().trim()))).inner,
            7 => ui.vertical_centered(|ui| ui.checkbox(checked, "")).inner,
            8 => ui.label(task_payload.task_description.trim()),
            _ => ui.label(""),
        };
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> Column {
        let col_config = Column::auto();
        match self.selected {
            KothData::Task(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(20.).at_most(20.),
                    1 => col_config.resizable(true).at_least(200.).at_most(260.),
                    2 => col_config.resizable(true).at_least(90.).at_most(100.),
                    3 => col_config.resizable(true).at_least(110.).at_most(110.),
                    4 => col_config.resizable(true).at_least(60.).at_most(60.),
                    5 => col_config.resizable(true).at_least(70.).at_most(70.),
                    6 => col_config.resizable(true).at_least(80.).at_most(80.),
                    7 => col_config.resizable(true).at_least(50.).at_most(50.),
                    8 => col_config.resizable(true).at_least(600.),
                    _ => col_config,
                }
            },
            KothData::User(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(180.).at_most(180.),
                    1 => col_config.resizable(true).at_least(120.).at_most(120.),
                    2 => col_config.resizable(true).at_least(155.).at_most(155.),
                    3 => col_config.resizable(true).at_least(280.).at_most(280.),
                    4 => col_config.resizable(true).at_least(60.).at_most(60.),
                    5 => col_config.resizable(true).at_least(80.).at_most(80.),
                    6 => col_config.resizable(true).at_least(60.).at_most(60.),
                    7 => col_config.resizable(true).at_least(60.).at_most(60.),
                    _ => col_config,
                }
            },
            KothData::Customer(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(180.).at_most(180.),
                    1 => col_config.resizable(true).at_least(120.).at_most(120.),
                    2 => col_config.resizable(true).at_least(155.).at_most(155.),
                    3 => col_config.resizable(true).at_least(280.).at_most(280.),
                    _ => col_config,
                }
            },
            KothData::Computer(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(200.).at_most(200.),
                    1 => col_config.resizable(true).at_least(140.).at_most(140.),
                    2 => col_config.resizable(true).at_least(115.).at_most(155.),
                    3 => col_config.resizable(true).at_least(115.).at_most(155.),
                    4 => col_config.resizable(true).at_least(115.).at_most(155.),
                    5 => col_config.resizable(true).at_least(115.).at_most(155.),
                    6 => col_config.resizable(true).at_least(200.).at_most(200.),
                    7 => col_config.resizable(true).at_least(200.).at_most(200.),
                    8 => col_config.resizable(true).at_least(60.).at_most(60.),
                    9 => col_config.resizable(true).at_least(150.).at_most(160.),
                    10 => col_config.resizable(true).at_least(115.).at_most(115.),
                    _ => col_config,
                }
            },
            KothData::Service(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(180.).at_most(210.),
                    1 => col_config.resizable(true).at_least(100.).at_most(120.),
                    2 => col_config.resizable(true).at_least(100.).at_most(120.),
                    3 => col_config.resizable(true).at_least(100.).at_most(120.),
                    4 => col_config.resizable(true).at_least(100.).at_most(120.),
                    5 => col_config.resizable(true).at_least(100.).at_most(120.),
                    6 => col_config.resizable(true).at_least(100.).at_most(150.),
                    7 => col_config.resizable(true).at_least(250.).at_most(350.),
                    _ => col_config,
                }
            },
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut KothData,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        let response: Option<Response> = match row {
            KothData::Task(task_payload) => {
                match column {
                    1 => Some(task_payload.interact_task_name(ui)),
                    2 => Some(task_payload.interact_assignee(ui, &self.store_users, &self.current_user)),
                    3 => Some(task_payload.interact_due_date(ui)),
                    4 => Some(task_payload.interact_service_number(ui)),
                    5 => Some(task_payload.interact_priority(ui)),
                    6 => Some(task_payload.interact_status(&self.current_user, ui)),
                    7 => Some(task_payload.interact_completed(ui)),
                    8 => Some(task_payload.interact_task_description(ui)),
                    _ => None,
                }
                .into()
            },
            KothData::User(user) => {
                match column {
                    0 => Some(ui.label(user.get_id().key().to_string())),
                    1 => Some(TextEdit::singleline(&mut user.get_name()).show(ui).response),
                    2 => Some(TextEdit::singleline(&mut user.get_username()).show(ui).response),
                    3 => Some(TextEdit::singleline(&mut user.get_email()).show(ui).response),
                    4 => Some(TextEdit::singleline(&mut user.get_store().as_str()).show(ui).response),
                    5 => Some(TextEdit::singleline(&mut user.get_employee_id().unwrap_or(0).to_string()).show(ui).response),
                    6 => Some(TextEdit::singleline(&mut user.get_store_id().unwrap_or(String::new())).show(ui).response),
                    7 => Some(TextEdit::singleline(&mut user.get_authorization().as_str()).show(ui).response),
                    _ => None
                }
                .into()
            },
            KothData::Customer(_customer) => None,
            KothData::Computer(_computer) => None,
            KothData::Service(_ticket) => None,
        };
        // ui.shrink_height_to_current();
        // ui.shrink_width_to_current();
        response
    }
    
    fn persist_ui_state(&self) -> bool {
        true
    }
    
    fn on_cell_view_response(
        &mut self,
        _row: &KothData,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<KothData>> {
        match column {
            _ => {}
        }
    
        resp
            .clone()
            .on_hover_and_drag_cursor(eframe::egui::CursorIcon::Crosshair)
            .dnd_release_payload::<String>()
            .map(|_| Box::new(KothData::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &KothData,
        dst: &mut KothData,
        _column: usize,
    ) {
        *dst = src.clone();
    }

    fn compare_cell(
        &self,
        row_l: &KothData,
        row_r: &KothData,
        column: usize,
    ) -> std::cmp::Ordering {
        match (row_l, row_r) {
            (KothData::Task(task_l), KothData::Task(task_r)) => {
                let user_l = self.store_users
                    .iter()
                    .find(|u| u.get_id() == task_l.assignee)
                    .cloned()
                    .unwrap_or_default();

                let user_r = self.store_users
                    .iter()
                    .find(|u| u.get_id() == task_r.assignee)
                    .cloned()
                    .unwrap_or_default();

                match column {
                    1 => task_l.task_name.cmp(&task_r.task_name),
                    2 => user_l.get_username().cmp(&user_r.get_username()),
                    3 => task_l.due_date.cmp(&task_r.due_date),
                    4 => task_l.service_number.clone().unwrap_or_default().cmp(&task_r.service_number.clone().unwrap_or_default()),
                    5 => task_l.priority.as_str().cmp(&task_r.priority.as_str()),
                    6 => task_l.status.as_str().cmp(&task_r.status.as_str()),
                    7 => task_l.completed.cmp(&task_r.completed),
                    8 => task_l.task_description.cmp(&task_r.task_description),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            }
            (KothData::User(user_l), KothData::User(user_r)) => {
                match column {
                    0 => user_l.get_id().cmp(&user_r.get_id()),
                    1 => user_l.get_name().cmp(&user_r.get_name()),
                    2 => user_l.get_email().cmp(&user_r.get_email()),
                    3 => user_l.get_store().cmp(&user_r.get_store()),
                    4 => user_l.is_active().cmp(&user_r.is_active()),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            }
            (KothData::Service(svc_l), KothData::Service(svc_r)) => {
                match column { // "ID", "SO#", "Tech", "Salesman", "Check-In Rep", "Customer", "Computer", "Check-In Note"
                    1 => svc_l.service_number.cmp(&svc_r.service_number),
                    2 => svc_l.tech.cmp(&svc_r.tech),
                    3 => svc_l.salesman.cmp(&svc_r.salesman),
                    4 => svc_l.checkin_rep.cmp(&svc_r.checkin_rep),
                    5 => svc_l.customer.clone().unwrap_or_default().name.cmp(&svc_r.customer.clone().unwrap_or_default().name),
                    6 => svc_l.computer.clone().unwrap_or_default().device_model.cmp(&svc_r.computer.clone().unwrap_or_default().device_model),
                    7 => svc_l.checkin_notes.cmp(&svc_r.checkin_notes),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            },
            (KothData::Customer(cust_l), KothData::Customer(cust_r)) => {
                match column {
                    1 => cust_l.name.cmp(&cust_r.name),
                    2 => cust_l.phone_number.cmp(&cust_r.phone_number),
                    3 => cust_l.email.cmp(&cust_r.email),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            },
            (KothData::Computer(computer_l), KothData::Computer(computer_r)) => {
                match column {
                    1 => computer_l.hostname.cmp(&computer_r.hostname),
                    2 => computer_l.device_mfg.clone().unwrap_or_default().cmp(&computer_r.device_mfg.clone().unwrap_or_default()),
                    3 => computer_l.device_model.clone().unwrap_or_default().cmp(&computer_r.device_model.clone().unwrap_or_default()),
                    4 => computer_l.device_name.clone().unwrap_or_default().cmp(&computer_r.device_name.clone().unwrap_or_default()),
                    5 => computer_l.device_serial.clone().unwrap_or_default().cmp(&computer_r.device_serial.clone().unwrap_or_default()),
                    6 => computer_l.cpu.as_str().cmp(&computer_r.cpu),
                    7 => computer_l.gpu.cmp(&computer_r.gpu),
                    8 => computer_l.ram.cmp(&computer_r.ram),
                    9 => computer_l.operating_system.cmp(&computer_r.operating_system),
                    10 => computer_l.customer.clone().unwrap_or(
                        RecordId::from((CUSTOMER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into())))
                        ).cmp(&computer_r.customer.clone().unwrap_or(
                            RecordId::from((CUSTOMER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into())))
                            )
                        ),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            },
            (_, _) => Ordering::Equal
        }
    }

    fn new_empty_row(&mut self) -> KothData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        KothData::default()
    }
}