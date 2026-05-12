use crate::{PlatformSpawner, Spawner, channel_manager::ChannelManager, tabs::{ai_playground::enhanced::EnhancedAiPlayground, tasks::task_layout::{SortField, SortOptions}}, ui_tools::toasts::{Toast, ToastOptions, ToastStyle}, virtual_filesystem::FileSystem};
use eframe::egui::{self, Align, Button, CentralPanel, Color32, Context, Frame, Layout, Margin, ScrollArea, Stroke, Ui, Vec2, Widget};
use database::schema::{utilities::get_connected_clients, ConnectedClient, RecordIdExt, Sortable};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap};
use client_interface::WebSocketClient;
use crate::app_state::SharedContext;
use crate::tabs::tasks::client_cards::should_show_connected_client_in_summaries;
use client_action::ClientUiAction;
use client_interface::TransportKind;
use serde::Serialize;
use log::info;
use core::f32;

use super::script_editor::ScriptEditor;

pub mod client_action;
pub mod client_interface;
pub mod relink_popup;
pub mod ui;

pub use relink_popup::RelinkClientPopup;

/// Controls whether a remote-client session is shown inline (docked in the
/// central panel when it is also the focused client) or in its own floating
/// OS viewport / egui Window.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SessionLayout {
    /// Show in the admin-console central panel.  Only one docked client is
    /// rendered at a time; whichever one matches `AdminConsole::focused_client`
    /// is the active one that receives keyboard input and plugin commands.
    #[default]
    Docked,
    /// Render in a separate OS viewport (native) or egui Window (WASM).
    Floating,
}

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
    open_menu: bool,
    #[serde(skip)]
    pub ui_actions_channel: (Sender<ClientUiAction>, Receiver<ClientUiAction>),
    state: WebConsolePageState,
    pub sort_by: HashMap<String, SortOptions>,
    pub last_sort_field: Option<SortField>,
    pub loading: bool,
    /// Per-session display mode.  Each entry maps a `connection_string` to
    /// either `Docked` (show in the central panel) or `Floating` (own window).
    /// Replaces the old `undock_client: HashMap<String, bool>` whose `bool`
    /// meaning was inverted and confusing.
    pub session_layout: HashMap<String, SessionLayout>,
    /// The connection string of the machine that currently has keyboard /
    /// script / plugin-command focus.  `None` when no session is open.
    pub focused_client: Option<String>,
    #[serde(skip)]
    pub filesystem: FileSystem,
    #[serde(skip)]
    pub ws_clients: HashMap<String, WebSocketClient>,
    /// Map of `connection_string` -> open `diagnostic_session.id` for any
    /// client that an AI agent is currently diagnosing through the MCP
    /// bridge. Populated when `create_diagnostic_session` succeeds and
    /// cleared when `close_diagnostic_session` runs. Read by the My Tasks
    /// connected-client cards to show an "AI active" badge.
    #[serde(skip)]
    pub active_diagnostic_sessions: HashMap<String, String>,
    pub error: String,
    script_editor: ScriptEditor,
    pub ai_playground: EnhancedAiPlayground,
    /// Open re-link popup. `Some(_)` while the admin is searching for the
    /// correct customer to bind to a connected client (the used-machine
    /// scenario where OA-key auto-detection resolves to the wrong owner).
    /// See `relink_popup.rs`.
    #[serde(skip)]
    pub relink_popup: Option<RelinkClientPopup>,
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
            session_layout: Default::default(),
            focused_client: None,
            filesystem: FileSystem::new(),
            ws_clients: Default::default(),
            active_diagnostic_sessions: Default::default(),
            ui_actions_channel,
            error: Default::default(),
            state: Default::default(),
            script_editor: ScriptEditor::new(),
            ai_playground: EnhancedAiPlayground::default(),
            relink_popup: None,
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

        // Drive the re-link popup, if open. Poll its background channel
        // first (search/payload/apply events arrive here) and then render.
        // When the admin closes the window or Apply succeeds the popup
        // returns `false` and we drop it.
        if let Some(popup) = self.relink_popup.as_mut() {
            popup.poll();
            let still_open = popup.ui(ctx);
            if !still_open {
                // Trigger a refresh so the freshly-updated friendly_name
                // shows up in the side panel without waiting for the
                // periodic poll.
                self.relink_popup = None;
                ctx.request_repaint();
            }
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
                // Stable secondary sort: clients assigned to the logged-in user float
                // to the top regardless of the primary sort direction.
                if let Some(me) = crate::get_current_user_from_auth() {
                    let my_id = me.get_id();
                    clients.sort_by(|a, b| {
                        let a_mine = a.assigned_user.as_ref()
                            .is_some_and(|u| u.key_string() == my_id.key_string());
                        let b_mine = b.assigned_user.as_ref()
                            .is_some_and(|u| u.key_string() == my_id.key_string());
                        b_mine.cmp(&a_mine) // mine first; equal elements keep prior order (stable)
                    });
                }
                        let visible_indices: Vec<usize> = clients
                    .iter()
                    .enumerate()
                    .filter_map(|(i, client)| {
                        let is_ws_connected = ws_client
                            .ws_clients
                            .get(&client.connection_string)
                            .map(|wsc| {
                                if wsc.transport.kind() == TransportKind::Tcp {
                                    wsc.is_connected
                                } else {
                                    wsc.is_connected && wsc.last_pong_time.is_some()
                                }
                            })
                            .unwrap_or(false);
                        should_show_connected_client_in_summaries(client, is_ws_connected)
                            .then_some(i)
                    })
                    .collect();

                let row_height = ui.spacing().interact_size.y; // if you are adding buttons instead of labels.
                let total_rows = visible_indices.len();
                if visible_indices.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No connected clients with activity in the last 2 hours (or an open admin session).",
                        )
                        .weak(),
                    );
                    ui.add_space(6.);
                }
                ScrollArea::vertical()
                    .max_height(f32::INFINITY)
                    .max_width(f32::INFINITY)
                    .show_rows(ui, row_height, total_rows, |ui, row_range| 
                {
                    for row in row_range {
                        ui.add_space(4.);
                        let Some(&index) = visible_indices.get(row) else {
                            continue;
                        };
                        if let Some(client) = clients.get(index) {
                            // Check if we have an active WebSocket connection with confirmed remote client activity
                            // Green requires both: master connected AND client actively responding
                    let is_ws_connected = ws_client.ws_clients
                        .get(&client.connection_string)
                        .map(|wsc| {
                            // TCP connections don't use WebSocket pings/pongs;
                            // liveness is proven by the TCP session itself.
                            if wsc.transport.kind() == TransportKind::Tcp {
                                wsc.is_connected
                            } else {
                                wsc.is_connected && wsc.last_pong_time.is_some()
                            }
                        })
                        .unwrap_or(false);
                            
                            AdminConsole::client_header(
                                ui,
                                ws_client.ui_actions_channel.0.clone(),
                                client,
                                ws_client.session_layout.clone(),
                                ws_client.focused_client.as_deref(),
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

