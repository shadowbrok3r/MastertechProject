use crate::{app_state::MtechServerContext, utilities::{display_tasks::setup_display, FilterTasks}};
use database::schema::{Status, TaskPayload};
use egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = self.store_tasks.as_mut(){
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();
            // let mut filtered_tasks = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
                
            }
            


            setup_display(ui, col_names, &mut *tasks, self.database.as_ref().unwrap().clone());
        }
    }
}

