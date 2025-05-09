use database::{live_data::update_or_insert_layout, schema::{Store, TaskPayload}};
use log::{error, info};

use crate::app_state::SharedContext;

impl SharedContext {
    pub fn receive_ticket(&mut self) {
        if let Ok(channel) = self.new_ticket_rx.try_recv() {
            info!("New Ticket Update");

            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            let new_task = channel.new_task.1.clone();
            let new_task_id = new_task.id.clone().key().to_string();
            let new_ticket = Some(channel.new_ticket.clone());
            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());

            let mut task_updated = false;

            // Skip if layout_configs not initialized
            if let Some(layout_configs) = &self.layout_configs {
                for (layout_key, layout) in self.task_layouts.iter_mut() {
                    let Some(config) = layout_configs.get(layout_key) else {
                        log::warn!("No config defined for layout: {}", layout_key);
                        continue;
                    };

                    // Check if the task belongs in this layout
                    let should_include = (config.filter)(
                        &new_task,
                        &self.current_user,
                        &self.store_users,
                        &store_selection,
                    );

                    // Determine the task_map key
                    let key = if layout_key == "MyTasks" {
                        new_task.status.as_str().to_string()
                    } else {
                        self.store_users
                            .iter()
                            .find(|u| u.id == new_task.assignee)
                            .map(|u| u.everest_initials.to_string())
                            .unwrap_or_default()
                    };

                    // Check if the task exists in this layout's task_map
                    let mut layout_updated = false;
                    if let Some(task_list) = layout.task_map.get_mut(&key) {
                        for task in task_list.iter_mut() {
                            if task.id.key().to_string() == new_task_id {
                                if let Err(e) = update_or_insert_layout(
                                    &mut self.tasks,
                                    new_task.clone(),
                                    new_ticket.clone(),
                                    task,
                                ) {
                                    error!("Error updating task in layout {}: {e:?}", layout_key);
                                } else {
                                    info!("Updated task in layout: {}", layout_key);
                                    task_updated = true;
                                    layout_updated = true;
                                }
                                break;
                            }
                        }
                    }

                    // Add new task if it belongs and wasn't updated
                    if should_include && !layout_updated {
                        if config.valid_keys.is_empty() || config.valid_keys.contains(&key) {
                            let task_list = layout.task_map.entry(key.clone()).or_insert_with(Vec::new);
                            if !task_list.iter().any(|t| t.id.key().to_string() == new_task_id) {
                                let mut new_task_payload: TaskPayload = new_task.clone().into();
                                new_task_payload.service_ticket = new_ticket.clone();
                                task_list.push(new_task_payload.clone());
                                if !task_updated {
                                    if let Err(e) = update_or_insert_layout(
                                        &mut self.tasks,
                                        new_task.clone(),
                                        new_ticket.clone(),
                                        &mut new_task_payload,
                                    ) {
                                        error!("Error inserting task into tasks: {e:?}");
                                    } else {
                                        task_updated = true;
                                    }
                                }
                                info!("Inserted new task into layout: {}", layout_key);
                            }
                        }
                    } else if !should_include {
                        // Remove task from this layout if it no longer belongs
                        for task_list in layout.task_map.values_mut() {
                            task_list.retain(|task| task.id.key().to_string() != new_task_id);
                        }
                    }
                }
            }

            // Insert into global tasks if not updated
            if !task_updated {
                if let Err(e) = update_or_insert_layout(
                    &mut self.tasks,
                    new_task.clone(),
                    new_ticket,
                    &mut TaskPayload::default(),
                ) {
                    error!("Error inserting new task into tasks: {e:?}");
                } else {
                    info!("Inserted new task into global tasks");
                }
            }

            info!("Processed task update for ID: {}", new_task_id);
        }
    }
}

// impl SharedContext {
//     pub fn receive_ticket(&mut self) {
//         if let Ok(channel) = self.new_ticket_rx.try_recv() {
//             info!("New Ticket Update");

//             let new_task_id = channel.new_task.1.id.clone().key().to_string();

//             for layout in self.task_layouts.values_mut() {
//                 for tasks in layout.task_map.values_mut() {
//                     for task in tasks.iter_mut() {
//                         if task.id.key().to_string() == new_task_id {
//                             info!(
//                                 "\nReplacing {:?}\n with \n{:?}\n",
//                                 task.task_name.clone(),
//                                 channel.new_task.1.task_name.clone()
//                             );

//                             if let Err(e) = update_or_insert_layout(
//                                 &mut self.tasks,
//                                 channel.new_task.1.clone(),
//                                 Some(channel.new_ticket.clone()),
//                                 task,
//                             ) {
//                                 error!("Error updating existing task: {e:?}");
//                             }
//                             self.rerun_filtering_my_tasks = true;
//                             self.rerun_filtering_store_tasks = true;
//                             self.rerun_filtering_completed = true;
//                             info!("Updated existing task");
//                             break;
//                         }
//                     }
//                 }
//             }

//             // If no matching task was found in the layouts, add the task to the global context
//             if !self
//                 .tasks
//                 .iter()
//                 .any(|task| task.id.key().to_string() == new_task_id)
//             {
//                 if let Err(e) = update_or_insert(
//                     &mut self.tasks,
//                     channel.new_task.1.clone(),
//                     Some(channel.new_ticket.clone()),
//                 ) {
//                     error!("Error updating existing task: {e:?}");
//                 } 
//                 self.rerun_filtering_my_tasks = true;
//                 self.rerun_filtering_store_tasks = true;
//                 self.rerun_filtering_completed = true;
//                 info!("Inserted new task");
                
//             }
//         }
//     }
// }

