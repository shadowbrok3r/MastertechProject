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
    /// In-memory cache of the latest security inventory we received
    /// from each connected client (slice 2 of the AV refactor).
    /// Keyed by `connection_string`. Populated by the
    /// `Cmd::SecurityInventoryResponse` handler on the admin side
    /// every time a session is opened and the client replies. Also
    /// persisted to the linked `computer` row's `current_antivirus`
    /// field via a `DATABASE.query("UPDATE …")`, so a later session
    /// on a different admin still sees the data; the in-memory copy
    /// just lets the expanded client-row body render without a DB
    /// round trip per frame.
    #[serde(skip)]
    pub security_inventory: HashMap<String, Vec<database::schema::InstalledSecurityProduct>>,
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
            security_inventory: Default::default(),
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

        // Slice 2 of the AV-data refactor: drain any
        // `SecurityInventoryResponse`s the per-session
        // `WebSocketClient` pumped through the global channel. We do
        // two things per event: (1) cache the in-memory copy so the
        // expanded client-row body renders without hitting the DB,
        // and (2) fire-and-forget upsert it onto the linked
        // `computer` row's `current_antivirus` field so the data
        // outlives the admin session.
        let inv_rx = crate::get_security_inventory_receiver();
        while let Ok(event) = inv_rx.try_recv() {
            log::info!(
                "AdminConsole::receive -> caching security inventory for {} ({} products)",
                event.connection_string,
                event.products.len(),
            );
            self.security_inventory
                .insert(event.connection_string.clone(), event.products.clone());

            // Find the linked computer record (if any) and upsert.
            // Doing the lookup via the cached `clients` list is
            // cheaper than a DB round-trip per response.
            let computer_id = self
                .clients
                .iter()
                .find(|c| c.connection_string == event.connection_string)
                .and_then(|c| c.computer.clone());

            if let Some(id) = computer_id {
                let products = event.products.clone();
                let cs = event.connection_string.clone();
                crate::PlatformSpawner::spawn(async move {
                    // Use a raw UPDATE so we touch only this one
                    // field — avoids reading the whole ComputerData,
                    // mutating, and re-upserting (which is racy if
                    // anything else writes the row concurrently).
                    let res: Result<_, surrealdb::Error> = database::DATABASE
                        .query("UPDATE $id SET current_antivirus = $products")
                        .bind(("id", id))
                        .bind(("products", products))
                        .await;
                    match res {
                        Ok(_) => log::info!(
                            "Persisted security inventory for {cs} to computer row"
                        ),
                        Err(e) => log::error!(
                            "Failed to persist security inventory for {cs}: {e}"
                        ),
                    }
                });
            } else {
                // No linked computer — the data still lives in the
                // in-memory cache so the row can render, just won't
                // survive this session. Common for freshly checked-in
                // machines that haven't been linked yet.
                log::debug!(
                    "AdminConsole::receive -> no linked computer for {}; inventory is in-memory only",
                    event.connection_string,
                );
            }
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

        // Drain `pending_admin_console_focus`, set by clicking
        // "Open Console" on a My Tasks client card. The action handler
        // in `receive_ui_action.rs` only used to flip
        // `pending_activate_tab` to "Admin Console" — actually opening
        // the session on the named client was never wired through, so
        // the user landed here with nothing focused. We now:
        //
        //   1. If there's already an open `ws_clients` entry, just
        //      flip `focused_client` (avoids re-dialing).
        //   2. Otherwise look the full `ConnectedClient` up by
        //      connection_string and dispatch `ConnectClient` so the
        //      transport actually connects.
        //   3. If the lookup misses (the live-data feed hasn't
        //      populated `clients` yet on this frame), re-store the
        //      pending value so the next frame retries.
        if let Some(cs) = self.pending_admin_console_focus.take() {
            if self.web_console_layout.ws_clients.contains_key(&cs) {
                self.web_console_layout.focused_client = Some(cs);
            } else if let Some(client) = self
                .web_console_layout
                .clients
                .iter()
                .find(|c| c.connection_string == cs)
                .cloned()
            {
                self.web_console_layout
                    .handle_action(ClientUiAction::ConnectClient(client));
            } else {
                // Client list isn't ready yet — wait one frame.
                self.pending_admin_console_focus = Some(cs);
            }
        }

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

                // ── Active-client breadcrumb ────────────────────────────
                //
                // Until now operators had to remember which client they
                // last clicked on to know what the Admin Console's
                // central panel was talking to. We surface the focused
                // client's friendly_name + connection_string next to the
                // tab buttons so it's always at a glance.
                //
                // The breadcrumb is right-aligned in the remaining space
                // so it sits visually opposite the "Show Clients" toggle
                // on the left edge.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(focused) = self.web_console_layout.focused_client.as_deref() {
                        let lookup = self
                            .web_console_layout
                            .clients
                            .iter()
                            .find(|c| c.connection_string == focused);
                        let (name, conn) = match lookup {
                            Some(c) => (
                                c.friendly_name.clone().unwrap_or_else(|| "(unnamed)".into()),
                                c.connection_string.clone(),
                            ),
                            None => ("(unknown)".to_string(), focused.to_string()),
                        };

                        // Render right-to-left, so push them in reverse
                        // visual order: connection_string first → name →
                        // label.
                        ui.label(
                            egui::RichText::new(conn)
                                .small()
                                .color(Color32::from_rgb(160, 160, 180)),
                        );
                        ui.label(
                            egui::RichText::new(" · ")
                                .small()
                                .color(Color32::DARK_GRAY),
                        );
                        ui.label(
                            egui::RichText::new(name)
                                .small()
                                .strong()
                                .color(Color32::from_rgb(51, 255, 189)),
                        );
                        ui.label(
                            egui::RichText::new("Active:")
                                .small()
                                .color(Color32::GRAY),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("No active client")
                                .small()
                                .italics()
                                .color(Color32::DARK_GRAY),
                        );
                    }
                });
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

                if visible_indices.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No connected clients with activity in the last 2 hours (or an open admin session).",
                        )
                        .weak(),
                    );
                    ui.add_space(6.);
                }
                // Rows can grow/shrink as the user expands a client's
                // collapsing header, so we can't use `show_rows` (which
                // assumes a uniform row height for virtualization). The
                // visible-indices filter already trims to a handful of
                // active clients, so a non-virtualized `show` is fine.
                ScrollArea::vertical()
                    .max_height(f32::INFINITY)
                    .max_width(f32::INFINITY)
                    .show(ui, |ui|
                {
                    for &index in &visible_indices {
                        ui.add_space(4.);
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
                            
                            let inventory = ws_client
                                .security_inventory
                                .get(&client.connection_string)
                                .map(|v| v.as_slice());
                            AdminConsole::client_header(
                                ui,
                                ws_client.ui_actions_channel.0.clone(),
                                client,
                                ws_client.session_layout.clone(),
                                ws_client.focused_client.as_deref(),
                                is_ws_connected,
                                inventory,
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

