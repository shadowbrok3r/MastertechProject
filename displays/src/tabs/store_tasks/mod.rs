use crate::{app_state::SharedContext, tasks::task_layout::TaskLayout, FilterTasks};
use eframe::egui::{Color32, Spinner, Ui, Widget};
use std::collections::BTreeMap;

impl SharedContext {
    pub fn store_tasks(&mut self, ui: &mut Ui) {
        if !self.store_users.is_empty() {
            let users = &self.store_users;
            let page = "StoreTasks";
            let current_user = self.current_user.as_ref().unwrap();
            if let Some(layout) = self.task_layouts.get_mut(page) {
                if self.rerun_filtering_store_tasks {
                    self.rerun_filtering_store_tasks = false;
                    log::info!("Reruning filters for STORE tasks: {:?}", self.tasks.len());
                    let mut map = BTreeMap::new();
                    users.iter().for_each(|u| {
                        if u.store == current_user.store && u.email != current_user.email {
                            let filtered =
                                self.tasks.filter_by_assignee(u).filter_by_completion(false); //.filter_by_my_store(users, current_user);
                            map.entry(u.everest_initials.to_string())
                                .or_insert(filtered);
                            log::info!(
                                "STORE tasks map: {:?}", 
                                map
                                    .iter()
                                    .map(|m| m.0.clone())
                                    .collect::<Vec<String>>()
                            );
                        }
                    });
                    layout.task_map = map;
                }

                layout.layout_cols(ui);
            } else {
                log::info!("No layout");
                let mut map = BTreeMap::new();
                users.iter().for_each(|u| {
                    if u.store == current_user.store && u.email != current_user.email {
                        let filtered = self.tasks.filter_by_assignee(u).filter_by_completion(false); //.filter_by_my_store(users, current_user);
                        map.entry(u.everest_initials.to_string())
                            .or_insert(filtered);
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

