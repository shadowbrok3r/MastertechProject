use crate::{app_state::MtechServerContext, utilities::{displays::display_tasks::{setup_display, Filters}, Sortable}};
use egui::Ui;

impl MtechServerContext{
    pub fn completed_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = self.store_tasks.as_mut(){
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
                
            }
            let database = self.database.as_ref().unwrap().clone();
            let store_users = self.store_users.as_ref().unwrap();
            tasks.sort_task_payloads();

            let filters = vec![
                Filters::FilterAssignee, 
                Filters::FilterCompleted
            ];

            setup_display(ui, 
                col_names, 
                &mut *tasks, 
                database, 
                &filters, 
                &Some(store_users.to_owned()),
                false,
                &None,
                &Some(true),
                &None
            );
        }
    }
}