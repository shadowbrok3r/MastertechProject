use database::schema::{FilterLiveTasks, LiveTaskPayload, Status, Store};
use eframe::egui::{Color32, Spinner, Ui, Widget};
use crate::app_state::SharedContext;
use std::collections::BTreeMap;
use task_layout::TaskLayout;

pub mod task_cards;
pub mod task_layout;
pub mod interactable;

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

        // Always rebuild task_map to reflect current tasks
        let mut map = BTreeMap::new();
        let mut ordered_keys = Vec::new();

        // Use search_results if present, otherwise use all tasks
        let tasks_to_filter = self.search_results.clone().unwrap_or_else(|| {
            self.task_index.values().cloned().collect::<Vec<LiveTaskPayload>>()
        });

        if page == "My Tasks" {
            // Collect tasks for each status
            let mut temp_entries = Vec::new();
            for status_str in &config.valid_keys {
                let status = Status::from_str(status_str);
                let filtered = tasks_to_filter
                    .clone()
                    .filter_by_status(&status)
                    .filter_by_assignee(&current_user)
                    .into_iter()
                    .filter(|task| !task.completed)
                    .collect::<Vec<LiveTaskPayload>>();

                if !filtered.is_empty() {
                    temp_entries.push((status_str.clone(), filtered));
                }
            }
            // Sort entries: Todo first, then In Repair, then others
            temp_entries.sort_by(|(a, _), (b, _)| {
                match (a.as_str(), b.as_str()) {
                    ("Todo", _) => std::cmp::Ordering::Less,
                    (_, "Todo") => std::cmp::Ordering::Greater,
                    ("In Repair", _) => std::cmp::Ordering::Less,
                    (_, "In Repair") => std::cmp::Ordering::Greater,
                    _ => a.cmp(b),
                }
            });
            // Insert sorted entries into task_map and build ordered_keys
            for (status_str, filtered) in temp_entries {
                map.insert(status_str.clone(), filtered);
                ordered_keys.push(status_str);
            }
        } else {
            for user in self.store_users.iter() {
                let filtered = tasks_to_filter
                    .clone()
                    .filter_by_assignee(user)
                    .filter_by_completion(page == "Completed Tasks")
                    .filter_by_store(user, &store_selection);

                if !filtered.is_empty() {
                    let username = user.get_username().to_string();
                    map.insert(username.clone(), filtered);
                    ordered_keys.push(username);
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
                let mut target_ordered_keys = Vec::new();
                if *target_page == "My Tasks" {
                    let mut temp_entries = Vec::new();
                    for status_str in &target_config.valid_keys {
                        let status = Status::from_str(status_str);
                        let filtered = tasks_to_filter
                            .clone()
                            .filter_by_status(&status)
                            .filter_by_assignee(&current_user)
                            .into_iter()
                            .filter(|task| !task.completed)
                            .collect::<Vec<LiveTaskPayload>>();
                        if !filtered.is_empty() {
                            temp_entries.push((status_str.clone(), filtered));
                        }
                    }
                    // Sort entries for target page
                    temp_entries.sort_by(|(a, _), (b, _)| {
                        match (a.as_str(), b.as_str()) {
                            ("Todo", _) => std::cmp::Ordering::Less,
                            (_, "Todo") => std::cmp::Ordering::Greater,
                            ("In Repair", _) => std::cmp::Ordering::Less,
                            (_, "In Repair") => std::cmp::Ordering::Greater,
                            _ => a.cmp(b),
                        }
                    });
                    for (status_str, filtered) in temp_entries {
                        target_map.insert(status_str.clone(), filtered);
                        target_ordered_keys.push(status_str);
                    }
                } else {
                    for user in self.store_users.iter() {
                        let filtered = tasks_to_filter
                            .clone()
                            .filter_by_assignee(user)
                            .filter_by_completion(*target_page == "Completed Tasks")
                            .filter_by_store(user, &store_selection);
                        if !filtered.is_empty() {
                            let username = user.get_username().to_string();
                            target_map.insert(username.clone(), filtered);
                            target_ordered_keys.push(username);
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
                        let mut new_layout = TaskLayout::new(
                            target_map,
                            target_ordered_keys,
                            self.store_users.clone(),
                            self.search_results.clone(),
                        ); // TODO!!
                        todo!("We need to not call new() every iteration.. only when needed. otherwise we get a very verbose log 'WE HAVE A USER FROM GLOBAL STATE'");
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
                ordered_keys.clone(),
                self.store_users.clone(),
                self.search_results.clone(),
            );
            if config.update_assignees {
                layout.update_assignees(self.store_users.clone());
            }
            layout
        });

        // Update existing layout
        layout.task_map = map;
        layout.update_col_names(ordered_keys);

        // Render the layout
        layout.layout_cols(ui, self.ui_actions_tx.clone());
    }
}