#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
use crate::tabs::tur_sheet::scaffold::{ScaffoldActions, ScaffoldApps};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use crossbeam::channel;
use database::{SCAFFOLD_PASS, SCAFFOLD_URL, SCAFFOLD_USER};
use database::schema::Store;
use database::schema::{ExtendedSeb, GetKeysResponse, LocalSebData};
use log::{debug, error, info, trace};
use quick_xml::de::from_str;
use quick_xml::{events::Event, name::QName, Reader};
use regex::Regex;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::*;
use std::fmt::Debug;
use std::path::Path;
use std::result::Result;
use std::time::{Duration, Instant};
use std::{
    collections::HashMap,
    error::Error,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
};
use tokio::{io::AsyncWriteExt, sync::mpsc::error::TryRecvError};

// use super::email_builder::AsanaTask;

pub struct SendRequest {
    pub tx: crossbeam::channel::Sender<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct License {
    r#type: String, // `r#type` is used because `type` is a reserved keyword in Rust.
    key: String,
}


impl SendRequest {
    pub async fn get_cps(
        so_number: String,
        client: reqwest::Client,
    ) -> anyhow::Result<Vec<GetKeysResponse>, anyhow::Error> {
        let json = serde_json::json!({
            "user_email": SCAFFOLD_USER,
            "user_password": SCAFFOLD_PASS,
            "application": "software_license_fetch",
            "action": "fetch_keys",
            "id_order": &so_number,
            "company": if so_number.len() == 8 { "pcl" } else { "prestashop" }
        });

        let response = client
            .post(SCAFFOLD_URL)
            .header(CONTENT_TYPE, "application/json") // application/x-www-form-urlencoded
            .header(ACCEPT, "application/json")
            .json(&json)
            // .form(&params)
            .send()
            .await?;

        let response_text: Vec<License> = response.json().await?;
        info!("Response: {:?}", response_text);

        // Separate SAS and WRAV keys
        let mut sas_keys = Vec::new();
        let mut wrav_keys = Vec::new();

        for license in response_text {
            match license.r#type.as_str() {
                "SAS" => sas_keys.push(license.key),
                "WRAV" => wrav_keys.push(license.key),
                other => log::warn!("Unexpected license type: {}", other),
            }
        }

        // Pair keys, padding with empty strings if uneven
        let max_len = sas_keys.len().max(wrav_keys.len());
        let mut result = Vec::new();

        for i in 0..max_len {
            let sas_key = sas_keys.get(i).cloned().unwrap_or_default();
            let wrav_key = wrav_keys.get(i).cloned().unwrap_or_default();
            result.push(GetKeysResponse {
                superanti_key: sas_key,
                webroot_key: wrav_key,
            });
        }

        info!("Processed keys: {:?}", result);
        Ok(result)
    }

}

pub async fn request_seb_info<T>(
    client: reqwest::Client,
    customer_email: Option<String>,
) -> anyhow::Result<T, anyhow::Error>
where
    T: Debug + Serialize + for<'a> Deserialize<'a> + Clone + std::convert::From<LocalSebData>,
{
    let mut params: HashMap<&str, &str> = HashMap::new();
    params.insert("user_email", SCAFFOLD_USER);
    params.insert("user_password", SCAFFOLD_PASS);
    params.insert("application", "carbonite");
    params.insert("action", "search");

    if let Some(customer_email) = customer_email {
        // let mut params: HashMap<&str, &str> = HashMap::new();
        let json = serde_json::json!({
            "user_email": SCAFFOLD_USER,
            "user_password": SCAFFOLD_PASS,
            "application": "carbonite",
            "action": "search",
            "search": &customer_email
        });

        let response = client
        .post(SCAFFOLD_URL) //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&json)
        .send()
        .await?;

        // info!("response: {:?}", response.text().await?);

        let response_json: Vec<T> = response.json().await?;
        info!("response_json: {:?}", response_json);
        Ok(response_json.get(0).unwrap().clone())
    } else {
        // supereasybackup.com/downloads/SuperEasyBackup.exe
        let file_path = "C:\\DCProtectData\\Shared\\Logs\\InstallationTracking.log"; // "D:\\Users\\Owner\\Desktop\\SEB\\DCProtectData-Customer\\Shared\\Logs\\InstallationTracking.log";

        // Read the file content
        let file_content = fs::read_to_string(file_path)?;

        // Deserialize the XML content
        let mut result: LocalSebData = from_str(&file_content)?;

        params.insert("search", &result.InstalledDeviceId);

        let response = client
            .post(SCAFFOLD_URL) //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .form(&params)
            .send()
            .await?;

        let response_json: Vec<ExtendedSeb> = response.json().await?; // ExtendedSeb

        info!("response: {:?}", response_json);
        let actual_response = response_json.get(0);

        if let Some(extended_seb) = actual_response {
            debug!("Carbonite response: {extended_seb:#?}");
            result.ExtendedSeb = Some(extended_seb.clone());
        }

        let res: T = result.try_into()?;

        Ok(res)
    }
}

