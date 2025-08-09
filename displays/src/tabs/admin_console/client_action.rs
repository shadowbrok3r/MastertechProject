use super::{client_interface::tabs::command_shell::History, AdminConsole};
use database::{schema::{Record, CONNECTED_CLIENT_TABLE}, DATABASE, WS_MASTER_URL_LOCAL};
use crate::tabs::admin_console::client_interface::WebSocketClient;
use database::{WS_MASTER_URL, schema::ConnectedClient};
use crate::{PlatformSpawner, Spawner};

pub enum ClientUiAction {
    UndockClient(String),
    DeleteClient(ConnectedClient),
    ConnectClient(ConnectedClient),
    ExportHistory(ConnectedClient)
}

impl AdminConsole {
    pub fn handle_action(&mut self, action: ClientUiAction) {
        match action {
            ClientUiAction::UndockClient(connection_string) => {
                if let Some(docked) = self.undock_client.get_mut(&connection_string) {
                    *docked = !*docked; // Toggle the state
                    self.wants_to_undock = false; // Reset intent after toggle
                } else {
                    self.undock_client.insert(connection_string, false); // New client starts undocked
                    self.wants_to_undock = false; // No intent to undock yet
                }
            },
            ClientUiAction::DeleteClient(mut client) => {
                client.disconnect_client();
                if let Some(mut ws_client) = self.ws_clients.remove(&client.connection_string) {
                    ws_client.ws_sender.close();
                    drop(ws_client);
                }
                self.error = format!("WebConsole -> Client {} Deleted", client.connection_string.clone());
            },
            ClientUiAction::ConnectClient(mut client) => {
                self.open_menu = false;
                self.undock_client.insert(client.connection_string.clone(), false);
                log::info!("Received Connection Command");
                let url = format!(
                    "{}&room_id={}",
                    if cfg!(debug_assertions) {WS_MASTER_URL_LOCAL} else {WS_MASTER_URL},
                    client.connection_string.clone()
                );
                match ewebsock::connect(&url, Default::default()) {
                    Ok((ws_sender, ws_receiver)) => {
                        client.connected = true;

                        let mut ws_client = WebSocketClient::new(
                            ws_sender,
                            ws_receiver,
                            client.clone(),
                            self.filesystem.clone(),
                        );

                        ws_client.start_receiving_buffers();
                        
                        self.ws_clients
                            .entry(client.connection_string.clone())
                            .or_insert(ws_client);

                        self.error = format!("WebConsole -> Connected to server");
                    }
                    Err(error) => {
                        client.connected = false;
                        log::error!("Failed to connect to {:?}: {}", &url, error.clone());
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
            let update_history: Result<surrealdb::Response, surrealdb::Error> = DATABASE
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
                .delete((CONNECTED_CLIENT_TABLE, id.key().to_string()))
                .await;

            log::info!("History: {update_history:#?}");
        });
     }

    fn disconnect_client(&mut self) {
        let id = self.id.clone();
        PlatformSpawner::spawn(async move {
            let update_history: Result<surrealdb::Response, surrealdb::Error> = DATABASE
                .query("UPDATE $id SET connected = false")
                .bind(("id", id))
                .await;

            log::info!("History: {update_history:#?}");
        });
     }
}