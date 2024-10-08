use database::live_data::handle_live_data;
use log::{error, info};
use wasm_bindgen_futures::spawn_local;

use crate::{utilities::get_data::get_associated_ticket, MtechServer};

impl MtechServer {
    pub fn receive_task(&mut self) {
        if let Ok(new_task) = self.context.live_tasks_rx.try_recv() {
            info!("New Task Update: {:?}", new_task.0);
            let tx = self.context.new_ticket_tx.clone();
            if let Some(service_num) = new_task.clone().1.service_number {
                if !service_num.is_empty() {
                    let new_task = new_task.clone();
                    spawn_local(async move {
                        match get_associated_ticket(tx, new_task.clone()).await {
                            Ok(_) => {} // info!("Got associated ticket"),
                            Err(e) => error!("Error getting associated ticket: {e:?}"),
                        }
                    });
                }
            } else {
                info!("Inserting Task: {:?}", new_task.0);
                self.context.rerun_filtering_completed = true;
                self.context.rerun_filtering_my_tasks = true;
                self.context.rerun_filtering_store_tasks = true;
                if let Err(e) = handle_live_data(new_task.to_owned(), &mut self.context.tasks, None)
                {
                    error!("Error handling live data: {e:?}");
                }
            }
        }
    }
}
