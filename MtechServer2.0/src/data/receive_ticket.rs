use database::live_data::{update_or_insert, update_or_insert_layout};
use log::{error, info};

use crate::MtechServer;

impl MtechServer {
    pub fn receive_ticket(&mut self) {
        if let Ok(channel) = self.context.new_ticket_rx.try_recv() {
            info!("New Ticket Update");

            let new_task_id = channel.new_task.1.id.clone().key().to_string();

            for layout in self.context.task_layouts.values_mut() {
                for tasks in layout.task_map.values_mut() {
                    for task in tasks.iter_mut() {
                        if task.id.key().to_string() == new_task_id {
                            info!(
                                "\nReplacing {:?}\n with \n{:?}\n",
                                task.task_name.clone(),
                                channel.new_task.1.task_name.clone()
                            );

                            if let Err(e) = update_or_insert_layout(
                                &mut self.context.tasks,
                                channel.new_task.1.clone(),
                                Some(channel.new_ticket.clone()),
                                task,
                            ) {
                                error!("Error updating existing task: {e:?}");
                            } else {
                                self.context.rerun_filtering_my_tasks = true;
                                self.context.rerun_filtering_store_tasks = true;
                                self.context.rerun_filtering_completed = true;
                                info!("Updated existing task");
                            }
                            break;
                        }
                    }
                }
            }

            // If no matching task was found in the layouts, add the task to the global context
            if !self
                .context
                .tasks
                .iter()
                .any(|task| task.id.key().to_string() == new_task_id)
            {
                if let Err(e) = update_or_insert(
                    &mut self.context.tasks,
                    channel.new_task.1.clone(),
                    Some(channel.new_ticket.clone()),
                ) {
                    error!("Error updating existing task: {e:?}");
                } else {
                    self.context.rerun_filtering_my_tasks = true;
                    self.context.rerun_filtering_store_tasks = true;
                    self.context.rerun_filtering_completed = true;
                    info!("Inserted new task");
                }
            }
        }
    }
}

