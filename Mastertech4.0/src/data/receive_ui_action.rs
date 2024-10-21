use log::info;

use crate::{
    MasterTechApp,
    utilities::{
        displays::{
            chats::ChatView,
            modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal},
        },
        ModalType, TaskUiActions,
    },
};

impl MasterTechApp {
    pub fn receive_ui_action(&mut self) {
        if let Ok(action) = self.context.ui_actions_rx.try_recv() {
            match action {
                TaskUiActions::OpenTaskModal(task) => {
                    let task_modal = if !task.task_note.is_empty() {
                        let chat_modal = ChatView::new(
                            task.task_note.clone(),
                            self.context.current_user.as_ref().unwrap().clone(),
                            task.id.clone(),
                        );
                        TaskModal::new(chat_modal, task.clone())
                    } else {
                        TaskModal::new(ChatView::default(), task.clone())
                    };
                    self.context.current_modal = ModalType::TaskModal(task_modal);
                    self.context.task_modal_handler.open();
                }
                TaskUiActions::CreateTaskModal => {
                    let create_modal =
                        CreateTaskModal::new("Create Task", self.context.store_users.clone());
                    self.context.current_modal = ModalType::CreateTaskModal(create_modal);
                    self.context.create_task_modal_handler.open();
                }
                TaskUiActions::Response(_res) => {}
                TaskUiActions::OpenChatModal(pld) => {
                    info!("Got Chat action");
                    if let Some(current_user) = self.context.current_user.as_ref() {
                        let chat_modal =
                            ChatView::new(pld.1.to_owned(), current_user.clone(), pld.0.clone());
                        self.context.current_modal = ModalType::ChatView(chat_modal);
                        self.context.chat_modal_handler.open();
                    } // self.context.chat = ModalType::ChatView(pld);
                }
                _ => (),
            }
        }
    }
}
