use crate::{PlatformSpawner, Spawner, TaskUiActions, chats::ChatView, modals::{ModalType, create_task_modal::{CreateTaskModal, Tur}, task_modal::TaskModal}, tabs::TabId};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use database::schema::{RecordIdExt, TaskNoteRead};
use crate::app_state::SharedContext;
use crate::viewports::ViewportData;
use log::info;

impl SharedContext {
    pub fn receive_ui_action(&mut self) {
        if let Ok(action) = self.ui_actions_rx.try_recv() {
            match action {
                TaskUiActions::OpenTaskModal(task) => {
                    // Mark notes as read for this task when modal is opened
                    self.last_read_notes.insert(task.id.clone(), chrono::Utc::now());
                    let read_task_id = task.id.clone();
                    PlatformSpawner::spawn(async move {
                        if let Err(e) = TaskNoteRead::mark_read(read_task_id).await {
                            log::error!("receive_ui_action -> TaskNoteRead::mark_read failed: {e:?}");
                        }
                    });
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
                    // Mark notes as read for this task
                    self.last_read_notes.insert(task_id.clone(), chrono::Utc::now());
                    let read_task_id = task_id.clone();
                    PlatformSpawner::spawn(async move {
                        if let Err(e) = TaskNoteRead::mark_read(read_task_id).await {
                            log::error!("receive_ui_action -> TaskNoteRead::mark_read failed: {e:?}");
                        }
                    });
                    // Construct chat view, seed with any provided notes, and kick off a refresh
                    let mut chat_modal = ChatView::new(
                        self.store_users.clone(),
                        task_id.clone(),
                        service_number
                    );
                    if !notes.is_empty() {
                        // Seed initial notes so the UI displays immediately
                        chat_modal.set_notes(notes.clone());
                    }
                    // Also fetch fresh notes in the background to ensure sync with Prestashop/DB
                    chat_modal.refresh_notes();
                    
                    let task = self
                        .tasks
                        .iter()
                        .find(|task| task.id == task_id.clone());

                    let title = if let Some(task) = task {
                        task.task_name.clone()
                    } else {
                        task_id.key_string()
                    };

                    if self.opened_modals.get(&title).is_some() {
                        self.opened_modals.remove_entry(&title);
                    } else {
                        self.opened_modals
                            .entry(title)
                            .or_insert(ModalType::ChatView(chat_modal));
                    }
                }
                TaskUiActions::CreateTaskModal(optional_task_data) => {
                    if let Some(task_data) = optional_task_data {
                        let mut create_modal = CreateTaskModal::new(
                            "Create Task",
                            self.store_users.clone(),
                            self.tur_channel.0.clone(),
                        );
                        create_modal.update_tur_info(task_data);
                    } else {
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
                }
                TaskUiActions::OpenCreateTaskModalFromOrder(presta_payload) => {
                    info!("Opening create task modal from order: {}", presta_payload.order.id);
                    let mut create_modal = CreateTaskModal::new(
                        "Create Task",
                        self.store_users.clone(),
                        self.tur_channel.0.clone(),
                    );
                    
                    // Get service details for device info
                    let service = presta_payload.order.associations.order_service.get(0);
                    let device_mfg = service.map(|s| s.device_mfg.clone()).unwrap_or_default();
                    let device_model = service.map(|s| s.device_model.clone()).unwrap_or_default();
                    let checkin_notes = service.map(|s| s.check_in_notes.clone()).unwrap_or_default();
                    
                    // Create a Tur struct from the PrestashopPayload
                    let tur = Tur {
                        data: presta_payload.clone(),
                        ticket_data: database::schema::TicketData {
                            service_number: presta_payload.order.id.clone(),
                            checkin_notes,
                            ..Default::default()
                        },
                        customer_data: database::schema::CustomerData {
                            name: presta_payload.customer.name.clone(),
                            phone_number: presta_payload.customer.phone_number.clone(),
                            email: presta_payload.customer.email.clone(),
                            ..Default::default()
                        },
                        computer_data: database::schema::ComputerData {
                            device_mfg: Some(device_mfg),
                            device_model: Some(device_model),
                            ..Default::default()
                        },
                        task_data: database::schema::LiveTaskPayload::default(),
                        task_notes: Vec::new(),
                        store_users: self.store_users.clone(),
                    };
                    
                    create_modal.update_tur_info(tur);
                    
                    // Remove existing modal if present, then insert
                    self.opened_modals.remove(&create_modal.title);
                    self.opened_modals
                        .entry(create_modal.title.clone())
                        .or_insert(ModalType::CreateTaskModal(create_modal));
                }
                TaskUiActions::OpenCreateTaskModalFromSystem(system_data) => {
                    info!("Opening create task modal from system: {}", system_data.order_id);
                    let mut create_modal = CreateTaskModal::new(
                        "Create Task",
                        self.store_users.clone(),
                        self.tur_channel.0.clone(),
                    );
                    
                    // Create a Tur struct from the SystemInStoreData
                    // The computer_data is already populated in SystemInStoreData
                    let tur = Tur {
                        data: database::schema::prestashop_schema::PrestashopPayload::default(),
                        ticket_data: database::schema::TicketData {
                            service_number: system_data.order_id.clone(),
                            ..Default::default()
                        },
                        customer_data: database::schema::CustomerData::default(),
                        computer_data: system_data.computer_data.clone(),
                        task_data: database::schema::LiveTaskPayload::default(),
                        task_notes: Vec::new(),
                        store_users: self.store_users.clone(),
                    };
                    
                    create_modal.update_tur_info(tur);
                    
                    // Remove existing modal if present, then insert
                    self.opened_modals.remove(&create_modal.title);
                    self.opened_modals
                        .entry(create_modal.title.clone())
                        .or_insert(ModalType::CreateTaskModal(create_modal));
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
                TaskUiActions::OpenAdminConsole(connection_string) => {
                    info!("receive_ui_action -> OpenAdminConsole: {connection_string}");
                    self.pending_tab_opens.push(TabId::AdminConsole);
                    self.pending_activate_tab = Some(TabId::AdminConsole);
                    // Surface which client to focus once the tab is active.
                    self.pending_admin_console_focus = Some(connection_string);
                }
                TaskUiActions::OpenClientDiagnostics(connection_string) => {
                    info!("receive_ui_action -> OpenClientDiagnostics: {connection_string}");
                    self.client_diagnostics_popup = Some(connection_string);
                }
                TaskUiActions::RefreshOpenServiceSuggestions(connection_string) => {
                    info!(
                        "receive_ui_action -> RefreshOpenServiceSuggestions: {connection_string}"
                    );
                    // Fire `Cmd::RequestOpenServiceCandidates { refresh: true }`
                    // over whichever admin transport is currently open to
                    // this client.  The hub silently drops the call if no
                    // session is active for that connection_string; we
                    // surface a toast so the operator doesn't think the
                    // button is broken.
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let cmd = crate::Cmd::RequestOpenServiceCandidates { refresh: true };
                        match bincode::serde::encode_to_vec(&cmd, bincode::config::standard()) {
                            Ok(serialized) => {
                                match crate::plugins::remote_egui_control::hub()
                                    .send_raw_binary(&connection_string, serialized)
                                {
                                    Ok(()) => {
                                        let _ = crate::get_toast_sender().try_send(
                                            crate::ToastMessage::Info(format!(
                                                "Refresh requested for {connection_string}",
                                            )),
                                        );
                                    }
                                    Err(e) => {
                                        let _ = crate::get_toast_sender().try_send(
                                            crate::ToastMessage::Warning(format!(
                                                "No active admin session to refresh \
                                                 {connection_string}: {e}",
                                            )),
                                        );
                                    }
                                }
                            }
                            Err(e) => log::error!(
                                "RefreshOpenServiceSuggestions: bincode encode failed: {e}"
                            ),
                        }
                    }
                }
                TaskUiActions::OpenServiceCandidateModal {
                    connection_string,
                    candidate_index,
                } => {
                    // Stage 4 owns the actual modal rendering; for
                    // Stage 3 we just stash the selection so the modal
                    // (when wired in the next step) knows what to show.
                    info!(
                        "receive_ui_action -> OpenServiceCandidateModal: \
                         cs={connection_string} idx={candidate_index}"
                    );
                    self.pending_open_service_candidate =
                        Some((connection_string, candidate_index));
                }
                TaskUiActions::None => (),
            };
        }
    }
}

