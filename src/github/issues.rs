#![allow(non_snake_case)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use reqwest::header::{Authorization, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::error::Error;

const TOKEN: &str = "Bearer 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";

async fn create_new_issue(title: String, body: String, client: reqwest::Client)  
-> core::result::Result<Box<dyn Error>> {

    // Now you can use the method on the instance of ScaffoldRequestBuilder
    let params = serde_json::json!({
        "title": title,
        "body": body,
        "assignees": ["shadowbrok3r"],
        "labels": [
            "bug"
        ]
    });

    let response = client
        .post("https://api.github.com/repos/shadowbrok3r/Mastertech4.0/issues") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(Authorization, TOKEN)
        .header(ACCEPT, "application/vnd.github+json")
        .json(&params)
        .send()
        .await;

    match response {
        Ok(res) => {
            if cfg!(debug_assertions){
                
                let raw_response = res.json().await?;
                println!("raw resp: {raw_response:?}");
                Ok(raw_response)
            }else{
                let json_response: GetTicketResponse = res.json().await?;
                Ok(json_response)
            }
        },
        Err(e) => {
            println!("Boxed error: {e:?}");
            Err(Box::new(e))
        },
    }
}