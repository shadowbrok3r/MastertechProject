#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
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

impl RetrieveSystemInfo{
    pub fn get_antivirus() -> io::Result<Vec<(String, String)>> {
        let (sender, receiver) = channel::unbounded();
        let antivirus_names = vec![
            "mbam", // where /r "C:\Program Files" mbam
            "avast", // where /r "C:\Program Files" aswtoolssvc
            "avg", // where /r "C:\Program Files" avgToolsSvc.exe
            "mcaffee", // where /r "C:\Program Files (x86)" mcuicnt
            "norton", 
            "webroot", // where /r "C:\Program Files" wrsa
            "eset", // where /r "C:\Program Files" egui
            "superantispyware" // where /r "C:\Program Files" superantispyware
            ];
    
        for antivirus in antivirus_names.clone().into_iter() {
            let sender = sender.clone();
            tokio::spawn(async move {
                let where_cmd = ["where", "/r", "C:\\Program Files", antivirus];
                let output = tokio::process::Command::new("cmd")
                    .args(&["/C"])
                    .args(where_cmd)
                    .output()
                    .await
                    .map_err(|e| io::Error::new(ErrorKind::Other, format!("Failed to execute command: {}", e)))?;

                let path = String::from_utf8(output.stdout)
                    .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("Failed to convert output to String: {}", e)))?;

                sender.send((antivirus, path)).map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to send data through channel"))
            });
        }
    
        let mut antivirus_paths = Vec::new();
        for _ in 0..antivirus_names.len() {
            let path = receiver.recv().map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to receive data from channel"))?;
            antivirus_paths.push((path.0.to_string(), path.1));
        }
    
        Ok(antivirus_paths)
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
                    // "-Command {", 
                    //"(win32_videocontroller | select-object -property Name | ft -autosize -hidetableheaders | out-string).trim()}"
                    .output()
                    .await
                    .unwrap()
                    .stdout
                );
                let system_info = SystemInformation{
                    cpu_name: cpu_brand,
                    total_ram: ram,
                    system_name: system,
                    disks: data,
                    gpu: Some(gpu_name.unwrap())
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