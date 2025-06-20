use displays::{app_state::{AppState, MainPages}, pages::login_page::HASH, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use crate::{app_state::MasterTechApp, filesystem::system_info::ComputerInfo, utilities::save_encrypted_user_data};
use eframe::egui::Context;

impl MasterTechApp {
    pub fn receive_database(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.shared_ctx.db_rx.try_recv() {
            ctx.request_repaint();
            match db {
                Ok(db) => {
                    log::info!("3");
                    if self.context.shared_ctx.current_user.is_none() && db.user.is_some() {
                        let login_mut = self.context.shared_ctx.login_mut();
                        if let Some(login) = login_mut {
                            match save_encrypted_user_data(&login, HASH) {
                                Ok(_) => log::info!("User data saved successfully"),
                                Err(e) => log::error!("Failed to save user data: {e:?}"),
                            }
                            self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);

                            #[cfg(target_os = "windows")]
                            {
                                if self.context.computer_data.cpu.is_empty() {
                                    let specs_tx = self.context.computer_data_tx.clone();
                                    let current_antivirus_tx = self.context.current_antivirus_tx.clone();
                                    tokio::spawn(async move {
                                        match database::schema::ComputerData::default().get_computer_data().await {
                                            Ok(data) => { let _ = specs_tx.try_send(data); }
                                            Err(e) => log::error!("Error getting specs: {e:?}"),
                                        }
                                        let installed_antivirus = database::schema::ComputerData::get_antivirus().await.unwrap_or_default();
                                        log::error!("installed_antivirus: {installed_antivirus:?}");
                                        let _ = current_antivirus_tx.try_send(installed_antivirus);
                                    });
                                }
                            }
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
                    } else {
                        self.context.shared_ctx.first_run = true;
                        self.first_run(ctx, frame);
                        log::error!("2");
                        self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    }
                }
                Err(e) => {
                    log::info!("6");
                    if e.to_string().contains("Already connected") {
                        log::info!("7");
                        let usr = self.context.shared_ctx.current_user.clone();
                        if let Some(user) = usr {
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
                            log::error!("2");
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        }
                    } else {
                        log::info!("8");
                        log::info!("{e:?}");
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
