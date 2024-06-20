use crate::{app_state::MtechServerContext, utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks}};
use database::schema::TaskPayload;
use egui::Ui;

impl MtechServerContext{
    pub fn completed_tasks(&mut self, ui: &mut Ui){ 
        if let Some(tasks) = self.tasks.clone(){
            if let Some(users) = self.store_users.as_ref(){
                self.completed_tasks_opened = true;
                let mut col_names = Vec::new();

                let page = "completed_tasks";
                let database = self.database.as_ref().unwrap().clone();   

                self.task_map.clear();
                let tasks_by_column = &mut self.task_map;

                for user in users{ 
                    col_names.push(user.everest_initials.clone()); 
                    let filtered: Vec<TaskPayload> = tasks
                        .filter_by_assignee(&user)
                        .filter_by_completion(true);
                    tasks_by_column.insert(user.everest_initials.to_string(), filtered);
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