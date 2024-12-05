use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use log::info;

use crate::app_state::{AppState, MainPages, MtechServer};

impl MtechServer {
    pub fn receive_database(&mut self) {
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv() {
            match db {
                Ok(_db) => {
                    info!("3");
                    self.context.shared_ctx.load_data();
                }
                Err(e) => {
                    info!("6");
                    if e.to_string().contains("Already connected") {
                        info!("7");
                        self.context.shared_ctx.load_data();
                        self.state = AppState::Authenticated(MainPages::Tasks);
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
                        #[cfg(target_arch = "wasm32")]
                        {
                            wasm_cookies::delete("jwt");
                            wasm_cookies::delete("user");
                        }
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
                        self.state = AppState::NoAuth("Needs login".to_string());
                    }
                }
            }
        }
    }
}
