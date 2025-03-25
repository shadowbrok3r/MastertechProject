use database::live_data::handle_live_notes;
use crate::{app_state::SharedContext, modals::ModalType, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use log::info;
use surrealdb::Action;

impl SharedContext {
    pub fn receive_notes(&mut self) {
        if let Ok(mut note_payload) = self.notes_rx.try_recv() {
            info!("receive_notes -> New note: {:?}", note_payload);
            self.new_note = true;
            
            for (_title, modal) in self.opened_modals.iter_mut() {
                // info!("receive_notes -> {}-{:?}", title, modal);
                if let Some(ref note_task_id) = note_payload.1.task_id {
                    if let ModalType::TaskModal(task_modal) = modal {
                        handle_live_notes(note_payload.clone(), &mut task_modal.task).unwrap_or(());
                        if task_modal.task.id == *note_task_id {
                            if let Action::Delete = note_payload.0 {
                                task_modal.chat_view.delete_note(&note_payload.1);
                            } else {
                                task_modal.chat_view.insert_note(&mut note_payload.1);
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
                                if let Action::Delete = note_payload.0 {
                                    chat_view.delete_note(&note_payload.1);
                                } else {
                                    chat_view.insert_note(&mut note_payload.1);
                                }
                            }
                        } else {
                            info!("receive_notes -> No task, inserting note into modal");
                            if let Action::Create = note_payload.0 {
                                chat_view.insert_note(&mut note_payload.1);
                            }
                        }
                    }
                }
                if let Action::Create = note_payload.0 {
                    if let (Some(id), Some(user)) =
                        (&note_payload.1.clone().task_id, &self.current_user)
                    {
                        if let Some(task) = self.tasks.iter().find(|task| {
                            task.id == id.clone() && task.assignee == user.id && !task.completed
                        }) {
                            // This should work with ID and not initials
                            if note_payload.1.everest_initials != user.everest_initials {
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
        }
    }
}
