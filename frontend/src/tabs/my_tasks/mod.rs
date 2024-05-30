use crate::{app_state::MtechServerContext, utilities::displays::Filters};
use egui::Ui;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &self.my_tasks{
            self.my_tasks_opened = true;

            let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
            let database = self.database.as_ref().unwrap().clone();
            

            let filters = vec![
                Filters::FilterStatus
            ];

            self.initialize_task_layout("my_tasks", tasks.to_owned(), col_names, database, filters);

            if let Some(task_layout) = self.task_layouts.get_mut("my_tasks"){
                task_layout.display(
                    ui, 
                    &self.store_users, 
                    true, 
                    &None, 
                    &None, 
                    &self.current_user
                );

                let open = &mut task_layout.show_create_task_modal;
                if *open{
                    self.modal_handler.open();
                    *open = false;
                }
            }

        }
    }
}