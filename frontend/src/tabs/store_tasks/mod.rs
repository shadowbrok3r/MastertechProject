use crate::{app_state::MtechServerContext, utilities::FilterTasks};
use database::schema::TaskPayload;
use egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {
        
        if let Some(tasks) = self.my_tasks.clone(){
            self.store_tasks_opened = true;

            let mut col_names = Vec::new();
            for user in self.store_users.as_ref().unwrap(){ col_names.push(user.everest_initials.clone()); }
            let database = self.database.as_ref().unwrap().clone();

            self.initialize_task_layout("store_tasks", tasks.to_owned(), col_names, database); 
            
            if let Some(task_layout) = self.task_layouts.get_mut("store_tasks") {
                
                let store_users = self.store_users.as_ref();
                let filtered_tasks: Vec<TaskPayload> = tasks;
                let filter_items = || -> Vec<TaskPayload> {
                    
                    if let Some(users) = &store_users {
                        for user in *users {
                            filtered_tasks
                                .filter_by_assignee(&user)
                                .filter_by_completion(false);
                        }
                    }
                    // info!("Task: {filtered_tasks:?}");
                    filtered_tasks.to_owned()
                };

                // info!("filtered_iitems: {:?}", filter_items());
                task_layout.display(
                    ui,
                    &self.store_users,
                    // false,
                    // &None,
                    // &Some(false),
                    // &None,
                    filter_items
                );
            }
        }
    }
}

