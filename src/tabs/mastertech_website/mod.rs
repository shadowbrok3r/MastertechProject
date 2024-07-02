use eframe::egui::Ui;
use log::debug;
use task_layout::TaskLayout;
use tokio::spawn;

use crate::{app_state::MastertechContext, database::schema::{Status, TaskPayload}, utilities::{ColumnLayout, FilterTasks}};

pub mod sortable;
pub mod update_tasks;
pub mod interact_tasks;
pub mod task_cards;
pub mod task_layout;
pub mod filter;

impl MastertechContext {
    pub fn mastertech_website(&mut self, ui: &mut Ui){ 
        // ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        // ui.shrink_width_to_current();
        // ui.shrink_height_to_current();
        // ui.vertical(|ui|{ui.add_space(8.0);});
        // ui.horizontal(|ui|{ui.add_space(8.0);});

        // let sender = self.db_data_sender.clone();

        // if self.query_tasks_first_run{
        //     self.query_tasks_first_run = false;
        //     if let Some(db) = &self.database{
        //         let database = db.clone();
        //         spawn(async move {
        //             let task_data = database.query("SELECT * FROM task").await.unwrap();
                
        //             match sender.try_send(task_data){
        //                 Ok(_) => {
        //                     debug!("Sent task data");
        //                 },
        //                 Err(err) => debug!("Send error: {:?}", err.to_string()),
        //             }
        //         });
        //     }
        // }

        if let Ok(data) = self.db_data_receiver.try_recv(){
            self.ticket_data = Some(data);
        }

        if let Some(tasks) = self.ticket_data.clone(){
            if let Some(users) = self.store_users.as_ref(){
                let page = "my_tasks";
                let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
                let database = self.database.as_ref().unwrap().clone();   
                let current_user = self.current_user.as_ref().unwrap();
                self.task_map.clear();
                let tasks_by_column = &mut self.task_map;

                if !self.task_layouts.contains_key(page) {
                    let task_layout_opts = TaskLayout::new(
                        tasks_by_column.clone(),
                        col_names,
                        database,
                        self.ui_actions_tx.clone(),
                        Some(users.clone()),
                    );
                    self.task_layouts.insert(page.to_string(), task_layout_opts);
                } else if let Some(task_layout) = self.task_layouts.get_mut(page) {
                    for mut status in Status::VALUES{
                        let filtered: Vec<TaskPayload> = tasks
                            .filter_by_status(&status)
                            .filter_by_assignee(current_user);
                        tasks_by_column.insert(status.as_str().to_string(), filtered);
                    }
                    task_layout.update_tasks(tasks_by_column.clone(), col_names.clone());
                    task_layout.layout_cols(ui);
                }
            }
        }
        // if let Some(tasks) = &self.ticket_data{
        //     let task_layout = TaskLayout::new();
        //     let _ = task_layout.task_card(tasks, ui);
        // }
    }
}