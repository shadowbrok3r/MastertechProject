#![allow(non_snake_case)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use crossbeam::channel;
use reqwest::header::{CONTENT_TYPE, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use tokio::io::AsyncWriteExt;
use std::{error::Error, path::PathBuf};
use crate::scaffold::*;
use asana::{
    apis::{
        configuration::Configuration, 
        tasks_api::{
            create_task,
            delete_task,
        }, attachments_api::create_attachment_for_task
    }, 
    models::{
        task_request,
        InlineObject35, TaskResponse
    }
};

#[derive(Debug, Deserialize)]
pub struct GetTicketResponse {
    pub header: Header,
    pub customer: Customer,
    //pub transactions: Transactions,
    pub addresses: Addresses,
    pub items: Vec<Value>,
}

pub struct GetKeysResponse{
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Deserialize, Debug)]
pub struct Header {
    pub CUST_CODE: String,
    pub USER_ID: String, // "USER_ID": "BP3", //checkin rep
    pub TERMS: String, // "TERMS": "CC",
    pub DOC_ALIAS: String, // "DOC_ALIAS": "SERVICE ORDER",
    pub DEP: String, // "DEP": "LTN"
    pub JURISCODE: String, //"JURISCODE": "LTN",
    pub COG: String, // "COG": "7.1000", //Cost of goods?
    pub INV_AMOUNT: Option<String>, // "INV_AMOUNT": "53.6100",
}

#[derive(Deserialize, Debug)]
pub struct Customer {
    pub NAME: String, // "NAME": "Timber Ridge Fireplace LLC",
    //pub CUSTOMER_ADDRESS: String,
    pub LI_DOC: Option<String>, //"LI_DOC": "53745333",
    pub LI_AMT: Option<String>,  //"LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    //pub LAST_TUNEUP_DATE: String, // <-- HERE
    pub DW_UPDATE_DATE: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    pub NUM_INV: Option<String>, // "NUM_INV": "21",
/*		"LP_AMT": "-53.6100",
		"LP_DOC": "52883815",
		"LP_DOC_TYP": "8",
		"LP_DATE": "2023-05-04 00:00:00.000", 
*/

}

#[derive(Deserialize, Debug)]
pub struct Transactions{
    pub TRANSAC_OBJ_ONE: TransacObjectOne,
}

#[derive(Deserialize, Debug)]
pub struct TransacObjectOne{
/*
    "TRANHIST_DATE": "2023-05-04 14:25:36.000",
    "USER_ID": "KMJ",
    "AMOUNT": "53.6100",
    "PAY_TYPE": "LTNVM",
    "DESCRIPT": "PAYMENT RECEIVED ON SALES ORDER",
 */
}

#[derive(Deserialize, Debug)]
pub struct Addresses {
    pub address_object: AddressObject,
/*
    "ACCT_NAME": "Timber Ridge Fireplace LLC",
    "NAME": "Timber Ridge Fireplace LLC",
    "LAST_NAME": "Hale",
    "FIRST_NAME": "Lisa",
    "MOBILE_PHONE": "8013501447",
    "ADDRESS_LINE1": "3080 N Fairfield Rd Suite #1",
 */
}

#[derive(Deserialize, Debug)]
pub struct AddressObject{
    pub TEL1: String, // "TEL1": "8018376254",
    pub TEL2: String, // "TEL2": "",
    pub EMAIL: String,
}
#[derive(Deserialize, Debug)]
pub struct ItemsArray{ // number of items is the number of item codes you have on an order 
    // this could also get srvc/etc
   pub item_objects: Vec<Value>,

}

