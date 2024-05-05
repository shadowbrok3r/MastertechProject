use std::{collections::HashMap, error::Error, io::Cursor, sync::Arc};
use futures::StreamExt;
use log::info;
use reqwest::{header::CONTENT_LENGTH, Client};
use lazy_static::lazy_static;
use async_trait::async_trait;
use sha2::{Sha256, Digest};
use tokio::{fs, io::{self, AsyncWriteExt}, process::Command, sync::Mutex};

use crate::{data::GetKeysResponse, ticket_request::request::SendRequest};
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[async_trait]
pub trait ScriptAction {
    async fn execute(&self, scripts: &Scripts) -> Result<(), Box<dyn std::error::Error>>;
}


pub struct Scripts{
    pub service_number: Option<String>,
    pub client: Client,
    pub wrsa: String,
    pub sas: String,
    pub check_driver: String,
    pub running_tasks: String,
}

pub struct InstallWebroot;
pub struct InstallSAS;
pub struct CheckDriverIssues;
pub struct RunningTasks;

lazy_static! {
    pub static ref SCRIPT_ACTIONS: HashMap<&'static str, Arc<dyn ScriptAction + Send + Sync>> = {
        let mut m = HashMap::new();
        let install_webroot: Arc<dyn ScriptAction + Send + Sync> = Arc::new(InstallWebroot {});
        let install_sas: Arc<dyn ScriptAction + Send + Sync> = Arc::new(InstallSAS {});
        let check_drivers: Arc<dyn ScriptAction + Send + Sync> = Arc::new(CheckDriverIssues {});
        let running_tasks: Arc<dyn ScriptAction + Send + Sync> = Arc::new(RunningTasks{});

        m.insert("Install Webroot", install_webroot);
        m.insert("Install SAS", install_sas);
        m.insert("Check Driver Issues", check_drivers);
        m.insert("Running Tasks", running_tasks);
        m
    };
}

impl Default for Scripts{
    fn default() -> Self{
        
        Self{
            service_number: None,
            client: Client::new(),
            wrsa: "Install Webroot".to_string(),
            sas: "Install SAS".to_string(),
            check_driver: "Check Driver Issues".to_string(),
            running_tasks: "Running Tasks".to_string()
        }
    }
}

impl Scripts{
    pub async fn new(service_number: String) -> Self{
        Self{
            service_number: Some(service_number),
            client: Client::new(),
            wrsa: "Install Webroot".to_string(),
            sas: "Install SAS".to_string(),
            check_driver: "Check Driver Issues".to_string(),
            running_tasks: "Running Tasks".to_string()
        }
    }

    pub async fn install_webroot(&self) -> Result<(), Box<dyn Error>> {
        info!("running install_webroot!");
        
        if let Some(service_number) = &self.service_number{
            let response = self.client.get(
                format!("https://anywhere.webrootcloudav.com/zerol/wsainstall.exe")
                ) 
                .send()
                .await?;
                
            let total_length = response
                .content_length()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Content-Length header is missing"))?;
            let mut downloaded_bytes: u64 = 0;

            let temp_directory = std::env::temp_dir();
            let wrv_path = format!("{}\\wrv.exe", temp_directory.display());

            let mut file = fs::File::create(wrv_path.clone()).await?;
            let mut sha = sha2::Sha256::new();

            let mut stream = response.bytes_stream();

            while let Some(item) = stream.next().await{
                let chunk = item?;
                file.write_all(&chunk).await?;
                sha.update(&chunk);
                downloaded_bytes += chunk.len() as u64;
            }

            if downloaded_bytes == total_length {
                let cps_request = SendRequest::get_cps(service_number.clone(), self.client.clone());
                let cps_keys =  cps_request.await.unwrap_or(GetKeysResponse::default());

                info!("cps_keys: {:?}", cps_keys.clone());

                let hash = sha.finalize();
                info!("Download complete. SHA-256: {:x}", hash);
    
                let cmd_stdout = Command::new("cmd")
                    .arg("/c ")
                    .arg(wrv_path)
                    .arg(format!("/keycode={}", cps_keys.webroot_key))
                    .arg("/silent")
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()?
                    .stdout;

                info!("cmd_stdout: {:?}", cmd_stdout);
            }
        }else{
            info!("No service number found");
        }
        Ok(())
    }
    
    pub async fn install_sas(&self) -> Result<(), Box<dyn Error>> {
        info!("running install_sas!");

        if let Some(service_number) = &self.service_number{
            let response = self.client.get(
                format!("https://secure.superantispyware.com/SUPERAntiSpyware.exe")
                ) 
                .send()
                .await?;
                
            let total_length = response
                .content_length()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Content-Length header is missing"))?;
            let mut downloaded_bytes: u64 = 0;

            let temp_directory = std::env::temp_dir();
            let sas_path = format!("{}\\sas.exe", temp_directory.display());

            let mut file = fs::File::create(sas_path.clone()).await?;
            let mut sha = sha2::Sha256::new();

            let mut stream = response.bytes_stream();

            while let Some(item) = stream.next().await{
                let chunk = item?;
                file.write_all(&chunk).await?;
                sha.update(&chunk);
                downloaded_bytes += chunk.len() as u64;
            }

            if downloaded_bytes == total_length {
                let cps_request = SendRequest::get_cps(service_number.clone(), self.client.clone());
                let cps_keys =  cps_request.await.unwrap_or(GetKeysResponse::default());

                info!("cps_keys: {:?}", cps_keys.clone());

                let hash = sha.finalize();
                info!("Download complete. SHA-256: {:x}", hash);

                let cmd_stdout = Command::new("cmd")
                    .arg("/c ")
                    .arg(sas_path)
                    .arg(format!("/REGCODE={}", cps_keys.superanti_key))
                    .arg("/silent")
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()?
                    .stdout;

                info!("cmd_stdout: {:?}", cmd_stdout);
            }
        }else{
            info!("No service number found");
        }

        Ok(())
    }
    
    pub async fn check_driver_issues(&self) -> Result<(), Box<dyn Error>> {
        info!("running check_driver_issues!");
        Ok(())
    }
    
    pub async fn running_tasks(&self) -> Result<(), Box<dyn Error>> {
        info!("running running_tasks!");
        Ok(())
    }
}



#[async_trait]
impl ScriptAction for InstallWebroot {
    async fn execute(&self, scripts: &Scripts) -> Result<(), Box<dyn std::error::Error>> {
        Scripts::install_webroot(scripts).await
    }
}

#[async_trait]
impl ScriptAction for InstallSAS {
    async fn execute(&self, scripts: &Scripts) -> Result<(), Box<dyn std::error::Error>> {
        Scripts::install_sas(scripts).await
    }
}

#[async_trait]
impl ScriptAction for CheckDriverIssues {
    async fn execute(&self, scripts: &Scripts) -> Result<(), Box<dyn std::error::Error>> {
        Scripts::check_driver_issues(scripts).await
    }
}

#[async_trait]
impl ScriptAction for RunningTasks {
    async fn execute(&self, scripts: &Scripts) -> Result<(), Box<dyn std::error::Error>> {
        Scripts::running_tasks(scripts).await
    }
}

