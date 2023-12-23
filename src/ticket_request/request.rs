
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use crossbeam::channel;
use async_trait::async_trait;
use reqwest::{header::{CONTENT_TYPE, ACCEPT}, multipart::{Form, Part}};
use serde::{Deserialize, Serialize};
use serde_json::*;
use tokio::io::AsyncWriteExt;
use xml::{EventReader, reader::XmlEvent};
use std::{error::Error, path::PathBuf, fs::File, io::BufReader};
use log::{info, debug, trace, error};
use crate::{scaffold::*, data::{PulledKeys, PreTicketData}, ticket_request::AddressObject};
use std::result::Result;
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

use super::{GetTicketResponse, GetKeysResponse, AsanaResponse, Store, request_builder::{AsanaTask, TaskAssignee}};

pub struct SendRequest {
    pub tx: std::sync::mpsc::Sender<String>,
}

#[async_trait]
pub trait SendReq<T>{
    async fn retrieve_data(so_number: &str, client: reqwest::Client) -> Result<T, Box<dyn Error>>;
}

#[async_trait]
impl SendReq<GetTicketResponse> for SendRequest{
    async fn retrieve_data(so_number: &str, client: reqwest::Client) -> Result<GetTicketResponse, Box<dyn Error>> {
        todo!()
    }
}

#[async_trait]
impl SendReq<GetKeysResponse> for SendRequest{
    async fn retrieve_data<'a>(so_number: &'a str, client: reqwest::Client) -> Result<GetKeysResponse, Box<dyn Error>> {
        todo!()
    }
}




impl SendRequest{
    pub fn get_ticket(
        so_number: String, 
        tx: std::sync::mpsc::Sender<String>, 
        client: reqwest::Client)
    {
        debug!("Getting Ticket");
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
                                    let note = &parts[1].to_string();
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
            
            let response = request_keys(scaffold_builder, client)
                .await;

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
                            debug!("Error while sending ticket information: {}", e.to_string());
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
                            debug!("Error while sending error message: {}", e);
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
        asana_task: AsanaTask,
        due_date: String,
    ) 
    {
        let (sender, receiver) = channel::bounded::<String>(5);
        let send = tx.clone();

        let mut assigned_salesman = "1202792432658520".to_string(); // Jake
        let mut assigned_tech = "1199992640930465".to_string(); // Logan

        match asana_task.assignee.salesman{
            Salesman::Jake => {assigned_salesman = "1202792432658520".to_string()},
            Salesman::Danny => {assigned_salesman = "1202791016369879".to_string()},
        };

         match asana_task.assignee.tech{
            Techs::Logan => { assigned_tech = "1199992640930465".to_string()},
            Techs::Bread => { assigned_tech = "1202792432551073".to_string()},
            Techs::Taco => { assigned_tech = "1202792432421640".to_string()},
        };

        let asana_response = AsanaResponse{
            gid: Some("".to_string()),
            //created_at: Some("".to_string()),
            status: Some(200),
            //raw_resp: Some("".to_string())
        };

        tokio::spawn(async move{
            // ideally, id like to also add the functionality to search for a task by the SO number
            // so we can update the ticket or delete it, or add an attachment
            // i should use create_attachment_for_task,
            //  dependencies: Option<Vec<AsanaResource>> 
            // to add spo as dependancy to task

            let mut asana_config = Configuration::new();
            let mut task = task_request::TaskRequest::new();

            asana_config.client = client.clone();
            asana_config.bearer_access_token = Some("1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2".to_string());
            asana_config.user_agent = None;
            
            task.name = Some(asana_task.task_name);// Some(format!("{cust} - {so_num}"));
            
            task.workspace = Some("13314583095021".to_string());
            if !cfg!(debug_assertions){
                task.projects = Some(vec!["1202792139600600".to_string()]);
                task.assignee = Some(assigned_salesman.clone());
            }else{
                task.assignee = Some("1199992640930465".to_string());
            }
            
            task.followers = Some(vec![assigned_salesman, assigned_tech]); // Logan: 1199992640930465
            task.html_notes = Some(asana_task.html_notes);
            task.due_on = Some(due_date);
            //task.resource_subtype = Some("".to_string());
            //task.dependencies = Some("".to_string());

            let new_task = 
            InlineObject35{ data: Some(Box::new(task)) };

            // Serialize the html_notes to a JSON string and calculate its length
            let html_notes_json = serde_json::to_string(&new_task).unwrap();
            let content_length = html_notes_json.len();
            
            match create_task(&asana_config, 
                new_task, 
                Some(true), //only set to true if debugging
                None
                ).await
            {
                Ok(res) => 
                {
                    match res.data
                    {
                        Some(data) =>
                        {
                            let file = asana_task.file_attachment.clone();

                            if let Some(file) = file
                            {
                                let attachment_client = client.clone();

                                match data.gid
                                {
                                    Some(gid) => 
                                    {
                                        let file_name = file.file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("no file name");
                                        
                                        let file_attachment = asana_task.file_attachment.clone();
                                        let new_path = file_attachment.as_ref().map(|p| p.as_path().to_owned());
                                        
                                        let byte_content = tokio::fs::read(new_path.unwrap()).await.unwrap();
                                        let part = Part::bytes(byte_content).file_name(format!("{file_name}"));

                                        let form = Form::new()
                                        .part("file", part)
                                        .text("parent", gid);

                                        let response = attachment_client
                                        .post("https://app.asana.com/api/1.0/attachments")
                                        .header("Authorization", "Bearer 1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2")
                                        .header(ACCEPT, "application/json")
                                        .multipart(form)
                                        .send()
                                        .await;
                
                
                                        match response
                                        {
                                            Ok(resp) => 
                                            {
                                                // error!("{resp:?}");
                                                let asana_response: AsanaResponse = resp.json().await.unwrap(); 
                                                // error!("{asana_response:?}");

                                                match sender.send(serde_json::to_string(&asana_response).unwrap()){
                                                    Ok(_) => drop(sender),
                                                    Err(e) => error!("error sending message: {e}"),
                                                }
                                            },
                                            Err(e) => error!("{e:?}") 
                                        }
                                    }, 
                                    None => error!("no gid received")
                                    
                                }
                            }

                        }, None => error!("no data")
                    }               
                },
                Err(e) => {
                    match e{
                        asana::apis::Error::Reqwest(e) => error!("reqwest error: {e}"),
                        asana::apis::Error::Serde(e) => error!("Serde error: {e}"),
                        asana::apis::Error::Io(e) => error!("IO error: {e}"),
                        asana::apis::Error::ResponseError(e) => {
                            let send_tx = tx.clone();
                            match send_tx.send(e.content){
                                Ok(_) => { info!("sent error successfully"); drop(send_tx); },
                                Err(e) => error!("send error: {e}")
                            };
                        }
                    }
                }
            }
        });

        if let Ok(message) = receiver.recv(){
            let msg = message.clone();
            trace!("message: {msg}");
            match send.send(msg){
                Ok(_) => drop(send),
                Err(e) => error!("{e}")
            }
            info!("received: {message}");
        }
    }
}

