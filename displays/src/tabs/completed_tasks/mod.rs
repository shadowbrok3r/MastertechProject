use std::collections::BTreeMap;
use crate::{app_state::SharedContext, tasks::task_layout::TaskLayout, FilterTasks};
use database::schema::Store;
use eframe::egui::{Color32, Spinner, Ui, Widget};

impl SharedContext{
    pub fn completed_tasks(&mut self, ui: &mut Ui){ 
        ui.ctx().request_repaint();
        if !self.store_users.is_empty(){
            let users = &self.store_users;
            let page = "CompletedTasks";
            // let current_user = self.current_user.as_ref().unwrap();
            if let Some(layout) = self.task_layouts.get_mut(page){
                if self.rerun_filtering_completed{
                    self.rerun_filtering_completed = false;
                    log::info!("Reruning filters for COMPLETED tasks: {:?}", self.tasks.len());
                    let mut map = BTreeMap::new();
                    users.iter().for_each(|u| {
                        let store_sel = self.store_selection.clone();
                        let store_selection = std::convert::Into::<Store>::into(store_sel);
                        let filtered = self
                            .tasks
                            .filter_by_assignee(u)
                            .filter_by_completion(true)
                            .filter_by_store(u, &store_selection);

                        if !filtered.is_empty() {
                            map.entry(u.everest_initials.to_string())
                                .or_insert(filtered);
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
                    let store_sel = self.store_selection.clone();
                    let store_selection = std::convert::Into::<Store>::into(store_sel);
                    let filtered = self
                        .tasks
                        .filter_by_assignee(u)
                        .filter_by_completion(true)
                        .filter_by_store(u, &store_selection);

                    if !filtered.is_empty() {
                        map.entry(u.everest_initials.to_string())
                            .or_insert(filtered);
                    }
                });
                let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                // let user_initials: Vec<String> = users.iter().map(|u| u.everest_initials.clone()).collect();
                let layout = TaskLayout::new(map, user_names, self.ui_actions_tx.clone(), users.clone());
                self.task_layouts.insert(page.to_string(), layout);
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.label("Loading..");
                Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui)
            });
        }
    }
}