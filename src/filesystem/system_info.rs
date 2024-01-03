#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use std::{collections::HashMap, sync::{Arc, mpsc::Sender}, path::Path, error::Error};
use log::{debug, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sysinfo::*;
use serde_json::Value;
use tokio::{io::{self, ErrorKind}, runtime::Handle};
use crossbeam::channel;
use regex::Regex;
use num_format::{Locale, ToFormattedString};
use crate::{data::{ComputerData, DriveData}, ticket_request::request::request_seb_info};

const CREATE_NO_WINDOW: u32 = 0x08000000;

// pub struct RetrieveSystemInfo {
//     pub tx: std::sync::mpsc::Sender<String>,
// }
// impl RetrieveSystemInfo{
    // #[cfg(target_os="windows")]
    // pub fn get_antivirus() -> io::Result<Vec<(String, Option<bool>)>> {
    //     let (sender, receiver) = channel::unbounded();
    //     let av_to_search = vec![
    //         "mbam", // MALWAREBYTES
    //         "aswtoolssvc", // AVAST
    //         "avgToolsSvc", // AVG
    //         "mcuicnt", // MCAFFEE
    //         "norton", 
    //         "wrsa", // WEBROOT
    //         "egui", // ESET
    //         "superantispyware" // SUPERANTI
    //         ];          
    //         let antivirus_mapping = Arc::new([
    //             ("mbam", "Malwarebytes"),
    //             ("aswtoolssvc", "Avast"),
    //             ("avgToolsSvc", "AVG"),
    //             ("mcuicnt", "McAfee"),
    //             ("norton", "Norton"),
    //             ("wrsa", "Webroot"),
    //             ("egui", "ESET"),
    //             ("superantispyware", "SuperAntiSpyware"),
    //         ].iter().cloned().collect::<HashMap<&str, &str>>());
    //     for antivirus in av_to_search.clone().into_iter() {
    //         let sender = sender.clone();
    //         let antivirus_mapping = Arc::clone(&antivirus_mapping);
    //         tokio::spawn(async move {
    //             let where_cmd = ["where", "/r", "C:\\Program Files", antivirus];
    //             let output = tokio::process::Command::new("cmd")
    //                 .args(&["/C"])
    //                 .args(where_cmd)
    //                 .creation_flags(CREATE_NO_WINDOW)
    //                 .output()
    //                 .await
    //                 .map_err(|e| io::Error::new(ErrorKind::Other, format!("Failed to execute command: {}", e)))?;
    //                 let exists = if output.stdout.is_empty() {
    //                     None
    //                 } else {
    //                     Some(true)
    //                 };
    //                 let name = antivirus_mapping.get(antivirus).unwrap_or(&antivirus);
    //                 sender.send((name.to_string(), exists)).map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to send data through channel"))
    //         });
    //     }
    //     let mut antivirus_exists = Vec::new();
    //     for _ in 0..av_to_search.len() {
    //         let exists = receiver.recv().map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "Failed to receive data from channel"))?;
    //         antivirus_exists.push(exists);
    //     }
    //     Ok(antivirus_exists)
    // }
// }

impl ComputerData{
    pub fn get_computer_data() -> Result<ComputerData, Box<dyn Error>>{
        let (tx, rx) = std::sync::mpsc::channel();
        tokio::spawn(async move {

            let sys = System::new_all(); // Create `System` struct.

            let cpu = sys.cpus()[0].brand().to_string();
            let ram = (sys.total_memory() / ( 1024 * 1024 * 1024 ) + 1).to_formatted_string(&Locale::en);
            let operating_system = System::long_os_version().unwrap_or_default(); //sys.long_os_version().unwrap_or_else(|| "<unknown>".to_owned());
            let mut disks = Disks::new_with_refreshed_list();
            let hostname = System::host_name().unwrap_or_default();

            // let mut data = DiskData::new();
            let mut data = ComputerData::new();

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
            let client = Client::new();
            let seb_data = request_seb_info(client)
                .await
                .or_else(|err|{
                    debug!("Error: {:?}", err.to_string());
                    Err(err)
                }).and_then(|data|{
                    info!("Pulled SEB Data successfully: {data:#?}");
                    Ok(data)
            }); 

            let seb_info: Option<crate::data::LocalSebData>;
            if let Ok(seb) = seb_data{
                seb_info = Some(seb);
            }else {
                seb_info = None;
            }

            #[cfg(target_os = "windows")]
            {
                let gpu = 
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
                let clone_gpu_name = gpu.clone().unwrap_or("no gpu detected".to_string());
                let parse_gpu_name: Vec<&str> = clone_gpu_name.split("Name").collect();
                if parse_gpu_name[0].is_empty(){
                    new_gpu_name = parse_gpu_name.clone()[1].trim();
                }


                let system_info = ComputerData{
                    cpu,
                    ram,
                    operating_system,
                    drives: data.drives,
                    gpu: Some(new_gpu_name.to_string()),
                    hostname,
                    seb_info,
                };

                match tx.send(system_info){
                    Ok(_) => info!("sent computer data"),
                    Err(e) => debug!("Error sending computer data: {e:?}"),
                }
            }


            #[cfg(target_os = "linux")]
            {
                // let re = Regex::new(r"\[(.*)\]").unwrap();
                // let mut gpu_name = String::new();
                // let gpu = 
                // String::from_utf8(
                //     tokio::process::Command::new("sh")
                //         .arg("-c")
                //         .arg("lspci | grep VGA")
                //         .output()
                //         .await
                //         .unwrap()
                //         .stdout
                // );
                // if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str()){
                //     let full_gpu_name = &captures[1];
                //     gpu_name = full_gpu_name.split_whitespace().take(3).collect::<Vec<&str>>().join(" ");
                // }
    
                let system_info = ComputerData{
                    cpu,
                    ram,
                    hostname,
                    drives: data.drives,
                    gpu: Some( "Todo".to_string() ),// gpu_name),
                    operating_system,
                    seb_info,
                };

                match tx.send(system_info){
                    Ok(_) => info!("sent computer data"),
                    Err(e) => debug!("Error sending computer data: {:?}", e.0),
                }
            }
        });

        match rx.recv(){
            Ok(data) => {
                info!("Received computer data");
                Ok(data)
            },
            Err(e) => {
                debug!("Error receiving data: {e}");
                Err(Box::new(e))
            },
        }
            
        // Ok(system_info)
    }

    #[cfg(target_os="windows")]
    pub fn get_antivirus() -> io::Result<Vec<(String, Option<bool>)>> {
        let (sender, receiver) = channel::unbounded();
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
}

