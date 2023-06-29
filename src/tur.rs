use reqwest::header::{CONTENT_TYPE, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::watch;


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

pub async fn request_ticket_info(tx: watch::Sender<Option<Result<String, reqwest::Error>>>, so_number: String) {
    let params = serde_json::json!({
        //"user_email": "logan.lees@pclaptops.com",
        //"user_password": "Poolparty1", 
        //"call": "getOrder", 
        //"action": "everest_call",
        //"application": "everest", 
        //"arg1": so_number, 
        "arg2": "false", 
        //"company": "pcl"
});

    let client = reqwest::Client::new();
    let resp = client.post("https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io") //("https://scaffold.pclaptops.com/api/index")
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params).send().await;

    // Convert the response to a string and send the result through the channel.
    let resp_string = match resp {
        //Ok(response) => Some(Ok(response.text().await { //.unwrap_or_else(|_| String::from("Failed to read response text")))),
        Ok(response) => match response.text().await {
            Ok(text) => Some(Ok(text)),
            Err(e) => Some(Err(e)),
        }

        Err(e) => {
            eprintln!("Error: {}", e);
            None
        }
    };
    
    match tx.send(resp_string) {
        Ok(_) => println!("Response sent successfully!"),
        Err(e) => eprintln!("Failed to send the response: {}", e),
    }
    
}


