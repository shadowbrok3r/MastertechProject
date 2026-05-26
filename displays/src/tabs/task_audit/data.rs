use database::schema::{TaskNotePayload, User, get_data::get_services_by_status, helper_traits::EmployeeHelper, prestashop::OrderState, prestashop_schema::{self, Employee, MissedCallOrder, PrestashopOrderType, PrestashopPayload}, utilities::{create_full_task_payload, get_prestashop_payload}};
use crossbeam::channel::Sender;
use egui_data_table::DataTable;
use itertools::Itertools;
use chrono::Utc;

use crate::{PlatformSpawner, Spawner};

use super::{row_viewer::TaskRowViewer, TaskAudit, TaskAuditViewer};

impl TaskAuditViewer { // NEED TO LOOK INTO SOME NOTES THINKING THERE IS NOT A SERVICE NUMBER IF THERE ISNT A THREAD
    pub fn get_services(
        selected: TaskAudit, 
        current_user: Option<User>, 
        order_tx: Sender<prestashop_schema::PrestashopPayload>, 
        current_orders: Vec<String>,
        start_idx: i32,
        missed_calls_tx: Sender<Vec<MissedCallOrder>>,
        id_store: String
    ) {
        let time = web_time::Instant::now();
        let usr = current_user.clone().unwrap_or_default();
        let id = usr.get_employee_id().unwrap_or_default();
        let mut employee = Employee::default();
        employee.id = format!("{id}");
        employee.id_store = usr.get_store_id().unwrap_or_default();
        let id_store = id_store.clone();
        PlatformSpawner::spawn(async move {
            match selected {
                TaskAudit::CheckinShelf => {
                    // Fetch services within the range
                    let orders = employee
                        .get_services_by_status(OrderState::CheckinShelf.to_id_str(), start_idx, start_idx+30, &id_store)
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e)
                    };
                },
                TaskAudit::MyInRepair => {
                    // Fetch services within the range            
                    let orders = employee
                        .get_my_services_in_repair()
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e)
                    };
                },
                TaskAudit::InRepair => {
                    // Fetch services within the range
                    let orders = employee
                        .get_services_by_status(OrderState::InRepair.to_id_str(), start_idx, start_idx+30, &id_store)
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::error!("Error getting inrepair services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::error!("Error getting in repair shelf services: {:?}", e)
                    };
                },
                TaskAudit::DoneShelf => {
                    // Fetch services within the range
                    let orders = employee
                        .get_services_by_status(OrderState::DoneShelf.to_id_str(), start_idx, start_idx+30, &id_store)
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::error!("Error with get_services_by_status 40: : {:?}", e)
                    };
                },
                TaskAudit::AllServices => {
                    // Fetch services within the range
                    let orders = employee
                        .get_all_my_services()
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::error!("Error with get_all_services_in_my_store: {:?}", e)
                    };
                },
                TaskAudit::MyServices => {
                    // Fetch services within the range
                    let orders = employee
                        .get_all_my_services()
                        .await;

                    // Handle the fetched services
                    match orders {
                        Ok(svcs) => {
                            for order_num in svcs.iter() {
                                if !current_orders.contains(&order_num.id) {
                                    let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                    match presta_payload {
                                        Ok(service) => order_tx.try_send(service).unwrap(),
                                        Err(e) => log::error!("Error getting check-in shelf services: {:?}", e),
                                    }
                                }
                            }
                        },
                        Err(e) => log::error!("Error with get_my_services_in_repair: {:?}", e)
                    };
                },
                TaskAudit::NeedsCall => {
                    // let endpoint = PrestashopOrderType::CheckinShelf;
                    for endpoint in PrestashopOrderType::VALUES {
                        if endpoint != PrestashopOrderType::DoneShelf {
                            // If refresh is true, grab new data immediately
                            match get_services_by_status(endpoint.id(), &employee.id_store).await {
                                Ok(missed_calls) => {
                                    let _ = missed_calls_tx.try_send(missed_calls.clone());
                                    for order_num in missed_calls.iter() {
                                        if !current_orders.contains(&order_num.id) {
                                            let presta_payload = Employee::to_prestashop_payload(&order_num.id).await;
                                            match presta_payload {
                                                Ok(service) => order_tx.try_send(service).unwrap(),
                                                Err(e) => log::error!("Error getting check-in shelf services: {:?}", e),
                                            }
                                        }
                                    }
                                },
                                Err(e) => log::error!("Error with get_services_by_status: {:?}", e),
                            };
                        }
                    }
                },
            }
        });

        let elapsed = time.elapsed();
        log::info!("Time elapsed: {elapsed:?}");
    }

    pub fn receive(&mut self, store_users: Vec<User>, _frame: &mut eframe::Frame) {
        if let Ok(missed_calls) = self.missed_calls_rx.try_recv() {
            for new_call in missed_calls {
                if !self
                    .services_viewer
                    .missed_calls
                    .iter()
                    .any(|existing| existing.id == new_call.id) 
                {
                    self.services_viewer.missed_calls.push(new_call);
                }
            }
        }

        if let Ok(order) = self.order_channel.1.try_recv() {
            self.loading = true;
            let key = self.audit_selection.as_str();

            self
                .service_map
                .entry(key.to_string())
                .or_insert(DataTable::default());

            
            if let Some(k) = self.service_map.get_mut(&key.to_string()) {
                if !k.iter().contains(&order) {
                    log::info!("Order: {order:?}");
                    k.push(order);
                }
            }


            // if let self.time.el {
                // self.loading = false;
                // if let Some(storage) = frame.storage_mut() {
                //     let map: &HashMap<String, PrestashopPayload> = &self.service_map
                //         .iter()
                //         .map(|(k, v)| (k.clone(), v.clone().into()))
                //         .collect::<&HashMap<String, PrestashopPayload>>();
                //     match serde_json::to_string(map) {
                //         Ok(service_map) => storage.set_string("service_data", service_map),
                //         Err(e) => log::error!("error converting service_data to string: {e:?}"),
                //     }
                // }
            // }
        }
    
        if let Ok(notes) = self.services_viewer.notes_channel.1.try_recv() {
            log::info!("Got notes: {notes:?}");
            if self.services_viewer.selected.is_some() {
                log::info!("Creating chat view");
                let svc_num = self.services_viewer.selected.clone().unwrap_or_default().order.id.clone();
                self.services_viewer.chat_view
                    .set_notes(notes.clone())
                    .set_service_number(svc_num.clone())
                    .set_users(store_users.clone());
            }
        }

        if let Ok(order_data) = self.services_viewer.tur_channel.1.try_recv() {
            log::info!("Got order_data: {order_data:?}");
            // if self.services_viewer.selected.is_some() {
            //     self.services_viewer.chat_view = ChatView::new(order_data, current_user, store_users);
            // }
        }

    }

}

