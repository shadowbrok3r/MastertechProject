use crate::{app_state::MtechServerContext, utilities::{display_tasks::setup_display, filter::TaskRefs}};
use database::schema::{Status, TaskPayload};
use egui::Ui;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = &mut self.store_tasks{
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
            }

            let filtered_tasks = TaskRefs::from(tasks)
                .filter_by_completed(false)
                .filter_by_status(&Status::InRepair)
                .get_tasks();

            // setup_display(ui, col_names, );
        }
    }
}

