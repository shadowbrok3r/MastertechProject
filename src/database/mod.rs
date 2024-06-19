use std::collections::HashMap;
use std::fmt::Debug;
use database::{Database, Record};
use log::info;
use schema::{ComputerId, CustomerId, LiveTaskPayload, Status, Store, TicketId, User, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use surrealdb::sql::Thing;
use crate::database::schema::{CustomerData, TicketData};

use self::schema::{ComputerData, HardwareTests};

pub mod database;
pub mod schema;
pub mod prestashop_schema;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct PreTicketData{
    pub cust_code: String,
    pub sales_rep: String,
    pub due_date: Option<String>,
    pub checkin_rep: String, // "USER_ID": "BP3", //checkin rep
    pub terms: String, // "TERMS": "CC",
    pub doc_alias: String, // "DOC_ALIAS": "SERVICE ORDER",
    pub dep: Store, // "DEP": "LTN"
    pub jurisdiction: String, //"JURISCODE": "LTN",
    pub ticket_total: String,

    pub customer_name: String, // "NAME": "Timber Ridge Fireplace LLC",
    pub customer_phone_1: String,
    pub customer_phone_2: String,
    pub customer_email: String,
    pub last_invoice_number: String, // "LI_DOC": "53745333",
    pub last_invoice_amount: String,  // "LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    pub total_invoice_count: String,
    pub checkin_notes: String,
    pub item_codes: String,
    //last_tuneup_date: String, // <-- HERE
    //last_checkin_date: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
}

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

// impl TicketPayload{
//     pub fn serialize_payload(
//         pre_ticket: &PreTicketData, 
//         computer_data: &ComputerData,
//         service_number: &String,
//         _antivirus_installed: &String,
//         recommendations: &String,
//         tech: String,
//         salesman: String, 
//         hardware_test_results: HardwareTests,
//     ) -> Self{
//         let pre_ticket_clone = pre_ticket.clone();
//         let customer_data = CustomerData{
//             id: None,
//             computers: None,
//             services: None,
//             cust_code: pre_ticket_clone.cust_code.parse::<i32>().unwrap_or(0),
//             name: pre_ticket_clone.customer_name,
//             phone_number: pre_ticket_clone.customer_phone_1,
//             phone_number_2: pre_ticket_clone.customer_phone_2,
//             email: pre_ticket_clone.customer_email,
//             li_doc: pre_ticket_clone.last_invoice_number.parse::<i32>().unwrap_or(0),
//             li_amnt: pre_ticket_clone.last_invoice_amount,
//             num_inv: pre_ticket_clone.total_invoice_count.parse::<i32>().unwrap_or(0),
//             part_order_links: None,
//         };
//         let mut current_antivirus: Vec<String> = Vec::new();
//         current_antivirus.push("webroot".to_string());
//         current_antivirus.push("superantiSpyware".to_string());
//         let service_ticket = TicketData {
//             service_number: service_number.parse::<i32>().unwrap(),
//             checkin_rep: pre_ticket_clone.checkin_rep,
//             sales_rep: pre_ticket_clone.sales_rep,
//             checkin_notes: pre_ticket_clone.checkin_notes,
//             recommendations: recommendations.to_string(),
//             tech,
//             salesman,
//             dep: pre_ticket_clone.dep.as_str().to_string(),
//             terms: pre_ticket_clone.terms,
//             ticket_total: pre_ticket_clone.ticket_total,
//             doc_alias: pre_ticket_clone.doc_alias,
//             current_antivirus: Some(current_antivirus),
//             hardware_test_results,
//             ..Default::default()
//         }; 
//         // due_date: pre_ticket_clone.due_date.unwrap(),
//         let _ticket_payload = TicketPayload { 
//             id: None,
//             created_at: None,
//             service_task: None, 
//             customer: Some(customer_data.clone()),
//             computer: Some(computer_data.clone()),
//             service_number: service_ticket.service_number, 
//             checkin_rep: todo!(), 
//             sales_rep: todo!(), 
//             checkin_notes: todo!(), 
//             recommendations: todo!(), 
//             tech, salesman, dep: todo!(), 
//             terms: todo!(), ticket_total: todo!(), 
//             doc_alias: todo!(), current_antivirus: todo!(), 
//             hardware_test_results };
//         info!("Ticket Response: {ticket_payload:#?}");
//         ticket_payload
//     }
//  }

pub async fn send_payload(        
    pre_ticket: PreTicketData, 
    computer_data: ComputerData,
    service_number: String,
    _antivirus_installed: String,
    recommendations: String,
    tech: String,
    salesman: String, 
    hardware_test_results: HardwareTests,
    database: Database
)  -> anyhow::Result<Vec<Record>, anyhow::Error> {
    let cust_code = pre_ticket.cust_code.parse::<i32>()?;
    let customer_id: CustomerId = CustomerId(Thing::from((CUSTOMER_TABLE.to_string(), cust_code.to_string().clone())));

    let ticket_id: TicketId = TicketId(Thing::from((TICKET_TABLE.to_string(), service_number.clone())));

    let computer_customer_id: String = format!("{}-{}", computer_data.hostname.clone(), cust_code);
    let computer_id: ComputerId = ComputerId(Thing::from((COMPUTER_TABLE.to_string() , computer_customer_id)));

    let queried_salesman = query_user_from_initials(
        database.clone(), 
        salesman.clone(),
    ).await?;

    let queried_tech = query_user_from_initials(
        database.clone(), 
        tech.clone(),
    ).await?;

    let mut pre_ticket_clone = pre_ticket.clone();

    let mut current_antivirus: Vec<String> = Vec::new();
    current_antivirus.push("webroot".to_string());
    current_antivirus.push("superantiSpyware".to_string());

    let mut owned_computers: Vec<ComputerId> = Vec::new();
    let mut services: Vec<TicketId> = Vec::new();

    owned_computers.push(computer_id.clone());
    services.push(ticket_id.clone());

    let computer = ComputerData {
        id: Some(computer_id.clone()),
        customer: Some(customer_id.clone()),
        seb_info: computer_data.seb_info,
        hostname: computer_data.hostname,
        operating_system: computer_data.operating_system.trim().to_string(),
        cpu: computer_data.cpu.trim().to_string(),
        gpu: computer_data.gpu.trim().to_string(),
        ram: computer_data.ram.trim().to_string(),
        drives: computer_data.drives,
    };

    let customer = CustomerData{
        id: Some(customer_id.clone()),
        computers: Some(owned_computers),
        services: Some(services),
        cust_code: pre_ticket_clone.cust_code.parse::<i32>()?,
        name: pre_ticket_clone.customer_name,
        phone_number: pre_ticket_clone.customer_phone_1,
        phone_number_2: pre_ticket_clone.customer_phone_2,
        email: pre_ticket_clone.customer_email,
        li_doc: pre_ticket_clone.last_invoice_number.parse::<i32>()?,
        li_amnt: pre_ticket_clone.last_invoice_amount,
        num_inv: pre_ticket_clone.total_invoice_count.parse::<i32>()?,
        ..Default::default()
    };

    // todo!(), // this sales rep shit is wild, and completely wrong, i need to look at this at work..
    let service_ticket = TicketData {
        id: Some(ticket_id.clone()),
        customer: Some(customer_id.clone()),
        computer: Some(computer_id.clone()),
        service_number: service_number.parse::<i32>()?,
        checkin_rep: pre_ticket_clone.checkin_rep,
        sales_rep: pre_ticket_clone.sales_rep,
        checkin_notes: pre_ticket_clone.checkin_notes,
        recommendations: recommendations.to_string(),
        tech: queried_tech.everest_initials.clone(),
        salesman: queried_salesman.everest_initials,
        dep: pre_ticket_clone.dep.as_str().to_string(),
        terms: pre_ticket_clone.terms,
        ticket_total: pre_ticket_clone.ticket_total,
        doc_alias: pre_ticket_clone.doc_alias,
        current_antivirus: Some(current_antivirus),
        hardware_test_results,
        ..Default::default()
    };
    
    if let Some(cust) = query_id(database.clone(), CUSTOMER_TABLE, customer_id).await?{
        let update_cust_record: Option<Record> = database.database.update(cust.id).content(customer.clone()).await.unwrap();
        info!("Customer updated: {update_cust_record:?}");

        if let Some(computer_record) = query_id(database.clone(), COMPUTER_TABLE, computer_id).await?{
            let create_computer_record: Option<Record> = database.database.update(computer_record.id).content(computer).await.unwrap();
            info!("create_computer_record: {create_computer_record:?}");
        }else{
            let create_computer_record: Vec<Record> = database.database.create(COMPUTER_TABLE).content(computer).await.unwrap();
            info!("create_computer_record: {create_computer_record:?}");
        }
        if let Some(ticket) = query_id(database.clone(), TICKET_TABLE, ticket_id.clone()).await?{
            let service_ticket_record: Option<Record> = database.database.update(ticket.id).content(service_ticket).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }else{
            let service_ticket_record: Vec<Record> = database.database.create(TICKET_TABLE).content(service_ticket).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }
    }else{
        let create_cust_record: Vec<Record> = database.database.create(CUSTOMER_TABLE).content(customer.clone()).await.unwrap();
        info!("create_cust_record created: {create_cust_record:?}");
        let create_computer_record: Vec<Record> = database.database.create(COMPUTER_TABLE).content(computer).await?;
        info!("create_computer_record created: {create_computer_record:?}");
        let service_ticket_record: Vec<Record> = database.database.create(TICKET_TABLE).content(service_ticket).await?;
        info!("service_ticket_record created: {service_ticket_record:?}");
    }
    

    let task = LiveTaskPayload {
        task_name: format!("{} - {}", &customer.name, service_number),
        service_ticket: Some(ticket_id),
        assignee: Some(queried_salesman.id),
        service_number: Some(service_number.parse::<i32>()?),
        due_date: pre_ticket_clone.due_date.unwrap(),
        priority: schema::Priority::Normal,
        task_note: None,
        completed: false,
        status: Status::Todo,
        dep: Some(queried_salesman.store.clone().as_str().to_string()),
        everest_initials: queried_tech.everest_initials,
        // task_description: todo!(),
        ..Default::default()
    };

    let create_task_record: Vec<Record> = database
        .database
        .create(TASK_TABLE)
        .content(task)
        .await?;

    info!("create_task_record: {create_task_record:?}");

    Ok(create_task_record)
}


pub async fn query_user_from_initials(database: Database, initials: String) -> anyhow::Result<User, anyhow::Error>{
    let query = format!("SELECT id, name, everest_initials, email, store FROM user WHERE everest_initials == $everest_initials");
    database.database.set("everest_initials", initials).await?;
    let user: Vec<Value> = database.database.query(query).await?.take(0)?;
    let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
    Ok(usr)
}

pub async fn query_id<'a, T>(database: Database, table: &'a str, id: T) 
    -> anyhow::Result<Option<Record>, anyhow::Error>
        where T: Serialize + Debug + Clone
{
    let query = format!("SELECT * FROM {table} WHERE id == ${table}");
    database.database.set(table, id).await.unwrap();
    let record: Option<Record> = database.database.query(query.clone()).await?.take(0).unwrap();
    info!("Query: {:?}  // {}", record, query);
    Ok(record)
}


pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    bincode::serialize(system_info).expect("Failed to serialize SystemInformation")
}


pub fn deserialize_system_info(bytes: &[u8]) -> SystemInformation {
    bincode::deserialize(bytes).expect("Failed to deserialize SystemInformation")
}
