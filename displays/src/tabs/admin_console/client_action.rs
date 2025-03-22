use database::{WS_MASTER_URL, schema::ConnectedClient};
use crate::tabs::admin_console::client_interface::WebSocketClient;

use super::{client_interface::client_handler::ClientHandler, AdminConsole};


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
                    if *docked {
                        *docked = false;
                        self.wants_to_undock = false;
                    } else {
                        *docked = true;
                        self.wants_to_undock = true;
                    }
                } else {
                    self.undock_client.insert(connection_string, true);
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
                log::info!("Received Connection Command");
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
                        log::info!("Failed to connect to {:?}: {}", &url, error.clone());
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