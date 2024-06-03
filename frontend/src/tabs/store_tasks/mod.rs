use crate::{app_state::MtechServerContext, utilities::{displays::Filters, FilterTasks}};
use database::schema::{Priority, TaskPayload, User};
use egui::Ui;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = &self.my_tasks{
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
                
            }
            let database = self.database.as_ref().unwrap().clone();

            let filters = vec![
                Filters::FilterAssignee, 
                Filters::FilterCompleted
            ];



            // self.initialize_task_layout("store_tasks", tasks.to_owned(), col_names, database, filters); // , self.ticket_data_tx.clone()

            if let Some(task_layout) = self.task_layouts.get_mut("store_tasks"){

                let filter_items = 
                    | filters: &Vec<Filters>, assignee: &Option<&User>, status: &Option<bool>, priority: &Option<Priority>, complete: &Option<bool>
                    | -> Vec<TaskPayload> 
                {
                    let mut filtered_tasks = tasks.clone();
                    if let Some(users) = &self.store_users{
                        
                        for user in users{
                            filtered_tasks = tasks
                                .filter_by_assignee(user)
                                .filter_by_completed(true);
                        }
                        filtered_tasks.clone()
                    }else{
                        tasks.clone()
                    }
                };
                task_layout.display(
                    ui, 
                    &self.store_users, 
                    false, 
                    &None, 
                    &Some(false), 
                    &None,
                    filter_items
                );
            }
        }
    }
}

