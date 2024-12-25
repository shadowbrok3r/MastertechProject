use crate::app_state::{AppState, MtechServer};
use displays::{egui_data_table::DataTable, tabs::{ai_playground::ChatThread, task_audit::PrestashopOrderData}, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use wasm_bindgen_futures::spawn_local;
use std::collections::HashMap;
use log::{info, debug, error};
use database::DATABASE;
use eframe::Frame;

#[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // use mtechserver::{webworker::Input, live_worker::LiveInput}
};

impl MtechServer {
    pub fn first_run(&mut self, frame: &mut Frame) {
        self.context.first_run = false;

        if let Some(storage) = frame.storage_mut() {
            // Get existing chats a user has with ChatGPT
            if let Some(chat_history) = storage.get_string("chat_history") {
                // info!("chat_history: {chat_history:?}");
                let chat_threads: HashMap<String, ChatThread> = serde_json::from_str(&chat_history).unwrap_or_default();
                // info!("chat_threads: {chat_threads:?}");
                if let Some((nth, _)) = chat_threads.iter().nth(0) {
                    self.context.shared_ctx.ai_playground.selected_thread = nth.to_string();
                }
                self.context.shared_ctx.ai_playground.set_threads(chat_threads);
            }

            if let Some(service_map) = storage.get_string("service_data") {
                match serde_json::from_str::<HashMap<String, DataTable<PrestashopOrderData>>>(&service_map) {
                    Ok(map) => self.context.shared_ctx.task_audit_table.service_map = map,
                    Err(e) => info!("Error converting service_map: {e:?}"),
                }
            }

            if let Some(version) = storage.get_string("version") {
                if env!("CARGO_PKG_VERSION") != version {
                    gloo_console::info!(format!("Mismatched Cargo Version. Doing update from {:?} to -> {:?}", version, env!("CARGO_PKG_VERSION")));
                    #[cfg(target_arch = "wasm32")]
                    {
                        wasm_cookies::delete("user");
                        wasm_cookies::delete("jwt");
                    }
                    spawn_local(async move {
                        let invalidation = DATABASE.invalidate().await;
                        info!("invalidated connection: {:?}", invalidation);
                    });

                    if let Some(window) = web_sys::window() {
                        let reload = window.location().reload();
                        info!("Reloading winodw: {reload:?}");
                        if let Ok(storage) = window.local_storage() {
                            if let Some(storage) = storage {
                                let clear = storage.clear();
                                info!("Clearing storage: {clear:?}");
                            }
                        }
                    } else {
                        info!("No window");
                    }
                    let logout_msg = "Logged out".to_string();
                    self.state = AppState::NoAuth(logout_msg.clone());
                    match self
                        .context
                        .app_state_tx
                        .try_send(AppState::NoAuth(logout_msg))
                    {
                        Ok(_) => info!("Logged out"),
                        Err(e) => error!("Error: {e:?}"),
                    }
                }
            }
            else {
                storage.set_string(
                    "version",
                    env!("CARGO_PKG_VERSION").to_string()
                );
            }
        }

        #[cfg(target_arch="wasm32")]
        match check_authentication(self.context.db_tx.clone()) {
            Ok(d) => {
                info!("1");
                if let AppState::NoAuth(reason) = &d.0 {
                    let toast = &mut self.context.shared_ctx.toasts;
    
                    let error_toast = Toast {
                        kind: ToastKind::Error,
                        text: format!("Message from Database: {reason}").into(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(6.0),
                    };
                    toast.add(error_toast);
                }else {
                    if let Some(ref usr) = d.1 {
                        self.context.shared_ctx.current_user = Some(usr.clone());
                        self.context.shared_ctx.filesystem.set_user(usr.clone());
                        spawn_local(async move {
                            match DATABASE.health().await {
                                Ok(_) => info!("Healthy connection"),
                                Err(e) => info!("Database connection health: {e:?}"),
                            }
                        });
                    }
                }
                self.state = d.0;
            }
            Err(e) => {
                info!("2");
                error!("Error with auth: {e:?}");
                self.state = AppState::NoAuth(e.to_string());
                self.context.shared_ctx.current_user = None;
            }
        };
    }

    pub fn receive(&mut self) {
        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            self.context.github_releases = releases;
        }

        if let Ok(state) = self.context.app_state_rx.try_recv() {
            gloo_console::info!(format!("Got a new state: {state:?}"));
            
            if let AppState::NoAuth(reason) = &state {
                let toast = &mut self.context.shared_ctx.toasts;

                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: reason.into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
            }
            self.state = state;
        }
    }
}
