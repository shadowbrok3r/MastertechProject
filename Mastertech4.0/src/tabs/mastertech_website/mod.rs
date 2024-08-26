use std::collections::BTreeMap;

use crate::{app_state::MastertechContext, utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks}};
use database::{schema::{Status, TaskPayload}, DATABASE};
use eframe::egui::Ui;
use tokio::spawn;
use log::{debug, error};
use tracing::info;


pub mod sortable;
pub mod interact_tasks;
pub mod filter;

impl MastertechContext {
    pub fn mastertech_website(&mut self, ui: &mut Ui){ 
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.vertical(|ui|{ui.add_space(8.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});

        let sender = self.db_data_sender.clone();

        if self.query_tasks_first_run{
            self.query_tasks_first_run = false;
            spawn(async move {
                let task_data: Result<surrealdb::Response, surrealdb::Error> = DATABASE.query("SELECT * FROM task FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note").await;
                match task_data{
                    Ok(mut res) => {
                        let task_data: Result<Vec<TaskPayload>, surrealdb::Error> = res.take(0);
                        match task_data {
                            Ok(tasks) => {
                                match sender.try_send(tasks){
                                    Ok(_) => info!("Sent task data"),
                                    Err(err) => debug!("Send error: {:?}", err.to_string()),
                                }
                            },
                            Err(e) => error!("Error unwrapping task data: {e:?}"),
                        }
                    },
                    Err(e) => error!("Error retrieving task data: {e:?}"),
                }

            });
        }

        if let Ok(data) = self.db_data_receiver.try_recv(){
            self.task_payload = Some(data);
        }

        if let Some(tasks) = self.task_payload.clone(){
            if let Some(users) = self.store_users.as_ref(){
                let page = "MyTasks";
                let current_user = self.current_user.as_ref().unwrap();
    
                let mut vals = Status::VALUES;
                    // Define the custom sort order
                let order = |name: Status| match name.as_str() {
                    "Todo" => 1,
                    "In Repair" => 2,
                    "Complete" => 3,
                    _ => 4, // Default case if there are other unexpected items
                };
                
                vals.sort_unstable_by_key(|x| order(x.clone()));
                
                if let Some(layout) = self.task_layouts.get_mut(page){
                    if self.rerun_filtering_my_tasks{
                        self.rerun_filtering_my_tasks = false;
                        let mut map = BTreeMap::new();
                        vals.iter_mut().for_each(|status| {
                            let filtered = tasks.filter_by_status(&status).filter_by_assignee(current_user);
                            map.entry(status.as_str().to_string()).or_insert(filtered);
                        });
                        layout.task_map = map;
                    }
                    layout.layout_cols(ui);
                } else {
                    let mut map = BTreeMap::new();
                    vals.iter_mut().for_each(|status| {
                        let filtered = tasks.filter_by_status(&status).filter_by_assignee(current_user);
                        map.entry(status.as_str().to_string()).or_insert(filtered);
                    });
                    let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                    let layout = TaskLayout::new(map, user_names, self.ui_actions_tx.clone(), users.clone());
                    self.task_layouts.insert(page.to_string(), layout);
                }
            }
        }
    }
}