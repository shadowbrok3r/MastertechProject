use eframe::egui::Ui;
use log::debug;
use tokio::spawn;

use crate::app_state::MastertechContext;

use self::task_cards::TaskLayout;

pub mod task_cards;

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
            if let Some(db) = &self.database{
                let database = db.clone();
                spawn(async move {
                    let task_data = database.query("SELECT * FROM task").await.unwrap();
                
                    match sender.try_send(task_data){
                        Ok(_) => {
                            debug!("Sent task data");
                        },
                        Err(err) => debug!("Send error: {:?}", err.to_string()),
                    }
                });
            }
        }

        if let Ok(data) = self.db_data_receiver.try_recv(){
            self.ticket_data = Some(data);
        }

        if let Some(tasks) = &self.ticket_data{
            let task_layout = TaskLayout::default();
            let _ = task_layout.task_card(tasks.to_vec(), ui);
        }
    }
}