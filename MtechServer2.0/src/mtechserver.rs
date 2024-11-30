use crate::app_state::{AppState, MainPages, MtechServer};
use displays::ui_tools::theme_config::set_custom_style;
use eframe::egui::{
    Color32, Context, Frame, Margin, Rounding, Stroke,
    Vec2, Window,
};
use log::info;

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // This is our 'dummy' worker that retrieves Minio bucket storage
        // contents, then builds our 'virtual' file system ui in the
        // crate::tabs::toolbox tab
        // let data_update = self.context.data_update.as_mut().unwrap();
        // if let Some(items) = data_update.take() {
        //     if !items.is_empty() && self.context.file_system.paths.is_empty() {
        //         debug!("Files: {items:?}");
        //         self.context.file_system.build_file_system(items);
        //     }
        // }

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
        self.receive_database(frame);
        self.receive_client();
        self.receive_inventory();
        self.receive_ui_action();
        self.receive_prestashop();
        self.receive_task();
        self.receive_ticket();
        self.receive_notes();
        self.receive_notification();
        self.menu_bar(ctx);
        self.context.handle_modals(ctx);
        self.context.toasts.show(ctx);

        // Get User settings from local storage
        // this bool gets switched via button click
        // in the crate::tabs::json_viewer module
        if self.context.get_settings {
            if let Some(storage) = frame.storage() {
                if let Some(_settings) = storage.get_string("user_settings") {}
            }
        }

        // Get User settings from local storage
        // this bool gets switched via clicking
        // the submit button in the crate::tabs::json_viewer
        // module
        if self.context.update_settings {
            self.context.user_settings.startup_tabs =
                Some(serde_json::to_value(self.tree.clone()).unwrap());

            self.context.update_settings = false;
            info!("Saving settings: {:?}", self.context.user_settings.clone());
            frame.storage_mut().unwrap().set_string(
                "user_settings",
                serde_json::to_string(&self.context.user_settings).unwrap(),
            );
        }

        if self.context.ai_playground.save_chats {
            self.context.ai_playground.save_chats = false;
            if let Some(_usr) = &self.context.current_user {
                let threads = self.context.ai_playground.get_threads();
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

        // most important part of the whole app.. setting up our styling
        // currently this just sets the style of the app, but in the near
        // future i will be making this the setup to allow user customization
        // to the style of any part of the app
        if self.context.modify_theme {
            Window::new("Theme Mods").max_height(600.).title_bar(true).show(ctx, |ui| {
                // info!("Settings: {:?}", self.context.theme_config);
                let theme = self.context.theme_config.edit_ui(ui);
                if theme.0 {
                    #[cfg(target_arch = "wasm32")]
                    {
                        if let Some(user) = self.context.current_user.clone().as_mut() {
                            wasm_cookies::delete("user");
                            user.user_settings.as_mut().unwrap().color_scheme = serde_json::to_value(theme.1.clone()).unwrap();
                            let duration = web_time::Duration::from_secs(172800);
                            let usr = serde_json::to_string(&user.clone()).unwrap();
                            let cookie_opts = wasm_cookies::CookieOptions::default()
                                .with_same_site(wasm_cookies::SameSite::Strict)
                                .secure()
                                .expires_after(duration);
                            wasm_cookies::set("user", &usr, &cookie_opts);
                        }
                    }
                    self.context.theme_config = theme.1;
                    self.context.modify_theme = false;
                }
            });
        }

        let custom_style = set_custom_style(&self.context.theme_config);
        ctx.set_style((*custom_style).clone());

        // Handle changes to state from various places, such as
        // hitting the login button, clicking the 'home page' button
        // (which is clicking Mtechserver in the top middle of the page),
        // if session cookie expires (gets checked in the first_run method),
        // if manually logged out, etc
        match &self.state {
            // Always checking authentication
            AppState::Authenticated(MainPages::Tasks) => self.main_page(ctx),
            AppState::Authenticated(MainPages::Downloads) => self.downloads_page(ctx),
            AppState::Authenticated(MainPages::AccountSettings) => {
                self.account_settings_page(ctx, self.context.app_state_tx.clone())
            }
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
                    if self.context.current_user.is_some() {
                        self.load_data(frame);
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

    fn persist_egui_memory(&self) -> bool {
        true
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self)
    }

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