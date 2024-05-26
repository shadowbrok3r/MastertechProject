use crate::{app_state::MtechServerContext, utilities::{display_tasks::setup_display}};
use egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = self.store_tasks.as_mut(){
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
                
            }
            let database = self.database.as_ref().unwrap().clone();
            let store_users = self.store_users.as_ref().unwrap();
            setup_display(ui, col_names, &mut *tasks, database, &store_users);
        }
    }
}

