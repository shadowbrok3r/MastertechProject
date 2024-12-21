use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{popup_below_widget, text::LayoutJob, Align, Button, Color32, ComboBox, FontFamily, FontId, Frame, Layout, Margin, PopupCloseBehavior, RichText, Rounding, ScrollArea, Spinner, Stroke, Style, TextEdit, TextFormat, TopBottomPanel, Ui, Vec2, Widget, WidgetText};
use database::schema::{ConnectedClient, utilities::get_connected_clients};
use displays::{channel_manager::ChannelManager, tasks::task_layout::{SortField, SortOptions}, ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts}, virtual_filesystem::FileSystem, SortDirection};
use egui_extras::{Size, Strip, StripBuilder};
use serde::Serialize;
use websockets::WebSocketClient;
use std::collections::{BTreeMap, HashMap};
use crate::app_state::MtechServerContext;
use wasm_bindgen_futures::spawn_local;
use std::collections::BTreeSet;
use std::borrow::BorrowMut;
use displays::Sortable;
use std::sync::Arc;
use log::info;

pub mod websockets;
pub mod charts;
pub mod display;
use crate::tabs::web_console::websockets::ClientHandler;

pub enum ClientUiAction {
    UndockClient(String),
    DeleteClient(ConnectedClient),
    ConnectClient(ConnectedClient),
    ExportHistory(ConnectedClient)
}

#[derive(Serialize)]
pub struct WebConsoleLayout {
    pub search_inputs: HashMap<String, String>,
    pub client_map: BTreeMap<String, Vec<ConnectedClient>>,
    pub open_menu: bool,
    #[serde(skip)]
    pub ui_actions_channel: (Sender<ClientUiAction>, Receiver<ClientUiAction>),
    pub sort_by: HashMap<String, SortOptions>,
    pub last_sort_field: Option<SortField>,    
    pub loading: bool,
    /// tracking for which client we want to undock
    /// into a floating UI when we click the undock button
    pub undock_client: HashMap<String, bool>,
    /// The undock button was clicked for a ConnectedClient
    pub wants_to_undock: bool,
    #[serde(skip)]
    pub toasts: Toasts,
    #[serde(skip)]
    pub file_system: FileSystem,
    #[serde(skip)]
    pub ws_clients: HashMap<String, WebSocketClient>
}

impl WebConsoleLayout {
    pub fn new(client_map: BTreeMap<String, Vec<ConnectedClient>>) -> Self 
    {
        let ui_actions_channel = ClientUiAction::create_unbounded_channel();
        Self {  
            client_map,
            search_inputs: HashMap::new(), 
            open_menu: false,
            sort_by: HashMap::new(),
            last_sort_field: None,
            loading: false,
            undock_client: HashMap::new(),
            wants_to_undock: false,
            toasts: Toasts::new(),
            file_system: FileSystem::new(),
            ws_clients: HashMap::new(),
            ui_actions_channel
        }
    }

