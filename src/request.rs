use reqwest::header::{CONTENT_TYPE, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::error::Error;

use crate::scaffold_builder::*;

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
    pub INV_AMOUNT: String, // "INV_AMOUNT": "53.6100",
}

#[derive(Deserialize, Debug)]
pub struct Customer {
    pub NAME: String, // "NAME": "Timber Ridge Fireplace LLC",
    //pub CUSTOMER_ADDRESS: String,
    pub LI_DOC: String, //"LI_DOC": "53745333",
    pub LI_AMT: String,  //"LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    //pub LAST_TUNEUP_DATE: String, // <-- HERE
    pub DW_UPDATE_DATE: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    pub NUM_INV: String, // "NUM_INV": "21",
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

pub async fn request_ticket_info(mut scaffold_builder: ScaffoldRequestBuilder)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {
    // Now you can use the method on the instance of ScaffoldRequestBuilder
    let params: Value = scaffold_builder.build_scaffold_call();

    //println!("{:?}", json_string);
    let response = reqwest::Client::new().post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await; //need to find a way for this to return the response not the result

        match response {
            Ok(res) => {
                let json_response: GetTicketResponse = res.json().await?;// serde_json::from_str(&raw_response)?;
                //let raw_response = res.text().await?;
                //println!("Server response: {}", raw_response);
                //let json_response: GetTicketResponse = serde_json::from_str(&raw_response)?;

                Ok(json_response)
            },
            Err(e) => Err(Box::new(e)),
        }
}

pub async fn request_keys(mut scaffold_builder: ScaffoldRequestBuilder)  -> core::result::Result<GetKeysResponse, Box<dyn Error>> {
        // Now you can use the method on the instance of ScaffoldRequestBuilder
        let params: Value = scaffold_builder.build_scaffold_call();

        //println!("{:?}", json_string);
        let response = reqwest::Client::new().post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&params)
            .send()
            .await; //need to find a way for this to return the response not the result
    
            match response {
                Ok(res) => {
                    let response_text = res.text().await?;// serde_json::from_str(&raw_response)?;
                    println!("response: {:?}", response_text);
                    // Assume `response_text` is the string response you got
                    let lines = response_text.split("\n");  // Split by line
                    let mut webroot_key = "";
                    let mut superanti_key = "";

                    for line in lines {
                        let parts: Vec<&str> = line.split(": ").collect();  // Split each line by ": "
                        if parts.len() == 2 {
                            match parts[0] {
                                "WRAV" => webroot_key = parts[1].trim(), // .trim() to remove leading/trailing spaces
                                "SAS" => superanti_key = parts[1].trim(), // .trim() to remove leading/trailing spaces
                                _ => {}
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

//

//pub async fn get_computer_purchases(cust_id: String)  -> core::result::Result<GetTicketResponse, Box<dyn Error>> {}



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


/*Bounded channel: If you need a bounded channel, you should use a bounded Tokio mpsc channel for both directions of communication. 
Instead of calling the async send or recv methods, in synchronous code you will need to use the blocking_send or blocking_recv methods.

Unbounded channel: You should use the kind of channel that matches where the receiver is. So for sending a message from async to sync, 
you should use the standard library unbounded channel or crossbeam. Similarly, for sending a message from sync to async, you should use an unbounded Tokio mpsc channel.

Please be aware that the above remarks were written with the mpsc channel in mind, but they can also be generalized to other kinds of channels. 
In general, any channel method that isn’t marked async can be called anywhere, including outside of the runtime. For example, sending a message on a 
oneshot channel from outside the runtime is perfectly fine. */


