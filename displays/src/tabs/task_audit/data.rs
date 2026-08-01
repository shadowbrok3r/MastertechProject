use database::schema::{TaskNotePayload, User, helper_traits::EmployeeHelper, prestashop::OrderState, prestashop_schema::{self, Employee, MissedCallOrder, PrestashopPayload}, utilities::{create_full_task_payload, get_missing_call_days, get_prestashop_payload, needs_call_today}};
use crossbeam::channel::Sender;
use egui_data_table::DataTable;
use chrono::Utc;

use crate::{PlatformSpawner, Spawner};

use super::{row_viewer::{RowFieldUpdate, TaskRowViewer}, TaskAudit, TaskAuditViewer};

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
            let orders = match &selected {
                TaskAudit::MyInRepair => employee.get_my_services_in_repair().await,
                TaskAudit::MyServices => employee.get_all_my_services().await,
                TaskAudit::Status(state) => {
                    employee.get_services_by_status(state.to_id_str(), start_idx, start_idx + 30, &id_store).await
                }
                TaskAudit::NeedsCallToday => {
                    employee.get_services_by_status(OrderState::CheckinShelf.to_id_str(), start_idx, start_idx + 30, &id_store).await
                }
                TaskAudit::AllExcept { order_type, excluded } => {
                    let included = order_type.included_states(excluded);
                    if included.is_empty() {
                        Ok(Vec::new())
                    } else {
                        let ids: Vec<&str> = included.iter().map(|s| s.to_id_str()).collect();
                        employee
                            .get_services_by_states(order_type.to_id_str(), &ids, start_idx, start_idx + 30, &id_store)
                            .await
                    }
                }
            };

            match orders {
                Ok(svcs) => {
                    for order_num in svcs.iter() {
                        if current_orders.contains(&order_num.id) {
                            continue;
                        }
                        match Employee::to_prestashop_payload(&order_num.id).await {
                            Ok(service) => {
                                // For the "needs call today" view, only keep orders that
                                // were checked in on a prior day and have no call today.
                                if matches!(selected, TaskAudit::NeedsCallToday)
                                    && !needs_call_today(&service.order.date_add, &service.customer_messages)
                                {
                                    continue;
                                }
                                let missing_days = get_missing_call_days(&service.order.date_add, &service.customer_messages);
                                if !missing_days.is_empty() {
                                    let _ = missed_calls_tx.try_send(vec![MissedCallOrder {
                                        id: service.order.id.clone(),
                                        date_add: service.order.date_add.clone(),
                                        missing_days,
                                    }]);
                                }
                                let _ = order_tx.try_send(service);
                            }
                            Err(e) => log::error!("Error getting service payload: {e:?}"),
                        }
                    }
                }
                Err(e) => log::error!("Error getting services: {e:?}"),
            }
        });

        let elapsed = time.elapsed();
        log::info!("Time elapsed: {elapsed:?}");
    }

    /// Writes a field update into every cached copy of the order's row.
    fn apply_field_update(&mut self, update: &RowFieldUpdate) {
        for table in self.service_map.values_mut() {
            for row in table.iter_mut() {
                match update {
                    RowFieldUpdate::Status { order_id, new_state } if &row.order.id == order_id => {
                        row.order.current_state = new_state.clone();
                    }
                    RowFieldUpdate::SalesRep { order_id, employee } if &row.order.id == order_id => {
                        row.sales_rep = Some(employee.clone());
                    }
                    RowFieldUpdate::SplitRep { order_id, employee } if &row.order.id == order_id => {
                        row.split_rep = employee.clone();
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn receive(&mut self, store_users: Vec<User>, _frame: &mut eframe::Frame) {
        // Apply immediate in-table edits emitted by the row comboboxes so the
        // table reflects status / sales rep / split rep changes right away.
        while let Ok(update) = self.services_viewer.field_update_channel.1.try_recv() {
            self.apply_field_update(&update);
        }

        // A rejected write restores the previous value and flags the cell it came from.
        while let Ok(failure) = self.services_viewer.write_failure_channel.1.try_recv() {
            log::error!(
                "Reverting order {} after rejected write: {}",
                failure.revert.order_id(),
                failure.message
            );
            self.services_viewer.write_errors.insert(
                failure.revert.order_id().to_string(),
                (failure.revert.column(), failure.message.clone()),
            );
            self.apply_field_update(&failure.revert);
        }

        // When a note is created from the "needs call today" view, the order
        // now has a call today, so drop it from the table.
        while let Ok(service_number) = self.services_viewer.note_created_channel.1.try_recv() {
            if matches!(self.audit_selection, TaskAudit::NeedsCallToday) {
                let key = self.audit_selection.cache_key();
                if let Some(table) = self.service_map.get_mut(&key) {
                    table.retain(|row| row.order.id != service_number);
                }
            }
        }

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
            let key = self.audit_selection.cache_key();

            self
                .service_map
                .entry(key.clone())
                .or_insert(DataTable::default());


            // Replace by order id so a re-read refreshes the row in place.
            if let Some(k) = self.service_map.get_mut(&key) {
                match k.iter_mut().find(|row| row.order.id == order.order.id) {
                    Some(existing) => *existing = order,
                    None => {
                        log::info!("Loaded order {}", order.order.id);
                        k.push(order);
                    }
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
                let note_created_tx = self.services_viewer.note_created_channel.0.clone();
                self.services_viewer.chat_view
                    .set_notes(notes.clone())
                    .set_service_number(svc_num.clone())
                    .set_users(store_users.clone())
                    .set_prestashop_only(true)
                    .set_note_created_tx(note_created_tx);
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
            None,
        )
        .await;

        Ok(value)
    }
}
