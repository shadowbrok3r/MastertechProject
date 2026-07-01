#[allow(unused_imports)]
use crate::{schema::{prestashop::xml::{modify_xml, remove_xml_tag}, prestashop_schema::{Address, Customer, CustomerMessage, CustomerThread, Employee, Order, Prestashop}, ConnectedClient, Priority, Qc, Record, RecordId, RecordIdExt, SurrealValue, Store, TaskNotePayload, User, UserAuthorization, CUSTOMER_TABLE, TASK_TABLE}, PlatformSpawner, Spawner, DATABASE};
#[allow(unused_imports)]
use super::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, LiveTaskPayload, LocalSebData, Notification, TicketData};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Utc, Weekday};
use std::{collections::HashMap, fmt::Debug};
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use async_trait::async_trait;
use log::{debug, info, warn};
use anyhow::{Error, Result};
use web_time::Instant;
use regex::Regex;

/// Result type for task creation operations
#[derive(Debug, Clone)]
pub enum TaskCreationResult {
    /// Task was created successfully
    Created { service_number: String },
    /// Task already exists with this service number
    AlreadyExists { service_number: String },
    /// Task was updated (same ID found)
    Updated { service_number: String },
    /// An error occurred during creation
    Error { message: String },
}

pub trait LiveUpdate {
    fn handle_live_create(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_update(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_delete(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>;
}

#[async_trait]
pub trait Task {
    // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static + SurrealValue>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static + SurrealValue>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static + SurrealValue>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static + SurrealValue>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
}

pub async fn query_id<T>(_table: String, id: RecordId) -> Result<Option<T>, Error>
where
    T: Serialize + Debug + Clone + 'static + for <'de> Deserialize <'de> + SurrealValue
{
    // Select directly by record id.  The prior form was
    // `SELECT * FROM $table WHERE id == $id`, where `$table` was bound to a
    // plain String — that doesn't resolve to a table identifier under the
    // SurrealDB 3.x type system, so the query silently returned `None` for
    // every call after the 3.1.0-beta.3 bump.  That broke every "is this
    // row already in the DB?" check: the OA3 friendly-name cache, the
    // create-vs-update branch in `create_client`, etc., which in turn
    // wiped admin-set fields like `friendly_name` on reconnect.
    //
    // Selecting by record id sidesteps the table-binding problem entirely
    // and is more efficient (no scan + filter).  `_table` is kept in the
    // signature so the 30+ callers compile unchanged.
    let record: Option<T> = DATABASE
        .query("SELECT * FROM ONLY $id")
        .bind(("id", id.clone()))
        .await?
        .take(0)?;

    info!("schema/utilities.rs/query_id -> Record: {:?}\nSELECT * FROM ONLY ${id:?}", record);
    Ok(record)
}

pub async fn check_id_existence<T>(_table: String, id: T) -> Result<Option<bool>, Error>
where
    T: Serialize + Debug + Clone + 'static + SurrealValue,
{
    let query = format!(
        r#"
            LET $query = (SELECT * FROM $id);
            IF $query != NULL || NONE {{ true }} ELSE {{ false }};
        "#
    );
    let record: Option<bool> = DATABASE
        .query(query.clone())
        .bind(("id", id))
        .await?
        .take(1)?;

    info!("schema/utilities.rs -> Query: {:?}  // {}", record, query);
    Ok(record)
}

pub async fn record_exists(id: RecordId) -> Result<Option<bool>, Error>
{
    let record_exists: Option<bool> = DATABASE
        .query("RETURN record::exists($id)")
        .bind(("id", id))
        .await?
        .take(0)?;

    match record_exists {
        Some(exists) => if exists { Ok(Some(true)) } else { Ok(Some(false)) },
        None => Err(anyhow::anyhow!("Record does not exist")),
    }
}

pub async fn get_qcs() -> anyhow::Result<Vec<Qc>, anyhow::Error> {
    let qcs: Vec<Qc> = DATABASE
        .query("SELECT * FROM qc")
        .await?
        .take(0)?;

    Ok(qcs)
}

pub async fn get_tasks_for_store(tx: Sender<Vec<LiveTaskPayload>>, store: String) -> Result<(), Error> {
    debug!("get_tasks");

    let query = r#"
        SELECT * FROM task WHERE $this.assignee.store == $store AND $this.completed IS false 
    "#; // WITH INDEX idx_store_due_date

    /*
            SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task 
        WHERE $this.assignee.store == $store AND $this.completed IS false
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
        
     */
    let start_query = Instant::now(); // Start timing the query

    let query_results: Vec<LiveTaskPayload> = DATABASE
        .query(query)
        .bind(("store", store.clone()))
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed(); // Measure query duration
    warn!("Query execution time for chunk {query_duration:?}");

    tx.try_send(query_results)?;

    Ok(())
}

pub async fn get_completed_tasks_for_store(tx: Sender<Vec<LiveTaskPayload>>, store: String) -> Result<(), Error> {
    debug!("get_completed_tasks");
    let query = r#"
        SELECT * FROM task WHERE $this.assignee.store == $store AND $this.completed IS true 
    "#;
    
    /*
     r#"
        SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task 
        WHERE $this.assignee.store == $store AND $this.completed IS true
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
        
    "#; */ // 
    
    let start_query = Instant::now(); // Start timing the query

    let query_results: Vec<LiveTaskPayload> = DATABASE
        .query(query)
        .bind(("store", store.clone()))
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed(); // Measure query duration
    warn!("Query execution time for chunk {query_duration:?}\ntask len: {}", query_results.len());

    tx.try_send(query_results)?;
    
    Ok(())
}

pub async fn get_associated_task_notes(
    tx: Sender<Vec<TaskNotePayload>>,
    task_id: RecordId,
) -> Result<(), Error> {
    debug!("get_associated_task_notes");
    let query = "SELECT * FROM task_note WHERE task_id == $id"; 
    let query_results: Vec<TaskNotePayload> = DATABASE
        .query(query)
        .bind(("id", task_id))
        .await?
        .take(0)?;

    tx.try_send(query_results)?;
    Ok(())
}

pub async fn get_store_users(tx: Sender<Vec<User>>, store: Store) -> Result<(), Error> {
    debug!("get_store_users");
    let data: Vec<User> = DATABASE
        .query("SELECT * FROM user WHERE store == $store AND active == true ")
        .bind(("store", store))
        .await?
        .take(0)?;
    tx.try_send(data)?;
    Ok(())
}

/// When duplicate `connected_client` rows exist for the same
/// `connection_string`, keep the best candidate: online first, then
/// newest `last_update`.
fn dedupe_connected_clients_by_connection_string(
    clients: Vec<ConnectedClient>,
) -> Vec<ConnectedClient> {
    use super::client::ClientKind;

    let mut by_cs: HashMap<String, ConnectedClient> = HashMap::new();
    for client in clients {
        if client.client_kind == ClientKind::BuildWorker {
            continue;
        }
        let key = client.connection_string.trim().to_string();
        if key.is_empty() {
            continue;
        }
        match by_cs.get(&key) {
            Some(existing) if !prefer_connected_client_row(&client, existing) => {}
            _ => {
                by_cs.insert(key, client);
            }
        }
    }
    let mut out: Vec<ConnectedClient> = by_cs.into_values().collect();
    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| last_update_ord(b).cmp(&last_update_ord(a)))
    });
    out
}