impl TaskRowViewer {
    pub async fn get_order_notes(service_number: String) -> anyhow::Result<Vec<TaskNotePayload>, anyhow::Error> {
        let existing_notes = TaskNotePayload::get_db_notes_from_service(service_number.clone()).await?;
        if !existing_notes.is_empty() {
            log::info!("We already have notes");
            Ok(existing_notes)
        } else {
            let notes = TaskNotePayload::get_prestashop_notes_from_service(&service_number, None).await?;
            log::info!("notes: {notes:?}");
            Ok(notes)
        }
    }

    pub async fn get_prestashop_order(service_number: String) -> anyhow::Result<PrestashopPayload, anyhow::Error> {
        log::info!("Did not have a task, creating");
        let value = get_prestashop_payload(&service_number).await?;

        let mut draft = database::schema::EntityDraft::default();
        database::schema::apply_prestashop_payload(
            &value,
            &mut draft,
            &database::schema::PrestaMapOptions {
                mode: database::schema::PrestaMapMode::Audit,
                task_id_strategy: database::schema::TaskIdStrategy::MatchServiceNumber,
                ..Default::default()
            },
        );
        draft.task.due_date = Utc::now().into();
        draft.task.task_name = format!(
            "{} - {}",
            draft.customer.name,
            draft.ticket.service_number
        );

        create_full_task_payload(
            draft.ticket,
            draft.customer,
            draft.computer,
            draft.task,
            draft.task_notes,
            false,
            false,
        )
        .await;

        Ok(value)
    }
}
