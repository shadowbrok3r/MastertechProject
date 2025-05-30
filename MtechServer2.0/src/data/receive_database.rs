use displays::{app_state::{AppState, MainPages}, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
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
                    if !self.context.shared_ctx.load_data(ctx) {
                        log::info!("Couldnt load data, running first_run");
                        self.context.shared_ctx.first_run = true;
                        self.first_run(frame);
                        log::error!("5");
                        // tx.try_send(AppState::NoAuth("No user detected".to_string())).unwrap();
                    } else {
                        log::info!("Loaded Data");
                        tx.try_send(AppState::Authenticated(MainPages::Tasks)).unwrap();
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
                        if !self.context.shared_ctx.load_data(ctx) {
                            self.context.shared_ctx.first_run = true;
                            self.first_run(frame);
                            log::error!("6");
                            let _ = tx.try_send(AppState::NoAuth("No user detected".to_string()));
                        }
                        let _ = tx.try_send(AppState::Authenticated(MainPages::Tasks));
                        let toast = &mut self.context.shared_ctx.toasts;
                        let auth_toast = Toast {
                            kind: ToastKind::Success,
                            text: format!("Already Connected").into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                        };
                        toast.add(auth_toast);
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
