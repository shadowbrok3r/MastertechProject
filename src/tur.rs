use egui::Ui;
use reqwest::header::{CONTENT_TYPE, ACCEPT};
/*struct tur{struct Post {
    id: Option<i32>,
    title: String,
    body: String,
    #[serde(rename = "userId")]
    user_id: i32,
}}*/
//impl tur{}

//#[tokio::main]
pub async fn request_ticket_info(ui: &mut Ui) //, output_console_text: &mut String, service_order_num: &mut String) -> Result<(), reqwest::Error> {
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

    let client = reqwest::Client::new();
    let resp = client.post("https://scaffold.pclaptops.com/api/index")
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params).send().await?;
    
        match resp.status() {
            reqwest::StatusCode::OK => {
                println!("Success!");
                let resp_body = resp.text().await?;
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
    //let progress_bar = egui::ProgressBar::new(100.0).show_percentage().desired_width(30.0).fill(egui::Color32::LIGHT_GREEN);
    //ui.add(progress_bar);
    Ok(())
}