async fn request_ticket_info(mut scaffold_builder: ScaffoldRequestBuilder, client: reqwest::Client)  
-> core::result::Result<GetTicketResponse, Box<dyn Error>> {
    debug!("request_ticket_info");
    // Now you can use the method on the instance of ScaffoldRequestBuilder
    let params: Value = scaffold_builder.build_scaffold_call();

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
            Ok(json_response)
        },
        Err(e) => {
            debug!("Boxed error: {e:?}");
            Err(Box::new(e))
        },
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
                    debug!("response: {:?}", response_text);
        
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

pub fn request_seb_info(){ // cust_id: String -> core::result::Result<GetTicketResponse, Box<dyn Error>> {
    // supereasybackup.com/downloads/SuperEasyBackup.exe
    let file = File::open("D:\\Users\\Owner\\Desktop\\SEB\\DCProtectData-Customer\\Shared\\Logs\\InstallationTracking.log").unwrap();
    let file = BufReader::new(file);

    let parser = EventReader::new(file);
    let mut depth = 0;
    for e in parser {
        match e {
            Ok(XmlEvent::StartElement { name, .. }) => {
                // XmlEvent::ProcessingInstruction { name, data: () }
                /*
                    <InstalledDeviceId>8a8e1c4f-83ff-47cf-96fb-94b31695f68a</InstalledDeviceId>
                    <UtcClientTime>2023-12-21 23:22:00</UtcClientTime>
                    <LocalClientTime>2023-12-21 15:22:00</LocalClientTime>
                    <InstallInstanceId>1fc3b934-21ea-42a9-893a-4be7c335fcb9</InstallInstanceId>
                    <HasIssues>false</HasIssues>
                    <InstallationStage>Activated</InstallationStage>
                    <ReasonCode>0</ReasonCode>
                    <ActivationCode>6894-1dbe-ad8f-e320-d94d</ActivationCode>
                    <ProcessUsername>NT AUTHORITY\SYSTEM</ProcessUsername>
                    <ProcessUserIsMemberOfAdministratorsGroups>true</ProcessUserIsMemberOfAdministratorsGroups>
                    <InstallVersion>10.6.2.296</InstallVersion>
                    <MachineName>OWNERPC</MachineName>
                */
                println!("{:spaces$}+{name}", "", spaces = depth * 2);
                depth += 1;
            }
            Ok(XmlEvent::EndElement { name }) => {
                depth -= 1;
                println!("{:spaces$}-{name}", "", spaces = depth * 2);
            }
            Err(e) => {
                eprintln!("Error: {e}");
                break;
            }
            // There's more: https://docs.rs/xml-rs/latest/xml/reader/enum.XmlEvent.html
            _ => {}
        }
    }

}

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
                                info!("progress: {:?}", progress_bytes);
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


