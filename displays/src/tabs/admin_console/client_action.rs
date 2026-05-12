use super::{client_interface::tabs::command_shell::History, AdminConsole, SessionLayout};
use database::{
    schema::{ConnectedClient, Record, RecordIdExt, CONNECTED_CLIENT_TABLE},
    websocket_url_with_room, DATABASE, WS_MASTER_URL, WS_MASTER_URL_LOCAL,
};
use crate::tabs::admin_console::client_interface::{AdminTransport, WebSocketClient};
use crate::{Cmd, PlatformSpawner, Spawner};

pub enum ClientUiAction {
    /// Toggle a session between `Docked` and `Floating` display mode.
    ToggleClientFloat(String),
    /// Make the given connection string the focused client (receives commands).
    FocusClient(String),
    DeleteClient(ConnectedClient),
    ConnectClient(ConnectedClient),
    ExportHistory(ConnectedClient),
    /// Open the re-link customer popup for this client (the
    /// used-machine-with-wrong-owner workflow). See `relink_popup.rs`.
    RelinkCustomer(ConnectedClient),
}

impl AdminConsole {
    pub fn handle_action(&mut self, action: ClientUiAction) {
        match action {
            ClientUiAction::ToggleClientFloat(connection_string) => {
                let current = self.session_layout
                    .get(&connection_string)
                    .copied()
                    .unwrap_or_default();
                let next = match current {
                    SessionLayout::Docked   => SessionLayout::Floating,
                    SessionLayout::Floating => SessionLayout::Docked,
                };
                self.session_layout.insert(connection_string, next);
            }
            ClientUiAction::FocusClient(connection_string) => {
                if self.ws_clients.contains_key(&connection_string) {
                    self.focused_client = Some(connection_string);
                }
            }
            ClientUiAction::DeleteClient(mut client) => {
                if self.focused_client.as_deref() == Some(client.connection_string.as_str()) {
                    self.focused_client = None;
                }
                client.disconnect_client();
                if let Some(mut ws_client) = self.ws_clients.remove(&client.connection_string) {
                    ws_client.transport.close();
                    drop(ws_client);
                }
                self.session_layout.remove(&client.connection_string);
                self.error = format!("WebConsole -> Client {} Deleted", client.connection_string.clone());
            },
            ClientUiAction::ConnectClient(mut client) => {
                self.open_menu = false;
                log::info!("Received Connection Command for {}", client.connection_string);

                // Register a Docked layout for this client if not already present.
                // Existing sessions keep their current layout (Docked or Floating) so
                // the operator can open multiple machines simultaneously.
                self.session_layout
                    .entry(client.connection_string.clone())
                    .or_insert(SessionLayout::Docked);

                // Make this the focused client so commands go to it.
                self.focused_client = Some(client.connection_string.clone());

                let connect_via_ws = |this: &mut AdminConsole, client: &mut ConnectedClient| -> Option<AdminTransport> {
                    let url = websocket_url_with_room(
                        if cfg!(debug_assertions) {
                            WS_MASTER_URL_LOCAL
                        } else {
                            WS_MASTER_URL
                        },
                        &client.connection_string,
                        "master",
                    );
                    log::info!(
                        "ConnectClient -> using WebSocket relay for {} (no TCP or WASM build)",
                        client.connection_string
                    );
                    match ewebsock::connect(&url, Default::default()) {
                        Ok((ws_sender, ws_receiver)) => {
                            Some(AdminTransport::from_ws(ws_sender, ws_receiver))
                        }
                        Err(error) => {
                            client.connected = false;
                            log::error!("Failed to connect to {:?}: {}", &url, error);
                            this.error = format!("WebConsole Error -> {error}");
                            None
                        }
                    }
                };

                // Phase 1 transport selection: prefer direct TCP when the
                // client has published `local_ip` + `tcp_port` (native only;
                // WASM admin uses WebSocket relay).
                #[cfg(not(target_arch = "wasm32"))]
                let transport = match (client.local_ip.as_deref(), client.tcp_port) {
                    (Some(ip), Some(port)) if !ip.is_empty() => {
                        let target = format!("{ip}:{port}");
                        log::info!(
                            "ConnectClient -> attempting direct TCP to {target} for {}",
                            client.connection_string
                        );
                        AdminTransport::from_tcp(target, client.connection_string.clone())
                    }
                    _ => {
                        if let Some(t) = connect_via_ws(self, &mut client) {
                            t
                        } else {
                            return;
                        }
                    }
                };

                #[cfg(target_arch = "wasm32")]
                let transport = if let Some(t) = connect_via_ws(self, &mut client) {
                    t
                } else {
                    return;
                };

                client.connected = true;
                let mut ws_client = WebSocketClient::new(
                    transport,
                    client.clone(),
                    self.filesystem.clone(),
                );

                #[cfg(not(target_arch="wasm32"))]
                ws_client.start_receiving_buffers();

                // Automatically start the resource monitor stream so live
                // CPU/RAM/GPU stats appear in both the admin console and the
                // My Tasks connected-client cards without the admin needing to
                // manually click the "Charts" button.
                let _ = ws_client.send_cmd_tx.try_send(Cmd::LiveData);
                ws_client.live_stats_active = true;

                self.ws_clients
                    .entry(client.connection_string.clone())
                    .or_insert(ws_client);

                self.error = format!("WebConsole -> Connected to {}", client.connection_string);
            },
            ClientUiAction::ExportHistory(mut client) => {
                if let Some(ws_client) = self.ws_clients.get(&client.connection_string) {
                    client.export_logs(ws_client.history.clone());
                }
            },
            ClientUiAction::RelinkCustomer(client) => {
                self.relink_popup = Some(super::RelinkClientPopup::new(client));
            },
        }
    }
}

pub trait ClientHandler { 
    fn export_logs(&mut self, history: Vec<History>);
    fn delete_client(&mut self);
    fn disconnect_client(&mut self);
}

impl ClientHandler for ConnectedClient {
    fn export_logs(&mut self, history: Vec<History>) {
        let id = self.id.clone();
        PlatformSpawner::spawn(async move {
            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("history", Some(history.clone())).await.unwrap();
            let query = "UPDATE $id SET command_history += $history";
            let update_history: Result<_, surrealdb::Error> = DATABASE
                .query(query)
                .await;

            log::info!("History Response: {update_history:?}");
            log::info!("History: {:#?}", history.clone());
        });
     }

    fn delete_client(&mut self) {
        let id = self.id.clone();
        PlatformSpawner::spawn(async move {
            let update_history: Result<Option<Record>, surrealdb::Error> = DATABASE
                .delete((CONNECTED_CLIENT_TABLE, id.key_string()))
                .await;

            log::info!("History: {update_history:#?}");
        });
     }

    fn disconnect_client(&mut self) {
        let id = self.id.clone();
        PlatformSpawner::spawn(async move {
            let update_history: Result<_, surrealdb::Error> = DATABASE
                .query("UPDATE $id SET connected = false")
                .bind(("id", id))
                .await;

            log::info!("History: {update_history:#?}");
        });
     }
}