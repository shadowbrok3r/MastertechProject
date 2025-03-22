use eframe::egui::{popup_below_widget, text::LayoutJob, Align, Button, Color32, ComboBox, FontFamily, FontId, Frame, Layout, Margin, PopupCloseBehavior, RichText, ScrollArea, Spinner, Stroke, Style, TextEdit, TextFormat, Ui, Vec2, Widget, WidgetText};
use crate::{FilterClients, SortDirection, Sortable, tasks::task_layout::SortField};
use database::schema::ConnectedClient;
use super::ClientUiAction;
use egui_extras::{Size, Strip, StripBuilder};
use crossbeam::channel::Sender;
use std::collections::HashMap;
use std::collections::BTreeSet;
use chrono::{DateTime, Local};
use std::borrow::BorrowMut;
use std::sync::Arc;
use log::info;
use core::f32;

use super::{AdminConsole, WebConsolePageState};

impl AdminConsole {
    pub fn layout_cols(&mut self, ui: &mut Ui) {
        
        ui.style_mut().visuals.window_corner_radius = ui.style().visuals.window_corner_radius;
        let style = ui.style().clone();
        // Extract connected and disconnected clients using pattern matching
        // let connected_clients = client_map.get("Connected").cloned().unwrap_or_default();
        // let disconnected_clients = client_map.get("Disconnected").cloned().unwrap_or_default();

        match self.state {
            WebConsolePageState::ConnectedClients => {
                let column_width = Size::exact(ui.available_width());
                let x: f32 = ui.available_height() / 1.1;

                ScrollArea::horizontal()
                    .show_viewport(ui, |ui, _|
                {
                    // let mut connected_clients = Vec::new();
                    // let mut disconnected_clients = Vec::new();
                    self.client_map.retain(|name, _| name == "Connected");

                    StripBuilder::new(ui)
                        .cell_layout(Layout::top_down_justified(Align::Center))
                        .size(Size::exact(30.0))
                        .size(Size::exact(5.0))
                        .size(Size::exact(x))
                        .vertical(|mut strip| 
                    {
        
                        strip.strip(|strip| 
                        {
                            strip
                                .sizes(column_width, self.client_map.keys().len())
                                .horizontal(|strip| self.headers(strip, style.clone()));
                        });
        
                        strip.empty();
        
                        strip.strip(|strip| 
                        {
                            strip
                                .sizes(column_width, self.client_map.keys().len())
                                .horizontal( |mut strip| self.columns(strip.borrow_mut(), style.clone()));
                        });
                    });
                });
            },
            WebConsolePageState::DisconnectedClients => {
                let column_width = Size::exact(ui.available_width());
                let x: f32 = ui.available_height() / 1.1;

                ScrollArea::horizontal()
                    .show_viewport(ui, |ui, _|
                {
                    // let mut connected_clients = Vec::new();
                    // let mut disconnected_clients = Vec::new();
                    self.client_map.retain(|name, _| name == "Disconnected");

                    StripBuilder::new(ui)
                        .cell_layout(Layout::top_down_justified(Align::Center))
                        .size(Size::exact(30.0))
                        .size(Size::exact(5.0))
                        .size(Size::exact(x))
                        .vertical(|mut strip| 
                    {
        
                        strip.strip(|strip| 
                        {
                            strip
                                .sizes(column_width, self.client_map.keys().len())
                                .horizontal(|strip| self.headers(strip, style.clone()));
                        });
        
                        strip.empty();
        
                        strip.strip(|strip| 
                        {
                            strip
                                .sizes(column_width, self.client_map.keys().len())
                                .horizontal( |mut strip| self.columns(strip.borrow_mut(), style.clone()));
                        });
                    });
                });
            },
            WebConsolePageState::ScriptEditor => self.script_editor.ui(ui),
            WebConsolePageState::AllClients => {
                let column_width = Size::exact(ui.available_width()/2.0);
                let x: f32 = ui.available_height() / 1.1;
                ScrollArea::horizontal()
                    .show_viewport(ui, |ui, _|
                {
                    StripBuilder::new(ui)
                        .cell_layout(Layout::top_down_justified(Align::Center))
                        .size(Size::exact(30.0))
                        .size(Size::exact(5.0))
                        .size(Size::exact(x))
                        .vertical(|mut strip| 
                    {
        
                        strip.strip(|strip| 
                        {
                            strip
                                .sizes(column_width, self.client_map.keys().len())
                                .horizontal(|strip| self.headers(strip, style.clone()));
                        });
        
                        strip.empty();
        
                        strip.strip(|strip| 
                        {
                            strip
                                .sizes(column_width, self.client_map.keys().len())
                                .horizontal( |mut strip| self.columns(strip.borrow_mut(), style.clone()));
                        });
                    });
                });
            }
        };
    }

