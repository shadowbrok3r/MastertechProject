use bytes::Bytes;
use database::{live_data::listen_data, schema::{ChatAction, ChatMessageType, ChatThread, User, UserMessage, CHAT_THREAD_TABLE, USER_MESSAGE_TABLE}};
use crate::{get_current_user_from_auth, get_database_users, PlatformSpawner, Spawner};
// use surrealdb::RecordId;
use eframe::egui::Ui;
use super::UserChat;

impl UserChat {
    pub fn first_run(&mut self) {
        if self.store_users.is_empty() {
            self.set_users();
        } else {
            self.first_run = false;
            let tx = self.thread_listener_tx.clone();
            let msg_tx = self.message_listener_tx.clone();
            PlatformSpawner::spawn(async move {
                let _ = listen_data(tx, CHAT_THREAD_TABLE).await;
            }); 
            PlatformSpawner::spawn(async move {
                let _ = listen_data(msg_tx, USER_MESSAGE_TABLE).await;
            });
        }
    }

    pub fn receive(&mut self, ui: &mut Ui) {
        if let Ok(thread) = self.thread_rx.try_recv() {
            ui.ctx().request_repaint();
            // Add thread to threads list if not already present
            if !self.threads.iter().any(|t| t.id == thread.id) {
                self.threads.push(thread.clone());
            }
            self.selected_thread = Some(thread.clone());
            // self.chat_title = thread.
            // Ensure thread_messages has an entry
            self.thread_messages.entry(thread.id.clone()).or_insert(Vec::new());
            // Load messages for the selected thread
            let thread_id = thread.id.clone();
            let msg_tx = self.chat_msg_tx.clone();
            PlatformSpawner::spawn(async move {
                if let Ok(messages) = UserMessage::load_messages_from_thread(thread_id).await {
                    for msg in messages {
                        let _ = msg_tx.try_send(msg);
                    }
                }
            });
        }

        if let Ok(msg) = self.chat_msg_rx.try_recv() {
            ui.ctx().request_repaint();
            let messages = self.thread_messages.entry(msg.thread_id.clone()).or_insert(Vec::new());
            if !messages.iter().any(|m| m.id == msg.id) {
                messages.push(msg.clone());
                messages.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }

        // Append characters to the current streaming message
        if let Ok(response) = self.chat_action_rx.try_recv() {
            ui.ctx().request_repaint();
            log::info!("Received chat action: {response:?}");

            // Ensure the thread exists
            match response {
                ChatAction::SelectThread(user_id) => {
                    let current_user = self.current_user.clone();
                    let thread_tx = self.thread_tx.clone();
                    PlatformSpawner::spawn(async move {
                        let thread = ChatThread::find_or_create_thread(current_user, vec![user_id]).await;
                        if let Ok(thread) = thread {
                            log::info!("Thread found or created: {thread:?}");
                            let _ = thread_tx.try_send(thread);
                        } else {
                            log::error!("Failed to find or create thread: {:?}", thread.err());
                        }
                    });
                },
                ChatAction::NewThread(user_id) => {
                    let thread = ChatThread::new(self.current_user.clone())
                        .insert_user_to_thread(user_id.clone());
                    if !self.threads.iter().any(|t| t.id == thread.id) {
                        self.threads.push(thread.clone());
                    }
                    self.thread_messages.entry(thread.id.clone()).or_insert(Vec::new());
                    self.selected_thread = Some(thread.clone());
                    let thread_tx = self.thread_tx.clone();
                    PlatformSpawner::spawn(async move {
                        let thread_res = thread.create_thread().await;
                        log::info!("Thread created: {thread_res:?}");
                        if let Ok(Some(thread)) = thread_res {
                            let _ = thread_tx.try_send(thread);
                        }
                    });
                },
                ChatAction::ArchiveChat(_record_id) => {},
                ChatAction::RemoveChat(_record_id) => {},
                ChatAction::UpdateMessage(_record_id) => {},
                ChatAction::DeleteMessage(_record_id) => {},
                ChatAction::AddUser(_record_id) => {},
                ChatAction::RemoveUser(_record_id) => {},
                ChatAction::UpdateChat(_record_id) => {},
                ChatAction::CreateGroupThread(user_ids) => {
                    let current_user = self.current_user.clone();
                    let thread_tx = self.thread_tx.clone();
                    PlatformSpawner::spawn(async move {
                        let thread = ChatThread::find_or_create_thread(current_user, user_ids).await;
                        if let Ok(thread) = thread {
                            log::info!("Group thread found or created: {thread:?}");
                            let _ = thread_tx.try_send(thread);
                        } else {
                            log::error!("Failed to find or create group thread: {:?}", thread.err());
                        }
                    });
                },
                ChatAction::SubmitMessage(chat_message_type) => {
                    if let Some(thread) = self.selected_thread.clone() {
                        let user: surrealdb::RecordId = self.current_user.get_id().clone();
                        let tx = self.chat_msg_tx.clone();
                        PlatformSpawner::spawn(async move {
                            match Self::submit_message(thread.clone(), chat_message_type, tx, user).await {
                                Ok(_) => log::info!("Created Message"),
                                Err(e) => log::error!("Error creating message: {e:?}")
                            }
                        });
                    }
                },
                ChatAction::OpenModal((open, file_id)) => {
                    self.open_modal = open;
                    self.image_id = file_id.clone();
                },
                ChatAction::UploadedFiles(files) => {
                    if let Some(thread) = self.selected_thread.clone() {
                        let user = self.current_user.clone();
                        let tx = self.chat_msg_tx.clone();
                        PlatformSpawner::spawn(async move {
                            for file in files.iter() {
                                let img = file.read().await; 
                                let id = file.file_name();
                                let bytes = Bytes::copy_from_slice(&img);
                                match Self::submit_message(
                                    thread.clone(), 
                                    ChatMessageType::Image((id, bytes)), 
                                    tx.clone(), 
                                    user.get_id()
                                ).await {
                                    Ok(_) => log::info!("Created Message"),
                                    Err(e) => log::error!("Error creating message: {e:?}")
                                }
                            }
                        });
                    }
                }
                // ChatAction::SaveNote(record_id) => {
                //     if self.allow_edit.contains(&record_id) {
                //         if let Some(msg) = self.edit_text.get_mut(&task_note.id.to_string()){
                //             let mut task_note = msg.clone();
                //             task_note.note = msg.note.clone();
                //             PlatformSpawner::spawn(async move {
                //                 match task_note.update_note().await {
                //                     Ok(res) => info!("chats/mod.rs -> Modify note response:: {res:?}"),
                //                     Err(e) => error!("Error modifying note: {e:?}"),
                //                 }
                //             });
                //         }
                //     }
                //     self.allow_edit.remove(&task_note.id.to_string());
                // },
                // ChatEvent::Edit(id) => { self.allow_edit.insert(id.to_string()); },
                // ChatEvent::CancelEdit(id) => { self.allow_edit.remove(&id.to_string()); },
                // ChatEvent::DeleteNote(note) => {
                //     self.delete = Some(note.clone());
                //     let mut item = note.clone();
                //     PlatformSpawner::spawn(async move {
                //         match item.delete_note().await{
                //             Ok(_) => info!("chats/mod.rs -> Deleted Note"),
                //             Err(e) => error!("chats/mod.rs -> Error deleting note: {e:?}"),
                //         }
                //     })
                // },
                _ => {}
            };
        }

        if let Ok((action, msg)) = self.message_listener_rx.try_recv() {
            ui.ctx().request_repaint();
            let messages = self.thread_messages.entry(msg.thread_id.clone()).or_insert(Vec::new());
            match action {
                surrealdb::Action::Create => {
                    let messages = self.thread_messages.entry(msg.thread_id.clone()).or_insert(Vec::new());
                    if !messages.iter().any(|m| m.id == msg.id) {
                        messages.push(msg.clone());
                        messages.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                    }
                },
                surrealdb::Action::Update => {
                    if let Some(idx) = messages.iter().position(|m| m.id == msg.id) {
                        messages[idx] = msg.clone();
                        messages.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                    }
                },
                surrealdb::Action::Delete => {
                    messages.retain(|m| m.id != msg.id);
                },
                _ => {},
            }
        }

        while let Ok((action, chat_thread)) = self.thread_listener_rx.try_recv() {
            ui.ctx().request_repaint();
            match action {
                surrealdb::Action::Create => {
                    if !self.threads.iter().any(|t| t.id == chat_thread.id) {
                        self.threads.push(chat_thread.clone());
                        self.thread_messages.entry(chat_thread.id.clone()).or_insert(Vec::new());
                    }
                },
                surrealdb::Action::Update => {
                    if let Some(idx) = self.threads.iter().position(|t| t.id == chat_thread.id) {
                        self.threads[idx] = chat_thread.clone();
                    }
                },
                surrealdb::Action::Delete => {
                    self.threads.retain(|t| t.id != chat_thread.id);
                    self.thread_messages.remove(&chat_thread.id);
                    if self.selected_thread.as_ref().map_or(false, |t| t.id == chat_thread.id) {
                        self.selected_thread = None;
                    }
                },
                _ => {},
            }
        }
    }

    pub fn set_users(&mut self) {
        self.current_user = get_current_user_from_auth().unwrap_or(User::default());
        let me = self.current_user.clone();
        let users = get_database_users();
        self.store_users = users
            .iter()
            .filter(|u| u.get_store() == me.get_store() && u.get_id() != me.get_id())
            .cloned()
            .collect::<Vec<User>>();

        let tx = self.thread_tx.clone();
        PlatformSpawner::spawn(async move {
            match ChatThread::load_threads(me.get_id().clone()).await {
                Ok(threads) => {
                    for thread in threads {
                        let _ = tx.try_send(thread);
                    }
                },
                Err(e) => log::error!("Error loading threads: {e:?}"),
            }
        });
    }

    pub async fn submit_message(
        thread: ChatThread, 
        message: ChatMessageType, 
        tx: crossbeam::channel::Sender<UserMessage>, 
        user: surrealdb::RecordId
    ) -> anyhow::Result<(), anyhow::Error> {
        let msg = UserMessage::new(
            thread.id.clone(),
            user, 
            message
        )
        .create_message()
        .await?;

        if let Some(msg) = msg {
            let _ = tx.try_send(msg);
        }

        Ok(())
    }
}