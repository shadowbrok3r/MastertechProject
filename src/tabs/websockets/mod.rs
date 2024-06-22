use std::{collections::HashMap, env, process::Stdio, time::Duration};
use anyhow::Context;
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{CentralPanel, Color32, Key, TextEdit, TopBottomPanel, Ui, Widget};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use log::debug;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shell_words::split;
use surrealdb::sql::Thing;
use sysinfo::{Components, CpuRefreshKind, Disks, Networks, RefreshKind, System};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command, spawn, time::sleep};
use tracing::info;

use crate::{app_state::MastertechContext, database::{deserialize_command, schema::{ClientId, ComputerId, ConnectedClient, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE}, serialize_system_info, SystemInformation}};
use tui_input::Input;
pub mod websocket;

impl MastertechContext{
    pub fn websockets(&mut self, ui: &mut Ui) {
        // if let Some(frontend) = &mut self.frontend {
        //     self.terminal
        //         .draw(|frame| {
        //             let _area = frame.size();
        //             // render_chart1(frame, area, &app);
        //             frontend.ui(ui);
        //         })
        //     .expect("epic fail");
        // }
        // ui.add( self.terminal.backend_mut());
        // self.terminal.show_cursor().unwrap();

        let _db_tx = self.db_tx.clone();

        if self.current_user.is_none(){
            let _ = self.app_state_tx.send(crate::app_state::AppState::NoAuth("No User".to_string()));
        }
        
        ui.vertical_centered(|ui| {
            if ui.button("Connect").clicked()
            {
                if let Some(db) = self.database.clone(){
                    let client_hash = generate_client_id(self.system_info.hostname.clone(), self.system_info.cpu.trim().to_string());
                    let url_string = format!("{}:{}", self.system_info.hostname.clone(), client_hash.split_at(9).0);
                    info!("url_string: {}", url_string.clone());

                    self.url = Some(format!("ws://127.0.0.1:8081/websocket?room_id={}&role=client",  url_string.clone()));
                    info!("url: {:?}", self.url.clone());
                    let computer_id = &self.system_info.id.clone().unwrap_or( // i need to first check if a computer exists with a customer id or something..
                        ComputerId(
                            Thing::from(
                                (COMPUTER_TABLE,  url_string.clone().as_str())
                            )
                        )
                    );
                    
                    self.client_uuid = Some(
                        ClientId(
                            Thing::from((CONNECTED_CLIENT_TABLE.to_string(), computer_id.0.id.clone()))
                        )
                    );

                    let connected_client = ConnectedClient {
                        id: self.client_uuid.clone(),
                        client_hash,
                        ..Default::default()
                    };


                    info!("Client: {:?}", connected_client);

                    let tx = self.connected_clients_tx.clone();
                    spawn(async move {
                        let res: Result<Vec<ConnectedClient>, surrealdb::Error> = db.database
                            .query("CREATE connected_client CONTENT $content")
                            .bind(("content", connected_client.clone()))
                            .await
                            .unwrap().take(0);
                        match res{
                            Ok(data) => tx.try_send(data.clone()).unwrap(),
                            Err(e) => debug!("db error: {e:?}"),
                        }
                    });

                    if let Some(url) = &self.url{
                        let ctx = ui.ctx().clone();
                        let wakeup = move || ctx.request_repaint(); // wake up UI thread on new message
                        match ewebsock::connect_with_wakeup(url, Default::default(), wakeup) {
                            Ok((ws_sender, ws_receiver)) => {
                                self.frontend = Some(WebConsoleFrontend::new(ws_sender, ws_receiver));
                                self.error.clear();
                            }
                            Err(error) => {
                                log::error!("Failed to connect to {:?}: {}", &self.url, error);
                                self.error = error;
                            }
                        };
                    }
                }

            }
            if !self.error.is_empty() {
                TopBottomPanel::top("error").show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Error:");
                        ui.colored_label(Color32::RED, &self.error);
                    });
                });
            }
            if let Some(frontend) = &mut self.frontend {
                frontend.ui(ui);
            }
        });
    }
}

