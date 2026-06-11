use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Method};
use serde::Deserialize;

use crate::SCAFFOLD_URL;

#[derive(Debug, Deserialize)]
struct EverestSerialLookupEntry {
    #[serde(rename = "DOCNUM")] docnum: String,
}

#[derive(Debug, Deserialize)]
struct EverestOrderHeader {
    #[serde(rename = "ACCT_NAME")] acct_name: Option<String>,
    #[serde(rename = "NAME")] name: Option<String>,
    #[serde(rename = "FIRST_NAME")] first_name: Option<String>,
    #[serde(rename = "LAST_NAME")] last_name: Option<String>,
}

/// Perform Everest fallback flow:
/// 1) getDocnumBySerialNumber -> obtain docnum
/// 2) getOrder(docnum) -> obtain order/customer info
/// Returns formatted string "NameOrAcct - DOCNUM"
pub async fn request_everest(serial13: &str) -> Result<String> {
    let client = Client::builder().build()?;

    let user_email = crate::SCAFFOLD_USER;
    let user_password = crate::SCAFFOLD_PASS;

    // 1) Lookup DOCNUM by serial
    let docnum = lookup_docnum(&client, &user_email, &user_password, serial13).await
        .context("Everest docnum lookup failed")?;

    // 2) Fetch order header by DOCNUM
    let header = get_order(&client, &user_email, &user_password, &docnum).await
        .context("Everest order fetch failed")?;

    let name = header.first_name.as_ref()
        .and_then(|f| header.last_name.as_ref().map(|l| format!("{} {}", f.trim(), l.trim())))
        .filter(|s| !s.trim().is_empty())
        .or_else(|| header.name.clone().filter(|s| !s.trim().is_empty()))
        .or_else(|| header.acct_name.clone().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "Unknown Customer".into());

    Ok(format!("{} - {}", name.trim(), docnum))
}

/// Fetch the order header for a known Everest DOCNUM.
/// Returns `(customer_name, docnum)`.
pub async fn request_everest_header_by_docnum(docnum: &str) -> Result<(String, String)> {
    let client = Client::builder().build()?;
    let header = get_order(&client, crate::SCAFFOLD_USER, crate::SCAFFOLD_PASS, docnum)
        .await
        .context("Everest order fetch failed")?;

    let name = header.first_name.as_ref()
        .and_then(|f| header.last_name.as_ref().map(|l| format!("{} {}", f.trim(), l.trim())))
        .filter(|s| !s.trim().is_empty())
        .or_else(|| header.name.clone().filter(|s| !s.trim().is_empty()))
        .or_else(|| header.acct_name.clone().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "Unknown Customer".into());

    Ok((name.trim().to_string(), docnum.to_string()))
}

async fn lookup_docnum(client: &Client, email: &str, password: &str, serial: &str) -> Result<String> {
    let payload = serde_json::json!({
        "action": "everest_call",
        "application": "everest",
        "arg1": serial,
        "call": "getDocnumBySerialNumber",
        "user_email": email,
        "user_password": password,
    });

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    let resp = client
        .request(Method::POST, SCAFFOLD_URL)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;

    let status = resp.status();
    let body_txt = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("Everest serial lookup HTTP {}: {}", status, body_txt));
    }

    let parsed: serde_json::Value = serde_json::from_str(&body_txt)
        .context("Parsing Everest serial lookup JSON")?;
    let arr = parsed.as_array().ok_or_else(|| anyhow!("Everest serial lookup unexpected JSON shape"))?;
    let first = arr.get(0).ok_or_else(|| anyhow!("No entries returned for serial"))?;
    let entry: EverestSerialLookupEntry = serde_json::from_value(first.clone())?;
    if entry.docnum.trim().is_empty() {
        return Err(anyhow!("Empty DOCNUM returned"));
    }
    Ok(entry.docnum)
}

async fn get_order(client: &Client, email: &str, password: &str, docnum: &str) -> Result<EverestOrderHeader> {
    let payload = serde_json::json!({
        "action": "everest_call",
        "application": "everest",
        "arg1": docnum,
        "arg2": true,
        "call": "getOrder",
        "user_email": email,
        "user_password": password,
    });
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    let resp = client
        .request(Method::POST, SCAFFOLD_URL)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;
    let status = resp.status();
    let body_txt = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("Everest order fetch HTTP {}: {}", status, body_txt));
    }

    let parsed: serde_json::Value = serde_json::from_str(&body_txt)
        .context("Parsing Everest order JSON")?;
    let header_val = parsed.get("header").ok_or_else(|| anyhow!("Missing 'header' in order response"))?.clone();
    let header: EverestOrderHeader = serde_json::from_value(header_val)?;
    Ok(header)
}

