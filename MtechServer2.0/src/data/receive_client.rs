use crate::{
    app_state::{AppState, MtechServer},
    pages::downloads_page::get_github_releases,
    tabs::stock::{find_attached_serials, get_stock, BoolOrString, MyRowData},
    utilities::ModalType,
};
use database::{
    live_data::{handle_live_delete, listen_data, update_or_insert_anything},
    schema::{
        utilities::{get_connected_clients, get_store_users, get_tasks},
        TaskNotePayload, CONNECTED_CLIENT_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE,
    },
    DATABASE,
};
use database::{schema::Store, STORAGE_URL};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::{
    egui::{Color32, RichText},
    Frame,
};
use egui_dock::DockState;
use log::info;
use log::{debug, error};
use mtechserver::webworker::Input;
use surrealdb::{Action, RecordId};
use wasm_bindgen_futures::spawn_local;

// #[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // mtechserver::live_worker::LiveInput,
};

impl MtechServer {
    pub fn receive_client(&mut self) {
        if let Ok((action, new_client)) = self.context.live_clients_rx.try_recv() {
            info!("new_client: {action:?} // {new_client:?}");

            if let (Some(usr), Some(current_user)) =
                (&new_client.assigned_user, &self.context.current_user)
            {
                if usr == &current_user.id {
                    let toast = &mut self.context.toasts;
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
                    update_or_insert_anything(&mut self.context.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Update => {
                    update_or_insert_anything(&mut self.context.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Delete => {
                    handle_live_delete(&mut self.context.clients, new_client.clone()).unwrap_or(())
                }
                _ => (),
            };
        }

        if let Ok(connected_clients) = self.context.connected_clients_rx.try_recv() {
            self.context.clients = connected_clients.clone();
            for client in connected_clients {
                self.context
                    .undock_client
                    .insert(client.connection_string, false);
            }
        }
    }
}