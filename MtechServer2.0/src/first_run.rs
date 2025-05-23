use displays::{tabs::ai_playground::ChatThread, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use crate::app_state::{AppState, MtechServer};
use wasm_bindgen_futures::spawn_local;
use std::collections::HashMap;
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
        let current_version = env!("CARGO_PKG_VERSION");

        if let Some(storage) = frame.storage_mut() {
            gloo_console::info!("We have Storage Mut Access");
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

            if let Some(_service_map) = storage.get_string("service_data") {
                // match serde_json::from_str::<HashMap<String, DataTable<PrestashopPayload>>>(&service_map) {
                //     Ok(map) => self.context.shared_ctx.task_audit_table.service_map = map,
                //     Err(e) => log::error!("Error converting service_map: {e:?}"),
                // }
            }

            if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
                gloo_console::info!("2 We have a user");
                let user_version = user.get_version();
                gloo_console::info!(format!("2 current_version: {current_version}\nuser_version: {user_version}"));
                if let Some(version) = storage.get_string("version") {
                    if (current_version != version) || (current_version != user_version) {
                        gloo_console::info!("1 Mismatched Cargo Version. Doing update");
                        self.invalidate();
                    } else {
                        let mut usr = user.clone();
                        let v = current_version;
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = usr.save_version(v).await;
                            gloo_console::info!(format!("Saving user version: {res:?}"));
                        });
                    }
                } else {
                    if current_version != user_version {
                        gloo_console::info!("3 Mismatched Cargo Version. Doing update");
                        self.invalidate();
                    } else {
                        let mut usr = user.clone();
                        let v = current_version;
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = usr.save_version(v).await;
                            gloo_console::info!(format!("Saving user version: {res:?}"));
                        });
                    }
                }
            } else if let Some(version) = storage.get_string("version") {
                if current_version != version {
                    gloo_console::info!("1 Mismatched Cargo Version. Doing update");
                    self.invalidate();
                }
            }
            //     } else {
            //         if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
            //             gloo_console::info!("1 We have a user");
            //             let current_version = env!("CARGO_PKG_VERSION");
            //             let user_version = user.get_version();
            //             gloo_console::info!(format!("1 current_version: {version}\nuser_version: {user_version}"));
            //             if current_version != user_version {
            //                 gloo_console::info!("2 Mismatched Cargo Version. Doing update");
            //                 self.invalidate();
            //             } else {
            //                 let mut usr = user.clone();
            //                 let v = current_version;
            //                 wasm_bindgen_futures::spawn_local(async move {
            //                     let res = usr.save_version(v).await;
            //                     gloo_console::info!(format!("Saving user version: {res:?}"));
            //                 });
            //             }
            //         }
            //     }
            // } // else {
            //     if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
            //         gloo_console::info!("2 We have a user");
            //         let user_version = user.get_version();
            //         gloo_console::info!(format!("2 current_version: {current_version}\nuser_version: {user_version}"));
            //         if current_version != user_version {
            //             gloo_console::info!("3 Mismatched Cargo Version. Doing update");
            //             self.invalidate();
            //         } else {
            //             let mut usr = user.clone();
            //             let v = current_version;
            //             wasm_bindgen_futures::spawn_local(async move {
            //                 let res = usr.save_version(v).await;
            //                 gloo_console::info!(format!("Saving user version: {res:?}"));
            //             });
            //         }
            //     } else {
            //         gloo_console::error!("No user");
            //         storage.set_string(
            //             "version",
            //             env!("CARGO_PKG_VERSION").to_string()
            //         );
            //     }
            // }
        }

        #[cfg(target_arch="wasm32")]
        match check_authentication(self.context.db_tx.clone()) {
            Ok(d) => {
                log::info!("1");
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
                        self.context.shared_ctx.web_console_layout.filesystem.set_user(usr.clone());
                        spawn_local(async move {
                            match DATABASE.health().await {
                                Ok(_) => log::info!("Healthy connection"),
                                Err(e) => log::error!("Database connection health: {e:?}"),
                            }
                        });
                    }
                }
                self.state = d.0;
            }
            Err(e) => {
                log::info!("2");
                log::error!("Error with auth: {e:?}");
                self.state = AppState::NoAuth(e.to_string());
                self.context.shared_ctx.current_user = None;
            }
        };
    }

    pub fn invalidate(&mut self) {
        gloo_console::info!("Invalidating");
        #[cfg(target_arch = "wasm32")]
        {
            wasm_cookies::delete("user");
            wasm_cookies::delete("jwt");
        }

        spawn_local(async move {
            let invalidation = DATABASE.invalidate().await;
            gloo_console::info!(format!("invalidated connection: {:?}", invalidation));
        });

        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let clear = storage.clear();
                gloo_console::info!(format!("Clearing storage: {clear:?}"));
            }
            if let Ok(caches) = window.caches() {
                gloo_console::error!(format!("Caches: {:?}", caches.keys().as_string()));
                // for cache in caches.keys().then(cb)
                //     let success_closure = Closure::wrap(Box::new(move |_value: JsValue| {
                //         gloo_console::info!(format!("Initialized worker with {} threads", num_threads));
                //     }) as Box<dyn FnMut(JsValue)>);
            }
            let reload = window.location().reload();
            gloo_console::info!(format!("Reloading window: {reload:?}"));
        } else {
            gloo_console::info!(format!("No window"));
        }
        let logout_msg = "Logged out".to_string();
        self.state = AppState::NoAuth(logout_msg.clone());
        let _ = self.context.app_state_tx.try_send(AppState::NoAuth(logout_msg));
        let toast = &mut self.context.shared_ctx.toasts;

        let error_toast = Toast {
            kind: ToastKind::Error,
            text: format!("Detected older crate version").into(),
            options: ToastOptions::default().show_progress(true).duration_in_seconds(10.0),
        };
        toast.add(error_toast);
    }

    pub fn receive(&mut self) {
        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            log::debug!("Releases: {releases:?}");
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
