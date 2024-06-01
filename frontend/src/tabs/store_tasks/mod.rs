use crate::{app_state::MtechServerContext, utilities::displays::Filters};
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

            self.initialize_task_layout("store_tasks", tasks.to_owned(), col_names, database, filters, self.ticket_data_tx.clone());

            if let Some(task_layout) = self.task_layouts.get_mut("store_tasks"){
                task_layout.display(
                    ui, 
                    &self.store_users, 
                    false, 
                    &None, 
                    &Some(false), 
                    &None
                );
            }
        }
    }
}

