#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use serde::{Deserialize, Serialize};
use sysinfo::*;
use serde_json::Value;
use tokio::runtime::Handle;

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

    pub fn get_system_specs(tx: std::sync::mpsc::Sender<String>){
        let handle = Handle::current();
        
        std::thread::spawn(move||{
            handle.block_on(async{
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

                let gpu = 
                String::from_utf8(
                    std::process::Command::new("cmd")
                    .args(["/C", "wmic path win32_VideoController get name"])
                    .output().unwrap().stdout
                );

                // match gpu{
                //     Ok(output) => {
                //         println!("{:?}", output)
                //     }
                //     Err(e) => {
                //         println!("Error: {}", e);
                //     }
                // }
                if let Ok(gpu) = gpu{
                    let system_info = SystemInformation{
                        cpu_name: cpu_brand,
                        total_ram: ram,
                        system_name: system,
                        disks: data,
                        gpu: Some(gpu)
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
                else {
                    let system_info = SystemInformation{
                        cpu_name: cpu_brand,
                        total_ram: ram,
                        system_name: system,
                        disks: data,
                        gpu: None
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



                


            });
        });
    }
    

    #[cfg(target_os = "windows")]
    pub fn get_gpu(){
        
    }
}