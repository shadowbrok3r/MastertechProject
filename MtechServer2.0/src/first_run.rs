use displays::{
    app_state::AppState,
    tabs::{
        admin_console::{AdminConsole, SessionLayout},
        admin_console::client_interface::TransportKind,
    },
    ui_tools::{
        encode_style,
        theme_config::{apply_user_color_scheme, bootstrap_startup_theme},
        toasts::{Toast, ToastKind, ToastOptions, ToastStyle},
    },
};
use eframe::{egui::{Color32, Context, Margin, Stroke, Vec2, Window}, Frame};
use crate::{app_state::MtechServer, webworker::decode_task_payload};
use wasm_bindgen_futures::spawn_local;
use egui_dock::DockState;
use database::db;
#[cfg(target_arch="wasm32")]
use crate::app_state::check_authentication;

impl MtechServer {    
    pub fn first_run(&mut self, ctx: &Context, frame: &mut Frame) {
        self.shared_ctx.first_run = false;
        let current_version = env!("CARGO_PKG_VERSION");
        bootstrap_startup_theme(ctx);
        Self::sweep_stale_service_workers();
        
        if let Some(storage) = frame.storage_mut() {
            gloo_console::info!("We have Storage Mut Access");

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
                apply_user_color_scheme(ctx, &user.get_color_scheme());
                self.shared_ctx.user_theme_loaded = true;
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
                bootstrap_startup_theme(ctx);
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
                        user_dismissed: false,
                        style: ToastStyle::default(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(6.0),
                    };
                    toast.add(error_toast);
                }else {
                    spawn_local(async move {
                        match db().health().await {
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

        // Register the `document.visibilitychange` listener once. We push
        // the *current* visibility state (true=visible, false=hidden) on
        // every change so `receive_shared_logic` can time the hide. Short
        // hides are ignored; long hides (>= 45min, browser-tab-suspend
        // territory) auto-reload the page. The closure is leaked via
        // `Closure::forget` so it lives for the page's lifetime, matching
        // how every other long-lived JS callback in this app is registered.
        #[cfg(target_arch = "wasm32")]
        if !self.shared_ctx.visibility_listener_installed {
            use wasm_bindgen::{closure::Closure, JsCast};
            self.shared_ctx.visibility_listener_installed = true;
            let tx = self.shared_ctx.visibility_signal_tx.clone();
            let cb = Closure::wrap(Box::new(move |_e: web_sys::Event| {
                if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                    let is_visible =
                        doc.visibility_state() == web_sys::VisibilityState::Visible;
                    let _ = tx.try_send(is_visible);
                }
            }) as Box<dyn FnMut(_)>);
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Err(e) = doc.add_event_listener_with_callback(
                    "visibilitychange",
                    cb.as_ref().unchecked_ref(),
                ) {
                    log::error!(
                        "Failed to register visibilitychange listener: {e:?}"
                    );
                } else {
                    log::info!("visibilitychange listener installed");
                }
            }
            cb.forget();
        }

        // use displays::Spawner;
        // displays::PlatformSpawner::spawn(async move {
        //     let results = database::test_database_wasm().await;
        //     gloo_console::info!(format!("Results: {results:?}"));
        // });
    }

    /// One-shot boot sweep: unregisters any service worker left behind by an
    /// old deployed build and purges its caches. Reloads only when a
    /// registration was actually removed, so healed clients never loop.
    fn sweep_stale_service_workers() {
        spawn_local(async move {
            use js_sys::wasm_bindgen::JsCast;
            use wasm_bindgen_futures::JsFuture;

            let Some(win) = web_sys::window() else { return };
            let mut removed = false;
            let swc = win.navigator().service_worker();
            if let Ok(regs_js) = JsFuture::from(swc.get_registrations()).await {
                let regs = js_sys::Array::from(&regs_js);
                for reg_val in regs.iter() {
                    if let Ok(reg) = reg_val.dyn_into::<web_sys::ServiceWorkerRegistration>() {
                        if let Ok(promise) = reg.unregister() {
                            if JsFuture::from(promise)
                                .await
                                .ok()
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                removed = true;
                            }
                        }
                    }
                }
            }
            if removed {
                if let Ok(caches) = win.caches() {
                    if let Ok(keys_js) = JsFuture::from(caches.keys()).await {
                        let keys = js_sys::Array::from(&keys_js);
                        for key in keys.iter() {
                            if let Some(k) = key.as_string() {
                                let _ = JsFuture::from(caches.delete(&k)).await;
                            }
                        }
                    }
                }
                gloo_console::warn!("Stale service worker removed — reloading for a clean boot");
                let _ = win.location().reload();
            }
        });
    }

    pub fn invalidate(&mut self) {
        gloo_console::info!("Invalidating");
        #[cfg(target_arch = "wasm32")]
        {
            wasm_cookies::delete("user");
            wasm_cookies::delete("jwt");
        }

        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let clear = storage.clear();
                gloo_console::info!(format!("Clearing localStorage: {clear:?}"));
            }
            if let Ok(Some(storage)) = window.session_storage() {
                let clear = storage.clear();
                gloo_console::info!(format!("Clearing sessionStorage: {clear:?}"));
            }
        }

