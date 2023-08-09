#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use std::{collections::HashMap, sync::Arc, path::Path};
use serde::{Deserialize, Serialize};
use sysinfo::*;
use serde_json::Value;
use tokio::{io::{self, ErrorKind}, runtime::Handle};
use crossbeam::channel;
use regex::Regex;
pub struct RetrieveSystemInfo {
    pub tx: std::sync::mpsc::Sender<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SystemInformation{
    pub cpu_name: String,
    pub total_ram: String,
    pub system_name: String,
    pub disks: DiskData, //Option<String>
    pub gpu: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct DiskData {
    pub disks: Vec<Value>,
}

impl DiskData {
    pub fn new() -> Self {
        DiskData {
            disks: Vec::new(),
        }
    }

    fn add_disk(&mut self, disk: Value){
        self.disks.push(disk);
    }
}
const CREATE_NO_WINDOW: u32 = 0x08000000;

impl RetrieveSystemInfo{
    #[cfg(target_os="windows")]
    pub fn get_antivirus() -> io::Result<Vec<(String, Option<bool>)>> {
        let (sender, receiver) = channel::unbounded();
        let av_to_search = vec![
            "mbam", // MALWAREBYTES
            "aswtoolssvc", // AVAST
            "avgToolsSvc", // AVG
            "mcuicnt", // MCAFFEE
            "norton", 
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

            tokio::spawn(async move {

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

                    sender.send((name.to_string(), exists)).map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to send data through channel"))
            });
        }
    
        let mut antivirus_exists = Vec::new();
        for _ in 0..av_to_search.len() {
            let exists = receiver.recv().map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to receive data from channel"))?;
            antivirus_exists.push(exists);
        }
    
        Ok(antivirus_exists)
    }

    pub fn get_system_specs(tx: std::sync::mpsc::Sender<String>){
        
        tokio::spawn(async {
            let sys = System::new_all(); // Create `System` struct.

            let cpu_brand = sys.cpus()[0].brand().to_string();
            let ram = (sys.total_memory() / ( 1024 * 1024 * 1024 ) + 1).to_string();
            let system = sys.long_os_version().unwrap_or_else(|| "<unknown>".to_owned());
            let disks = sys.disks();
            let disks_clone = disks.clone();


            let mut data = DiskData::new();
            
            for disk in disks_clone{
                if !disk.is_removable(){
                    data.add_disk(serde_json::json!({
                        "name": disk.name(),
                        "letter": disk.mount_point().to_str(),
                        "total space": (disk.total_space() / ( 1024 * 1024 * 1024)).to_string(),
                        "available space": (disk.available_space() / ( 1024 * 1024 * 1024)).to_string(),
                    }));
                }   
            }
            
            #[cfg(target_os = "windows")]
            {
                let gpu_name = 
                String::from_utf8(
                    tokio::process::Command::new("cmd")
                    .args(["/C", "wmic path win32_VideoController get name"])
                    .creation_flags(CREATE_NO_WINDOW)
                    // "-Command {", 
                    //"(win32_videocontroller | select-object -property Name | ft -autosize -hidetableheaders | out-string).trim()}"
                    .output()
                    .await
                    .unwrap()
                    .stdout
                );

                let mut new_gpu_name = "";
                let clone_gpu_name = gpu_name.clone().unwrap_or("no gpu detected".to_string());
                let parse_gpu_name: Vec<&str> = clone_gpu_name.split("Name").collect();
                if parse_gpu_name[0].is_empty(){
                    new_gpu_name = parse_gpu_name.clone()[1].trim();
                }

                let system_info = SystemInformation{
                    cpu_name: cpu_brand,
                    total_ram: ram,
                    system_name: system,
                    disks: data,
                    gpu: Some(new_gpu_name.to_string())
                };

                let system_info_json = serde_json::to_string(&system_info).unwrap();
                match tx.send(system_info_json) {
                    Ok(_) => {
                        drop(tx);
                    },
                    Err(e) => {
                        eprintln!("Error while sending ticket information: {}", e.to_string());
                        drop(tx);
                    }
                }
            }


            #[cfg(target_os = "linux")]
            {
                let re = Regex::new(r"\[(.*)\]").unwrap();
                let mut gpu_name = String::new();
                let gpu = 
                String::from_utf8(
                    tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg("lspci | grep VGA")
                        .output()
                        .await
                        .unwrap()
                        .stdout
                );
                if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str()){
                    let full_gpu_name = &captures[1];
                    gpu_name = full_gpu_name.split_whitespace().take(3).collect::<Vec<&str>>().join(" ");
                }
    
                let system_info = SystemInformation{
                    cpu_name: cpu_brand,
                    total_ram: ram,
                    system_name: system,
                    disks: data,
                    gpu: Some(gpu_name)
                };
                let system_info_json = serde_json::to_string(&system_info).unwrap();
                println!("system info json: {}", system_info_json);
                match tx.send(system_info_json) {
                    Ok(_) => {
                        drop(tx);
                    },
                    Err(e) => {
                        eprintln!("Error while sending ticket information: {}", e.to_string());
                        drop(tx);
                    }
                }
            }         
        });
    }
}