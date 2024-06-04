use crate::{app_state::MtechServerContext, utilities::FilterTasks};
use database::schema::{Status, TaskPayload};
use egui::Ui;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        if let Some(tasks) = self.tasks.clone(){
            self.my_tasks_opened = true;
            let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
            let database = self.database.as_ref().unwrap().clone();

            self.initialize_task_layout("my_tasks", col_names.clone(), database); 

            if let Some(task_layout) = self.task_layouts.get_mut("my_tasks"){
                self.task_map.clear();
                let tasks_by_column = &mut self.task_map;
                let current_user = self.current_user.as_ref().unwrap();
                for mut status in Status::VALUES{
                        let filtered: Vec<TaskPayload> = tasks
                            .filter_by_status(&status)
                            .filter_by_assignee(current_user);
                        tasks_by_column.insert(status.as_str().to_string(), filtered);
                }
                
                task_layout.display(ui,  &self.store_users, tasks_by_column.to_owned());
            }
        }
    }
}