use std::collections::HashMap;
use std::fmt::Debug;
use database::DATABASE;
use log::info;
use schema::{CustomerData, TicketData, Record, LiveTaskPayload, TaskNotePayload, User, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE};
use serde::{Serialize, Deserialize};
use crate::tabs::websockets::Cmd;

use self::schema::ComputerData;  // HardwareTests

pub mod deserializer;
pub mod database;
pub mod schema;
pub mod prestashop_schema;

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct GetKeysResponse{
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SystemInformation {
    /// Live CPU usage as a percentaget
    pub cpu_percentage: f32,
    /// Live CPU clock speed
    pub cpu_clock: f32,
    /// Live system temps
    pub component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: f32,
    /// Total RAM
    pub total_memory: f32,
    /// Disk usage
    pub disks: String,
    /// Name of machine
    pub name: String,
    /// Kernel version
    pub kernel_version: String,
    /// OS version
    pub os_version: String,
    /// Hostname based on DNS
    pub hostname: String,
    /// Number of Physical CPU's
    pub number_of_cpus: String,

    pub network_interfaces: HashMap<String, String>,
}

pub async fn send_payload(
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    mut task_notes: Vec<TaskNotePayload>,
)  -> anyhow::Result<Vec<Record>, anyhow::Error> {
    info!("Send_Payload");
    let queried_salesman = query_user_from_email(ticket_data.salesman.clone()).await?;
    let _queried_tech = query_user_from_email(ticket_data.tech.clone()).await?;
    
    let task_id = task_data.id.clone();
    let ticket_id = ticket_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();

    task_data.task_name = format!("{} - {}", &customer_data.name, ticket_data.service_number.clone());
    task_data.service_ticket = ticket_id.clone();
    task_data.service_number = Some(ticket_data.service_number.clone());
    task_data.priority = schema::Priority::Normal;
    // task_data.dep = Some(queried_salesman.store.clone().as_str().to_string());
    task_data.everest_initials = queried_salesman.everest_initials;
    task_data.assignee = Some(queried_salesman.id);


    if let Some(cust) = query_id(CUSTOMER_TABLE, customer_id).await?{
        let update_cust_record: Option<Record> = DATABASE.update(cust.id).content(customer_data.clone()).await.unwrap();
        info!("Customer updated: {update_cust_record:?}");

        if let Some(computer_record) = query_id(COMPUTER_TABLE, computer_id).await?{
            let create_computer_record: Option<Record> = DATABASE.update(computer_record.id).content(computer_data).await.unwrap();
            info!("create_computer_record: {create_computer_record:?}");
        }else{
            let create_computer_record: Vec<Record> = DATABASE.create(COMPUTER_TABLE).content(computer_data).await.unwrap();
            info!("create_computer_record: {create_computer_record:?}");
        }
        if let Some(ticket) = query_id(TICKET_TABLE, ticket_id).await?{
            let service_ticket_record: Option<Record> = DATABASE.update(ticket.id).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }else{
            let service_ticket_record: Vec<Record> = DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }
    } else {
        let create_cust_record: Vec<Record> = DATABASE.create(CUSTOMER_TABLE).content(customer_data.clone()).await.unwrap();
        info!("create_cust_record created: {create_cust_record:?}");
        let create_computer_record: Vec<Record> = DATABASE.create(COMPUTER_TABLE).content(computer_data).await?;
        info!("create_computer_record created: {create_computer_record:?}");
        let service_ticket_record: Vec<Record> = DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
        info!("service_ticket_record created: {service_ticket_record:?}");
    }
    
    let create_task_record: Vec<Record> = DATABASE.create(TASK_TABLE).content(task_data).await?;
    info!("create_task_record: {create_task_record:?}");

    if task_notes.len() > 0 {
        info!("Task Notes: {:?}", task_notes);
        for note in task_notes.iter_mut() {
            note.task_id = task_id.clone();
            let create_task_note_record: Vec<Record> = DATABASE.create(TASK_NOTE_TABLE).content(note).await?;
            info!("create_task_note_record: {:?}", create_task_note_record);
        }
    }

    Ok(create_task_record)
}


pub async fn query_user_from_email(email: String) -> anyhow::Result<User, anyhow::Error>{
    let query = format!("SELECT id, name, everest_initials, email, store FROM user WHERE email == $email"); //  OR email == $email

    if email.contains("@pclaptops.com"){
        DATABASE.set("email", email.clone()).await?;
    } else {
        DATABASE.set("email", format!("{}@pclaptops.com", email.clone())).await?;
    }

    info!("Email: {}", email);
    let user: Option<User> = DATABASE.query(query).await?.take(0)?;
    info!("user: {:?}", user.clone());
    // let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
    Ok(user.unwrap())
}

pub async fn query_id<'a, T>(table: &'a str, id: T) 
    -> anyhow::Result<Option<Record>, anyhow::Error>
        where T: Serialize + Debug + Clone
{
    let query = format!("SELECT * FROM {table} WHERE id == ${table}");
    DATABASE.set(table, id).await.unwrap();
    let record: Option<Record> = DATABASE.query(query.clone()).await?.take(0).unwrap();
    info!("Query: {:?}  // {}", record, query);
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