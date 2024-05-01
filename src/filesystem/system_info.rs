#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use std::{collections::HashMap, error::Error, fmt::Display, path::Path, process::{Output, Stdio}, str, sync::{mpsc::Sender, Arc}, time::Duration};
use anyhow::anyhow;
use dotenv::dotenv;
use log::{debug, info};
use reqwest::{header::{HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE}, Client};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};
use sysinfo::*;
use serde_json::Value;
use tokio::{io::{self, ErrorKind}, process::Child, runtime::Handle, spawn};
use crossbeam::channel;
use regex::Regex;
use num_format::{Locale, ToFormattedString};
use futures_util::FutureExt;
use shell_words::{join, quote, split, ParseError};
use serde_json::json;
use sysinfo::{Components, CpuRefreshKind, Disks, Networks, RefreshKind, System};
use tokio::{sync::{mpsc, Mutex}, time::{self, sleep}};
use rust_socketio::{
    asynchronous::{Client as SocketClient, ClientBuilder},
    Payload
};
use uuid::Uuid;

use crate::{data::{get_cookie, ComputerData, DriveData, SystemInformation}, ticket_request::{request::request_seb_info, Store}};

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

#[derive(Debug, Deserialize, Serialize)]
pub struct Auth {
    pub session_id: Option<String>,
    pub username: String,
    pub room: Store,
}

struct WebSocket {
    pub finish: bool,
}

impl WebSocket {
    pub fn new() -> Self {
        WebSocket { finish: false }
    }