        // Async teardown, reload only after every step completed: DB session,
        // CacheStorage, and any lingering service-worker registrations.
        spawn_local(async move {
            use js_sys::wasm_bindgen::JsCast;
            use wasm_bindgen_futures::JsFuture;

            let invalidation = db().invalidate().await;
            gloo_console::info!(format!("invalidated connection: {:?}", invalidation));

            let Some(win) = web_sys::window() else { return };
            if let Ok(caches) = win.caches() {
                if let Ok(keys_js) = JsFuture::from(caches.keys()).await {
                    let keys = js_sys::Array::from(&keys_js);
                    for key in keys.iter() {
                        if let Some(k) = key.as_string() {
                            let deleted = JsFuture::from(caches.delete(&k)).await;
                            gloo_console::info!(format!("Deleted cache {k}: {deleted:?}"));
                        }
                    }
                }
            }
            let swc = win.navigator().service_worker();
            if let Ok(regs_js) = JsFuture::from(swc.get_registrations()).await {
                let regs = js_sys::Array::from(&regs_js);
                for reg_val in regs.iter() {
                    if let Ok(reg) = reg_val.dyn_into::<web_sys::ServiceWorkerRegistration>() {
                        if let Ok(promise) = reg.unregister() {
                            let _ = JsFuture::from(promise).await;
                        }
                    }
                }
            }
            let reload = win.location().reload();
            gloo_console::info!(format!("Reloading window: {reload:?}"));
        });

        let logout_msg = "Logged out".to_string();
        self.shared_ctx.state = AppState::NoAuth(logout_msg.clone());
        let _ = self.shared_ctx.app_state_tx.try_send(AppState::NoAuth(logout_msg));
        let toast = &mut self.shared_ctx.toasts;

        let error_toast = Toast {
            kind: ToastKind::Error,
            style: ToastStyle::default(),
            text: format!("Detected older crate version").into(),
            user_dismissed: false,
            options: ToastOptions::default().show_progress(true).duration_in_seconds(10.0),
        };
        toast.add(error_toast);
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        // do some initial setting up
        if self.shared_ctx.first_run { self.first_run(ctx, frame); }
        
        self.shared_ctx.receive_shared(frame, ctx);
        
