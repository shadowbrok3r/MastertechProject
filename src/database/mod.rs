use std::{collections::HashMap, sync::Arc};
use log::info;
use reqwest::header::{COOKIE, CONTENT_TYPE, ACCEPT, HeaderValue};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Serialize, Deserialize};
use serde_json::json;
use crate::{database::schema::{CustomerData, TicketData, TicketPayload}, handle_api::Store};
use self::database::Database;
use self::schema::{ComputerData, HardwareTests};

pub mod database;
pub mod schema;

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
    pub cpu_clock: u64,
    /// Live system temps
    pub component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: u64,
    /// Total RAM
    pub total_memory: u64,
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

impl TicketPayload{
    pub fn serialize_payload(
        pre_ticket: &PreTicketData, 
        computer_data: &ComputerData,
        service_number: &String,
        _antivirus_installed: &String,
        recommendations: &String,
        tech: String,
        salesman: String, 
        hardware_test_results: HardwareTests,
    ) -> Self{
        let pre_ticket_clone = pre_ticket.clone();

        let customer_data = CustomerData{
            id: None,
            computers: None,
            services: None,
            cust_code: pre_ticket_clone.cust_code.parse::<i32>().unwrap_or(0),
            name: pre_ticket_clone.customer_name,
            phone_number: pre_ticket_clone.customer_phone_1,
            phone_number_2: pre_ticket_clone.customer_phone_2,
            email: pre_ticket_clone.customer_email,
            li_doc: pre_ticket_clone.last_invoice_number.parse::<i32>().unwrap_or(0),
            li_amnt: pre_ticket_clone.last_invoice_amount,
            num_inv: pre_ticket_clone.total_invoice_count.parse::<i32>().unwrap_or(0),
            /*
                pub created_at: Option<String>,
                pub id: Option<TicketId>,
                pub due_date: String,
                pub customer: Option<CustomerId>,
                pub computer: Option<ComputerId>,
                pub service_task: Option<TaskId>,
                pub service_number: i32,
                /// Person that checked computer in
                pub checkin_rep: String,
                /// This is main initials on ticket
                pub sales_rep: String,
                pub checkin_notes: String,
                pub recommendations: String,
                pub tech: String,
                pub salesman: String,
                pub dep: String, // Store
                pub terms: String,
                pub ticket_total: String,
                pub doc_alias: String, // type of order (service,sales,transfer)
                pub current_antivirus: Option<Vec<String>>,
                pub hardware_test_results: HardwareTests,
             */
        };

        let mut current_antivirus: Vec<String> = Vec::new();
        current_antivirus.push("webroot".to_string());
        current_antivirus.push("superantiSpyware".to_string());


        let ticket_data = TicketData {
            created_at: None,
            id: None,
            due_date: pre_ticket_clone.due_date.unwrap(),
            customer: None,
            computer: None,
            service_task: None,
            service_number: service_number.parse::<i32>().unwrap(),
            checkin_rep: pre_ticket_clone.checkin_rep,
            sales_rep: pre_ticket_clone.sales_rep,
            checkin_notes: pre_ticket_clone.checkin_notes,
            recommendations: recommendations.to_string(),
            tech,
            salesman,
            dep: pre_ticket_clone.dep.as_str().to_string(),
            terms: pre_ticket_clone.terms,
            ticket_total: pre_ticket_clone.ticket_total,
            doc_alias: pre_ticket_clone.doc_alias,
            current_antivirus: Some(current_antivirus),
            hardware_test_results
        };

        let ticket_payload = TicketPayload { ticket_data, customer_data: customer_data.clone(), computer_data: computer_data.clone()};
        info!("Ticket Response: {ticket_payload:#?}");
        
        ticket_payload
    }

 }

pub async fn send_payload(payload: TicketPayload, client: reqwest::Client, cookie_store: Arc<CookieStoreMutex>, db: Database)  
-> anyhow::Result<String, anyhow::Error> {

    // let api_url = dotenv::var("API_URL").unwrap();
    // let submit_ticket_url = format!("{}/api/submitTicket", api_url.clone());
    let api_url = "https://axum.master-tech.app";// "http://localhost:4000";// "https://axum.master-tech.app";

    
    let params = json!({
        "name": "Logan",
        "email": "logan.lees@pclaptops.com",
        "password": "Poolparty10!9",
        "store": "RIV",
        "everest_initials": "LL"
    });

    info!("Sending signin req");
    let signin_response = client.post(format!("{api_url}/login")) 
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await;

    info!("Sent signin req");
    match signin_response{
        Ok(response) => {
            info!("Response => {response:?}");

            let cookie = get_cookie(cookie_store.lock().unwrap());


            let response = client
                .post(format!("{api_url}/api/submitTicket")) //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .header(COOKIE, HeaderValue::from_str(cookie.as_str())?)
                .json(&payload)
                .send()
                .await;

            Ok(response?.text().await?)
        },
        Err(err) => {
            info!("error with mastertech.app req => {err:?}");
            Err(err.into())
        }
    }
}

pub fn get_cookie(cookie_store: std::sync::MutexGuard<'_, CookieStore>) -> String{
    info!("getting cookie");
    let next_cookie = cookie_store.iter_any().next();
    let cookie_string = next_cookie.unwrap().to_string();
    cookie_string
}