    async fn on_message(&mut self, payload: Payload, socket: SocketClient) {
        info!("error: {:#?}", payload);
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
                    info!("Error: {:?}", err.to_string());
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
                    Err(e) => info!("Error sending computer data: {e:?}"),
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
                    Err(e) => info!("Error sending computer data: {:?}", e.0),
                }
            }
        });

        match rx.recv(){
            Ok(data) => {
                info!("Received computer data");
                Ok(data)
            },
            Err(e) => {
                info!("Error receiving data: {e}");
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

    pub fn initialize_websocket(client_uuid: Uuid)-> String{
        let app: Arc<Mutex<WebSocket>> = Arc::new(Mutex::new(WebSocket::new()));
        let event_app: Arc<Mutex<WebSocket>> = app.clone();

        let socket_io_url = "ws://localhost:4000";// "wss://axum.master-tech.app";

        tokio::spawn(async move{
            let cookie = match get_auth().await{
                Ok(cookie) => {
                    Ok(cookie)
                },
                Err(err) => {
                    info!("Could not retrieve cookie: {err:?}");
                    Err(err)
                },
            };

            let cookie_clone = cookie.unwrap().clone();
            let mut auth = Auth{username: "Mastertech".to_string(), room: Store::RIV, session_id: None };

            let socket = ClientBuilder::new(socket_io_url)
                .transport_type(rust_socketio::TransportType::Websocket)
                .auth(serde_json::to_value(auth).unwrap())
                .opening_header("cookie", cookie_clone)
                .namespace("/ws") // .opening_header("jwt", cookie.unwrap_or("Nil"))
                .on("open", |_, client| async move{
                    info!("open => socket opened");
                    client.emit("join", json!({"room": "RIV"})).await.unwrap();
                    info!("connect => sent join event");
                }.boxed())
                .on("connect", |_, client| async move{
                    info!("connect => client connected");
                    client.emit("join", json!({"room": "RIV"})).await.unwrap();
                }.boxed())
                .on("close", |_, client| async move { 
                    info!("close => attempting reconnect");
                    sleep(Duration::from_secs(3)).await;
                    client.emit("join", json!({"room": "RIV"})).await.unwrap();
                    info!("Disconnected");
                }.boxed())
                .on("join", |msg, _| async move { info!("Joined") }.boxed())
                .on("session", |msg: Payload, _| async move { 
                    match msg{
                        Payload::Binary(bin_payload) => { println!("bin_payload: {:#?}", bin_payload); },
                        Payload::Text(text_payload) => { info!("Got a Text payload: {:?}", text_payload.clone()); /* Self::handle_command_payload(string_payload, client).await; */ },
                        Payload::String(string_payload) => { 
                            let _auth = Auth{session_id: Some(string_payload), username: "Mastertech".to_string(), room: Store::RIV};
                         },
                    }
                    
                }.boxed())
                .on("command", | payload: Payload, client: SocketClient | async move {
                    match payload{
                        Payload::Binary(bin_payload) => { println!("bin_payload: {:#?}", bin_payload); },
                        Payload::Text(text_payload) => { info!("Got a Text payload: {:?}", text_payload.clone()); /* Self::handle_command_payload(string_payload, client).await; */ },
                        Payload::String(string_payload) => { Self::handle_command_payload(string_payload, client).await; },
                    }
                }.boxed())
                .on("error", move|err, client| {
                    let x = event_app.clone();
                    async move { x.lock().await.on_message(err, client).await }.boxed()
                })
                .on("message", |msg, _| {
                    async move { info!("Received message: {:#?}", msg) }.boxed()
                })
                .connect()
                .await
                .unwrap();
            
        
            socket.emit("join", json!({"room": "RIV"})).await.unwrap();
            // socket.emit("command", json!({"command": "LIST", "room": "RIV"})).await.unwrap();
            // let msg: Vec<u8> = "hello from client".as_bytes().to_vec();

            
            // Spawn a task or run a loop that listens for a shutdown signal
            // tokio::spawn(async move {
            loop{
                sleep(Duration::from_secs(1)).await;

                let systeminfo: SystemInformation = Self::get_sysinfo().await;

                let json_payload = json!({
                    "room": "RIV",
                    "sysinfo": systeminfo,
                    "client_uuid": client_uuid.to_string()
                }); 

                let socket_event = socket.emit("clientSysInfo", json_payload.clone())
                    .await.unwrap();

                info!("in loop");

                if app.lock().await.finish {
                    break;
                }
            }
            socket.disconnect().await.unwrap();
        });
       
       return format!("Socket connected");
    }
    
    async fn handle_websocket(){
        
    }

    async fn handle_command_payload(string_payload: String, client: SocketClient) 
    { // -> Result<Vec<String>, ParseError>
        println!("string_payload: {}", string_payload.clone());
        if string_payload.contains("cd"){
            // TODO: need to find a way to keep track of current directory using this
            // std::env::set_current_dir(&path)
        }
        let command_payload = split(string_payload.as_str()).unwrap();
        if cfg!(target_os = "windows"){
            let _ = Self::handle_windows_cmd(command_payload, client).await;
        }else if cfg!(target_os = "linux"){
            let _ = Self::handle_linux_cmd(command_payload, client).await;
        }

        
    }

    async fn handle_windows_cmd(command_payload: Vec<String>, client: SocketClient)
    { // -> Result<Child, dyn Error>
        let process = tokio::process::Command::new("cmd")
        .arg("/C")
        .args(command_payload)
        .stdout(Stdio::piped())
        .spawn();
        
        match process{
            Ok(child) => {
                let output = child
                    .wait_with_output()
                    .await;
                
                match output{
                    Ok(out) => {
                        let result: Vec<u8> = out.stdout;

                        let socket_emit = client.emit(
                        "clientCmdResponse", 
                        json!({"message": String::from_utf8(result).unwrap()})
                        ).await;

                        match socket_emit{
                            Ok(_) => info!("Emit socket event successfully"),
                            Err(e) => info!("Error emitting socket event: {e:?}"),
                        }
                    },
                    Err(e) => info!("Error reading Output => {e:?}")
                }
            },
            Err(err) =>{
                info!("Error in process => {err:?}");
            },
        }
    }

    async fn handle_linux_cmd(command_payload: Vec<String>, client: SocketClient)
    { // -> Result<Child, dyn Error>
        let process = tokio::process::Command::new("sh")
            .arg("-c")
            .args(command_payload)
            .stdout(Stdio::piped())
            .spawn();
            
        match process{
            Ok(child) => {
                let output = child
                    .wait_with_output()
                    .await;
                
                match output{
                    Ok(out) => {
                        let result: Vec<u8> = out.stdout;

                        let socket_emit = client.emit(
                        "clientCmdResponse", 
                        json!({"message": String::from_utf8(result).unwrap()})
                        ).await;

                        match socket_emit{
                            Ok(_) => info!("Emit socket event successfully"),
                            Err(e) => info!("Error emitting socket event: {e:?}"),
                        }
                    },
                    Err(e) => info!("Error reading Output => {e:?}")
                }
            },
            Err(err) =>{
                info!("Error in process => {err:?}");
            },
        }
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
        std::thread::sleep(Duration::from_millis(200));
    
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
        info!("SystemInfo: \n{sysinf}");
        return sysinf;
    }
}

async fn get_auth() -> Result<String, anyhow::Error>{
    let api_url = "http://localhost:4000";// "https://axum.master-tech.app";


    let params = json!({
        "name": "Logan",
        "email": "logan.lees@pclaptops.com",
        "password": "Poolparty10!9",
        "store": "RIV",
        "everest_initials": "LL"
    });

    info!("Sending signin req");

    let cookies = CookieStore::default();
    let cookie_store = CookieStoreMutex::new(cookies);
    let cookie_store  = Arc::new(cookie_store);

    let client_build = reqwest::Client::builder()
        .cookie_provider(std::sync::Arc::clone(&cookie_store))
        .build();

    let client = match client_build{
        Ok(client) => {
            info!("Sending reqwest");
            Ok(client)
        }, Err(err) => {info!("Error with client_build => {err:?}"); Err(err)},
    };

    let signin_response = client.unwrap().post(format!("{api_url}/login")) 
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await;

    let mut cookie_string = String::new();
    let mut cookie: &str = "";

    info!("Sent signin req");

    match signin_response{
        Ok(response) => {
            info!("Response => {response:?}");

            let cookie = get_cookie(cookie_store.lock().unwrap());
            Ok(cookie)
        },
        Err(err) => {
            info!("error with mastertech.app req => {err:?}");
            Err(anyhow!(err))
        }
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