use crate::{app_state::MtechServerContext, utilities::{ColumnLayout, FilterTasks}};
use database::schema::TaskPayload;
use eframe::egui::Ui;

impl MtechServerContext{
    pub fn completed_tasks(&mut self, ui: &mut Ui){ 
        let mut col_names = Vec::new();
        if let Some(users) = self.store_users.as_ref(){
            // let temp_map: BTreeMap<String, Vec<TaskPayload>> = std::mem::replace(&mut self.task_map, BTreeMap::new());
            self.task_map.clear();
            for user in users.iter() {
                col_names.push(user.everest_initials.clone());
                col_names.sort();
                let filtered: Vec<TaskPayload> = self.tasks
                    .filter_by_assignee(user)
                    .filter_by_completion(true);
                self.task_map.entry(user.everest_initials.to_string()).or_insert(filtered);
            }
        
            self.task_layout.update_assignees(Some(users.clone()))
                .update_col_names(col_names)
                .update_tasks(self.task_map.clone())
                .layout_cols(ui);
        }
    }
}