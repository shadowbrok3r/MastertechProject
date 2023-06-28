use std::borrow::BorrowMut;

use egui::Ui;
use pollster::FutureExt;
use reqwest::header::{CONTENT_TYPE, ACCEPT};

struct post_request{
    //user_email_passw: [(String, String); 2],
    //call: String,
    //action: String,
    //application: String,
    //arg1: String,
    //arg2: String,
    //company: String,

    // Sender/Receiver for async notifications.

}

/*impl Default for post_request{
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
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

pub async fn request_ticket_info(tx: tokio::sync::watch::Sender<Option<Result<String, reqwest::Error>>>,
    mut text_to_update: String) {//, output_console_text: &mut String, service_order_num: &mut String) -> Result<(), reqwest::Error> {
    let params = [
            //("user_email", "logan.lees@pclaptops.com"), 
            //("user_password", "Poolparty1"),
            //("call", "getOrder"),
            //("action", "everest_call"),
            //("application", "everest"),
            ("arg1", "52886482"),
            ("arg2", "false"),
            ("company", "pcl")
        ];

  
        let client = reqwest::Client::new();

        let resp = client.post("https://api.spotify.com/v1/search")//("https://scaffold.pclaptops.com/api/index")
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&params).send().await;
    

        // Convert the response to a string and send the result through the channel.
    let resp_string = match resp {
        Ok(mut response) => Some(Ok(response.text().await.unwrap_or_else(|_| String::from("Failed to read response text")))),
        Err(e) => {
            eprintln!("Error: {}", e);
            None
        }
    };

    match tx.send(resp_string) {
        Ok(_) => (),
        Err(e) => eprintln!("Error while sending the response: {}", e),
    };
    

        //update_output_text(&mut ui, &resp_body);
        //let progress_bar = egui::ProgressBar::new(100.0).show_percentage().desired_width(30.0).fill(egui::Color32::LIGHT_GREEN);
        //ui.add(progress_bar);
}


