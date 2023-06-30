use reqwest::header::{CONTENT_TYPE, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::error::Error;
/*impl Default for post_request{
    fn default() -> Self {
        post_request {
            user_email_passw: [
                ("user_email".to_string(), "logan.lees@pclaptops.com".to_string()), 
                ("user_password".to_string(), "Poolparty1".to_string())
            ],

            call: "getOrder".to_string(),
            action: "everest_call".to_string(),
            application: "everest".to_string(),
            arg1: "".to_string(),
            arg2: "".to_string(),
            company: "pcl".to_string()
            
        }
        
    }
}*/

#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    main_json: MainJson,
    header: Header,
    customer: Customer,
    transactions: TransacObjectOne,
    addresses: Addresses,
    items: ItemObjects,
}

#[derive(Deserialize, Debug)]
struct MainJson {
    header: Header,
    customer: Customer,
    transactions: Transactions,
    addresses: Vec<Addresses>,
    items_array: Vec<ItemsArray>,
}

#[derive(Deserialize, Debug)]
struct Header {
    cust_code: String,
    user_id: String,
    terms: String, // "TERMS": "CC",
    doc_alias: String, // "DOC_ALIAS": "SERVICE ORDER",
    department: String, // "DEP": "LTN"
    jurisdiction: String, //"JURISCODE": "LTN",
    invoice_amnt: String, // "INV_AMOUNT": "53.6100",
}

#[derive(Deserialize, Debug)]
struct Customer {
    name: String, // "NAME": "Timber Ridge Fireplace LLC",
    address: String,
    last_invoice_number: String, //"LI_DOC": "53745333",
    
    last_invoice_date: String,  //"LI_AMT": "53.6100", //I COULD USE THIS TO CHECK LAST TUNEUP
    last_tuneup_date: String, // <-- HERE
    last_checkin_date: String, // "DW_UPDATE_DATE": "2023-06-27 13:38:50.440",
    total_invoice_count: String, // "NUM_INV": "21",
/*		"LP_AMT": "-53.6100",
		"LP_DOC": "52883815",
		"LP_DOC_TYP": "8",
		"LP_DATE": "2023-05-04 00:00:00.000", */
}

#[derive(Deserialize, Debug)]
struct Transactions{
    transac_obj_one: TransacObjectOne,
}

#[derive(Deserialize, Debug)]
struct TransacObjectOne{
/*

"TRANHIST_DATE": "2023-05-04 14:25:36.000",
"USER_ID": "KMJ",
"AMOUNT": "53.6100",
"PAY_TYPE": "LTNVM",
"DESCRIPT": "PAYMENT RECEIVED ON SALES ORDER",
 */
}

#[derive(Deserialize, Debug)]
struct Addresses {
/*
"ACCT_NAME": "Timber Ridge Fireplace LLC",
"NAME": "Timber Ridge Fireplace LLC",
"LAST_NAME": "Hale",
"FIRST_NAME": "Lisa",
"TEL1": "8018376254",
"TEL2": "",
"EMAIL": "sales@trfireplace.com",
"MOBILE_PHONE": "8013501447",
"ADDRESS_LINE1": "3080 N Fairfield Rd Suite #1",
 */
}

#[derive(Deserialize, Debug)]
struct ItemsArray{ // Okay, so the number of items is the number of item codes you have on an order....  
    //so i may need to iterate through them to get all line items. especially if i check for a new build
   item_objects: Vec<ItemObjects>,
}

