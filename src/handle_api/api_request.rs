
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use chrono::{DateTime, Utc, SecondsFormat};
use crossbeam::channel;
use async_trait::async_trait;
use reqwest::header::{CONTENT_TYPE, ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::*;
use tokio::{io::AsyncWriteExt, sync::mpsc::error::TryRecvError};
use quick_xml::{Reader, events::Event, name::QName};
use quick_xml::de::from_str;
use std::time::{Duration, Instant};
use std::{error::Error, path::PathBuf, fs::{File, self}, io::BufReader, collections::HashMap};
use log::{info, debug, trace, error};
use crate::database::schema::{ExtendedSeb, LocalSebData};
use crate::{scaffold::*, database::{PreTicketData, GetKeysResponse}, handle_api::AddressObject};
use std::result::Result;
use std::fmt::Debug;
use super::{GetTicketResponse, AsanaResponse, Store, email_builder::{AsanaTask, TaskAssignee}};

pub struct SendRequest {
    pub tx: crossbeam::channel::Sender<String>,
}

impl SendRequest{
    pub fn get_ticket(
        so_number: String, 
        tx: crossbeam::channel::Sender<String>, 
        client: reqwest::Client)
    {
        debug!("Getting Ticket");
        tokio::spawn(async move{
            // Await the response 
            let response: Result<GetTicketResponse, Box<dyn Error>> = request_ticket_info(so_number, client).await;

            // Handle the response
            match response { // Successfully received GetTicketResponse
                Ok(get_ticket_response) => {
                    debug!("get_ticket_response -> {get_ticket_response:?}");
                    let header = get_ticket_response.header;
                    let customer = get_ticket_response.customer;
                    let addresses = get_ticket_response.addresses;
                    let items_objects = get_ticket_response.items;
                    //let transactions = &get_ticket_response.transactions;

                    let mut checkin_note = String::new();
                    let mut itemcodes = String::new();

                    // iterates through the array of objects, gets note if not null and not empty, parses, assigns to checkin_note
                    for object in items_objects{
                        let x = object.clone();
                        // If i want to....
                        // "COST": "7.100000", this is our cost
                        // ITEM_PR_FEX is what we charge the customer, although AMOUNT is the same value
                        x
                        .unwrap_or("empty".into())
                        .get("NOTE")
                        .and_then(|v| v.as_str())
                        .map(|note| {
                            if note != "null" && !note.is_empty() {
                                let parts: Vec<&str> = note.split("Symptoms (Details):").collect();
                                if parts.len() > 1{
                                    let note = &parts[1].trim().to_string();
                                    checkin_note = note.to_string();
                                }
                            }
                        });
                        

                        object
                            .unwrap_or("".into())
                            .get("ITEM_CODE")
                            .and_then(|v| v.as_str())
                            .map(|item_code| {
                                itemcodes += &format!("{item_code}\n").to_string();
                            });
                    }

                    /// Collects non-None values into a vector.
                    /// 
                    /// # Arguments
                    /// * `values` - A mutable reference to a vector of strings where the values will be collected.
                    /// * `value` - An Option<String> that may contain a value to be collected.
                    fn collect_values(values: &mut Vec<String>, value: Option<String>) {
                        // Check if the value is Some and, if so, push it into the provided vector.
                        // The `if let` syntax is a concise way to handle `Option` types that are `Some`.
                        if let Some(v) = value {
                            values.push(v);
                        }
                    }


                    // Temporary vectors to collect all non-None values for each field.
                    let mut temp_tel1 = Vec::new();
                    let mut temp_tel2 = Vec::new();
                    let mut temp_email = Vec::new();

                    // Iterate through the addresses and collect all non-None values.
                    for address in addresses.into_iter().flatten() {
                        // Collect non-None TEL1 values.
                        collect_values(&mut temp_tel1, address.TEL1);
                        // Collect non-None TEL2 values.
                        collect_values(&mut temp_tel2, address.TEL2);
                        // Collect non-None EMAIL values.
                        collect_values(&mut temp_email, address.EMAIL);
                    }

                    // Initialize address_object with the last value from each collection,
                    // which is assumed to be the most recent or relevant.
                    let address_object = AddressObject {
                        TEL1: temp_tel1.pop(), // Get and remove the last TEL1 value, if any.
                        TEL2: temp_tel2.pop(), // Get and remove the last TEL2 value, if any.
                        EMAIL: temp_email.pop(), // Get and remove the last EMAIL value, if any.
                    };

                    // The remaining values in the temporary vectors are assigned to the extra vectors.
                    // These are the primary phone numbers and emails, excluding the most recent ones assigned to address_object.
                    let extra_tel1 = temp_tel1;
                    let extra_tel2 = temp_tel2;
                    let extra_email = temp_email;
                    
                    let mut originating_store: Store = Store::None;
                    if let Some(store) = header.DEP{
                        originating_store = store;
                    }

                    let ticket_information = PreTicketData{
                        cust_code: header.CUST_CODE.unwrap_or("empty".to_string()),
                        checkin_rep: header.USER_ID.unwrap_or("empty".to_string()),
                        customer_phone_1: address_object.TEL1.unwrap_or("empty".to_string()),
                        customer_phone_2: address_object.TEL2.unwrap_or("empty".to_string()),
                        customer_email: address_object.EMAIL.unwrap_or("empty".to_string()),
                        last_invoice_amount: customer.LI_AMT.unwrap_or("empty".to_string()),
                        terms: header.TERMS.unwrap_or("empty".to_string()),
                        doc_alias: header.DOC_ALIAS.unwrap_or("empty".to_string()),
                        dep: originating_store ,
                        jurisdiction: header.JURISCODE.unwrap_or("empty".to_string()),
                        ticket_total: header.INV_AMOUNT.unwrap_or("empty".to_string()),
                        customer_name: customer.NAME.unwrap_or("empty".to_string()),
                        checkin_notes: checkin_note,
                        last_invoice_number: customer.LI_DOC.unwrap_or("empty".to_string()),
                        item_codes: itemcodes.clone(),
                        total_invoice_count: customer.NUM_INV.unwrap_or("empty".to_string()),
                        due_date: None,
                        sales_rep: header.SALES_REP.unwrap_or("empty".to_string())
                    };
                    
                    let ticket_info_json = serde_json::to_string(&ticket_information).unwrap_or("No Ticket Information".to_string());

                    match tx.send(ticket_info_json) {
                        Ok(_) => drop(tx),
                        Err(e) => {
                            debug!("Error while sending ticket information: {}", e.to_string());
                            drop(tx)
                        }
                    }
                    
                },
                Err(e) => { 
                    debug!("response error -> {e:?}");
                    match tx.send(e.to_string()) {
                        Ok(_) => {
                            drop(tx)
                        },
                        Err(e) => {
                            debug!("Error while sending error message: {}", e);
                            drop(tx)
                        }
                    }
                }
            }
        });
        
    }
       
    pub async fn get_cps(so_number: String, client: reqwest::Client) 
        -> anyhow::Result<GetKeysResponse, anyhow::Error>
    {

            let params: Value = serde_json::json!({
                "user_email": "logan.lees@pclaptops.com", 
                "user_password": "Poolparty1",
                "action": ScaffoldActions::FetchKeys,
                "application": ScaffoldApps::SoftwareLicenseFetch, 
                "company": "pcl",
                "id_order": so_number,
            });

            let response = client.post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .json(&params)
                .send()
                .await?;
        
            let mut response_text = response.text().await?;
            debug!("response: {:?}", response_text);

            let mut webroot_key = "";
            let mut superanti_key = "";

            if response_text.contains("WRAV: ") || response_text.contains("SAS: "){
                let wrav_offset = response_text.find("WRAV: ").unwrap_or(response_text.len());

                response_text.drain(..wrav_offset); 

                let split_lines: Vec<&str> = response_text.split("\nSAS: ").collect();

                let split_wrav: Vec<&str> = split_lines[0].split("WRAV: ").collect();

                webroot_key = split_wrav[1].trim();
                superanti_key = split_lines[1].trim();
            }
            else{
                webroot_key = "Error";
                superanti_key = "Check console";
            }

            let response_keys = GetKeysResponse {
                webroot_key: webroot_key.to_string(),
                superanti_key: superanti_key.to_string(),
            };
        Ok(response_keys)
    }

    pub async fn send_ticket_request(
        tx: crossbeam::channel::Sender<String>, 
        client: reqwest::Client, 
        asana_task: AsanaTask,
        due_date: DateTime<Utc>,
    ) -> anyhow::Result<(), anyhow::Error>
    {
        let send = tx.clone();

        let mut assigned_salesman = "1202792432658520".to_string(); // Jake
        let mut assigned_tech = "1199992640930465".to_string(); // Logan

        match asana_task.assignee.salesman{
            Salesman::Jake => {assigned_salesman = "1202792432658520".to_string()},
            Salesman::Danny => {assigned_salesman = "1202791016369879".to_string()},
        };

         match asana_task.assignee.tech{
            Techs::Logan => { assigned_tech = "1199992640930465".to_string()},
            Techs::Bread => { assigned_tech = "1202792432421640".to_string()},
            Techs::Taco => { assigned_tech = "1202792432551073".to_string()},
        };



        let params = serde_json::json!({
            "data": {
                "name": asana_task.task_name,
                "html_notes": asana_task.html_notes,
                "followers": [
                    assigned_salesman,
                    assigned_tech
                ],
                "due_at": due_date.to_rfc3339_opts(SecondsFormat::Secs, true),
                "workspace": "13314583095021",
                "assignee": assigned_salesman,
                "projects": ["1202792139600600"]
            }
        });

        
        let response = client
            .post("https://app.asana.com/api/1.0/tasks") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, "Bearer 1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2")
            .json(&params)
            .send()
            .await?;
        
        let res_body: Value = response.json().await?;
        debug!("Asana Response Body: {res_body:?}");
        let gid: Value = res_body.get("gid").unwrap_or(&Value::default()).clone();

        debug!("Asana Response: {gid:?}");

        let file = asana_task.file_attachment.clone();

        if let Some(file) = file{
            let file_name = file.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("no file name");
            
            let file_attachment = asana_task.file_attachment.clone();
            let new_path = file_attachment.as_ref().map(|p| p.as_path().to_owned());
            
            // let byte_content = tokio::fs::read(new_path.unwrap()).await.unwrap();
            // let part = Part::bytes(byte_content).file_name(format!("{file_name}"));

            let mut form = HashMap::new();
            form.insert("file", "part"); //part
            form.insert("parent", "gid"); //text

            let response = client
                .post("https://app.asana.com/api/1.0/attachments")
                .header("Authorization", "Bearer 1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2")
                .header(ACCEPT, "application/json")
                .form(&form)
                .send()
                .await?;
        }
        
        Ok(())
        
    }
}

