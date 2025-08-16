use database::{live_data::listen_data,schema::{utilities::{get_notifications, get_qcs, get_store_users, get_tasks_for_store}, User, NOTIFICATION_TABLE, TASK_NOTE_TABLE, TASK_TABLE, USER_TABLE}};
use crate::ui_tools::{decode_style, toasts::{Toast, ToastKind, ToastOptions}};
use crate::{PlatformSpawner, Spawner};
use eframe::egui::Style;
use std::sync::Arc;

pub mod receive_notes;
pub mod receive_notifications;
pub mod receive_prestashop;
pub mod receive_task;
pub mod receive_ui_action;
pub mod receive_client;
pub mod receive_users;
pub mod admin_notification;

impl crate::app_state::SharedContext {
    pub fn load_data(&mut self, ctx: &eframe::egui::Context, user: &User) {
        self.refresh_client_list();
        self.timer = Some(web_time::Instant::now());
        // get all of our channel Senders from crossbeam to get user/store/completed tasks,
        // as well as store users and live task notifications
        let live_tasks_tx = self.live_tasks_tx.clone();
        let notes_tx = self.notes_tx.clone();
        let live_notif_tx = self.live_notification_tx.clone();        
        let live_user_tx = self.live_user_tx.clone();
        self.store_selection = std::convert::Into::<u64>::into(user.get_store());
        let user = user.clone();
        let name = user.get_name();
        log::info!("Getting Initial data: {}", self.store_selection);
        if self.filesystem.paths.is_empty() {
            self.filesystem.set_user(user.clone());
            let _ = self.filesystem.request_contents("");
        }
        if self.web_console_layout.filesystem.paths.is_empty() {
            self.web_console_layout.filesystem.set_user(user.clone());
            let _ = self.web_console_layout.filesystem.request_contents("");
            // self.web_console_layout.set_filesystem(self.filesystem.clone());
        }

        if self.tasks.is_empty() || self.store_users.is_empty() {
            let initial_tasks_tx = self.initial_tasks_tx.clone();
            let store_users_tx = self.store_users_tx.clone();
            let store = user.get_store();
            let notifs_tx = self.notification_tx.clone();
            PlatformSpawner::spawn(async move {
                let get_store_users = get_store_users(store_users_tx, store).await;
                log::info!("get_store_users: {get_store_users:?}");
            });

            PlatformSpawner::spawn(async move {
                let get_tasks = get_tasks_for_store(initial_tasks_tx, store.as_str().to_string()).await;
                log::info!("get_tasks: {get_tasks:?}");
            });

            PlatformSpawner::spawn(async move {
                let get_qcs = get_qcs().await;
                log::error!("get_qcs: {get_qcs:?}");
            });

            PlatformSpawner::spawn(async move {
                let get_notifications = get_notifications(notifs_tx).await;
                log::info!("get_notifications: {get_notifications:?}");
            });
            
            self.task_layouts
                .iter_mut()
                .filter(|(page, _)| *page == "Completed Tasks" || *page == "Store Tasks")
                .for_each(|(_, layout)| {
                    layout.loading = false;
            });
        }

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(notes_tx, TASK_NOTE_TABLE).await;
            log::info!("listen_task_notes: {listen_data:?}");
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_user_tx, USER_TABLE).await;
            log::info!("listen_user: {listen_data:?}");
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_tasks_tx, TASK_TABLE).await;
            log::info!("listen_tasks: {listen_data:?}");
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_notif_tx.clone(), NOTIFICATION_TABLE).await;
            log::info!("listen_notifications: {listen_data:?}");
        });

        self.stock_tables.first_run();
        match decode_style(&user.get_color_scheme()) {
            Ok(color_settings) => {
                ctx.set_style(color_settings);
                ctx.request_repaint();
            },
            Err(e) => {
                log::error!("Error setting theme config: {e:?}");
                match serde_json::from_str::<Style>(crate::STYLE) {
                    Ok(theme) => {
                        let style = Arc::new(theme);
                        ctx.set_style(style);
                    }
                    Err(e) => log::error!("Error setting theme: {e:?}")
                };
            },
        }

        ctx.request_repaint();
        
        let toast = &mut self.toasts;
        let auth_toast = Toast {
            kind: ToastKind::Success,
            text: format!("Logged in successfully\nWelcome, {}", name).into(),
            options: ToastOptions::default()
                .show_progress(true)
                .duration_in_seconds(6.0),
        };
        toast.add(auth_toast);
    }

    pub fn receive_shared(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        if let Ok(state) = self.app_state_rx.try_recv() {
            log::info!("Got a new state: {state:?}\nbefore state: {:?}", self.state);
            if let crate::app_state::AppState::NoAuth(reason) = &state {
                let toast = &mut self.toasts;
                let error_toast = crate::ui_tools::toasts::Toast {
                    kind: crate::ui_tools::toasts::ToastKind::Error,
                    text: reason.into(),
                    options: crate::ui_tools::toasts::ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
            }

            self.state = state;

            /* 
            if state == crate::app_state::AppState::Authenticated(MainPages::Tasks)
                && self.state == AppState::NoAuth("Needs Login".to_string()) 
            {
                
            } else {
                match state {
                    AppState::Authenticated(main_pages) => {
                        match main_pages {
                            MainPages::Tasks => {
                                if self.state == AppState::NoAuth("Needs Login".to_string()) {
                                } else {
                                    self.state = state;
                                }
                            },
                            MainPages::Downloads => todo!(),
                            MainPages::UserPreferences => todo!(),
                        }
                    },
                    AppState::CreateAccount => self.signup_page(
                        ctx,
                        self.db_tx.clone(),
                        self.app_state_tx.clone(),
                    ),
                    AppState::NoAuth(_) => todo!(),
                } 
            }
            */
            ctx.request_repaint();
        }

        // match self.state {
        //     AppState::Authenticated(page) => match page {
        //         MainPages::UserPreferences => self.account_settings_page(ctx, self.app_state_tx.clone()),
        //         _ => {}
        //     },
        //     AppState::Authenticated(page) => {      
        //         if self.current_user.is_none() {
        //             self.state = AppState::NoAuth("AppState was Authenticated, but user is not set.".to_string());
        //         }
        //     },
        //     AppState::NoAuth(reason) => {
        //         if reason.to_string().contains("Already connected") {
        //             info!("Already connected");
        //             let usr = self.current_user.clone();
        //             if let Some(user) = usr {
        //                 self.load_data(ctx, &user);
        //                 self.state = AppState::Authenticated(MainPages::Tasks);
        //             } else {
        //                 self.first_run = true;
        //                 self.first_run(frame);
        //                 log::error!("1");
        //                 self.state = AppState::NoAuth("No user detected".to_string());
        //             }
        //         } else {
        //             self.login_page(
        //                 ctx,
        //                 self.db_tx.clone(),
        //                 self.app_state_tx.clone(),
        //             )
        //         }
        //     },
        //     AppState::CreateAccount => self.signup_page(
        //         ctx,
        //         self.db_tx.clone(),
        //         self.app_state_tx.clone(),
        //     ),
        //     _ => {}
        // }

        self.admin_notification_ui(ctx);
        self.koth.receive();
        self.query_editor.receive();
        self.receive_ui_action();
        self.receive_users();
        self.receive_task();
        // self.receive_ticket();
        self.receive_notes();
        self.receive_notification();
        self.stock_tables.receive();
        self.sales_tracker.receive();
        self.receive_client();
        self.receive_prestashop();
        self.filesystem.receive();
        self.handle_viewports(ctx);
        self.handle_modals(ctx);
        self.toasts.show(ctx);
        self.task_audit_table.receive(self.store_users.clone(), frame);

        if let Ok(releases) = self.github_releases_channel.1.try_recv() {
            log::debug!("Releases: {releases:?}");
            ctx.request_repaint();
            self.github_releases = releases;
        }

        if let Ok(settings) = self.settings_receiver.try_recv() {
            ctx.request_repaint();
            ctx.set_style(settings);
        }

        if let Ok(thread_obj) = self.ai_thread_channel.1.try_recv() {
            let mut thread_map = std::collections::HashMap::new();
            self.ai_playground.save_chats = true;
            thread_map.insert(thread_obj.id.clone(), crate::tabs::ai_playground::ChatThread {
                id: thread_obj.id.clone(),
                messages: Vec::new(),
                images: Vec::new(),
                input: String::new(),
            });
            self.ai_playground.selected_thread = thread_obj.id;
            self.ai_playground.set_threads(thread_map);
        }

        // Handle changes to state from various places, such as
        // hitting the login button, clicking the 'home page' button
        // (which is clicking Mtechserver in the top middle of the page),
        // if session cookie expires (gets checked in the first_run method),
        // if manually logged out, etc
        // match &self.state {
        //     AppState::Authenticated(MainPages::Tasks) => self.main_page(ctx),
        //     AppState::Authenticated(MainPages::Downloads) => self.downloads_page(ctx),
        //     AppState::Authenticated(MainPages::UserPreferences) => self.account_settings_page(ctx, self.app_state_tx.clone()),
        //     AppState::Authenticated(_) => self.main_page(ctx),
        //     AppState::CreateAccount => self.signup_page(
        //         ctx,
        //         self.db_tx.clone(),
        //         self.app_state_tx.clone(),
        //     ),
        //     AppState::NoAuth(reason) => {
        //         if reason.to_string().contains("Already connected") {
        //             info!("Already connected");
        //             if self.current_user.is_some() {
        //                 if !self.load_data(ctx) {
        //                     self.first_run = true;
        //                     self.first_run(frame);
        //                     self.state = AppState::NoAuth("No user detected".to_string());
        //                 }
        //             } else {
        //                 self.first_run = true;
        //                 self.first_run(frame)
        //             }
        //             self.state = AppState::Authenticated(MainPages::Tasks);
        //         } else {
        //             self.login_page(
        //                 ctx,
        //                 self.db_tx.clone(),
        //                 self.app_state_tx.clone(),
        //             )
        //         }
        //     }
        // }
    }
}

