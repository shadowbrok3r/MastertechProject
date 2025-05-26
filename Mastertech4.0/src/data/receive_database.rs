use displays::{app_state::{AppState, MainPages}, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use crate::app_state::MasterTechApp;
use eframe::egui::Context;
use log::info;


impl MasterTechApp {
    pub fn receive_database(&mut self, ctx: &Context) { // , frame: &mut eframe::Frame
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.shared_ctx.db_rx.try_recv() {
            match db {
                Ok(db) => {
                    info!("3");
                    if !self.context.shared_ctx.load_data(ctx) {
                        self.context.first_run = true;
                        self.first_run();
                        self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    } else {
                        self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
                    }

                    if self.context.shared_ctx.current_user.is_none() && db.user.is_some() {
                        info!("10");
                        self.context.shared_ctx.current_user = db.user;
                    } else {
                        info!("11");
                    }
                }
                Err(e) => {
                    info!("6");
                    if e.to_string().contains("Already connected") {
                        info!("7");
                        if !self.context.shared_ctx.load_data(ctx) {
                            self.context.first_run = true;
                            self.first_run();
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        }
                        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
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
                        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth("Needs login".to_string()));
                    }
                }
            }
        }
    }
}
