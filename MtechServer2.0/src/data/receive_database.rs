use displays::{app_state::{AppState, MainPages}, ui_tools::{decode_style, toasts::{Toast, ToastKind, ToastOptions}}};
use crate::app_state::MtechServer;
use eframe::{egui::Context, Frame};


impl MtechServer {
    pub fn receive_database(&mut self, frame: &mut Frame, ctx: &Context) {
        ctx.request_repaint();
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.shared_ctx.db_rx.try_recv() {
            let tx = self.context.shared_ctx.app_state_tx.clone();
            match db {
                Ok(db) => {
                    log::info!("3");
                    if self.context.shared_ctx.current_user.is_none() && db.user.is_some() {
                        let login_mut = self.context.shared_ctx.login_mut();
                        if login_mut.is_some() {
                            self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
                        } else {
                            log::error!("No login mut");
                        }
                        log::info!("10");
                        self.context.shared_ctx.current_user = db.user;
                    } else {
                        log::info!("11");
                    }

                    let usr = self.context.shared_ctx.current_user.clone();
                    if let Some(user) = usr {
                        self.context.shared_ctx.load_data(ctx, &user);
                        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                    } else {
                        self.context.shared_ctx.first_run = true;
                        self.first_run(ctx,frame);
                        log::error!("1");
                        self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    }
                    
                    if let Some(token) = db.jwt.clone() {
                        self
                        .context
                        .bridge
                        .send(
                            crate::webworker::Input(token.into_insecure_token())
                        );
                    } else { log::info!("No token"); }
                }
                Err(e) => {
                    log::info!("6");
                    if e.to_string().contains("Already connected") {
                        log::info!("7");
                        let usr = self.context.shared_ctx.current_user.clone();
                        if let Some(user) = usr {
                            ctx.set_style(decode_style(&user.get_color_scheme()).unwrap_or_default());
                            self.context.shared_ctx.load_data(ctx, &user);
                            let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                            let toast = &mut self.context.shared_ctx.toasts;
                            let auth_toast = Toast {
                                kind: ToastKind::Success,
                                text: format!("{e:?}").into(),
                                options: ToastOptions::default()
                                    .show_progress(true)
                                    .duration_in_seconds(6.0),
                            };
                            toast.add(auth_toast);
                        } else {
                            self.context.shared_ctx.first_run = true;
                            self.first_run(ctx, frame);
                            log::error!("1");
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
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
                        let toast = &mut self.context.shared_ctx.toasts;
                        let auth_toast = Toast {
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
    }
}
