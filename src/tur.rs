use std::borrow::BorrowMut;

use egui::Ui;
use reqwest::header::{CONTENT_TYPE, ACCEPT};

use crate::MasterTechApp;
//use tokio::sync::mpsc::{Sender, Receiver};
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
//#[tokio::main]


// TODO
//// I Could maybe make the state of the button a bool and then use that to determine if the button is pressed or not,
/// Then i could check to see if the button is pressed and then run the function that i want to run inside of the main loop?
/// Then i could request_ticket_info() and store the ctx.clone from the main loop and then use that inside the request_ticket_info() 
/// to update the ui with the ticket info
/// 
pub async fn request_ticket_info(tx: std::sync::mpsc::Sender<u32>, ctx: egui::Context) //, output_console_text: &mut String, service_order_num: &mut String) -> Result<(), reqwest::Error> {
    -> Result<(), reqwest::Error> {

    let params = [
            ("user_email", "logan.lees@pclaptops.com"), 
            ("user_password", "Poolparty1"),
            ("call", "getOrder"),
            ("action", "everest_call"),
            ("application", "everest"),
            ("arg1", "52886482"),
            ("arg2", "false"),
            ("company", "pcl")
        ];

    //let update_output_text = main_context.output_text.clone();


    
    let client = reqwest::ClientBuilder::new().build()?;

    let response = client.post("https://scaffold.pclaptops.com/api/index")
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params).send().await?;
    
        match response.status() {
            reqwest::StatusCode::OK => {
                println!("Success!");
                let resp_body = response.text().await?;
                //resp_body.find("path").unwrap();
                println!("resp_body: {}", resp_body);
                ui.label(&resp_body);
                //output_console_text.push_str(&resp_body);
            },
            reqwest::StatusCode::UNAUTHORIZED => {
                println!("Need to grab a new token");
            },
            _ => {
                panic!("Uh oh! Something unexpected happened.");
            },
        };

        ctx.request_repaint();
         //update_output_text(&mut ui, &resp_body);
    //let progress_bar = egui::ProgressBar::new(100.0).show_percentage().desired_width(30.0).fill(egui::Color32::LIGHT_GREEN);
    //ui.add(progress_bar);
    Ok(())
}