pub struct SendRequest {
    pub tx: std::sync::mpsc::Sender<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AsanaResponse{
    pub gid: Option<String>,
    pub created_at: Option<String>,
    pub error: Option<String>,
    pub raw_resp: Option<String>,
}

impl SendRequest{
    pub fn get_ticket(so_number: String, tx: std::sync::mpsc::Sender<String>, client: reqwest::Client){
        
        tokio::spawn(async move{
            let args = vec![
                serde_json::json!(so_number),
                serde_json::json!("false"),
            ];
        
            // Construct the scaffold call
            let scaffold_builder = ScaffoldRequestBuilder{
                app: ScaffoldApps::Everest,
                action: ScaffoldActions::EverestCall, 
                call: Some(ScaffoldCalls::GetOrder), 
                arguments: Some(args.clone())
        
            };

            // Await the response 
            let response = request_ticket_info(scaffold_builder, client).await;

            // Handle the response
            match response { // Successfully received GetTicketResponse
                Ok(get_ticket_response) => {
                    
                    // You can now use fields of get_ticket_response
                    let header = &get_ticket_response.header;
                    let customer = &get_ticket_response.customer;
                    let addresses = &get_ticket_response.addresses.address_object;
                    let items_objects = get_ticket_response.items;
                    //let transactions = &get_ticket_response.transactions;

                    let mut checkin_note = "".to_string();
                    let mut itemcodes = "".to_string();

                    let mut li_amt = "".to_string();
                    let mut li_doc = "".to_string();
                    let mut inv_amnt = "".to_string();
                    let mut num_inv = "".to_string();

                    if customer.LI_AMT.is_some() { 
                        li_amt = customer.LI_AMT.clone().unwrap_or_else(||{
                            "null value".to_string()
                        }) 
                    }
                    if customer.LI_DOC.is_some() { 
                        li_doc = customer.LI_DOC.clone().unwrap_or_else(||{
                            "null value".to_string()
                        }) 
                    }
                    if header.INV_AMOUNT.is_some() { 
                        inv_amnt = header.INV_AMOUNT.clone().unwrap_or_else(||{
                            "null value".to_string()
                        }) 
                    }
                    if customer.NUM_INV.is_some() { 
                        num_inv = customer.NUM_INV.clone().unwrap_or_else(||{
                            "null value".to_string()
                        }) 
                    }
                    // DW_UPDATE_DATE is the exact time that the line item (AKA 'items') was added.
                    // iterates through the array of objects, gets note if not null and not empty, parses, assigns to checkin_note
                    
                    for object in items_objects{

                        // If i want to....
                        // "COST": "7.100000", this is our cost
                        // ITEM_PR_FEX is what we charge the customer, although AMOUNT is the same value
                        object.get("NOTE")
                        .and_then(|v| v.as_str())
                        .map(|note| {
                            if note != "null" && !note.is_empty() {
                                let parts: Vec<&str> = note.split("Symptoms (Details):").collect();
                                if parts.len() > 1{
                                    let note = &parts[1].to_string();
                                    checkin_note = note.to_string();
                                }
                            }
                        });

                        object.get("ITEM_CODE")
                        .and_then(|v| v.as_str())
                        .map(|item_code| {
                            itemcodes += &format!("{item_code}\n").to_string();
                        });
                    }


                    let ticket_information = TicketInformation{
                        cust_code: header.CUST_CODE.clone(),
                        user_id: header.USER_ID.clone(),
                        customer_phone_1: addresses.TEL1.clone(),
                        customer_phone_2: addresses.TEL2.clone(),
                        customer_email: addresses.EMAIL.clone(),
                        last_invoice_amount: li_amt,
                        terms: header.TERMS.clone(),
                        doc_alias: header.DOC_ALIAS.clone(),
                        department: header.DEP.clone(),
                        jurisdiction: header.JURISCODE.clone(),
                        invoice_amnt: inv_amnt,
                        customer_name: customer.NAME.clone(),
                        checkin_notes: checkin_note.clone(),
                        last_invoice_number: li_doc,
                        item_codes: itemcodes.clone(),
                        total_invoice_count: num_inv,
                        //last_tuneup_date: customer.LAST_TUNEUP_DATE.clone(),
                        //last_checkin_date: customer.LI_AMT.clone(),
                    };
                    

                    let ticket_info_json = serde_json::to_string(&ticket_information).unwrap();
                    match tx.send(ticket_info_json) {
                        Ok(_) => {
                            drop(tx)
                        },
                        Err(e) => {
                            eprintln!("Error while sending ticket information: {}", e.to_string());
                            drop(tx)
                        }
                    }
                    
                },
                Err(e) => { 
                    match tx.send(e.to_string()) {
                        Ok(_) => {
                            drop(tx)
                        },
                        Err(e) => {
                            eprintln!("Error while sending error message: {}", e);
                            drop(tx)
                        }
                    }
                }
                
            }
        });
    }
    
