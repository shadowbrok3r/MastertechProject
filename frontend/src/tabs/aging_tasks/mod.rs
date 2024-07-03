use crate::{app_state::MtechServerContext, utilities::{displays::tasks::task_layout::TaskLayout, ColumnLayout, FilterTasks}};
use database::schema::TaskPayload;
use eframe::egui::Ui;

impl MtechServerContext{
    pub fn aging_tasks(&mut self, ui: &mut Ui) {
        // if let Some(tasks) = self.tasks.clone(){
        //     if let Some(users) = self.store_users.as_ref(){
        //         self.my_tasks_opened = true;
        //         let page = "aging_tasks";
        //         let col_names = vec!["> 1 Week".to_string(), "> 1 Month".to_string(), "Everything else".to_string()];
        //         let database = self.database.as_ref().unwrap().clone();   
        //         let current_user = self.current_user.as_ref().unwrap();
        //         self.task_map.clear();
        //         let tasks_by_column = &mut self.task_map;

        //         if !self.task_layouts.contains_key(page) {
        //             let task_layout_opts = TaskLayout::new(
        //                 tasks_by_column.clone(),
        //                 col_names,
        //                 database,
        //                 self.ui_actions_tx.clone(),
        //                 Some(users.clone()),
        //             );
        //             self.task_layouts.insert(page.to_string(), task_layout_opts);
        //         } else if let Some(task_layout) = self.task_layouts.get_mut(page) {
        //             for mut status in Status::VALUES{
        //                 let filtered: Vec<TaskPayload> = tasks
        //                     .filter_by_status(&status)
        //                     .filter_by_assignee(current_user);
        //                 tasks_by_column.insert(status.as_str().to_string(), filtered);
        //             }
        //             task_layout.update_tasks(tasks_by_column.clone(), col_names.clone());
        //             task_layout.layout_cols(ui);
        //         }
        //     }
        // }
    }
}

