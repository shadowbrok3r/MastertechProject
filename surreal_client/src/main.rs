use std::sync::{Arc, Condvar, Mutex};

use async_trait::async_trait;
use database::{schema::{ComputerData, ConnectedClient, DriveData, COMPUTER_TABLE}, *};
use anyhow::{Result, Error};
use log::info;
use num_format::{Locale, ToFormattedString};
use surrealdb::RecordId;
use sysinfo::{Disks, System};
use regex::Regex;

struct App {
    first_run: bool
}

impl Default for App {
    fn default() -> Self {
        Self { 
            first_run: true
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut app = App::default();
    let app_run = app.run().await;
    println!("app run: {app_run:?}");
    Ok(())
}

impl App {
    async fn run(&mut self) -> Result<(), Error> {
        if self.first_run {
            let email = "logan.lees@pclaptops.com".to_string();
            let pw = "toor".to_string();

            let _ = Database::new(email, pw, None).await?;
            
            let pair = Arc::new((Mutex::new(ComputerData::default()), Condvar::new()));
            let pair_clone = Arc::clone(&pair);
    
            tokio::spawn(async move {
                match ComputerData::default().get_computer_data().await {
                    // sysinfo_tx
                    Ok(data) => {
                        let (lock, cvar) = &*pair_clone;
                        let mut comp_data = lock.lock().unwrap();
                        *comp_data = data;
                        info!("Computer Data: {comp_data:?}");
                        cvar.notify_one();
                    }
                    Err(e) => println!("Error getting specs: {e:?}"),
                }
            });
    
            // Wait for the spawned task to complete and notify the condition variable
            let (lock, cvar) = &*pair;
            let mut comp_data = lock.lock().unwrap();
            while comp_data.cpu.is_empty() {
                comp_data = cvar.wait(comp_data).unwrap();
            }
            
            let client_hash = generate_client_id(
                comp_data.hostname.clone(), 
                comp_data.cpu.trim().to_string()
            );
    
            let url_string = format!(
                "{}:{}", 
                comp_data.hostname.clone(), 
                client_hash.split_at(9).0
            );

            println!("URL: {url_string:?}");

            let call: Option<ConnectedClient> = DATABASE
                .query("SELECT * FROM connected_client WHERE connection_string == $url")
                .bind(("url", url_string))
                .await?
                .take(0)?;

            if let Some(client) = call {
                println!("client: {client:#?}");
                
            }

            
        }

        Ok(())
    }
}

// Function to generate client ID
pub fn generate_client_id(hostname: String, cpu: String) -> String {
    use sha2::{Digest, Sha256};
    let cpu_id = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown-cpu".to_string());
    let combined = format!("{}-{}-{}", hostname, cpu, cpu_id);
    info!("Filesystem -> generate_client_id -> combined: {}", combined.clone());
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let result = hasher.finalize();
    let hex_string = hex::encode(result);
    info!("Filesystem -> generate_client_id -> hex_string: {}", hex_string.clone());
    hex_string
}

#[async_trait]
pub trait ComputerInfo {
    async fn get_computer_data(&mut self) -> anyhow::Result<ComputerData, anyhow::Error>;
}

#[async_trait]
impl ComputerInfo for ComputerData {
    async fn get_computer_data(&mut self) -> anyhow::Result<Self, anyhow::Error> {
        info!("Filesystem -> get_computer_data -> Getting sysinfo");
        // let machine = get_machine_instance().await?.clone();
        // let card = machine.gpu_info()?;
        // let usage = machine.graphics_status()?;
    
        // let gpu_info = Gpu {
        //     card,
        //     usage
        // };
        
        let mut sys = sysinfo::System::new_all();
        // info!("GPU: {gpu_info:?}");
        sys.refresh_all();
        
        info!("Filesystem -> get_computer_data -> Pulling Drive information");
        let mut disks = Disks::new_with_refreshed_list();
        // let client = Client::new();

        for disk in &mut disks {
            if !disk.is_removable() {
                self.add_disk(DriveData {
                    drive_type: format!("{:?}", disk.kind()),
                    total_size: (disk.total_space() / (1024 * 1024 * 1024))
                        .to_formatted_string(&Locale::en),
                    space_left: (disk.available_space() / (1024 * 1024 * 1024))
                        .to_formatted_string(&Locale::en),
                    drive_letter: disk.mount_point().to_str().unwrap_or("").to_string(),
                });
                info!("Filesystem -> get_computer_data -> DriveData: {:?}", disk.name());
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            info!("Filesystem -> get_computer_data -> Pulling linux gpu");
            let re = Regex::new(r"\[(.*)\]").unwrap();
            let gpu = String::from_utf8(
                tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("lspci | grep VGA")
                    .output()
                    .await?
                    .stdout,
            );

            if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str())
            {
                let full_gpu_name = &captures[1];
                self.gpu = full_gpu_name
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<&str>>()
                    .join(" ");
            }
        }

        info!("Filesystem -> get_computer_data -> Pulling CPU");
        self.cpu = sys.cpus()[0].brand().trim().to_string();
        info!("Filesystem -> get_computer_data -> Pulling RAM");
        self.ram = (sys.total_memory() / (1024 * 1024 * 1024) + 1)
            .to_formatted_string(&Locale::en)
            .trim()
            .to_string();
        info!("Filesystem -> get_computer_data -> Pulling OS");
        self.operating_system = System::long_os_version().unwrap_or_default();
        info!("Filesystem -> get_computer_data -> Pulling Hostname");
        self.hostname = System::host_name().unwrap_or_default();

        let client_hash = generate_client_id(self.hostname.clone(), self.cpu.trim().to_string());
        let id = format!("{}:{}", self.hostname.clone(), client_hash.split_at(9).0);
        info!("Filesystem -> get_computer_data -> ID: {id}");
        self.id = RecordId::from((COMPUTER_TABLE, id.clone().as_str()));
        info!("Filesystem -> get_computer_data -> RecordID: {:?}", self.id.clone());
        Ok(self.to_owned())
    }
}