    fn headers(&mut self, mut s: Strip, style: Arc<Style>){
        let header_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4))
            .outer_margin(Margin::symmetric(8, 1))
            .corner_radius(style.visuals.window_corner_radius)
            .stroke(style.visuals.window_stroke);

        let mut idx = 0;
        for (name, _clients) in self.client_map.iter() {
            idx += 1;
            s.cell(|ui|{
                if ui.available_width() < 10.0 || ui.available_height() < 10.0 {
                    info!("First strip avail size: {:?}", ui.available_size());
                }
                header_frame.show(ui, |ui|
                {
                    ui.horizontal_top(|ui| 
                    {
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| 
                        {
                            let search_input = self.search_inputs.entry(name.clone()).or_insert_with(String::new);
                            let mut margin = Margin::default();
                            margin.top = 6;
                            margin.left = 4;
                            
                            TextEdit::singleline(search_input).hint_text("Search").desired_width(100.0).margin(margin).ui(ui);

                            ui.add_space(15.);

                            let response = Button::new(RichText::new(name.to_owned())
                                    .color(style.visuals.warn_fg_color)
                                    .size(13.0).monospace()
                                )
                                .fill(style.visuals.noninteractive().bg_fill)
                                .corner_radius(eframe::egui::CornerRadius::same(2))
                                .min_size(Vec2::new(60.0, 15.0))
                                .ui(ui);

                            if response.clicked(){
                                ui.memory_mut(|mem| mem.open_popup(format!("sub_menu-{:?}",name).into()));
                            }
                            
                            popup_below_widget(
                                ui, 
                                format!("sub_menu-{:?}",name).into(), 
                                &response, 
                                PopupCloseBehavior::CloseOnClickOutside, 
                                |_ui| 
                            {

                            });
                        });
                        
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| 
                        {
                            
                            let selected = self.sort_by.entry(name.clone()).or_default();
                            let txt = match selected.direction {
                                SortDirection::Asc => ("↗", ui.style().visuals.warn_fg_color),
                                SortDirection::Desc => ("↘", ui.style().visuals.error_fg_color),
                            };
                            let selected_text = match selected.field {
                                SortField::Default => RichText::new(format!("Default {}", txt.0)).color(txt.1).small(),
                                SortField::Date => RichText::new(format!("Date {}", txt.0)).color(txt.1).small(),
                                SortField::Name => RichText::new(format!("Name {}", txt.0)).color(txt.1).small(),
                            };
                            ComboBox::new(format!("SortBy for {name:?}-{idx}"), "")
                                .selected_text(selected_text)
                                .width(70.)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_value(
                                        &mut selected.field, 
                                        SortField::Default, 
                                        RichText::new(format!("Default {}", txt.0)).color(txt.1).small())
                                    .clicked() {
                                        if let Some(last_field) = self.last_sort_field.clone() {
                                            if last_field == SortField::Default {
                                                // Toggle the direction if the same field is clicked again
                                                selected.direction = match selected.direction {
                                                    SortDirection::Asc => SortDirection::Desc,
                                                    SortDirection::Desc => SortDirection::Asc,
                                                };
                                            }
                                        }
                                        // Update the last selected field
                                        self.last_sort_field = Some(SortField::Default);
                                    }
                                    if ui.selectable_value(
                                        &mut selected.field, 
                                        SortField::Name, 
                                        RichText::new(format!("Name {}", txt.0)).color(txt.1).small())
                                    .clicked() {
                                        if let Some(last_field) = self.last_sort_field.clone() {
                                            if last_field == SortField::Name {
                                                // Toggle the direction if the same field is clicked again
                                                selected.direction = match selected.direction {
                                                    SortDirection::Asc => SortDirection::Desc,
                                                    SortDirection::Desc => SortDirection::Asc,
                                                };
                                            }
                                        }
                                        // Update the last selected field
                                        self.last_sort_field = Some(SortField::Name);
                                    }
                                    if ui.selectable_value(
                                        &mut selected.field, 
                                        SortField::Date, 
                                        RichText::new(format!("Date {}", txt.0)).color(txt.1).small())
                                    .clicked() {
                                        if let Some(last_field) = self.last_sort_field.clone() {
                                            if last_field == SortField::Date {
                                                // Toggle the direction if the same field is clicked again
                                                selected.direction = match selected.direction {
                                                    SortDirection::Asc => SortDirection::Desc,
                                                    SortDirection::Desc => SortDirection::Asc,
                                                };
                                            }
                                        }
                                        // Update the last selected field
                                        self.last_sort_field = Some(SortField::Date);
                                    }
                            });
                        });
                    });
                });
            });
        }
    }

    fn columns(&mut self, s: &mut Strip, style: Arc<Style>) {
        let column_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(6))
            .corner_radius(style.visuals.menu_corner_radius)
            .stroke(style.visuals.window_stroke);

        let mut inputs = BTreeSet::new();
        
        for (name, clients) in self.client_map.iter_mut(){
            
            let sort_by = self.sort_by.entry(name.clone()).or_default();
            let direction = &sort_by.direction;
            match sort_by.field {
                SortField::Default => clients.default_sort(direction.clone()),
                SortField::Date => clients.sort_by_date(direction.clone()),
                SortField::Name => clients.sort_by_name(direction.clone()),
            };
            
            for client in clients.iter(){
                inputs.insert(client.connection_string.clone());
                inputs.insert(client.friendly_name.as_ref().cloned().unwrap_or_default());
                // inputs.insert(client.client_hash.clone());
            }

            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        // let row_height = if self.ws_clients.get(&client.).is_some() {
                        //     50.
                        // } else {
                        //     400.
                        // };
                        let row_height = 35.;
                        let total_rows = clients.len(); 
                        let scroll_area = ScrollArea::vertical().auto_shrink(false);
                        ui.ctx().options_mut(|o| o.line_scroll_speed = 30.0);
                        scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                            // Retrieve search input for the current context, or default to an empty string.
                            let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();

                            // Iterate only over the rows in the current viewport range.
                            for row in row_range {
                                if !search_input.is_empty() {
                                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                                }
                                let mut filtered_clients = clients.filter_by_client(inputs.clone(), search_input.clone());

                                if let Some(client) = filtered_clients.get_mut(row) {
                                    let connection_string = client.connection_string.clone();
                                    self.undock_client.entry(connection_string.clone()).or_insert(false);
                                    let color = if client.connected{ Color32::LIGHT_BLUE } else { Color32::LIGHT_RED };
                            
                                    let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 14))
                                        .inner_margin(Margin::same(4)).outer_margin(Margin::symmetric(5, 3))
                                        .corner_radius(eframe::egui::CornerRadius::same(10)).stroke(Stroke::new(0.5, color));
                            
                                    let undock = if let Some(undock) = self.undock_client.get(&connection_string){
                                        undock
                                    } else { &false };
                                    
                                    if !*undock {
                                        column_frame.show(ui, |ui| {
                                            // ui.set_min_size(Vec2::new(400., 400.));
                                            ui.vertical_centered_justified(|ui| {
                                                let tx = self.ui_actions_channel.0.clone();
                                                ui.horizontal(|ui| Self::client_header(ui, tx, client, self.undock_client.clone()));
                                                if let Some(ws_client) = self.ws_clients.get_mut(&connection_string) {
                                                    ws_client.show(ui);
                                                }
                                            });
                                        });
                                    }
                                }
                            }
                            if self.loading {
                                ui.vertical_centered(|ui| {
                                    ui.label("Loading..");
                                    Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui)
                                });
                            }                   
                        });
                    });
                });
            });
        }
        // }
    }

    pub fn client_header(ui: &mut Ui, tx: Sender<ClientUiAction>, client: &ConnectedClient, undock_client: HashMap<String, bool>) {
        let style = ui.style().clone();
        Frame::default()
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4))
            .outer_margin(Margin::symmetric(3, 0))
            .corner_radius(eframe::egui::CornerRadius::same(5))
            .stroke(style.visuals.window_stroke)
            .show(ui, |ui| 
        {
            // let ui = &mut header_frame.content_ui;
            ui.set_height(25.);
            ui.horizontal_top(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // Create a new LayoutJob
                    let mut job = LayoutJob::default();

                    if let Some(friendly_name) = client.clone().friendly_name {
                        job.append(
                            &friendly_name,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(12.0, FontFamily::Proportional),
                                color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
                                valign: Align::Min,
                                ..Default::default()
                            },
                        );
                    } else {
                        let conn_string = &client.connection_string;
                        let txt = conn_string.split_once(':');
                        if let Some(txt) = txt {
                            let text = format!("{}:", txt.0);
                            job.append(
                                &text,
                                0.0,
                                TextFormat {
                                    font_id: FontId::new(12.0, FontFamily::Proportional),
                                    color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
                                    valign: Align::Min,
                                    ..Default::default()
                                },
                            );
                            job.append(
                                txt.1,
                                0.0,
                                TextFormat {
                                    font_id: FontId::new(12.0, FontFamily::Proportional),
                                    color: Color32::from_rgb(199, 202, 245),
                                    valign: Align::Min,
                                    ..Default::default()
                                },
                            );
                        }
                    };

                    // Convert LayoutJob to WidgetText
                    let formatted_text = WidgetText::from(job);
                    let parsed_date = DateTime::parse_from_rfc3339(
                        &client.last_update.as_ref().cloned().unwrap_or_default()
                    )
                    .unwrap_or_default()
                    .with_timezone(&Local);
            
                    let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                    let _ = Button::new(formatted_text).ui(ui).on_hover_text(formatted_date);
                    // if ui.button(formatted_text).clicked() {};
                });


                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let button = Button::new(RichText::new("⮫").color(Color32::LIGHT_RED))
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30.0, 25.))
                        .ui(ui);

                    if button.clicked() {
                        info!("Sent Connection Command");
                        let _ = tx.try_send(ClientUiAction::ConnectClient(client.clone()));
                    }

                    let txt = if let Some(docked) =
                        undock_client.get(client.connection_string.as_str())
                    {
                        if !*docked { "🔓" } 
                        else { "🔒" }
                    } 
                    else { "🔒" };

                    let undock = Button::new(RichText::new(txt).color(Color32::LIGHT_RED))
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30., 25.))
                        .ui(ui);

                    if undock.clicked() {
                        let _ = tx.try_send(ClientUiAction::UndockClient(client.connection_string.clone()));
                    }

                    let button = Button::new(RichText::new("✖").color(Color32::LIGHT_RED))
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30., 25.))
                        .ui(ui);

                    if button.clicked() {
                        let _ = tx.try_send(ClientUiAction::DeleteClient(client.clone()));
                    }
                    // ui.add_space(10.0);

                    // let export =
                    //     Button::new(RichText::new("Export").size(10.0).color(Color32::LIGHT_RED))
                    //         .fill(Color32::TRANSPARENT)
                    //         .min_size(Vec2::new(30.0, ui.available_height()))
                    //         .ui(ui);

                    // if export.clicked() {
                    //     let _ = tx.try_send(ClientUiAction::ExportHistory(client.clone()));
                    // }
                    
                });
            });
        });

        // let response = header_frame.allocate_space(ui);
        // if response.hovered() {
        //     header_frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
        //     header_frame.frame.shadow = style.visuals.window_shadow;
        // } else {
        //     header_frame.frame.stroke = style.visuals.widgets.open.bg_stroke;
        // }
        // header_frame.
        // header_frame.paint(ui);

    }

    pub fn ui(&mut self, ui: &mut Ui) {
        match self.state {
            WebConsolePageState::ScriptEditor => self.script_editor.ui(ui),
            _ => {
                for client in self.clients.iter() {
                    if let Some(ws_client) = self.ws_clients.get_mut(&client.connection_string) {
                        ws_client.show(ui);
                    }
                }
            }
        }
    }

}