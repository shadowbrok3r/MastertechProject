use database::live_data::handle_live_notes;
use crate::{app_state::SharedContext, modals::ModalType, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use log::info;
use surrealdb::Action;

impl SharedContext {
    pub fn receive_notes(&mut self) {
        if let Ok(note_payload) = self.notes_rx.try_recv() {
            info!("receive_notes -> self.notes_rx.try_recv() -> New note: {:?}", note_payload);
            self.new_note = true;
            let mut note = note_payload.1.clone();
            let action = note_payload.0.clone();

            if action == Action::Create {
                if let Some(task_id) = &note.task_id {
                    // Walk every `Task` mutably, stop at the first hit, and push the note.
                    if let Some(task) = self
                        .task_layouts
                        .values_mut()                    // throw away the outer map keys
                        .flat_map(|layout| layout.task_map.values_mut())
                        .flat_map(|tasks| tasks.iter_mut())
                        .find(|task| task.id == *task_id) // short-circuit on the match
                    {
                        log::warn!("receive_notes -> self.notes_rx.try_recv() -> Found associated task to insert note into: {:?}", task.id.clone());
                        task.task_note.push(note.clone());       // `note` is moved here
                    }
                }
            }

            for (_title, modal) in self.opened_modals.iter_mut() {
                // info!("receive_notes -> {}-{:?}", title, modal);
                if let Some(ref note_task_id) = note.task_id {
                    if let ModalType::TaskModal(task_modal) = modal {
                        handle_live_notes(note_payload.clone(), &mut task_modal.task).unwrap_or(());
                        if task_modal.task.id == *note_task_id {
                            if let Action::Delete = action {
                                task_modal.chat_view.delete_note(&note);
                            } else {
                                task_modal.chat_view.insert_note(&mut note);
                            }
                        }
                    } else if let ModalType::ChatView(chat_view) = modal {

                        let task = self
                            .tasks
                            .iter_mut()
                            .find(|task| 
                                Some(task.id.clone()) == chat_view.messages.first().cloned().unwrap_or_default().task_id.clone()
                            );

                        if let Some(task) = task {
                            info!("receive_notes -> We have a task, inserting note into modal");
                            handle_live_notes(note_payload.clone(), task).unwrap_or(());
                            if task.id == *note_task_id {
                                if let Action::Delete = action {
                                    chat_view.delete_note(&note);
                                } else {
                                    chat_view.insert_note(&mut note);
                                }
                            }
                        } else {
                            info!("receive_notes -> No task, inserting note into modal");
                            if let Action::Create = action {
                                chat_view.insert_note(&mut note);
                            }
                        }
                    }
                }
                if let Action::Create = action {
                    if let (Some(id), Some(user)) =
                        (&note.clone().task_id, &self.current_user)
                    {
                        if let Some(task) = self.tasks.iter().find(|task| {
                            task.id == id.clone() && task.assignee == user.id && !task.completed
                        }) {
                            // This should work with ID and not initials
                            if note.user != Some(user.id.clone()) {
                                let toast = &mut self.toasts;
                                let new_msg_toast = Toast {
                                    kind: ToastKind::Success,
                                    text: format!("New Message for {}", task.task_name).into(),
                                    options: ToastOptions::default()
                                        .show_progress(true)
                                        .duration_in_seconds(6.0),
                                };
                                toast.add(new_msg_toast);
                            }
                        }
                    }
                }
            }
        
            self.rerun_filtering_completed = true;
            self.rerun_filtering_my_tasks = true;
            self.rerun_filtering_store_tasks = true;
        }

        if let Ok(notes) = self.associated_notes_rx.try_recv() {
            for note in notes.iter() {
                if let Some(task_id) = &note.task_id {
                    // Walk every `Task` mutably, stop at the first hit, and push the note.
                    if let Some(task) = self
                        .task_layouts
                        .values_mut()                    // throw away the outer map keys
                        .flat_map(|layout| layout.task_map.values_mut())
                        .flat_map(|tasks| tasks.iter_mut())
                        .find(|task| task.id == *task_id) // short-circuit on the match
                    {
                        log::warn!("receive_notes -> self.associated_notes_rx.try_recv() -> Found associated task to insert note into: {:?}", task.id.clone());
                        task.task_note.push(note.clone());       // `note` is moved here
                    }
                }
            }
            self.rerun_filtering_completed = true;
            self.rerun_filtering_my_tasks = true;
            self.rerun_filtering_store_tasks = true;
        }
    }
}
