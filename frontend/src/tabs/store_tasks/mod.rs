use crate::{app_state::MtechServerContext, utilities::{display_tasks::setup_display}};
use database::schema::{Status, TaskPayload};
use egui::Ui;
use crate::utilities::FilterTasks;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = &mut self.store_tasks{
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
            }

                // // Chain filter tasks by assignee, status, and priority
                // let filtered_tasks: Vec<&mut TaskPayload> = tasks.iter_mut()
                // // .filter_by_assignee(&assignee)
                // .filter_by_completed(false)
                // // .filter_by_status(status)
                // // .filter_by_priority(priority)
                // .collect();
        
            // Define a filter closure using the trait methods
            // let filtered_tasks: Vec<&mut TaskPayload> = tasks
            //     .into_iter()
            //     .filter_by_completed(false)
            //     .filter_by_status(status)
            //     .filter_by_priority(priority)
            //     .collect();
            
            let x = tasks.filter_by_completed(false);

            // setup_display(ui, col_names, );
        }
    }
}
