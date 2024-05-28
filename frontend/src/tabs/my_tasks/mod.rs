use crate::{app_state::MtechServerContext, utilities::{displays::display_tasks::{setup_display, Filters}, Sortable}};
use egui::Ui;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &mut self.my_tasks{
            self.my_tasks_opened = true;

            let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
            let database = self.database.as_ref().unwrap().clone();
            tasks.sort_task_payloads();

            let filters = vec![
                Filters::FilterStatus
            ];

            setup_display(ui, 
                col_names, 
                &mut *tasks, 
                database, 
                &filters, 
                &self.store_users,
                true,
                &None,
                &None,
                &self.current_user
            );
        }
    }
}