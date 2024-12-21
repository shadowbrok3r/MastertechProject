use super::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, LiveTaskPayload, Notification, TicketData, TicketPayload};
use crate::{
    schema::{
        helper_traits::TaskNotePayloadHelper, prestashop_schema::{Address, Customer, CustomerMessage, CustomerThread, Employee, Order, Prestashop}, Cmd, ConnectedClient, Priority, Record, Status, Store, SystemInformation, TaskNotePayload, TaskPayload, User, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE
    },
    DATABASE,
};
use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use crossbeam::channel::Sender;
use log::{debug, info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};
use surrealdb::RecordId;
use web_time::Instant;

pub trait FilterTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: &String) -> Vec<TaskPayload>;
    fn filter_by_my_store(&self, assignees: &Vec<User>, current_user: &User) -> Vec<TaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `name`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<TaskPayload>;
}

pub trait Sortable {
    fn sort_task_payloads(&mut self) -> &mut Vec<TaskPayload>;
}

// pub trait LiveUpdate{
//     fn handle_live_create<T: StructDiff + PartialEq>(self, existing_tasks: &mut Vec<T>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
//     fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
//     fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
// }

pub trait LiveUpdate {
    fn handle_live_create(
        self,
        existing_tasks: &mut Vec<TaskPayload>,
        new_ticket: Option<TicketPayload>,
    ) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_update(
        self,
        existing_tasks: &mut Vec<TaskPayload>,
        new_ticket: Option<TicketPayload>,
    ) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_delete(
        self,
        existing_tasks: &mut Vec<TaskPayload>,
        new_ticket: Option<TicketPayload>,
    ) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
}

#[async_trait]
pub trait Task {
    // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
}

pub async fn query_user_from_email(email: String) -> Result<User, Error> {
    let query = if email.eq("checkinshelf") || email.is_empty() {
        "RETURN (SELECT * FROM user WHERE id == $auth.id)"
    } else { "SELECT * FROM user WHERE email == $email" };

    if email.contains("@pclaptops.com") {
        DATABASE.set("email", email.clone()).await?;
    } else {
        DATABASE
            .set("email", format!("{}@pclaptops.com", email.clone()))
            .await?;
    }

    info!("schema/utilities.rs -> Email: {}", email);
    let user: Option<User> = DATABASE.query(query).await?.take(0)?;
    // let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
    user.clone().context("No User Found") // Ok(user.unwrap())
}

pub async fn get_task_notes_from_db_with_service_number(service_number: String) -> Result<Vec<TaskNotePayload>, Error> {
    debug!("get_task_from_service_number");
    let query_results: Vec<TaskNotePayload> = DATABASE
        .query("SELECT * FROM task_note WHERE task_id.service_number == $service_number PARALLEL")
        .bind(("service_number", service_number.clone()))
        .await?
        .take(0)?;

    if query_results.is_empty() {
        let alt_query: Vec<TaskNotePayload> = DATABASE
            .query("SELECT * FROM task_note WHERE service_number == $service_number PARALLEL")
            .bind(("service_number", service_number))
            .await?
            .take(0)?;
        info!("schema/utilities.rs -> get_task_notes_from_service_number: {alt_query:?}");
        Ok(alt_query)
    } else {
        info!("schema/utilities.rs -> get_task_notes_from_service_number: {query_results:?}");
        Ok(query_results)
    }
}

pub async fn query_id<T>(_table: String, id: RecordId) -> Result<Option<T>, Error>
where
    T: Serialize + Debug + Clone + 'static + for <'de> Deserialize <'de>
{
    let mut record: surrealdb::Response = DATABASE
        .query("SELECT * FROM <record>$id")
        .bind(("id", id.clone()))
        // .bind(("table", table.clone()))
        .await?;

    info!("schema/utilities.rs/query_id -> Record: {:?}\nSELECT * FROM ${id:?}", record);
    Ok(record.take::<Option<T>>(0)?)
}

pub async fn check_id_existence<T>(table: String, id: T) -> Result<Option<bool>, Error>
where
    T: Serialize + Debug + Clone + 'static,
{
    let query = format!(
        r#"
        LET $query = (SELECT $id FROM $table);
        IF $query != NULL || NONE {{ true }} ELSE {{ false }};
    "#
    );
    DATABASE.set("id", id).await?;
    DATABASE.set("table", table).await?;
    let record: Option<bool> = DATABASE.query(query.clone()).await?.take(1)?;
    info!("schema/utilities.rs -> Query: {:?}  // {}", record, query);
    Ok(record)
}

pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    bincode::serialize(system_info).expect("Failed to serialize SystemInformation")
}

pub fn _deserialize_system_info(bytes: &[u8]) -> SystemInformation {
    bincode::deserialize(bytes).expect("Failed to deserialize SystemInformation")
}

pub fn deserialize_command(bytes: &[u8]) -> Cmd {
    bincode::deserialize(bytes).expect("Failed to deserialize Cmd")
}

pub async fn get_tasks(tx: Sender<Vec<TaskPayload>>) -> Result<(), Error> {
    debug!("get_tasks");
    let query = r#"
        SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task
        
        WHERE $this.assignee.store == $auth.store 
        
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
        PARALLEL
    "#; // ORDER BY due_date ASC WITH INDEX idx_store_due_date
    let query_results: Vec<TaskPayload> = DATABASE.query(query).await?.take(0)?;
    tx.try_send(query_results)?;
    Ok(())
}

pub async fn get_tasks_for_store(tx: Sender<Vec<TaskPayload>>, store: String) -> Result<(), Error> {
    debug!("get_tasks");

    let query = r#"
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
        PARALLEL
    "#; // WITH INDEX idx_store_due_date

    let start_query = Instant::now(); // Start timing the query

    let query_results: Vec<TaskPayload> = DATABASE
        .query(query)
        .bind(("store", store.clone()))
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed(); // Measure query duration
    warn!("Query execution time for chunk {query_duration:?}");

    tx.try_send(query_results)?;

    Ok(())
}

pub async fn get_completed_tasks_for_store(tx: Sender<Vec<TaskPayload>>, store: String) -> Result<(), Error> {
    debug!("get_completed_tasks");
    let query = r#"
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
        PARALLEL
    "#; // PARALLEL
    
    let start_query = Instant::now(); // Start timing the query

    let query_results: Vec<TaskPayload> = DATABASE
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
        .query("SELECT * FROM user WHERE store == $store PARALLEL")
        .bind(("store", store))
        .await?
        .take(0)?;
    tx.try_send(data)?;
    Ok(())
}

pub async fn get_connected_clients(tx: Sender<Vec<ConnectedClient>>) -> Result<(), Error> {
    debug!("get_connected_clients");
    let query: Vec<ConnectedClient> = DATABASE
        .query("SELECT * FROM connected_client WHERE assigned_user == $auth.id PARALLEL")
        .await?
        .take(0)?;
    info!("Clients: {:?}", query);
    tx.try_send(query)?;
    Ok(())
}

pub async fn disconnect_client(tx: Sender<Vec<RecordId>>, id: RecordId) -> Result<(), Error> {
    let query: Vec<RecordId> = DATABASE
        .query("UPDATE connected_client SET connected = false WHERE id == $id")
        .bind(("id", id.key().to_string()))
        .await?
        .take(0)?;
    tx.try_send(query)?;

    Ok(())
}

pub async fn modify_connected_client(tx: Sender<Vec<ConnectedClient>>) -> Result<(), Error> {
    let query: Vec<ConnectedClient> = DATABASE
        .query("SELECT * FROM connected_client WHERE assigned_user == $auth.id")
        .await?
        .take(0)?;
    tx.try_send(query)?;
    Ok(())
}

pub async fn delete_task(id: RecordId) -> Result<(), Error> {
    info!("schema/utilities.rs -> deleting id: {id:?}");
    let x = id.clone();
    let delete_result: Option<Record> = DATABASE.delete(
        (TASK_TABLE, id.key().to_string())
    )
    .await
    .unwrap();

    info!("schema/utilities.rs -> delete_result: {delete_result:?} for {:?}", x.key().to_string());
    
    Ok(())
}

pub async fn get_notifications(tx: Sender<Vec<Notification>>) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_notifications");
    let notifications: Vec<Notification> = DATABASE
        .query("SELECT * FROM notification WHERE user == $auth.id PARALLEL")
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
pub trait TaskNoteMod {
    async fn delete_note(&mut self) -> Result<(), Error>;
}

