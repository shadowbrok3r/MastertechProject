use database::{live_data::update_or_insert_layout, schema::{Status, Store, TaskPayload}};
use log::{error, info};

use crate::app_state::SharedContext;

impl SharedContext {
    pub fn receive_ticket(&mut self) {
        if let Ok(channel) = self.new_ticket_rx.try_recv() {
            log::info!("New Ticket Update");

            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            let new_task: TaskPayload = channel.new_task.1.clone().into(); // Convert LiveTaskPayload to TaskPayload
            let new_task_id = new_task.id.clone().key().to_string();
            let new_ticket = Some(channel.new_ticket.clone());
            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());

            let task_updated = &mut false;

            // Skip if layout_configs not initialized
            if let Some(layout_configs) = &self.layout_configs {
                for (layout_key, layout) in self.task_layouts.iter_mut() {
                    let Some(config) = layout_configs.get(layout_key) else {
                        log::warn!("No config defined for layout: {}", layout_key);
                        continue;
                    };

                    // Remove task from all task_map entries if it no longer belongs or is in the wrong key
                    let mut old_key = None;
                    for (key, task_list) in layout.task_map.iter_mut() {
                        if let Some(pos) = task_list
                            .iter()
                            .position(|task| task.id.key().to_string() == new_task_id)
                        {
                            let should_include = (config.filter)(
                                &new_task.clone().into(),
                                &self.current_user,
                                &self.store_users,
                                &store_selection,
                            );
                            // Remove task if it doesn't belong or is in the wrong status column
                            let is_wrong_key = layout_key == "MyTasks" && key != new_task.status.as_str();
                            if !should_include || is_wrong_key {
                                task_list.remove(pos);
                                info!(
                                    "Removed task {} from layout {} (key: {}) as it {} belongs",
                                    new_task_id,
                                    layout_key,
                                    key,
                                    if should_include && is_wrong_key { "is in wrong status column" } else { "no longer" }
                                );
                            } else {
                                old_key = Some(key.clone());
                            }
                        }
                    }

                    // Determine the new task_map key
                    let new_key = if layout_key == "MyTasks" {
                        match &new_task.status {
                            Status::CustomStatus(name) => name.clone(),
                            _ => new_task.status.as_str().to_string(),
                        }
                    } else {
                        self.store_users
                            .iter()
                            .find(|u| u.get_id() == new_task.assignee)
                            .map(|u| u.get_initials().to_string())
                            .unwrap_or_default()
                    };

                    // Check if the task belongs in this layout
                    let should_include = (config.filter)(
                        &new_task.clone().into(),
                        &self.current_user,
                        &self.store_users,
                        &store_selection,
                    );

                    // Update or insert task in the correct task_map entry
                    if should_include && (config.valid_keys.is_empty() || config.valid_keys.contains(&new_key)) {
                        let task_list = layout
                            .task_map
                            .entry(new_key.clone())
                            .or_insert_with(Vec::new);
                        let layout_updated = &mut false;

                        // Check if task exists in the new key's task_list
                        if let Some(task) = task_list
                            .iter_mut()
                            .find(|t| t.id.key().to_string() == new_task_id)
                        {
                            // Update existing task
                            if let Err(e) = update_or_insert_layout(
                                &mut self.tasks,
                                new_task.clone().into(),
                                new_ticket.clone(),
                                task,
                            ) {
                                error!("Error updating task in layout {}: {e:?}", layout_key);
                            } else {
                                // Update tasks and task_index
                                if let Some(index_task) = self.task_index.get_mut(&new_task_id) {
                                    *index_task = new_task.clone();
                                    // Update self.tasks to maintain consistency
                                    if let Some(pos) = self.tasks.iter().position(|t| t.id.key().to_string() == new_task_id) {
                                        self.tasks[pos] = new_task.clone();
                                    }
                                }
                                log::info!("Updated task in layout: {} (key: {})", layout_key, new_key);
                                *task_updated = true;
                                *layout_updated = true;
                            }
                        } else if old_key.as_ref() != Some(&new_key) {
                            // Insert task if it wasn't found in the new key
                            let mut new_task_payload: TaskPayload = new_task.clone();
                            new_task_payload.service_ticket = new_ticket.clone();
                            task_list.push(new_task_payload.clone());
                            if !*task_updated {
                                if let Err(e) = update_or_insert_layout(
                                    &mut self.tasks,
                                    new_task.clone().into(),
                                    new_ticket.clone(),
                                    &mut new_task_payload,
                                ) {
                                    log::error!("Error inserting task into tasks: {e:?}");
                                } else {
                                    // Update tasks and task_index
                                    self.task_index.insert(new_task_id.clone(), new_task.clone());
                                    self.tasks.push(new_task.clone());
                                    *task_updated = true;
                                }
                            }
                            log::info!("Inserted task into layout: {} (key: {})", layout_key, new_key);
                            *layout_updated = true;
                        }
                    }
                }
            }

            // Insert into global tasks if not updated
            if !*task_updated {
                if let Err(e) = update_or_insert_layout(
                    &mut self.tasks,
                    new_task.clone().into(),
                    new_ticket,
                    &mut TaskPayload::default(),
                ) {
                    log::error!("Error inserting new task into tasks: {e:?}");
                } else {
                    // Update tasks and task_index
                    self.task_index.insert(new_task_id.clone(), new_task.clone());
                    self.tasks.push(new_task.clone());
                    log::info!("Inserted new task into global tasks");
                }
            }

            log::info!("Processed task update for ID: {}", new_task_id);
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

