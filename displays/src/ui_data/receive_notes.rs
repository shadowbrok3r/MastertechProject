use crate::{app_state::SharedContext, modals::ModalType, ui_tools::toasts::{Toast, ToastKind, ToastOptions, ToastStyle}};
use database::schema::{LiveTaskPayload, Store};
use database::live_data::Action;
use log::info;

impl SharedContext {
    pub fn receive_notes(&mut self) {
        // Handle single note updates from notes_rx
        if let Ok(note_payload) = self.notes_rx.try_recv() {
            info!("receive_notes -> notes_rx.try_recv() -> New note: {:?}", note_payload);
            self.new_note = true;
            let mut note = note_payload.1.clone();
            let action = note_payload.0.clone();

            // Initialize layout_configs if store_users is available
            self.init_layout_configs();

            // Update modals
            for (_title, modal) in self.opened_modals.iter_mut() {
                if let Some(note_task_id) = &note.task_id {
                    if let ModalType::TaskModal(task_modal) = modal {
                        // if let Err(e) = handle_live_notes(note_payload.clone(), &mut task_modal.task) {
                        //     log::error!("receive_notes -> Error in handle_live_notes for TaskModal: {:?}", e);
                        // }
                        if task_modal.task.id == *note_task_id {
                            if action == Action::Delete {
                                task_modal.chat_view.delete_note(&note);
                            } else {
                                task_modal.chat_view.insert_note(&mut note);
                            }
                        }
                    } else if let ModalType::ChatView(chat_view) = modal {
                        let task = self
                            .tasks
                            .iter_mut()
                            .find(|task| Some(task.id.clone()) == Some(chat_view.task_id.clone()) );

                        if let Some(task) = task {
                            info!("receive_notes -> Inserting note into ChatView modal for task {}", task.id);
                            // if let Err(e) = handle_live_notes(note_payload.clone(), task) {
                            //     log::error!("receive_notes -> Error in handle_live_notes for ChatView: {:?}", e);
                            // }
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

            // Show toast for new notes - notify all users who have previously commented on this task
            if action == Action::Create || action == Action::Update {
                if let (Some(task_id), Some(current_user)) = (note.task_id.clone(), &self.current_user) {
                    // Find the task to get task info
                    if let Some(task) = self.tasks.iter().find(|t| t.id == task_id && !t.completed) {
                        let current_user_id = current_user.get_id();
                        let note_author_id = note.user.clone();
                        
                        // Check if the note is from someone else
                        if note_author_id != current_user_id {
                            // Check if current user is the assignee
                            let is_assignee = task.assignee == current_user_id;
                            
                            let has_commented = self.task_layouts.iter().any(|(_, layout)| layout.get_notes(&task_id).iter().any(|n| n.user == current_user_id));
                            
                            // Show toast if user is assignee OR has previously commented
                            if is_assignee || has_commented {
                                let action_text = if action == Action::Create { "New" } else { "Updated" };
                                let toast = Toast {
                                    kind: ToastKind::Success,
                                    text: format!("{} message for {}", action_text, task.task_name).into(),
                                    options: ToastOptions::default()
                                        .show_progress(true)
                                        .duration_in_seconds(6.0),
                                    style: ToastStyle::default(),
                                };
                                self.toasts.add(toast);
                            }
                        }
                    }
                }
            }
        }

        // Handle batch of associated notes from associated_notes_rx
        if let Ok(notes) = self.associated_notes_rx.try_recv() {

            info!("receive_notes -> associated_notes_rx.try_recv() -> Received {} notes", notes.len());

            // Initialize layout_configs if needed
            self.init_layout_configs();

            let store_selection = std::convert::Into::<Store>::into(self.store_selection.clone());
            let layout_configs = self.layout_configs.as_ref();

            for note in notes.iter() {
                if let Some(task_id) = &note.task_id {
                    // Update tasks in task_layouts
                    if let Some(layout_configs) = layout_configs {
                        for (layout_key, layout) in self.task_layouts.iter_mut() {
                            let Some(config) = layout_configs.get(layout_key) else {
                                log::warn!("No config defined for layout: {}", layout_key);
                                continue;
                            };

                            let new_notes = &mut vec![];
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
                                        log::debug!(
                                            "receive_notes -> Adding associated note to task {} in layout {} should_include: {should_include}",
                                            task.id, layout_key
                                        );
                                    } else {
                                        // Remove task from this layout if it no longer belongs
                                        task_list.retain(|t| t.id != *task_id);
                                        log::debug!(
                                            "receive_notes -> Removed task {} from layout {} as it no longer belongs",
                                            task_id, layout_key
                                        );
                                    }
                                }
                            }
                            layout.insert_notes(new_notes.clone());
                        }
                    }
                }
            }
        }
    }
}
