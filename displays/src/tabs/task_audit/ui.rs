use eframe::egui::{Button, CentralPanel, CollapsingHeader, ComboBox, Id, Layout, RichText, ScrollArea, Separator, SidePanel, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::User;
use egui_data_table::Renderer;
use crate::PlatformSpawner;
use crate::Spawner;
use log::info;


use super::row_viewer::TaskRowViewer;
use super::TaskAudit;
use super::TaskAuditViewer;


impl TaskAuditViewer {
    pub fn show(&mut self, ui: &mut Ui, current_user: Option<User>) {
        SidePanel::right(Id::new("Task Audit Side Panel"))
            .default_width(280.)
            .max_width(900.)
            .resizable(true)
            .show_separator_line(true)
            .show_inside(ui, |ui| 
        {
            let service = self.services_viewer.selected.clone();

            let header = if let Some(service) = &service {
                &format!("{} - {}", service.customer.name, service.order.id)
            } else { "Task Details" };

            if let Some(order) = self.services_viewer.selected.clone() {
                ui.vertical_centered_justified(|ui| {
                    let service = self.services_viewer.selected.clone();

                    let header = if let Some(service) = &service {
                        &format!("{} - {}", service.customer.name, service.order.id)
                    } else { "Task Details" };
                    ui.add_space(5.);
                    ui.heading(header.to_uppercase());
                    Separator::default().horizontal().shrink(ui.available_width()/2.5).ui(ui);
                    ui.add_space(5.0);


                    ScrollArea::vertical()
                    .auto_shrink(true)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
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

                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label("Status");

                            let status = match order.order.current_state.as_str() {
                                "30" => "In Repair",
                                "40" => "Done Shelf",
                                "4" => "Shipped",
                                "29" => "Check-in Shelf",
                                "239" => "Accepted by Odoo",
                                _ => ""
                            };

                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                ui.label(status);
                                ui.add_space(10.);
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label("Sales Rep");
                            let sales_rep = order.sales_rep.unwrap_or_default();
                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                ui.label(format!("{} {}", sales_rep.firstname, sales_rep.lastname));
                                ui.add_space(10.);
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label("Split Rep");
                            let split_rep = order.split_rep.unwrap_or_default();

                            ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                ui.label(format!("{} {}", split_rep.firstname, split_rep.lastname));
                                ui.add_space(10.);
                            });
                        });
                    });

                    // ui.add_space(ui.available_height());
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
            } else {
                ui.vertical_centered_justified(|ui| {

                    ui.add_space(5.);
                    ui.heading(header.to_uppercase());
                    Separator::default().horizontal().shrink(ui.available_width()/2.5).ui(ui);
                    ui.add_space(5.0);

                });
            }
        });

        TopBottomPanel::top("Task Audit Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.services_viewer.filter)
                    .hint_text(" Search for SO# / Customer")
                    .ui(ui);

                ui.add_space(10.);

                if Button::new(" Refresh ").ui(ui).clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let selection = selected.clone().as_str();

                    let start_idx = self
                        .index
                        .entry(selection.clone())
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&selection) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx);
                }
                ui.add_space(10.);
                if Button::new(" Load +10 ").ui(ui).clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let selection = selected.clone().as_str();

                    let start_idx = self
                        .index
                        .entry(selection.clone())
                        .and_modify(|i| *i+=10)
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&selection) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user.clone(), order_tx, svcs, start_idx);
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
            ui.horizontal(|ui| {
                ui.add_space(10.);

                let selected_text = self.audit_selection.to_owned().as_str();
                let selected = &mut self.audit_selection;
                let current_selection = selected.clone();

                ComboBox::new("Store_Selection", "")
                    .selected_text(selected_text.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(selected, TaskAudit::AllServices, " All Services ");
                        ui.selectable_value(selected, TaskAudit::CheckinShelf, " Check-in Shelf ");
                        ui.selectable_value(selected, TaskAudit::MyInRepair, " My In Repair ");
                        ui.selectable_value(selected, TaskAudit::InRepair, " In Repair ");
                        ui.selectable_value(selected, TaskAudit::DoneShelf, " Done Shelf ");
                        ui.selectable_value(selected, TaskAudit::MyServices, " My Services ");
                    })
                    .response;

                if current_selection != *selected {
                    self.loading = true;
                    let order_tx = self.order_channel.0.clone();
                    let selection = selected.clone().as_str();
                    let start_idx = self.index.entry(selection.clone()).or_insert(0).clone();
                    let svcs = if let Some(k) = self.service_map.get_mut(&selection) {
                        k.iter().map(|k| k.order.id.clone()).collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    info!("Services from cache: {:?}", svcs.clone());
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(selected.clone(), current_user, order_tx, svcs, start_idx);
                }
            
                if self.loading {
                    ui.ctx().request_repaint();
                    ui.add_space(10.);
                    Spinner::new().color(ui.style().visuals.error_fg_color).ui(ui);
                }
                
                if let Some(time) = self.time.clone() {
                    if time.elapsed() > web_time::Duration::from_secs(5) {
                        self.loading = false;
                    }
                }
            });
            ui.add_space(5.);

            if let Some(table) = self.service_map.get_mut(&self.audit_selection.clone().as_str()) {
                let mut style = egui_data_table::Style::default();
                style.single_click_edit_mode = true;
                Renderer::new(table, &mut self.services_viewer).with_style(style).ui(ui);
            }
        });  
    }

    

}