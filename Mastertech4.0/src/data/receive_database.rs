use displays::{PlatformSpawner, Spawner, app_state::{AppState, MainPages}, pages::login_page::HASH, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use database::{create_guest_notification, schema::{NOTIFICATION_TABLE, Notification, USER_TABLE, random_record_id}};
use crate::{app_state::MasterTechApp, utilities::{load_encrypted_user_data, save_encrypted_user_data}};
use surrealdb::types::{Datetime, RecordId};
use eframe::egui::Context;

impl MasterTechApp {
    pub fn receive_database(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.shared_ctx.db_rx.try_recv() {
            ctx.request_repaint();
            match db {
                Ok(db) => {
                    log::info!("3");
                    if self.context.shared_ctx.current_user.is_none() && db.user.is_some() {
                        self.context.shared_ctx.current_user = db.user;
                        self.context.shared_ctx.state = AppState::NoAuth("Setting encrypted data".to_string());
                        let login_mut = self.context.shared_ctx.login_mut();
                        if let Some(login) = login_mut {
                            match save_encrypted_user_data(&login, HASH) {
                                Ok(_) => log::info!("User data saved successfully"),
                                Err(e) => log::error!("Failed to save user data: {e:?}"),
                            }
                            self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
                        } else {
                            log::error!("No login mut: {:?}", self.context.shared_ctx.state);
                        }
                        log::info!("10");
                        
                    } else {
                        log::info!("11");
                    }

                    let usr = self.context.shared_ctx.current_user.clone();
                    if let Some(user) = usr {
                        self.context.ticket_data.tech = user.get_username().to_string();
                        self.context.shared_ctx.load_data(ctx, &user);
                    } else {
                        self.context.shared_ctx.first_run = true;
                        self.first_run(ctx);
                        log::error!("2");
                        self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    }
                }
                Err(ref e) => {
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
                                ..Default::default()
                            };
                            toast.add(auth_toast);
                        } else {
                            self.context.shared_ctx.first_run = true;
                            self.first_run(ctx);
                            log::error!("2");
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        }
                    } else {
                        log::info!("8");
                        log::error!("{e:?}");
                        // eframe::web::storage::local_storage_get(key)
                        let toast = &mut self.context.shared_ctx.toasts;
                        let auth_toast = Toast {
                            kind: ToastKind::Error,
                            text: format!("{e:?} \nYou may need to login again").into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                            ..Default::default()
                        };
                        toast.add(auth_toast);

                        let user: Option<String> = if let Ok(database) = db {
                            database.user.map(|u| u.get_email().to_string())
                        } else {
                            self.context.shared_ctx.state = AppState::NoAuth("Getting username from login".to_string());
                            let login_mut = self.context.shared_ctx.login_mut();
                            if let Some(login) = login_mut {      
                                Some(login.username.clone())
                            } else {
                                None
                            }                   
                        };
                        
                        let msg = match user {
                            Some(u) => {
                                if u.is_empty() {
                                    if let Some(login) = load_encrypted_user_data(HASH) {
                                        let username = login.username.clone();
                                        if username.is_empty() {
                                            format!("A user ran into an error logging in: {e:?}")
                                        } else {
                                            format!("{username} ran into an error logging in: {e:?}")
                                        }
                                    } else {
                                        format!("A user ran into an error logging in: {e:?}")
                                    }
                                } else {
                                    format!("{u} ran into an error logging in: {e:?}")
                                }
                            },
                            None => format!("A user ran into an error logging in: {e:?}"),
                        };

                        PlatformSpawner::spawn(async move {
                            let notification = Notification {
                                id: random_record_id(NOTIFICATION_TABLE),
                                user: RecordId::new(USER_TABLE, "jm9a7l3v32gsiccr7pgw"),
                                notification_description: msg,
                                notification_type: "ALERT".to_string(),
                                status: "Unread".to_string(),
                                created_at: Datetime::now(),
                                accessed_at: None,
                            };
                            let res = create_guest_notification(notification).await;
                            match res {
                                Ok(_) => log::info!("Notification from guest account created successfully"),
                                Err(e) => log::error!("Failed to create notification: {e:?}"),
                            }
                        });

                        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth("Needs login".to_string()));
                    }
                }
            }
        }
    }
}
