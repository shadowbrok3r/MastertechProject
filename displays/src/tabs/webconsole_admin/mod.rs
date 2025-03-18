use eframe::egui::{popup_below_widget, text::LayoutJob, Align, Button, CentralPanel, Color32, ComboBox, Context, FontFamily, FontId, Frame, Layout, Margin, PopupCloseBehavior, RichText, ScrollArea, SidePanel, Spinner, Stroke, Style, TextEdit, TextFormat, TopBottomPanel, Ui, Vec2, Widget, WidgetText};
use crate::{channel_manager::ChannelManager, remote_viewer::ratagui::DiffMerge, tasks::task_layout::{SortField, SortOptions}, ui_tools::toasts::{Toast, ToastOptions}, virtual_filesystem::FileSystem, FilterClients, PlatformSpawner, SortDirection, Sortable, Spawner};
use database::{schema::{utilities::{decompress_data, get_connected_clients}, ConnectedClient}, WS_MASTER_URL};
use ratatui::{buffer::Buffer, layout::{Position, Rect}, style::{Color, Style as TermStyle}};
use websockets::{WebSocketClient, ClientHandler};
use base64::{engine::general_purpose, Engine};
use egui_extras::{Size, Strip, StripBuilder};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap};
use crate::app_state::SharedContext;
use std::collections::BTreeSet;
use chrono::{DateTime, Local};
use std::borrow::BorrowMut;
use serde::Serialize;
use std::sync::Arc;
use log::info;
use core::f32;

use super::script_editor::ScriptEditor;

pub mod websockets;

/// Decompress and decode the given Vec<u8> (which is base64-encoded compressed JSON)
/// and deserialize it back into a Buffer.
pub fn decompress_buffer(input: Vec<u8>) -> anyhow::Result<Buffer, anyhow::Error> {
    // Convert the input Vec<u8> into a String.
    let encoded_str = String::from_utf8(input)?;
    // Base64-decode into the compressed data.
    let compressed = general_purpose::STANDARD.decode(&encoded_str)?;
    // Decompress the data.
    let decompressed = decompress_data(&compressed)?;
    // Convert decompressed bytes into a string.
    let decompressed_string = String::from_utf8(decompressed)?;
    // Deserialize the JSON string into a Buffer.
    let buf = serde_json::from_str::<Buffer>(&decompressed_string)?;
    Ok(buf)
}

// Helper function to resize a buffer
fn resize_buffer(source: &Buffer, target_area: Rect) -> Buffer {
    let mut new_buffer = Buffer::empty(target_area);

    // Copy content from source to new buffer, respecting bounds
    for y in 0..source.area.height.min(target_area.height) {
        for x in 0..source.area.width.min(target_area.width) {
            if let Some(source_cell) = source.cell((x, y)) {
                if let Some(target_cell) = new_buffer.cell_mut(Position::new(x, y)) {
                    target_cell.clone_from(source_cell);
                }
            }
        }
    }

    new_buffer
}

impl SharedContext {
    pub fn egui_terminal(&mut self, ui: &mut Ui) {
        // Handle incoming WebSocket events
        while let Some(ws_event) = self.ws_receiver.try_recv() {
            match ws_event {
                ewebsock::WsEvent::Message(ws_message) => {
                    if let ewebsock::WsMessage::Binary(buffer_array) = ws_message {
                        let buffer_tx = self.buffer_tx.clone(); // Clone sender for the task

                        // Spawn a task to process the buffer
                        PlatformSpawner::spawn(async move {
                            if let Ok(new_buffer) = decompress_buffer(buffer_array) {
                                // Send the processed buffer back to the main thread
                                if buffer_tx.try_send(new_buffer).is_err() {
                                    log::warn!("Failed to send processed buffer to main thread");
                                }
                            }
                        });
                    }
                }
                _ => {}
            }
        }

        // Get the available size from the CentralPanel
        let available_size = ui.available_size();
        let target_width = available_size.x as u16;
        let target_height = available_size.y as u16;
        let target_area = Rect::new(0, 0, target_width, target_height);

        // Process received buffers from the spawned task
        while let Ok(new_buffer) = self.buffer_rx.try_recv() {
            let resized_buffer = if new_buffer.area != target_area {
                resize_buffer(&new_buffer, target_area)
            } else {
                new_buffer
            };

            // let should_update = if let Some(ref cached) = self.cached_buffer {
            //     !cached.diff(&resized_buffer).is_empty()
            // } else {
            //     true
            // };

            // if should_update {
                log::info!("Should update");
                log::info!("target_area: {target_area:?}");
                log::info!("resized_buffer.area: {:?}", resized_buffer.area);

                self.terminal
                    .draw(|f| {
                        let available_area = f.area();
                        if available_area != resized_buffer.area {
                            f.buffer_mut().resize(resized_buffer.area);
                        }
                        // f.buffer_mut().set_style(resized_buffer.area, TermStyle::default().bg(Color::Rgb(8, 8, 12)));
                        f.buffer_mut().merge(&resized_buffer); // .diff_merge(&resized_buffer);
                    })
                    .expect("Failed to draw terminal frame");

                self.cached_buffer = Some(resized_buffer);
                ui.ctx().request_repaint();
            // }
        }

        // Render the terminal in egui
        eframe::egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add(self.terminal.backend_mut());
        });
    }
}



