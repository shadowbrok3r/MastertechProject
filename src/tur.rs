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
//#[tokio::main]
pub async fn request_ticket_info(mut text_to_update: String) -> Result<(), reqwest::Error> {//, output_console_text: &mut String, service_order_num: &mut String) -> Result<(), reqwest::Error> {
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

   

        //resp_body.find("path").unwrap();
        //println!("resp_body: {:?}", resp_body);

        tokio::spawn(async move {
            let client = reqwest::Client::new();
            //let my_fut = async{
                let response = client.post("https://scaffold.pclaptops.com/api/index")
                    .header(CONTENT_TYPE, "application/json")
                    .header(ACCEPT, "application/json")
                    .json(&params).send().await;
        
        
                let resp_body = response.expect("failure?");
                text_to_update.push_str(&resp_body.text().await.unwrap());
            //response.await;
             //tx.send(response).await.unwrap();
            
        });
       //};
        //response.json();
        //let result = my_fut.block_on();
        
        /*match response.status() {
            reqwest::StatusCode::OK => {
                println!("Success!");
                let resp_body = response.text().await;
                //resp_body.find("path").unwrap();
                println!("resp_body: {:?}", resp_body);
                //ui.label(&resp_body);
                //output_console_text.push_str(&resp_body);
            },
            reqwest::StatusCode::UNAUTHORIZED => {
                println!("Need to grab a new token");
            },
            _ => {
                panic!("Uh oh! Something unexpected happened.");
            },
        };*/


        //let _ = tx.send(response.json());

        
        //update_output_text(&mut ui, &resp_body);
        //let progress_bar = egui::ProgressBar::new(100.0).show_percentage().desired_width(30.0).fill(egui::Color32::LIGHT_GREEN);
        //ui.add(progress_bar);
    Ok(())
}


