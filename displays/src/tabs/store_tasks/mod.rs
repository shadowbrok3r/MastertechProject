use crate::{app_state::SharedContext, tasks::task_layout::TaskLayout, FilterTasks};
use database::schema::Store;
use eframe::egui::{Color32, Spinner, Ui, Widget};
use std::collections::BTreeMap;

impl SharedContext {
    pub fn store_tasks(&mut self, ui: &mut Ui) {
        ui.ctx().request_repaint();
        if !self.store_users.is_empty() {
            let users = &self.store_users;
            // log::info!("StoreTasks Store users: {:?}", users);
            let page = "StoreTasks";
            let current_user = self.current_user.as_ref().unwrap();
            let store_sel = self.store_selection.clone();
            // info!("Store_sel: {:?}", store_sel);
            let store_selection = std::convert::Into::<Store>::into(store_sel);
            if let Some(layout) = self.task_layouts.get_mut(page) {
                if self.rerun_filtering_store_tasks {
                    self.rerun_filtering_store_tasks = false;
                    // log::info!("Reruning filters for STORE tasks: {:?}", self.tasks.len());
                    let mut map = BTreeMap::new();
                    users.iter().for_each(|u| {
                        if u.email != current_user.email { // u.store == current_user.store && 
                            // info!("Reruning filters -> store_selection: {store_selection:?}");
                            let filtered = self
                                .tasks
                                .filter_by_assignee(u)
                                .filter_by_completion(false)
                                .filter_by_store(u, &store_selection);
                            
                            if !filtered.is_empty() {
                                map.entry(u.everest_initials.to_string())
                                    .or_insert(filtered);
                            }
                        }
                    });
                    layout.task_map = map;
                    layout.update_assignees(users.clone());
                }

                layout.layout_cols(ui);
            } else {
                log::info!("No layout");
                let mut map = BTreeMap::new();
                users.iter().for_each(|u| {
                    if u.store == current_user.store && u.email != current_user.email {
                        // info!("No layout -> store_selection: {store_selection:?}");
                        let filtered = self
                            .tasks
                            .filter_by_assignee(u)
                            .filter_by_completion(false)
                            .filter_by_store(u, &store_selection);

                        if !filtered.is_empty() {
                            map.entry(u.everest_initials.to_string())
                                .or_insert(filtered);
                        }
                    }
                });
                let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                let layout =
                    TaskLayout::new(map, user_names, self.ui_actions_tx.clone(), users.clone());
                self.task_layouts.insert(page.to_string(), layout);
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.label("Loading..");
                Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
            });
        }
    }
}

