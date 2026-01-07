use chrono::{DateTime, NaiveDateTime, Utc};
use eframe::egui::{Color32, Grid, Style};
use eframe::egui::{Button, CentralPanel, CollapsingHeader, ComboBox, Id, Layout, RichText, ScrollArea, Separator, SidePanel, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::{Store, User};
use egui_data_table::Renderer;
use crate::PlatformSpawner;
use crate::Spawner;
use log::info;

use super::row_viewer::TaskRowViewer;
use super::TaskAudit;
use super::TaskAuditViewer;

impl TaskAuditViewer {
    pub fn show(&mut self, ui: &mut Ui, current_user: Option<User>) {
        if let Some(order) = self.services_viewer.selected.clone() {
            let header = &format!("{} - {}", order.customer.name, order.order.id);

            SidePanel::right(Id::new("Task Audit Side Panel"))
                .default_width(280.)
                .max_width(900.)
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
                                if ui.button(RichText::new("Create Task").code().color(ui.style().visuals.error_fg_color)).clicked() {
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
    
                                let status = match order.order.current_state.as_str() {
                                    "30" => "In Repair",
                                    "40" => "Done Shelf",
                                    "4" => "Shipped",
                                    "29" => "Check-in Shelf",
                                    "239" => "Accepted by Odoo",
                                    _ => ""
                                };
    
                                ui.label(status);
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
                    CollapsingHeader::new(
                        RichText::new("Order Notes")
                            .color(ui.style().visuals.error_fg_color)
                            .monospace()
                        )
                        .default_open(true)
                        .id_salt(format!("Order Notes - {}", header))
                        .show_unindented(ui, |ui| 
                    {
                        self.services_viewer.chat_view.ui(ui);
                    });
                });
            });
        }
         
        TopBottomPanel::top("Task Audit Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.services_viewer.filter)
                    .hint_text(" Search for SO# / Customer")
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
                    ui.selectable_value(selected, Store::WJ.into_store_id() as u64, Store::WJ.as_str());
                    ui.selectable_value(selected, Store::ORE.into_store_id() as u64, Store::ORE.as_str());
                    ui.selectable_value(selected, Store::SAN.into_store_id() as u64, Store::SAN.as_str());
                });

                let selected_text = self.audit_selection.as_str().to_string();
                let selected = &mut self.audit_selection;
                let current_selection = selected.clone();

                ComboBox::new("TaskAudit Type Selection", "")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(selected, TaskAudit::MyInRepair, " My In Repair ");
                        ui.selectable_value(selected, TaskAudit::NeedsCall, " Missed Calls ");
                        ui.selectable_value(selected, TaskAudit::CheckinShelf, " Check-in Shelf ");
                        ui.selectable_value(selected, TaskAudit::InRepair, " In Repair ");
                        ui.selectable_value(selected, TaskAudit::DoneShelf, " Done Shelf ");
                        ui.selectable_value(selected, TaskAudit::AllServices, " All Services ");
                    })
                    .response;

                ui.add_space(10.);
                
                if current_selection != *selected {
                    self.loading = true;
                    let order_tx = self.order_channel.0.clone();
                    let selection = selected.as_str();
                    let start_idx = self.index.entry(selection.to_string()).or_insert(0).clone();
                    let svcs = if let Some(k) = self.service_map.get_mut(&selection.to_string()) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    info!("Services from cache: {:?}", svcs.clone());
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx, self.missed_calls_tx.clone(), self.services_viewer.store_selection.to_string());
                }
                
                if let Some(time) = self.time.clone() {
                    if time.elapsed() > web_time::Duration::from_secs(5) {
                        self.loading = false;
                    }
                }

                if Button::new(" Refresh ").ui(ui).clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let selection = selected.as_str();

                    let start_idx = self
                        .index
                        .entry(selection.to_string())
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&selection.to_string()) {
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
                    let selection = selected.as_str();

                    let start_idx = self
                        .index
                        .entry(selection.to_string())
                        .and_modify(|i| *i+=10)
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&selection.to_string()) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx, self.missed_calls_tx.clone(), self.services_viewer.store_selection.to_string());
                }
                ui.add_space(10.);
                let label = if self.services_viewer.open_hotkeys {
                    " Hide Hotkeys "
                } else {
                    " Show Hotkeys "
                };

                if Button::new(label).ui(ui).clicked() {
                    self.services_viewer.open_hotkeys = !self.services_viewer.open_hotkeys;
                }
                
                ui.add_space(10.);

                if self.loading {
                    ui.ctx().request_repaint();
                    Spinner::new().color(ui.style().visuals.error_fg_color).ui(ui);
                }
            });
        });

        TopBottomPanel::bottom(Id::new("Task Audit Hot Keys"))
            .max_height(240.)
            .show_animated_inside(ui, self.services_viewer.open_hotkeys, |ui| 
        {
            ui.vertical_centered(|ui| ui.heading("Hotkeys"));
            ui.vertical_centered_justified(|ui| ui.separator());

            ui.horizontal_wrapped(|ui| {
                ui.style_mut().spacing.item_spacing.y = 5.0;
                ui.add_space(2.);
                let mut count = 0;
                for (k, a) in &self.services_viewer.hotkeys {
                    Button::new(format!("{a:?}"))
                        .min_size(Vec2::new(280., 25.))
                        .shortcut_text(
                            RichText::new(ui.ctx().format_shortcut(k))
                            .code()
                            .color(ui.style().visuals.warn_fg_color)
                        )
                        .ui(ui);
                    
                    count += 1;
                    if count % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            if let Some(table) = self.service_map.get_mut(&self.audit_selection.as_str().to_string()) {
                // style.single_click_edit_mode = true;
                Renderer::new(table, &mut self.services_viewer)
                    .with_style(egui_data_table::Style::default())
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