pub async fn request_seb_info_from_drive<T>(
    client: reqwest::Client,
    customer_email: Option<String>,
    drive: String,
) -> anyhow::Result<T, anyhow::Error>
where
    T: Debug + Serialize + for<'a> Deserialize<'a> + Clone + std::convert::From<LocalSebData>,
{
    let mut params: HashMap<&str, &str> = HashMap::new();
    params.insert("user_email", SCAFFOLD_USER);
    params.insert("user_password", SCAFFOLD_PASS);
    params.insert("application", "carbonite");
    params.insert("action", "search");

    if let Some(customer_email) = customer_email {
        params.insert("search", &customer_email);

        let response = client
            .post(SCAFFOLD_URL) //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            // .header(ACCEPT, "application/json")
            .form(&params)
            .send()
            .await?;

        // info!("response: {:?}", response.text().await?);

        let response_json: Vec<T> = response.json().await?;
        info!("response_json: {:?}", response_json);
        Ok(response_json.get(0).unwrap().clone())
    } else {
        // supereasybackup.com/downloads/SuperEasyBackup.exe
        let mut file_path = PathBuf::from(drive);
        if file_path.exists() {
            file_path.push("DCProtectData\\Shared\\Logs\\InstallationTracking.log")
        }

        if file_path.exists() { // "D:\\Users\\Owner\\Desktop\\SEB\\DCProtectData-Customer\\Shared\\Logs\\InstallationTracking.log";

            // Read the file content
            let file_content = fs::read_to_string(file_path)?;

            // Deserialize the XML content
            let mut result: LocalSebData = from_str(&file_content)?;

            params.insert("search", &result.InstalledDeviceId);

            let response = client
                .post(SCAFFOLD_URL) //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .form(&params)
                .send()
                .await?;

            let response_json: Vec<ExtendedSeb> = response.json().await?; // ExtendedSeb

            info!("response: {:?}", response_json);
            let actual_response = response_json.get(0);

            if let Some(extended_seb) = actual_response {
                debug!("Carbonite response: {extended_seb:#?}");
                result.ExtendedSeb = Some(extended_seb.clone());
            }

            let res: T = result.try_into()?;
            Ok(res)
        } else {
            let mut seb = LocalSebData::default();
            seb.ActivationCode = "COULD NOT GET SEB INFORMATION".to_string();
            Ok(seb.into())
        }
    }
}


/*
    pub async fn send_ticket_request(
        tx: crossbeam::channel::Sender<String>,
        client: reqwest::Client,
        asana_task: AsanaTask,
        due_date: DateTime<Utc>,
    ) -> anyhow::Result<(), anyhow::Error> {
        let send = tx.clone();

        let mut _assigned_salesman = "1202792432658520".to_string(); // Jake
        let mut _assigned_tech = "1199992640930465".to_string(); // Logan

        if asana_task.assignee.salesman == "JDH2" {
            _assigned_salesman = "1202792432658520".to_string();
        } else if asana_task.assignee.salesman == "DMK" {
            _assigned_salesman = "1202791016369879".to_string();
        }

        if asana_task.assignee.tech == "LL" {
            _assigned_tech = "1199992640930465".to_string();
        } else if asana_task.assignee.tech == "BLK" {
            _assigned_tech = "1202792432421640".to_string();
        } else if asana_task.assignee.tech == "TBN" {
            _assigned_tech = "1202792432551073".to_string();
        }

        // salesman_map.insert("Jake", "1202792432658520");
        // salesman_map.insert("Danny", "1202791016369879");
        // tech_map.insert("Logan", "1199992640930465");
        // tech_map.insert("Bread", "1202792432421640");
        // tech_map.insert("Taco", "1202792432551073");

        let params = serde_json::json!({
            "data": {
                "name": asana_task.task_name,
                "html_notes": asana_task.html_notes,
                "followers": [
                    _assigned_salesman,
                    _assigned_tech
                ],
                "due_at": due_date.to_rfc3339_opts(SecondsFormat::Secs, true),
                "workspace": "13314583095021",
                "assignee": _assigned_salesman,
                "projects": ["1202792139600600"]
            }
        });

        let response = client
            .post("https://app.asana.com/api/1.0/tasks") //https://5dccaa60-8a54-47f1-8ff6-ce32034dd0f6.mock.pstmn.io
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(
                AUTHORIZATION,
                "Bearer 1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2",
            )
            .json(&params)
            .send()
            .await?;

        let res_body: Value = response.json().await?;
        debug!("Asana Response Body: {res_body:?}");
        let gid: Value = res_body.get("gid").unwrap_or(&Value::default()).clone();

        debug!("Asana Response: {gid:?}");

        let file = asana_task.file_attachment.clone();

        if let Some(file) = file {
            let file_name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("no file name");

            let file_attachment = asana_task.file_attachment.clone();
            let new_path = file_attachment.as_ref().map(|p| p.as_path().to_owned());

            // let byte_content = tokio::fs::read(new_path.unwrap()).await.unwrap();
            // let part = Part::bytes(byte_content).file_name(format!("{file_name}"));

            let mut form = HashMap::new();
            form.insert("file", "part"); //part
            form.insert("parent", "gid"); //text

            let response = client
                .post("https://app.asana.com/api/1.0/attachments")
                .header(
                    "Authorization",
                    "Bearer 1/1199992640930465:629a6fec5c395f50c92e878dcf1d32e2",
                )
                .header(ACCEPT, "application/json")
                .form(&form)
                .send()
                .await?;
        }

        Ok(())
    }
*/