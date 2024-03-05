#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use std::{str, collections::HashMap, error::Error, fmt::Display, path::Path, process::Stdio, sync::{mpsc::Sender, Arc}, time::Duration};
use log::{debug, info};
use reqwest::{header::{HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE}, Client};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};
use sysinfo::*;
use serde_json::Value;
use tokio::{io::{self, ErrorKind}, runtime::Handle, spawn};
use crossbeam::channel;
use regex::Regex;
use num_format::{Locale, ToFormattedString};
use futures_util::FutureExt;
use shell_words::{join, split, quote};
use serde_json::json;
use sysinfo::{Components, CpuRefreshKind, Disks, Networks, RefreshKind, System};
use tokio::{sync::{mpsc, Mutex}, time::{self, sleep}};
use rust_socketio::{
    asynchronous::{Client as SocketClient, ClientBuilder},
    Payload
};

use crate::{data::{ComputerData, DriveData, SystemInformation}, ticket_request::request::request_seb_info};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const URL: &str = "wss://axum.master-tech.app";
const SIGNIN_URL: &str = "https://axum.master-tech.app/login";

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

struct WebSocket {
    pub finish: bool,
}

impl WebSocket {
    pub fn new() -> Self {
        WebSocket { finish: false }
    }

    async fn on_message(&mut self, payload: Payload, socket: SocketClient) {
        println!("message: {:#?}", payload);
        socket
            .emit("disconnect", "received message")
            .await
            .expect("Server unreachable");
        self.finish = true;
    }
}

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
                // let gpu =  String::from_utf8(
                //     tokio::process::Command::new("cmd")
                //     .args(["/C", "wmic path win32_VideoController get name"])
                //     .creation_flags(CREATE_NO_WINDOW)
                //     // "-Command {", 
                //     //"(win32_videocontroller | select-object -property Name | ft -autosize -hidetableheaders | out-string).trim()}"
                //     .output()
                //     .await
                //     .expect("msg")
                //     .stdout
                // );

                let mut new_gpu_name = "";
                // let clone_gpu_name = gpu.clone().unwrap_or("no gpu detected".to_string());
                // let parse_gpu_name: Vec<&str> = clone_gpu_name.split("Name").collect();
                // if parse_gpu_name[0].is_empty(){
                //     new_gpu_name = parse_gpu_name.clone()[1].trim();
                // }


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

    pub fn initiate_websocket(run_once: &mut bool) {
        debug!("Starting WS");


        let cookies = CookieStore::default();
        let cookie_store = CookieStoreMutex::new(cookies);
        let cookie_store = Arc::new(cookie_store);

        debug!("Pre Reqwest");

        let client_build = Client::builder()
            .cookie_provider(std::sync::Arc::clone(&cookie_store))
            .build();

        debug!("Post Build");
        match client_build{
            Ok(client) => {
                debug!("Sending reqwest");
                spawn(async move {
                    Self::login(client, cookie_store).await;
                });
            }, Err(err) => debug!("Error with client_build => {err:?}"),
        };
    }
    
    async fn login(client: Client, cookie_store: Arc<CookieStoreMutex>) {
        let params = json!({
            "name": "",
            "email": "logan@test.com",
            "password": "Poolparty10!9",
            "store": "",
            "everest_initials": ""
        });

        let res = client.post(SIGNIN_URL) 
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json") // .header(COOKIE, HeaderValue::from_static("jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzUxMiJ9.eyJpYXQiOjE3MDU4OTA4MzAsIm5iZiI6MTcwNTg5MDgzMCwiZXhwIjoxNzA1OTc3MjMwLCJpc3MiOiJTdXJyZWFsREIiLCJOUyI6Ik1hc3RlcnRlY2giLCJEQiI6Ik1hc3RlcnRlY2hEQiIsIlNDIjoidXNlciIsIklEIjoidXNlcjpqbTlhN2wzdjMyZ3NpY2NyN3BndyJ9.YtTKxOAMfsR5sxFcNAxtrAx9VHL7kqR8tnmPQXnSm2nI_xEWVGPI8Cu5C12zHb4a9Xq5D7PkfY9suGrBirYeJg"))
            .json(&params)
            .send()
            .await;

        debug!("Post Reqwest");
        let mut cookie: Option<&str> = None;
        let mut cookie_string = String::new();

        match res{
            Ok(response) => {
                // let res: String = response.text().await.unwrap();
                debug!("Response => {response:?}");

                let store = cookie_store.lock().unwrap();
                let next_cookie = store.iter_any().next();
                cookie_string = next_cookie.unwrap().to_string().clone();
                let cookie_split: Vec<&str> = cookie_string.split("jwt=").collect();
                let y = cookie_split.get(1).copied();
                cookie = y;
            }Err(err) => {
                debug!("error with mastertech.app req => {err:?}");
                cookie = None;
            }
        };


        let app: Arc<Mutex<WebSocket>> = Arc::new(Mutex::new(WebSocket::new()));
        let event_app: Arc<Mutex<WebSocket>> = app.clone();

        debug!("Using jwt? {:?}", cookie.clone());

        let socket = ClientBuilder::new(URL)
        .transport_type(rust_socketio::TransportType::Websocket)
            .namespace("/ws")
            // .opening_header("jwt", cookie.unwrap_or("Nil"))
            
            .on("open", |_, client| async move{
                client.emit("join", json!({"room": "RIV"})).await.unwrap();
            }.boxed())
            .on("connect", |_, client| async move{
                client.emit("join", json!({"room": "RIV"})).await.unwrap();
            }.boxed())
            .on("close", |_, client| async move { 
                sleep(Duration::from_secs(3)).await;
                client.emit("join", json!({"room": "RIV"})).await.unwrap();
                println!("Disconnected");
            }.boxed())
            .on("join", |msg, _| async move { println!("Joined") }.boxed())
            .on("command", | payload: Payload, client: SocketClient | async move {
                match payload{
                    Payload::Binary(bin_payload) => {
                        println!("bin_payload: {:#?}", bin_payload);
                        let command_payload = split(str::from_utf8(&bin_payload.to_vec()).unwrap());
                        // let command_payload = split(bin_payload.to_vec().);
    
                        let process: std::process::Output = std::process::Command::new("sh")
                            .arg("-c")
                            .args(command_payload.unwrap())
                            .stdout(Stdio::piped())
                            .spawn()
                            .unwrap()
                            .wait_with_output()
                            .unwrap();
    
                    let _ = client.emit("clientCmdResponse", json!({"message": process.stdout}));
                        // println!("{bin_payload:#?}");
                    },
                    Payload::String(string_payload) => {
                        println!("string_payload: {}", string_payload.clone());
                        if string_payload.contains("cd"){
                            // TODO: need to find a way to keep track of current directory using this
                            // std::env::set_current_dir(&path)
                        }
                        let command_payload = split(string_payload.as_str());
                        let process = tokio::process::Command::new("sh")
                            .arg("-c")
                            .args(command_payload.unwrap())
                            .stdout(Stdio::piped())
                            .spawn()
                            .unwrap()
                            .wait_with_output()
                            .await
                            .unwrap();
    
                        let result: Vec<u8> = process.stdout;
    
                        client.emit(
                        "clientCmdResponse", 
                           json!({"message": String::from_utf8(result).unwrap()})
                        ).await
                        .unwrap();
                    }
                }
            }.boxed())
            .on("message", move|msg, client| {
                let x = event_app.clone();
                async move { x.lock().await.on_message(msg, client).await }.boxed()
            })
            .on("error", |err, _| {
                async move { eprintln!("Error: {:#?}", err) }.boxed()
            })
            .connect()
            .await
            .expect("Connection failed");
    
        socket.emit("join", json!({"room": "RIV"})).await.unwrap();
        // socket.emit("command", json!({"command": "LIST", "room": "RIV"})).await.unwrap();
        // let msg: Vec<u8> = "hello from client".as_bytes().to_vec();
    
        let json_payload = json!({
            "room": "RIV",
            "sysinfo": Self::get_sysinfo().await,
            "hostname": "shadowbrokerPC"
        }); 
        
        sleep(Duration::from_secs(2)).await;
        // Spawn a task or run a loop that listens for a shutdown signal
        // tokio::spawn(async move {
            loop{
                sleep(Duration::from_secs(2)).await;
    
                socket.emit("clientSysInfo", json_payload.clone())
                    .await
                    .unwrap();
    
                debug!("in loop");
    
                if app.lock().await.finish {
                    break;
                }
            }
        // });
        socket.disconnect().await.unwrap()
    }
    async fn get_sysinfo() -> SystemInformation {
        let mut sys = System::new_all();
        // First we update all information of our `System` struct.
        sys.refresh_all();
    
        let mut cpu_percentage = f32::default();
        let mut cpu_clock = u64::default();
        let mut disks = String::new();
        let mut network_interfaces: HashMap<String, String> = HashMap::new();
        let mut component_temps: HashMap<String, f32> = HashMap::new();
        // RAM and swap information:
        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();
        // Display system information:
        let name = System::name().unwrap();
        let kernel_version = System::kernel_version().unwrap();
        let os_version = System::os_version().unwrap();
        let hostname = System::host_name().unwrap();
    
        // Number of CPUs:
        let number_of_cpus = format!("NB CPUs: {} \n", sys.cpus().len());
    
        // Display processes ID, name na disk usage:
        // for (pid, process) in sys.processes() {
        //     println!("[{pid}] {} {:?}", process.name(), process.disk_usage());
        // }
    
        let disk_list = Disks::new_with_refreshed_list();
        for disk in &disk_list {
            disks += format!("{disk:?}").as_str();
        }
    
        // Network interfaces name, total data received and total data transmitted:
        let networks = Networks::new_with_refreshed_list();
    
        for (interface_name, data) in &networks {
            if data.total_received() > 1 {
                let up_down = format!("{}/{}", data.total_received(), data.total_transmitted());
                network_interfaces.insert(interface_name.clone(), up_down);
            }
        }
    
        // Components temperature:
        let components = Components::new_with_refreshed_list();
    
        // let component_temp = String::new();
    
        for component in &components {
            // component_temp += format!("{}/{}", component.temperature(), component.max()).as_str();
            component_temps.insert(component.label().to_string(), component.temperature());
    
            // comps += format!("{component:#?} \n", component.).as_str();
        }
    
        let mut s =
            System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::everything()));
    
        let sysinf: SystemInformation;
        //loop{
        std::thread::sleep(Duration::from_millis(600));
    
        s.refresh_cpu(); // Refreshing CPU information.
        for cpu in s.cpus() {
            cpu_percentage = cpu.cpu_usage();
            cpu_clock = cpu.frequency();
        }
    
        sysinf = SystemInformation {
            cpu_percentage,
            cpu_clock,
            component_temps,
            disks,
            total_memory,
            used_memory,
            name,
            kernel_version,
            os_version,
            hostname,
            number_of_cpus,
            network_interfaces,
        };
    
        println!("SystemInfo: \n{sysinf}");
    
        return sysinf;
        //}
    }
}



impl Display for SystemInformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "==> cpu_percentage: {} \n==> comps: {:?} \n==> used_memory: {} \n==> total_memory: {} \n==> disks: {} \n==> name: {} \n==> kernel_version: {} \n==> os_version: {} \n==> hostname: {} \n==> number_of_cpus: {} \n==> network_interfaces: {:#?} \n", 
            self.cpu_percentage,
            self.component_temps,
            self.used_memory,
            self.total_memory,
            self.disks,
            self.name,
            self.kernel_version,
            self.os_version,
            self.hostname,
            self.number_of_cpus,
            self.network_interfaces,
        )
    }
}