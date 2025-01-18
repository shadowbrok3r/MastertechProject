use database::{live_data::{handle_live_delete, update_or_insert_anything}, schema::ConnectedClient};
use crate::{app_state::SharedContext, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use eframe::egui::{Color32, RichText};
use std::collections::BTreeMap;
use surrealdb::Action;



impl SharedContext {
    pub fn receive_client(&mut self) {
        if let Ok((action, new_client)) = self.live_clients_rx.try_recv() {
            log::info!("new_client: {action:?} // {new_client:?}");

            if let (Some(usr), Some(current_user)) =
                (&new_client.assigned_user, &self.current_user)
            {
                if usr == &current_user.id {
                    let toast = &mut self.toasts;
                    let txt = match action {
                        Action::Create => RichText::new(format!(
                            "Client has connected: {}",
                            &new_client.connection_string
                        ))
                        .color(Color32::LIGHT_GREEN),
                        // Action::Update => RichText::new(
                        //     format!("Client update: {:#?}", &new_client.clone())
                        // ).color(Color32::LIGHT_BLUE),
                        Action::Delete => RichText::new(format!(
                            "Client has disconnected: {}",
                            &new_client.connection_string
                        ))
                        .color(Color32::LIGHT_RED),
                        _ => RichText::new(format!(
                            "Client has connected: {}",
                            &new_client.connection_string
                        ))
                        .color(Color32::LIGHT_GREEN),
                    };
                    let toast_opts = ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(5.0);

                    let client_connected_toast = Toast {
                        kind: ToastKind::Success,
                        text: txt.into(),
                        options: toast_opts,
                    };

                    toast.add(client_connected_toast);
                }
            }

            match action {
                Action::Create => {
                    update_or_insert_anything(&mut self.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Update => {
                    update_or_insert_anything(&mut self.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Delete => {
                    handle_live_delete(&mut self.clients, new_client.clone()).unwrap_or(())
                }
                _ => (),
            };
        }

        if let Ok(connected_clients) = self.connected_clients_rx.try_recv() {
            self.clients = connected_clients.clone();
            let mut client_map = BTreeMap::new();
            let connected = self.clients.iter().filter(|c| c.connected).cloned().collect::<Vec<ConnectedClient>>();
            let disconnected = self.clients.iter().filter(|c| c.connected == false).cloned().collect::<Vec<ConnectedClient>>();

            client_map.insert("Connected".to_string(), connected);
            client_map.insert("Disconnected".to_string(), disconnected);

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