    pub fn receive(&mut self) {
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
                        "wss://sock.master-tech.app/websocket?role=master&room_id={}",
                        client.connection_string.clone()
                    );
                    client.connected = false;
                    client.delete_client();
                    if let Some(ws_client) = self.ws_clients.get_mut(&client.connection_string)
                    {
                        ws_client.ws_sender.close();
                    }
                },
                ClientUiAction::ConnectClient(mut client) => {
                    let url = format!(
                        "wss://sock.master-tech.app/websocket?role=master&room_id={}",
                        client.connection_string.clone()
                    );
                    match ewebsock::connect(&url, Default::default()) {
                        Ok((mut ws_sender, ws_receiver)) => {
                            client.connected = true;

                            ws_sender.send(ewebsock::WsMessage::Text(
                                "Server Connected".to_string(),
                            ));

                            let ws_client = WebSocketClient::new(
                                ws_sender,
                                ws_receiver,
                                client.clone(),
                                self.file_system.clone(),
                            );
                            
                            self.ws_clients
                                .entry(client.connection_string.clone())
                                .or_insert(ws_client);
                        }
                        Err(error) => {
                            client.connected = false;
                            info!("Failed to connect to {:?}: {}", &url, error);
                            let toast = &mut self.toasts;

                            let error_toast = Toast {
                                kind: ToastKind::Error,
                                text: format!("{error:?}").into(),
                                options: ToastOptions::default()
                                    .show_progress(true)
                                    .duration_in_seconds(6.0),
                            };
                            toast.add(error_toast);
                        }
                    };
                },
                ClientUiAction::ExportHistory(mut client) => {
                    if let Some(ws_client) = self.ws_clients.get(&client.connection_string) {
                        client.export_logs(ws_client.history.clone());
                    }
                },
            }
        }
    }

    pub fn layout_cols(&mut self, ui: &mut Ui) {
        self.receive();
        ui.style_mut().visuals.window_rounding = ui.style().visuals.window_rounding;
        let column_width = Size::exact(ui.available_width()/2.0);
        let x: f32 = ui.available_height() / 1.1;
        let style = ui.style().clone();
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

    fn headers(&mut self, mut s: Strip, style: Arc<Style>){
        let header_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(8.0, 1.0))
            .rounding(style.visuals.window_rounding)
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
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| 
                        {
                            let search_input = self.search_inputs.entry(name.clone()).or_insert_with(String::new);
                            let mut margin = Margin::default();
                            margin.top = 6.0;
                            margin.left = 4.0;
                            
                            TextEdit::singleline(search_input).hint_text("Search").desired_width(100.0).margin(margin).ui(ui);

                            ui.add_space(15.);

                            let response = Button::new(RichText::new(name.to_owned())
                                    .color(style.visuals.warn_fg_color)
                                    .size(13.0).monospace()
                                )
                                .fill(style.visuals.noninteractive().bg_fill)
                                .rounding(Rounding::same(2.))
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
                        
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
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
            .inner_margin(Margin::same(6.0))
            .rounding(style.visuals.menu_rounding)
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
                // inputs.insert(client.friendly_name.as_ref().cloned().unwrap_or_default());
                // inputs.insert(client.client_hash.clone());
            }

            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        let row_height = 450.;
                        let total_rows = clients.len(); 
                        let scroll_area = ScrollArea::vertical().auto_shrink(false);
                        ui.ctx().options_mut(|o| o.line_scroll_speed = 30.0);
                        scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                            // Retrieve search input for the current context, or default to an empty string.
                            let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();

                            // Iterate only over the rows in the current viewport range.
                            for row in row_range {
                                if !search_input.is_empty() {
                                    ui.scroll_to_cursor(Some(Align::Center));
                                }
                                if let Some(client) = clients.get_mut(row) {
                                    let connection_string = client.connection_string.clone();
                                    let color = if client.connected{ Color32::LIGHT_BLUE } else { Color32::LIGHT_RED };
                            
                                    let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 14))
                                        .inner_margin(Margin::same(4.0)).outer_margin(Margin::symmetric(5.0, 3.0))
                                        .rounding(Rounding::same(10.0)).stroke(Stroke::new(1.0, color));
                            
                                    let undock = if let Some(undock) = self.undock_client.get(&connection_string){
                                        undock
                                    } else { &false };
                                    
                                    if !*undock {
                                        column_frame.show(ui, |ui| {
                                            // ui.set_min_size(Vec2::new(400., 400.));
                                            ui.vertical_centered_justified(|ui| {
                                                let tx = self.ui_actions_channel.0.clone();
                                                ui.horizontal(|ui| Self::client_headers(ui, tx, client, self.undock_client.clone()));
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

    pub fn client_headers(ui: &mut Ui, tx: Sender<ClientUiAction>, client: &ConnectedClient, undock_client: HashMap<String, bool>) {
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(3.0, 0.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
        header_frame.show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    let button = Button::new(RichText::new("✖").color(Color32::LIGHT_RED))
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if button.clicked() {
                        let _ = tx.try_send(ClientUiAction::DeleteClient(client.clone()));
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
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if undock.clicked() {
                        let _ = tx.try_send(ClientUiAction::UndockClient(client.connection_string.clone()));
                    }
                });

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add_space(ui.available_width() / 3.1);
                    // Create a new LayoutJob
                    let mut job = LayoutJob::default();

                    if let Some(friendly_name) = client.clone().friendly_name {
                        job.append(
                            &friendly_name,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(14.0, FontFamily::Proportional),
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
                                    font_id: FontId::new(14.0, FontFamily::Proportional),
                                    color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
                                    valign: Align::Min,
                                    ..Default::default()
                                },
                            );
                            job.append(
                                txt.1,
                                0.0,
                                TextFormat {
                                    font_id: FontId::new(14.0, FontFamily::Proportional),
                                    color: Color32::from_rgb(199, 202, 245),
                                    valign: Align::Min,
                                    ..Default::default()
                                },
                            );
                        }
                    };

                    // Convert LayoutJob to WidgetText
                    let formatted_text = WidgetText::from(job);

                    if ui.button(formatted_text).clicked() {};
                });

                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    let button = Button::new(RichText::new("⮫").color(Color32::LIGHT_RED))
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(30.0, ui.available_height()))
                        .ui(ui);

                    if button.clicked() {
                        let _ = tx.try_send(ClientUiAction::ConnectClient(client.clone()));
                    }

                    ui.add_space(10.0);

                    let export =
                        Button::new(RichText::new("Export").size(10.0).color(Color32::LIGHT_RED))
                            .fill(Color32::TRANSPARENT)
                            .min_size(Vec2::new(30.0, ui.available_height()))
                            .ui(ui);

                    if export.clicked() {
                        let _ = tx.try_send(ClientUiAction::ExportHistory(client.clone()));
                    }

                    ui.add_space(45.0);
                });
            });
        });
    }
}


impl MtechServerContext {
    pub fn web_console(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();

        let side_panel_frame = Frame::default()
            .inner_margin(Margin::same(6.0))
            .outer_margin(Margin::same(6.0))
            .fill(Color32::from_rgb(17,17,19))
            .rounding(Rounding::same(5.0)) ;

        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 4.0);

        TopBottomPanel::top("Client_Top_panel").frame(side_panel_frame)
        .show_separator_line(false)
        .show_animated_inside(ui, true, |ui |{
            ui.vertical_centered(|ui |{
                if Button::new("Refresh").min_size(Vec2::new(50.0, 15.0)).ui(ui).clicked()
                {
                    let tx = self.shared_ctx.connected_clients_tx.clone();
                    spawn_local(async move {
                        get_connected_clients(tx).await.unwrap();
                    });
                    
                }
            });
        });

        if !self.error.is_empty() {
            TopBottomPanel::bottom("error").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Error:");
                    ui.colored_label(Color32::RED, &self.error);
                });
            });
        }
        
 
        self.web_console_layout.layout_cols(ui);
    }
}

