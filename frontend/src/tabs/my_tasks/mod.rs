use crate::{app_state::MtechServerContext, utilities::{displays::{modal::Modal, task_layout::{TaskLayout, TaskLayoutOpts}, Filters}, Displayable}};
use egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &mut self.my_tasks{
            self.my_tasks_opened = true;

            let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
            let database = self.database.as_ref().unwrap().clone();
            

            let filters = vec![
                Filters::FilterStatus
            ];

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
                true, 
                &None, 
                &None, 
                &self.current_user
            );
            

        }
    }
}