fn prefer_connected_client_row(candidate: &ConnectedClient, existing: &ConnectedClient) -> bool {
    if candidate.connected != existing.connected {
        return candidate.connected;
    }
    last_update_ord(candidate) > last_update_ord(existing)
}

fn last_update_ord(c: &ConnectedClient) -> String {
    c.last_update
        .as_ref()
        .map(|d| d.to_string())
        .unwrap_or_default()
}

pub async fn get_connected_clients(tx: Sender<Vec<ConnectedClient>>) -> Result<(), Error> {
    debug!("get_connected_clients");

    // Check if current user is Root - they can see all clients
    let is_root = match User::get_current_user_from_auth().await {
        Ok(Some(user)) => user.get_authorization() == UserAuthorization::Root,
        _ => false,
    };

    const LIST_FILTER: &str = "(client_kind IS NONE OR client_kind = 'machine') AND connected == true";

    if is_root {
        let query: Vec<ConnectedClient> = DATABASE
            .query(&format!(
                "SELECT * FROM connected_client \
                 WHERE {LIST_FILTER} AND assigned_user.id_store == $auth.id_store \
                 ORDER BY connected DESC, last_update DESC LIMIT 15",
            ))
            .await?
            .take(0)?;
        tx.try_send(dedupe_connected_clients_by_connection_string(query))?;
    } else {
        let query: Vec<ConnectedClient> = DATABASE
            .query(&format!(
                "SELECT * FROM connected_client \
                 WHERE assigned_user == $auth.id \
                   AND {LIST_FILTER} \
                 ORDER BY last_update DESC LIMIT 15",
            ))
            .await?
            .take(0)?;
        tx.try_send(dedupe_connected_clients_by_connection_string(query))?;
    }

    Ok(())
}

pub async fn disconnect_client(tx: Sender<Vec<RecordId>>, id: RecordId) -> Result<(), Error> {
    let query: Vec<RecordId> = DATABASE
        .query("UPDATE connected_client SET connected = false WHERE id == $id")
        .bind(("id", id.key_string()))
        .await?
        .take(0)?;
    tx.try_send(query)?;

    Ok(())
}

pub async fn modify_connected_client(tx: Sender<Vec<ConnectedClient>>) -> Result<(), Error> {
    // Check if current user is Root - they can see all clients
    let is_root = match User::get_current_user_from_auth().await {
        Ok(Some(user)) => user.get_authorization() == UserAuthorization::Root,
        _ => false,
    };

    let query: Vec<ConnectedClient> = if is_root {
        DATABASE
            .query("SELECT * FROM connected_client")
            .await?
            .take(0)?
    } else {
        DATABASE
            .query("SELECT * FROM connected_client WHERE assigned_user == $auth.id")
            .await?
            .take(0)?
    };
    tx.try_send(query)?;
    Ok(())
}

pub async fn delete_task(id: RecordId) -> Result<(), Error> {
    info!("schema/utilities.rs -> deleting id: {id:?}");
    let x = id.clone();
    let delete_result: Option<Record> = DATABASE.delete(
        (TASK_TABLE, id.key_string())
    )
    .await?;

    info!("schema/utilities.rs -> delete_result: {delete_result:?} for {:?}", x.key_string());
    
    Ok(())
}

pub async fn get_notifications(tx: Sender<Vec<Notification>>) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_notifications");
    let notifications: Vec<Notification> = DATABASE
        .query(
            "SELECT * FROM notification WHERE user == $auth.id ORDER BY created_at DESC LIMIT 50 "
        )
        .await?
        .take(0)?;
    // info!("schema/utilities.rs -> Notifications: {:?}", notifications.clone());
    tx.try_send(notifications)?;
    Ok(())
}

// pub async fn get_associated_ticket(tx: Sender<NewTicketChannel>, new_task: (Action, LiveTaskPayload)) -> Result<(), Error> {
//     debug!("get_associated_ticket");
//     let service_num = new_task.1.clone().service_number.unwrap_or_default();
//     DATABASE.set("service_num", service_num).await?;
//     let ticket: Option<TicketPayload> = DATABASE.query(format!("SELECT * FROM service_order WHERE service_number == $service_num FETCH computer, customer")).await?.take(0)?;
//     debug!("ticket: {:?}", ticket);
//     let new_ticket = ticket.unwrap_or_default();
//     let chnnl = NewTicketChannel { new_ticket, new_task };
//     tx.try_send(chnnl)?;
//     Ok(())
// }


#[async_trait]
pub trait NotificationMod {
    async fn delete_notification(&mut self) -> Result<(), Error>;
    async fn mark_notification(&mut self, read: bool) -> Result<(), Error>;
}

#[async_trait]
impl NotificationMod for Notification {
    async fn delete_notification(&mut self) -> Result<(), Error> {
        let query: Option<Record> = DATABASE
            .delete(("notification", self.id.key_string()))
            .await?;
        info!("schema/utilities.rs -> Deleted notification: {query:?}");
        Ok(())
    }

    async fn mark_notification(&mut self, read: bool) -> Result<(), Error> {
        let query: Option<Record> = DATABASE
            .query("UPDATE notification SET status = $read WHERE id == $id")
            .bind(("id", self.id.clone()))
            .bind(("read", if read { "Read" } else { "Unread" }))
            .await?
            .take(0)?;
        
        info!("schema/utilities.rs -> Updated notification: {query:?}");

        Ok(())
    }
}

#[cfg(not(target_arch="wasm32"))]
pub fn get_local_seb_data() -> anyhow::Result<LocalSebData, anyhow::Error> {
    let (tx, rx) = crossbeam::channel::bounded(1);
    PlatformSpawner::spawn(async move {
        let res = async {
            // supereasybackup.com/downloads/SuperEasyBackup.exe
            let file_path = "C:\\DCProtectData\\Shared\\Logs\\InstallationTracking.log"; // "D:\\Users\\Owner\\Desktop\\SEB\\DCProtectData-Customer\\Shared\\Logs\\InstallationTracking.log";

            // Read the file content
            let file_content = tokio::fs::read_to_string(file_path).await?;

            // Deserialize the XML content
            let result: LocalSebData = serde_json::from_str(&file_content)?;
            let _ = tx.send(result);
            Ok::<(), anyhow::Error>(())
        }.await;
        log::info!("Res: {res:?}");
    });

    if let Ok(seb) = rx.recv() {
        Ok(seb)
    } else {
        Err(anyhow::anyhow!("Could not get LocalSebData"))
    }
}

