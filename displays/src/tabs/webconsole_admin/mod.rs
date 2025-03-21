use crate::{channel_manager::ChannelManager, remote_viewer::term_viewer::RemoteTerminal, tasks::task_layout::{SortField, SortOptions}, ui_tools::toasts::{Toast, ToastOptions}, virtual_filesystem::FileSystem, PlatformSpawner, Sortable, Spawner};
use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Frame, Layout, Margin, ScrollArea, SidePanel, Stroke, TopBottomPanel, Ui, Vec2, Widget};
use database::{schema::{utilities::get_connected_clients, ConnectedClient}, WS_MASTER_URL};
use client_interface::{WebSocketClient, ClientHandler};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap};
use crate::app_state::SharedContext;
use serde::Serialize;
use log::info;
use core::f32;

use super::script_editor::ScriptEditor;

pub mod shell;
pub mod client_interface;
pub mod ui;

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
    script_editor: ScriptEditor,
    #[serde(skip)]
    pub remote_viewer: Option<RemoteTerminal>
}

impl WebConsoleLayout {
    pub fn new(client_map: BTreeMap<String, Vec<ConnectedClient>>, clients: Vec<ConnectedClient>) -> Self {
        let ui_actions_channel = ClientUiAction::create_unbounded_channel();
        Self {
            remote_viewer: None,
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

