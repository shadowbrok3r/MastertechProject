use database::{live_data::handle_live_notes, schema::{LiveTaskPayload, Store}};
use crate::{app_state::SharedContext, modals::ModalType, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use log::info;
use surrealdb::Action;

impl SharedContext {
    pub fn receive_notes(&mut self) {
        // Handle single note updates from notes_rx
        if let Ok(note_payload) = self.notes_rx.try_recv() {
            info!("receive_notes -> notes_rx.try_recv() -> New note: {:?}", note_payload);
            self.new_note = true;
            let mut note = note_payload.1.clone();
            let action = note_payload.0.clone();
            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());

            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            // Update tasks in task_layouts
            if let Some(task_id) = &note.task_id {
                let layout_configs = self.layout_configs.as_ref();
                let mut task_updated = false;

                if let Some(layout_configs) = layout_configs {
                    for (layout_key, layout) in self.task_layouts.iter_mut() {
                        let Some(config) = layout_configs.get(layout_key) else {
                            log::warn!("No config defined for layout: {}", layout_key);
                            continue;
                        };

                        // Find the task in all task_map entries
                        for (_, task_list) in layout.task_map.iter_mut() {
                            if let Some(task) = task_list.iter_mut().find(|t| t.id == *task_id) {
                                // Verify the task still belongs in this layout
                                let live_task: LiveTaskPayload = task.clone().into();
                                let should_include = (config.filter)(
                                    &live_task,
                                    &self.current_user,
                                    &self.store_users,
                                    &store_selection,
                                );

                                if should_include {
                                    if action == Action::Create {
                                        info!(
                                            "receive_notes -> Adding note to task {} in layout {}",
                                            task.id.clone(),
                                            layout_key
                                        );
                                        task.task_note.push(note.clone());
                                        task_updated = true;
                                    } else if action == Action::Delete {
                                        info!(
                                            "receive_notes -> Deleting note from task {} in layout {}",
                                            task.id.clone(),
                                            layout_key
                                        );
                                        task.task_note.retain(|n| n != &note);
                                        task_updated = true;
                                    }
                                } else {
                                    // Remove task from this layout if it no longer belongs
                                    task_list.retain(|t| t.id != *task_id);
                                    info!(
                                        "receive_notes -> Removed task {} from layout {} as it no longer belongs",
                                        task_id, layout_key
                                    );
                                }
                            }
                        }
                    }
                }

                // Update the task in self.tasks if not already updated
                if !task_updated {
                    if let Some(task) = self.tasks.iter_mut().find(|task| task.id == *task_id) {
                        if action == Action::Create {
                            info!("receive_notes -> Adding note to task {} in global tasks", task.id);
                            task.task_note.push(note.clone());
                        } else if action == Action::Delete {
                            info!(
                                "receive_notes -> Deleting note from task {} in global tasks",
                                task.id
                            );
                            task.task_note.retain(|n| n != &note);
                        }
                    }
                }
            }

            // Update modals
            for (_title, modal) in self.opened_modals.iter_mut() {
                if let Some(note_task_id) = &note.task_id {
                    if let ModalType::TaskModal(task_modal) = modal {
                        if let Err(e) = handle_live_notes(note_payload.clone(), &mut task_modal.task) {
                            log::error!("receive_notes -> Error in handle_live_notes for TaskModal: {:?}", e);
                        }
                        if task_modal.task.id == *note_task_id {
                            if action == Action::Delete {
                                task_modal.chat_view.delete_note(&note);
                            } else {
                                task_modal.chat_view.insert_note(&mut note);
                            }
                        }
                    } else if let ModalType::ChatView(chat_view) = modal {
                        let task = self.tasks.iter_mut().find(|task| {
                            Some(task.id.clone())
                                == chat_view
                                    .messages
                                    .first()
                                    .cloned()
                                    .unwrap_or_default()
                                    .task_id
                                    .clone()
                        });

                        if let Some(task) = task {
                            info!("receive_notes -> Inserting note into ChatView modal for task {}", task.id);
                            if let Err(e) = handle_live_notes(note_payload.clone(), task) {
                                log::error!("receive_notes -> Error in handle_live_notes for ChatView: {:?}", e);
                            }
                            if task.id == *note_task_id {
                                if action == Action::Delete {
                                    chat_view.delete_note(&note);
                                } else {
                                    chat_view.insert_note(&mut note);
                                }
                            }
                        } else if action == Action::Create {
                            info!("receive_notes -> Inserting note into ChatView modal (no task)");
                            chat_view.insert_note(&mut note);
                        }
                    }
                }
            }

            // Show toast for new notes on current user's tasks
            if action == Action::Create {
                if let (Some(id), Some(user)) = (note.task_id.clone(), &self.current_user) {
                    if let Some(task) = self.tasks.iter().find(|task| {
                        task.id == id && task.assignee == user.get_id() && !task.completed
                    }) {
                        if note.user != Some(user.get_id().clone()) {
                            let toast = Toast {
                                kind: ToastKind::Success,
                                text: format!("New Message for {}", task.task_name).into(),
                                options: ToastOptions::default()
                                    .show_progress(true)
                                    .duration_in_seconds(6.0),
                            };
                            self.toasts.add(toast);
                        }
                    }
                }
            }
        }

        // Handle batch of associated notes from associated_notes_rx
        if let Ok(notes) = self.associated_notes_rx.try_recv() {
            info!(
                "receive_notes -> associated_notes_rx.try_recv() -> Received {} notes",
                notes.len()
            );

            // Initialize layout_configs if needed
            self.init_layout_configs();

            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());
            let layout_configs = self.layout_configs.as_ref();

            for note in notes {
                if let Some(task_id) = &note.task_id {
                    let mut task_updated = false;

                    // Update tasks in task_layouts
                    if let Some(layout_configs) = layout_configs {
                        for (layout_key, layout) in self.task_layouts.iter_mut() {
                            let Some(config) = layout_configs.get(layout_key) else {
                                log::warn!("No config defined for layout: {}", layout_key);
                                continue;
                            };

                            // Find the task in all task_map entries
                            for (_, task_list) in layout.task_map.iter_mut() {
                                if let Some(task) = task_list.iter_mut().find(|t| t.id == *task_id) {
                                    // Verify the task still belongs in this layout
                                    let live_task: LiveTaskPayload = task.clone().into();
                                    let should_include = (config.filter)(
                                        &live_task,
                                        &self.current_user,
                                        &self.store_users,
                                        &store_selection,
                                    );

                                    if should_include {
                                        info!(
                                            "receive_notes -> Adding associated note to task {} in layout {}",
                                            task.id, layout_key
                                        );
                                        task.task_note.push(note.clone());
                                        task_updated = true;
                                    } else {
                                        // Remove task from this layout if it no longer belongs
                                        task_list.retain(|t| t.id != *task_id);
                                        info!(
                                            "receive_notes -> Removed task {} from layout {} as it no longer belongs",
                                            task_id, layout_key
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Update the task in self.tasks if not already updated
                    if !task_updated {
                        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == *task_id) {
                            info!(
                                "receive_notes -> Adding associated note to task {} in global tasks",
                                task.id
                            );
                            task.task_note.push(note.clone());
                        }
                    }
                }
            }
        }
    }
}