// Function to generate client ID
fn generate_client_id(hostname: String, cpu: String) -> String {
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


pub struct WebConsoleFrontend {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,

    pub tx: Sender<Vec<u8>>,
    pub rx: Receiver<Vec<u8>>,
    pub command_tx: Sender<Vec<u8>>,
    pub command_rx: Receiver<Vec<u8>>,

    pub events: Vec<WsEvent>,
    pub text_to_send: String,
    /// Position of cursor in the editor area.
    pub character_index: usize,
    /// Current value of the input box
    pub input: Input,
    /// Current input mode
    // pub input_mode: InputMode,
    /// History of recorded messages
    pub messages: Vec<String>,
    pub command: Cmd,
    pub send_specs: bool
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Cmd{
    LiveData,
    Command,
    Tuneup,
    Cps,
    Qc,
    SfcScan,
    DismScan,
    ChkDsk,
    Mbr2Gpt,
    None
}

impl WebConsoleFrontend {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        let (tx, rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let (command_tx, command_rx) = crossbeam::channel::unbounded::<Vec<u8>>();

        Self {
            ws_sender, ws_receiver,
            command_tx, command_rx,
            tx, rx,
            events: Default::default(),
            text_to_send: String::new(),
            input: Input::default(),
            // input_mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
            command: Cmd::None,
            send_specs: false
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }

        CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                let text_edit = TextEdit::singleline(&mut self.text_to_send).hint_text("Send message").ui(ui);
                let key_press = ui.input(|i| i.key_pressed(Key::Enter));
                if text_edit.lost_focus() && key_press
                {
                    text_edit.request_focus();
                    self.ws_sender
                        .send(WsMessage::Text(std::mem::take(&mut self.text_to_send)));
                }
            });

            ui.separator();
            ui.heading("Received events:");
            self.initialize_websocket(ui).unwrap();
        });
    }

    pub fn initialize_websocket(&mut self, ui: &mut Ui)
        -> anyhow::Result<(), anyhow::Error>
    {
        // if let Some(event) = &self.ws_receiver.try_recv() {
        //     ui.ctx().request_repaint();
        //     match event{
        //         WsEvent::Message(msg) => {
        //             match msg{
        //                 WsMessage::Binary(bin) => { 
        //                     ui.label(format!("{bin:?}")); 
        //                     let cmd = deserialize_command(bin);
        //                     info!("222Cmd: {bin:?}");
        //                     match cmd{
        //                         Cmd::LiveData => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::Command => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::Tuneup => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::Cps => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::Qc => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::SfcScan => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::DismScan => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::ChkDsk => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::Mbr2Gpt => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                         Cmd::None => {
        //                             ui.label(format!("Cmd: {:?}", cmd));
        //                             info!("Cmd: {cmd:?}");
        //                         },
        //                     }
        //                 },
        //                 WsMessage::Text(txt) => {
        //                     ui.label(format!("267Raw Command: {}", txt.clone()));
        //                     let tx = self.command_tx.clone();
        //                     let text = txt.clone();
        //                     spawn(async move {
        //                         handle_command_payload(text.clone(), tx.clone()).await.unwrap();
        //                     });
        //                 },
        //                 _ => {}
        //             }
        //         },
        //         _ => {}
        //     }
        // }

        if let Ok(sysinfo) = &mut self.rx.try_recv(){
            self.ws_sender.send(WsMessage::Binary(std::mem::take(sysinfo)));
        }
        
        if let Ok(cmd_output) = &mut self.command_rx.try_recv(){
            self.ws_sender.send(WsMessage::Binary(std::mem::take(cmd_output)));
        }

        for event in &self.events{
            match event{
                WsEvent::Message(msg) => {
                    
                    match msg{
                        WsMessage::Binary(bin) => {
                            ui.label(format!("{:?}", deserialize_command(&bin.clone())));
                            let cmd = deserialize_command(&bin.clone());
                            info!("297Cmd: {bin:?}");
                            match cmd{
                                Cmd::LiveData => {
                                    let tx = self.tx.clone();
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    spawn(async move { 
                                        match live_computer_stats(tx.clone()).await{
                                            Ok(_) => drop(tx),
                                            Err(e) => debug!("Error with live data {e:?}"),
                                        }
                                    });
                                },
                                Cmd::Tuneup => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                Cmd::Cps => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                Cmd::Qc => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                Cmd::SfcScan => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                Cmd::DismScan => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                Cmd::ChkDsk => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                Cmd::Mbr2Gpt => {
                                    ui.label(format!("Cmd: {:?}", cmd));
                                    info!("Cmd: {cmd:?}");
                                },
                                _ => {
                                    ui.label(format!("Raw Binary: {:?}", bin));
                                },
                            }
                        },
                        WsMessage::Text(txt) => {
                            ui.label(format!("343Raw Command: {}", txt.clone()));
                            let tx = self.command_tx.clone();
                            let text = txt.clone();
                            spawn(async move {
                                handle_command_payload(text.clone(), tx.clone()).await.unwrap();
                            });
                        },
                        _ => {}
                    }
                },
                WsEvent::Error(e) => {
                    ui.label(format!("Error: {e}"));
                },
                _ => {}
            }
        }
        self.events.clear();
       Ok(())
    }

    
}

