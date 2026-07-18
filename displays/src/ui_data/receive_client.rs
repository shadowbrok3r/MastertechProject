use database::{live_data::{handle_live_delete_client, update_or_insert_client}, schema::{ConnectedClient, ComputerData, RecordId, UserAuthorization}, db};
use crate::{app_state::SharedContext, ui_tools::toasts::{Toast, ToastKind, ToastOptions, ToastStyle}, PlatformSpawner, Spawner};
use eframe::egui::{Color32, RichText};
use std::collections::BTreeMap;
use database::live_data::Action;



impl SharedContext {
    pub fn receive_client(&mut self) {
        self.drain_resolved_client_customers();

        // Process only ONE notification per frame to avoid duplicates
        if let Ok((action, new_client)) = self.live_clients_rx.try_recv() {
            log::debug!("new_client: {action:?} // connected={} // {}", new_client.connected, new_client.connection_string);

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
                        Action::Create => false,
                        Action::Update => {
                            // Check if this client's connection state changed
                            let old_connected = self.clients.iter()
                                .find(|c| c.connection_string == new_client.connection_string)
                                .map(|c| c.connected);
                            
                            // Only notify if connected state actually changed
                            old_connected != Some(new_client.connected)
                        }
                        Action::Delete => false,
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
                    .duration_in_seconds(2.0);

                let client_toast = Toast {
                    kind,
                    text: txt.into(),
                    options: toast_opts,
                    style: ToastStyle::default(),
                    ..Default::default()
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

            // Update the admin console's client list.
            //
            // `web_console_layout.clients` used to be just the
            // `connected == true` subset, which meant a client that
            // flipped to false (intentional shutdown, transient TCP
            // blip the live query then missed) effectively vanished
            // from the UI.  Match the philosophy of
            // `should_show_connected_client_in_summaries` instead:
            // include every assigned row in the rendered list and let
            // the per-card status dot tell the operator which ones are
            // online.  `client_map` keeps the connected/disconnected
            // split for callers that still need it (sidebar grouping).
            let connected: Vec<ConnectedClient> = self.clients.iter().filter(|c| c.connected).cloned().collect();
            let disconnected: Vec<ConnectedClient> = self.clients.iter().filter(|c| !c.connected).cloned().collect();
            let mut client_map = BTreeMap::new();
            client_map.insert("Connected".to_string(), connected.clone());
            client_map.insert("Disconnected".to_string(), disconnected);
            self.web_console_layout.clients = self.clients.clone();
            self.web_console_layout.client_map = client_map;

            // Refresh the prober's shared snapshot so the next probe
            // round (every 30 s) sees the same fresh data without
            // re-querying SurrealDB. Lock is short and uncontended.
            if let Ok(mut guard) = self.clients_for_prober.lock() {
                *guard = self.clients.clone();
            }

            self.resolve_missing_client_customers();
        }

        if let Ok(connected_clients) = self.connected_clients_rx.try_recv() {
            self.clients = connected_clients.clone();
            let mut client_map = BTreeMap::new();
            let connected = self.clients.iter().filter(|c| c.connected).cloned().collect::<Vec<ConnectedClient>>();
            let disconnected = self.clients.iter().filter(|c| c.connected == false).cloned().collect::<Vec<ConnectedClient>>();

            client_map.insert("Connected".to_string(), connected.clone());
            client_map.insert("Disconnected".to_string(), disconnected);
            // Same loosening as the live-update path above: render every
            // row, not just `connected == true`, so a transiently-offline
            // client doesn't vanish from the UI between heartbeats.
            self.web_console_layout.clients = self.clients.clone();
            self.web_console_layout.client_map = client_map;

            // Same snapshot refresh as above — this path runs on the
            // one-shot `refresh_client_list` fetch and on the per-store
            // `connected_clients_rx` push.
            if let Ok(mut guard) = self.clients_for_prober.lock() {
                *guard = self.clients.clone();
            }

            self.resolve_missing_client_customers();
        }
    }

    /// Batch-resolves the customer of clients whose `customer` is null but
    /// whose linked `computer` carries one, pushing results to the drain.
    fn resolve_missing_client_customers(&mut self) {
        let pending: Vec<(String, RecordId)> = self
            .clients
            .iter()
            .filter(|c| {
                c.customer.is_none()
                    && c.computer.is_some()
                    && !c.connection_string.trim().is_empty()
                    && !self.client_customer_resolving.contains(&c.connection_string)
            })
            .filter_map(|c| {
                c.computer
                    .clone()
                    .map(|comp| (c.connection_string.clone(), comp))
            })
            .collect();
        if pending.is_empty() {
            return;
        }

        for (cs, _) in pending.iter() {
            self.client_customer_resolving.insert(cs.clone());
        }
        let tx = self.client_customer_resolved_tx.clone();
        PlatformSpawner::spawn(async move {
            let mut resolved: Vec<(String, RecordId)> = Vec::new();
            for (cs, computer_id) in pending {
                let computer: Result<Option<ComputerData>, surrealdb::Error> =
                    db().select(computer_id).await;
                if let Ok(Some(comp)) = computer {
                    if let Some(customer) = comp.customer {
                        resolved.push((cs, customer));
                    }
                }
            }
            if !resolved.is_empty() {
                let _ = tx.try_send(resolved);
            }
        });
    }

    /// Applies resolved customers in-memory only (never written back to the
    /// DB), then re-syncs the admin console views and prober snapshot.
    fn drain_resolved_client_customers(&mut self) {
        let mut touched = false;
        while let Ok(resolved) = self.client_customer_resolved_rx.try_recv() {
            for (cs, customer) in resolved {
                self.client_customer_resolving.remove(&cs);
                if let Some(client) = self
                    .clients
                    .iter_mut()
                    .find(|c| c.connection_string == cs && c.customer.is_none())
                {
                    client.customer = Some(customer);
                    touched = true;
                }
            }
        }
        if !touched {
            return;
        }

        let mut client_map = BTreeMap::new();
        let connected = self.clients.iter().filter(|c| c.connected).cloned().collect::<Vec<ConnectedClient>>();
        let disconnected = self.clients.iter().filter(|c| !c.connected).cloned().collect::<Vec<ConnectedClient>>();
        client_map.insert("Connected".to_string(), connected);
        client_map.insert("Disconnected".to_string(), disconnected);
        self.web_console_layout.clients = self.clients.clone();
        self.web_console_layout.client_map = client_map;

        if let Ok(mut guard) = self.clients_for_prober.lock() {
            *guard = self.clients.clone();
        }
    }
}

