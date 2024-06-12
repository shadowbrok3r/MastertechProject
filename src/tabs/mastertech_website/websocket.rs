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
use crate::{database::{schema::{ComputerData, DriveData, LocalSebData}, SystemInformation}, handle_api::{api_request::request_seb_info, Store}};
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct WebSocket {
    pub finish: bool,
    pub url: String,
    pub error: String,
    // pub frontend: Option<FrontEnd>,
}

impl Default for WebSocket{
    fn default() -> Self {
        Self {
            finish: false,
            url: String::new(),
            error: String::new(),
            // frontend: None,
        }
    }
}

impl WebSocket {
    async fn on_message(&mut self, payload: Payload, socket: SocketClient) {
        info!("error: {:#?}", payload);
        socket
            .emit("disconnect", "received message")
            .await
            .expect("Server unreachable");
        self.finish = true;
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

// struct FrontEnd {
//     ws_sender: WsSender,
//     ws_receiver: WsReceiver,
//     events: Vec<WsEvent>,
//     text_to_send: String,
// }

// impl FrontEnd {
//     fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
//         Self {
//             ws_sender,
//             ws_receiver,
//             events: Default::default(),
//             text_to_send: Default::default(),
//         }
//     }

//     fn ui(&mut self, ctx: &egui::Context) {
//         while let Some(event) = self.ws_receiver.try_recv() {
//             self.events.push(event);
//         }

//         egui::CentralPanel::default().show(ctx, |ui| {
//             ui.horizontal(|ui| {
//                 ui.label("Message to send:");
//                 if ui.text_edit_singleline(&mut self.text_to_send).lost_focus()
//                     && ui.input(|i| i.key_pressed(egui::Key::Enter))
//                 {
//                     self.ws_sender
//                         .send(WsMessage::Text(std::mem::take(&mut self.text_to_send)));
//                 }
//             });

//             ui.separator();
//             ui.heading("Received events:");
//             for event in &self.events {
//                 ui.label(format!("{event:?}"));
//             }
//         });
//     }
// }



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


/* 
    pub async fn new_websocket_connection(client_uuid: Uuid, _disconnect: bool)-> anyhow::Result<String, anyhow::Error>{
        let app: Arc<Mutex<WebSocket>> = Arc::new(Mutex::new(WebSocket::default()));
        let event_app: Arc<Mutex<WebSocket>> = app.clone();  
        let socket_io_url = "".to_string();
        
        let socket = ClientBuilder::new(socket_io_url)
            .transport_type(rust_socketio::TransportType::Websocket)
            // .opening_header("cookie", cookie_clone)
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

            if app.lock().await.finish{
                break;
            }
        }
        socket.disconnect().await?;
    
    Ok(format!("Socket connected"))
    }
*/