async fn live_computer_stats(tx: Sender<Vec<u8>>) 
    -> anyhow::Result<(), anyhow::Error>
{
    loop{
        sleep(Duration::from_secs(4)).await;
        let systeminfo: SystemInformation = get_sysinfo().await?;
        tx.send(serialize_system_info(&systeminfo))?;
        // if app.lock().await.finish {
        //     break;
        // }
    }
}
async fn handle_command_payload(string_payload: String, tx: Sender<Vec<u8>>) 
    -> anyhow::Result<(), anyhow::Error>  
{ 
    println!("string_payload: {}", string_payload.clone());
    // let command_payload = split(string_payload.as_str()).unwrap_or(Vec::new());

    #[cfg(target_os="windows")]{ handle_windows_cmd(string_payload, tx.clone()).await?; }
    #[cfg(target_os="linux")]{ handle_linux_cmd(string_payload, tx.clone()).await?; }

    Ok(())
}

#[cfg(target_os="windows")]
async fn handle_windows_cmd(command_payload: String, tx: Sender<Vec<u8>>)
    -> anyhow::Result<(), anyhow::Error> 
{
    use tokio::time::Instant;

    let start = Instant::now();
    info!("Executing command: {}", command_payload);
    let mut process = Command::new("cmd")
        .arg("/C")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Handle stdout and stderr
    let mut stdout = process.stdout.take().expect("Failed to open stdout");
    let mut stderr = process.stderr.take().expect("Failed to open stderr");

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut stdout_buf = Vec::new();
        stdout.read_to_end(&mut stdout_buf).await.ok();
        info!(
            "{:?}", stdout_buf
        );
        tx_clone.send(stdout_buf).ok();
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut stderr_buf = Vec::new();
        stderr.read_to_end(&mut stderr_buf).await.ok();
        tx_clone.send(stderr_buf).ok();
    });

    let output = process.wait_with_output().await?;
    let duration = start.elapsed();
    info!("Command executed in {:?}", duration);
    let tx_clone = tx.clone();
    if !output.status.success() {
        tx_clone.send(output.stderr).ok();
    }

    Ok(())
}

#[cfg(target_os="linux")]
async fn handle_linux_cmd(command_payload: String, tx: Sender<Vec<u8>>)
    -> anyhow::Result<(), anyhow::Error> 
{
    info!("Executing command: {}", command_payload);
    let mut process = Command::new("sh")
        .arg("-c")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // // Handle stdin
    // if let Some(mut stdin) = process.stdin.take() {
    //     tokio::spawn(async move {
    //         stdin.write_all(command_payload.as_bytes()).await.ok();
    //     });
    // }


    // Handle stdout and stderr
    let mut stdout = process.stdout.take().expect("Failed to open stdout");
    let mut stderr = process.stderr.take().expect("Failed to open stderr");

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut stdout_buf = Vec::new();
        stdout.read_to_end(&mut stdout_buf).await.ok();
        tx_clone.send(stdout_buf).ok();
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut stderr_buf = Vec::new();
        stderr.read_to_end(&mut stderr_buf).await.ok();
        tx_clone.send(stderr_buf).ok();
    });

    let output = process.wait_with_output().await?;
    let tx_clone = tx.clone();
    if !output.status.success() {
        tx_clone.send(output.stderr).ok();
    }

    Ok(())
}

async fn get_sysinfo() 
    -> anyhow::Result<SystemInformation, anyhow::Error> 
{
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
