use database::schema::utilities::get_connected_clients;

use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use super::{PageState, WebconsoleTab};

impl <'a> ActionHandler for WebconsoleTab <'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("WebconsoleTab".to_string()) // Unique ID for the tab
    }
    
    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            WidgetId("GetClients".to_string()),
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id: _ } => {}
            WidgetEvent::ButtonClick { widget_id, button: _} => {
                match widget_id.0.as_str() {
                    "GetClients" => {
                        let tx = self.connected_clients_tx.clone();
                        tokio::spawn(async move {
                            match get_connected_clients(tx).await {
                                Ok(_) => log::info!("web_console/mod.rs -> get_connected_clients ran ok"),
                                Err(e) => log::warn!("web_console/mod.rs -> get_connected_clients error: {e:?}"),
                            }
                        });
                    }
                    _ => {
                        if let Some((connection_string, _)) = self.ws_clients.iter().find(|(_, btn)| btn.get_widget_id().0 == widget_id.0) {
                            // Switch page state and start WebSocket for the selected client
                            self.page_state = PageState::RemoteTerminal(connection_string.clone());
                            self.start_remote_websocket(connection_string.clone());
                        }
                        // log::info!("Received Connection Command");
                        // let url = format!(
                        //     "{WS_MASTER_URL}&room_id={}",
                        //     client.connection_string.clone()
                        // );
                        // match ewebsock::connect(&url, Default::default()) {
                        //     Ok((ws_sender, ws_receiver)) => {
                        //         client.connected = true;
        
                        //         let ws_client = WebSocketClient::new(
                        //             ws_sender,
                        //             ws_receiver,
                        //             client.clone(),
                        //             self.filesystem.clone(),
                        //         );
                                
                        //         self.ws_clients
                        //             .entry(client.connection_string.clone())
                        //             .or_insert(ws_client);
        
                        //         self.error = format!("WebConsole -> Connected to server");
                        //     }
                        //     Err(error) => {
                        //         client.connected = false;
                        //         log::info!("Failed to connect to {:?}: {}", &url, error.clone());
                        //         self.error = format!("WebConsole Error -> {error}");
                        //     }
                        // };
                    }
                }
            }
            _ => {}
        }
    }
}