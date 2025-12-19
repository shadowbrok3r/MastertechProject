use database::schema::{get_data::get_services_by_status, helper_traits::{parse_email_user, EmployeeHelper}, prestashop_schema::{self, Employee, MissedCallOrder, PrestashopOrderType, PrestashopPayload}, utilities::{create_full_task_payload, get_prestashop_payload}, ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, User, TASK_TABLE, TICKET_TABLE};
use crossbeam::channel::Sender;
use egui_data_table::DataTable;
use itertools::Itertools;
use database::schema::RecordId;
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
                        .get_services_by_status("29", start_idx, start_idx+30, &id_store)
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
                        .get_services_by_status("30", start_idx, start_idx+30, &id_store)
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
                        .get_services_by_status("40", start_idx, start_idx+30, &id_store)
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
        let mut value = get_prestashop_payload(&service_number).await?;
        let mut customer = CustomerData::default();
        let mut ticket = TicketPayload::default();
        let mut task: TaskPayload = TaskPayload::default();
        let mut task_notes = Vec::new();

        let service_details = value.order.associations.order_service.clone();
        let mut services: Vec<RecordId> = Vec::new();

        let sales_rep = value.sales_rep.clone().unwrap_or_default();
        let split_rep = value.split_rep.clone().unwrap_or_default();
        let email = parse_email_user(&sales_rep.email);
        let email_split_rep = parse_email_user(&split_rep.email);

        customer.id = value.customer.id.clone();
        customer.cust_code = value.customer.cust_code.clone();
        customer.email = value.customer.email.clone();
        customer.name = value.customer.name.clone();
        customer.phone_number = value.customer.phone_number.clone();
        ticket.salesman = email_split_rep.to_string();
        ticket.sales_rep = email.to_string();
        ticket.tech = email.to_string();
        log::info!(
            "Salesman: {:?}\nTech: {:?}",
            ticket.salesman.clone(),
            ticket.tech.clone()
        );
        ticket.customer = Some(customer.clone());
        ticket.checkin_rep = email.to_string();
        ticket.terms = value.order.payment.clone();
        ticket.ticket_total = value.order.total_products_wt.clone();
        ticket.doc_alias = value.order.order_type.clone();
        ticket.service_number = value.order.id.clone();
        ticket.id = RecordId::from((
            TICKET_TABLE.to_string(),
            ticket.service_number.clone(),
        ));
        task.id = RecordId::from((
            TASK_TABLE.to_string(),
            ticket.service_number.clone(),
        ));

        for note in value.task_notes.iter_mut() {
            note.task_id = Some(task.id.clone());

            task_notes.push(note.clone());
        }
        task.task_note = task_notes.clone();
        task.due_date = Utc::now().into();

        services.push(ticket.id.clone());
        let mut computer_data = ComputerData::default();
        if !service_details.is_empty() {
            if service_details.len() == 1 {
                let svc = service_details.get(0);
                if let Some(service) = svc {
                    ticket.checkin_notes = service.check_in_notes.clone();
                    computer_data.device_name = Some(service.device_name.clone());
                    computer_data.device_mfg = Some(service.device_mfg.clone());
                    computer_data.device_model = Some(service.device_model.clone());
                    computer_data.device_serial = Some(service.device_serial.clone());
                }
            } else {
                log::info!("Theres a couple.... {:?}", service_details);
            }
        }

        task.service_ticket = Some(ticket.clone());

        task.task_name = format!(
            "{} - {}",
            &customer.name,
            ticket.service_number.clone()
        );

        create_full_task_payload(
            ticket.into(), 
            customer, 
            computer_data, 
            task.clone().into(), 
            task.clone().task_note, 
            false
        ).await;

        Ok(value)
    }
}