async fn request_ticket_info(so_number: String, client: reqwest::Client)  
-> core::result::Result<GetTicketResponse, Box<dyn Error>> {
    info!("request_ticket_info");

    let params: Value = serde_json::json!({
        "user_email": "logan.lees@pclaptops.com",
        "user_password": "Poolparty1",
        "action": "everest_call",
        "application": "everest",
        "call": "getOrder",
        "company": "pcl",
        "arg1": so_number.as_str(),
        "arg2": "false"
    });

    debug!("request_ticket_info -> params -> {params:?}");

    let response = client
        .post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await;

    match response {
        Ok(res) => {
            let json_response: GetTicketResponse  = res.json().await?;
            debug!("request_ticket_info -> Json_response -> {json_response:?}");

            Ok(json_response)
        },
        Err(e) => {
            debug!("Boxed error: {e:?}");
            Err(Box::new(e))
        },
    }
}

pub async fn request_seb_info<T>(client: reqwest::Client, customer_email: Option<String>) 
-> anyhow::Result<T, anyhow::Error> 
    where T: Debug + Serialize + for<'a> Deserialize<'a> + Clone + std::convert::From<LocalSebData>
{
    if let Some(customer_email) = customer_email{
        let params = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com",
            "user_password": "Poolparty1",
            "action": "search",
            "application": "carbonite",
            "search": customer_email.as_str()
        });

        let response = client.post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&params)
            .send()
            .await?;

        let response_json: Vec<T> = response.json().await?;
        
        Ok(response_json.get(0).unwrap().clone())
    }else{
        // supereasybackup.com/downloads/SuperEasyBackup.exe
        let file_path = "C:\\DCProtectData\\Shared\\Logs\\InstallationTracking.log"; // "D:\\Users\\Owner\\Desktop\\SEB\\DCProtectData-Customer\\Shared\\Logs\\InstallationTracking.log"; 

        // Read the file content
        let file_content = fs::read_to_string(file_path)?;

        // Deserialize the XML content
        let mut result: LocalSebData = from_str(&file_content)?;

        let params = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com",
            "user_password": "Poolparty1",
            "action": "search",
            "application": "carbonite",
            "search": result.InstalledDeviceId.as_str()
        });

        let response = client.post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&params)
            .send()
            .await?;

        let response_json: Vec<ExtendedSeb> = response.json().await?; // ExtendedSeb
        
        let actual_response = response_json.get(0);

        if let Some(extended_seb) = actual_response{
            debug!("Carbonite response: {extended_seb:#?}");

            result.ExtendedSeb = Some(extended_seb.clone());
        }

        let res: T = result.try_into()?;

        Ok(res)
    }
}