pub async fn create_full_task_payload(
    mut ticket_data: TicketData,
    customer_data: CustomerData,
    mut computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    mut task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
    allow_placeholder_computer: bool,
    assignee_override: Option<RecordId>,
) -> TaskCreationResult {
    info!("schema/utilities.rs -> Send_Payload");
    if send_specs
        && !allow_placeholder_computer
        && !super::entity_link::computer_has_minimal_hardware(&computer_data)
    {
        return TaskCreationResult::Error {
            message: "Refusing to create a Presta-only placeholder computer: link live \
                      hardware via the entity-link flow or confirm placeholder explicitly."
                .into(),
        };
    }
    // Honor an explicit assignee override (the create-task modal's verified
    // pick); otherwise derive from the ticket salesman, falling back to the
    // authenticated creator only as a last resort.
    let assignee_id = if let Some(id) = assignee_override {
        log::info!("schema/utilities.rs -> using assignee override: {id:?}");
        id
    } else {
        let queried_salesman = match User::query_user_from_email(ticket_data.salesman.clone()).await {
            Ok(user) => user,
            Err(e) => {
                warn!("Could not resolve salesman '{}' to a user ({e:?}); assigning to current user", ticket_data.salesman);
                match User::get_current_user_from_auth().await {
                    Ok(Some(user)) => user,
                    _ => return TaskCreationResult::Error {
                        message: "Could not resolve task assignee: salesman has no user record and no authenticated user is available.".into(),
                    },
                }
            }
        };
        log::error!("schema/utilities.rs -> Queried Salesman (Which will be assignee): {:?}", queried_salesman);
        queried_salesman.get_id()
    };
    
    // let task_id = task_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();
    let connection_string = computer_id.key_string();
    let service_number = ticket_data.service_number.clone();

    // A service_order with this service_number may already exist under a
    // different record id (e.g. a random-id placeholder from a partial
    // check-in). The service_number_idx UNIQUE index rejects a second row
    // with the same number, so reuse the existing record's id and turn the
    // write into an in-place update instead of a conflicting insert.
    if !service_number.is_empty() {
        let existing_service_order: Option<RecordId> = match DATABASE
            .query("SELECT VALUE id FROM service_order WHERE service_number == $service_number LIMIT 1")
            .bind(("service_number", service_number.clone()))
            .await
        {
            Ok(mut response) => response.take(0).unwrap_or_default(),
            Err(e) => {
                warn!("Could not check for existing service_order by number: {e:?}");
                None
            }
        };
        if let Some(existing_id) = existing_service_order {
            if existing_id != ticket_data.id {
                info!("Reusing existing service_order {existing_id:?} for service #{service_number}");
            }
            ticket_data.id = existing_id;
        }
    }
    let ticket_id = ticket_data.id.clone();

    task_data.task_name = format!(
        "{} - {}",
        &customer_data.name,
        service_number.clone()
    );
    task_data.service_ticket = Some(ticket_id.clone());
    task_data.service_number = Some(service_number.clone());
    task_data.priority = Priority::Normal;
    task_data.assignee = assignee_id;

    ticket_data.customer = Some(customer_id.clone());
    ticket_data.computer = Some(computer_id.clone());

    info!("schema/utilities.rs -> cust_record: {customer_data:?}");
    let update_customer: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
        .upsert(customer_id.clone())
        .content(customer_data.clone())
        .await;
    
    match update_customer {
        Ok(record) => {
            log::info!("Updated Customer {record:?}");
            // Always ensure computer has the customer linked
            if let Some(record_id) = record {
                computer_data.customer = Some(record_id.id);
            } else {
                computer_data.customer = Some(customer_id.clone());
            }
        },
        Err(e) => {
            log::warn!("Error updating Customer {e:?}");
            // Even if customer update failed (e.g., duplicate email),
            // we still need to link the computer to the customer
            computer_data.customer = Some(customer_id.clone());
        }
    }

    if send_specs {
        let create_computer_record: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
            .upsert(computer_id)
            .content(computer_data)
            .await;
        match create_computer_record {
            Ok(record) => info!("schema/utilities.rs -> create_computer_record: {record:?}"),
            Err(e) => return TaskCreationResult::Error { message: format!("Failed to create computer record: {e}") },
        }
    }

    info!("schema/utilities.rs -> ticket record: {ticket_data:?}");
    let service_ticket_record: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
        .upsert(ticket_id)
        .content(ticket_data)
        .await;
    
    match service_ticket_record {
        Ok(record) => info!("schema/utilities.rs -> service_ticket_record: {record:?}"),
        Err(e) => return TaskCreationResult::Error { message: format!("Failed to create service ticket: {e}") },
    }

    // Canonical computer id key equals the connection_string; link the
    // connected_client so the admin console reads customer instead of None.
    if super::entity_link::is_canonical_computer_key(&connection_string) {
        if let Err(e) = super::entity_link::link_connected_client_record(
            &connection_string,
            &customer_id.key_string(),
            None,
        )
        .await
        {
            log::warn!("create_full_task_payload: link_connected_client_record failed (non-fatal): {e:?}");
        }
    }

    info!("schema/utilities.rs -> Task Data: {:?}", &task_data);

    
    let check_task_record: Vec<LiveTaskPayload> = match DATABASE
        .query("SELECT * FROM task WHERE service_number == $service_number")
        .bind(("service_number", service_number.clone()))
        .await
    {
        Ok(mut response) => response.take(0).unwrap_or_default(),
        Err(e) => return TaskCreationResult::Error { message: format!("Failed to check for existing task: {e}") },
    };

    info!("schema/utilities.rs -> check_task_record: {check_task_record:?}");

    let result = if !check_task_record.is_empty() {
        // Task already exists with this service number
        let mut updated = false;
        for task in check_task_record.iter() {
            if task.id == task_data.id {
                // Same task ID - this is an update
                let upsert_result: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
                    .update(task.id.clone())
                    .content(LiveTaskPayload {
                        id: task.id.clone(),
                        ..task_data.clone()
                    }).await;

                match upsert_result {
                    Ok(record) => {
                        for note in task_notes.iter_mut() {
                            if note.task_id == Some(task_data.id.clone()) && note.task_id != Some(task.id.clone()) {
                                note.task_id = Some(task.id.clone());
                            }
                        }
                        info!("schema/utilities.rs -> upsert_task_record: {record:?}");
                        updated = true;
                    },
                    Err(e) => return TaskCreationResult::Error { message: format!("Failed to update task: {e}") },
                }
            }
        }
        
        if updated {
            TaskCreationResult::Updated { service_number: service_number.clone() }
        } else {
            // Task exists but with a different ID - this is a duplicate
            TaskCreationResult::AlreadyExists { service_number: service_number.clone() }
        }
    } else {
        // No existing task - create new one
        let create_result: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
            .create(TASK_TABLE)
            .content(task_data).await;
        match create_result {
            Ok(record) => {
                info!("schema/utilities.rs -> create_task_record: {record:?}");
                TaskCreationResult::Created { service_number: service_number.clone() }
            },
            Err(e) => return TaskCreationResult::Error { message: format!("Failed to create task: {e}") },
        }
    };

    // Process notes regardless of task creation result (notes might already exist, that's fine)
    for mut note in task_notes {
        let res = note.handle_note_creation().await;
        info!("schema/utilities.rs -> Task Note Creation from Mastertech: {res:?}");
    }

    result
}

