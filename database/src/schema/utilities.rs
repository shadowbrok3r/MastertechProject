use crate::{schema::{prestashop_schema::{Address, Customer, CustomerMessage, CustomerThread, Employee, Order, Prestashop}, ConnectedClient, Priority, Qc, Record, Store, TaskNotePayload, TaskPayload, User, CUSTOMER_TABLE, TASK_TABLE}, PlatformSpawner, Spawner, DATABASE};
use super::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, LiveTaskPayload, LocalSebData, Notification, TicketData};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, Weekday};
use std::{collections::HashMap, fmt::Debug};
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use async_trait::async_trait;
use log::{debug, info, warn};
use anyhow::{Error, Result};
use surrealdb::RecordId;
use web_time::Instant;
use regex::Regex;

pub trait LiveUpdate {
    fn handle_live_create(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_update(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_delete(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>;
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

pub async fn query_id<T>(table: String, id: RecordId) -> Result<Option<T>, Error>
where
    T: Serialize + Debug + Clone + 'static + for <'de> Deserialize <'de>
{
    let mut record: surrealdb::Response = DATABASE
        .query("SELECT * FROM $table WHERE id == $id")
        .bind(("id", id.clone()))
        .bind(("table", table.clone()))
        .await?;

    info!("schema/utilities.rs/query_id -> Record: {:?}\nSELECT * FROM ${id:?}", record);
    Ok(record.take::<Option<T>>(0)?)
}

pub async fn check_id_existence<T>(_table: String, id: T) -> Result<Option<bool>, Error>
where
    T: Serialize + Debug + Clone + 'static,
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
        SELECT * FROM task WHERE $this.assignee.store == $store AND $this.completed IS false PARALLEL
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
        PARALLEL
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

impl CustomerData {
    pub async fn get_customers(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let customers: Vec<Self> = DATABASE
            .query("SELECT * FROM customer START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(customers)
    }
}

impl ComputerData {
    pub async fn get_computers(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let computers: Vec<Self> = DATABASE
            .query("SELECT * FROM computer START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(computers)
    }
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
        .query("SELECT * FROM user WHERE store == $store AND active == true PARALLEL")
        .bind(("store", store))
        .await?
        .take(0)?;
    tx.try_send(data)?;
    Ok(())
}

pub async fn get_connected_clients(tx: Sender<Vec<ConnectedClient>>) -> Result<(), Error> {
    debug!("get_connected_clients");
    let query: Vec<ConnectedClient> = DATABASE
        .query("SELECT * FROM connected_client WHERE assigned_user == $auth.id && connected == true ")
        .await?
        .take(0)?;
    // info!("Clients: {:?}", query);
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
    .await?;

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
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    mut task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
) -> anyhow::Result<(), anyhow::Error> {
    info!("schema/utilities.rs -> Send_Payload");
    let queried_salesman = User::query_user_from_email(ticket_data.salesman.clone()).await.unwrap_or_default();
    let _queried_tech = User::query_user_from_email(ticket_data.tech.clone()).await.unwrap_or_default();
    
    
    // let task_id = task_data.id.clone();
    let ticket_id = ticket_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();
    let service_number = ticket_data.service_number.clone();
    task_data.task_name = format!(
        "{} - {}",
        &customer_data.name,
        service_number.clone()
    );
    task_data.service_ticket = Some(ticket_id.clone());
    task_data.service_number = Some(service_number.clone());
    task_data.priority = Priority::Normal;
    task_data.assignee = queried_salesman.get_id();

    // if ticket_data.computer.is_none() {
    //     ticket_data.computer = Some(computer_data.id.clone());
    // }

    info!("schema/utilities.rs -> cust_record: {customer_data:?}");
    let update_customer: Result<Option<Record>, surrealdb::Error> = DATABASE
        .upsert(customer_id)
        .content(customer_data.clone())
        .await;
    
    match update_customer {
        Ok(record) => log::info!("Updated Customer {record:?}"),
        Err(e) => {
            log::warn!("Error updating Customer {e:?}");
            // if i have a customer from everest, i will need to delete
            // and recreate the record.. 
        }
    }

    if send_specs {
        let create_computer_record: Option<Record> = DATABASE
            .upsert(computer_id)
            .content(computer_data)
            .await?;
        info!("schema/utilities.rs -> create_computer_record: {create_computer_record:?}");
    }

    info!("schema/utilities.rs -> ticket record: {ticket_data:?}");
    let service_ticket_record: Option<Record> = DATABASE
        .upsert(ticket_id)
        .content(ticket_data)
        .await?;
    info!("schema/utilities.rs -> service_ticket_record: {service_ticket_record:?}");

    info!("schema/utilities.rs -> Task Data: {:?}", &task_data);

    
    let check_task_record: Vec<LiveTaskPayload> = DATABASE
        .query("SELECT * FROM task WHERE service_number == $service_number")
        .bind(("service_number", service_number.clone()))
        .await?
        .take(0)?;

    info!("schema/utilities.rs -> check_task_record: {check_task_record:?}");

    if !check_task_record.is_empty() {
        for task in check_task_record.iter() {
            if task.id == task_data.id {
                let upsert_task_record: Option<Record> = DATABASE
                    .update(task.id.clone())
                    .content(LiveTaskPayload {
                        id: task.id.clone(),
                        ..task_data.clone()
                    }).await?;

                for note in task_notes.iter_mut() {
                    if note.task_id == Some(task_data.id.clone()) && note.task_id != Some(task.id.clone()) {
                        note.task_id = Some(task.id.clone());
                    }
                }
                info!("schema/utilities.rs -> upsert_task_record: {upsert_task_record:?}");
            }

        } 
    } else {
        let create_task_record: Option<Record> = DATABASE
            .create(TASK_TABLE)
            .content(task_data).await?;
        info!("schema/utilities.rs -> create_task_record: {create_task_record:?}");
    }

    for mut note in task_notes {
        let res = note.handle_note_creation().await;
        info!("schema/utilities.rs -> Task Note Creation from Mastertech: {res:?}");
    }

    Ok(())
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
            id: RecordId::from((
                CUSTOMER_TABLE.to_string(),
                id_customer,
            )),
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

impl CustomerData {
    pub async fn find_customer_by_id(id_customer: &str) -> anyhow::Result<Self, anyhow::Error> {
        let api_call = Prestashop::default();
        let mut query = HashMap::new();
        let mut tmp_address = Address::default();
        query.insert("filter[id_customer]", id_customer);
        query.insert("output_format", "JSON");

        let addresses: Vec<Address> = api_call
            .request_resources_wasm("addresses", query.clone())
            .await?;

        if let Some(address) = addresses.get(0) {
            tmp_address = address.clone();
        }

        let cust: Customer = api_call
            .request_subresources_by_id_wasm("customers", "customer", id_customer)
            .await?;

        Ok(CustomerData { 
            id: RecordId::from((
                CUSTOMER_TABLE.to_string(),
                id_customer,
            )),
            cust_code: id_customer.to_string(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            phone_number: tmp_address.phone.clone().to_string(),
            email: cust.email,
            phone_number_2: tmp_address.phone_mobile.clone().to_string(),
            ..Default::default()
        })
    }
}

impl TaskPayload {

}

impl LiveTaskPayload {

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
        let employee: Employee = api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &potential_order.id_employee_sales_rep,
            )
            .await?;

        info!("schema/utilities.rs -> employee: {employee:#?}");
        Some(employee)
    } else {
        None
    };

    let split_rep: Option<Employee> = if !potential_order.id_employee_split_rep.eq("0") {
        let employee_2: Employee = api_call
            .request_subresources_by_id_wasm(
                "employees",
                "employee",
                &potential_order.id_employee_split_rep,
            )
            .await?;

        info!("schema/utilities.rs -> employee: {sales_rep:#?}");
        Some(employee_2)
    } else {
        None
    };

    let cust: Customer = api_call
        .request_subresources_by_id_wasm("customers", "customer", &tmp_address.id_customer)
        .await?;


    info!("schema/utilities.rs -> address: {tmp_address:#?}");

    let customer = CustomerData {
        id: RecordId::from((
            CUSTOMER_TABLE.to_string(),
            potential_order.id_customer.clone(),
        )),
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

    query.insert("filter[id_order]", order_number);
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

                match msg.into_task_note(order_number).await {
                    Ok(task_note) => task_notes.push(task_note),
                    Err(e) => log::error!("Error converting cust msg into task note: {e:?}"),
                }

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
        phone_number_2: address.phone_mobile.clone().to_string(),
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