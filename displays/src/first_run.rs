use database::{live_data::listen_data,schema::{utilities::{get_notifications, get_store_users, get_tasks_for_store}, Status, Store, TaskPayload, NOTIFICATION_TABLE, TASK_NOTE_TABLE, TASK_TABLE}};
use crate::{app_state::{AppState, SharedContext}, tabs::ai_playground::ChatThread, FilterTasks, PlatformSpawner, Spawner}; // virtual_filesystem::S3Fetcher, 
use crate::ui_tools::{theme_config::ThemeConfig, toasts::{Toast, ToastKind, ToastOptions}};
use std::collections::{BTreeMap, HashMap, HashSet};
use eframe::egui::Context;
use log::info;

impl SharedContext {
    pub fn load_data(&mut self, ctx: &Context) -> bool {
        self.refresh_client_list();
        self.timer = Some(web_time::Instant::now());
        // get all of our channel Senders from crossbeam to get user/store/completed tasks,
        // as well as store users and live task notifications
        let live_tasks_tx = self.live_tasks_tx.clone();
        let notes_tx = self.notes_tx.clone();
        let live_notif_tx = self.live_notification_tx.clone();        

        if let Some(usr) = self.current_user.as_ref() {
            self.store_selection = std::convert::Into::<u64>::into(usr.get_store());
            let user = usr.clone();
            let name = user.get_name();
            info!("Getting Initial data: {}", self.store_selection);
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
                let store = usr.get_store();
                let notifs_tx = self.notification_tx.clone();
                PlatformSpawner::spawn(async move {
                    let get_store_users = get_store_users(store_users_tx, store).await;
                    info!("get_store_users: {get_store_users:?}");
                });

                PlatformSpawner::spawn(async move {
                    let get_tasks = get_tasks_for_store(initial_tasks_tx, store.as_str().to_string()).await;
                    info!("get_tasks: {get_tasks:?}");
                });

                PlatformSpawner::spawn(async move {
                    let get_notifications = get_notifications(notifs_tx).await;
                    info!("get_notifications: {get_notifications:?}");
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
                info!("listen_task_notes: {listen_data:?}");
            });

            PlatformSpawner::spawn(async move {
                let listen_data = listen_data(live_tasks_tx, TASK_TABLE).await;
                info!("listen_tasks: {listen_data:?}");
            });

            PlatformSpawner::spawn(async move {
                let listen_data = listen_data(live_notif_tx.clone(), NOTIFICATION_TABLE).await;
                info!("listen_notifications: {listen_data:?}");
            });

            match serde_json::from_value::<ThemeConfig>(usr.get_color_scheme()) {
                Ok(color_settings) => {
                    self.theme_config = color_settings.clone();
                    ctx.request_repaint();
                },
                Err(e) => log::error!("Error setting theme config: {e:?}"),
            }

            let toast = &mut self.toasts;
            let auth_toast = Toast {
                kind: ToastKind::Success,
                text: format!("Logged in successfully\nWelcome, {}", name).into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(6.0),
            };
            toast.add(auth_toast);
            true
        } else {
            info!("4");
            false
        }
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &Context) {
        if let Ok(users) = self.store_users_rx.try_recv() {
            info!("Received new store users: {} users", users.len());

            // Check if store_users has changed significantly
            let old_username: HashSet<String> = self
                .store_users
                .iter()
                .map(|u| u.get_username().to_string())
                .collect();
            let new_username: HashSet<String> = users
                .iter()
                .map(|u| u.get_username().to_string())
                .collect();
            let users_changed = old_username != new_username;

            // Check if user_statuses have changed for current_user
            let old_statuses = self.current_user.as_ref().map(|u| {
                u.get_statuses()
                    .into_iter()
                    .map(|status| match status {
                        Status::CustomStatus(name) => name,
                        _ => status.as_str().to_string(),
                    })
                    .collect::<HashSet<String>>()
            });
            let new_statuses = users
                .iter()
                .find(|u| self.current_user.as_ref().map(|cu| cu.get_id() == u.get_id()).unwrap_or(false))
                .map(|u| {
                    u.get_statuses()
                    .into_iter()
                    .map(|status| match status {
                        Status::CustomStatus(name) => name,
                        _ => status.as_str().to_string(),
                    })
                    .collect::<HashSet<String>>()
                });
                
            let statuses_changed = old_statuses != new_statuses;

            // Update store_users
            self.store_users.clear();
            self.store_users = users;

            // Reinitialize layout_configs if statuses or users changed
            if users_changed || statuses_changed {
                self.layout_configs = None; // Force reinitialization
                self.init_layout_configs();
            }

            // Get layout_configs
            let layout_configs = match &self.layout_configs {
                Some(configs) => configs,
                None => {
                    log::warn!("layout_configs not initialized; skipping task_map updates");
                    return;
                }
            };

            // Update layouts
            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());
            let current_user = self.current_user.as_ref().cloned().unwrap_or_default();

            for (page, layout) in self.task_layouts.iter_mut() {
                let config = match layout_configs.get(page) {
                    Some(config) => config,
                    None => {
                        log::warn!("No config defined for layout: {}", page);
                        layout.task_map.clear();
                        continue;
                    }
                };

                // Clear task_map, assignees, and search_inputs only if switching stores
                if (page == "Store Tasks" || page == "Completed Tasks") && self.pending_store.is_some() {
                    layout.task_map.clear();
                    layout.assignees.clear();
                    layout.search_inputs.clear();
                }

                // Rebuild task_map if users or statuses changed, or store switched
                if users_changed || statuses_changed || self.pending_store.is_some() {
                    let mut new_task_map = BTreeMap::new();
                    // Use search_results if present, otherwise use all tasks
                    let tasks_to_filter = self.search_results.clone().unwrap_or_else(|| {
                        self.task_index.values().cloned().collect::<Vec<TaskPayload>>()
                    });

                    if page == "My Tasks" {
                        // Initialize by status, only include non-empty columns
                        for status_str in &config.valid_keys {
                            let status = Status::from_str(status_str);
                            let filtered = tasks_to_filter
                                .iter()
                                .cloned()
                                .collect::<Vec<TaskPayload>>()
                                .filter_by_status(&status)
                                .filter_by_assignee(&current_user)
                                .into_iter()
                                .filter(|task| !task.completed)
                                .collect::<Vec<TaskPayload>>();
                            if !filtered.is_empty() {
                                new_task_map.entry(status_str.clone()).or_insert(filtered);
                            }
                        }
                    } else {
                        // Initialize by user initials
                        for user in self.store_users.iter() {
                            let filtered = tasks_to_filter
                                .iter()
                                .cloned()
                                .collect::<Vec<TaskPayload>>()
                                .filter_by_assignee(user)
                                .filter_by_completion(page == "Completed Tasks")
                                .filter_by_store(user, &store_selection);

                            if !filtered.is_empty() {
                                new_task_map
                                    .entry(user.get_username().to_string())
                                    .or_insert(filtered);
                            }
                        }
                    }
                    // Update column_names to match task_map keys
                    layout.update_col_names(new_task_map.keys().cloned().collect());
                    layout.task_map = new_task_map;
                }

                // Update assignees if required
                if config.update_assignees {
                    layout.update_assignees(self.store_users.clone());
                }
            }

            // Reset pending_store if users match the new store
            if let Some(pending_store) = self.pending_store {
                if pending_store.as_str() == store_selection.as_str() {
                    self.pending_store = None;
                }
            }
        }

        if let Ok(settings) = self.settings_receiver.try_recv() {
            self.theme_config = settings;
        }

        if let Ok(thread_obj) = self.ai_thread_channel.1.try_recv() {
            let mut thread_map = HashMap::new();
            self.ai_playground.save_chats = true;
            thread_map.insert(thread_obj.id.clone(), ChatThread {
                id: thread_obj.id.clone(),
                messages: Vec::new(),
                images: Vec::new(),
                input: String::new(),
            });
            self.ai_playground.selected_thread = thread_obj.id;
            self.ai_playground.set_threads(thread_map);
        }

        if let Ok(state) = self.app_state_rx.try_recv() {
            info!("Got a new state: {state:?}");
            if let AppState::NoAuth(reason) = &state {
                let toast = &mut self.toasts;
                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: reason.into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
            }
            self.state = state;
            ctx.request_repaint();
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

        self.filesystem.receive();
        self.task_audit_table.receive(self.store_users.clone(), frame);
    }
}
