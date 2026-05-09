use database::schema::utilities::get_connected_clients;

use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use super::{PageState, WebconsoleTab};

impl <'a> ActionHandler for WebconsoleTab <'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("WebconsoleTab".to_string()) // Unique ID for the tab
    }
    
    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        let mut ids = vec![
            WidgetId("GetClients".to_string()),
            WidgetId("ToggleSidePanel".to_string())
        ];
        
        for (_, btn) in self.ws_clients.iter() {
            ids.push(btn.get_widget_id());
        }
        ids
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id: _ } => {}
            WidgetEvent::ButtonClick { widget_id, button: _, source: _ } => {
                if !event.is_source_me() { }

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
                    "ToggleSidePanel" => { self.show_side_panel = !self.show_side_panel; }
                    _ => {
                        if let Some((connection_string, _)) = self
                            .ws_clients
                            .iter()
                            .find(|(_, btn)| btn.get_widget_id().0 == widget_id.0) 
                        {
                            // Switch page state and start the best available connection
                            // (direct TCP when the client has published local_ip/tcp_port,
                            // WS relay otherwise).
                            self.show_side_panel = false;
                            self.page_state = PageState::RemoteTerminal(connection_string.clone());
                            self.start_remote_connection(connection_string.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
}