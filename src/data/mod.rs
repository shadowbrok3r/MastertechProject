use std::{collections::HashMap, error::Error, sync::Arc};

use dotenv::dotenv;
use log::{debug, info};
use reqwest::header::{COOKIE, CONTENT_TYPE, ACCEPT, HeaderValue};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Serialize, Deserialize};
use serde_json::json;
use tokio::spawn;
use crate::ticket_request::Store;

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

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TicketData{
    #[serde(flatten)]
    pub pre_ticket_data: PreTicketData,

    pub current_antivirus: Vec<String>,
    pub service_number: i32,
    pub recommendations: String,
    pub tech: String,
    pub salesman: String,
    pub hardware_test_results: HardwareTests
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct GetKeysResponse{
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
#[serde(rename = "xml")]
pub struct LocalSebData {
    pub InstalledDeviceId: String,
    pub InstallInstanceId: String,
    pub HasIssues: String,
    pub InstallationStage: String,
    pub ReasonCode: String,
    pub ActivationCode: String,
    pub InstallVersion: String,
    pub MachineName: String,
    pub ExtendedSeb: Option<ExtendedSeb>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ExtendedSeb {
    pub email: String,
    pub phone: String,
    pub userid: String,
    pub device_name: String,
    pub device_id: String,
    pub state: String,
    pub usage_gb: String,
    pub date_device_created: String,
    pub activated: String,
    pub activation_code: String,
    pub last_complete_backup: String,
    pub last_client_status_update: String,
    pub id_recurly_account: String,
    pub date_last_scan: String,
    pub date_email_sent: String,
    pub date_canceled_account: String,
    pub date_deleted_account: String,
    pub current_period_ends_at: String,
    pub date_modified: String,
    pub date_created: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct ComputerData{
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: Option<String>,
    pub ram: String,
    pub drives: Vec<DriveData>,
    pub seb_info: Option<LocalSebData>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TicketResponse{
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CustomerData{
    pub cust_code: i32,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String,
    pub email: String, 
    pub li_doc: i32,
    pub li_amnt: String,
    pub num_inv: i32,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct HardwareTests{
    pub hdd_test: String,
    pub ssd_test: String,
    pub ram_test: String
}

impl ComputerData{
    pub fn new() -> Self{
        ComputerData{
            drives: Vec::new(),
            ..Default::default()
        }
    }

    pub fn add_disk(&mut self, disk: DriveData){
        self.drives.push(disk);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DriveData{
    pub drive_letter: String,
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SystemInformation {
    /// Live CPU usage as a percentage
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

impl TicketResponse{
    pub fn serialize_payload(
        pre_ticket: &PreTicketData, 
        computer_data: &ComputerData,
        service_number: &String,
        antivirus_installed: &String,
        recommendations: &String,
        tech: String,
        salesman: String, 
        hardware_results: HardwareTests,
    ) -> Self{
        let pre_ticket_clone = pre_ticket.clone();

        let customer_data = CustomerData{
            cust_code: pre_ticket_clone.cust_code.parse::<i32>().unwrap_or(0),
            name: pre_ticket_clone.customer_name,
            phone_number: pre_ticket_clone.customer_phone_1,
            phone_number_2: pre_ticket_clone.customer_phone_2,
            email: pre_ticket_clone.customer_email,
            li_doc: pre_ticket_clone.last_invoice_number.parse::<i32>().unwrap_or(0),
            li_amnt: pre_ticket_clone.last_invoice_amount,
            num_inv: pre_ticket_clone.total_invoice_count.parse::<i32>().unwrap_or(0),
        };

        let mut current_antivirus: Vec<String> = Vec::new();
        current_antivirus.push("webroot".to_string());
        current_antivirus.push("superantiSpyware".to_string());

        let ticket_data = TicketData{
            pre_ticket_data: pre_ticket.clone(),
            current_antivirus, //.clone(),
            service_number: service_number.parse::<i32>().unwrap_or(0),
            recommendations: recommendations.clone(),
            tech,
            salesman,
            hardware_test_results: hardware_results,
        };

        let ticket_response = TicketResponse { 
            ticket_data, 
            customer_data, 
            computer_data: computer_data.clone() 
        };

        info!("Ticket Response: {ticket_response:#?}");
        
        ticket_response
    }

 }

pub async fn send_payload(payload: TicketResponse, client: reqwest::Client, cookie_store: Arc<CookieStoreMutex>)  
-> core::result::Result<String, Box<dyn Error>> {

    // let api_url = dotenv::var("API_URL").unwrap();
    // let submit_ticket_url = format!("{}/api/submitTicket", api_url.clone());
    let api_url = "https://axum.master-tech.app";

    
    let params = json!({
        "name": "Logan",
        "email": "logan.lees@pclaptops.com",
        "password": "Poolparty10!9",
        "store": "RIV",
        "everest_initials": "LL"
    });

    // spawn(async move{
        
    // })
    info!("Sending signin req");
    let signin_response = client.post(format!("{api_url}/login")) 
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await;

    let mut cookie_string = String::new();
    let mut cookie: &str = "";
    info!("Sent signin req");
    match signin_response{
        Ok(response) => {
            info!("Response => {response:?}");

            let cookie = get_cookie(cookie_store.lock().unwrap());


            let response = client
                .post(format!("{api_url}/api/submitTicket")) //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .header(COOKIE, HeaderValue::from_str(cookie.as_str()).unwrap())
                .json(&payload)
                .send()
                .await;

            Ok(response.unwrap().text().await.unwrap_or("default".to_string()))
        },
        Err(err) => {
            info!("error with mastertech.app req => {err:?}");
            Err(Box::new(err))
        }
    }



    // match response {
    //     Ok(res) => {
    //         let text_response = res.text().await?;
    //         Ok(text_response)
    //     },
    //     Err(e) => {
    //         info!("Boxed error: {e:?}");
    //         Err(Box::new(e))
    //     },
    // }
}

fn get_cookie(cookie_store: std::sync::MutexGuard<'_, CookieStore>) -> String{
    info!("getting cookie");
    let next_cookie = cookie_store.iter_any().next();
    let cookie_string = next_cookie.unwrap().to_string();
    cookie_string
}