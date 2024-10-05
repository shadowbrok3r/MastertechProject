use database::live_data::handle_live_notes;
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use log::info;
use surrealdb::Action;

use crate::{utilities::ModalType, MtechServer};

impl MtechServer {
    pub fn receive_notes(&mut self) {
        if let Ok(mut payload) = self.context.notes_rx.try_recv() {
            info!("{:?}", payload);
            self.context.new_note = true;
            if let ModalType::TaskModal(task_modal) = &mut self.context.current_modal {
                handle_live_notes(payload.clone(), &mut task_modal.task).unwrap_or(());

                if let Action::Delete = payload.0 {
                    task_modal.chat_view.delete_note(&payload.1);
                } else {
                    task_modal.chat_view.insert_note(&mut payload.1);
                }
            } else if let ModalType::ChatView(chat_view) = &mut self.context.current_modal {
                let task = self
                    .context
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == chat_view.task_id);
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
                    (&payload.1.clone().task_id, &self.context.current_user)
                {
                    if let Some(task) = self
                        .context
                        .tasks
                        .iter()
                        .find(|task| task.id == Some(id.clone()) && task.assignee == user.id)
                    {
                        // This should work with ID and not initials
                        if payload.1.everest_initials != user.everest_initials {
                            let toast = &mut self.context.toasts;
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
