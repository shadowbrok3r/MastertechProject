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
pub struct ResponseData {
    some_key: String,
    // more fields...
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    message: String,
    data: ResponseData,
    // more fields...
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
                let api_response: ApiResponse = res.json().await?;
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