    pub fn get_cps(so_number: String, tx: std::sync::mpsc::Sender<String>, client: reqwest::Client){
        tokio::spawn(async move{
            let args = vec![
                serde_json::json!(so_number),
            ];
        
            let scaffold_builder = ScaffoldRequestBuilder{
                app: ScaffoldApps::SoftwareLicenseFetch,
                action: ScaffoldActions::FetchKeys, 
                call: Some(ScaffoldCalls::None),
                arguments: Some(args.clone())
            };
            
            let response = request_keys(scaffold_builder, client).await;

            match response { // Successfully received GetTicketResponse
                Ok(get_keys_response) => {

                    let webroot_key = &get_keys_response.webroot_key;
                    let superanti_key = &get_keys_response.superanti_key;

            

                    let cps_keys = PulledKeys{
                        webroot_key: webroot_key.to_string(),
                        superanti_key: superanti_key.to_string()
                    };

                    let cps_keys_json = serde_json::to_string(&cps_keys).unwrap();
                    
                    match tx.send(cps_keys_json) {
                        Ok(_) => {
                            drop(tx)
                        },
                        Err(e) => {
                            eprintln!("Error while sending ticket information: {}", e.to_string());
                            drop(tx)
                        }
                    }
                    
                },
                Err(e) => { 
                    match tx.send(e.to_string()) {
                        Ok(_) => {
                            drop(tx)
                        },
                        Err(e) => {
                            eprintln!("Error while sending error message: {}", e);
                            drop(tx)
                        }
                    }
                }                    
            }
        });
    }

    pub fn send_ticket_request(
        tx: std::sync::mpsc::Sender<String>, 
        client: reqwest::Client, 
        task_name: (&String, &String),
        html_notes: String,
        assignees: (&String, &String),
        due_date: String,
        file_attachment: Option<PathBuf>
    ){
        let (sender, receiver) = channel::bounded::<String>(5);

        let cust = task_name.0.clone();
        let so_num = task_name.1.clone();

        let mut assigned_salesman = "1202792432658520".to_string(); // Jake
        let mut assigned_tech = "1199992640930465".to_string(); // Logan

        if assignees.0 == "JDH2"{ assigned_salesman = "1202792432658520".to_string(); }
        else if assignees.0 == "DMK"{ assigned_salesman = "1202791016369879".to_string() }

        if assignees.1 == "LL" { assigned_tech = "1199992640930465".to_string(); }
        else if assignees.1 == "BLK" { assigned_tech = "1202792432421640".to_string(); }
        else if assignees.1 == "TBN" { assigned_tech = "1202792432551073".to_string(); }

        let mut asana_response = AsanaResponse{
            gid: Some("".to_string()),
            created_at: Some("".to_string()),
            error: Some("".to_string()),
            raw_resp: Some("".to_string())
        };

        tokio::spawn(async move{
            // ideally, id like to also add the functionality to search for a task by the SO number
            // so we can update the ticket or delete it, or add an attachment
            // i should use create_attachment_for_task,
            //  dependencies: Option<Vec<AsanaResource>> 
            // to add spo as dependancy to task

            let mut asana_config = Configuration::new();
            let mut task = task_request::TaskRequest::new();

            asana_config.client = client;
            asana_config.bearer_access_token = Some("1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2".to_string());
            asana_config.user_agent = None;
            
            task.name = Some(format!("{cust} - {so_num}"));
            task.assignee = Some(assigned_salesman.clone());
            task.workspace = Some("13314583095021".to_string());
            task.projects = Some(vec!["1202792139600600".to_string()]);
            task.followers = Some(vec![assigned_salesman, assigned_tech]); // Logan: 1199992640930465
            task.html_notes = Some(html_notes);
            task.due_on = Some(due_date);
            //task.resource_subtype = Some("".to_string());
            //task.dependencies = Some("".to_string());

            let asana_task = 
            InlineObject35{ data: Some(Box::new(task)) };

            // Serialize the html_notes to a JSON string and calculate its length
            let html_notes_json = serde_json::to_string(&asana_task).unwrap();
            let content_length = html_notes_json.len();
            println!("Content length: {}", content_length);
            

            

            match create_task(&asana_config, 
                asana_task, 
                Some(true), //only set to true if debugging
                None
                ).await
            {
                Ok(res) => {
                    let data = res.data.unwrap(); //.gid.unwrap().to_string();
                    println!("{data:?}");
                    asana_response.gid = Some(data.gid.unwrap().to_string());
                    asana_response.created_at = Some(data.created_at.unwrap().to_string());

                    let asana_json_response = serde_json::to_string(&asana_response).unwrap();

                    match sender.send(asana_json_response){
                        Ok(_) => println!("sent data successfully"),
                        Err(e) => println!("{e}")
                    }
                    
                },
                Err(e) => {
                    match e{
                        asana::apis::Error::Reqwest(e) => println!("reqwest error: {e}"),
                        asana::apis::Error::Serde(e) => println!("Serde error: {e}"),
                        asana::apis::Error::Io(e) => println!("IO error: {e}"),
                        asana::apis::Error::ResponseError(e) => {
                            match tx.send(e.content){
                                Ok(_) => println!("sent error successfully"),
                                Err(e) => println!("send error: {e}")
                            };
                        }
                    }
                }
            }

            let mut task_gid = String::new();
            if let Ok(gid) = receiver.recv(){
                println!("received gid: {gid}");
                task_gid = gid;
            }
            

            match create_attachment_for_task(
                &asana_config, 
                task_gid.as_str(), 
                Some(true), 
                None, 
                None, 
                None, 
                file_attachment
                ).await
            {
                Ok(res) => println!("response without data: {:?}", res.data.unwrap()),
                Err(e) => {
                    match e{
                        asana::apis::Error::Reqwest(e) => println!("reqwest error: {e}"),
                        asana::apis::Error::Serde(e) => println!("Serde error: {e}"),
                        asana::apis::Error::Io(e) => println!("IO error: {e}"),
                        asana::apis::Error::ResponseError(e) => println!("Response error: {:?}", e.content.as_str()),
                    }
                }
            }

        });
    }
}

async fn request_ticket_info(mut scaffold_builder: ScaffoldRequestBuilder, client: reqwest::Client)  
-> core::result::Result<GetTicketResponse, Box<dyn Error>> {

    // Now you can use the method on the instance of ScaffoldRequestBuilder
    let params: Value = scaffold_builder.build_scaffold_call();

    let response = client.post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await;

    match response {
        Ok(res) => {
            let json_response: GetTicketResponse = res.json().await?;
            /*
                let raw_response = res.text().await?;
                println!("Server response: {}", raw_response);
                let json_response: GetTicketResponse = serde_json::from_str(&raw_response).unwrap();
            */
           Ok(json_response)
        },
        Err(e) => Err(Box::new(e)),
    }
}

async fn request_keys(mut scaffold_builder: ScaffoldRequestBuilder, client: reqwest::Client)  
-> core::result::Result<GetKeysResponse, Box<dyn Error>> {

        let params: Value = scaffold_builder.build_scaffold_call();

        let response = client.post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&params)
            .send()
            .await;
    
