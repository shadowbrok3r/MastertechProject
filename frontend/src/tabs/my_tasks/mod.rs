use crate::{app_state::MtechServerContext, utilities::{ColumnLayout, FilterTasks}};
use database::schema::{Status, TaskPayload};
use eframe::egui::Ui;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        if let Some(users) = self.store_users.as_ref(){
            // let temp_map: BTreeMap<String, Vec<TaskPayload>> = std::mem::replace(&mut self.task_map, BTreeMap::new());
            self.task_map.clear();
            let mut col_names = Vec::new();
            col_names.push("Complete".to_string());
            col_names.push("In Repair".to_string());
            col_names.push("Todo".to_string());
            let current_user = self.current_user.as_ref().unwrap();
            let mut vals = Status::VALUES;
                // Define the custom sort order
            let order = |mut name: Status| match name.as_str() {
                "Todo" => 1,
                "In Repair" => 2,
                "Complete" => 3,
                _ => 4, // Default case if there are other unexpected items
            };
            
            vals.sort_unstable_by_key(|x| order(x.clone()));

            for mut status in vals{
                let filtered: Vec<TaskPayload> = self.tasks
                    .filter_by_status(&status)
                    .filter_by_assignee(current_user);
                
                self.task_map.entry(status.as_str().to_string()).or_insert(filtered);
                self.task_layout.update_tasks(self.task_map.to_owned());
            }
        
            self.task_layout
                .update_assignees(Some(users.clone()))
                .update_col_names(col_names)
                .layout_cols(ui);
        }
    }
}