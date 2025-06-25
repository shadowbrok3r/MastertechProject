use displays::{app_state::AppState, tabs::ai_playground::ChatThread, ui_tools::{decode_style, encode_style, toasts::{Toast, ToastKind, ToastOptions}}};
use displays::{tabs::admin_console::AdminConsole, ui_tools::theme_config::set_custom_style};
use crate::{app_state::MtechServer, webworker::decode_task_payload};
use eframe::{egui::{Color32, Context, Margin, Stroke, Vec2, Window}, Frame};
use wasm_bindgen_futures::spawn_local;
use std::collections::HashMap;
use database::DATABASE;
use egui_dock::DockState;
#[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // use mtechserver::{webworker::Input, live_worker::LiveInput}
};

impl MtechServer {
    pub fn first_run(&mut self, ctx: &Context, frame: &mut Frame) {
        self.context.shared_ctx.first_run = false;
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

            // if let Some(service_map) = storage.get_string("service_data") {
            //     match serde_json::from_str::<HashMap<String, PrestashopPayload>>(&service_map) {
            //         Ok(map) => {
            //             for (key, v) in map.iter() {
            //                 if let Some(k) = self.context.shared_ctx.task_audit_table.service_map.get_mut(key) {
            //                     if !k.iter().contains(&v) {
            //                         log::info!("Order: {v:?}");
            //                         k.push(v.clone());
            //                     }
            //                 }
            //             }
            //         },
            //         Err(e) => log::error!("Error converting service_map: {e:?}"),
            //     }
            // }

            if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
                ctx.set_style(decode_style(&user.get_color_scheme()).unwrap_or_default());
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
            } else {
                let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
                ctx.set_style((*custom_style).clone());
            }
            
