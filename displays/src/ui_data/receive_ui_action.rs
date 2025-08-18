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
                    let task_modal = TaskModal::new(
                        ChatView::new(
                            // task.task_note.clone(),
                            self.store_users.clone(),
                            task.id.clone(),
                            task.service_number.clone()
                        ),
                        task.clone()
                    );
                    
                    let title = &task_modal.title;

                    if self.opened_modals.get(title).is_some() {
                        self.opened_modals.remove_entry(title);
                    } else {
                        self.opened_modals
                            .entry(title.to_string())
                            .or_insert(ModalType::TaskModal(task_modal));
                    }
                }
                TaskUiActions::OpenChatModal((task_id, notes, service_number)) => {
                    info!("receive_ui_action -> Got Chat action: {:?}", task_id);

                    let payload = (task_id.clone(), notes.clone(), service_number.clone());
                    PlatformSpawner::spawn(async move {
                        match get_or_insert_notes(payload).await {
                            Ok(_) => info!("receive_ui_action -> get_or_insert_notes ran ok"),
                            Err(e) => log::error!("receive_ui_action -> Error with get_or_insert_notes: {e:?}"),
                        }
                    });

                    let chat_modal = ChatView::new(
                        self.store_users.clone(),
                        task_id.clone(),
                        service_number
                    );
                    
                    let task = self
                        .tasks
                        .iter()
                        .find(|task| task.id == task_id.clone());

                    let title = if let Some(task) = task {
                        task.task_name.clone()
                    } else {
                        task_id.to_string()
                    };

                    if self.opened_modals.get(&title).is_some() {
                        self.opened_modals.remove_entry(&title);
                    } else {
                        self.opened_modals
                            .entry(title)
                            .or_insert(ModalType::ChatView(chat_modal));
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
                TaskUiActions::OpenViewport(task) => {
                    info!("receive_ui_action -> TaskUiActions::OpenViewport");
                    let modal = TaskModal::new(
                        ChatView::new(
                            // task.task_note.clone(),
                            self.store_users.clone(),
                            task.id.clone(),
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
async fn get_or_insert_notes(
    note_payload: (surrealdb::RecordId, Vec<TaskNotePayload>, Option<String>)
) -> anyhow::Result<(), anyhow::Error> {
    // I will probably want to do this manually opposed to using TaskNotePayload::get_thread_id_from_order(&self)
    // Because, that will have to make a separate API call for every single note, since &Self, in the 
    // TaskNotePayloadHelper is TaskNotePayload, not Vec<TaskNotePayload>. so I will just want to 
    // take a order number, query all the notes, see if all the notes in the db match all the notes
    // in prestashop, and if not, sync the two databases.
    let (task_id, mut notes, service_number) = note_payload.clone();

    if notes.is_empty() || service_number.is_some() {
        if let Some(service) = service_number.as_ref() {
            let notes_res = TaskNotePayload::get_prestashop_notes_from_service(&service, Some(task_id.clone())).await;
            match notes_res {
                Ok(notes) => {log::info!("Got notes: {notes:?}"); },
                Err(e) => log::error!("Error getting notes from service number: {e:?}"),
            };
        }

    } else if !notes.is_empty() || service_number.is_none() {
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
                Err(e) => log::error!("receive_ui_action -> Error getting thread ID from order: {e:?}"),
            }
        } else {
            return Err(anyhow::anyhow!("receive_ui_action -> Error getting thread ID from order"));
        }
    } else {
        let notes_res = TaskNotePayload::get_db_notes_from_task_id(task_id.clone()).await;
        match notes_res {
            Ok(notes) => {log::info!("Got notes: {notes:?}"); },
            Err(e) => log::error!("Error getting notes from service number: {e:?}"),
        };
    }
    Ok(())
}