            match response {
                Ok(res) => {
                    let response_text = res.text().await?;// serde_json::from_str(&raw_response)?;
                    //println!("response: {:?}", response_text);
        
                    let mut webroot_key = "";
                    let mut superanti_key = "";

                    let lines: Vec<&str> = response_text.split("\n").collect();
                    for line in lines {
                        let parts: Vec<&str> = line.split(": ").collect();
                        if parts.len() >= 2 {
                            let prefix = parts[0].trim();
                            let key = parts[1].trim();
                            match prefix {
                                "WRAV" => webroot_key = key,
                                "SAS" => superanti_key = key,
                                _ => (),
                            }
                        }
                    }
                    

                    let response_keys = GetKeysResponse {
                        webroot_key: webroot_key.to_string(),
                        superanti_key: superanti_key.to_string(),
                    };

                    
                    Ok(response_keys)
                },
                Err(e) => Err(Box::new(e)),
            }
}




//pub async fn request_seb_info(cust_id: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {}

//pub async fn get_computer_purchases(cust_id: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {}

/*
        match response {
            Ok(mut res) => {
                let total: u64 = res.headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|len| len.to_str().ok())
                    .and_then(|number| number.parse().ok())
                    .unwrap_or(0);
        
                let mut downloaded: u64 = 0;
                let mut data = bytes::BytesMut::new();
        
                while let Ok(chunk_result) = res.chunk().await {
                    match chunk_result {
                        Some(chunk) => {
                            downloaded += chunk.len() as u64; // Here we update our downloaded count with the size of the chunk
                            data.extend_from_slice(&chunk);
                
                            if total > 0 {
                                let progress = (downloaded as f64 / total as f64 * 100.0) as u32;
                                progress_bytes = progress;
                                println!("progress: {:?}", progress_bytes);
                            }
                        },
                        None => break, // The stream has ended
                    }
                }
                
        
                let json_response: GetTicketResponse = serde_json::from_slice(&data)?; // Parse the buffered data
        
                Ok(json_response)
            },
            Err(e) => Err(Box::new(e)),
        }
 */

//     let params = serde_json::json!({
//         "user_email": user,
//         "password": pass,
//         "call": "getOrder", 
//         "action": "everest_call",
//         "application": "everest", 
//         "arg1": so_number, 
//         "arg2": "false", 
//         "company": "pcl"
//      });    


