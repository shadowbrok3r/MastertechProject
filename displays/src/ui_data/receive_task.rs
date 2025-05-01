use database::{live_data::handle_live_data, schema::get_data::get_associated_ticket};
use log::{error, info};

use crate::{app_state::SharedContext, PlatformSpawner, Spawner};
impl SharedContext {
    pub fn receive_task(&mut self) {
        if let Ok(new_task) = self.live_tasks_rx.try_recv() {
            info!("New Task Update: {:?}", new_task.0);
            let tx = self.new_ticket_tx.clone();
            if let Some(service_num) = new_task.clone().1.service_number {
                if !service_num.is_empty() {
                    let new_task = new_task.clone();
                    PlatformSpawner::spawn(async move {
                        match get_associated_ticket(tx, new_task.clone()).await {
                            Ok(_) => {} // info!("Got associated ticket"),
                            Err(e) => error!("Error getting associated ticket: {e:?}"),
                        }
                    });
                }
            } else {

                if let Err(e) = handle_live_data(new_task.to_owned(), &mut self.tasks, None)
                {
                    error!("Error handling live data: {e:?}");
                }
                self.rerun_filtering_completed = true;
                self.rerun_filtering_my_tasks = true;
                self.rerun_filtering_store_tasks = true;
            }
        }
    }
}