        // Reconnects are fully automatic: `receive_shared_logic` drains
        // stream errors / canary timeouts, rebuilds the connection with
        // backoff, and re-issues the LIVE SELECTs plus a snapshot refetch.
        // The only operator prompt left is the auth-lost banner.
        
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
                            apply_user_color_scheme(ctx, &user.get_color_scheme());
                            self.shared_ctx.user_theme_loaded = true;
                            self.shared_ctx.load_data(ctx, &user);
                            let _ = self.shared_ctx.app_state_tx.try_send(AppState::Authenticated(displays::app_state::MainPages::Tasks));
                            let toast = &mut self.shared_ctx.toasts;
                            let auth_toast = Toast {
                                style: ToastStyle::default(),
                                kind: ToastKind::Success,
                                text: format!("{e:?}").into(),
                                user_dismissed: false,
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
                            user_dismissed: false,
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
                    // Mutates the stored user so reconnect re-applies the saved scheme, not stale bytes.
                    if let Some(user) = self.shared_ctx.current_user.as_mut() {
                        user.set_color_scheme(encode_style(&r.1.clone()).unwrap_or_default());
                        ctx.set_global_style(r.1.clone());
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

        {
            let clients_snapshot = self.shared_ctx.clients.clone();
            let layout = &mut self.shared_ctx.web_console_layout;
            let mut to_dock: Vec<String> = Vec::new();

            for client in &clients_snapshot {
                let is_floating = layout
                    .session_layout
                    .get(&client.connection_string)
                    .copied()
                    .unwrap_or_default()
                    == SessionLayout::Floating;

                if !is_floating {
                    continue;
                }

                let is_ws_connected = layout
                    .ws_clients
                    .get(&client.connection_string)
                    .map(|wsc| {
                        if wsc.transport.kind() == TransportKind::Tcp {
                            wsc.is_connected
                        } else {
                            wsc.is_connected && wsc.last_pong_time.is_some()
                        }
                    })
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

                // Clone everything we need out of `layout` before entering the
                // window closure so we can still call `layout.ws_clients.get_mut`
                // inside without conflicting borrows.
                let tx = layout.ui_actions_channel.0.clone();
                let session_layout = layout.session_layout.clone();
                let focused = layout.focused_client.clone();
                let inventory = layout
                    .security_inventory
                    .get(&client.connection_string)
                    .cloned();
                let transport = layout
                    .ws_clients
                    .get(&client.connection_string)
                    .map(|w| (w.transport.kind(), w.is_connected));

                let mut is_open = true;
                // MtechServer2.0 is the wasm-only browser admin and has no
                // foreign-key health prober wired in — pass empty placeholder
                // channels/maps so the shared `client_header` signature
                // (which the native Mastertech4.0 caller fills with real
                // `ws_client.fk_health_tx`/`fk_health_cache`) stays unified.
                // No probes will ever queue here because nothing drains the
                // receiver end; `fk_health_cache` is empty so the per-row
                // FK health badge renders as "unknown".
                let (fk_health_tx, _fk_health_rx) =
                    crossbeam::channel::unbounded::<(String, bool, bool)>();
                let fk_health_cache: std::collections::HashMap<String, (bool, bool)> =
                    std::collections::HashMap::new();
                Window::new(&client.connection_string)
                    .open(&mut is_open)
                    .frame(column_frame)
                    .min_size(Vec2::new(700., 400.))
                    .max_size(Vec2::new(1500., 900.))
                    .default_size(Vec2::new(1000., 900.))
                    .show(ctx, |ui| {
                        ui.vertical_centered_justified(|ui| {
                            ui.horizontal(|ui| {
                                AdminConsole::client_header(
                                    ui,
                                    tx,
                                    client,
                                    session_layout,
                                    focused.as_deref(),
                                    is_ws_connected,
                                    &fk_health_tx,
                                    &fk_health_cache,
                                    inventory.as_deref(),
                                    None,
                                    transport,
                                );
                            });
                            if let Some(ws_client) =
                                layout.ws_clients.get_mut(&client.connection_string)
                            {
                                ws_client.show(ui);
                            }
                        });
                    });

                if !is_open {
                    to_dock.push(client.connection_string.clone());
                }
            }

            for cs in to_dock {
                layout.session_layout.insert(cs, SessionLayout::Docked);
            }
        }

        // Get User settings from local storage
        if let Some(user) = &self.shared_ctx.current_user {
            if self.shared_ctx.get_settings {
                self.shared_ctx.get_settings = false;
                let layout = user.get_user_settings().get_ui_layout_mtechserver();
                if let Ok(tree) = serde_json::from_value::<egui_dock::DockState<displays::tabs::TabId>>(layout.clone()) {
                    self.shared_ctx.dock.tree = tree;
                } else {
                    match serde_json::from_value::<DockState<String>>(layout) {
                        Ok(legacy) => {
                            self.shared_ctx.dock =
                                displays::tabs::DockSession::from_legacy_tree(legacy)
                        }
                        Err(e) => log::error!(
                            "Could not get UI layout from user: {e:?}: {:#?}",
                            user.get_user_settings().get_ui_layout_mtechserver()
                        ),
                    }
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
    }
}