/// Creates and links customer + canonical computer + service_order without
/// creating or sending a task, so remote diagnostics can resolve
/// customer_id/computer_id without a throwaway temp task.
pub async fn create_and_link_records(
    mut ticket_data: TicketData,
    customer_data: CustomerData,
    mut computer_data: ComputerData,
    connection_string: String,
) -> TaskCreationResult {
    info!("schema/utilities.rs -> create_and_link_records");
    let service_number = ticket_data.service_number.clone();
    if service_number.is_empty() {
        return TaskCreationResult::Error {
            message: "Cannot link records without a service number.".into(),
        };
    }

    let canonical = super::entity_link::canonical_computer_id(&connection_string);
    computer_data.id = canonical.clone();

    let customer_id = customer_data.id.clone();
    info!("schema/utilities.rs -> create_and_link_records cust_record: {customer_data:?}");
    let update_customer: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
        .upsert(customer_id.clone())
        .content(customer_data.clone())
        .await;
    match update_customer {
        Ok(record) => {
            log::info!("Updated Customer {record:?}");
            if let Some(record_id) = record {
                computer_data.customer = Some(record_id.id);
            } else {
                computer_data.customer = Some(customer_id.clone());
            }
        }
        Err(e) => {
            log::warn!("Error updating Customer {e:?}");
            computer_data.customer = Some(customer_id.clone());
        }
    }

    if super::entity_link::computer_has_minimal_hardware(&computer_data) {
        let create_computer_record: std::result::Result<Option<Record>, surrealdb::Error> =
            DATABASE.upsert(canonical.clone()).content(computer_data).await;
        match create_computer_record {
            Ok(record) => info!("schema/utilities.rs -> create_computer_record: {record:?}"),
            Err(e) => {
                return TaskCreationResult::Error {
                    message: format!("Failed to create computer record: {e}"),
                }
            }
        }
    } else {
        let hostname = connection_string
            .split(':')
            .next()
            .unwrap_or(&connection_string)
            .to_string();
        let upsert_computer: std::result::Result<Option<ComputerData>, surrealdb::Error> = DATABASE
            .query("UPSERT $cid SET customer = $cust, hostname = $host")
            .bind(("cid", canonical.clone()))
            .bind(("cust", computer_data.customer.clone()))
            .bind(("host", hostname))
            .await
            .and_then(|mut response| response.take(0));
        match upsert_computer {
            Ok(record) => info!("schema/utilities.rs -> placeholder computer record: {record:?}"),
            Err(e) => {
                return TaskCreationResult::Error {
                    message: format!("Failed to link placeholder computer record: {e}"),
                }
            }
        }
    }

    let existing_service_order: Option<RecordId> = match DATABASE
        .query("SELECT VALUE id FROM service_order WHERE service_number == $service_number LIMIT 1")
        .bind(("service_number", service_number.clone()))
        .await
    {
        Ok(mut response) => response.take(0).unwrap_or_default(),
        Err(e) => {
            warn!("Could not check for existing service_order by number: {e:?}");
            None
        }
    };
    if let Some(existing_id) = existing_service_order {
        ticket_data.id = existing_id;
    }
    ticket_data.customer = Some(customer_id.clone());
    ticket_data.computer = Some(canonical.clone());

    let ticket_id = ticket_data.id.clone();
    info!("schema/utilities.rs -> create_and_link_records ticket record: {ticket_data:?}");
    let service_ticket_record: std::result::Result<Option<Record>, surrealdb::Error> = DATABASE
        .upsert(ticket_id)
        .content(ticket_data)
        .await;
    match service_ticket_record {
        Ok(record) => info!("schema/utilities.rs -> service_ticket_record: {record:?}"),
        Err(e) => {
            return TaskCreationResult::Error {
                message: format!("Failed to create service ticket: {e}"),
            }
        }
    }

    if let Err(e) = super::entity_link::link_connected_client_record(
        &connection_string,
        &customer_id.key_string(),
        None,
    )
    .await
    {
        log::warn!("create_and_link_records: link_connected_client_record failed (non-fatal): {e:?}");
    }

    TaskCreationResult::Created { service_number }
}

