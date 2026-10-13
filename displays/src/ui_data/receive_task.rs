use database::{live_data::handle_live_data, schema::{get_data::get_associated_ticket, LiveTaskPayload, RecordIdExt, Status, Store, TaskNotePayload}};
use crate::{app_state::SharedContext, get_current_user_from_auth, PlatformSpawner, Spawner};

/// Local state a received live task update changes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveUpdateEffects {
    pub description_edited: bool,
}

/// Effects of applying `new` over the cached `old` copy of a task.
pub(crate) fn live_update_effects(old: Option<&LiveTaskPayload>, new: &LiveTaskPayload) -> LiveUpdateEffects {
    LiveUpdateEffects {
        description_edited: old.is_some_and(|old| old.task_description != new.task_description),
    }
}

impl SharedContext {
    pub fn receive_task(&mut self) {
        // Promote the completed-task latch only once its fetch has come back
        // clean; a failure clears the in-flight marker so the next tab click
        // tries again.
        while let Ok((store, ok)) = self.completed_load_rx.try_recv() {
            if self.completed_pending_for == Some(store) {
                self.completed_pending_for = None;
            }
            let current = Store::from_presta_store_id(&self.store_selection.to_string());
            if ok && store == current {
                // A result for a store the operator has already left says
                // nothing about the one they are looking at.
                self.completed_loaded_for = Some(store);
            } else if !ok && self.completed_loaded_for == Some(store) {
                self.completed_loaded_for = None;
            }
            // The tab-click path raises this spinner; the result is the only
            // thing that can lower it, success or failure.
            self.task_layouts
                .iter_mut()
                .filter(|(page, _)| *page == "Completed Tasks")
                .for_each(|(_, layout)| layout.loading = false);
        }

        while let Ok(mut tasks) = self.initial_tasks_rx.try_recv() {
            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            // Clear layout-related data only if switching stores
            if self.pending_store.is_some() {
                self.task_layouts
                    .iter_mut()
                    .filter(|(page, _)| *page == "Completed Tasks" || *page == "Store Tasks")
                    .for_each(|(_, layout)| {
                        layout.task_map.clear();
                        layout.assignees.clear();
                        layout.search_inputs.clear();
                        if let Some(time) = self.timer {
                            if time.elapsed() > web_time::Duration::from_secs(5) {
                                layout.loading = false;
                            }
                        }
                    });
            }

            // Process new tasks
            let store_selection = Store::from_presta_store_id(&self.store_selection.to_string());
            let layout_configs = self.layout_configs.as_ref();

            // Positions are indexed once per payload. A reconnect re-delivers
            // the whole snapshot — 4.4k rows on RIV — and scanning `self.tasks`
            // per row is ten million string allocations in one frame.
            let mut positions: std::collections::HashMap<String, usize> = self
                .tasks
                .iter()
                .enumerate()
                .map(|(index, task)| (task.id.key_string(), index))
                .collect();

            tasks.drain(..).for_each(|new_task| {
                // A snapshot is newer than whatever the index holds, so it
                // overwrites: skipping known ids left rows that went stale
                // during a disconnect wrong until the process restarted.
                let task_id = new_task.id.key_string();
                if self.live_task_updates.contains(&task_id) {
                    return;
                }
                let is_new = self
                    .task_index
                    .insert(task_id.clone(), new_task.clone())
                    .is_none();
                if is_new {
                    positions.insert(task_id.clone(), self.tasks.len());
                    self.tasks.push(new_task.clone());

                    // Distribute to layouts if layout_configs is initialized
                    if let Some(layout_configs) = layout_configs {
                        for (layout_key, layout) in self.task_layouts.iter_mut() {
                            let Some(config) = layout_configs.get(layout_key) else {
                                log::warn!("No config defined for layout: {}", layout_key);
                                continue;
                            };

                            // Check if the task belongs in this layout
                            let should_include = (config.filter)(
                                &new_task.clone().into(),
                                &self.current_user,
                                &self.store_users,
                                &store_selection,
                            );

                            // Determine the task_map key
                            let key = if layout_key == "My Tasks" {
                                match &new_task.status {
                                    Status::CustomStatus(name) => name.clone(),
                                    _ => new_task.status.as_str().to_string(),
                                }
                            } else {
                                self.store_users
                                    .iter()
                                    .find(|u| u.get_id() == new_task.assignee)
                                    .map(|u| u.get_username().to_string())
                                    .unwrap_or_default()
                            };

                            // Add task to task_map if it belongs
                            if should_include && (config.valid_keys.is_empty() || config.valid_keys.contains(&key)) {
                                let task_list = layout
                                    .task_map
                                    .entry(key.clone())
                                    .or_insert_with(Vec::new);
                                if !task_list.iter().any(|t| t.id.key_string() == task_id) {
                                    task_list.push(new_task.clone());
                                    // log::info!("Added initial task to layout: {}", layout_key);
                                }
                            }
                        
                            layout.has_run = false;
                        }
                    }
                } else {
                    match positions.get(&task_id).copied() {
                        Some(pos) => self.tasks[pos] = new_task,
                        // Indexed but missing from the list: push rather than
                        // drop, or the divergence never heals.
                        None => {
                            positions.insert(task_id.clone(), self.tasks.len());
                            self.tasks.push(new_task);
                        }
                    }
                }
            });

            // Reset pending_store if tasks were processed for the new store
            if let Some(pending_store) = self.pending_store {
                if pending_store.as_str() == store_selection.as_str() {
                    self.pending_store = None;
                }
            }
        }

        if let Ok(new_task) = self.live_tasks_rx.try_recv() {
            log::debug!("New Task Update: {:?}", new_task.0);
            
            let tx = self.new_ticket_tx.clone();
            let notes_tx = self.associated_notes_tx.clone();
            if let Some(service_num) = new_task.clone().1.service_number {
                if !service_num.is_empty() {
                    let new_task = new_task.clone();
                    PlatformSpawner::spawn(async move {
                        if let Err(e) = get_associated_ticket(tx, new_task.clone()).await {
                            log::error!("Error getting associated ticket: {e:?}");
                        }
                        match TaskNotePayload::get_db_notes_from_task_id(new_task.1.id.clone()).await {
                            Ok(notes) => { let _ = notes_tx.try_send(notes); },
                            Err(e) => log::error!("Error getting associated notes: {e:?}"),
                        }
                    });
                }
            }

            // Get existing task for diff comparison before applying changes
            let task_id = new_task.1.id.key_string();
            let existing_task: Option<LiveTaskPayload> = self.task_index.get(&task_id).cloned();

            match handle_live_data(new_task.to_owned(), &mut self.tasks) {
                Ok(_) => {
                    // Update task_index
                    self.live_task_updates.insert(task_id.clone());
                    self.task_index.insert(task_id.clone(), new_task.1.clone().into());
                    // Update self.tasks to maintain consistency
                    if let Some(pos) = self.tasks.iter().position(|t| t.id.key_string() == task_id) {
                        self.tasks[pos] = new_task.1.clone().into();
                    } else {
                        self.tasks.push(new_task.1.clone().into());
                    }
                    
                    // Feeds the card's description-edit indicator for edits after login.
                    let new_task_payload = new_task.1.clone();
                    let effects = live_update_effects(existing_task.as_ref(), &new_task_payload);
                    if effects.description_edited {
                        let editor = get_current_user_from_auth()
                            .map(|u| u.get_username().to_string())
                            .unwrap_or_else(|| "System".to_string());
                        self.description_changes.insert(
                            new_task_payload.id.clone(),
                            (chrono::Utc::now(), editor),
                        );
                    }
                }
                Err(e) => {
                    log::error!("Error handling task data: {e:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::RecordId;

    #[test]
    fn a_received_status_and_assignee_change_records_nothing() {
        let old = LiveTaskPayload {
            status: Status::CustomStatus("follow up".into()),
            ..Default::default()
        };
        let new = LiveTaskPayload {
            status: Status::CustomStatus("REPO".into()),
            assignee: RecordId::new("user", "someone_else"),
            ..old.clone()
        };
        assert_eq!(live_update_effects(Some(&old), &new), LiveUpdateEffects::default());
    }

    #[test]
    fn a_received_description_edit_only_flags_the_indicator() {
        let old = LiveTaskPayload::default();
        let new = LiveTaskPayload {
            task_description: "move all data".into(),
            ..old.clone()
        };
        assert_eq!(
            live_update_effects(Some(&old), &new),
            LiveUpdateEffects { description_edited: true }
        );
    }

    #[test]
    fn an_uncached_task_has_no_effects() {
        assert_eq!(
            live_update_effects(None, &LiveTaskPayload::default()),
            LiveUpdateEffects::default()
        );
    }
}
