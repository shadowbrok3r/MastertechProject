use std::collections::HashMap;

use crate::{app_state::MtechServerContext, utilities::FilterTasks};
use database::schema::{Status, TaskPayload};
use egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        if let Some(tasks) = self.my_tasks.clone(){
            self.my_tasks_opened = true;
            let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
            let database = self.database.as_ref().unwrap().clone();
            // let filters = vec![ Filters::FilterStatus, Filters::FilterAssignee ];

            self.initialize_task_layout("my_tasks", tasks.to_owned(), col_names, database); 

            if let Some(task_layout) = self.task_layouts.get_mut("my_tasks"){
                
                let current_user = self.current_user.as_ref().unwrap();
                
                let x = || -> HashMap<String, Vec<TaskPayload>> {
                    let mut tasks_by_column: HashMap<String, Vec<TaskPayload>> = HashMap::new();
                    let mut filtered_tasks = Vec::new();
                    for mut status in Status::VALUES{
                        // let col_name = *status.as_str().to_string();
                        let filtered = tasks
                            .filter_by_status(&status)
                            .filter_by_assignee(current_user);
                        filtered_tasks.extend(filtered);
                        tasks_by_column.insert(status.as_str().to_string(), filtered_tasks.clone());
                    }
                    // info!("filtered tasks hashmap: {:?}", tasks_by_column);
                    tasks_by_column.to_owned()
                };
                // info!("X: {:?}", x());
                task_layout.display(
                    ui, 
                    &self.store_users, 
                    // true, 
                    // &None, 
                    // &None, 
                    // &self.current_user,
                    x
                );
            }

        }
    }
}