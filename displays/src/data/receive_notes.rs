use database::live_data::handle_live_notes;
use crate::{app_state::SharedContext, modals::ModalType, ui_tools::toasts::{Toast, ToastKind, ToastOptions}};
use log::info;
use surrealdb::Action;

impl SharedContext {
    pub fn receive_notes(&mut self) {
        if let Ok(mut payload) = self.notes_rx.try_recv() {
            info!("{:?}", payload);
            self.new_note = true;
            if let ModalType::TaskModal(task_modal) = &mut self.current_modal {
                handle_live_notes(payload.clone(), &mut task_modal.task).unwrap_or(());

                if let Action::Delete = payload.0 {
                    task_modal.chat_view.delete_note(&payload.1);
                } else {
                    task_modal.chat_view.insert_note(&mut payload.1);
                }
            } else if let ModalType::ChatView(chat_view) = &mut self.current_modal {
                let task = self
                    
                    
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == chat_view.task_id.clone().unwrap());
                if let Some(task) = task {
                    handle_live_notes(payload.clone(), task).unwrap_or(());

                    if let Action::Delete = payload.0 {
                        chat_view.delete_note(&payload.1);
                    } else {
                        chat_view.insert_note(&mut payload.1);
                    }
                }
            }
            if let Action::Create = payload.0 {
                if let (Some(id), Some(user)) =
                    (&payload.1.clone().task_id, &self.current_user)
                {
                    if let Some(task) = self.tasks.iter().find(|task| {
                        task.id == id.clone() && task.assignee == user.id && !task.completed
                    }) {
                        // This should work with ID and not initials
                        if payload.1.everest_initials != user.everest_initials {
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
