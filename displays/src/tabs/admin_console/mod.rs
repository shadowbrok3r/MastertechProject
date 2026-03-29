use crate::{PlatformSpawner, Spawner, channel_manager::ChannelManager, tabs::{ai_playground::enhanced::EnhancedAiPlayground, tasks::task_layout::{SortField, SortOptions}}, ui_tools::toasts::{Toast, ToastOptions, ToastStyle}, virtual_filesystem::FileSystem};
use eframe::egui::{self, Align, Button, CentralPanel, Color32, Context, Frame, Layout, Margin, ScrollArea, Stroke, Ui, Vec2, Widget};
use database::schema::{utilities::get_connected_clients, ConnectedClient, Sortable};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap};
use client_interface::WebSocketClient;
use crate::app_state::SharedContext;
use client_action::ClientUiAction;
use serde::Serialize;
use log::info;
use core::f32;

use super::script_editor::ScriptEditor;

pub mod client_action;
pub mod client_interface;
pub mod ui;

#[derive(Serialize, Default)]
pub enum WebConsolePageState {
    #[default]
    AllClients,
    ScriptEditor,
    #[cfg(not(target_arch = "wasm32"))]
    AiPlayground,
}

#[derive(Serialize)]
pub struct AdminConsole {
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
    pub ai_playground: EnhancedAiPlayground,
}

impl AdminConsole {
    pub fn new(client_map: BTreeMap<String, Vec<ConnectedClient>>, clients: Vec<ConnectedClient>) -> Self {
        let ui_actions_channel = ClientUiAction::create_unbounded_channel();
        Self {
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
            script_editor: ScriptEditor::new(),
            ai_playground: EnhancedAiPlayground::default()
        }
    }

    // pub fn set_filesystem(&mut self, filesystem: FileSystem) -> &mut Self {
    //     self.filesystem = filesystem.clone();
    //     self.script_editor.set_filesystem(filesystem);
    //     self
    // }

    pub fn receive(&mut self, ctx: &Context) {
        self.filesystem.receive();
        if let Ok(action) = self.ui_actions_channel.1.try_recv() {
            self.handle_action(action);
            ctx.request_repaint();
        }
    }
}


impl SharedContext {
    pub fn admin_console(&mut self, ui: &mut Ui){
        self.web_console_layout.receive(ui.ctx());
        let inner_margin = Margin::same(3);
        let outer_margin = Margin::same(0);
        let stroke = Stroke::new(0.7, Color32::from_additive_luminance(150));
        let radius = eframe::egui::CornerRadius::same(5);

        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 4.0);

        eframe::egui::Panel::top("Client_Top_panel")
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(17,17,19))
                    .inner_margin(inner_margin)
                    .outer_margin(outer_margin)
                    .stroke(stroke)
                    .corner_radius(radius)
            )
            .show_separator_line(false)
            .exact_size(35.)
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
                let button_size = Vec2::new(70.0, 15.0);
                if Button::new("Clients")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() 
                {
                    self.refresh_client_list();
                    self.web_console_layout.state = WebConsolePageState::AllClients;
                }
                ui.add_space(5.);
                if Button::new("Script Editor")
                    .min_size(button_size)
                    .ui(ui)
                    .clicked() 
                {
                    self.web_console_layout.state = WebConsolePageState::ScriptEditor;
                }
                ui.add_space(5.);
                #[cfg(not(target_arch = "wasm32"))]
                if Button::new("🎮 AI Playground")
                    .min_size(Vec2::new(95.0, 15.0))
                    .ui(ui)
                    .clicked() 
                {
                    self.web_console_layout.state = WebConsolePageState::AiPlayground;
                }
            });
        });

        eframe::egui::Panel::left("Client_Side_panel")
            .frame(
                Frame::default()
                    .fill(ui.style().visuals.extreme_bg_color)
                    .inner_margin(inner_margin)
                    .outer_margin(outer_margin)
                    .stroke(stroke)
                    .corner_radius(radius)
            )
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
                            // Check if we have an active WebSocket connection with confirmed remote client activity
                            // Green requires both: master connected AND client actively responding
                            let is_ws_connected = ws_client.ws_clients
                                .get(&client.connection_string)
                                .map(|wsc| wsc.is_connected && wsc.last_pong_time.is_some())
                                .unwrap_or(false);
                            
                            AdminConsole::client_header(
                                ui, 
                                ws_client.ui_actions_channel.0.clone(), 
                                client, 
                                ws_client.undock_client.clone(),
                                is_ws_connected,
                            );
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
                    style: ToastStyle::default(),
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