/// Performs a cascade duplicate check for all related entities.
/// Checks Task -> ServiceOrder -> Customer + Computer chain.
pub async fn check_for_duplicates(
    service_number: &str,
    new_task: &LiveTaskPayload,
    new_ticket: &TicketData,
    new_customer: &CustomerData,
    new_computer: Option<&ComputerData>,
) -> anyhow::Result<super::DuplicateCheckResult, anyhow::Error> {
    use super::{DuplicateCheckResult, DuplicatePair};
    
    let mut result = DuplicateCheckResult::new(service_number.to_string());
    let service_number_owned = service_number.to_string();
    
    // 1. Check for existing task by service_number
    let existing_tasks: Vec<LiveTaskPayload> = DATABASE
        .query("SELECT * FROM task WHERE service_number == $service_number")
        .bind(("service_number", service_number_owned.clone()))
        .await?
        .take(0)?;
    
    if let Some(existing_task) = existing_tasks.first() {
        result.task = Some(DuplicatePair::new(existing_task.clone(), new_task.clone()));
        
        // 2. If task exists, fetch associated service order
        if let Some(ref ticket_id) = existing_task.service_ticket {
            let existing_ticket: Option<TicketData> = DATABASE
                .select(ticket_id.clone())
                .await?;
            
            if let Some(existing) = existing_ticket {
                // Also fetch the computer linked to this service order if it exists
                if let Some(ref computer_id) = existing.computer {
                    let existing_computer: Option<ComputerData> = DATABASE
                        .select(computer_id.clone())
                        .await?;
                    
                    if let Some(existing_comp) = existing_computer {
                        // Compare with the new computer if provided, otherwise show existing for reference
                        if let Some(new_comp) = new_computer {
                            result.computer = Some(DuplicatePair::new(existing_comp, new_comp.clone()));
                        } else {
                            // No new computer data, but existing has one - show for reference
                            result.computer = Some(DuplicatePair::new(existing_comp.clone(), existing_comp));
                        }
                    }
                }
                result.service_order = Some(DuplicatePair::new(existing, new_ticket.clone()));
            }
        }
    } else {
        // No existing task, but still check for service order by service_number
        let existing_tickets: Vec<TicketData> = DATABASE
            .query("SELECT * FROM service_order WHERE service_number == $service_number")
            .bind(("service_number", service_number_owned.clone()))
            .await?
            .take(0)?;
        
        if let Some(existing) = existing_tickets.first() {
            // Also fetch the computer linked to this service order if it exists
            if let Some(ref computer_id) = existing.computer {
                let existing_computer: Option<ComputerData> = DATABASE
                    .select(computer_id.clone())
                    .await?;
                
                if let Some(existing_comp) = existing_computer {
                    if let Some(new_comp) = new_computer {
                        result.computer = Some(DuplicatePair::new(existing_comp, new_comp.clone()));
                    } else {
                        result.computer = Some(DuplicatePair::new(existing_comp.clone(), existing_comp));
                    }
                }
            }
            result.service_order = Some(DuplicatePair::new(existing.clone(), new_ticket.clone()));
        }
    }
    
    // 3. Check for existing customer by phone, email, or cust_code
    let phone = new_customer.phone_number.clone();
    let email = new_customer.email.clone();
    let cust_code = new_customer.cust_code.clone();
    
    let customer_query = r#"
        SELECT * FROM customer WHERE 
            (phone_number == $phone AND phone_number != "" AND phone_number != "801-334-6262") OR
            (email == $email AND email != "") OR
            (cust_code == $cust_code AND cust_code != "")
        LIMIT 1
    "#;
    
    let existing_customers: Vec<CustomerData> = DATABASE
        .query(customer_query)
        .bind(("phone", phone))
        .bind(("email", email))
        .bind(("cust_code", cust_code))
        .await?
        .take(0)?;
    
    if let Some(existing) = existing_customers.first() {
        result.customer = Some(DuplicatePair::new(existing.clone(), new_customer.clone()));
    }
    
    // 4. Check for existing computer by hostname or serial (if specs are being sent)
    // Only do this if we haven't already found a computer from the service order
    if result.computer.is_none() {
        if let Some(new_comp) = new_computer {
            let hostname = new_comp.hostname.clone();
            let product_serial = new_comp.product_serial.clone();
            let motherboard_serial = new_comp.motherboard_serial.clone();
            
            // If we found a customer match, first try to find a computer belonging to THAT customer
            // This prevents matching computers from unrelated customers that happen to share 
            // common values like hostname="Owner-PC" or motherboard_serial="Standard"
            let existing_computer = if let Some(ref customer_dup) = result.customer {
                // First, try to find a matching computer owned by this specific customer
                let customer_id = customer_dup.existing.id.clone();
                let customer_scoped_query = r#"
                    SELECT * FROM computer WHERE 
                        customer == $customer_id AND (
                            (hostname == $hostname AND hostname != "") OR
                            (product_serial == $product_serial AND product_serial != "") OR
                            (motherboard_serial == $motherboard_serial AND motherboard_serial != "" AND motherboard_serial != "Standard")
                        )
                    LIMIT 1
                "#;
                
                let scoped_computers: Vec<ComputerData> = DATABASE
                    .query(customer_scoped_query)
                    .bind(("customer_id", customer_id))
                    .bind(("hostname", hostname.clone()))
                    .bind(("product_serial", product_serial.clone()))
                    .bind(("motherboard_serial", motherboard_serial.clone()))
                    .await?
                    .take(0)?;
                
                scoped_computers.first().cloned()
            } else {
                // No customer match found - only look for computers with highly unique identifiers
                // (product_serial or motherboard_serial, but NOT common values like "Standard")
                // Avoid hostname-only matches as they're not unique enough without customer context
                let strict_computer_query = r#"
                    SELECT * FROM computer WHERE 
                        (product_serial == $product_serial AND product_serial != "" AND product_serial != "System Serial Number" AND product_serial != "Default string") OR
                        (motherboard_serial == $motherboard_serial AND motherboard_serial != "" AND motherboard_serial != "Standard" AND motherboard_serial != "Default string")
                    LIMIT 1
                "#;
                
                let strict_computers: Vec<ComputerData> = DATABASE
                    .query(strict_computer_query)
                    .bind(("product_serial", product_serial.clone()))
                    .bind(("motherboard_serial", motherboard_serial.clone()))
                    .await?
                    .take(0)?;
                
                strict_computers.first().cloned()
            };
        
            if let Some(existing) = existing_computer {
                result.computer = Some(DuplicatePair::new(existing, new_comp.clone()));
            }
        }
    }
    
    info!("Duplicate check result for service #{}: has_conflicts={}, has_any_duplicates={}", 
        service_number, result.has_conflicts(), result.has_any_duplicates());
    
    Ok(result)
}

/// Creates task payload with resolved duplicates.
/// Call this after the user has resolved any duplicate conflicts.
pub async fn create_resolved_task_payload(
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    task_data: LiveTaskPayload,
    task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
) -> TaskCreationResult {
    // This is a simplified version that just creates/updates without duplicate checking
    // The duplicate checking should happen before this is called
    create_full_task_payload(
        ticket_data,
        customer_data,
        computer_data,
        task_data,
        task_notes,
        send_specs,
        false,
        None,
    )
    .await
}

impl PrestashopPayload {}
/* 
impl Customer {
    pub async fn find_customer_by_email(email: &str) -> anyhow::Result<Self, anyhow::Error> {
        let api_call = Prestashop::default();
        let mut query = HashMap::new();
        let tmp_customer = &mut Customer::default();
        query.insert("filter[email]", email);
        query.insert("output_format", "JSON");

        let customers: Vec<Customer> = api_call
            .request_resources_wasm("customers",  query)
            .await?;

        if let Some(customer) = customers.get(0) {
            *tmp_customer = customer.clone();
        }

        Ok(tmp_customer.clone())
    }

    pub async fn find_customer_address(mut self) -> anyhow::Result<Address, anyhow::Error> {
        let api_call = Prestashop::default();
        let mut tmp_address = Address::default();

        let address: Address = api_call
            .request_subresources_by_id_wasm("customers", self.)
            .await?;

        Ok(CustomerData { 
            id: RecordId::new(
                CUSTOMER_TABLE,
                id_customer,
            ),
            cust_code: id_customer.to_string(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            phone_number: tmp_address.phone.clone().to_string(),
            email: cust.email,
            phone_number_2: tmp_address.phone_mobile.clone().to_string(),
            ..Default::default()
        })
    }
}
 */

