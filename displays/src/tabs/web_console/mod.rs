//! Web Console - Revamped admin console for managing connected Mastertech clients.
//!
//! Features:
//! - Live view of connected devices with assigned SurrealDB users
//! - Robust ping/pong disconnect detection (10s timeout)
//! - Client actions: Delete, Create TUR, Remote Shell, File Explorer
//! - AI-enhanced shell with MCP integration

use crate::{virtual_filesystem::FileSystem, PlatformSpawner, Spawner};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{
    utilities::get_connected_clients, ComputerData, ConnectedClient, User,
};
use eframe::egui::{
    CentralPanel, Color32, Context, CornerRadius, Frame, Margin, RichText, SidePanel, Stroke,
    TopBottomPanel, Ui,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use web_time::Instant;

pub mod client_card;
pub mod client_grid;
pub mod connection_manager;
pub mod file_explorer_view;
pub mod shell_view;
pub mod tur_modal;

pub use client_card::ClientCard;
pub use client_grid::ClientGrid;
pub use connection_manager::{ConnectionManager, ConnectionState};
pub use shell_view::{ShellType, ShellView};

/// Actions that can be triggered from the web console UI
#[derive(Debug, Clone)]
pub enum WebConsoleAction {
    /// Refresh the client list from database
    RefreshClients,
    /// Connect to a specific client via WebSocket
    ConnectClient(ConnectedClient),
    /// Disconnect from a client
    DisconnectClient(String),
    /// Delete a client from the database
    DeleteClient(ConnectedClient),
    /// Open TUR sheet modal for a client
    OpenTurModal(ConnectedClient),
    /// Open remote shell for a client
    OpenShell(ConnectedClient, ShellType),
    /// Open file explorer for a client
    OpenFileExplorer(ConnectedClient),
    /// Mark client as disconnected (ping timeout)
    MarkDisconnected(String),
}

/// View mode for the client list
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

/// Filter options for client list
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ClientFilter {
    #[default]
    All,
    Connected,
    Disconnected,
}

/// Main state for the revamped web console
#[derive(Serialize)]
pub struct WebConsole {
    /// All clients fetched from database
    pub clients: Vec<ConnectedClient>,
    /// Cached computer data for clients (keyed by connection_string)
    #[serde(skip)]
    pub computer_cache: HashMap<String, ComputerData>,
    /// Cached user data for assigned users (keyed by user RecordId string)
    #[serde(skip)]
    pub user_cache: HashMap<String, User>,
    /// Active WebSocket connections managed per client
    #[serde(skip)]
    pub connections: HashMap<String, ConnectionManager>,
    /// Currently selected client for detail view
    pub selected_client: Option<String>,
    /// View mode (grid or list)
    pub view_mode: ViewMode,
    /// Client filter
    pub filter: ClientFilter,
    /// Search input for filtering clients
    pub search_query: String,
    /// Show side panel
    pub show_side_panel: bool,
    /// Loading state
    pub loading: bool,
    /// Error message if any
    pub error: Option<String>,
    /// Last refresh time
    #[serde(skip)]
    pub last_refresh: Option<Instant>,
    /// Action channel for UI events
    #[serde(skip)]
    pub action_tx: Sender<WebConsoleAction>,
    #[serde(skip)]
    pub action_rx: Receiver<WebConsoleAction>,
    /// Channel for receiving client list updates
    #[serde(skip)]
    pub clients_tx: Sender<Vec<ConnectedClient>>,
    #[serde(skip)]
    pub clients_rx: Receiver<Vec<ConnectedClient>>,
    /// Shared filesystem for file explorer
    #[serde(skip)]
    pub filesystem: FileSystem,
    /// TUR modal state (connection_string of client being created)
    pub tur_modal_client: Option<String>,
    /// Active shell views (keyed by connection_string)
    #[serde(skip)]
    pub shell_views: HashMap<String, ShellView>,
    /// Ping timeout duration in seconds
    pub ping_timeout_secs: u64,
}

impl Default for WebConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl WebConsole {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let (clients_tx, clients_rx) = crossbeam::channel::unbounded();

        Self {
            clients: Vec::new(),
            computer_cache: HashMap::new(),
            user_cache: HashMap::new(),
            connections: HashMap::new(),
            selected_client: None,
            view_mode: ViewMode::default(),
            filter: ClientFilter::default(),
            search_query: String::new(),
            show_side_panel: true,
            loading: false,
            error: None,
            last_refresh: None,
            action_tx,
            action_rx,
            clients_tx,
            clients_rx,
            filesystem: FileSystem::new(),
            tur_modal_client: None,
            shell_views: HashMap::new(),
            ping_timeout_secs: 10,
        }
    }

    /// Process incoming messages from channels
    pub fn receive(&mut self, ctx: &Context) {
        // Receive client list updates
        while let Ok(clients) = self.clients_rx.try_recv() {
            self.clients = clients;
            self.loading = false;
            self.last_refresh = Some(Instant::now());
            ctx.request_repaint();
        }

        // Process UI actions
        while let Ok(action) = self.action_rx.try_recv() {
            self.handle_action(action);
            ctx.request_repaint();
        }

        // Update connection states and check for timeouts
        self.update_connections(ctx);

        // Receive filesystem updates
        self.filesystem.receive();

        // Update shell views
        for shell in self.shell_views.values_mut() {
            shell.receive(ctx);
        }
    }

    /// Handle UI actions
    fn handle_action(&mut self, action: WebConsoleAction) {
        match action {
            WebConsoleAction::RefreshClients => {
                self.refresh_clients();
            }
            WebConsoleAction::ConnectClient(client) => {
                self.connect_to_client(&client);
            }
            WebConsoleAction::DisconnectClient(conn_string) => {
                self.disconnect_client(&conn_string);
            }
            WebConsoleAction::DeleteClient(client) => {
                self.delete_client(&client);
            }
            WebConsoleAction::OpenTurModal(client) => {
                self.tur_modal_client = Some(client.connection_string.clone());
                self.fetch_computer_data(&client);
            }
            WebConsoleAction::OpenShell(client, shell_type) => {
                self.open_shell(&client, shell_type);
            }
            WebConsoleAction::OpenFileExplorer(client) => {
                self.open_file_explorer(&client);
            }
            WebConsoleAction::MarkDisconnected(conn_string) => {
                self.mark_client_disconnected(&conn_string);
            }
        }
    }

    /// Refresh client list from database
    pub fn refresh_clients(&mut self) {
        self.loading = true;
        let tx = self.clients_tx.clone();
        PlatformSpawner::spawn(async move {
            match get_connected_clients(tx).await {
                Ok(_) => log::info!("WebConsole: Refreshed client list"),
                Err(e) => log::error!("WebConsole: Failed to refresh clients: {e:?}"),
            }
        });
    }

    /// Connect to a client via WebSocket
    fn connect_to_client(&mut self, client: &ConnectedClient) {
        let conn_string = client.connection_string.clone();
        
        // Create connection manager if not exists
        if !self.connections.contains_key(&conn_string) {
            let manager = ConnectionManager::new(
                client.clone(),
                self.filesystem.clone(),
                self.ping_timeout_secs,
            );
            self.connections.insert(conn_string.clone(), manager);
        }

        // Initiate connection
        if let Some(manager) = self.connections.get_mut(&conn_string) {
            manager.connect();
        }
    }

    /// Disconnect from a client
    fn disconnect_client(&mut self, conn_string: &str) {
        if let Some(manager) = self.connections.get_mut(conn_string) {
            manager.disconnect();
        }
        self.connections.remove(conn_string);
        self.shell_views.remove(conn_string);
    }

    /// Delete client from database
    fn delete_client(&mut self, client: &ConnectedClient) {
        let id = client.id.clone();
        let conn_string = client.connection_string.clone();
        
        // Disconnect first
        self.disconnect_client(&conn_string);
        
        // Remove from local list
        self.clients.retain(|c| c.connection_string != conn_string);
        
        // Delete from database
        PlatformSpawner::spawn(async move {
            use database::{DATABASE, schema::CONNECTED_CLIENT_TABLE};
            let result: Result<Option<database::schema::Record>, _> = DATABASE
                .delete((CONNECTED_CLIENT_TABLE, id.key().to_string()))
                .await;
            match result {
                Ok(_) => log::info!("WebConsole: Deleted client {}", conn_string),
                Err(e) => log::error!("WebConsole: Failed to delete client: {e:?}"),
            }
        });
    }

    /// Fetch computer data for a client
    fn fetch_computer_data(&mut self, client: &ConnectedClient) {
        if let Some(computer_id) = &client.computer {
            let computer_id = computer_id.clone();
            let conn_string = client.connection_string.clone();
            let cache_tx = crossbeam::channel::unbounded::<(String, ComputerData)>().0;
            
            // Check cache first
            if self.computer_cache.contains_key(&conn_string) {
                return;
            }

            PlatformSpawner::spawn(async move {
                use database::DATABASE;
                let result: Result<Option<ComputerData>, _> = DATABASE
                    .select(computer_id)
                    .await;
                match result {
                    Ok(Some(computer)) => {
                        log::info!("WebConsole: Fetched computer data for {}", conn_string);
                        let _ = cache_tx.send((conn_string, computer));
                    }
                    Ok(None) => log::warn!("WebConsole: No computer data for {}", conn_string),
                    Err(e) => log::error!("WebConsole: Failed to fetch computer: {e:?}"),
                }
            });
        }
    }

    /// Open shell for a client
    fn open_shell(&mut self, client: &ConnectedClient, shell_type: ShellType) {
        let conn_string = client.connection_string.clone();
        
        // Ensure we have a connection
        if !self.connections.contains_key(&conn_string) {
            self.connect_to_client(client);
        }

        // Create or update shell view
        if let Some(manager) = self.connections.get(&conn_string) {
            let shell = ShellView::new(
                client.clone(),
                shell_type,
                manager.send_cmd_tx.clone(),
            );
            self.shell_views.insert(conn_string, shell);
        }
    }

    /// Open file explorer for a client
    fn open_file_explorer(&mut self, client: &ConnectedClient) {
        let conn_string = client.connection_string.clone();
        
        // Ensure we have a connection
        if !self.connections.contains_key(&conn_string) {
            self.connect_to_client(client);
        }

        self.selected_client = Some(conn_string);
    }

    /// Mark a client as disconnected in the database
    fn mark_client_disconnected(&mut self, conn_string: &str) {
        let conn_string = conn_string.to_string();
        
        // Update local state
        if let Some(client) = self.clients.iter_mut().find(|c| c.connection_string == conn_string) {
            client.connected = false;
        }

        // Update database
        PlatformSpawner::spawn(async move {
            use database::DATABASE;
            let _: Result<surrealdb::Response, _> = DATABASE
                .query("UPDATE connected_client SET connected = false, last_update = time::now() WHERE connection_string == $conn")
                .bind(("conn", conn_string.clone()))
                .await;
            log::info!("WebConsole: Marked {} as disconnected", conn_string);
        });
    }

    /// Update connection states and check for ping timeouts
    fn update_connections(&mut self, ctx: &Context) {
        let mut disconnected = Vec::new();

        for (conn_string, manager) in self.connections.iter_mut() {
            manager.update();

            // Check for ping timeout
            if manager.is_timed_out() {
                disconnected.push(conn_string.clone());
            }
        }

        // Mark timed-out clients as disconnected
        for conn_string in disconnected {
            self.mark_client_disconnected(&conn_string);
            if let Some(manager) = self.connections.get_mut(&conn_string) {
                manager.state = ConnectionState::Disconnected;
            }
        }

        // Request repaint if we have active connections
        if !self.connections.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// Get filtered and searched clients
    pub fn filtered_clients(&self) -> Vec<&ConnectedClient> {
        self.clients
            .iter()
            .filter(|c| {
                // Apply filter
                match self.filter {
                    ClientFilter::All => true,
                    ClientFilter::Connected => c.connected,
                    ClientFilter::Disconnected => !c.connected,
                }
            })
            .filter(|c| {
                // Apply search
                if self.search_query.is_empty() {
                    true
                } else {
                    let query = self.search_query.to_lowercase();
                    c.connection_string.to_lowercase().contains(&query)
                        || c.friendly_name
                            .as_ref()
                            .map(|n| n.to_lowercase().contains(&query))
                            .unwrap_or(false)
                }
            })
            .collect()
    }

    /// Get connection state color for a client
    pub fn get_connection_color(&self, conn_string: &str) -> Color32 {
        if let Some(manager) = self.connections.get(conn_string) {
            manager.state.color()
        } else {
            // Check database connected status
            if let Some(client) = self.clients.iter().find(|c| c.connection_string == conn_string) {
                if client.connected {
                    Color32::YELLOW // Connected in DB but no active WS connection
                } else {
                    Color32::RED
                }
            } else {
                Color32::GRAY
            }
        }
    }

    /// Get elapsed time since last pong for a client
    pub fn get_last_pong_elapsed(&self, conn_string: &str) -> Option<std::time::Duration> {
        self.connections
            .get(conn_string)
            .and_then(|m| m.last_pong_time)
            .map(|t| t.elapsed())
    }

    /// Main UI display function
    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive(ui.ctx());

        // Handle TUR modal if open
        if let Some(conn_string) = self.tur_modal_client.clone() {
            if let Some(client) = self.clients.iter().find(|c| c.connection_string == conn_string) {
                let mut modal_state = tur_modal::TurModalState::new(client.clone());
                
                // Populate with cached computer data if available
                if let Some(computer) = self.computer_cache.get(&conn_string) {
                    modal_state.populate_from_computer(computer);
                }

                match tur_modal::show_tur_modal(ui.ctx(), &mut modal_state) {
                    tur_modal::TurModalResult::Cancelled => {
                        self.tur_modal_client = None;
                    }
                    tur_modal::TurModalResult::Confirmed(data) => {
                        log::info!("TUR creation confirmed: {:?}", data);
                        // TODO: Navigate to TUR sheet tab with pre-populated data
                        self.tur_modal_client = None;
                    }
                    tur_modal::TurModalResult::Open => {}
                }
            }
        }

        // Main layout
        let inner_margin = Margin::same(3);
        let stroke = Stroke::new(0.7, Color32::from_additive_luminance(150));
        let radius = CornerRadius::same(5);

        // Top panel with title
        TopBottomPanel::top("web_console_top")
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(17, 17, 19))
                    .inner_margin(inner_margin)
                    .stroke(stroke)
                    .corner_radius(radius),
            )
            .exact_height(40.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("🖥 Web Console")
                            .size(16.0)
                            .color(Color32::from_rgb(51, 255, 189)),
                    );

                    ui.add_space(20.0);

                    // Toggle side panel
                    if ui
                        .button(if self.show_side_panel {
                            "◀ Hide Panel"
                        } else {
                            "▶ Show Panel"
                        })
                        .clicked()
                    {
                        self.show_side_panel = !self.show_side_panel;
                    }
                });
            });

        // Side panel with shell/explorer views for selected client
        if self.show_side_panel {
            SidePanel::right("web_console_detail")
                .min_width(400.0)
                .max_width(600.0)
                .frame(
                    Frame::default()
                        .fill(Color32::from_rgb(15, 17, 22))
                        .inner_margin(Margin::same(8))
                        .stroke(stroke)
                        .corner_radius(radius),
                )
                .show_inside(ui, |ui| {
                    if let Some(conn_string) = &self.selected_client.clone() {
                        // Show shell view if available
                        if let Some(shell) = self.shell_views.get_mut(conn_string) {
                            shell.show(ui);
                        } else if let Some(manager) = self.connections.get(conn_string) {
                            // Show file explorer for connected client
                            let mut explorer = file_explorer_view::FileExplorerView::new(
                                manager.client.clone(),
                                manager.send_cmd_tx.clone(),
                            );
                            explorer.show(ui);
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label(
                                    RichText::new("Select a client and connect to view details")
                                        .color(Color32::from_rgb(150, 155, 165)),
                                );
                            });
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new("No client selected")
                                    .color(Color32::from_rgb(150, 155, 165)),
                            );
                        });
                    }
                });
        }

        // Central panel with client grid
        CentralPanel::default()
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(12, 14, 18))
                    .inner_margin(Margin::same(12)),
            )
            .show_inside(ui, |ui| {
                ClientGrid::show(ui, self);
            });
    }
}

