use crate::{app_state::MtechServerContext, utilities::FilterTasks};
use database::schema::TaskPayload;
use egui::Ui;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {
        // ui.style_mut().always_scroll_the_only_direction = false;
        if let Some(tasks) = self.tasks.clone(){
            self.store_tasks_opened = true;

            let mut col_names = Vec::new();
            if let Some(users) = self.store_users.as_ref(){
                for user in users{ col_names.push(user.everest_initials.clone()); }
                let database = self.database.as_ref().unwrap().clone();   
                self.initialize_task_layout("store_tasks", col_names.clone(), database); 
            }
            
            if let Some(task_layout) = self.task_layouts.get_mut("store_tasks") {
                let store_users = self.store_users.as_ref();
                self.task_map.clear();
                let tasks_by_column = &mut self.task_map;
                if let Some(users) = store_users{
                    for user in users.iter() {
                        let filtered: Vec<TaskPayload> = tasks
                            .filter_by_assignee(&user)
                            .filter_by_completion(false);
                        tasks_by_column.insert(user.everest_initials.to_string(), filtered);
                    }
                }

                task_layout.display(ui,  &self.store_users, tasks_by_column.to_owned());
            }
        }
    }
}