impl Customer {
    pub async fn find_customer_by_email(email: &str) -> anyhow::Result<Vec<(Customer, Address)>, anyhow::Error> {
        let api_call = Prestashop::default();
        let mut query = HashMap::new();
        let customers = &mut vec![];
        query.insert("filter[email]", email);
        query.insert("output_format", "JSON");

        let possible_customers: Vec<Customer> = api_call
            .request_resources_wasm("customers", query.clone())
            .await?;

        for cust in possible_customers.iter() {
            if !cust.id.is_empty() {
                let mut query = HashMap::new();
                query.insert("filter[id_customer]", cust.id.as_str());
                query.insert("output_format", "JSON");

                let addresses: Vec<Address> = api_call
                    .request_resources_wasm("addresses", query.clone())
                    .await?;

                for addr in addresses.iter() {
                    if addr.id_customer == cust.id {
                        customers.push((cust.clone(), addr.clone()));
                    }
                }
            }
        }

        Ok(customers.clone())
    }

    pub async fn find_customer_by_phone(phone: &str) -> anyhow::Result<Vec<(Customer, Address)>, anyhow::Error> {
        let api_call = Prestashop::default();

        let customers = &mut vec![];

        for phone_number in format_us_phone_number(phone).iter() {
            let mut query = HashMap::new();
            query.insert("filter[phone]", phone_number.as_str());
            query.insert("output_format", "JSON");
        
            let customer_addresses: Vec<Address> = api_call
                .request_resources_wasm("addresses", query.clone())
                .await?;

            log::info!("Addresses: {customer_addresses:#?}");

            for addr in customer_addresses.iter() {
                if !addr.id_customer.is_empty() {
                    let cust: Customer = api_call
                        .request_subresources_by_id_wasm("customers", "customer", &addr.id_customer)
                        .await?;

                    customers.push((cust.clone(), addr.clone()));
                }

            }
        }

        Ok(customers.clone())
    }
}

pub fn format_us_phone_number(phone: &str) -> Vec<String> {
    // Remove all non-numeric characters
    let re = Regex::new(r"\D").unwrap();
    let digits: String = re.replace_all(phone, "").to_string();

    // Ensure it's a valid 10-digit number
    if digits.len() != 10 {
        return vec![]; // Return empty vector if not valid
    }

    // Extract parts
    let area_code = &digits[0..3];
    let prefix = &digits[3..6];
    let line_number = &digits[6..10];

    // Create different formats
    vec![
        format!("{}-{}-{}", area_code, prefix, line_number),
        format!("({}){}-{}", area_code, prefix, line_number),
        format!("{}", digits),
        format!("({}) {}-{}", area_code, prefix, line_number),
    ]
}

pub async fn get_prestashop_payload_from_phone(phone: &str) -> anyhow::Result<PrestashopPayload, anyhow::Error> {
    let api_call = Prestashop::default();

    let mut tmp_address = Address::default();
    let mut potential_order = Order::default();
    for phone_number in format_us_phone_number(phone).iter() {
        let mut query = HashMap::new();
        query.insert("filter[phone]", phone_number.as_str());
        query.insert("output_format", "JSON");
    
        let customer_addresses: Vec<Address> = api_call
            .request_resources_wasm("addresses", query.clone())
            .await?;

        log::info!("Addresses: {customer_addresses:#?}");

        if let Some(address) = customer_addresses.get(0) {
            tmp_address = address.clone();
            break;
        }
    }

    if tmp_address == Default::default() {
        return Err(
            anyhow::anyhow!("Could not find customer info from phone number")
        );
    }

    let mut query = HashMap::new();
    query.insert("filter[id_customer]", tmp_address.id_customer.as_str());
    query.insert("sort", "[id_DESC]");
    query.insert("output_format", "JSON");

    let orders: Vec<Order> = api_call
        .request_resources_wasm("orders", query.clone())
        .await?;

    if let Some(order) = orders.get(0) {
        potential_order = order.clone();
    }
    
    if potential_order == Default::default() {
        return Err(
            anyhow::anyhow!("Could not find order from customer ID")
        );
    }

    let mut query = HashMap::new();

    query.insert("filter[id_order]", potential_order.id.as_str());
    query.insert("output_format", "JSON");

    let customer_threads: Vec<CustomerThread> = api_call
        .request_resources_wasm("customer_threads", query.clone())
        .await?;

    let mut customer_messages: Vec<CustomerMessage> = Vec::new();
    let mut task_notes: Vec<TaskNotePayload> = Vec::new();

    if !customer_threads.is_empty() {
        for thread in customer_threads.iter() {
            for msg in thread.associations.customer_messages.iter() {
                let msg: CustomerMessage =  api_call
                    .request_subresources_by_id_wasm(
                        "customer_messages",
                        "customer_message",
                        msg.id.as_str(),
                    )
                    .await?;

                match msg.into_task_note(potential_order.id.as_str()).await {
                    Ok(task_note) => task_notes.push(task_note),
                    Err(e) => log::error!("Error converting cust msg into task note: {e:?}"),
                }
                customer_messages.push(msg)
            }
        }
    }

    if potential_order.id_customer.is_empty() {
        info!("schema/utilities.rs -> Order is likely gonna fuKKKK");
    }

    info!("schema/utilities.rs -> order: {potential_order:#?}");

    let sales_rep: Option<Employee> = if !potential_order.id_employee_sales_rep.eq("0") {
        api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &potential_order.id_employee_sales_rep,
            )
            .await
            .ok()
    } else {
        None
    };

    let split_rep: Option<Employee> = if !potential_order.id_employee_split_rep.eq("0") {
        api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &potential_order.id_employee_split_rep,
            )
            .await
            .ok()
    } else {
        None
    };

    let cust: Customer = api_call
        .request_subresources_by_id_wasm("customers", "customer", &tmp_address.id_customer)
        .await?;


    info!("schema/utilities.rs -> address: {tmp_address:#?}");

    let customer = CustomerData {
        id: RecordId::new(
            CUSTOMER_TABLE,
            potential_order.id_customer.clone(),
        ),
        cust_code: potential_order.id_customer.clone(),
        name: format!("{} {}", &cust.firstname, &cust.lastname),
        phone_number: tmp_address.phone.clone().to_string(),
        email: cust.email,
        ..Default::default()
    };

    Ok( 
        PrestashopPayload {
            customer,
            order: potential_order,
            sales_rep,
            split_rep,
            address: tmp_address,
            customer_threads,
            customer_messages,
            task_notes
        }
    )
}

