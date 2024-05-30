use crate::{app_state::MtechServerContext, utilities::displays::Filters};
use egui::Ui;

impl MtechServerContext{
    pub fn completed_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &self.store_tasks{
            self.completed_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
                
            }
            let database = self.database.as_ref().unwrap().clone();
            // let store_users = self.store_users.as_ref().unwrap();
            // tasks.sort_task_payloads();

            let filters = vec![
                Filters::FilterAssignee, 
                Filters::FilterCompleted
            ];
            
            self.initialize_task_layout("completed_tasks", tasks.to_owned(), col_names, database, filters);

            if let Some(task_layout) = self.task_layouts.get_mut("completed_tasks"){
                task_layout.display(
                    ui, 
                    &self.store_users, 
                    false, 
                    &None, 
                    &Some(true), 
                    &None
                );
            }
        }

        if self.completed_tasks_opened{

        }else{

        }
    }
}