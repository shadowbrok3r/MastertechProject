use crate::app_state::SharedContext;
use crate::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal, ModalType}, TaskUiActions};
use database::schema::TaskNotePayload;
use log::info;
use tokio::task::spawn_local;

impl SharedContext {
    pub fn receive_ui_action(&mut self) {
        if let Ok(action) = self.ui_actions_rx.try_recv() {
            match action {
                TaskUiActions::OpenTaskModal(task) => {
                    if let Some(usr) = self.current_user.clone() {
                        let task_modal = if !task.task_note.is_empty() {
                            TaskModal::new(ChatView::new(
                                    task.task_note.clone(),
                                    usr,
                                    task.id.clone(),
                                    self.store_users.clone()
                                ),
                                task.clone()
                            )
                        } else {
                            TaskModal::new(
                                ChatView::new(
                                    task.task_note.clone(),
                                    usr,
                                    task.id.clone(),
                                    self.store_users.clone(),
                                ),
                                task.clone()
                            )
                        };
                        let title = format!("{} - Task Modal", task_modal.title);

                        if self.opened_modals.get(&title).is_some() {
                            self.opened_modals.remove_entry(&title);
                        } else {
                            self.opened_modals
                                .entry(title)
                                .or_insert(ModalType::TaskModal(task_modal));
                        }
                    }
                }
                TaskUiActions::CreateTaskModal => {
                    let create_modal = CreateTaskModal::new(
                        "Create Task",
                        self.store_users.clone(),
                        self.tur_channel.0.clone(),
                    );

                    if self.opened_modals.get(&create_modal.title).is_some() {
                        self.opened_modals.remove_entry(&create_modal.title);
                    } else {
                        self.opened_modals
                            .entry(create_modal.title.clone())
                            .or_insert(ModalType::CreateTaskModal(create_modal));
                    }
                }
                TaskUiActions::OpenChatModal(pld) => {
                    info!("Got Chat action");

                    let notes = 
                    spawn_local(async move {

                    });
                    if let Some(current_user) = self.current_user.as_ref() {
                        let chat_modal = ChatView::new(
                            pld.1.to_owned(),
                            current_user.clone(),
                            pld.0.clone(),
                            self.store_users.clone(),
                        );
                        let task = self
                            .tasks
                            .iter()
                            .find(|task| task.id == pld.0.clone());

                        let title = if let Some(task) = task {
                            task.task_name.clone()
                        } else {
                            "New Chat".to_string()
                        };

                        if self.opened_modals.get(&title).is_some() {
                            self.opened_modals.remove_entry(&title);
                        } else {
                            self.opened_modals
                                .entry(title)
                                .or_insert(ModalType::ChatView(chat_modal));
                        }
                        // info!("self.opened_modals: {:?}", self.opened_modals);
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


async fn get_or_insert_notes(notes: Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error> {
    // I will probably want to do this manually opposed to using TaskNotePayload::get_thread_id_from_order(&self)
    // Because, that will have to make a separate API call for every single note, since &Self, in the 
    // TaskNotePayloadHelper is TaskNotePayload, not Vec<TaskNotePayload>. so I will just want to 
    // take a order number, query all the notes, see if all the notes in the db match all the notes
    // in prestashop, and if not, sync the two databases.
    Ok(())
}