use crate::{app_state::MtechServerContext, utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks}};
use std::collections::BTreeMap;
use eframe::egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {
        if let Some(users) = self.store_users.as_ref(){
            let page = "StoreTasks";
            let current_user = self.current_user.as_ref().unwrap();
            if let Some(layout) = self.task_layouts.get_mut(page){
                if self.rerun_filtering_store_tasks{
                    self.rerun_filtering_store_tasks = false;
                    let mut map = BTreeMap::new();
                    users.iter().for_each(|u| {
                        let filtered = self.tasks.filter_by_assignee(u).filter_by_completion(false);
                        map.entry(u.everest_initials.to_string()).or_insert(filtered);
                    });
                    layout.task_map = map;
                }

                layout.layout_cols(ui);
            } else {
                let mut map = BTreeMap::new();
                users.iter().for_each(|u| {
                    if u.store == current_user.store{
                    let filtered = self.tasks.filter_by_assignee(u).filter_by_completion(false); //.filter_by_my_store(users, current_user);
                        map.entry(u.everest_initials.to_string()).or_insert(filtered);
                    }
                });
                let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                let mut layout = TaskLayout::new(BTreeMap::new(), user_names, self.ui_actions_tx.clone(), users.clone());
                layout.update_tasks(map);
                self.task_layouts.insert(page.to_string(), layout);
            }
        }
    }
}