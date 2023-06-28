use std::borrow::BorrowMut;

use egui::Ui;
use pollster::FutureExt;
use reqwest::header::{CONTENT_TYPE, ACCEPT};
use tokio::sync::watch;


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

pub async fn request_ticket_info(tx: watch::Sender<Option<Result<String, reqwest::Error>>>, so_number: String) {
    let params = [
            ("user_email", "logan.lees@pclaptops.com"), 
            ("user_password", "Poolparty1"),
            ("call", "getOrder"),
            ("action", "everest_call"),
            ("application", "everest"),
            ("arg1", &so_number.to_string()),
            ("arg2", "false"),
            ("company", "pcl")
    ];

    let client = reqwest::Client::new();
    let resp = client.post("https://scaffold.pclaptops.com/api/index")
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params).send().await;

    // Convert the response to a string and send the result through the channel.
    let resp_string = match resp {
        Ok(response) => Some(Ok(response.text().await.unwrap_or_else(|_| String::from("Failed to read response text")))),

        Err(e) => {
            eprintln!("Error: {}", e);
            None
        }
    };
    
    tx.send(resp_string).unwrap();
        //update_output_text(&mut ui, &resp_body);
        //let progress_bar = egui::ProgressBar::new(100.0).show_percentage().desired_width(30.0).fill(egui::Color32::LIGHT_GREEN);
        //ui.add(progress_bar);
}


