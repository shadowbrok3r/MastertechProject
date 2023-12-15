use std::error::Error;

use log::debug;
use reqwest::header::{COOKIE, CONTENT_TYPE, ACCEPT, HeaderValue};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use crate::ticket_request::{Store, scaffold::HardwareTest};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct PreTicketData{
    pub cust_code: String,
    pub checkin_rep: String, // "USER_ID": "BP3", //checkin rep
    pub terms: String, // "TERMS": "CC",
    pub doc_alias: String, // "DOC_ALIAS": "SERVICE ORDER",
    pub dep: String, // "DEP": "LTN"
    pub jurisdiction: Store, //"JURISCODE": "LTN",
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

#[derive(Serialize, Deserialize)]
pub struct PulledKeys{
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct ComputerData{
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: Option<String>,
    pub ram: String,
    pub drives: Vec<DriveData>,
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

impl TicketResponse{
    pub fn serialize_payload(
        pre_ticket: &PreTicketData, 
        computer_data: &ComputerData,
        service_number: &String,
        antivirus_installed: &String,
        recommendations: &String,
        tech: String,
        salesman: String, 
        hardware_results: HardwareTests
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

        debug!("Ticket Response: {ticket_response:#?}");
        
        ticket_response
    }

 }

pub async fn send_payload(payload: TicketResponse, client: reqwest::Client)  
-> core::result::Result<String, Box<dyn Error>> {
    debug!("sending payload");

    
    let response = client
        .post("http://localhost:8080/api/submitTicket") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header(COOKIE, HeaderValue::from_static("jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzUxMiJ9.eyJpYXQiOjE3MDE5NjY0NjYsIm5iZiI6MTcwMTk2NjQ2NiwiZXhwIjoxNzAyMDUyODY2LCJpc3MiOiJTdXJyZWFsREIiLCJOUyI6Ik1hc3RlcnRlY2giLCJEQiI6Ik1hc3RlcnRlY2hEQiIsIlNDIjoidXNlciIsIklEIjoidXNlcjpkcDZpMnFldHJ2enYzdWY2Z3ZvdSJ9.vUoMmULjUZ7yTejrqAyYyP8Hl3jXqPmChQYCB228daC3DImwOid8MSa0uOI0_y-AwWv1m1X-6h87DGouNGXFpg"))
        .json(&payload)
        .send()
        .await;

    match response {
        Ok(res) => {
            let text_response = res.text().await?;
            Ok(text_response)
        },
        Err(e) => {
            debug!("Boxed error: {e:?}");
            Err(Box::new(e))
        },
    }
}