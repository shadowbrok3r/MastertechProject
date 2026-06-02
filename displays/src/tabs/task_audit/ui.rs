use eframe::egui::{Button, CentralPanel, ComboBox, Id, Layout, RichText, ScrollArea, Separator, Spinner, TextEdit, Ui, Widget};
use eframe::egui::{Color32, Grid, Style, scroll_area};
use database::schema::prestashop::{OrderState, OrderType};
use crate::{PlatformSpawner, TaskUiActions};
use chrono::{DateTime, NaiveDateTime, Utc};
use database::schema::{Store, User};
use database::get_database_users;
use crossbeam::channel::Sender;
use egui_data_table::Renderer;
use crate::Spawner;
use log::info;

use super::row_viewer::TaskRowViewer;
use super::TaskAudit;
use super::TaskAuditViewer;

impl TaskAuditViewer {
    pub fn show(&mut self, ui: &mut Ui, current_user: Option<User>, ui_actions_tx: Sender<TaskUiActions>) {
        // First run: load database users for comboboxes
        if self.services_viewer.first_run {
            let users = get_database_users();
            if !users.is_empty() {
                self.services_viewer.users = users;
                self.services_viewer.first_run = false;
                info!("Loaded {} users for task audit comboboxes", self.services_viewer.users.len());
            }
        }
        
        // Handle create task channel
        if let Ok(order_payload) = self.services_viewer.create_task_channel.1.try_recv() {
            info!("Received create task request for order: {}", order_payload.order.id);
            // Send the order to the task creation modal
            let _ = ui_actions_tx.try_send(TaskUiActions::OpenCreateTaskModalFromOrder(order_payload));
        }
        
        if let Some(order) = self.services_viewer.selected.clone() {
            let header = &format!("{} - {}", order.customer.name, order.order.id);

            eframe::egui::Panel::right(Id::new("Task Audit Side Panel"))
                .default_size(280.)
                .max_size(900.)
                .resizable(true)
                .show_separator_line(true)
                .show_animated_inside(ui, self.services_viewer.selected.is_some(), |ui|
            {
                ui.vertical_centered_justified(|ui| {
                    ui.add_space(5.);
                    ui.heading(header.to_uppercase());
                    Separator::default().horizontal().shrink(ui.available_width()/2.5).ui(ui);
                    ui.add_space(5.0);
    
                    ScrollArea::vertical()
                    .auto_shrink(true)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Close ->").clicked() {
                                self.services_viewer.selected = None;
                            }

                            ui.add_space(10.);
                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                if ui.button(RichText::new("Create Task").code().color(ui.global_style().visuals.error_fg_color)).clicked() {
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
    
                        ui.group(|ui| {
                            Grid::new(order.order.id.clone())
                            .num_columns(2)
                            .min_col_width(ui.available_width()/2.1)
                            .max_col_width(425.)
                            .with_row_color(return_colors)
                            .show(ui, |ui| {
                                ui.colored_label(Color32::LIGHT_RED, " Status");
    
                                let order_type = OrderState::from_id_str(order.order.current_state.as_str());

                                ui.label(order_type);
                                ui.end_row();
    
                                ui.colored_label(Color32::LIGHT_RED, " Phone#");
                                let phone = order.customer.phone_number.clone();
                                ui.label(phone);
                                ui.end_row();

                                ui.colored_label(Color32::LIGHT_RED, " Email");
                                let phone = order.customer.email.clone();
                                ui.label(phone);
                                ui.end_row();

                                ui.colored_label(Color32::LIGHT_RED, " Sales Rep");
                                let sales_rep = order.sales_rep.unwrap_or_default();
                                ui.label(format!("{} {}", sales_rep.firstname, sales_rep.lastname));
                                ui.end_row();
    
                                ui.colored_label(Color32::LIGHT_RED, " Split Rep");
                                let split_rep = order.split_rep.unwrap_or_default();
                                ui.label(format!("{} {}", split_rep.firstname, split_rep.lastname));
                                ui.end_row();
    
                                // Parse the input into a NaiveDateTime
                                let naive_datetime = NaiveDateTime::parse_from_str(&order.order.date_add, "%Y-%m-%d %H:%M:%S").unwrap_or_default();
                                // Convert to a DateTime with Utc timezone
                                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);
                                // Format the DateTime into yyyy/mm/dd
                                let formatted_date = datetime.format(" %m/%d/%Y").to_string();
    
                                ui.colored_label(Color32::LIGHT_RED, " Check-in Date: ");
                                ui.label(formatted_date);
                                ui.end_row();
                                    
                                ui.colored_label(Color32::LIGHT_RED, " Missed Calls");
                                ui.label("");
                                ui.end_row();
    
                                let id = order.order.id.clone();
                                let missed_call = self
                                    .services_viewer
                                    .missed_calls
                                    .iter()
                                    .find(|o| *o.id == id);
    
                                if let Some(call) = missed_call {
    
                                    for missing_day in call.missing_days.iter() {
                                        ui.label(" -> ");
                                        ui.colored_label(Color32::RED, missing_day);
                                        ui.end_row();
                                    }
                                }
                            });
                        });
    
                        ui.add_space(5.0);
                    });
                });
    
                ui.with_layout(Layout::bottom_up(eframe::egui::Align::Min), |ui| {
                    self.services_viewer.chat_view.ui(ui);
                });
            });
        }
         
        eframe::egui::Panel::top("Task Audit Top Panel")
            .exact_size(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.services_viewer.filter)
                    .hint_text(" Search SO# / Customer / Device / Model / Notes / Date")
                    .ui(ui);

                ui.add_space(10.);
                let selected = &mut self.services_viewer.store_selection;

                let selected_text = Store::from_presta_store_id(&selected.to_string());
                
                ComboBox::new("TaskAudit Store Selection", "")
                .selected_text(selected_text.as_str())
                .show_ui(ui, |ui| {
                    ui.selectable_value(selected, Store::RIV.into_store_id() as u64, Store::RIV.as_str());
                    ui.selectable_value(selected, Store::LTN.into_store_id() as u64, Store::LTN.as_str());
                    ui.selectable_value(selected, Store::MUR.into_store_id() as u64, Store::MUR.as_str());
                    ui.selectable_value(selected, Store::ORE.into_store_id() as u64, Store::ORE.as_str());
                    ui.selectable_value(selected, Store::SAN.into_store_id() as u64, Store::SAN.as_str());
                });

                let selected_text = match &self.audit_selection {
                    TaskAudit::MyInRepair => " My In Repair ".to_string(),
                    TaskAudit::MyServices => " My Services ".to_string(),
                    TaskAudit::NeedsCallToday => " Calls Due Today ".to_string(),
                    TaskAudit::Status(OrderState::CheckinShelf) => " Check-in Shelf ".to_string(),
                    TaskAudit::Status(OrderState::InRepair) => " In Repair ".to_string(),
                    TaskAudit::Status(OrderState::DoneShelf) => " Done Shelf ".to_string(),
                    TaskAudit::Status(state) => format!(" {} ", state.as_str()),
                    TaskAudit::AllExcept { order_type, .. } => match order_type {
                        OrderType::SalesOrder => " All Sales ".to_string(),
                        _ => " All Service ".to_string(),
                    },
                };

                ComboBox::new("TaskAudit Type Selection", "")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.audit_selection, TaskAudit::MyInRepair, " My In Repair ");
                        ui.selectable_value(&mut self.audit_selection, TaskAudit::MyServices, " My Services ");
                        ui.selectable_value(&mut self.audit_selection, TaskAudit::NeedsCallToday, " Calls Due Today ");
                        ui.selectable_value(&mut self.audit_selection, TaskAudit::Status(OrderState::CheckinShelf), " Check-in Shelf ");
                        ui.selectable_value(&mut self.audit_selection, TaskAudit::Status(OrderState::InRepair), " In Repair ");
                        ui.selectable_value(&mut self.audit_selection, TaskAudit::Status(OrderState::DoneShelf), " Done Shelf ");

                        let is_service = matches!(&self.audit_selection, TaskAudit::AllExcept { order_type: OrderType::ServiceOrder, .. });
                        if ui.selectable_label(is_service, " All Service ").clicked() && !is_service {
                            self.audit_selection = TaskAudit::AllExcept { order_type: OrderType::ServiceOrder, excluded: Vec::new() };
                        }

                        let is_sales = matches!(&self.audit_selection, TaskAudit::AllExcept { order_type: OrderType::SalesOrder, .. });
                        if ui.selectable_label(is_sales, " All Sales ").clicked() && !is_sales {
                            self.audit_selection = TaskAudit::AllExcept { order_type: OrderType::SalesOrder, excluded: Vec::new() };
                        }
                    });

                if let TaskAudit::AllExcept { order_type, excluded } = &mut self.audit_selection {
                    let applicable = order_type.applicable_states();
                    let exclude_text = if excluded.is_empty() {
                        " Exclude ".to_string()
                    } else {
                        format!(" Exclude ({}) ", excluded.len())
                    };
                    ComboBox::new("TaskAudit Exclude Selection", "")
                        .selected_text(exclude_text)
                        .show_ui(ui, |ui| {
                            for state in applicable {
                                let mut is_excluded = excluded.contains(&state);
                                if ui.checkbox(&mut is_excluded, state.as_str()).changed() {
                                    if is_excluded {
                                        excluded.push(state);
                                    } else {
                                        excluded.retain(|s| s != &state);
                                    }
                                }
                            }
                        });
                }

                ui.add_space(10.);

                if let Some(time) = self.time.clone() {
                    if time.elapsed() > web_time::Duration::from_secs(5) {
                        self.loading = false;
                    }
                }

                if Button::new(" Load ").ui(ui).clicked() {
                    self.loading = true;
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let key = selected.cache_key();

                    let start_idx = self
                        .index
                        .entry(key.clone())
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&key) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx, self.missed_calls_tx.clone(), self.services_viewer.store_selection.to_string());
                }
                ui.add_space(10.);
                if Button::new(" Load +10 ").ui(ui).clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let key = selected.cache_key();

                    let start_idx = self
                        .index
                        .entry(key.clone())
                        .and_modify(|i| *i+=10)
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&key) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx, self.missed_calls_tx.clone(), self.services_viewer.store_selection.to_string());
                }
                
                ui.add_space(10.);

                if self.loading {
                    ui.ctx().request_repaint();
                    Spinner::new().color(ui.global_style().visuals.error_fg_color).ui(ui);
                }
            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            if let Some(table) = self.service_map.get_mut(&self.audit_selection.cache_key()) {
                // style.single_click_edit_mode = true;
                Renderer::new(table, &mut self.services_viewer)
                .with_style_modify(|s| {
                    s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                    s.single_click_edit_mode = true;
                    s.auto_shrink = [false, false].into();
                })
                .ui(ui);
            }
        });  
    }
}


fn return_colors(num: usize, _style: &Style) -> Option<Color32> {
    let mut _col = Color32::from_rgb(30, 30, 38);
    if num % 2 == 0 {
        _col = Color32::from_rgb(15, 15, 22);
    } else {
        _col = Color32::from_rgb(30, 30, 38);
    }
    Some(_col)
}