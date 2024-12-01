use crate::app_state::MtechServer;
use displays::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal, ModalType}, TaskUiActions};
use log::info;

impl MtechServer {
    pub fn receive_ui_action(&mut self) {
        if let Ok(action) = self.context.shared_ctx.ui_actions_rx.try_recv() {
            match action {
                TaskUiActions::OpenTaskModal(task) => {
                    if let Some(usr) = self.context.shared_ctx.current_user.clone() {
                        let task_modal = if !task.task_note.is_empty() {
                            TaskModal::new(ChatView::new(
                                    task.task_note.clone(),
                                    usr,
                                    task.id.clone(),
                                    self.context.shared_ctx.store_users.clone()
                                ),
                                task.clone()
                            )
                        } else {
                            TaskModal::new(
                                ChatView::new(
                                    task.task_note.clone(),
                                    usr,
                                    task.id.clone(),
                                    self.context.shared_ctx.store_users.clone(),
                                ),
                                task.clone()
                            )
                        };
                        self.context.opened_modals
                            .entry(format!("{} - Task Modal", task_modal.title))
                            .or_insert(ModalType::TaskModal(task_modal));
                    }
                }
                TaskUiActions::CreateTaskModal => {
                    let create_modal = CreateTaskModal::new(
                        "Create Task",
                        self.context.shared_ctx.store_users.clone(),
                        self.context.tur_channel.0.clone(),
                    );
                    self.context.opened_modals
                        .entry(create_modal.title.clone())
                        .or_insert(ModalType::CreateTaskModal(create_modal));
                }
                TaskUiActions::OpenChatModal(pld) => {
                    info!("Got Chat action");
                    if let Some(current_user) = self.context.shared_ctx.current_user.as_ref() {
                        let chat_modal = ChatView::new(
                            pld.1.to_owned(),
                            current_user.clone(),
                            pld.0.clone(),
                            self.context.shared_ctx.store_users.clone(),
                        );
                        let task = self
                            .context
                            .shared_ctx
                            .tasks
                            .iter()
                            .find(|task| task.id == pld.0.clone());

                        let title = if let Some(task) = task {
                            task.task_name.clone()
                        } else {
                            "New Chat".to_string()
                        };

                        self.context.opened_modals
                            .entry(title)
                            .or_insert(ModalType::ChatView(chat_modal));
                        // info!("self.context.opened_modals: {:?}", self.context.opened_modals);
                    }
                }
                TaskUiActions::Response(_res) => (),
                TaskUiActions::Editing(_record_id) => (),
                TaskUiActions::CommitChanges(_record_id) => (),
                TaskUiActions::None => (),
                
            };
        }
    }
}