#[derive(Deserialize, Debug)]
struct ItemObjects{
    //object_one: Option<String>,// Item_code //I should pull the ITEM_CODE here too ("brand/pcl"), this could also get srvc/etc
    //object_two: String, // NOTE (which is likely null i guess)
/*
   
////////////////////////////////////    ARRAY ONE
"ITEM_CODE": "BRAND-PCL",
"X_INVOICE_ID": "16994221",
"ITEM_QTY": "1.000000",
"QTY_SHIP": "1.000000",
"ITEM_PRICE": ".000000",
"serials": [] //each of these in the 'Items' array of objects store all serials attached
}, {

////////////////////////////////////    ARRAY TWO
"NOTE": "This service,


////////////////////////////////////    ARRAY THREE

"ITEM_CODE": "SRVC/TUNEUP/PCL",
"DESC_TYPE": "1",
"ITEM_QTY": "1.000000",
"QTY_SHIP": "1.000000",
"ITEM_PRICE": "159.990000",
"DISCOUNT_V": "159.990000",
"SALES_REP": "KMJ",
		"STK_ITEM_QTY": "1.000000",
		"STK_QTY_SHIP": "1.000000",

////////////////////////////////////    ARRAY FOUR


"ITEM_CODE": "SW/PCLCPS/O",
"DESC_TYPE": "1",
"ITEM_QTY": "1.000000",
"QTY_SHIP": "1.000000",

"COST": "7.100000",
"MISC_COST": "1.420000", 
"C_COST": "7.100000", //I CAN SEE COST
"ITEM_PRICE": "49.990000", // VS WHAT WE CHARGED
"FACTORED_COST_PER": "20.000000", // ????




#[derive(Deserialize, Debug)]
struct ItemsObjectTwo{
    checkin_notes: String, // NOTE <-- Bingo
    object_one: Option<String>, 
    object_two: String, 
}

#[derive(Deserialize, Debug)]
struct ItemsObjectThree{
    checkin_notes: String,
    object_one: Option<String>,// Item_code //I should pull the ITEM_CODE here too ("brand/pcl"), this could also get srvc/etc
    object_two: String, // NOTE
}
 */
}


//tx: watch::Sender<Option<Result<String, reqwest::Error>>>
pub async fn request_ticket_info(so_number: String)  -> core::result::Result<ApiResponse, Box<dyn Error>> {
    let params = serde_json::json!({
        "user_email": "logan.lees@pclaptops.com",
        "user_password": "Poolparty1", 
        "call": "getOrder", 
        "action": "everest_call",
        "application": "everest", 
        "arg1": so_number, 
        "arg2": "false", 
        "company": "pcl"
});    
    
let response = reqwest::Client::new().post("https://scaffold.pclaptops.com/api/index") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await; //need to find a way for this to return the response not the result

        match response {
            Ok(res) => {
                //let api_response = res.text().await?;
                //let api_response: ApiResponse = res.json().await?;
                    // Lets do all of the unwrapping of the different objects
                    //example of us just pulling one string at a time VV
                //let cust_code = api_response.main_json.header.cust_code; 

                let api_response: ApiResponse = res.json().await?;


                let main_json = api_response.main_json; //iterate through this? for loop in for loop

                let header = api_response.header;
                let customer = api_response.customer;
                let transactions = api_response.transactions;
                let addresses = api_response.addresses;
                //let items = api_response.items;


                for addr_arr in main_json.addresses{
                    println!("info: {:?}", addr_arr);
                }






                //let header: Header = serde_json::from_str(&api_response.header);

                Ok(api_response)
            },
            Err(e) => Err(Box::new(e)),
        }
    //Ok(())
}
/*Bounded channel: If you need a bounded channel, you should use a bounded Tokio mpsc channel for both directions of communication. 
Instead of calling the async send or recv methods, in synchronous code you will need to use the blocking_send or blocking_recv methods.

Unbounded channel: You should use the kind of channel that matches where the receiver is. So for sending a message from async to sync, 
you should use the standard library unbounded channel or crossbeam. Similarly, for sending a message from sync to async, you should use an unbounded Tokio mpsc channel.

Please be aware that the above remarks were written with the mpsc channel in mind, but they can also be generalized to other kinds of channels. 
In general, any channel method that isn’t marked async can be called anywhere, including outside of the runtime. For example, sending a message on a 
oneshot channel from outside the runtime is perfectly fine. */