pub enum ClientUiAction {
    UndockClient(String),
    DeleteClient(ConnectedClient),
    ConnectClient(ConnectedClient),
    ExportHistory(ConnectedClient)
}

#[derive(Serialize, Default)]
pub enum WebConsolePageState {
    #[default]
    ConnectedClients,
    DisconnectedClients,
    ScriptEditor,
    AllClients
}

#[derive(Serialize)]
pub struct WebConsoleLayout {
    pub client_map: BTreeMap<String, Vec<ConnectedClient>>,
    pub clients: Vec<ConnectedClient>,
    pub search_inputs: HashMap<String, String>,
    // pub connected_clients: Vec<ConnectedClient>,
    // pub disconnected_clients: Vec<ConnectedClient>,
    open_menu: bool,
    #[serde(skip)]
    pub ui_actions_channel: (Sender<ClientUiAction>, Receiver<ClientUiAction>),
    state: WebConsolePageState,
    pub sort_by: HashMap<String, SortOptions>,
    pub last_sort_field: Option<SortField>,    
    pub loading: bool,
    /// tracking for which client we want to undock
    /// into a floating UI when we click the undock button
    pub undock_client: HashMap<String, bool>,
    /// The undock button was clicked for a ConnectedClient
    pub wants_to_undock: bool,
    #[serde(skip)]
    pub filesystem: FileSystem,
    #[serde(skip)]
    pub ws_clients: HashMap<String, WebSocketClient>,
    pub error: String,
    script_editor: ScriptEditor
}

impl WebConsoleLayout {
    pub fn new(client_map: BTreeMap<String, Vec<ConnectedClient>>, clients: Vec<ConnectedClient>) -> Self {
        let ui_actions_channel = ClientUiAction::create_unbounded_channel();
        // let mut connected_clients = Vec::new();
        // let mut disconnected_clients = Vec::new();
        // client_map.iter().filter(|(name, list)| {
        //     match name {
        //         "Connected" => {
        //             connected_clients = list;
        //         },
        //         "Disconnected" => {
        //             disconnected_clients = list;
        //         }
        //     }
        // });

        Self {  
            // connected_clients,
            // disconnected_clients,
            clients,
            client_map,
            search_inputs: Default::default(), 
            open_menu: true,
            sort_by: Default::default(),
            last_sort_field: Default::default(),
            loading: false,
            undock_client: Default::default(),
            wants_to_undock: false,
            filesystem: FileSystem::new(),
            ws_clients: Default::default(),
            ui_actions_channel,
            error: Default::default(),
            state: Default::default(),
            script_editor: ScriptEditor::new()
        }
    }

    pub fn set_filesystem(&mut self, filesystem: FileSystem) -> &mut Self {
        self.filesystem = filesystem.clone();
        self.script_editor.set_filesystem(filesystem);
        self
    }

    pub fn receive(&mut self, ctx: &Context) {
        self.filesystem.receive(ctx);
        if let Ok(action) = self.ui_actions_channel.1.try_recv() {
            match action {
                ClientUiAction::UndockClient(connection_string) => {
                    if let Some(docked) = self.undock_client.get_mut(&connection_string)
                    {
                        if *docked {
                            *docked = false;
                            self.wants_to_undock = false;
                        } else {
                            *docked = true;
                            self.wants_to_undock = true;
                        };
                    }
                },
                ClientUiAction::DeleteClient(mut client) => {
                    // CONNECT
                    let _url = format!(
                        "{WS_MASTER_URL}&room_id={}",
                        client.connection_string.clone()
                    );
                    client.connected = false;
                    client.delete_client();
                    if let Some(ws_client) = self.ws_clients.get_mut(&client.connection_string)
                    {
                        ws_client.ws_sender.close();
                    }
                    self.error = format!("WebConsole -> Client {} Deleted", client.connection_string.clone());
                },
                ClientUiAction::ConnectClient(mut client) => {
                    info!("Received Connection Command");
                    let url = format!(
                        "{WS_MASTER_URL}&room_id={}",
                        client.connection_string.clone()
                    );
                    match ewebsock::connect(&url, Default::default()) {
                        Ok((ws_sender, ws_receiver)) => {
                            client.connected = true;

                            let ws_client = WebSocketClient::new(
                                ws_sender,
                                ws_receiver,
                                client.clone(),
                                self.filesystem.clone(),
                            );
                            
                            self.ws_clients
                                .entry(client.connection_string.clone())
                                .or_insert(ws_client);

                            self.error = format!("WebConsole -> Connected to server");
                        }
                        Err(error) => {
                            client.connected = false;
                            info!("Failed to connect to {:?}: {}", &url, error.clone());
                            self.error = format!("WebConsole Error -> {error}");
                        }
                    };
                },
                ClientUiAction::ExportHistory(mut client) => {
                    if let Some(ws_client) = self.ws_clients.get(&client.connection_string) {
                        client.export_logs(ws_client.history.clone());
                    }
                },
            }
            
            ctx.request_repaint();
        }
    }

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

