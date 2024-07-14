use crate::{app_state::MtechServerContext, utilities::{displays::tasks::task_layout::TaskLayout, ColumnLayout, FilterTasks}};
use std::collections::BTreeMap;
use eframe::egui::Ui;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {
        if let Some(users) = self.store_users.as_ref(){
            let page = "StoreTasks";
            if let Some(layout) = self.task_layouts.get_mut(page){
                let mut map = BTreeMap::new();
                users.iter().for_each(|u| {
                    let filtered = self.tasks.filter_by_assignee(u).filter_by_completion(false);
                    map.entry(u.everest_initials.to_string()).or_insert(filtered);
                });
                layout.update_tasks(map).layout_cols(ui);
            } else {
                let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                let layout = TaskLayout::new(BTreeMap::new(), user_names, self.ui_actions_tx.clone(), users.clone());
                self.task_layouts.insert(page.to_string(), layout);
            }
        }
    }
}