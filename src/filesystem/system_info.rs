#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use std::{collections::HashMap, env, error::Error, fmt::Display, path::Path, process::{Output, Stdio}, str, sync::Arc, time::Duration};
use anyhow::{anyhow, Context};
use dotenv::dotenv;
use log::{debug, info};
use reqwest::{header::{HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE}, Client};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use surrealdb::sql::Thing;
use sysinfo::*;
use serde_json::Value;
use tokio::{io::{self, ErrorKind}, process::{Child, Command}, runtime::Handle, spawn};
use crossbeam::channel::Sender;
use regex::Regex;
use num_format::{Locale, ToFormattedString};
use futures_util::FutureExt;
use shell_words::{join, quote, split, ParseError};
use serde_json::json;
use sysinfo::{Components, CpuRefreshKind, Disks, Networks, RefreshKind, System};
use tokio::{sync::{mpsc, Mutex}, time::{self, sleep}};
use uuid::Uuid;
use crate::{database::{schema::{ComputerData, ComputerId, DriveData, LocalSebData, COMPUTER_TABLE}, SystemInformation}, tabs::tur_sheet::get_ticket::request_seb_info};
const CREATE_NO_WINDOW: u32 = 0x08000000;

impl ComputerData{
    pub async fn get_computer_data(&mut self, tx: Sender<ComputerData>) -> anyhow::Result<Self, anyhow::Error>{
        info!("Getting sysinfo");
        let sys = System::new_all();
        let mut disks = Disks::new_with_refreshed_list();
        let mut data = ComputerData::new();
        let client = Client::new();

        for disk in &mut disks{
            if !disk.is_removable(){
                data.add_disk(
                    DriveData{
                        drive_type: format!("{:?}", disk.kind()),
                        total_size: (disk.total_space() / ( 1024 * 1024 * 1024)).to_formatted_string(&Locale::en),
                        space_left: (disk.available_space() / ( 1024 * 1024 * 1024)).to_formatted_string(&Locale::en),
                        drive_letter: disk.mount_point().to_str().unwrap_or("").to_string()
                    }
                );
                println!("DriveData: {:?}", disk.name());
            }   
        }

        let seb_data: Result<LocalSebData, anyhow::Error> = request_seb_info(client, None)
            .await
            .or_else(|err|{
                info!("Error: {:?}", err.to_string());
                Err(err)
            }).and_then(|data|{
                info!("Pulled SEB Data successfully: {data:#?}");
                Ok(data)
        }); 

        #[cfg(target_os = "windows")]
        {
            let gpu =  String::from_utf8(
                tokio::process::Command::new("cmd")
                .args(["/C", "wmic path win32_VideoController get name"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await?
                .stdout
            );

            let clone_gpu_name = gpu.clone().unwrap_or("no gpu detected".to_string());
            let parse_gpu_name: Vec<&str> = clone_gpu_name.split("Name").collect();
            if parse_gpu_name[0].is_empty(){
                self.gpu = parse_gpu_name.clone()[1].trim().to_string();
            }
        }

        #[cfg(target_os = "linux")]
        {
            let re = Regex::new(r"\[(.*)\]").unwrap();
            let gpu = String::from_utf8(
                tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("lspci | grep VGA")
                    .output()
                    .await?
                    .stdout
            );
            
            if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str()){
                let full_gpu_name = &captures[1];
                self.gpu = full_gpu_name.split_whitespace().take(3).collect::<Vec<&str>>().join(" ");
            }
        }

        if let Ok(seb) = seb_data{
            self.seb_info = Some(seb);
        }

        self.cpu = sys.cpus()[0].brand().trim().to_string();
        self.ram = (sys.total_memory() / ( 1024 * 1024 * 1024 ) + 1).to_formatted_string(&Locale::en).trim().to_string();
        self.operating_system = System::long_os_version().unwrap_or_default(); //sys.long_os_version().unwrap_or_else(|| "<unknown>".to_owned());
        self.hostname = System::host_name().unwrap_or_default();
        self.drives = data.drives;

        let client_hash = generate_client_id(self.hostname.clone(), self.cpu.trim().to_string());
        let id = format!("{}:{}", self.hostname.clone(), client_hash.split_at(9).0);

        self.id = Some(ComputerId(Thing::from((COMPUTER_TABLE,  id.clone().as_str()))));

        match tx.try_send(self.to_owned()) {
            Ok(_) => {
                info!("Sent sysinfo");
                drop(tx)
            },
            Err(e) => info!("Couldnt send sysinfo: {e:?}")
        }

        Ok(self.to_owned())
    }

    #[cfg(target_os="windows")]
    pub fn get_antivirus() -> io::Result<Vec<(String, Option<bool>)>> {
        let (sender, receiver) = crossbeam::channel::unbounded();

        let av_to_search = vec![
            "mbam", // MALWAREBYTES
            "aswtoolssvc", // AVAST
            "avgToolsSvc", // AVG
            "mcuicnt", // MCAFFEE
            "norton", // NORTON
            "wrsa", // WEBROOT
            "egui", // ESET
            "superantispyware" // SUPERANTI
            ];
            
        let antivirus_mapping = Arc::new([
            ("mbam", "Malwarebytes"),
            ("aswtoolssvc", "Avast"),
            ("avgToolsSvc", "AVG"),
            ("mcuicnt", "McAfee"),
            ("norton", "Norton"),
            ("wrsa", "Webroot"),
            ("egui", "ESET"),
            ("superantispyware", "SuperAntiSpyware"),
        ].iter().cloned().collect::<HashMap<&str, &str>>());

        for antivirus in av_to_search.clone().into_iter() {

            let sender = sender.clone();
            let antivirus_mapping = Arc::clone(&antivirus_mapping);

            spawn(async move {

                let where_cmd = ["where", "/r", "C:\\Program Files", antivirus];

                let output = tokio::process::Command::new("cmd")
                    .args(&["/C"])
                    .args(where_cmd)
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .await
                    .map_err(|e| io::Error::new(ErrorKind::Other, format!("Failed to execute command: {}", e)))?;

                    let exists = if output.stdout.is_empty() {
                        None
                    } else {
                        Some(true)
                    };

                    let name = antivirus_mapping.get(antivirus).unwrap_or(&antivirus);

                    sender.try_send((name.to_string(), exists)).map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to send data through channel"))
            });
        }
    
        let mut antivirus_exists = Vec::new();
        for _ in 0..av_to_search.len() {
            let exists = receiver.try_recv().map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to receive data from channel"))?;
            antivirus_exists.push(exists);
        }
    
        Ok(antivirus_exists)
    }
}

pub async fn get_sysinfo() -> anyhow::Result<SystemInformation, anyhow::Error> {
    let mut sys = System::new_all();
    let sysinf: SystemInformation;

    // First we update all information of our `System` struct.
    sys.refresh_all();

    let mut cpu_percentage = f32::default();
    let mut cpu_clock = u64::default();
    let mut disks = String::new();
    let disk_list = Disks::new_with_refreshed_list();
    // let component_temp = String::new();
    let mut network_interfaces: HashMap<String, String> = HashMap::new();
    let mut component_temps: HashMap<String, f32> = HashMap::new();
    // Components temperature:
    let components = Components::new_with_refreshed_list();
    // Network interfaces name, total data received and total data transmitted:
    let networks = Networks::new_with_refreshed_list();
    // RAM and swap information:
    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();

    // Display system information:
    let name = System::name().context("Could not retrieve system name")?;
    let kernel_version = System::kernel_version().context("Could not retrieve kernel_version")?;
    let os_version = System::os_version().context("Could not retrieve os_version")?;
    let hostname = System::host_name().context("Could not retrieve hostname")?;

    // Number of CPUs:
    let number_of_cpus = format!("NB CPUs: {} \n", sys.cpus().len());

    // Display processes ID, name na disk usage:
    // for (pid, process) in sys.processes() {println!("[{pid}] {} {:?}", process.name(), process.disk_usage());}

    for disk in &disk_list {disks += format!("{disk:?}").as_str();}

    for (interface_name, data) in &networks {
        if data.total_received() > 1 {
            let up_down = format!("{}/{}", data.total_received(), data.total_transmitted());
            network_interfaces.insert(interface_name.clone(), up_down);
        }
    }

    for component in &components {
        // component_temp += format!("{}/{}", component.temperature(), component.max()).as_str();
        component_temps.insert(component.label().to_string(), component.temperature());
        // comps += format!("{component:#?} \n", component.).as_str();
    }

    let mut s = System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::everything()));

    std::thread::sleep(Duration::from_millis(200));

    s.refresh_cpu(); // Refreshing CPU information.
    for cpu in s.cpus() {
        cpu_percentage = cpu.cpu_usage();
        cpu_clock = cpu.frequency();
    }

    sysinf = SystemInformation {
        cpu_percentage,
        cpu_clock: cpu_clock as f32,
        component_temps,
        disks,
        total_memory: total_memory as f32,
        used_memory: used_memory as f32,
        name,
        kernel_version,
        os_version,
        hostname,
        number_of_cpus,
        network_interfaces,
    };

    Ok(sysinf)
}

// Function to generate client ID
pub fn generate_client_id(hostname: String, cpu: String) -> String {
    let cpu_id = env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown-cpu".to_string());
    let combined = format!("{}-{}-{}", hostname, cpu, cpu_id);
    info!("combined: {}", combined.clone());
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let result = hasher.finalize();
    let hex_string = hex::encode(result);
    info!("hex_string: {}", hex_string.clone());
    hex_string
}
