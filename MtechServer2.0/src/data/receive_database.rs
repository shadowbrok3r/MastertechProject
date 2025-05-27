use displays::{app_state::{AppState, MainPages}, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use crate::app_state::MtechServer;
use eframe::{egui::Context, Frame};
use log::info;


impl MtechServer {
    pub fn receive_database(&mut self, frame: &mut Frame, ctx: &Context) {
        ctx.request_repaint();
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.shared_ctx.db_rx.try_recv() {
            info!("No token");
            match db {
                Ok(db) => {
                    info!("3");
                    if !self.context.shared_ctx.load_data(ctx) {
                        self.context.shared_ctx.first_run = true;
                        self.first_run(frame);
                        self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    } else {
                        self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
                    }
                    
                    if let Some(token) = db.jwt.clone() {
                        self
                        .context
                        .bridge
                        .send(
                            crate::webworker::Input(token.into_insecure_token())
                        );
                    } else { info!("No token"); }
                }
                Err(e) => {
                    info!("6");
                    if e.to_string().contains("Already connected") {
                        info!("7");
                        if !self.context.shared_ctx.load_data(ctx) {
                            self.context.shared_ctx.first_run = true;
                            self.first_run(frame);
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        }
                        self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
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
                        info!("8");
                        info!("{e:?}");
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
                        self.context.shared_ctx.state = AppState::NoAuth("Needs login".to_string());
                    }
                }
            }
        }
    }
}