#[async_trait]
impl TaskNoteMod for TaskNotePayload {
    async fn delete_note(&mut self) -> Result<(), Error> {
        let id = self.id.clone();
        info!("schema/utilities.rs -> deleting id: {:?}", id.clone());
        DATABASE.set("id", id.key().to_string().clone()).await?;
        let y: Option<Record> = DATABASE
            .delete((TASK_NOTE_TABLE, id.key().to_string()))
            .await?;
        info!("schema/utilities.rs -> Deleted note: {:?}", y);
        Ok(())
    }
}

pub async fn update_task_notes(new_msg: String, task_id: RecordId) -> Result<(), Error> {
    let id = task_id.clone();
    let task_note = TaskNotePayload {
        task_id: Some(id),
        note: new_msg,
        ..Default::default()
    };

    let query = format!("CREATE task_note CONTENT $note");
    DATABASE.set("note", task_note).await.unwrap();
    let update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;

    info!("schema/utilities.rs -> Updated notes: {update_task:?}");
    Ok(())
}

#[async_trait]
pub trait NotificationMod {
    async fn delete_notification(&mut self) -> Result<(), Error>;
    async fn mark_notification(&mut self) -> Result<(), Error>;
}

#[async_trait]
impl NotificationMod for Notification {
    async fn delete_notification(&mut self) -> Result<(), Error> {
        let query: Option<Record> = DATABASE
            .delete(("notification", self.id.key().to_string()))
            .await?;
        info!("schema/utilities.rs -> Deleted notification: {query:?}");
        Ok(())
    }

    async fn mark_notification(&mut self) -> Result<(), Error> {
        DATABASE.set("id", self.id.clone()).await?;
        let query: Option<Record> = DATABASE
            .query("UPDATE notification SET status = 'Read' WHERE id == $id")
            .await?
            .take(0)?;
        info!("schema/utilities.rs -> Updated notification: {query:?}");
        Ok(())
    }
}


pub async fn create_full_task_payload(
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
) -> anyhow::Result<(), anyhow::Error> {
    info!("schema/utilities.rs -> Send_Payload");
    let queried_salesman = query_user_from_email(ticket_data.salesman.clone()).await?;
    let _queried_tech = query_user_from_email(ticket_data.tech.clone()).await?;

    // let task_id = task_data.id.clone();
    let ticket_id = ticket_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();

    task_data.task_name = format!(
        "{} - {}",
        &customer_data.name,
        ticket_data.service_number.clone()
    );
    task_data.service_ticket = Some(ticket_id.clone());
    task_data.service_number = Some(ticket_data.service_number.clone());
    task_data.priority = Priority::Normal;
    task_data.everest_initials = queried_salesman.everest_initials;
    task_data.assignee = queried_salesman.id;

    if let Some(cust_record) = query_id::<CustomerData>(CUSTOMER_TABLE.to_string(), customer_id.clone()).await? {
        info!("schema/utilities.rs -> cust_record: {cust_record:?}");
        let update_cust_record: Option<Record> = DATABASE
            .update(customer_id)
            .content(customer_data.clone())
            .await?;
        info!("schema/utilities.rs -> Customer updated: {update_cust_record:?}");

        if let Some(computer_record) = query_id::<ComputerData>(COMPUTER_TABLE.to_string(), computer_id.clone()).await? {
            info!("schema/utilities.rs -> computer_record: {computer_record:?}");
            if send_specs {
                let create_computer_record: Option<Record> = DATABASE
                    .update(computer_id)
                    .content(computer_data)
                    .await?;
                info!("schema/utilities.rs -> create_computer_record: {create_computer_record:?}");
            }
        } else {
            let create_computer_record: Option<RecordId> = DATABASE
                .create(COMPUTER_TABLE)
                .content(computer_data)
                .await?;
            info!("schema/utilities.rs -> create_computer_record: {create_computer_record:?}");
        }
        if let Some(ticket) = query_id::<TicketData>(TICKET_TABLE.to_string(), ticket_id.clone()).await? {
            info!("schema/utilities.rs -> ticket record: {ticket:?}");
            let service_ticket_record: Option<Record> = DATABASE
                .update(ticket_id)
                .content(ticket_data)
                .await?;
            info!("schema/utilities.rs -> service_ticket_record: {service_ticket_record:?}");
        } else {
            let service_ticket_record: Option<RecordId> =
                DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
            info!("schema/utilities.rs -> service_ticket_record: {service_ticket_record:?}");
        }
    } else {
        match DATABASE
            .create::<Option<Record>>(CUSTOMER_TABLE)
            .content(customer_data.clone())
            .await
        {
            Ok(create_cust_record) => info!("schema/utilities.rs -> Created Record: {create_cust_record:?}"),
            Err(e) => log::error!("Error with create_cust_record: {e:?}"),
        }
        if send_specs {
            match DATABASE
                .create::<Option<Record>>(COMPUTER_TABLE)
                .content(computer_data)
                .await
            {
                Ok(create_computer_record) => info!("schema/utilities.rs -> Created Record: {create_computer_record:?}"),
                Err(e) => log::error!("Error with create_computer_record: {e:?}"),
            }
        }
        match DATABASE
            .create::<Option<Record>>(TICKET_TABLE)
            .content(ticket_data)
            .await
        {
            Ok(create_ticket_record) => info!("schema/utilities.rs -> Created Record: {create_ticket_record:?}"),
            Err(e) => log::error!("Error with create_ticket_record: {e:?}"),
        }
    }

    info!("schema/utilities.rs -> Task Data: {:?}", &task_data);

    let create_task_record: Option<Record> = DATABASE
        .create(TASK_TABLE)
        .content(task_data).await?;

    info!("schema/utilities.rs -> create_task_record: {create_task_record:?}");

    if !task_notes.is_empty() {
        for mut note in task_notes {
            let res = note.handle_note_creation().await;
            info!("schema/utilities.rs -> Task Note Creation from Mastertech: {res:?}");
        }
    }

    Ok(())
}

