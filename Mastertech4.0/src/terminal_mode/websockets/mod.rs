use database::{schema::{utilities::{check_id_existence, query_id}, ConnectedClient, CONNECTED_CLIENT_TABLE}, DATABASE, WS_CLIENT_URL, WS_CLIENT_URL_LOCAL};
use displays::{deserialize_command, remote_viewer::{encode_buffer_with_timestamp, ratagui::TerminalEvent}, serialize_system_info, tabs::admin_console::client_action::ClientHandler, Cmd, FileSystemAction};
use crate::{filesystem::{get_client_hash, system_info::get_sysinfo_no_gpu}, tabs::file_browser::read_folder};
use std::{path::Path, time::{Duration, Instant}};
use command::{handle_windows_cmd_interactive, PersistentShell};
use bincode::{config::standard, serde::*};
use ewebsock::{WsEvent, WsMessage};
use ratatui::buffer::Buffer;

use super::{data::LocalTermEvent, TerminalApp};

pub mod command;

pub struct TerminalWebsocketClient {
    // explorer: FileSystem, 
    bin_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>, 
    bin_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    command_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    command_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    // process: Arc<Mutex<Option<ChildStdin>>>,
    interactive_input_tx: tokio::sync::mpsc::UnboundedSender<String>, 
    interactive_input_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    client: ConnectedClient, // Store client info
    live_stats_stop_tx: Option<tokio::sync::watch::Sender<bool>>,
    sysinfo_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    sysinfo_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    join_handle: Option<tokio::task::JoinHandle<()>>,
    persistent_shell: Option<PersistentShell>,
}

impl TerminalWebsocketClient {
    // Constructor to initialize the client
    pub fn new() -> Self {
        let (bin_tx, bin_rx) = tokio::sync::mpsc::unbounded_channel();
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (interactive_input_tx, interactive_input_rx) = tokio::sync::mpsc::unbounded_channel();
        let (sysinfo_tx, sysinfo_rx) = tokio::sync::mpsc::unbounded_channel();
        // let process = Arc::new(Mutex::new(None));


        Self {
            bin_tx, bin_rx,
            sysinfo_tx, sysinfo_rx,
            client: get_client_hash(),
            // process,
            command_tx,
            command_rx,
            interactive_input_tx,
            interactive_input_rx,
            live_stats_stop_tx: None,
            join_handle: None,
            persistent_shell: None,
            // explorer: FileSystem::new()
        }
    }
    
