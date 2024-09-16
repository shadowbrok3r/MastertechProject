use crate::{
    app_state::MtechServerContext,
    utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks},
};
use database::schema::Status;
use eframe::egui::Ui;
use log::info;
use std::collections::BTreeMap;

impl MtechServerContext {
    pub fn my_tasks(&mut self, ui: &mut Ui) {
        if !self.store_users.is_empty() {
            let users = &self.store_users;
            let page = "MyTasks";
            let current_user = self.current_user.as_ref().unwrap();

            let mut vals = Status::VALUES;
            // Define the custom sort order
            let order = |name: Status| match name.as_str() {
                "Todo" => 1,
                "In Repair" => 2,
                "Complete" => 3,
                _ => 4, // Default case if there are other unexpected items
            };

            vals.sort_unstable_by_key(|x| order(x.clone()));

            if let Some(layout) = self.task_layouts.get_mut(page) {
                if self.rerun_filtering_my_tasks {
                    self.rerun_filtering_my_tasks = false;
                    log::info!("Reruning my tasks filter");
                    let mut map = BTreeMap::new();
                    let mut user_settings = [Status::Todo, Status::InRepair];
                    let mut statuses = vals
                        .iter_mut()
                        .filter(|s| user_settings.iter_mut().any(|st| st == *s))
                        .collect::<Vec<&mut Status>>();

                    info!("Statuses from user: {:?}", statuses);
                    statuses.iter_mut().for_each(|status| {
                        if Status::Complete != **status {
                            let filtered = self
                                .tasks
                                .filter_by_status(&status)
                                .filter_by_assignee(current_user); //.filter_by_my_store(users, current_user);
                            map.entry(status.as_str().to_string()).or_insert(filtered);
                        }
                    });
                    // layout.update_tasks(map);
                    layout.task_map = map;
                }
                layout.layout_cols(ui);
            } else {
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
                            .filter_by_assignee(current_user); //.filter_by_my_store(users, current_user);
                        map.entry(status.as_str().to_string()).or_insert(filtered);
                    }
                });
                let col_names = vals
                    .iter()
                    .map(|v| v.as_str().to_string())
                    .collect::<Vec<String>>();
                // let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                let layout =
                    TaskLayout::new(map, col_names, self.ui_actions_tx.clone(), users.clone());
                self.task_layouts.insert(page.to_string(), layout);
            }
        }
    }
}

