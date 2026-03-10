use displays::{app_state::AppState, tabs::ai_playground::ChatThread, ui_tools::{decode_style, encode_style, toasts::{ToastStyle, Toast, ToastKind, ToastOptions}}};
use displays::{tabs::admin_console::AdminConsole, ui_tools::theme_config::set_custom_style};
use eframe::{egui::{Color32, Context, Margin, Stroke, Style, Vec2, Window}, Frame};
use crate::{app_state::MtechServer, webworker::decode_task_payload};
use std::{collections::HashMap, sync::Arc};
use wasm_bindgen_futures::spawn_local;
use egui_dock::DockState;
use database::DATABASE;
#[cfg(target_arch="wasm32")]
use crate::app_state::check_authentication;

impl MtechServer {    
    pub fn first_run(&mut self, ctx: &Context, frame: &mut Frame) {
        self.shared_ctx.first_run = false;
        let current_version = env!("CARGO_PKG_VERSION");
        match serde_json::from_str::<Style>(displays::STYLE) {
            Ok(theme) => {
                let style = Arc::new(theme);
                ctx.set_style(style);
            }
            Err(e) => log::error!("Error setting theme: {e:?}")
        };
        
        if let Some(storage) = frame.storage_mut() {
            gloo_console::info!("We have Storage Mut Access");
            // Get existing chats a user has with ChatGPT
            if let Some(chat_history) = storage.get_string("chat_history") {
                // info!("chat_history: {chat_history:?}");
                let chat_threads: HashMap<String, ChatThread> = serde_json::from_str(&chat_history).unwrap_or_default();
                // info!("chat_threads: {chat_threads:?}");
                if let Some((nth, _)) = chat_threads.iter().nth(0) {
                    self.shared_ctx.ai_playground.selected_thread = nth.to_string();
                }
                self.shared_ctx.ai_playground.set_threads(chat_threads);
            }

            // if let Some(service_map) = storage.get_string("service_data") {
            //     match serde_json::from_str::<HashMap<String, PrestashopPayload>>(&service_map) {
            //         Ok(map) => {
            //             for (key, v) in map.iter() {
            //                 if let Some(k) = self.shared_ctx.task_audit_table.service_map.get_mut(key) {
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

            if let Some(user) = self.shared_ctx.current_user.as_ref() {
                ctx.set_style(
                    decode_style(&user.get_color_scheme()
                )
                .unwrap_or_else(|e| {
                    log::error!("Error setting theme: {e:?}");
                    serde_json::from_str::<Style>(displays::STYLE).unwrap_or_default()
                }));
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
                let custom_style = set_custom_style(&self.shared_ctx.theme_config);
                ctx.set_style((*custom_style).clone());
            }
            
            if let Some(version) = storage.get_string("version") {
                if current_version != version {
                    gloo_console::info!("1 Mismatched Cargo Version. Doing update");
                    self.invalidate();
                }
            }
            //     } else {
            //         if let Some(user) = self.shared_ctx.current_user.as_ref() {
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
            //     if let Some(user) = self.shared_ctx.current_user.as_ref() {
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
        match check_authentication(self.shared_ctx.db_tx.clone()) {
            Ok(state) => {
                log::info!("1");
                if let AppState::NoAuth(reason) = &state {
                    use displays::ui_tools::toasts::ToastStyle;

                    let toast = &mut self.shared_ctx.toasts;
    
                    let error_toast = Toast {
                        kind: ToastKind::Error,
                        text: format!("Message from Database: {reason}").into(),
                        style: ToastStyle::default(),
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
                self.shared_ctx.app_state_tx.try_send(state);
            }
            Err(e) => {
                log::info!("2");
                log::error!("Error with auth: {e:?}");
                self.shared_ctx.state = AppState::NoAuth(e.to_string());
                self.shared_ctx.current_user = None;
            }
        };

        // use displays::Spawner;
        // displays::PlatformSpawner::spawn(async move {
        //     let results = database::test_database_wasm().await;
        //     gloo_console::info!(format!("Results: {results:?}"));
        // });
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
        self.shared_ctx.state = AppState::NoAuth(logout_msg.clone());
        let _ = self.shared_ctx.app_state_tx.try_send(AppState::NoAuth(logout_msg));
        let toast = &mut self.shared_ctx.toasts;

        let error_toast = Toast {
            kind: ToastKind::Error,
            style: ToastStyle::default(),
            text: format!("Detected older crate version").into(),
            options: ToastOptions::default().show_progress(true).duration_in_seconds(10.0),
        };
        toast.add(error_toast);
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        // do some initial setting up
        if self.shared_ctx.first_run { self.first_run(ctx, frame); }
        
        self.shared_ctx.receive_shared(frame, ctx);
        
        // Handle live query connection errors - reconnect if needed (WASM only)
        #[cfg(target_arch = "wasm32")]
        if self.shared_ctx.needs_reconnect {
            log::info!("Reconnecting due to live query connection error...");
            self.shared_ctx.needs_reconnect = false;
            
            // Re-run check_authentication which will reconnect and trigger load_data
            let db_tx = self.shared_ctx.db_tx.clone();
            match check_authentication(db_tx) {
                Ok(state) => {
                    log::info!("Reconnection initiated, new state: {:?}", state);
                }
                Err(e) => {
                    log::error!("Failed to initiate reconnection: {:?}", e);
                }
            }
        }
        
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.shared_ctx.db_rx.try_recv() {
            ctx.request_repaint();
            let tx = self.shared_ctx.app_state_tx.clone();
            match db {
                Ok(db) => {
                    log::info!("3");
                    
                    if self.shared_ctx.current_user.is_none() && db.user.is_some() {
                        let login_mut = self.shared_ctx.login_mut();
                        if login_mut.is_some() {
                            self.shared_ctx.state = AppState::Authenticated(displays::app_state::MainPages::Tasks);
                        } else {
                            log::error!("No login mut");
                        }
                        log::info!("10");
                        self.shared_ctx.current_user = db.user;
                    } else {
                        log::info!("11");
                    }

                    let usr = self.shared_ctx.current_user.clone();
                    if let Some(user) = usr {
                        self.shared_ctx.load_data(ctx, &user);
                        let _ = self.shared_ctx.app_state_tx.try_send(AppState::Authenticated(displays::app_state::MainPages::Tasks));
                    } else {
                        self.shared_ctx.first_run = true;
                        self.first_run(ctx,frame);
                        log::error!("1");
                        self.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    }
                    
                    if let Some(token) = db.jwt.clone() {
                        self
                        .bridge
                        .send(
                            crate::webworker::Input(token)
                        );
                    } else { log::info!("No token"); }
                }
                Err(e) => {
                    log::info!("6");
                    if e.to_string().contains("Already connected") {
                        log::info!("7");
                        let usr = self.shared_ctx.current_user.clone();
                        if let Some(user) = usr {
                            ctx.set_style(decode_style(&user.get_color_scheme()).unwrap_or_default());
                            self.shared_ctx.load_data(ctx, &user);
                            let _ = self.shared_ctx.app_state_tx.try_send(AppState::Authenticated(displays::app_state::MainPages::Tasks));
                            let toast = &mut self.shared_ctx.toasts;
                            let auth_toast = Toast {
                                style: ToastStyle::default(),
                                kind: ToastKind::Success,
                                text: format!("{e:?}").into(),
                                options: ToastOptions::default()
                                    .show_progress(true)
                                    .duration_in_seconds(6.0),
                            };
                            toast.add(auth_toast);
                        } else {
                            self.shared_ctx.first_run = true;
                            self.first_run(ctx, frame);
                            log::error!("1");
                            self.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        }
                    } else {
                        log::info!("8");
                        log::info!("{e:?}");
                        #[cfg(target_arch = "wasm32")]
                        {
                            wasm_cookies::delete("jwt");
                            wasm_cookies::delete("user");
                        }
                        // eframe::web::storage::local_storage_get(key)
                        let toast = &mut self.shared_ctx.toasts;
                        let auth_toast = Toast {
                            style: ToastStyle::default(),
                            kind: ToastKind::Error,
                            text: format!("{e:?} \nYou may need to login again").into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                        };
                        toast.add(auth_toast);
                        let _ = tx.try_send(AppState::NoAuth("Needs login".to_string()));
                    }
                }
            }
        }
    
        // most important part of the whole app.. setting up our styling
        // currently this just sets the style of the app, but in the near
        // future i will be making this the setup to allow user customization
        // to the style of any part of the app
        let theme_res = Window::new("Theme Configuration")
        .open(&mut self.shared_ctx.modify_theme)
        .max_height(600.)
        .min_width(700.)
        .title_bar(true)
        .show(ctx, |ui| {
            self.shared_ctx.theme_config.edit_ui(ui, ctx, self.shared_ctx.settings_sender.clone())
        });
        
        if let Some(window_res) = theme_res {
            if let Some(r) = window_res.inner {
                if r.0 {
                    if let Some(user) = self.shared_ctx.current_user.clone().as_mut() {
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
                    self.shared_ctx.theme = r.1;
                    self.shared_ctx.modify_theme = false;
                }
            }
        }

        // let received_completed_tasks = &mut false;
        // Getting responses from our webworker
        if let Some(items) = self.data_update.take() {
            let tx = self.shared_ctx.initial_tasks_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                // log::info!("Got data update from webworker: {:?}", items.len());
                let _ = tx.try_send(decode_task_payload(&items).unwrap_or_default());
            });
            // *received_completed_tasks = true;
        }

        // if *received_completed_tasks { self.bridge }

        // if let Some(decompressed_data) = self.admin_console_data_helper.deser_data_update.take() {
        //     if let Some(sysinfo) = deserializer::<SystemInformation>(&decompressed_data){
        //         info!("Got sysinfo from admin console");
        //         self.shared_ctx. resource_mon.set_sysinfo(sysinfo);
        //     }
        // }

        if self.shared_ctx.web_console_layout.wants_to_undock {
            let layout = &mut self.shared_ctx.web_console_layout;
            let undock_client = layout.undock_client.clone();
            for client in self.shared_ctx.clients.clone() {
                let should_we_undock = if let Some(undock) = undock_client.get(&client.connection_string)
                {
                    undock
                } else {
                    &false
                };

                if *should_we_undock {
                    let is_ws_connected = layout.ws_clients
                        .get(&client.connection_string)
                        .map(|wsc| wsc.is_connected && wsc.last_pong_time.is_some())
                        .unwrap_or(false);
                    
                    let color = if is_ws_connected {
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
                                
                                ui.horizontal(|ui| AdminConsole::client_header(ui, tx, &client.clone(), undock_client.clone(), is_ws_connected));
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
        if let Some(user) = &self.shared_ctx.current_user {
            if self.shared_ctx.get_settings {
                self.shared_ctx.get_settings = false;
                match serde_json::from_value::<DockState<String>>(user.get_user_settings().get_ui_layout_mtechserver()){
                    Ok(tree) => self.shared_ctx.tree = tree,
                    Err(e) => log::error!("Could not get UI layout from user: {e:?}: {:#?}", user.get_user_settings().get_ui_layout_mtechserver()),
                }
            } 
        }

        // Get User settings from local storage
        // this bool gets switched via clicking
        // the submit button in the crate::tabs::json_viewer
        // module
        if self.shared_ctx.update_settings {
            self.shared_ctx.update_settings = false;
            log::info!("Saving settings: {:?}", self.shared_ctx.user_settings.clone());
            frame.storage_mut().unwrap().set_string(
                "user_settings",
                serde_json::to_string(&self.shared_ctx.user_settings).unwrap(),
            );
        }

        if self.shared_ctx.ai_playground.save_chats {
            self.shared_ctx.ai_playground.save_chats = false;
            if let Some(_usr) = &self.shared_ctx.current_user {
                let threads = self.shared_ctx.ai_playground.get_threads();
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
