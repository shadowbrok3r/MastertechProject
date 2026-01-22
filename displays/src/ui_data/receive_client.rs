use database::{live_data::{handle_live_delete_client, update_or_insert_client}, schema::{ConnectedClient, UserAuthorization}};
use crate::{app_state::SharedContext, ui_tools::toasts::{Toast, ToastKind, ToastOptions, ToastStyle}};
use eframe::egui::{Color32, RichText};
use std::collections::BTreeMap;
use database::live_data::Action;



impl SharedContext {
    pub fn receive_client(&mut self) {
        // Process only ONE notification per frame to avoid duplicates
        if let Ok((action, new_client)) = self.live_clients_rx.try_recv() {
            log::info!("new_client: {action:?} // connected={} // {}", new_client.connected, new_client.connection_string);

            // Check if this is a meaningful change (Create action or actual connection state change)
            let should_notify = if let (Some(usr), Some(current_user)) = (&new_client.assigned_user, &self.current_user) {
                let is_root = if let Some(user) = &self.current_user {
                    user.get_authorization() == UserAuthorization::Root
                } else {
                    false
                };
                
                if usr == &current_user.get_id() || is_root {
                    // For Create action, always notify (new client)
                    // For Update action, only notify if connection state actually changed
                    // For Delete action, always notify (client removed)
                    match action {
                        Action::Create => true,
                        Action::Update => {
                            // Check if this client's connection state changed
                            let old_connected = self.clients.iter()
                                .find(|c| c.connection_string == new_client.connection_string)
                                .map(|c| c.connected);
                            
                            // Only notify if connected state actually changed
                            old_connected != Some(new_client.connected)
                        }
                        Action::Delete => true,
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if should_notify {
                let toast = &mut self.toasts;
                
                // Use the `connected` field to determine the message, not the action
                let (txt, kind) = if action == Action::Delete {
                    (
                        RichText::new(format!("Client removed: {}", &new_client.connection_string))
                            .color(Color32::LIGHT_RED),
                        ToastKind::Warning
                    )
                } else if new_client.connected {
                    (
                        RichText::new(format!("Client connected: {}", &new_client.connection_string))
                            .color(Color32::LIGHT_GREEN),
                        ToastKind::Success
                    )
                } else {
                    (
                        RichText::new(format!("Client disconnected: {}", &new_client.connection_string))
                            .color(Color32::LIGHT_RED),
                        ToastKind::Warning
                    )
                };
                
                let toast_opts = ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(5.0);

                let client_toast = Toast {
                    kind,
                    text: txt.into(),
                    options: toast_opts,
                    style: ToastStyle::default(),
                };

                toast.add(client_toast);
            }

            match action {
                Action::Create | Action::Update => {
                    update_or_insert_client(&mut self.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Delete => {
                    handle_live_delete_client(&mut self.clients, new_client.clone()).unwrap_or(())
                }
            };
            
            // Update the admin console's client list
            let connected: Vec<ConnectedClient> = self.clients.iter().filter(|c| c.connected).cloned().collect();
            let disconnected: Vec<ConnectedClient> = self.clients.iter().filter(|c| !c.connected).cloned().collect();
            let mut client_map = BTreeMap::new();
            client_map.insert("Connected".to_string(), connected.clone());
            client_map.insert("Disconnected".to_string(), disconnected);
            self.web_console_layout.clients = connected;
            self.web_console_layout.client_map = client_map;
        }

        if let Ok(connected_clients) = self.connected_clients_rx.try_recv() {
            self.clients = connected_clients.clone();
            let mut client_map = BTreeMap::new();
            let connected = self.clients.iter().filter(|c| c.connected).cloned().collect::<Vec<ConnectedClient>>();
            let disconnected = self.clients.iter().filter(|c| c.connected == false).cloned().collect::<Vec<ConnectedClient>>();

            client_map.insert("Connected".to_string(), connected.clone());
            client_map.insert("Disconnected".to_string(), disconnected);
            self.web_console_layout.clients = connected;
            self.web_console_layout.client_map = client_map;
            // for client in self.clients.iter() {
            //     self.web_console_layout.ws_clients
            //         .entry(client.connection_string.clone())
            //         .or_insert(ws_client);
            // }
            for client in connected_clients {
                self
                    .undock_client
                    .insert(client.connection_string, false);
            }
        }
    }
}

