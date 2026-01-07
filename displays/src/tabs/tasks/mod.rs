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

        let store_selection = Store::from_presta_store_id(&self.store_selection.to_string());
        let current_user = self.current_user.as_ref().cloned().unwrap_or_default();

        // Always rebuild task_map to reflect current tasks
        let mut map = BTreeMap::new();
        let mut ordered_keys = Vec::new();

        // Use search_results if present, otherwise use all tasks
        let tasks_to_filter = self.search_results.clone().unwrap_or_else(|| {
            self.task_index.values().cloned().collect::<Vec<LiveTaskPayload>>()
        });

        if page == "My Tasks" {
            // Collect tasks for each status and include empty columns so saved order persists
            for status_str in &config.valid_keys {
                let status = Status::from_str(status_str);
                let filtered = tasks_to_filter
                    .clone()
                    .filter_by_status(&status)
                    .filter_by_assignee(&current_user)
                    .into_iter()
                    .filter(|task| !task.completed)
                    .collect::<Vec<LiveTaskPayload>>();

                map.insert(status_str.clone(), filtered);
                ordered_keys.push(status_str.clone());
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

        // Apply user's saved column order for this page, if any
        if let Some(user) = self.current_user.as_ref() {
            if let Some(saved) = user.get_page_task_columns(page) {
                let mut applied: Vec<String> = Vec::new();
                for k in saved.iter() {
                    if map.contains_key(k) && !applied.contains(k) {
                        applied.push(k.clone());
                    }
                }
                for k in ordered_keys.iter() {
                    if !applied.contains(k) {
                        applied.push(k.clone());
                    }
                }
                ordered_keys = applied;
            }
        }

        // When searching, render the best-matching page's layout ephemerally in this tab
        if self.search_results.is_some() {
            // Helper to build the map for a target page and return (map, ordered_keys, total_count)
            let mut build_for_page = |target_page: &str| -> (BTreeMap<String, Vec<LiveTaskPayload>>, Vec<String>, usize) {
                let mut target_map = BTreeMap::new();
                let mut target_ordered_keys = Vec::new();
                if target_page == "My Tasks" {
                    let mut temp_entries: Vec<(String, Vec<LiveTaskPayload>)> = Vec::new();
                    if let Some(cfg) = layout_configs.get(target_page) {
                        for status_str in &cfg.valid_keys {
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
                    }
                    temp_entries.sort_by(|(a, _), (b, _)| match (a.as_str(), b.as_str()) {
                        ("Todo", _) => std::cmp::Ordering::Less,
                        (_, "Todo") => std::cmp::Ordering::Greater,
                        ("In Repair", _) => std::cmp::Ordering::Less,
                        (_, "In Repair") => std::cmp::Ordering::Greater,
                        _ => a.cmp(b),
                    });
                    for (status_str, filtered) in temp_entries {
                        target_ordered_keys.push(status_str.clone());
                        target_map.insert(status_str, filtered);
                    }
                } else {
                    for user in self.store_users.iter() {
                        let filtered = tasks_to_filter
                            .clone()
                            .filter_by_assignee(user)
                            .filter_by_completion(target_page == "Completed Tasks")
                            .filter_by_store(user, &store_selection);
                        if !filtered.is_empty() {
                            let username = user.get_username().to_string();
                            target_ordered_keys.push(username.clone());
                            target_map.insert(username, filtered);
                        }
                    }
                }
                let total_count: usize = target_map.values().map(|v| v.len()).sum();
                (target_map, target_ordered_keys, total_count)
            };

            // Compute best page by precedence order
            let pages = ["My Tasks", "Store Tasks", "Completed Tasks"];
            let mut best_page: Option<(&str, BTreeMap<String, Vec<LiveTaskPayload>>, Vec<String>)> = None;
            let mut best_count = 0usize;
            for p in pages.iter() {
                let (p_map, p_order, p_count) = build_for_page(p);
                if p_count > 0 && (best_page.is_none() || p_count > best_count) {
                    best_count = p_count;
                    best_page = Some((p, p_map, p_order));
                }
            }

            if let Some((best, target_map, mut target_ordered_keys)) = best_page {
                // If the best page is the current page, continue with the normal path below
                if best != page {
                    // Apply user's saved order to target_ordered_keys if any (for the best page)
                    if let Some(user) = self.current_user.as_ref() {
                        if let Some(saved) = user.get_page_task_columns(best) {
                            let mut applied: Vec<String> = Vec::new();
                            for k in saved.iter() {
                                if target_map.contains_key(k) && !applied.contains(k) { applied.push(k.clone()); }
                            }
                            for k in target_ordered_keys.iter() {
                                if !applied.contains(k) { applied.push(k.clone()); }
                            }
                            target_ordered_keys = applied;
                        }
                    }

                    // Ephemeral render of the best-page layout inside the current tab
                    let mut temp_layout = TaskLayout::new(
                        target_map,
                        target_ordered_keys,
                        self.store_users.clone(),
                        self.search_results.clone(),
                        best.to_string(),
                        current_user.clone(),
                    );
                    if let Some(cfg) = layout_configs.get(best) {
                        if cfg.update_assignees {
                            temp_layout.update_assignees(self.store_users.clone());
                        }
                    }
                    temp_layout.layout_cols(ui, self.ui_actions_tx.clone());
                    return;
                }
            }
        }

        // Update or create layout
        let layout = self.task_layouts.entry(page.to_string()).or_insert_with(|| {
            let mut layout = TaskLayout::new(
                map.clone(),
                ordered_keys.clone(),
                self.store_users.clone(),
                self.search_results.clone(),
                page.to_string(),
                current_user.clone(),
            );
            // Try to load and apply user's saved order for this page
            if let Some(user) = self.current_user.as_ref() {
                if let Some(saved) = user.get_page_task_columns(page) {
                    layout.update_col_names(saved);
                }
            }
            if config.update_assignees {
                layout.update_assignees(self.store_users.clone());
            }
            layout
        });

        // Update existing layout
        layout.task_map = map;
        // Preserve current layout order; just merge in any new/removed columns
        layout.update_col_names(ordered_keys);

        // Render the layout
        layout.layout_cols(ui, self.ui_actions_tx.clone());
    }
}