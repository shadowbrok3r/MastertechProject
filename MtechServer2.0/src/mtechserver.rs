use crate::app_state::{AppState, MainPages, MtechServer};
use displays::ui_tools::theme_config::set_custom_style;
use eframe::egui::{
    Color32, Context, Frame, Margin, Rounding, Stroke,
    Vec2, Window,
};
use egui_dock::DockState;
use log::info;

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        // currently this just sets the style of the app, but in the near
        // future i will be making this the setup to allow user customization
        // to the style of any part of the app
        if self.context.shared_ctx.modify_theme {
            Window::new("Theme Mods").max_height(600.).title_bar(true).show(ctx, |ui| {
                // info!("Settings: {:?}", self.context.theme_config);
                let theme = self.context.shared_ctx.theme_config.edit_ui(ui, self.context.shared_ctx.settings_sender.clone());
                if theme.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.user_settings.color_scheme = serde_json::to_value(theme.1.clone()).unwrap();
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&user.user_settings).unwrap_or_default());
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
                            info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr.len());
                            wasm_cookies::set("user", &encoded, &cookie_opts);
                        }
                    }
                    self.context.shared_ctx.theme_config = theme.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            });
        }

        let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        ctx.set_style((custom_style).clone());

        // Getting responses from our webworker
        if let Some(items) = self.context.data_update.take() {
            let tx = self.context.shared_ctx.initial_tasks_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = tx.try_send(items);
            });
        }

        // do some initial setting up
        if self.context.first_run { self.first_run(frame); }

        if self.context.wants_to_undock {
            for client in self.context.clients.clone() {
                let undock = if let Some(undock) =
                    self.context.undock_client.get(&client.connection_string)
                {
                    undock
                } else {
                    &false
                };

                if *undock {
                    let color = if client.connected {
                        Color32::LIGHT_BLUE
                    } else {
                        Color32::LIGHT_RED
                    };

                    let column_frame = Frame::default()
                        .fill(Color32::from_rgb(12, 12, 14))
                        .inner_margin(Margin::same(4.0))
                        .outer_margin(Margin::symmetric(5.0, 3.0))
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, color));

                    Window::new(&client.connection_string)
                        .frame(column_frame)
                        .max_size(Vec2::new(700., 400.))
                        .show(ctx, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ui.horizontal(|ui| self.context.headers(ui, client.clone()));
                                if let Some(ws_client) =
                                    self.context.ws_clients.get_mut(&client.connection_string)
                                {
                                    ws_client.show(ui);
                                }
                            });
                        });
                }
            }
        }

        // Branch out all the different crossbeam channels to receive
        // in their own methods to clean up a lot of boilerplate code
        // as well as being able to find specific code a lot easier
        // self.receive() is the same thing but those crossbeam channels
        // being received have literally one line in them that i dont want to
        // justify creating a separate file / module for
        self.receive();
        self.context.shared_ctx.receive(frame);
        self.receive_database(frame, ctx);
        self.context.shared_ctx.receive_inventory();
        self.context.shared_ctx.receive_client();
        self.context.shared_ctx.receive_ui_action();
        self.context.shared_ctx.receive_prestashop();
        self.context.shared_ctx.receive_task();
        self.context.shared_ctx.receive_ticket();
        self.context.shared_ctx.receive_notes();
        self.context.shared_ctx.receive_notification();
        self.context.shared_ctx.handle_modals(ctx);
        self.context.shared_ctx.handle_viewports(ctx);
        self.context.shared_ctx.toasts.show(ctx);
        self.menu_bar(ctx);

        // Get User settings from local storage
        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<DockState<String>>(user.user_settings.ui_layout.mtechserver.clone()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => info!("Could not get UI layout from user: {e:?}: {:#?}", user.user_settings.ui_layout),
                }
            } 
        }

        // Get User settings from local storage
        // this bool gets switched via clicking
        // the submit button in the crate::tabs::json_viewer
        // module
        if self.context.update_settings {
            self.context.update_settings = false;
            info!("Saving settings: {:?}", self.context.user_settings.clone());
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

        // Handle changes to state from various places, such as
        // hitting the login button, clicking the 'home page' button
        // (which is clicking Mtechserver in the top middle of the page),
        // if session cookie expires (gets checked in the first_run method),
        // if manually logged out, etc
        match &self.state {
            // Always checking authentication
            AppState::Authenticated(MainPages::Tasks) => self.main_page(ctx),
            AppState::Authenticated(MainPages::Downloads) => self.downloads_page(ctx),
            AppState::Authenticated(MainPages::AccountSettings) => self.account_settings_page(ctx, self.context.app_state_tx.clone()),
            AppState::Authenticated(MainPages::WebConsole) => self.web_console(ctx),
            AppState::Authenticated(_) => self.main_page(ctx),
            AppState::CreateAccount => self.signup_page(
                ctx,
                self.context.db_tx.clone(),
                self.context.app_state_tx.clone(),
            ),
            AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.shared_ctx.current_user.is_some() {
                        if !self.context.shared_ctx.load_data(ctx) {
                            self.context.first_run = true;
                            self.first_run(frame);
                            self.state = AppState::NoAuth("No user detected".to_string());
                        }
                    } else {
                        self.context.first_run = true;
                        self.first_run(frame)
                    }
                    self.state = AppState::Authenticated(MainPages::Tasks);
                } else {
                    self.login_page(
                        ctx,
                        self.context.db_tx.clone(),
                        self.context.app_state_tx.clone(),
                    )
                }
            }
        }
    }

    // fn persist_egui_memory(&self) -> bool {
    //     true
    // }

    // fn save(&mut self, storage: &mut dyn eframe::Storage) {
    //     eframe::set_value(storage, eframe::APP_KEY, self)
    // }

    // fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
    //     if let Some(window) = web_sys::window() {
    //         if let Ok(storage) = window.local_storage() {
    //             if let Some(storage) = storage {
    //                 let clear = storage.clear();
    //                 info!("Clearing storage: {clear:?}");
    //             }
    //         }
    //     }
    // }
}