use std::borrow::{Borrow, BorrowMut};

use crate::{app_state::MtechServerContext, utilities::{displays::Filters, FilterTasks}};
use database::schema::{Priority, TaskPayload, User};
use egui::Ui;

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(mut tasks) = &self.my_tasks{
            self.store_tasks_opened = true;
            let mut col_names = Vec::new();

            for user in self.store_users.as_ref().unwrap(){
                col_names.push(user.everest_initials.clone());
                
            }
            let database = self.database.as_ref().unwrap().clone();

            self.initialize_task_layout("store_tasks", tasks.to_owned(), col_names, database); 

            if let Some(task_layout) = self.task_layouts.borrow_mut().get_mut("store_tasks") {
                let store_users = self.store_users.borrow();
                let mut filtered_tasks: Vec<TaskPayload> = tasks.borrow_mut::<Vec<TaskPayload>>().clone();
                let filter_items = || -> Vec<TaskPayload> {
                    
                    if let Some(users) = &store_users {
                        for user in users {
                            filtered_tasks = filtered_tasks
                                .filter_by_assignee(&user)
                                .filter_by_completed(true);
                        }
                    }
                    filtered_tasks.to_owned()
                };

                task_layout.display(
                    ui,
                    &self.store_users,
                    false,
                    &None,
                    &Some(false),
                    &None,
                    filter_items
                );
            }
        }
    }
}