    // Migrated start_websocket_sender function
    pub async fn start_websocket_sender(
        &mut self,
        mut buffer_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Buffer)>,
        start_tx: tokio::sync::mpsc::UnboundedSender<bool>,
        connection_state_tx: tokio::sync::mpsc::UnboundedSender<(bool, String)>,
        event_tx: tokio::sync::mpsc::UnboundedSender<LocalTermEvent>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) 
        -> anyhow::Result<()> 
    {
        let connection_url = format!("{}&room_id={}", if cfg!(debug_assertions) {WS_CLIENT_URL_LOCAL} else {WS_CLIENT_URL}, self.client.connection_string);
        let connection = ewebsock::connect(connection_url, ewebsock::Options::default());
        
        match connection {
            Ok((mut sender, receiver)) => {
                let ready = &mut false;
                log::info!("start_websocket_sender -> ready");
                loop {
                    // Handle WebSocket events (e.g., READY or TerminalEvent from egui)
                    while let Some(event) = receiver.try_recv() {
                        // log::info!("Received WebSocket event: {:?}", event);
                        // update client to connected = false in db
                        match event {
                            WsEvent::Opened => { 
                                log::info!("start_websocket_sender -> Connection Opened");
                                let _ = connection_state_tx.send((true, "Connected".to_string())); 
                            },
                            WsEvent::Error(e) => { 
                                log::info!("start_websocket_sender -> Error: {e:?}");
                                let _ = connection_state_tx.send((false, format!("{e:?}"))); 
                            },
                            WsEvent::Closed => { 
                                log::info!("start_websocket_sender -> Connection Closed");
                                let _ = connection_state_tx.send((false, "Disconnected".to_string())); 
                            },
                            WsEvent::Message(ws_message) => {
                                match ws_message {
                                    WsMessage::Pong(_) => { let _ = connection_state_tx.send((true, "Pong".to_string())); },
                                    WsMessage::Text(txt) => {
                                        if !*ready && txt == "READY".to_string() {
                                            let _ = start_tx.send(true);
                                            *ready = true;
                                            log::info!("WebSocket sender marked as ready");
                                        } else if *ready && txt != "READY".to_string() {
                                            log::error!("GOT TEXT: {txt:?}");
                                            // Check if we need to start a persistent shell
                                            if self.persistent_shell.is_none() {
                                                log::error!("persistent_shell IS NONE");
                                                let shell = PersistentShell::new(
                                                    self.command_tx.clone()
                                                );
                                                self.persistent_shell = Some(shell);
                                                
                                                if let Some(shell) = &mut self.persistent_shell {
                                                    log::error!("persistent_shell IS SOME");
                                                    if let Err(e) = shell.start().await {
                                                        log::error!("Failed to start persistent shell: {}", e);
                                                        // Fallback to old method
                                                        let tx = self.command_tx.clone();
                                                        let (new_input_tx, new_input_rx) = tokio::sync::mpsc::unbounded_channel();
                                                        let handle_windows_cmd_interactive = handle_windows_cmd_interactive(
                                                            txt, 
                                                            tx, 
                                                            new_input_rx
                                                        ).await;
                                                        log::info!("start_websocket_sender -> handle_windows_cmd_interactive: {handle_windows_cmd_interactive:?}");
                                                        self.interactive_input_tx = new_input_tx.clone();
                                                        self.persistent_shell = None;
                                                    } else {
                                                        log::error!("STARTED persistent_shell");
                                                        // Send the command to the persistent shell
                                                        if let Err(e) = shell.send_command(txt).await {
                                                            log::error!("Failed to send command to persistent shell: {}", e);
                                                        }
                                                    }
                                                }
                                            } else {
                                                log::error!("USING persistent_shell");
                                                // Use existing persistent shell
                                                if let Some(shell) = &mut self.persistent_shell {
                                                    if let Err(e) = shell.send_command(txt).await {
                                                        log::error!("Failed to send command to persistent shell: {}", e);
                                                        // Reset shell on error
                                                        self.persistent_shell = None;
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    WsMessage::Binary(bin) => {
                                        if *ready {
                                            // Deserialize incoming TerminalEvent from egui and forward to rendering loop
                                            if let Ok(event) = serde_json::from_slice::<TerminalEvent>(&bin) {
                                                log::info!("Received TerminalEvent from egui: {:?}", event);
                                                if event_tx.send(event.into()).is_ok() {
                                                    log::info!("Forwarded TerminalEvent to rendering loop");
                                                } else {
                                                    log::warn!("Failed to forward TerminalEvent to rendering loop");
                                                }
                                            } else {
                                                self.handle_command(deserialize_command(&bin.clone()), &mut sender).await;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            },
                        }
                    }
    
                    if let Ok(()) = shutdown_rx.try_recv() {
                        self.client.disconnect_client();
                        *ready = false;
                        break;
                    }
    
                    if *ready {
                        tokio::select! {
                            Some((frame_count, buffer)) = buffer_rx.recv() => {
                                log::debug!("Sending buffer, frame_count={}", frame_count);
                                let send_start = Instant::now();
                                let serialized = encode_buffer_with_timestamp(frame_count as u64, &buffer)?;
                                sender.send(WsMessage::Binary(serialized));
                                let send_duration = send_start.elapsed();
                                log::debug!("Buffer sent, frame_count={}, send_duration={:?}", frame_count, send_duration);
                            }
                            Some(cmd_output) = self.command_rx.recv() => {
                                sender.send(WsMessage::Binary(cmd_output));
                            }
                            Some(sysinfo) = self.sysinfo_rx.recv() => {
                                sender.send(WsMessage::Binary(sysinfo));
                            }
                        }
                    }

                    // Let other tasks run before looping again // THIS IS REQUIRED, or else the 
                    // server will not actually receive an Open event, and will terminate the loop
                    // immediately. we need to give some CPU time to yield, allowing the websocket
                    // handshake to complete
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            Err(e) => log::error!("Failed to establish WebSocket connection: {e:?}")
        }
        Ok(())
    }

    async fn handle_command(&mut self, cmd: Cmd, sender: &mut ewebsock::WsSender) {
        match cmd {
            Cmd::FileSystemAction(FileSystemAction::RequestNewContents(new_path)) => {
                let path = if new_path == "current" {
                    let current_path = std::env::current_dir().unwrap_or_default();
                    log::info!("websockets -> Current_path: {current_path:?}");
                    current_path
                } else {
                    Path::new(&new_path).to_path_buf()
                };
                if path.is_dir() {
                    let paths = read_folder(&path, 1, false);
                    // info!("websockets -> Paths: {:?}", paths.clone());
                    if paths.len() > 0 {
                        // let node = self.explorer.build_virtual_file_system(path, paths);
                        // info!("websockets -> Node: {:?}", node);
    
                        // let payload = serialize(
                        //     &Cmd::FileSystemAction(FileSystemAction::GetNode(node))
                        // );
        
                        // match payload {
                        //     Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                        //     Err(e) => error!("Error serializing paths: {e:?}"),
                        // }
                    }
                } else { sender.send(WsMessage::Text(format!("{new_path} is not a directory"))); }
            },
            Cmd::FileSystemAction(FileSystemAction::Execute(path)) => {
                let tx = self.bin_tx.clone();
                let p = path.clone();
                log::info!("websockets -> executing: {path:?}");
                let (new_input_tx, new_input_rx) = tokio::sync::mpsc::unbounded_channel();
                let handle_windows_cmd_interactive = handle_windows_cmd_interactive(
                    p, tx, 
                    new_input_rx
                ).await;

                self.interactive_input_tx = new_input_tx.clone();
                log::info!("websockets -> handle_windows_cmd_interactive: {handle_windows_cmd_interactive:?}");
            },
            Cmd::FileSystemAction(FileSystemAction::CopyFromClient(_path)) => {

            }
            Cmd::FileSystemAction(FileSystemAction::CopyToClient(_minio_path)) => {
                // self.explorer
            } // self.explorer.previewed_file = Some(String::from_utf8(byte_vec.clone()));
            Cmd::FileSystemAction(FileSystemAction::Select((_, path))) => {
                match std::fs::read_to_string(path) {
                    Ok(file) => {
                        let payload = encode_to_vec(
                            &Cmd::FileSystemAction(FileSystemAction::PreviewedFile(file)),
                            standard()
                        );
        
                        match payload {
                            Ok(bytes) => sender.send(WsMessage::Binary(bytes)),
                            Err(e) => log::error!("Error serializing paths: {e:?}"),
                        };
                    },
                    Err(e) => {let _ = self.bin_tx.send(format!("Error with file preview: {e:?}").as_bytes().to_vec());},
                };
            }
            Cmd::FileSystemAction(FileSystemAction::Delete(path)) => {
                let tx = self.bin_tx.clone();
                log::info!("websockets -> deleting: {path:?}");
                let path = Path::new(&path);
                if !path.is_dir() {
                    let remove_dir = tokio::fs::remove_dir_all(path).await;
                    let _ = match remove_dir {
                        Ok(_) => tx.send("Removed Directory".as_bytes().to_vec()),
                        Err(e) => tx.send(format!("Error removing path: {e:?}").as_bytes().to_vec()),
                    };
                } else {
                    let remove_file = tokio::fs::remove_file(path).await;
                    let _ = match remove_file {
                        Ok(_) => tx.send("Removed Path".as_bytes().to_vec()),
                        Err(e) => tx.send(format!("Error removing path: {e:?}").as_bytes().to_vec()),
                    };
                }
            }
            Cmd::InteractiveInput(cmd) => {
                if cmd.ends_with("tron.bat") {
                    let path = Path::new(&cmd);
                    if path.exists() {
                        let whitelist = if cfg!(target_os="windows") {
                            path.join("tron\\resources\\stage_0_prep\\processkiller\\whitelist.txt")
                        } else { path.join("tron/resources/stage_0_prep/processkiller/whitelist.txt") };

                        if whitelist.exists() {

                        }
                    }
                } else {

                }

                // Send to persistent shell if available, otherwise use the old method
                if let Some(shell) = &mut self.persistent_shell {
                    let shell_cmd = cmd.clone();
                    let shell_ptr = shell as *mut PersistentShell;
                    // Use a more direct approach to avoid lifetime issues
                    if let Err(e) = shell.send_command(shell_cmd).await {
                        log::error!("Failed to send interactive input to persistent shell: {}", e);
                        // Fallback to old method
                        let _ = self.interactive_input_tx.send(cmd);
                    }
                } else {
                    let _ = self.interactive_input_tx.send(cmd);
                }
            },
            Cmd::ReadEvents => {
                
            },
            Cmd::QuitInteractive => {
                if let Some(shell) = self.persistent_shell.take() {
                    let mut shell = shell;
                    tokio::spawn(async move {
                        if let Err(e) = shell.close().await {
                            log::error!("Failed to close persistent shell: {}", e);
                        }
                    });
                } else {
                    let _ = self.interactive_input_tx.send("quit".to_string());
                }
            },
            Cmd::LiveData => {
                // If already running, do nothing
                if self.join_handle.is_some() {
                    return;
                }
                let tx = self.sysinfo_tx.clone();
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                self.live_stats_stop_tx = Some(stop_tx);
                self.join_handle = Some(tokio::spawn(async move {
                    let res = live_computer_stats(tx, stop_rx).await;
                    log::info!("live_computer_stats: {res:?}");
                }));
            }
            Cmd::TaskManager => todo!(),
            // Cmd::UninstallProgram(_) => todo!(),
            // Cmd::PullKeys(_) => todo!(),
            // Cmd::PullTicket(_) => todo!(),
            Cmd::Quit => {
                // Signal the live stats task to stop and await it
                if let Some(stop_tx) = self.live_stats_stop_tx.take() {
                    let _ = stop_tx.send(true);
                }
                if let Some(handle) = self.join_handle.take() {
                    let _ = handle.await;
                }
            }
            Cmd::None => {},
            _ => {}
        }
    }
}

pub async fn live_computer_stats(tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>, mut stop_rx: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<(), anyhow::Error> {
    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    log::info!("live_computer_stats: received stop signal");
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_secs_f32(0.4)) => {
                match get_sysinfo_no_gpu().await {
                    Ok(systeminfo) => {
                        log::info!("websockets -> {systeminfo:?}");
                        tx.send(serialize_system_info(&systeminfo))?
                    }
                    Err(e) => log::error!("Error with live data {e:?}"),
                }
            }
        }
    }
    Ok(())
}

impl<'a> TerminalApp<'a> {
    pub fn send_buffer(
        f: &mut ratatui::Frame, 
        last_sent: &mut Instant, 
        send_interval: Duration, 
        can_start: &mut bool,
        buffer_tx: tokio::sync::mpsc::UnboundedSender<(usize, Buffer)>
    ) {
        let now = Instant::now(); // Changed: Throttle buffer sending
        if now.duration_since(*last_sent) >= send_interval {
            if *can_start {
                let buffer_to_send = f.buffer_mut().clone();
                let count = f.count();
                std::thread::scope(|s| {
                    s.spawn(|| {
                        if let Err(e) = buffer_tx.send((count, buffer_to_send)) {
                            log::warn!("Failed to send buffer: {:?}", e);
                        }
                    });
                });
                *last_sent = now;
            }
        }
    }
}

pub async fn create_client(mut client: ConnectedClient) -> anyhow::Result<(), anyhow::Error> {
    client.connected = true;
    
    // log::info!("Client: {client:?}");
    
    let query_id = query_id::<ConnectedClient>(
        CONNECTED_CLIENT_TABLE.to_string(), 
        client.id.clone()
    ).await;

    log::info!("websockets -> query_id: {query_id:?}");

    let check_id_existence = check_id_existence(
        CONNECTED_CLIENT_TABLE.to_string(), 
        client.id.clone()
    ).await;
    
    log::info!("websockets -> check_id_existence: {check_id_existence:?}");
    
    if let Ok(Some(_)) = query_id {
        log::info!("WE HAVE A CLIENT");
    } else {
        let res: Option<ConnectedClient> = DATABASE
            .upsert(client.id.clone())
            .content(client)
            .await?
            .take();

        log::info!("websockets -> Upsert: {res:?}");
    }
    Ok(())
}