            if let Some(version) = storage.get_string("version") {
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
        match check_authentication(self.context.shared_ctx.db_tx.clone()) {
            Ok(state) => {
                log::info!("1");
                if let AppState::NoAuth(reason) = &state {
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
                    spawn_local(async move {
                        match DATABASE.health().await {
                            Ok(_) => log::info!("Healthy connection"),
                            Err(e) => log::error!("Database connection health: {e:?}"),
                        }
                    });
                }
                self.context.shared_ctx.app_state_tx.try_send(state);
            }
            Err(e) => {
                log::info!("2");
                log::error!("Error with auth: {e:?}");
                self.context.shared_ctx.state = AppState::NoAuth(e.to_string());
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
        self.context.shared_ctx.state = AppState::NoAuth(logout_msg.clone());
        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth(logout_msg));
        let toast = &mut self.context.shared_ctx.toasts;

        let error_toast = Toast {
            kind: ToastKind::Error,
            text: format!("Detected older crate version").into(),
            options: ToastOptions::default().show_progress(true).duration_in_seconds(10.0),
        };
        toast.add(error_toast);
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        // do some initial setting up
        if self.context.shared_ctx.first_run { self.first_run(ctx, frame); }
        self.receive_database(frame, ctx);
        self.context.shared_ctx.receive_shared(frame, ctx);

        // most important part of the whole app.. setting up our styling
        // currently this just sets the style of the app, but in the near
        // future i will be making this the setup to allow user customization
        // to the style of any part of the app
        let theme_res = Window::new("Theme Configuration")
        .open(&mut self.context.shared_ctx.modify_theme)
        .max_height(600.)
        .min_width(700.)
        .title_bar(true)
        .show(ctx, |ui| {
            self.context.shared_ctx.theme_config.edit_ui(ui, ctx, self.context.shared_ctx.settings_sender.clone())
        });
        
        if let Some(window_res) = theme_res {
            if let Some(r) = window_res.inner {
                if r.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.set_color_scheme(encode_style(&r.1.clone()).unwrap_or_default());
                        ctx.set_style(r.1.clone());
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&user.get_user_settings()).unwrap_or_default());
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            wasm_cookies::delete("user");
                            let duration = web_time::Duration::from_secs(172800);
                            let usr = serde_json::to_string(&user.clone()).unwrap();
                            let cookie_opts = wasm_cookies::CookieOptions::default()
                                .with_same_site(wasm_cookies::SameSite::Strict)
                                .secure()
                                .expires_after(duration);
                        
                            use brotli::CompressorReader;
                            use base64::{engine::general_purpose, Engine as _};

                            fn compress_string(input: &str) -> Vec<u8> {
                                let mut compressed = Vec::new();
                                {
                                    let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                                    std::io::copy(&mut compressor, &mut compressed).unwrap();
                                }
                                compressed
                            }

                            let compressed: Vec<u8> = compress_string(&usr);
                            let encoded: String = general_purpose::STANDARD.encode(&compressed);
                            log::info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr.len());
                            wasm_cookies::set("user", &encoded, &cookie_opts);
                        }
                    }
                    self.context.shared_ctx.theme = r.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            }
        }

        // if !self.context.shared_ctx.modify_theme {
        //     let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        //     ctx.set_style((custom_style).clone());
        // }

        // Getting responses from our webworker
        if let Some(items) = self.context.data_update.take() {
            let tx = self.context.shared_ctx.initial_tasks_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                // log::info!("Got data update from webworker: {:?}", items.len());
                let _ = tx.try_send(decode_task_payload(&items).unwrap_or_default());
            });
        }

        // if let Some(decompressed_data) = self.context.admin_console_data_helper.deser_data_update.take() {
        //     if let Some(sysinfo) = deserializer::<SystemInformation>(&decompressed_data){
        //         info!("Got sysinfo from admin console");
        //         self.context.shared_ctx. resource_mon.set_sysinfo(sysinfo);
        //     }
        // }

        if self.context.shared_ctx.web_console_layout.wants_to_undock {
            let layout = &mut self.context.shared_ctx.web_console_layout;
            let undock_client = layout.undock_client.clone();
            for client in self.context.shared_ctx.clients.clone() {
                let should_we_undock = if let Some(undock) = undock_client.get(&client.connection_string)
                {
                    undock
                } else {
                    &false
                };

                if *should_we_undock {
                    let color = if client.connected {
                        Color32::LIGHT_BLUE
                    } else {
                        Color32::LIGHT_RED
                    };

                    let column_frame = eframe::egui::Frame::default()
                        .fill(Color32::from_rgb(12, 12, 14))
                        .inner_margin(Margin::same(4))
                        .outer_margin(Margin::symmetric(5, 3))
                        .corner_radius(eframe::egui::CornerRadius::same(10))
                        .stroke(Stroke::new(1.0, color));

                    Window::new(&client.connection_string)
                        .frame(column_frame)
                        .min_size(Vec2::new(700., 400.))
                        .max_size(Vec2::new(1500., 900.))
                        .default_size(Vec2::new(1000., 900.))
                        .show(ctx, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                
                                let tx = layout.ui_actions_channel.0.clone();
                                
                                ui.horizontal(|ui| AdminConsole::client_header(ui, tx, &client.clone(), undock_client.clone()));
                                if let Some(ws_client) =
                                    layout.ws_clients.get_mut(&client.connection_string)
                                {
                                    ws_client.show(ui);
                                }
                            });
                        });
                }
            }
        }

        // Get User settings from local storage
        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<DockState<String>>(user.get_user_settings().get_ui_layout_mtechserver()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => log::error!("Could not get UI layout from user: {e:?}: {:#?}", user.get_user_settings().get_ui_layout_mtechserver()),
                }
            } 
        }

        // Get User settings from local storage
        // this bool gets switched via clicking
        // the submit button in the crate::tabs::json_viewer
        // module
        if self.context.update_settings {
            self.context.update_settings = false;
            log::info!("Saving settings: {:?}", self.context.user_settings.clone());
            frame.storage_mut().unwrap().set_string(
                "user_settings",
                serde_json::to_string(&self.context.user_settings).unwrap(),
            );
        }

        if self.context.shared_ctx.ai_playground.save_chats {
            self.context.shared_ctx.ai_playground.save_chats = false;
            if let Some(_usr) = &self.context.shared_ctx.current_user {
                let threads = self.context.shared_ctx.ai_playground.get_threads();
                // for (id, thread) in threads {
                    // thread.messages
                // }
                // info!("Saving chats: {:?}", threads);
                frame.storage_mut().unwrap().set_string(
                    "chat_history",
                    serde_json::to_string(&threads).unwrap(),
                );
            }
        }
    }
}
