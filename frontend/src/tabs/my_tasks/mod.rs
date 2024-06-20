use crate::{app_state::MtechServerContext, utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks}};
use database::schema::{Status, TaskPayload};
use egui::Ui;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        if let Some(tasks) = self.tasks.clone(){
            if let Some(users) = self.store_users.as_ref(){
                self.my_tasks_opened = true;
                let page = "my_tasks";
                let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
                let database = self.database.as_ref().unwrap().clone();   
                let current_user = self.current_user.as_ref().unwrap();

                self.task_map.clear();
                let tasks_by_column = &mut self.task_map;

                for mut status in Status::VALUES{
                    let filtered: Vec<TaskPayload> = tasks
                        .filter_by_status(&status)
                        .filter_by_assignee(current_user);
                    tasks_by_column.insert(status.as_str().to_string(), filtered);
                }
                
                if !self.task_layouts.contains_key(page) {
                    let task_layout_opts = TaskLayout::new(
                        tasks_by_column.clone(),
                        col_names,
                        database,
                        self.ui_actions_tx.clone(),
                        Some(users.clone()),
                    );
                    self.task_layouts.insert(page.to_string(), task_layout_opts);
                } else if let Some(task_layout) = self.task_layouts.get_mut(page) {
                    task_layout.display(ui);
                }
            }
        }
    }
}