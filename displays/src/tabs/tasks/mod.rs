use database::schema::{Status, Store};
use crate::{app_state::SharedContext, tasks::task_layout::TaskLayout, FilterTasks};
use eframe::egui::{Color32, Spinner, Ui, Widget};
use std::collections::BTreeMap;

impl SharedContext {
    pub fn render_layout(&mut self, ui: &mut Ui, page: &str) {
        ui.ctx().request_repaint(); // Ensure continuous updates
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

        // Initialize layout_configs if needed
        self.init_layout_configs();

        // Ensure layout_configs exists
        let layout_configs = match &self.layout_configs {
            Some(configs) => configs,
            None => {
                log::warn!("Layout configs not initialized for {}", page);
                return;
            }
        };

        // Ensure the layout exists
        self.task_layouts.entry(page.to_string()).or_insert_with(|| {
            let config = layout_configs
                .get(page)
                .expect(&format!("Layout config not found for {}", page));
            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());
            let current_user = self.current_user.as_ref().cloned().unwrap_or_default();

            let mut map = BTreeMap::new();
            let col_names = (config.key_provider)(&self.store_users);

            if page == "MyTasks" {
                // Initialize by status
                for status_str in &config.valid_keys {
                    let status = Status::from_str(status_str);
                    let filtered = self
                        .tasks
                        .filter_by_status(&status)
                        .filter_by_assignee(&current_user);
                    map.entry(status_str.clone()).or_insert(filtered);
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
                        map.entry(user.everest_initials.to_string())
                            .or_insert(filtered);
                    }
                }
            }

            let mut layout = TaskLayout::new(
                map,
                col_names,
                self.ui_actions_tx.clone(),
                self.store_users.clone(),
            );
            if config.update_assignees {
                layout.update_assignees(self.store_users.clone());
            }
            layout
        });

        // Render the layout
        if let Some(layout) = self.task_layouts.get_mut(page) {
            layout.layout_cols(ui);
        }
    }
}

impl SharedContext {
    pub fn my_tasks(&mut self, ui: &mut Ui) {
        if !self.store_users.is_empty() {
            let page = "MyTasks";
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

