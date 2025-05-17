
use database::{live_data::listen_data,schema::{utilities::{get_notifications, get_store_users, get_tasks_for_store}, Status, Store, TaskPayload, NOTIFICATION_TABLE, TASK_NOTE_TABLE, TASK_TABLE}};
use crate::{app_state::SharedContext, tabs::ai_playground::ChatThread, FilterTasks, PlatformSpawner, Spawner}; // virtual_filesystem::S3Fetcher, 
use crate::ui_tools::{theme_config::ThemeConfig, toasts::{Toast, ToastKind, ToastOptions}};
use std::collections::{BTreeMap, HashMap};
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
                    .filter(|(page, _)| *page == "CompletedTasks" || *page == "StoreTasks")
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

    pub fn receive(&mut self, frame: &mut eframe::Frame, _ctx: &Context) {
        if let Ok(mut tasks) = self.initial_tasks_rx.try_recv() {
            // info!("Received initial task payload with {} tasks", tasks.len());

            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            // Clear layout-related data for specific pages when switching stores
            self.task_layouts
                .iter_mut()
                .filter(|(page, _)| *page == "CompletedTasks" || *page == "StoreTasks")
                .for_each(|(_, layout)| {
                    if self.switching_store {
                        layout.task_map.clear();
                        layout.assignees.clear();
                        layout.search_inputs.clear();
                    }
                    if let Some(time) = self.timer {
                        if time.elapsed() > web_time::Duration::from_secs(5) {
                            layout.loading = false;
                        }
                    }
                });

            // Process new tasks
            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());
            let layout_configs = self.layout_configs.as_ref();

            tasks.drain(..).for_each(|new_task| {
                // Check for duplicates using task ID
                if !self
                    .tasks
                    .iter()
                    .any(|task| task.id.key().to_string() == new_task.id.key().to_string())
                {
                    // Add to global tasks
                    let new_task_payload: TaskPayload = new_task.clone();
                    self.tasks.push(new_task_payload.clone());

                    // Distribute to layouts if layout_configs is initialized
                    if let Some(layout_configs) = layout_configs {
                        for (layout_key, layout) in self.task_layouts.iter_mut() {
                            let Some(config) = layout_configs.get(layout_key) else {
                                log::warn!("No config defined for layout: {}", layout_key);
                                continue;
                            };

                            // Check if the task belongs in this layout
                            let should_include = (config.filter)(
                                &new_task.clone().into(),
                                &self.current_user,
                                &self.store_users,
                                &store_selection,
                            );

                            // Determine the task_map key
                            let key = if layout_key == "MyTasks" {
                                new_task.status.as_str().to_string()
                            } else {
                                self.store_users
                                    .iter()
                                    .find(|u| u.get_id() == new_task.assignee)
                                    .map(|u| u.get_initials().to_string())
                                    .unwrap_or_default()
                            };

                            // Add task to task_map if it belongs
                            if should_include && (config.valid_keys.is_empty() || config.valid_keys.contains(&key)) {
                                let task_list = layout
                                    .task_map
                                    .entry(key.clone())
                                    .or_insert_with(Vec::new);
                                if !task_list
                                    .iter()
                                    .any(|t| t.id.key().to_string() == new_task.id.key().to_string())
                                {
                                    task_list.push(new_task_payload.clone());
                                    // info!("Added initial task to layout: {}", layout_key);
                                }
                            }
                        }
                    }
                }
            });

            // Reset switching_store flag if set
            if self.switching_store {
                self.switching_store = false;
            }
        }

        if let Ok(users) = self.store_users_rx.try_recv() {
            info!("Received new store users: {} users", users.len());

            // Update store_users
            self.store_users.clear();
            self.store_users = users;

            // Initialize layout_configs now that store_users is available
            self.init_layout_configs();

            // Get layout_configs, if initialized
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
                        continue;
                    }
                };

                // Clear task_map, assignees, and search_inputs for StoreTasks and CompletedTasks
                if page == "StoreTasks" || page == "CompletedTasks" {
                    layout.task_map.clear();
                    layout.assignees.clear();
                    layout.search_inputs.clear();
                }

                // Rebuild task_map
                let mut new_task_map = BTreeMap::new();
                if page == "MyTasks" {
                    // Initialize by status
                    for status_str in &config.valid_keys {
                        let status = Status::from_str(status_str);
                        let filtered = self
                            .tasks
                            .filter_by_status(&status)
                            .filter_by_assignee(&current_user);
                        new_task_map.entry(status_str.clone()).or_insert(filtered);
                    }
                } else {
                    // Initialize by user initials
                    for user in self.store_users.iter() {
                        let filtered = self
                            .tasks
                            .filter_by_assignee(user)
                            .filter_by_completion(page == "CompletedTasks")
                            .filter_by_store(user, &store_selection);
                        if !filtered.is_empty() {
                            new_task_map
                                .entry(user.get_initials().to_string())
                                .or_insert(filtered);
                        }
                    }
                }

                layout.task_map = new_task_map;

                // Update assignees if required
                if config.update_assignees {
                    layout.update_assignees(self.store_users.clone());
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

        self.filesystem.receive();
        self.task_audit_table.receive(self.store_users.clone(), frame);
    }
}
