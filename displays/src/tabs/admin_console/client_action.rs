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
    /// Close this admin's local session to the client and drop it from the
    /// visible list. The `connected_client` row stays in the database — only
    /// its `connected` flag flips to `false`, so the client can be re-opened
    /// later. Renamed from the misleading `DeleteClient`: this button has
    /// never hard-deleted the DB row, despite the old name and toast text.
    DisconnectClient(ConnectedClient),
    ConnectClient(ConnectedClient),
    ExportHistory(ConnectedClient),
    /// Open the re-link customer popup for this client (the
    /// used-machine-with-wrong-owner workflow). See `relink_popup.rs`.
    RelinkCustomer(ConnectedClient),
    /// Open entity-link modal to create/fix the computer FK.
    LinkComputer(ConnectedClient),
    /// Open entity-link modal focused on customer linkage.
    LinkCustomer(ConnectedClient),
    /// Run automated repair for this connection_string.
    RepairAssociations(ConnectedClient),
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
            ClientUiAction::DisconnectClient(mut client) => {
                // Drop focus first so any UI that reads `focused_client` next
                // frame doesn't dereference a connection_string we're about
                // to tear down.
                if self.focused_client.as_deref() == Some(client.connection_string.as_str()) {
                    self.focused_client = None;
                }
                // Soft-disconnect: mark the row `connected = false` in the DB
                // so the live-data feed filters this client out of the
                // connected list on its next tick. The `connected_client`
                // record itself is preserved for future reconnects — use the
                // database-level `delete_client()` path explicitly if you
                // really want the row gone.
                client.disconnect_client();
                if let Some(mut ws_client) = self.ws_clients.remove(&client.connection_string) {
                    ws_client.transport.close();
                    drop(ws_client);
                }
                self.session_layout.remove(&client.connection_string);
                self.error = format!("WebConsole -> Disconnected from {}", client.connection_string.clone());
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
            ClientUiAction::LinkComputer(client) => {
                // Fire a fresh hardware-spec request through the active session
                // so open_service_suggestions is populated before the modal polls.
                if let Some(ws) = self.ws_clients.get(&client.connection_string) {
                    let _ = ws.send_cmd_tx.try_send(Cmd::RequestOpenServiceCandidates { refresh: false });
                }
                submit_admin_entity_link(&client, "computer");
            }
            ClientUiAction::LinkCustomer(client) => {
                submit_admin_entity_link(&client, "customer");
            }
            ClientUiAction::RepairAssociations(client) => {
                let cs = client.connection_string.clone();
                crate::PlatformSpawner::spawn(async move {
                    match database::schema::entity_link::repair_connection_links(&cs).await {
                        Ok(report) => log::info!("repair_connection_links({cs}): {report}"),
                        Err(e) => log::error!("repair_connection_links({cs}): {e}"),
                    }
                });
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
fn submit_admin_entity_link(client: &ConnectedClient, focus: &str) {
    use crate::plugins::entity_link_pending::{
        submit_manual_entity_link_request, EntityLinkRequest,
    };
    use database::schema::entity_link::LinkValidationIssue;
    use database::schema::RecordIdExt;

    let mut issues = Vec::new();
    if focus == "computer" {
        issues.push(LinkValidationIssue::MissingComputer);
    } else {
        issues.push(LinkValidationIssue::MissingCustomer);
    }
    submit_manual_entity_link_request(EntityLinkRequest {
        request_id: String::new(),
        connection_string: Some(client.connection_string.clone()),
        customer_id: client
            .customer
            .as_ref()
            .map(|c| c.key_string())
            .unwrap_or_default(),
        computer_id: client
            .computer
            .as_ref()
            .map(|c| c.key_string())
            .unwrap_or_default(),
        issues,
    });
}

#[cfg(not(all(not(target_arch = "wasm32"), feature = "tokio")))]
fn submit_admin_entity_link(_client: &ConnectedClient, _focus: &str) {}

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