use database::{live_data::handle_live_data, schema::{get_data::get_associated_ticket, TaskNotePayload}};
use crate::{app_state::SharedContext, PlatformSpawner, Spawner};

impl SharedContext {
    pub fn receive_task(&mut self) {
        if let Ok(new_task) = self.live_tasks_rx.try_recv() {
            log::info!("New Task Update: {:?}", new_task.0);
            let tx = self.new_ticket_tx.clone();
            let notes_tx = self.associated_notes_tx.clone();
            if let Some(service_num) = new_task.clone().1.service_number {
                if !service_num.is_empty() {
                    let new_task = new_task.clone();
                    PlatformSpawner::spawn(async move {
                        match get_associated_ticket(tx, new_task.clone()).await {
                            Ok(_) => log::info!("Got associated ticket"),
                            Err(e) => log::error!("Error getting associated ticket: {e:?}"),
                        }
                        match TaskNotePayload::get_db_notes_from_task_id(new_task.1.id.clone()).await {
                            Ok(notes) => { let _ = notes_tx.try_send(notes); },
                            Err(e) => log::error!("Error getting associated notes: {e:?}"),
                        }
                    });
                }
            }

            match handle_live_data(
                new_task.to_owned(), 
                &mut self.tasks, 
                None
            ) {
                Ok(_) => {
                    log::info!("Task data was handled successfully");
                }
                Err(e) => {
                    log::error!("Error handling task data: {e:?}");
                }
            }
        }
    }
}
