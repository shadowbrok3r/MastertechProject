use crate::{app_state::MtechServerContext, utilities::{displays::{task_layout::{TaskLayout, TaskLayoutOpts}, Filters}, ColumnLayout, Sortable}};
use egui::Ui;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = self.store_tasks.as_mut(){
            self.store_tasks_opened = true;
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

            
            // tasks.layout_task_colsui, 
            //     col_names, 
            //     database, 
            //     &filters, 
            //     &Some(store_users.to_owned()),
            //     false,
            //     &None,
            //     &Some(false),
            //     &None
            // );
            let task_layout_opts = TaskLayoutOpts::new(
                tasks.to_owned(), 
                filters,
                col_names,
                database.clone()
            );

            self.task_layout.task_opts = Some(task_layout_opts);

            
            self.task_layout.display(
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

