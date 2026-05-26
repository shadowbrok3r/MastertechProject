
impl crate::app_state::SharedContext {
    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.db_rx.try_recv() {
            ctx.request_repaint();
            let tx = self.app_state_tx.clone();
            match db {
                Ok(db) => {
                    log::info!("3");
                    
                    if self.current_user.is_none() && db.user.is_some() {
                        let login_mut = self.login_mut();
                        if login_mut.is_some() {
                            self.state = crate::app_state::AppState::Authenticated(crate::app_state::MainPages::Tasks);
                        } else {
                            log::error!("No login mut");
                        }
                        log::info!("10");
                        self.current_user = db.user;
                    } else {
                        log::info!("11");
                    }

                    let usr = self.current_user.clone();
                    if let Some(user) = usr {
                        self.load_data(ctx, &user);
                        let _ = self.app_state_tx.try_send(crate::app_state::AppState::Authenticated(crate::app_state::MainPages::Tasks));
                    } else {
                        self.first_run = true;
                        self.first_run(ctx,frame);
                        log::error!("1");
                        self.state = crate::app_state::AppState::NoAuth("No user detected".to_string());
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
                        let usr = self.current_user.clone();
                        if let Some(user) = usr {
                            displays::ui_tools::theme_config::apply_user_color_scheme(ctx, &user.get_color_scheme());
                            self.user_theme_loaded = true;
                            self.load_data(ctx, &user);
                            let _ = self.app_state_tx.try_send(crate::app_state::AppState::Authenticated(crate::app_state::MainPages::Tasks));
                            let toast = &mut self.toasts;
                            let auth_toast = crate::ui_data::Toast {
                                kind: crate::ui_data::ToastKind::Success,
                                text: format!("{e:?}").into(),
                                options: crate::ui_data::ToastOptions::default()
                                    .show_progress(true)
                                    .duration_in_seconds(6.0),
                            };
                            toast.add(auth_toast);
                        } else {
                            self.first_run = true;
                            self.first_run(ctx, frame);
                            log::error!("1");
                            self.state = crate::app_state::AppState::NoAuth("No user detected".to_string());
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
                        let toast = &mut self.toasts;
                        let auth_toast = crate::ui_data::Toast {
                            kind: crate::ui_data::ToastKind::Error,
                            text: format!("{e:?} \nYou may need to login again").into(),
                            options: crate::ui_data::ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                        };
                        toast.add(auth_toast);
                        let _ = tx.try_send(crate::app_state::AppState::NoAuth("Needs login".to_string()));
                    }
                }
            }
        }

    }
}