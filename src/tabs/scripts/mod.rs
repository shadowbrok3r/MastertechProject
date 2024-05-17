use std::{collections::HashMap, sync::Arc};
use eframe::egui::{Align, Button, Grid, Layout, RichText, Ui};
use futures::StreamExt;
use log::info;
use reqwest::Client;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::{fs, io::{self, AsyncWriteExt}, process::Command};
use crate::{app_state::MastertechContext, database::GetKeysResponse, handle_api::api_request::SendRequest};

#[cfg(target_os="windows")]
use wmi::{COMLibrary, WMIConnection, WMIError};

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
    pub query_antivirus: String,
}

pub struct InstallWebroot;
pub struct InstallSAS;
pub struct CheckDriverIssues;
pub struct RunningTasks;
pub struct QueryAntivirus;

impl MastertechContext{
    pub fn scripts(&mut self, ui: &mut Ui){
        ui.style_mut().spacing.button_padding = (4.0, 6.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.vertical(|ui|{ui.add_space(6.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});

        let scripts = Arc::new(Scripts::new(self.so_number.to_string()));
        let scripts_list  = scripts.get_scripts();
        // Collect keys and sort them
        let mut keys: Vec<&'static str> = scripts_list.keys().cloned().collect();
        keys.sort();  // Sort the script names alphabetically

        Grid::new("scripts").min_col_width(self.widget_size).num_columns(1).min_row_height(8.0).spacing([10.0, 8.0]).show(
            ui, |ui| 
        {
            ui  
                .with_layout(Layout::top_down_justified(Align::Center),|ui|
            {
                for key in keys.iter() {
                    if let Some(action) = scripts_list.get(*key) {
                        let button = Button::new(RichText::new(*key).small().size(12.0));
                            // .min_size(Vec2::new(25.0, 6.0));
        
                        if ui.add(button).clicked(){
                            info!("Clicked button: {}", *key);
        
                            let action_clone = action.clone();
                            let so_num = Arc::new(self.so_number.clone());
                            let scripts = scripts.clone();
                            info!("SO number: {}", &so_num);
                            tokio::spawn(async move {
                                action_clone.execute(&scripts).await.unwrap();
                            });
                        }
                        ui.end_row();
                    }
                }
            });
        });
     }
}

impl Default for Scripts{
    fn default() -> Self{
        Self{
            service_number: None,
            client: Client::new(),
            wrsa: "Install Webroot".to_string(),
            sas: "Install SAS".to_string(),
            check_driver: "Check Driver Issues".to_string(),
            running_tasks: "Running Tasks".to_string(),
            query_antivirus: "Query Antivirus".to_string()
        }
    }
}

impl Scripts{
    pub fn new(service_number: String) -> Self{
        Self{
            service_number: Some(service_number),
            client: Client::new(),
            wrsa: "Install Webroot".to_string(),
            sas: "Install SAS".to_string(),
            check_driver: "Check Driver Issues".to_string(),
            running_tasks: "Running Tasks".to_string(),
            query_antivirus: "Query Antivirus".to_string()
        }
    }
    pub fn get_scripts(&self) -> HashMap<&'static str, Arc<dyn ScriptAction + Send + Sync>> {
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
    }

    pub async fn install_webroot(&self) -> Result<(), Box<dyn std::error::Error>> {
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
                #[cfg(target_os="windows")]{    
                    let cmd_stdout = Command::new("cmd")
                        .arg("/c ")
                        .arg(wrv_path)
                        .arg(format!("/key={}", cps_keys.webroot_key))
                        .arg("/silent")
                        .creation_flags(CREATE_NO_WINDOW)
                        .spawn()?
                        .stdout;
                
                    info!("cmd_stdout: {:?}", cmd_stdout);
                }
            }
        }else{
            info!("No service number found");
        }
        Ok(())
    }
    
    pub async fn install_sas(&self) -> Result<(), Box<dyn std::error::Error>> {
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
                #[cfg(target_os="windows")]{
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
            }
        }else{
            info!("No service number found");
        }

        Ok(())
    }
    
    pub async fn check_driver_issues(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("running check_driver_issues!");
        Ok(())
    }
    
    pub async fn running_tasks(&self) -> Result<(), Box<dyn std::error::Error>> {
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

// #[cfg(target_os="windows")]
// #[async_trait]
// impl ScriptAction for QueryAntivirus {
//     async fn execute(&self, scripts: &Scripts) -> Result<(), Box<dyn std::error::Error>> {
//         Scripts::query_antivirus(scripts).await
//     }
// }

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Antivirus {
    pub product_state: String,
    pub display_name: String,
}

#[cfg(target_os="windows")]
pub fn query_antivirus() -> anyhow::Result<Vec<Antivirus>, WMIError>{
    // Initialize the COM Library

    
    let com_con = COMLibrary::new()?;
    let wmi_con = WMIConnection::new(com_con.into())?;

    // Perform a WMI query
    let results: Vec<Antivirus> = wmi_con.raw_query("SELECT * FROM Win32_OperatingSystem")?; // ("SELECT displayName, productState FROM AntiVirusProduct")?;

    drop(wmi_con);
    // let mut antivirus: Antivirus = Default::default();
    // for result in &results {
        //     let display_name = result.get("displayName").context("Could not get displayName").unwrap();
        //     let product_state = result.get("productState").context("Could not get productState").unwrap();
        //     let product_state = format!("{:X?}", product_state);
        //     let display_name = format!("{:?}", display_name);
        //     info!("Antivirus: {:?}, Product State: {:X?}", display_name.clone(), product_state.clone());
        // antivirus { product_state, display_name };
    // }

    Ok(results)
}