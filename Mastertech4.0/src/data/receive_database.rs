use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::egui::Context;
use crate::app_state::{AppState, MainPages, MasterTechApp};
use log::info;


impl MasterTechApp {
    pub fn receive_database(&mut self, ctx: &Context) { // , frame: &mut eframe::Frame
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv() {
            match db {
                Ok(db) => {
                    info!("3");
                    if self.context.shared_ctx.current_user.is_none() && db.user.is_some() {
                        info!("10");
                        self.context.shared_ctx.current_user = db.user;
                    } else {
                        info!("11");
                    }
                    if !self.context.shared_ctx.load_data(ctx) {
                        info!("12");
                        let _ = self.context.app_state_tx.try_send(AppState::NoAuth("No user detected".to_string()));
                    } else {
                        let _ = self.context.app_state_tx.try_send( AppState::Authenticated(MainPages::Tasks));
                    }
                }
                Err(e) => {
                    info!("6");
                    if e.to_string().contains("Already connected") {
                        info!("7");
                        if !self.context.shared_ctx.load_data(ctx) {
                            let _ = self.context.app_state_tx.try_send(AppState::NoAuth("No user detected".to_string()));
                        }
                        let _ = self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                        let toast = &mut self.context.toasts;
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
                        let toast = &mut self.context.toasts;
                        let auth_toast = Toast {
                            kind: ToastKind::Error,
                            text: format!("{e:?} \nYou may need to login again").into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                        };
                        toast.add(auth_toast);
                        let _ = self.context.app_state_tx.try_send(AppState::NoAuth("Needs login".to_string()));
                    }
                }
            }
        }
    }
}
