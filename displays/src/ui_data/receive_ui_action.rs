use crate::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal, ModalType}, TaskUiActions};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use crate::{PlatformSpawner, Spawner};
use database::schema::TaskNotePayload;
use crate::app_state::SharedContext;
use crate::viewports::ViewportData;
use log::info;

impl SharedContext {
    pub fn receive_ui_action(&mut self) {
        if let Ok(action) = self.ui_actions_rx.try_recv() {
            match action {
                TaskUiActions::OpenTaskModal(task) => {
                    let task_modal = if task.service_ticket.is_some() {
                        TaskModal::new(
                            ChatView::new(
                                task.task_note.clone(),
                                self.store_users.clone(),
                                Some(task.id.clone()),
                                task.service_number.clone()
                            ),
                            task.clone()
                        )
                    } else {
                        let mut task_modal = TaskModal::default();
                        task_modal.chat_view = ChatView::new(
                            task.task_note.clone(),
                            self.store_users.clone(),
                            Some(task.id.clone()),
                            task.service_number.clone()
                        );
                        task_modal.task = task.clone();
                        task_modal
                    };
                    
                    let title = &task_modal.title;

                    if self.opened_modals.get(title).is_some() {
                        self.opened_modals.remove_entry(title);
                    } else {
                        self.opened_modals
                            .entry(title.to_string())
                            .or_insert(ModalType::TaskModal(task_modal));
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
                    info!("receive_ui_action -> Got Chat action: {:?}", pld.0.clone());

                    let note_payload = pld.clone();
                    PlatformSpawner::spawn(async move {
                        match get_or_insert_notes((note_payload.0, note_payload.1)).await {
                            Ok(_) => info!("receive_ui_action -> get_or_insert_notes ran ok"),
                            Err(e) => info!("receive_ui_action -> Error with get_or_insert_notes: {e:?}"),
                        }
                    });

                    let chat_modal = ChatView::new(
                        pld.1.to_owned(),
                        self.store_users.clone(),
                        Some(pld.0.clone()),
                        note_payload.2
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
                }
                TaskUiActions::OpenViewport(task) => {
                    info!("receive_ui_action -> TaskUiActions::OpenViewport");
                    let modal = TaskModal::new(
                        ChatView::new(
                            task.task_note.clone(),
                            self.store_users.clone(),
                            Some(task.id.clone()),
                            task.service_number.clone()
                        ),
                        task.clone(),
                    );
                
                    self.show_tasks_viewport
                        .entry(task.id)
                        .and_modify(|viewport_data| {
                            viewport_data.is_visible.store(true, Ordering::Relaxed);
                        })
                        .or_insert(ViewportData {
                            is_visible: Arc::new(AtomicBool::new(true)),
                            modal: ModalType::TaskModal(modal),
                        });
                        info!("receive_ui_action -> self.show_tasks_viewport: {:?}", self.show_tasks_viewport);
                },
                TaskUiActions::None => (),
                
            };
        }
    }
}


/// We are calling this even though it doesnt return anything BECAUSE 
/// the get_thread_id_from_order() will also handle the creation of new notes
/// and in turn, will live update the modal with notes from prestashop / the database
async fn get_or_insert_notes(note_payload: (surrealdb::RecordId, Vec<TaskNotePayload>)) -> anyhow::Result<(), anyhow::Error> {
    // I will probably want to do this manually opposed to using TaskNotePayload::get_thread_id_from_order(&self)
    // Because, that will have to make a separate API call for every single note, since &Self, in the 
    // TaskNotePayloadHelper is TaskNotePayload, not Vec<TaskNotePayload>. so I will just want to 
    // take a order number, query all the notes, see if all the notes in the db match all the notes
    // in prestashop, and if not, sync the two databases.
    let mut notes = note_payload.1;
    let task_id = note_payload.0;

    let existing_note = notes
        .iter_mut()
        .next();

    if let Some(note) = existing_note {
        match note.get_thread_id_from_order().await {
            Ok(thread_id) => {
                if thread_id.is_empty() {
                    return Err(anyhow::anyhow!("Thread ID is empty"));
                } else {
                    info!("receive_ui_action -> Thread ID: {thread_id:?}");
                    return Ok(());
                }
                
            },
            Err(e) => info!("receive_ui_action -> Error getting thread ID from order: {e:?}"),
        }
    } else {
        info!("receive_ui_action -> There were not any notes, checking prestashop");
        let mut tmp_note = TaskNotePayload::default();
        tmp_note.task_id = Some(task_id);
        match tmp_note.get_thread_id_from_order().await {
            Ok(thread_id) => {
                if thread_id.is_empty() {
                    return Err(anyhow::anyhow!("Thread ID is empty"));
                } else {
                    info!("receive_ui_action -> Thread ID: {thread_id:?}");
                    return Ok(());
                }
            },
            Err(e) => info!("receive_ui_action -> Error getting thread ID from order: {e:?}"),
        }
    }
    Ok(())
}