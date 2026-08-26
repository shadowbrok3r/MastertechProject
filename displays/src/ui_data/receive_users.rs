use database::schema::{FilterLiveTasks, LiveTaskPayload, Status, Store};
use std::collections::{BTreeMap, HashSet};
use crate::app_state::SharedContext; 
use database::live_data::Action;

impl SharedContext {
    pub fn receive_users(&mut self) {
        if let Ok((action, user)) = self.live_user_rx.try_recv() {
            if let Action::Update = action {
                if self.current_user.as_ref().is_some_and(|cu| cu.get_id() == user.get_id()) {
                    self.current_user = Some(user);
                    self.layout_configs = None;
                    self.init_layout_configs();
                } else {
                    log::debug!("Received user update for a different user, ignoring.");
                }
            }
        }

        if let Ok(users) = self.store_users_rx.try_recv() {
            log::debug!("Received new store users: {} users", users.len());

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
                let statuses = u.get_statuses()
                    .into_iter()
                    .map(|status| match status {
                        Status::CustomStatus(name) => name,
                        _ => status.as_str().to_string(),
                    })
                    .collect::<HashSet<String>>();
                statuses
            });

            let new_statuses = users
                .iter()
                .find(|u| self.current_user.as_ref().map(|cu| cu.get_id() == u.get_id()).unwrap_or(false))
                .map(|u| {
                    let statuses = u.get_statuses()
                        .into_iter()
                        .map(|status| match status {
                            Status::CustomStatus(name) => name,
                            _ => status.as_str().to_string(),
                        })
                        .collect::<HashSet<String>>();
                    statuses
                });
            let statuses_changed = old_statuses != new_statuses;

            // Update store_users
            self.store_users.clear();
            self.store_users = users;

            // Reinitialize layout_configs if statuses or users changed
            if users_changed || statuses_changed {
                log::debug!("Reinitializing layout_configs (users_changed={users_changed}, statuses_changed={statuses_changed})");
                self.layout_configs = None;
                self.init_layout_configs();
            }

            // Get layout_configs
            let layout_configs = match &self.layout_configs {
                Some(configs) => configs,
                None => {
                    log::error!("layout_configs not initialized; skipping task_map updates");
                    return;
                }
            };

            // Update layouts
            let store_selection = Store::from_presta_store_id(&self.store_selection.to_string());
            let current_user = self.current_user.as_ref().cloned().unwrap_or_default();

            for (page, layout) in self.task_layouts.iter_mut() {
                let config = match layout_configs.get(page) {
                    Some(config) => config,
                    None => {
                        log::error!("No config defined for layout: {}", page);
                        layout.task_map.clear();
                        continue;
                    }
                };

                // Clear task_map, assignees, and search_inputs only if switching stores
                if (page == "StoreTasks" || page == "CompletedTasks") && self.pending_store.is_some() {
                    layout.task_map.clear();
                    layout.assignees.clear();
                    layout.search_inputs.clear();
                }

                // Rebuild task_map if users or statuses changed, or store switched
                if users_changed || statuses_changed || self.pending_store.is_some() {
                    let mut new_task_map = BTreeMap::new();
                    let tasks_to_filter = self.search_results.clone().unwrap_or_else(|| {
                        self.task_index.values().cloned().collect::<Vec<LiveTaskPayload>>()
                    });
                    
                    if page == "MyTasks" {
                        for status_str in &config.valid_keys {
                            let status = Status::from_str(status_str);
                            let filtered = tasks_to_filter
                                .filter_by_status(&status)
                                .filter_by_assignee(&current_user)
                                .into_iter()
                                .filter(|task| !task.completed)
                                .collect::<Vec<LiveTaskPayload>>();
                            log::trace!("receive_store_users: MyTasks status={}, tasks_found={}", status_str, filtered.len());
                            if !filtered.is_empty() {
                                new_task_map.entry(status_str.clone()).or_insert(filtered);
                            }
                        }
                    } else {
                        for user in self.store_users.iter() {
                            let filtered = tasks_to_filter
                                .clone()
                                .filter_by_assignee(user)
                                .filter_by_completion(page == "CompletedTasks")
                                .filter_by_store(user, &store_selection);
                            log::trace!("receive_store_users: {} user={:?}, tasks_found={}", page, user.get_initials(), filtered.len());
                            if !filtered.is_empty() {
                                new_task_map
                                    .entry(user.get_initials().to_string())
                                    .or_insert(filtered);
                            }
                        }
                    }
                    log::trace!("receive_store_users: {} task_map_keys={:?}", page, new_task_map.keys());
                    layout.task_map = new_task_map.clone();
                    layout.update_col_names(new_task_map.keys().cloned().collect());
                }

                if config.update_assignees {
                    layout.update_assignees(self.store_users.clone());
                }
                // Try to load and apply user's saved order for this page
                if let Some(user) = self.current_user.as_ref() {
                    if let Some(saved) = user.get_page_task_columns(page) {
                        layout.update_col_names(saved);
                    }
                }
            }

            // Reset pending_store if users match the new store
            if let Some(pending_store) = self.pending_store {
                if pending_store.as_str() == store_selection.as_str() {
                    self.pending_store = None;
                }
            }
        }
    }
}