pub async fn get_prestashop_payload(order_number: &str) -> anyhow::Result<PrestashopPayload, anyhow::Error> {
    let api_call = Prestashop::default();
    let mut query = HashMap::new();
    let customer_address = &mut Address::default();
    query.insert("filter[id_order]", order_number);
    query.insert("output_format", "JSON");

    let customer_threads: Vec<CustomerThread> = api_call
        .request_resources_wasm("customer_threads", query.clone())
        .await?;

    log::info!("CUSTOMER THREADS LEN: {}", customer_threads.len());
    
    let mut customer_messages: Vec<CustomerMessage> = Vec::new();
    let mut task_notes: Vec<TaskNotePayload> = Vec::new();

    if !customer_threads.is_empty() {
        for thread in customer_threads.iter() {
            if &thread.id != ""  && !thread.associations.customer_messages.is_empty() {
                for msg in thread.associations.customer_messages.iter() {
                    log::info!("Pulling Customer messages");
                    let msg: CustomerMessage =  api_call
                        .request_subresources_by_id_wasm(
                            "customer_messages",
                            "customer_message",
                            msg.id.as_str(),
                        )
                        .await?;

                    match msg.into_task_note(order_number).await {
                        Ok(task_note) => task_notes.push(task_note),
                        Err(e) => log::error!("Error converting cust msg into task note: {e:?}"),
                    }

                    customer_messages.push(msg)
                }
            }
        }
    }

    log::info!("Pulling order: {order_number}");

    let order: Order = api_call
        .request_subresources_by_id_wasm("orders", "order", order_number)
        .await?;

    log::info!("Pulled order");

    if order.id_customer.is_empty() {
        info!("schema/utilities.rs -> Order is likely gonna fuKKKK");
    }

    info!("schema/utilities.rs -> order: {order:#?}");

    // let user = &mut User::default();

    // This is the checkin shelf 'employee'
    // if order.id_employee_sales_rep.eq("1347") {
    //     return Err(anyhow::anyhow!(""));
    // }

    let sales_rep: Option<Employee> = if !order.id_employee_sales_rep.eq("0") && !order.id_employee_sales_rep.eq("1347") {
        api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &order.id_employee_sales_rep,
            )
            .await
            .ok()
    } else {
        None
    };

    let split_rep: Option<Employee> = if !order.id_employee_split_rep.eq("0") {
        api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &order.id_employee_split_rep,
            )
            .await
            .ok()
    } else {
        None
    };

    let cust: Customer = api_call
        .request_subresources_by_id_wasm("customers", "customer", &order.id_customer)
        .await?;

    // let address: Address = api_call
    //     .request_subresources_by_id_wasm("addresses", "address", &order.id_address_invoice)
    //     .await?;

    let mut query = HashMap::new();
    query.insert("filter[id_customer]", order.id_customer.as_str());
    query.insert("output_format", "JSON");

    let addresses: Vec<Address> = api_call
        .request_resources_wasm("addresses", query.clone())
        .await?;

    for address in addresses.iter() {
        *customer_address = address.clone();
    }

    if order.id_address_invoice == order.id_address_delivery {
        log::error!(
            "ADDRESS MISMATCH, order.id_address_invoice: {} == data.order.id_address_delivery: {}\nUpdating {} to {}", 
            order.id_address_invoice,
            order.id_address_delivery,
            order.id_address_invoice,
            customer_address.id
        );

        let order_id = order.id.clone();
        let id_addr = customer_address.id.clone();

        let api = Prestashop::default();
        match api.request_raw_resource_by_id("orders", &order_id).await {
            Ok(xml) => {
                match modify_xml(&xml, "id_address_invoice", &id_addr) {
                    Ok(new_xml) => {
                        log::debug!("NEW XML: {new_xml:#?}");
                        match remove_xml_tag(&new_xml, "tax_exempt") {
                            Ok(final_xml) => {
                                log::debug!("Final XML: {final_xml:#?}");
                                match api.modify_prestashop_order(&final_xml).await {
                                    Ok(prestashop_response) => log::debug!("Prestashop Response XML: {prestashop_response:#?}"),
                                    Err(e) => log::error!("Error modifying prestashop order: {e:?}"),
                                }
                            },
                            Err(e) => log::error!("Error removing tax_exempt tag from XML: {e:?}"),
                        }
                    }
                    Err(e) => log::error!("Error modifying XML: {e:?}")
                }
            },
            Err(e) => log::error!("Error getting XML order: {e:?}"),
        }
    }

    info!("schema/utilities.rs -> address: {customer_address:#?}");

    let customer = CustomerData {
        id: RecordId::new(
            CUSTOMER_TABLE,
            order.id_customer.clone(),
        ),
        cust_code: order.id_customer.clone(),
        name: format!("{} {}", &cust.firstname, &cust.lastname),
        phone_number: customer_address.phone.clone().to_string(),
        email: cust.email,
        phone_number_2: customer_address.phone_mobile.clone().to_string(),
        ..Default::default()
    };

    let address = customer_address.clone();
    Ok( 
        PrestashopPayload {
            customer,
            order,
            sales_rep,
            split_rep,
            address,
            customer_threads,
            customer_messages,
            task_notes,
        }
    )
}

/// Returns a vector of missing call days (formatted as "YYYY-MM-DD")
/// for days (between the day after check‑in and today, skipping Sundays)
/// that have no corresponding customer message.
pub fn get_missing_call_days(order_date_str: &str, customer_messages: &[CustomerMessage]) -> Vec<String> {
    // Parse the order's date_add. The format is "2025-04-04 16:48:01"
    let order_date = match NaiveDateTime::parse_from_str(order_date_str, "%Y-%m-%d %H:%M:%S") {
        Ok(dt) => dt.date(),
        Err(e) => {
            log::error!("Failed to parse order date {}: {}", order_date_str, e);
            return Vec::new();
        }
    };

    // Determine the current local date.
    let today: NaiveDate = Local::now().naive_local().date();
    log::info!("Order date: {}", order_date);
    log::info!("Today: {}", today);
    
    // Log all customer message dates for debugging.
    let msg_dates: Vec<String> = customer_messages
        .iter()
        .map(|msg| msg.date_add.clone())
        .collect();
    log::info!("Customer messages received: {:?}", msg_dates);

    let mut missing_days = Vec::new();
    let mut day = match order_date.succ_opt() {
        Some(d) => d,
        None => {
            log::error!("Failed to get successor for order date: {}", order_date);
            return missing_days;
        }
    };

    // Iterate until including today.
    while day <= today {
        log::info!("Checking day: {}", day.format("%Y-%m-%d"));
        // Skip Sundays.
        if day.weekday() == Weekday::Sun {
            log::info!("Skipping Sunday: {}", day.format("%Y-%m-%d"));
            day = match day.succ_opt() {
                Some(d) => d,
                None => {
                    log::error!("Failed to get successor for day: {}", day);
                    break;
                }
            };
            continue;
        }
        
        // Check if there's a customer message on this day.
        let mut called = false;
        for msg in customer_messages {
            match NaiveDateTime::parse_from_str(&msg.date_add, "%Y-%m-%d %H:%M:%S") {
                Ok(msg_dt) => {
                    log::info!("  Comparing {} with customer message date {}",
                             day.format("%Y-%m-%d"), msg_dt.date().format("%Y-%m-%d"));
                    if msg_dt.date() == day {
                        log::info!("  Found matching call for day {}", day.format("%Y-%m-%d"));
                        called = true;
                        break;
                    }
                },
                Err(e) => log::error!("  Failed to parse customer message date {}: {}", msg.date_add, e),
            }
        }
        
        if !called {
            log::info!("No call found for day {}", day.format("%Y-%m-%d"));
            missing_days.push(day.format("%Y-%m-%d").to_string());
        }
        
        day = match day.succ_opt() {
            Some(d) => d,
            None => {
                log::error!("Failed to get successor for day: {}", day);
                break;
            }
        };
    }
    log::info!("Missing days: {:?}", missing_days);
    missing_days
}

