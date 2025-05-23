use crate::{app_state::SharedContext, tasks::task_layout::TaskLayout, FilterTasks};
use database::schema::{Status, Store, TaskPayload};
use eframe::egui::{Color32, Spinner, Ui, Widget};
use std::collections::BTreeMap;

impl SharedContext {
pub fn render_layout(&mut self, ui: &mut Ui, page: &str) {
        ui.ctx().request_repaint();
        if self.store_users.is_empty() {
            ui.vertical_centered(|ui| {
                ui.label("Loading...");
                Spinner::new()
                    .size(50.0)
                    .color(Color32::from_rgb(150, 10, 150))
                    .ui(ui);
            });
            return;
        }

        // Initialize layout_configs
        self.init_layout_configs();

        // Ensure layout_configs exists
        let layout_configs = match &self.layout_configs {
            Some(configs) => configs,
            None => {
                log::warn!("Layout configs not initialized for {}", page);
                return;
            }
        };

        // Get or update the layout
        let Some(config) = layout_configs.get(page) else { return; };

        let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());
        let current_user = self.current_user.as_ref().cloned().unwrap_or_default();

        // Always rebuild task_map and col_names to reflect current tasks
        let mut map = BTreeMap::new();
        let mut col_names = Vec::new();

        // Use search_results if present, otherwise use all tasks
        let tasks_to_filter = self.search_results.clone().unwrap_or_else(|| {
            self.task_index.values().cloned().collect::<Vec<TaskPayload>>()
        });

        if page == "My Tasks" {
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
                    map.entry(status_str.clone()).or_insert(filtered);
                    col_names.push(status_str.clone());
                }
            }
        } else {
            col_names = (config.key_provider)(&self.store_users);
            for user in self.store_users.iter() {
                let filtered = tasks_to_filter
                    .iter()
                    .cloned()
                    .collect::<Vec<TaskPayload>>()
                    .filter_by_assignee(user)
                    .filter_by_completion(page == "Completed Tasks")
                    .filter_by_store(user, &store_selection);

                if !filtered.is_empty() {
                    map.entry(user.get_username().to_string()).or_insert(filtered);
                }
            }
        }

        // If task_map is empty and search_results exist, try switching to a page with matching tasks
        if map.is_empty() && self.search_results.is_some() {
            let other_pages = ["My Tasks", "Store Tasks", "Completed Tasks"]
                .iter()
                .filter(|&&p| p != page)
                .collect::<Vec<_>>();
            for &target_page in &other_pages {
                let target_config = layout_configs.get(*target_page).expect("Layout config not found");
                let mut target_map = BTreeMap::new();
                if *target_page == "My Tasks" {
                    for status_str in &target_config.valid_keys {
                        let status = Status::from_str(status_str);
                        let filtered = tasks_to_filter
                            .filter_by_status(&status)
                            .filter_by_assignee(&current_user)
                            .into_iter()
                            .filter(|task| !task.completed)
                            .collect::<Vec<TaskPayload>>();
                        if !filtered.is_empty() {
                            target_map.entry(status_str.clone()).or_insert(filtered);
                        }
                    }
                } else {
                    for user in self.store_users.iter() {
                        let filtered = tasks_to_filter
                            .filter_by_assignee(user)
                            .filter_by_completion(*target_page == "Completed Tasks")
                            .filter_by_store(user, &store_selection);
                        if !filtered.is_empty() {
                            target_map.entry(user.get_username().to_string()).or_insert(filtered);
                        }
                    }
                }
                if !target_map.is_empty() {
                    // Found a page with matching tasks; switch to it
                    if let Some((surface_index, node_index, tab_index)) = self.tree.find_tab(&target_page.to_string()) {
                        self.tree.set_active_tab((surface_index, node_index, tab_index));
                        // log::debug!("Switched to page {} with matching tasks", target_page);
                        // Update the current page to render the target page
                        let target_config = layout_configs.get(*target_page).expect("Layout config not found");
                        let mut target_col_names = Vec::new();
                        if *target_page == "My Tasks" {
                            for status_str in &target_config.valid_keys {
                                if target_map.contains_key(status_str) {
                                    target_col_names.push(status_str.clone());
                                }
                            }
                        } else {
                            target_col_names = (target_config.key_provider)(&self.store_users);
                            target_col_names.retain(|name| target_map.contains_key(name));
                        }
                        let mut new_layout = TaskLayout::new(
                            target_map,
                            target_col_names,
                            self.store_users.clone(),
                            self.search_results.clone(),
                        );

                        if target_config.update_assignees {
                            new_layout.update_assignees(self.store_users.clone());
                        }
                        self.task_layouts.insert(target_page.to_string(), new_layout);
                        if let Some(layout) = self.task_layouts.get_mut(*target_page) {
                             layout.layout_cols(ui, self.ui_actions_tx.clone());
                        }
                        return;
                    } else {
                        log::warn!("Tab {} not found in tree", target_page);
                    }
                }
            }
            log::debug!("No other page has matching tasks; staying on {}", page);
        }

        // Update or create layout
        let layout = self.task_layouts.entry(page.to_string()).or_insert_with(|| {
            let mut layout = TaskLayout::new(
                map.clone(),
                col_names.clone(),
                self.store_users.clone(),
                self.search_results.clone()
            );
            if config.update_assignees {
                layout.update_assignees(self.store_users.clone());
            }
            layout
        });

        // Update existing layout
        layout.task_map = map;
        layout.column_names = col_names; // Assuming TaskLayout has mutable col_names

        // Render the layout
        layout.layout_cols(ui, self.ui_actions_tx.clone());
    }
}


/* 
impl SharedContext {
    pub fn my_tasks(&mut self, ui: &mut Ui) {
        if !self.store_users.is_empty() {
            let page = "My Tasks";
            let current_user = self.current_user.as_ref().cloned().unwrap_or_default();

            let mut vals = Status::VALUES;
            // Define the custom sort order
            let order = |name: Status| match name.as_str() {
                "Todo" => 1,
                "In Repair" => 2,
                "Complete" => 3,
                _ => 4, // Default case if there are other unexpected items
            };

            vals.sort_unstable_by_key(|x| order(x.clone()));

            // Ensure the layout exists
            self.task_layouts.entry(page.to_string()).or_insert_with(|| {
                let mut map = BTreeMap::new();
                let mut user_settings = [Status::Todo, Status::InRepair];
                let mut statuses = vals
                    .iter_mut()
                    .filter(|s| user_settings.iter_mut().any(|st| st == *s))
                    .collect::<Vec<&mut Status>>();

                statuses.iter_mut().for_each(|status| {
                    if Status::Complete != **status {
                        let filtered = self
                            .tasks
                            .filter_by_status(&status)
                            .filter_by_assignee(&current_user);
                        map.entry(status.as_str().to_string()).or_insert(filtered);
                    }
                });
                TaskLayout::new(
                    map,
                    vals
                    .iter()
                    .map(|v| v.as_str().to_string())
                    .collect::<Vec<String>>(),
                    self.ui_actions_tx.clone(),
                    self.store_users.clone(),
                )
            });

            // Render the layout
            if let Some(layout) = self.task_layouts.get_mut(page) {
                layout.layout_cols(ui);
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.label("Loading..");
                Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
            });
        }
    }
}

 */