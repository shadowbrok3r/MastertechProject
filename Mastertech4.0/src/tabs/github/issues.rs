#![allow(non_snake_case)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::error::Error;

pub async fn create_new_issue(title: String, body: String, client: reqwest::Client)  
    -> anyhow::Result<String, anyhow::Error> 
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
        .post("https://api.github.com/repos/shadowbrok3r/MastertechProject/issues")
        .bearer_auth(database::ISSUE_TOKEN)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "Mastertech")
        .json(&params)
        .send()
        .await?
        .text()
        .await?;

    Ok(res)
}