/// True if an order needs a call today: it was checked in on a previous day
/// (not today) and has no customer message dated today.
pub fn needs_call_today(order_date_str: &str, customer_messages: &[CustomerMessage]) -> bool {
    let today: NaiveDate = Local::now().naive_local().date();

    let order_date = match NaiveDateTime::parse_from_str(order_date_str, "%Y-%m-%d %H:%M:%S") {
        Ok(dt) => dt.date(),
        Err(e) => {
            log::error!("needs_call_today: failed to parse order date {order_date_str}: {e}");
            return false;
        }
    };

    // Checked in today -> no call needed yet.
    if order_date >= today {
        return false;
    }

    let called_today = customer_messages.iter().any(|msg| {
        NaiveDateTime::parse_from_str(&msg.date_add, "%Y-%m-%d %H:%M:%S")
            .map(|dt| dt.date() == today)
            .unwrap_or(false)
    });

    !called_today
}


#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime, Weekday};

    // Define a dummy version of your CustomerMessage for testing purposes.
    #[derive(Debug)]
    struct DummyCustomerMessage {
        pub date_add: String,
    }

    impl DummyCustomerMessage {
        fn new(s: &str) -> Self {
            Self {
                date_add: s.to_string(),
            }
        }
    }

    // A helper function that mimics `get_missing_call_days` but allows you to specify what “today” is.
    fn get_missing_call_days_with_today(
        order_date_str: &str,
        customer_messages: &[DummyCustomerMessage],
        today: NaiveDate,
    ) -> Vec<String> {
        // Parse the order's date_add (format: "YYYY-MM-DD HH:MM:SS")
        let order_date = NaiveDateTime::parse_from_str(order_date_str, "%Y-%m-%d %H:%M:%S")
            .expect("Valid order date")
            .date();

        let mut missing_days = Vec::new();
        let mut day = order_date.succ_opt().expect("Order date should have a successor");

        // Iterate from the day after check‑in until including the specified `today`.
        while day <= today {
            // Skip Sundays.
            if day.weekday() == Weekday::Sun {
                day = day.succ_opt().expect("Day should have a successor");
                continue;
            }

            let mut called = false;
            for msg in customer_messages {
                // Parse the customer message date.
                let msg_dt = NaiveDateTime::parse_from_str(&msg.date_add, "%Y-%m-%d %H:%M:%S")
                    .expect("Valid message date")
                    .date();
                log::info!(
                    "Comparing day {} with customer message date {}",
                    day.format("%Y-%m-%d"),
                    msg_dt.format("%Y-%m-%d")
                );
                if msg_dt == day {
                    log::info!("Found call on {}", day.format("%Y-%m-%d"));
                    called = true;
                    break;
                }
            }

            if !called {
                log::info!("No call found on {}", day.format("%Y-%m-%d"));
                missing_days.push(day.format("%Y-%m-%d").to_string());
            }
            day = day.succ_opt().expect("Day should have a successor");
        }
        missing_days
    }

    #[test]
    fn test_get_missing_call_days_custom() {
        // Order check-in date: April 1, 2025 16:00:58 (Tuesday).
        let order_date_str = "2025-04-01 16:00:58";
        // Dummy customer messages: one call on April 03 and one call on April 04.
        let customer_messages = vec![
            DummyCustomerMessage::new("2025-04-03 10:00:00"),
            DummyCustomerMessage::new("2025-04-04 11:00:00"),
        ];

        // Let’s fix "today" as April 5, 2025.
        let today = NaiveDate::from_ymd_opt(2025, 4, 5).expect("Valid date");

        let missing = get_missing_call_days_with_today(order_date_str, &customer_messages, today);
        // Expected missing days:
        // - April 2 (Wednesday) is missing a call.
        // - April 3 and 4 have calls.
        // - April 5 (Saturday) is missing a call.
        let expected = vec!["2025-04-02".to_string(), "2025-04-05".to_string()];
        assert_eq!(missing, expected);
    }
}


#[derive(Serialize)]
#[allow(dead_code)]
pub struct PhoneNumberFormatter {
    pub cache: HashMap<String, String>,
    #[serde(skip)]
    pub re_digits: Regex,
    #[serde(skip)]
    pub re_dashes: Regex,
}

impl Default for PhoneNumberFormatter {
    fn default() -> Self {
        Self {
            cache: HashMap::new(),
            re_digits: Regex::new(r"^(\d{3})(\d{3})(\d{4})$").unwrap(),
            re_dashes: Regex::new(r"^(\d{3})-(\d{3})-(\d{4})$").unwrap(),
        }
    }
}

#[allow(dead_code)]
impl PhoneNumberFormatter {
    pub fn format_phone_number(&mut self, phone: &str) -> Option<String> {
        if let Some(cached) = self.cache.get(phone) {
            return Some(cached.clone());
        }

        let formatted = if let Some(caps) = self.re_digits.captures(phone) {
            Some(format!("({}) {}-{}", &caps[1], &caps[2], &caps[3]))
        } else if let Some(caps) = self.re_dashes.captures(phone) {
            Some(format!("({}) {}-{}", &caps[1], &caps[2], &caps[3]))
        } else {
            Some(phone.to_string())
        };

        if let Some(ref result) = formatted {
            self.cache.insert(phone.to_string(), result.clone());
        }

        formatted
    }
}

/// Compress data using Brotli.
pub fn compress_data(input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut compressed = Vec::new();
    {
        // Create a Brotli compressor with a buffer size of 4096, quality 11 and lgwin 22.
        let mut compressor = brotli::CompressorReader::new(input, 4096, 11, 22);
        std::io::copy(&mut compressor, &mut compressed)?;
    }
    Ok(compressed)
}

/// Decompress data using Brotli.
pub fn decompress_data(input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut decompressed = Vec::new();
    let mut decompressor = brotli::Decompressor::new(input, 4096);
    std::io::copy(&mut decompressor, &mut decompressed)?;
    Ok(decompressed)
}