impl PrestashopPayload {

}

impl TaskNotePayload {

}

impl TaskPayload {

}

impl LiveTaskPayload {

}



pub async fn get_prestashop_payload(order_number: &str) -> anyhow::Result<PrestashopPayload, anyhow::Error> {
    let api_call = Prestashop::default();
    let mut query = HashMap::new();

    query.insert("filter[id_order]", order_number);
    query.insert("output_format", "JSON");

    let customer_threads: Vec<CustomerThread> = api_call
        .request_resources_wasm("customer_threads", query.clone())
        .await?;

    let mut customer_messages: Vec<CustomerMessage> = Vec::new();

    if !customer_threads.is_empty() {
        for thread in customer_threads.iter() {
            for msg in thread.associations.customer_messages.iter() {
                let msg =  api_call
                    .request_subresources_by_id_wasm(
                        "customer_messages",
                        "customer_message",
                        msg.id.as_str(),
                    )
                    .await?;
                customer_messages.push(msg)
            }
        }
    }

    let order: Order = api_call
        .request_subresources_by_id_wasm("orders", "order", order_number)
        .await?;

    if order.id_customer.is_empty() {
        info!("schema/utilities.rs -> Order is likely gonna fuKKKK");
    }

    info!("schema/utilities.rs -> order: {order:#?}");

    let sales_rep: Option<Employee> = if !order.id_employee_sales_rep.eq("0") {
        let employee: Employee = api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &order.id_employee_sales_rep,
            )
            .await?;

        info!("schema/utilities.rs -> employee: {employee:#?}");
        Some(employee)
    } else {
        None
    };

    let split_rep: Option<Employee> = if !order.id_employee_split_rep.eq("0") {
        let employee_2: Employee = api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &order.id_employee_split_rep,
            )
            .await?;

        info!("schema/utilities.rs -> employee: {sales_rep:#?}");
        Some(employee_2)
    } else {
        None
    };

    let cust: Customer = api_call
        .request_subresources_by_id_wasm("customers", "customer", &order.id_customer)
        .await?;

    let address: Address = api_call
        .request_subresources_by_id_wasm("addresses", "address", &order.id_address_invoice)
        .await?;


    info!("schema/utilities.rs -> address: {address:#?}");

    let customer = CustomerData {
        id: RecordId::from((
            CUSTOMER_TABLE.to_string(),
            order.id_customer.clone(),
        )),
        cust_code: order.id_customer.clone(),
        name: format!("{} {}", &cust.firstname, &cust.lastname),
        phone_number: address.phone.clone().to_string(),
        email: cust.email,
        ..Default::default()
    };

    Ok( 
        PrestashopPayload {
            customer,
            order,
            sales_rep,
            split_rep,
            address,
            customer_threads,
            customer_messages,
        }
    )
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
            None
        };

        if let Some(ref result) = formatted {
            self.cache.insert(phone.to_string(), result.clone());
        }

        formatted
    }
}