    fn ui(&mut self, ui: &mut Ui) {
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


impl SharedContext {
    pub fn admin_console(&mut self, ui: &mut Ui){
        self.web_console_layout.receive(ui.ctx());

        let top_panel_frame = Frame::default()
            .inner_margin(Margin::same(3))
            .outer_margin(Margin::same(0))
            .fill(Color32::from_rgb(17,17,19))
            .stroke(Stroke::new(0.7, Color32::from_additive_luminance(150)))
            .corner_radius(eframe::egui::CornerRadius::same(5)) ;

        let side_panel_frame = Frame::default()
            .inner_margin(Margin::same(3))
            .outer_margin(Margin::same(0))
            .fill(ui.style().visuals.extreme_bg_color)
            .stroke(Stroke::new(0.7, Color32::from_additive_luminance(150)))
            .corner_radius(eframe::egui::CornerRadius::same(5)) ;

        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 4.0);

        TopBottomPanel::top("Client_Top_panel")
            .frame(top_panel_frame)
            .show_separator_line(false)
            .exact_height(35.)
            .show_inside(ui, |ui |
        {
            ui.with_layout(Layout::left_to_right(Align::Center),|ui | { 
                ui.set_height(15.);

                let txt = match self.web_console_layout.open_menu {
                    false => "Show Clients ->",
                    true => "<- Hide Clients",
                };

                if ui.button(txt).clicked() {
                    self.web_console_layout.open_menu = !self.web_console_layout.open_menu;
                }

                ui.add_space(ui.available_width()/3.1);
                let button_size = Vec2::new(50.0, 15.0);
                if Button::new("All Clients")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() 
                {
                    self.refresh_client_list();
                    self.web_console_layout.state = WebConsolePageState::AllClients;
                }
                ui.add_space(5.);
                if Button::new("Connected Clients")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked()
                {
                    self.refresh_client_list();
                    self.web_console_layout.state = WebConsolePageState::ConnectedClients;
                }
                ui.add_space(5.);
                if Button::new("Disconnected Clients")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() 
                {
                    self.refresh_client_list();
                    self.web_console_layout.state = WebConsolePageState::DisconnectedClients;
                }
                ui.add_space(5.);
                if Button::new("Script Editor")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() 
                {
                    self.web_console_layout.state = WebConsolePageState::ScriptEditor;
                }
            });
        });

        SidePanel::left("Client_Side_panel")
            .frame(side_panel_frame)
            .show_separator_line(false)
            .min_width(400.)
            .max_width(500.)
            .show_animated_inside(ui, self.web_console_layout.open_menu, |ui |
        {
            // ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            //     if Button::new("Refresh")
            //         .min_size(Vec2::new(50.0, 15.0))
            //         .ui(ui)
            //         .clicked() 
            //     {
            //             self.refresh_client_list();
            //     }
            // });
            ui.vertical_centered(|ui| {

                let ws_client = &mut self.web_console_layout;
                let clients = &mut ws_client.clients;
                let sort_by = ws_client.sort_by.entry("Connected".to_string()).or_default();
                let direction = &sort_by.direction;
                match sort_by.field {
                    SortField::Default => clients.default_sort(direction.clone()),
                    SortField::Date => clients.sort_by_date(direction.clone()),
                    SortField::Name => clients.sort_by_name(direction.clone()),
                };
                // let text_style = eframe::egui::TextStyle::Body;
                // let row_height = ui.text_style_height(&text_style);
                let row_height = ui.spacing().interact_size.y; // if you are adding buttons instead of labels.
                let total_rows = clients.len();
                ScrollArea::vertical()
                    .max_height(f32::INFINITY)
                    .max_width(f32::INFINITY)
                    .show_rows(ui, row_height, total_rows, |ui, row_range| 
                {
                    for index in row_range {
                        ui.add_space(4.);
                        if let Some(client) = clients.get(index) {
                            WebConsoleLayout::client_header(ui, ws_client.ui_actions_channel.0.clone(), client, ws_client.undock_client.clone());
                        }
                    }
                });
            });
        });

        CentralPanel::default().show_inside(ui, |ui| {
            let ws_layout = &mut self.web_console_layout;
            // let connection_string = ws_layout.c
            if !ws_layout.error.is_empty() {
                let options = ToastOptions::default();
                options.duration(Some(web_time::Duration::from_secs(3)));

                self.toasts.add(Toast {
                    kind: crate::ui_tools::toasts::ToastKind::Error,
                    text: ws_layout.error.clone().into(),
                    options,
                });
                ws_layout.error.clear();
            }
            ws_layout.ui(ui);
        });
    }

    pub fn refresh_client_list(&mut self) {
        let tx = self.connected_clients_tx.clone();
        PlatformSpawner::spawn(async move {
            match get_connected_clients(tx).await {
                Ok(_) => info!("web_console/mod.rs -> get_connected_clients ran ok"),
                Err(e) => log::warn!("web_console/mod.rs -> get_connected_clients error: {e:?}"),
            }
        });
    }
}

