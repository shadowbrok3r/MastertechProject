#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
use std::{collections::HashMap, error::Error, fmt::Display, path::Path, process::{Output, Stdio}, str, sync::{mpsc::Sender, Arc}, time::Duration};
use anyhow::{anyhow, Context};
use dotenv::dotenv;
use log::{debug, info};
use reqwest::{header::{HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE}, Client};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};
use sysinfo::*;
use serde_json::Value;
use tokio::{io::{self, ErrorKind}, process::{Child, Command}, runtime::Handle, spawn};
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
use crate::{database::{get_cookie, schema::{ComputerData, DriveData, LocalSebData}, SystemInformation}, handle_api::{api_request::request_seb_info, Store}};
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    pub async fn get_computer_data() -> anyhow::Result<ComputerData, anyhow::Error>{
        
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
        let seb_data = request_seb_info(client, None)
            .await
            .or_else(|err|{
                info!("Error: {:?}", err.to_string());
                Err(err)
            }).and_then(|data|{
                info!("Pulled SEB Data successfully: {data:#?}");
                Ok(data)
        }); 

        let seb_info: Option<LocalSebData>;

        if let Ok(seb) = seb_data{seb_info = Some(seb);
        }else {seb_info = None;}

        #[cfg(target_os = "windows")]
        {
            let gpu =  String::from_utf8(
                tokio::process::Command::new("cmd")
                .args(["/C", "wmic path win32_VideoController get name"])
                .creation_flags(CREATE_NO_WINDOW)
                // "-Command", "{(win32_videocontroller | select-object -property Name | ft -autosize -hidetableheaders | out-string).trim()}"
                .output()
                .await?
                .stdout
            );

            let mut new_gpu_name = "";
            let clone_gpu_name = gpu.clone().unwrap_or("no gpu detected".to_string());
            let parse_gpu_name: Vec<&str> = clone_gpu_name.split("Name").collect();
            if parse_gpu_name[0].is_empty(){
                new_gpu_name = parse_gpu_name.clone()[1].trim();
            }


            let system_info = ComputerData{
                id: None,
                customer: None,
                cpu,
                ram,
                operating_system,
                drives: data.drives,
                gpu: new_gpu_name.to_string(),
                hostname,
                seb_info
            };

            Ok(system_info)
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
                    .await?
                    .stdout
            );
            if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str()){
                let full_gpu_name = &captures[1];
                gpu_name = full_gpu_name.split_whitespace().take(3).collect::<Vec<&str>>().join(" ");
            }

            let system_info = ComputerData{
                cpu,
                ram,
                hostname,
                drives: data.drives,
                gpu: gpu_name,// gpu_name),
                operating_system,
                seb_info,
                id: None,
                customer: None,
            };

            Ok(system_info)
        }
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

    pub async fn initialize_websocket(client_uuid: Uuid)-> anyhow::Result<String, anyhow::Error>{
        let app: Arc<Mutex<WebSocket>> = Arc::new(Mutex::new(WebSocket::new()));
        let event_app: Arc<Mutex<WebSocket>> = app.clone();

        let socket_io_url = "wss://axum.master-tech.app";// "ws://localhost:4000";// "wss://axum.master-tech.app";

        let cookie = get_auth().await?;
        let cookie_clone = cookie.clone();


        let socket = ClientBuilder::new(socket_io_url)
            .transport_type(rust_socketio::TransportType::Websocket)
            .opening_header("cookie", cookie_clone)
            .namespace("/ws") 
            .on("open", |_, client| async move{
                info!("open => socket opened");
                client.emit("join", json!({"room": "RIV"})).await.unwrap_or(());
                info!("connect => sent join event");
            }.boxed())
            .on("connect", |_, client| async move{
                info!("connect => client connected");
                client.emit("join", json!({"room": "RIV"})).await.unwrap_or(());
            }.boxed())
            .on("close", |_, client| async move { 
                info!("close => attempting reconnect");
                sleep(Duration::from_secs(3)).await;
                client.emit("join", json!({"room": "RIV"})).await.unwrap_or(());
                info!("Disconnected");
            }.boxed())
            .on("join", |_msg, _| async move { info!("Joined") }.boxed())
            .on("command", | payload: Payload, client: SocketClient | async move {
                match payload{
                    Payload::Binary(bin_payload) => { println!("bin_payload: {:#?}", bin_payload); },
                    Payload::Text(text_payload) => { info!("Got a Text payload: {:?}", text_payload.clone()); /* Self::handle_command_payload(string_payload, client).await; */ },
                    Payload::String(string_payload) => { Self::handle_command_payload(string_payload, client).await.unwrap_or(()); },
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
            .await?;
        
    
        socket.emit("join", json!({"room": "RIV"})).await?;

        loop{ // constantly send information as well as wait for shutdown signal
            sleep(Duration::from_secs(2)).await;

            let systeminfo: SystemInformation = Self::get_sysinfo().await?;

            let json_payload = json!({
                "room": "RIV",
                "sysinfo": systeminfo,
                "client_uuid": client_uuid.to_string()
            }); 

            let _ = socket.emit("clientSysInfo", json_payload.clone())
                .await?;

            info!("in loop");

            if app.lock().await.finish {
                break;
            }
        }
        socket.disconnect().await?;
       
       Ok(format!("Socket connected"))
    }
    
    async fn handle_websocket(){
        
    }

    async fn handle_command_payload(string_payload: String, client: SocketClient) -> anyhow::Result<(), anyhow::Error>  { 
        println!("string_payload: {}", string_payload.clone());
        if string_payload.contains("cd"){
            // TODO: need to find a way to keep track of current directory using this
            // std::env::set_current_dir(&path)
        }
        let command_payload = split(string_payload.as_str()).unwrap_or(Vec::new());
        if cfg!(target_os = "windows"){
            Self::handle_windows_cmd(command_payload, client).await?;
        }else if cfg!(target_os = "linux"){
            Self::handle_linux_cmd(command_payload, client).await?;
        }

        Ok(())
    }

    async fn handle_windows_cmd(command_payload: Vec<String>, client: SocketClient)-> anyhow::Result<(), anyhow::Error> {
        let process = Command::new("cmd")
            .arg("/C")
            .args(command_payload)
            .stdout(Stdio::piped())
            .spawn();
        
        match process{
            Ok(child) => {
                let output = child.wait_with_output().await?;
                
                client.emit(
                    "clientCmdResponse", 
                    json!({"message": String::from_utf8(output.stdout)?})
                ).await?;
            },
            Err(err) =>{
                info!("Error in process => {err:?}");
            },
        }

        Ok(())
    }

    async fn handle_linux_cmd(command_payload: Vec<String>, client: SocketClient)-> anyhow::Result<(), anyhow::Error> {
        let process = Command::new("sh")
            .arg("-c")
            .args(command_payload)
            .stdout(Stdio::piped())
            .spawn();
            
        match process{
            Ok(child) => {
                let output = child
                    .wait_with_output()
                    .await?;

                client.emit(
                    "clientCmdResponse", 
                    json!({"message": String::from_utf8(output.stdout)?})
                ).await?;
            },
            Err(err) =>{
                info!("Error in process => {err:?}");
            },
        }
        Ok(())
    }

    async fn get_sysinfo() -> anyhow::Result<SystemInformation, anyhow::Error> {
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

        Ok(sysinf)
    }
}

async fn get_auth() -> anyhow::Result<String, anyhow::Error>{
    let api_url = "https://axum.master-tech.app"; // "http://localhost:4000";// "https://axum.master-tech.app";


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

    client?.post(format!("{api_url}/login")) 
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&params)
        .send()
        .await?;

    info!("Sent signin req");

    let cookie = get_cookie(cookie_store.lock().unwrap());

    Ok(cookie)

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