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
}

#[derive(Serialize, Deserialize)]
pub struct DiskData {
    pub disks: Vec<Value>,
}

impl DiskData {
    fn new() -> Self {
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
                let mut sys = System::new_all(); // Create `System` struct.

                let cpu_brand = sys.cpus()[0].brand().to_string();
                let ram = (sys.total_memory() / ( 1024 * 1024 * 1024)).to_string();
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
                
                // String for each disk: [name] [letter]:\\ [ Available space / Total space ]
                let system_info = SystemInformation{
                    cpu_name: cpu_brand,
                    total_ram: ram,
                    system_name: system,
                    disks: data
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
                


            });
        });
    }
    

    #[cfg(target_os = "windows")]
    pub fn get_gpu(){
        let gpu = std::process::Command::new("cmd").args(["/C", "wmic path win32_VideoController get name"]).output();
        match gpu{
            Ok(_) => {

            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}