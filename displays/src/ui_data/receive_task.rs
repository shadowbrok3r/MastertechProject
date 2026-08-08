use database::{live_data::handle_live_data, schema::{get_data::get_associated_ticket, LiveTaskPayload, RecordIdExt, Status, Store, TaskHistory, TaskNotePayload}};
use crate::{app_state::SharedContext, get_current_user_from_auth, PlatformSpawner, Spawner};

impl SharedContext {
    pub fn receive_task(&mut self) {
        if let Ok(mut tasks) = self.initial_tasks_rx.try_recv() {
            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            // Clear layout-related data only if switching stores
            if self.pending_store.is_some() {
                self.task_layouts
                    .iter_mut()
                    .filter(|(page, _)| *page == "Completed Tasks" || *page == "Store Tasks")
                    .for_each(|(_, layout)| {
                        layout.task_map.clear();
                        layout.assignees.clear();
                        layout.search_inputs.clear();
                        if let Some(time) = self.timer {
                            if time.elapsed() > web_time::Duration::from_secs(5) {
                                layout.loading = false;
                            }
                        }
                    });
            }

            // Process new tasks
            let store_selection = Store::from_presta_store_id(&self.store_selection.to_string());
            let layout_configs = self.layout_configs.as_ref();

            tasks.drain(..).for_each(|new_task| {
                // Check for duplicates using task ID
                let task_id = new_task.id.key_string();
                if !self.task_index.contains_key(&task_id) {
                    // Add to global tasks and index
                    self.tasks.push(new_task.clone());
                    self.task_index.insert(task_id.clone(), new_task.clone());

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
                            let key = if layout_key == "My Tasks" {
                                match &new_task.status {
                                    Status::CustomStatus(name) => name.clone(),
                                    _ => new_task.status.as_str().to_string(),
                                }
                            } else {
                                self.store_users
                                    .iter()
                                    .find(|u| u.get_id() == new_task.assignee)
                                    .map(|u| u.get_username().to_string())
                                    .unwrap_or_default()
                            };

                            // Add task to task_map if it belongs
                            if should_include && (config.valid_keys.is_empty() || config.valid_keys.contains(&key)) {
                                let task_list = layout
                                    .task_map
                                    .entry(key.clone())
                                    .or_insert_with(Vec::new);
                                if !task_list.iter().any(|t| t.id.key_string() == task_id) {
                                    task_list.push(new_task.clone());
                                    // log::info!("Added initial task to layout: {}", layout_key);
                                }
                            }
                        
                            layout.has_run = false;
                        }
                    }
                }
            });

            // Reset pending_store if tasks were processed for the new store
            if let Some(pending_store) = self.pending_store {
                if pending_store.as_str() == store_selection.as_str() {
                    self.pending_store = None;
                }
            }
        }

        if let Ok(new_task) = self.live_tasks_rx.try_recv() {
            log::debug!("New Task Update: {:?}", new_task.0);
            
            let tx = self.new_ticket_tx.clone();
            let notes_tx = self.associated_notes_tx.clone();
            if let Some(service_num) = new_task.clone().1.service_number {
                if !service_num.is_empty() {
                    let new_task = new_task.clone();
                    PlatformSpawner::spawn(async move {
                        if let Err(e) = get_associated_ticket(tx, new_task.clone()).await {
                            log::error!("Error getting associated ticket: {e:?}");
                        }
                        match TaskNotePayload::get_db_notes_from_task_id(new_task.1.id.clone()).await {
                            Ok(notes) => { let _ = notes_tx.try_send(notes); },
                            Err(e) => log::error!("Error getting associated notes: {e:?}"),
                        }
                    });
                }
            }

            // Get existing task for diff comparison before applying changes
            let task_id = new_task.1.id.key_string();
            let existing_task: Option<LiveTaskPayload> = self.task_index.get(&task_id).cloned();

            match handle_live_data(new_task.to_owned(), &mut self.tasks) {
                Ok(_) => {
                    // Update task_index
                    self.task_index.insert(task_id.clone(), new_task.1.clone().into());
                    // Update self.tasks to maintain consistency
                    if let Some(pos) = self.tasks.iter().position(|t| t.id.key_string() == task_id) {
                        self.tasks[pos] = new_task.1.clone().into();
                    } else {
                        self.tasks.push(new_task.1.clone().into());
                    }
                    
                    // Create TaskHistory record if there were changes
                    if let Some(old_task) = existing_task {
                        let new_task_payload: LiveTaskPayload = new_task.1.clone().into();
                        if old_task.has_changes_from(&new_task_payload) {
                            let diff_json = old_task.diff_to_json(&new_task_payload);
                            
                            // Get current user for history record
                            let (user_id, username) = if let Some(user) = get_current_user_from_auth() {
                                (user.get_id(), user.get_username().to_string())
                            } else {
                                (new_task_payload.assignee.clone(), "System".to_string())
                            };
                            
                            let history = TaskHistory::new(
                                new_task_payload.id.clone(),
                                user_id,
                                username,
                                diff_json,
                            );
                            
                            // Save history record asynchronously
                            PlatformSpawner::spawn(async move {
                                if let Err(e) = history.save().await {
                                    log::error!("Error saving task history: {:?}", e);
                                }
                            });
                        }
                    }
                }
                Err(e) => {
                    log::error!("Error handling task data: {e:?}");
                }
            }
        }
    }
}
