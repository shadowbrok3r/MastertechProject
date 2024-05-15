#![allow(non_snake_case)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use reqwest::header::{AUTHORIZATION, ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::error::Error;

const TOKEN: &str = "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";

async fn create_new_issue(title: String, body: String, client: reqwest::Client)  
    -> anyhow::Result<(), anyhow::Error> 
{
    // Now you can use the method on the instance of ScaffoldRequestBuilder
    let params = serde_json::json!({
        "title": title,
        "body": body,
        "assignees": ["shadowbrok3r"],
        "labels": [
            "bug"
        ]
    });

    let res = client
        .post("https://api.github.com/repos/shadowbrok3r/Mastertech4.0/issues") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(AUTHORIZATION, TOKEN)
        .header(ACCEPT, "application/vnd.github+json")
        .json(&params)
        .send()
        .await?
        .json()
        .await;

    Ok(res?)
}