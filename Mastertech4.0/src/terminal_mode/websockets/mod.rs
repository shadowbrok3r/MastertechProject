use database::{schema::{utilities::{check_id_existence, query_id}, ConnectedClient, CONNECTED_CLIENT_TABLE}, websocket_url_with_room, DATABASE, WS_CLIENT_URL, WS_CLIENT_URL_LOCAL};
use displays::{deserialize_command, remote_viewer::{encode_buffer_with_timestamp, ratagui::TerminalEvent}, serialize_system_info, tabs::admin_console::client_action::ClientHandler, Cmd, EventLogEntry, FileSystemAction, RegistryEdit, RegistryKeyInfo, RegistryValueEntry, RemoteDirEntry, RemoteScriptItem, RemoteScriptStatus, ScheduledTask, ServiceActionType, StartupApp, WindowsService};
use crate::{filesystem::{get_client_hash, system_info::get_sysinfo_no_gpu}, tabs::file_browser::read_folder, transport::ClientTransport};
use std::{path::Path, time::{Duration, Instant}};
use command::{handle_windows_cmd_interactive, PersistentShell};
use bincode::{config::standard, serde::*};
use ewebsock::{WsEvent, WsMessage};
use ratatui::buffer::Buffer;

use super::{data::LocalTermEvent, TerminalApp};

pub mod command;

/// Resolve special folder paths using Windows API or fallback to environment variables
#[cfg(target_os = "windows")]
fn resolve_special_path(path: &str) -> String {
    // Check for special folder keywords
    let lower_path = path.to_lowercase();
    
    // Try to use Windows API for known special folders
    if let Ok(user_data) = windows::Storage::UserDataPaths::GetDefault() {
        let resolved = match lower_path.as_str() {
            "desktop" | "%userprofile%\\desktop" => user_data.Desktop().ok().map(|p| p.to_string()),
            "documents" | "%userprofile%\\documents" => user_data.Documents().ok().map(|p| p.to_string()),
            "downloads" | "%userprofile%\\downloads" => user_data.Downloads().ok().map(|p| p.to_string()),
            "pictures" | "%userprofile%\\pictures" => user_data.Pictures().ok().map(|p| p.to_string()),
            "music" | "%userprofile%\\music" => user_data.Music().ok().map(|p| p.to_string()),
            "videos" | "%userprofile%\\videos" => user_data.Videos().ok().map(|p| p.to_string()),
            "appdata" | "%appdata%" => user_data.RoamingAppData().ok().map(|p| p.to_string()),
            "localappdata" | "%localappdata%" => user_data.LocalAppData().ok().map(|p| p.to_string()),
            _ => None,
        };
        
        if let Some(resolved_path) = resolved {
            log::info!("Resolved '{}' to '{}'", path, resolved_path);
            return resolved_path;
        }
    }
    
    // Fallback to environment variable expansion
    expand_env_vars(path)
}

#[cfg(not(target_os = "windows"))]
fn resolve_special_path(path: &str) -> String {
    expand_env_vars(path)
}

/// Expand environment variables like %USERPROFILE%, $HOME, etc.
fn expand_env_vars(path: &str) -> String {
    let mut result = path.to_string();
    
    // Windows-style environment variables
    let env_vars = [
        "USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA", 
        "TEMP", "TMP", "USERNAME", "HOMEDRIVE", "HOMEPATH",
        "PROGRAMFILES", "PROGRAMFILES(X86)", "SYSTEMROOT",
        "WINDIR", "SYSTEMDRIVE"
    ];
    
    for var_name in env_vars {
        let pattern = format!("%{}%", var_name);
        if result.contains(&pattern) {
            if let Ok(value) = std::env::var(var_name) {
                result = result.replace(&pattern, &value);
            }
        }
    }
    
    // Unix-style environment variables (e.g., $HOME)
    if result.starts_with('$') {
        let var_name = result.trim_start_matches('$').split('/').next().unwrap_or("");
        if let Ok(value) = std::env::var(var_name) {
            result = result.replacen(&format!("${}", var_name), &value, 1);
        }
    }
    
    result
}

pub struct TerminalWebsocketClient {
    // explorer: FileSystem,
    pub bin_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub bin_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    pub command_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub command_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    // process: Arc<Mutex<Option<ChildStdin>>>,
    pub interactive_input_tx: tokio::sync::mpsc::UnboundedSender<String>,
    pub interactive_input_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    pub client: ConnectedClient,
    pub live_stats_stop_tx: Option<tokio::sync::watch::Sender<bool>>,
    pub sysinfo_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub sysinfo_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    pub join_handle: Option<tokio::task::JoinHandle<()>>,
    pub persistent_shell: Option<PersistentShell>,
    /// Accumulates chunks for direct file transfers: filename → (total_chunks, received_chunks_data)
    pub file_transfer_buffers: std::collections::HashMap<String, (u32, Vec<(u32, Vec<u8>)>)>,
    /// Accumulates incoming self-update binary chunks from the admin console.
    pub self_update_buffer: crate::remote_self_update::SelfUpdateBuffer,
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
            file_transfer_buffers: std::collections::HashMap::new(),
            self_update_buffer: Default::default(),
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
        let connection_url = websocket_url_with_room(
            if cfg!(debug_assertions) {
                WS_CLIENT_URL_LOCAL
            } else {
                WS_CLIENT_URL
            },
            &self.client.connection_string,
            "client",
        );

        // After a drop (e.g. network driver during Windows Update), reconnect instead of spinning on a dead sender.
        const RECONNECT_DELAY: Duration = Duration::from_secs(2);

        'ws_session: loop {
            let connection = ewebsock::connect(connection_url.clone(), ewebsock::Options::default());

            match connection {
                Ok((ws_sender, receiver)) => {
                    // Wrap the raw `WsSender` in our transport-agnostic
                    // `ClientTransport`. Existing `sender.send(WsMessage::...)`
                    // call sites inside `handle_command` work unchanged
                    // because the wrapper preserves the same `send(WsMessage)`
                    // method shape — see `Mastertech4.0/src/transport.rs`.
                    let mut sender = ClientTransport::WebSocket(ws_sender);
                    let ready = &mut false;
                    log::info!("start_websocket_sender -> connecting");
                    loop {
                        let mut socket_lost = false;

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
                                let _ = start_tx.send(false);
                                *ready = false;
                                self.persistent_shell = None;
                                socket_lost = true;
                                break;
                            },
                            WsEvent::Closed => { 
                                log::info!("start_websocket_sender -> Connection Closed — will reconnect");
                                let _ = connection_state_tx.send((false, "Disconnected".to_string())); 
                                let _ = start_tx.send(false);
                                *ready = false;
                                self.persistent_shell = None;
                                socket_lost = true;
                                break;
                            },
                            WsEvent::Message(ws_message) => {
                                match ws_message {
                                    WsMessage::Pong(_) => { let _ = connection_state_tx.send((true, "Pong".to_string())); },
                                    WsMessage::Text(txt) => {
                                        // Handle master presence notifications
                                        if txt == "MASTER_CONNECTED" {
                                            log::info!("Master connected - resuming data transmission");
                                            let _ = connection_state_tx.send((true, "Master Connected".to_string()));
                                            // If we were waiting for master, mark as ready
                                            if !*ready {
                                                let _ = start_tx.send(true);
                                                *ready = true;
                                            }
                                            continue;
                                        } else if txt == "MASTER_DISCONNECTED" {
                                            log::info!("Master disconnected - pausing data transmission");
                                            let _ = connection_state_tx.send((true, "Master Disconnected - Waiting...".to_string()));
                                            // Don't set ready to false - keep the connection alive
                                            // but the render system will check master_connected before sending
                                            continue;
                                        } else if txt == "CLIENT_CONNECTED" || txt == "CLIENT_DISCONNECTED" {
                                            // These are for master-side, ignore on client
                                            continue;
                                        } else if txt.starts_with("MASTER_STATUS:") || txt.starts_with("CLIENT_STATUS:") {
                                            // Activity status is now tracked via SurrealDB, not websocket messages
                                            // Ignore these to prevent them from being executed as shell commands
                                            continue;
                                        }
                                        
                                        if !*ready && txt == "READY".to_string() {
                                            let _ = start_tx.send(true);
                                            *ready = true;
                                            log::info!("WebSocket sender marked as ready");
                                        } else if *ready && txt != "READY".to_string() {
                                            log::info!("GOT TEXT: {txt:?}");
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
                        self.persistent_shell = None;
                        return Ok(());
                    }

                    if socket_lost {
                        log::info!("start_websocket_sender -> reconnecting after {:?}...", RECONNECT_DELAY);
                        tokio::time::sleep(RECONNECT_DELAY).await;
                        continue 'ws_session;
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
                Err(e) => {
                    log::error!("Failed to establish WebSocket connection: {e:?}");
                    let _ = connection_state_tx.send((false, format!("Connect failed: {e:?}")));
                    let _ = start_tx.send(false);
                    tokio::time::sleep(RECONNECT_DELAY).await;
                }
            }
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    pub async fn handle_command(&mut self, cmd: Cmd, sender: &mut ClientTransport) {
        #[cfg(target_os = "windows")]
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
                    let _shell_ptr = shell as *mut PersistentShell;
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
            Cmd::ReadEvents => {},
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
                    log::info!("websockets -> LiveData already running, ignoring request");
                    return;
                }
                log::info!("websockets -> Starting live stats task");
                let tx = self.sysinfo_tx.clone();
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                self.live_stats_stop_tx = Some(stop_tx);
                self.join_handle = Some(tokio::spawn(async move {
                    let res = live_computer_stats(tx, stop_rx).await;
                    log::info!("live_computer_stats completed: {res:?}");
                }));
            }
            Cmd::TaskManager => todo!(),
            // Cmd::UninstallProgram(_) => todo!(),
            // Cmd::PullKeys(_) => todo!(),
            // Cmd::PullTicket(_) => todo!(),
            Cmd::Quit => {
                log::info!("websockets -> Received Cmd::Quit, stopping live stats");
                // Signal the live stats task to stop and await it
                if let Some(stop_tx) = self.live_stats_stop_tx.take() {
                    log::info!("websockets -> Sending stop signal to live stats task");
                    let _ = stop_tx.send(true);
                } else {
                    log::warn!("websockets -> No live stats stop channel found");
                }
                if let Some(handle) = self.join_handle.take() {
                    log::info!("websockets -> Waiting for live stats task to complete");
                    let _ = handle.await;
                    log::info!("websockets -> Live stats task completed");
                } else {
                    log::warn!("websockets -> No live stats join handle found");
                }
            }
            Cmd::KillProcess(pid) => {
                log::info!("websockets -> Killing process with PID: {}", pid);
                #[cfg(target_os = "windows")]
                {
                    let output = tokio::process::Command::new("taskkill")
                        .args(["/F", "/PID", &pid.to_string()])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Successfully killed process {}", pid);
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to kill process {}: {}", pid, stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing taskkill: {}", e),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let output = tokio::process::Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Successfully killed process {}", pid);
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to kill process {}: {}", pid, stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing kill: {}", e),
                    }
                }
            }
            Cmd::OpenProcessInExplorer(path) => {
                log::info!("websockets -> Opening path in explorer: {}", path);
                #[cfg(target_os = "windows")]
                {
                    // Get parent directory if path is a file
                    let target_path = Path::new(&path);
                    let dir_path = if target_path.is_file() {
                        target_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| target_path.to_path_buf())
                    } else {
                        target_path.to_path_buf()
                    };
                    
                    // Use explorer.exe to open and select the file
                    if target_path.exists() {
                        let _ = tokio::process::Command::new("explorer.exe")
                            .args(["/select,", &path])
                            .spawn();
                    } else {
                        // If file doesn't exist, just open the directory
                        let _ = tokio::process::Command::new("explorer.exe")
                            .arg(dir_path)
                            .spawn();
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let target_path = Path::new(&path);
                    if target_path.exists() {
                        let _ = tokio::process::Command::new("open")
                            .args(["-R", &path])
                            .spawn();
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let target_path = Path::new(&path);
                    let dir_path = if target_path.is_file() {
                        target_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| target_path.to_path_buf())
                    } else {
                        target_path.to_path_buf()
                    };
                    let _ = tokio::process::Command::new("xdg-open")
                        .arg(dir_path)
                        .spawn();
                }
            }
            Cmd::ListDirectory(path_str) => {
                log::info!("websockets -> Listing directory: {}", path_str);
                
                // Resolve special folder paths using Windows API or expand environment variables
                let expanded_path = resolve_special_path(&path_str);
                
                // Determine the actual path to list
                let target_path = if path_str == "current" {
                    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
                } else {
                    Path::new(&expanded_path).to_path_buf()
                };
                
                let resolved_path = target_path.to_string_lossy().to_string();
                let mut entries: Vec<RemoteDirEntry> = Vec::new();
                
                if target_path.is_dir() {
                    match std::fs::read_dir(&target_path) {
                        Ok(dir_iter) => {
                            for entry in dir_iter.flatten() {
                                let path = entry.path();
                                let name = entry.file_name().to_string_lossy().to_string();
                                let is_directory = path.is_dir();
                                let size = if is_directory {
                                    None
                                } else {
                                    entry.metadata().ok().map(|m| m.len())
                                };
                                let modified = entry.metadata().ok()
                                    .and_then(|m| m.modified().ok())
                                    .map(|t| {
                                        let datetime: chrono::DateTime<chrono::Local> = t.into();
                                        datetime.to_rfc3339()
                                    });
                                
                                entries.push(RemoteDirEntry {
                                    name,
                                    path: path.to_string_lossy().to_string(),
                                    is_directory,
                                    size,
                                    modified,
                                });
                            }
                        }
                        Err(e) => {
                            log::error!("Error reading directory: {}", e);
                        }
                    }
                }
                
                // Send the directory listing back with resolved path
                let response = Cmd::DirectoryListing(entries, Some(resolved_path));
                let payload = encode_to_vec(&response, standard()).expect("Failed to serialize DirectoryListing");
                sender.send(WsMessage::Binary(payload));
            }
            Cmd::GetDrives => {
                log::info!("websockets -> Getting drives");
                use sysinfo::Disks;
                
                let disks = Disks::new_with_refreshed_list();
                let drives: Vec<String> = disks.iter()
                    .filter_map(|disk| disk.mount_point().to_str().map(|s| s.to_string()))
                    .collect();
                
                log::info!("websockets -> Found {} drives: {:?}", drives.len(), drives);
                
                let response = Cmd::DriveList(drives);
                let payload = encode_to_vec(&response, standard()).expect("Failed to serialize DriveList");
                sender.send(WsMessage::Binary(payload));
            }
            Cmd::DownloadRemoteFile(path_str) => {
                log::info!("websockets -> Download request for: {}", path_str);
                
                let path = Path::new(&path_str);
                if path.is_file() {
                    // Check file size first to avoid reading huge files into memory
                    let metadata = match std::fs::metadata(path) {
                        Ok(m) => m,
                        Err(e) => {
                            log::error!("Error getting file metadata: {}", e);
                            sender.send(WsMessage::Text(format!("Error: Cannot read file metadata - {}", e)));
                            return;
                        }
                    };
                    
                    let file_size = metadata.len();
                    const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB limit
                    
                    if file_size > MAX_FILE_SIZE {
                        log::warn!("File too large for download: {} bytes", file_size);
                        sender.send(WsMessage::Text(format!("Error: File too large ({} MB). Maximum is 100 MB.", file_size / 1024 / 1024)));
                        return;
                    }
                    
                    log::info!("Reading file: {} ({} bytes)", path_str, file_size);
                    
                    match std::fs::read(path) {
                        Ok(data) => {
                            log::info!("File read successfully, {} bytes", data.len());
                            
                            // For large files, send in chunks to avoid memory issues
                            const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MB chunks
                            
                            if data.len() > CHUNK_SIZE {
                                // Send in multiple chunks
                                let chunks: Vec<&[u8]> = data.chunks(CHUNK_SIZE).collect();
                                let total_chunks = chunks.len();
                                
                                for (i, chunk) in chunks.into_iter().enumerate() {
                                    let is_last = i == total_chunks - 1;
                                    let response = Cmd::FileChunk(chunk.to_vec(), is_last);
                                    match encode_to_vec(&response, standard()) {
                                        Ok(payload) => {
                                            log::info!("Sending chunk {}/{} ({} bytes)", i + 1, total_chunks, payload.len());
                                            sender.send(WsMessage::Binary(payload));
                                        }
                                        Err(e) => {
                                            log::error!("Failed to serialize file chunk {}: {}", i, e);
                                            sender.send(WsMessage::Text(format!("Error: Failed to serialize chunk {} - {}", i, e)));
                                            return;
                                        }
                                    }
                                }
                                log::info!("All {} chunks sent successfully", total_chunks);
                            } else {
                                // Small file - send in one chunk
                                let response = Cmd::FileChunk(data, true);
                                match encode_to_vec(&response, standard()) {
                                    Ok(payload) => {
                                        log::info!("Serialized payload size: {} bytes", payload.len());
                                        sender.send(WsMessage::Binary(payload));
                                        log::info!("File chunk sent successfully");
                                    }
                                    Err(e) => {
                                        log::error!("Failed to serialize file chunk: {}", e);
                                        sender.send(WsMessage::Text(format!("Error: Failed to serialize file - {}", e)));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Error reading file for download: {}", e);
                            sender.send(WsMessage::Text(format!("Error: {}", e)));
                        }
                    }
                } else {
                    log::warn!("Path is not a file: {}", path_str);
                    sender.send(WsMessage::Text("Error: Path is not a file".to_string()));
                }
            }
            Cmd::ExecuteRemoteFile(path_str) => {
                log::info!("websockets -> Execute request for: {}", path_str);
                let path = Path::new(&path_str);
                
                if path.exists() {
                    #[cfg(target_os = "windows")]
                    {
                        // Use ShellExecuteW to open/execute the file
                        let _ = tokio::process::Command::new("cmd")
                            .args(["/c", "start", "", &path_str])
                            .spawn();
                        log::info!("Executed file: {}", path_str);
                    }
                    #[cfg(target_os = "macos")]
                    {
                        let _ = tokio::process::Command::new("open")
                            .arg(&path_str)
                            .spawn();
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let _ = tokio::process::Command::new("xdg-open")
                            .arg(&path_str)
                            .spawn();
                    }
                } else {
                    log::warn!("File does not exist: {}", path_str);
                    sender.send(WsMessage::Text(format!("Error: File not found: {}", path_str)));
                }
            }
            Cmd::PreviewRemoteFile(path_str) => {
                log::info!("websockets -> Preview request for: {}", path_str);
                let path = Path::new(&path_str);
                
                if path.is_file() {
                    // Check file size - don't preview huge files
                    let max_preview_size: u64 = 5 * 1024 * 1024; // 5 MB
                    
                    if let Ok(metadata) = std::fs::metadata(path) {
                        if metadata.len() > max_preview_size {
                            sender.send(WsMessage::Text(format!("Error: File too large for preview ({} MB)", metadata.len() / 1024 / 1024)));
                            return;
                        }
                    }
                    
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            let response = Cmd::FilePreviewContent(path_str, content);
                            match encode_to_vec(&response, standard()) {
                                Ok(payload) => {
                                    sender.send(WsMessage::Binary(payload));
                                    log::info!("Sent file preview content");
                                }
                                Err(e) => {
                                    log::error!("Failed to serialize preview content: {}", e);
                                    sender.send(WsMessage::Text(format!("Error: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            // May be binary - try reading as lossy UTF-8
                            if let Ok(bytes) = std::fs::read(path) {
                                let content = String::from_utf8_lossy(&bytes).to_string();
                                let response = Cmd::FilePreviewContent(path_str, content);
                                if let Ok(payload) = encode_to_vec(&response, standard()) {
                                    sender.send(WsMessage::Binary(payload));
                                    return;
                                }
                            }
                            log::error!("Error reading file for preview: {}", e);
                            sender.send(WsMessage::Text(format!("Error reading file: {}", e)));
                        }
                    }
                } else {
                    sender.send(WsMessage::Text("Error: Path is not a file".to_string()));
                }
            }
            Cmd::UploadToClient(dest_path, data) => {
                log::info!("websockets -> Upload to client: {} ({} bytes)", dest_path, data.len());
                
                match std::fs::write(&dest_path, &data) {
                    Ok(_) => {
                        log::info!("Successfully wrote file to: {}", dest_path);
                        let response = Cmd::SaveResult(true, format!("File saved: {}", dest_path));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to write file: {}", e);
                        let response = Cmd::SaveResult(false, format!("Failed to save: {}", e));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                }
            }
            Cmd::RequestThumbnail(path_str) => {
                log::info!("websockets -> Thumbnail request for: {}", path_str);
                
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::ffi::OsStrExt;
                    use windows::{
                        Win32::{
                            Foundation::SIZE,
                            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, IBindCtx},
                            UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF},
                            Graphics::Gdi::*,
                        },
                        core::{Interface, PCWSTR},
                    };
                    
                    // Initialize COM
                    unsafe {
                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    }
                    
                    let path = Path::new(&path_str);
                    let result: Result<Vec<u8>, String> = (|| -> Result<Vec<u8>, String> {
                        unsafe {
                            let wide: Vec<u16> = path
                                .as_os_str()
                                .encode_wide()
                                .chain(std::iter::once(0))
                                .collect();
                            
                            let shell_item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>)
                                .map_err(|e| format!("SHCreateItemFromParsingName: {e}"))?;
                            let factory: IShellItemImageFactory = shell_item
                                .cast()
                                .map_err(|e| format!("cast IShellItemImageFactory: {e}"))?;
                            let hbmp: HBITMAP = factory
                                .GetImage(SIZE { cx: 256, cy: 256 }, SIIGBF(0))
                                .map_err(|e| format!("GetImage: {e}"))?;
                            
                            // Convert HBITMAP to PNG bytes
                            hbitmap_to_png_bytes(hbmp)
                        }
                    })();
                    
                    match result {
                        Ok(png_bytes) => {
                            let response = Cmd::ThumbnailResponse(path_str, png_bytes);
                            match encode_to_vec(&response, standard()) {
                                Ok(payload) => {
                                    sender.send(WsMessage::Binary(payload));
                                    log::info!("Sent thumbnail");
                                }
                                Err(e) => {
                                    log::error!("Failed to serialize thumbnail: {}", e);
                                    sender.send(WsMessage::Text(format!("Error: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to generate thumbnail: {}", e);
                            sender.send(WsMessage::Text(format!("Error generating thumbnail: {}", e)));
                        }
                    }
                }
                
                #[cfg(not(target_os = "windows"))]
                {
                    // Use image crate as fallback
                    if let Ok(img) = image::open(&path_str) {
                        let thumb = img.thumbnail(256, 256);
                        let mut buf = Vec::new();
                        if thumb.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).is_ok() {
                            let response = Cmd::ThumbnailResponse(path_str, buf);
                            if let Ok(payload) = encode_to_vec(&response, standard()) {
                                sender.send(WsMessage::Binary(payload));
                            }
                        }
                    } else {
                        sender.send(WsMessage::Text("Error: Could not load image".to_string()));
                    }
                }
            }
            Cmd::SaveRemoteFile(path_str, content) => {
                log::info!("websockets -> Save file request: {}", path_str);
                
                match std::fs::write(&path_str, &content) {
                    Ok(_) => {
                        log::info!("Successfully saved file: {}", path_str);
                        let response = Cmd::SaveResult(true, format!("File saved: {}", path_str));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to save file: {}", e);
                        let response = Cmd::SaveResult(false, format!("Failed to save: {}", e));
                        if let Ok(payload) = encode_to_vec(&response, standard()) {
                            sender.send(WsMessage::Binary(payload));
                        }
                    }
                }
            }
            Cmd::RebootSystem { persist_mastertech, terminal_mode } => {
                log::info!("websockets -> Reboot system command received (persist={})", persist_mastertech);
                #[cfg(target_os = "windows")]
                {
                    if persist_mastertech {
                        // Create a scheduled task to run Mastertech on next login
                        // This uses schtasks to create a one-time task that runs at logon
                        let exe_path = std::env::current_exe().unwrap_or_default();
                        let exe_path_str = exe_path.to_string_lossy();
                        
                        let command = if terminal_mode {
                            format!("\"{}\" -t", exe_path_str)
                        } else {
                            format!("\"{}\"", exe_path_str)
                        };

                        // Create a scheduled task that runs once at next logon then deletes itself
                        let task_name = "MastertechAutoRestart";
                        let create_task = tokio::process::Command::new("schtasks")
                            .args([
                                "/Create",
                                "/TN", task_name,
                                "/TR", &command,
                                "/SC", "ONLOGON",
                                "/RL", "HIGHEST",
                                "/F", // Force overwrite if exists
                            ])
                            .output()
                            .await;
                        
                        match create_task {
                            Ok(out) => {
                                if out.status.success() {
                                    log::info!("Created scheduled task for Mastertech auto-restart");
                                } else {
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    log::error!("Failed to create scheduled task: {}", stderr);
                                }
                            }
                            Err(e) => log::error!("Error creating scheduled task: {}", e),
                        }
                    }
                    
                    // Initiate system reboot with 5 second delay
                    let output = tokio::process::Command::new("shutdown")
                        .args(["/r", "/t", "5", "/c", "Mastertech remote reboot requested"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Reboot initiated successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate reboot: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown command: {}", e),
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let output = tokio::process::Command::new("sudo")
                        .args(["shutdown", "-r", "+1", "Mastertech remote reboot"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate reboot: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown: {}", e),
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let output = tokio::process::Command::new("sudo")
                        .args(["shutdown", "-r", "+1"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate reboot: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown: {}", e),
                    }
                }
            }
            Cmd::ShutdownSystem => {
                log::info!("websockets -> Shutdown system command received");
                #[cfg(target_os = "windows")]
                {
                    let output = tokio::process::Command::new("shutdown")
                        .args(["/s", "/t", "5", "/c", "Mastertech remote shutdown requested"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Shutdown initiated successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate shutdown: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown command: {}", e),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let output = tokio::process::Command::new("sudo")
                        .args(["shutdown", "-h", "+1"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to initiate shutdown: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error executing shutdown: {}", e),
                    }
                }
            }
            Cmd::LockWorkstation => {
                log::info!("websockets -> Lock workstation command received");
                #[cfg(target_os = "windows")]
                {
                    // Use rundll32 to call the LockWorkStation function
                    let output = tokio::process::Command::new("rundll32.exe")
                        .args(["user32.dll,LockWorkStation"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("Workstation locked successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to lock workstation: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error locking workstation: {}", e),
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    // Try common screen lockers
                    let lockers = ["loginctl lock-session", "gnome-screensaver-command -l", "xdg-screensaver lock"];
                    for locker in lockers {
                        let parts: Vec<&str> = locker.split_whitespace().collect();
                        if let Some((cmd, args)) = parts.split_first() {
                            if let Ok(output) = tokio::process::Command::new(cmd)
                                .args(args)
                                .output()
                                .await
                            {
                                if output.status.success() {
                                    log::info!("Workstation locked using: {}", locker);
                                    break;
                                }
                            }
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = tokio::process::Command::new("pmset")
                        .args(["displaysleepnow"])
                        .output()
                        .await;
                }
            }
            Cmd::LogOffUser => {
                log::info!("websockets -> Log off user command received");
                #[cfg(target_os = "windows")]
                {
                    let output = tokio::process::Command::new("shutdown")
                        .args(["/l"])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if out.status.success() {
                                log::info!("User logged off successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to log off user: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error logging off user: {}", e),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    // On Linux/macOS, kill the user's session
                    let output = tokio::process::Command::new("pkill")
                        .args(["-KILL", "-u", &whoami::username().unwrap_or_default()])
                        .output()
                        .await;
                    
                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                log::error!("Failed to log off user: {}", stderr);
                            }
                        }
                        Err(e) => log::error!("Error logging off user: {}", e),
                    }
                }
            }
            // --- Event Log ---
            Cmd::ReadEventLog { log_name, max_entries, level_filter } => {
                log::info!("websockets -> Reading event log: {} (max: {}, filter: {:?})", log_name, max_entries, level_filter);

                let level_clause = match level_filter.as_deref() {
                    Some("Critical") => " -Level 1",
                    Some("Error") => " -Level 2",
                    Some("Warning") => " -Level 3",
                    Some("Information") => " -Level 4",
                    Some("Verbose") => " -Level 5",
                    _ => "",
                };

                let ps_cmd = format!(
                    "Get-WinEvent -LogName '{}' -MaxEvents {}{} -ErrorAction SilentlyContinue | Select-Object LevelDisplayName,TimeCreated,ProviderName,Id,Message | ConvertTo-Json -Compress",
                    log_name, max_entries, level_clause
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let mut entries = Vec::new();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for obj in json_array {
                            entries.push(EventLogEntry {
                                level: obj.get("LevelDisplayName").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                time: obj.get("TimeCreated").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                source: obj.get("ProviderName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                event_id: obj.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                message: obj.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            });
                        }
                    } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        entries.push(EventLogEntry {
                            level: obj.get("LevelDisplayName").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                            time: obj.get("TimeCreated").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            source: obj.get("ProviderName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            event_id: obj.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                            message: obj.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        });
                    }
                }

                log::info!("websockets -> Parsed {} event log entries", entries.len());
                let response = Cmd::EventLogResponse(entries);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // --- Windows Services ---
            Cmd::ListServices => {
                log::info!("websockets -> Listing services");

                let ps_cmd = "Get-CimInstance Win32_Service | Select-Object Name,DisplayName,State,StartMode,ProcessId | ConvertTo-Json -Compress";

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", ps_cmd])
                    .output()
                    .await;

                let mut services = Vec::new();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for obj in json_array {
                            services.push(WindowsService {
                                name: obj.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                display_name: obj.get("DisplayName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                status: obj.get("State").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                start_type: obj.get("StartMode").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                pid: obj.get("ProcessId").and_then(|v| v.as_u64()).map(|p| p as u32),
                            });
                        }
                    }
                }

                log::info!("websockets -> Found {} services", services.len());
                let response = Cmd::ServiceListResponse(services);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ControlService { name, action } => {
                log::info!("websockets -> Service control: {} - {:?}", name, action);

                let ps_cmd = match &action {
                    ServiceActionType::Start => format!("Start-Service -Name '{}' -ErrorAction Stop; 'OK'", name),
                    ServiceActionType::Stop => format!("Stop-Service -Name '{}' -Force -ErrorAction Stop; 'OK'", name),
                    ServiceActionType::Restart => format!("Restart-Service -Name '{}' -Force -ErrorAction Stop; 'OK'", name),
                    ServiceActionType::SetStartType(start_type) => format!("Set-Service -Name '{}' -StartupType '{}' -ErrorAction Stop; 'OK'", name, start_type),
                };

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, format!("Action completed: {:?}", action))
                        } else {
                            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            (false, stderr)
                        }
                    }
                    Err(e) => (false, format!("Failed to execute: {}", e)),
                };

                let response = Cmd::ServiceActionResponse { name, success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // --- Task Scheduler ---
            Cmd::ListScheduledTasks { folder } => {
                log::info!("websockets -> Listing scheduled tasks (folder: {:?})", folder);

                let folder_filter = folder.as_deref().unwrap_or("\\");
                let ps_cmd = format!(
                    r#"$tasks = Get-ScheduledTask -TaskPath '{}*' -ErrorAction SilentlyContinue; $results = @(); foreach($t in $tasks) {{ $info = $null; try {{ $info = Get-ScheduledTaskInfo -TaskName $t.TaskName -TaskPath $t.TaskPath -ErrorAction SilentlyContinue }} catch {{}}; $triggers = @(); foreach($tr in $t.Triggers) {{ $triggers += $tr.CimClass.CimClassName }}; $actions = @(); foreach($a in $t.Actions) {{ $actions += $a.Execute }}; $results += @{{ Name=$t.TaskName; Path=$t.TaskPath; State=$t.State.ToString(); LastRun=if($info){{$info.LastRunTime.ToString('o')}}else{{'Never'}}; NextRun=if($info){{$info.NextRunTime.ToString('o')}}else{{'N/A'}}; Description=$t.Description; Triggers=$triggers; Actions=$actions }} }}; $results | ConvertTo-Json -Compress -Depth 3"#,
                    folder_filter
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let mut tasks = Vec::new();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let parse_task = |obj: &serde_json::Value| -> ScheduledTask {
                        ScheduledTask {
                            name: obj.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            path: obj.get("Path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            state: obj.get("State").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                            last_run: obj.get("LastRun").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            next_run: obj.get("NextRun").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            description: obj.get("Description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            triggers: obj.get("Triggers").and_then(|v| v.as_array()).map(|arr| {
                                arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect()
                            }).unwrap_or_default(),
                            actions: obj.get("Actions").and_then(|v| v.as_array()).map(|arr| {
                                arr.iter().filter_map(|a| a.as_str().map(|s| s.to_string())).collect()
                            }).unwrap_or_default(),
                        }
                    };

                    if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for obj in &json_array {
                            tasks.push(parse_task(obj));
                        }
                    } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        tasks.push(parse_task(&obj));
                    }
                }

                log::info!("websockets -> Found {} scheduled tasks", tasks.len());
                let response = Cmd::ScheduledTaskListResponse(tasks);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ToggleScheduledTask { path, enable } => {
                log::info!("websockets -> {} task: {}", if enable { "Enable" } else { "Disable" }, path);

                let ps_cmd = if enable {
                    format!("Enable-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path)
                } else {
                    format!("Disable-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path)
                };

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, format!("Task {}", if enable { "enabled" } else { "disabled" }))
                        } else {
                            (false, String::from_utf8_lossy(&out.stderr).to_string())
                        }
                    }
                    Err(e) => (false, format!("Failed: {}", e)),
                };

                let response = Cmd::ScheduledTaskActionResponse { success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::RunScheduledTask(path) => {
                log::info!("websockets -> Running task: {}", path);

                let ps_cmd = format!("Start-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path);
                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, "Task started".to_string())
                        } else {
                            (false, String::from_utf8_lossy(&out.stderr).to_string())
                        }
                    }
                    Err(e) => (false, format!("Failed: {}", e)),
                };

                let response = Cmd::ScheduledTaskActionResponse { success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            // --- Registry ---
            Cmd::ListRegistryKeys(path) => {
                log::info!("websockets -> Listing registry keys: {}", path);

                let ps_cmd = format!(
                    r#"$subkeys = @(); $values = @(); try {{ Get-ChildItem -Path 'Registry::{path}' -ErrorAction Stop | ForEach-Object {{ $subkeys += @{{ Name=$_.PSChildName; Path=$_.Name; SubkeyCount=(Get-ChildItem -Path $_.PSPath -ErrorAction SilentlyContinue | Measure-Object).Count; ValueCount=(Get-ItemProperty -Path $_.PSPath -ErrorAction SilentlyContinue | Get-Member -MemberType NoteProperty | Where-Object {{ $_.Name -notmatch '^PS' }} | Measure-Object).Count }} }}; $props = Get-ItemProperty -Path 'Registry::{path}' -ErrorAction SilentlyContinue; if($props) {{ $props | Get-Member -MemberType NoteProperty | Where-Object {{ $_.Name -notmatch '^PS' }} | ForEach-Object {{ $n = $_.Name; $v = $props.$n; $kind = (Get-Item -Path 'Registry::{path}' -ErrorAction SilentlyContinue).GetValueKind($n); $values += @{{ Name=$n; Kind=$kind.ToString(); Data=[string]$v }} }} }} }} catch {{ }}; @{{ Subkeys=$subkeys; Values=$values }} | ConvertTo-Json -Compress -Depth 4"#
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let mut subkeys = Vec::new();
                let mut values = Vec::new();

                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        if let Some(sk_arr) = obj.get("Subkeys").and_then(|v| v.as_array()) {
                            for sk in sk_arr {
                                subkeys.push(RegistryKeyInfo {
                                    name: sk.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    path: sk.get("Path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    subkey_count: sk.get("SubkeyCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                    value_count: sk.get("ValueCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                });
                            }
                        }
                        if let Some(val_arr) = obj.get("Values").and_then(|v| v.as_array()) {
                            for val in val_arr {
                                values.push(RegistryValueEntry {
                                    name: val.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    kind: val.get("Kind").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                    data: val.get("Data").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                });
                            }
                        }
                    }
                }

                log::info!("websockets -> Registry: {} subkeys, {} values", subkeys.len(), values.len());
                let response = Cmd::RegistryKeyResponse { path, subkeys, values };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::BackupRegistryKey(path) => {
                log::info!("websockets -> Backing up registry key: {}", path);

                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                let backup_filename = format!("reg_backup_{}_{}.reg",
                    path.replace('\\', "_").replace('/', "_"),
                    timestamp
                );
                let backup_dir = std::env::temp_dir().join("mastertech_reg_backups");
                let _ = std::fs::create_dir_all(&backup_dir);
                let backup_path = backup_dir.join(&backup_filename);
                let backup_path_str = backup_path.to_string_lossy().to_string();

                let output = tokio::process::Command::new("reg")
                    .args(["export", &path, &backup_path_str, "/y"])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) => {
                        if out.status.success() {
                            (true, format!("Backup saved to {}", backup_path_str))
                        } else {
                            (false, String::from_utf8_lossy(&out.stderr).to_string())
                        }
                    }
                    Err(e) => (false, format!("Failed to backup: {}", e)),
                };

                let response = Cmd::RegistryBackupResponse {
                    success,
                    backup_path: backup_path_str,
                    message,
                };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::CommitRegistryEdits(edits) => {
                log::info!("websockets -> Committing {} registry edits", edits.len());

                let mut all_success = true;
                let mut messages = Vec::new();

                for edit in &edits {
                    let ps_cmd = match edit {
                        RegistryEdit::SetValue { path, name, kind, data } => {
                            let reg_type = match kind.as_str() {
                                "REG_DWORD" | "DWord" => "DWord",
                                "REG_QWORD" | "QWord" => "QWord",
                                "REG_BINARY" | "Binary" => "Binary",
                                "REG_MULTI_SZ" | "MultiString" => "MultiString",
                                "REG_EXPAND_SZ" | "ExpandString" => "ExpandString",
                                _ => "String",
                            };
                            format!(
                                "Set-ItemProperty -Path 'Registry::{}' -Name '{}' -Value '{}' -Type {} -ErrorAction Stop; 'OK'",
                                path, name, data, reg_type
                            )
                        }
                        RegistryEdit::DeleteValue { path, name } => {
                            format!(
                                "Remove-ItemProperty -Path 'Registry::{}' -Name '{}' -ErrorAction Stop; 'OK'",
                                path, name
                            )
                        }
                        RegistryEdit::CreateKey { path } => {
                            format!(
                                "New-Item -Path 'Registry::{}' -Force -ErrorAction Stop | Out-Null; 'OK'",
                                path
                            )
                        }
                        RegistryEdit::DeleteKey { path } => {
                            format!(
                                "Remove-Item -Path 'Registry::{}' -Recurse -Force -ErrorAction Stop; 'OK'",
                                path
                            )
                        }
                    };

                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output()
                        .await;

                    match output {
                        Ok(out) => {
                            if !out.status.success() {
                                all_success = false;
                                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                                messages.push(format!("Failed: {}", stderr));
                            }
                        }
                        Err(e) => {
                            all_success = false;
                            messages.push(format!("Error: {}", e));
                        }
                    }
                }

                let message = if all_success {
                    format!("All {} edit(s) applied successfully", edits.len())
                } else {
                    messages.join("; ")
                };

                let response = Cmd::RegistryEditResponse { success: all_success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ListStartupApps => {
                log::info!("websockets -> ListStartupApps");

                let ps_cmd = r#"
$paths = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
)
$results = @()
foreach ($path in $paths) {
    if (Test-Path $path) {
        $isApproved = $path -like "*StartupApproved*"
        $props = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue
        if ($props) {
            $memberNames = ($props | Get-Member -MemberType NoteProperty | Select-Object -ExpandProperty Name) | Where-Object { $_ -notin @('PSPath','PSParentPath','PSChildName','PSProvider','PSDrive') }
            foreach ($name in $memberNames) {
                $value = $props.$name
                $state = "Unknown"
                $cmd = ""
                if ($isApproved) {
                    if ($value -is [byte[]] -and $value.Length -ge 1) {
                        switch ($value[0]) {
                            0x02 { $state = "Enabled" }
                            0x03 { $state = "Disabled" }
                            0x06 { $state = "DisabledByUser" }
                            default { $state = "Unknown" }
                        }
                    }
                    $runPath = $path -replace 'StartupApproved\\Run','Run'
                    if (Test-Path $runPath) {
                        $runProps = Get-ItemProperty -Path $runPath -ErrorAction SilentlyContinue
                        if ($runProps -and ($runProps | Get-Member -Name $name -ErrorAction SilentlyContinue)) {
                            $cmd = [string]$runProps.$name
                        }
                    }
                } else {
                    $cmd = [string]$value
                    $approvedPath = $path -replace '\\Run$','\Explorer\StartupApproved\Run'
                    if (Test-Path $approvedPath) {
                        $approvedProps = Get-ItemProperty -Path $approvedPath -ErrorAction SilentlyContinue
                        if ($approvedProps -and ($approvedProps | Get-Member -Name $name -ErrorAction SilentlyContinue)) {
                            $aVal = $approvedProps.$name
                            if ($aVal -is [byte[]] -and $aVal.Length -ge 1) {
                                switch ($aVal[0]) {
                                    0x02 { $state = "Enabled" }
                                    0x03 { $state = "Disabled" }
                                    0x06 { $state = "DisabledByUser" }
                                    default { $state = "Unknown" }
                                }
                            }
                        } else { $state = "Enabled" }
                    } else { $state = "Enabled" }
                }

                $source = if ($path -like "HKLM:*") { "HKLM" } else { "HKCU" }
                if ($path -like "*WOW6432Node*") { $source = "HKLM (32-bit)" }
                if ($isApproved) { $source += " (Approved)" }

                if (-not $isApproved -or $cmd -ne "") {
                    $results += [pscustomobject]@{
                        name = $name
                        command = $cmd
                        registry_path = $path
                        state = $state
                        source = $source
                    }
                }
            }
        }
    }
}
$results | Sort-Object -Property name -Unique | ConvertTo-Json -Depth 3
"#;

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", ps_cmd])
                    .output()
                    .await;

                let apps: Vec<StartupApp> = match output {
                    Ok(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let trimmed = stdout.trim();
                        if trimmed.is_empty() || trimmed == "null" {
                            Vec::new()
                        } else {
                            serde_json::from_str::<Vec<StartupApp>>(trimmed)
                                .or_else(|_| serde_json::from_str::<StartupApp>(trimmed).map(|s| vec![s]))
                                .unwrap_or_default()
                        }
                    }
                    Ok(out) => {
                        log::error!("ListStartupApps failed: {}", String::from_utf8_lossy(&out.stderr));
                        Vec::new()
                    }
                    Err(e) => {
                        log::error!("ListStartupApps error: {e}");
                        Vec::new()
                    }
                };

                let response = Cmd::StartupAppsResponse(apps);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::ToggleStartupApp { name, registry_path, enable } => {
                log::info!("websockets -> ToggleStartupApp: {} -> enable={}", name, enable);

                // Determine the StartupApproved path from the registry_path
                let approved_path = if registry_path.contains("StartupApproved") {
                    registry_path.clone()
                } else {
                    registry_path.replace("\\Run", "\\Explorer\\StartupApproved\\Run")
                };

                let byte_val = if enable { "0x02" } else { "0x03" };

                let ps_cmd = format!(
                    r#"
$path = '{approved_path}'
$name = '{name}'
if (Test-Path $path) {{
    $props = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue
    if ($props -and ($props | Get-Member -Name $name -ErrorAction SilentlyContinue)) {{
        $current = $props.$name
        if ($current -is [byte[]]) {{
            $current[0] = {byte_val}
            Set-ItemProperty -Path $path -Name $name -Value ([byte[]]$current) -ErrorAction Stop
            'OK'
        }} else {{
            $newVal = [byte[]]@({byte_val},0,0,0,0,0,0,0,0,0,0,0)
            Set-ItemProperty -Path $path -Name $name -Value $newVal -ErrorAction Stop
            'OK'
        }}
    }} else {{
        $newVal = [byte[]]@({byte_val},0,0,0,0,0,0,0,0,0,0,0)
        New-ItemProperty -Path $path -Name $name -Value $newVal -PropertyType Binary -ErrorAction Stop | Out-Null
        'OK'
    }}
}} else {{
    Write-Error "Registry path not found: $path"
}}
"#
                );

                let output = tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &ps_cmd])
                    .output()
                    .await;

                let (success, message) = match output {
                    Ok(out) if out.status.success() => {
                        let action = if enable { "enabled" } else { "disabled" };
                        (true, format!("'{}' {}", name, action))
                    }
                    Ok(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        (false, format!("Failed to toggle '{}': {}", name, stderr))
                    }
                    Err(e) => (false, format!("Error: {}", e)),
                };

                let response = Cmd::StartupAppActionResponse { success, message };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::GetRemoteScriptList => {
                log::info!("websockets -> GetRemoteScriptList");
                let builtin = |name: &str, cat: &str| RemoteScriptItem {
                    name: name.into(), category: cat.into(), content: None,
                };
                let categories = vec![
                    ("Tuneup / QC".to_string(), vec![
                        builtin("Data Transfer", "Tuneup / QC"),
                        builtin("Activate Webroot", "Tuneup / QC"),
                        builtin("Activate SuperAnti", "Tuneup / QC"),
                        builtin("Activate SEB", "Tuneup / QC"),
                        builtin("Install Windows Updates", "Tuneup / QC"),
                        builtin("Disable Sleep / Hibernation", "Tuneup / QC"),
                        builtin("Run SuperAntiSpyware Scan", "Tuneup / QC"),
                        builtin("Run Webroot Scan", "Tuneup / QC"),
                        builtin("Run Tron", "Tuneup / QC"),
                        builtin("Install LibreOffice", "Tuneup / QC"),
                        builtin("Disable proxy settings", "Tuneup / QC"),
                        builtin("Disable Notifications", "Tuneup / QC"),
                        builtin("Change SuperAntiSpyware settings", "Tuneup / QC"),
                        builtin("Disable Startup Apps", "Tuneup / QC"),
                        builtin("Unpin Copilot", "Tuneup / QC"),
                        builtin("Align Taskbar to left", "Tuneup / QC"),
                    ]),
                    ("Informational".to_string(), vec![
                        builtin("Is SuperEasyBackup installed?", "Informational"),
                        builtin("Is Webroot installed?", "Informational"),
                        builtin("Is SuperAntiSpyware installed?", "Informational"),
                        builtin("Are there scheduled tasks for it?", "Informational"),
                        builtin("Is Windows Activated?", "Informational"),
                        builtin("Is Hibernation/Sleep enabled?", "Informational"),
                        builtin("Any Recent Blue Screens?", "Informational"),
                        builtin("When Was The Last Service Date?", "Informational"),
                        builtin("Windows Version", "Informational"),
                        builtin("Check Updates", "Informational"),
                        builtin("Run Prechecks", "Informational"),
                    ]),
                    ("Junkware Removal".to_string(), vec![
                        builtin("OneLaunch", "Junkware Removal"),
                        builtin("WebNavigator Browser", "Junkware Removal"),
                        builtin("Wave Browser", "Junkware Removal"),
                        builtin("Clear Browser", "Junkware Removal"),
                        builtin("Shift Browser", "Junkware Removal"),
                        builtin("Avast Browser", "Junkware Removal"),
                        builtin("Mcaffee Safe", "Junkware Removal"),
                        builtin("Driver Support", "Junkware Removal"),
                        builtin("Winzip", "Junkware Removal"),
                    ]),
                ];
                let response = Cmd::RemoteScriptListResponse { categories };
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::RunRemoteScripts { scripts, service_number, customer_email } => {
                log::info!("websockets -> RunRemoteScripts: {} scripts, SO={}", scripts.len(), service_number);

                let send_log = |sender: &mut ClientTransport, msg: String| {
                    let cmd = Cmd::RemoteScriptLog(msg);
                    if let Ok(payload) = encode_to_vec(&cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                };

                let send_result = |sender: &mut ClientTransport, name: &str, status: RemoteScriptStatus| {
                    let cmd = Cmd::RemoteScriptResult { name: name.to_string(), status };
                    if let Ok(payload) = encode_to_vec(&cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                };

                for script in &scripts {
                    send_log(sender, format!("Starting: {}", script.name));

                    match script.name.as_str() {
                        "Disable Sleep / Hibernation" => {
                            match crate::terminal_mode::tabs::script_categories::disable_hibernation_and_sleep() {
                                Ok(_) => {
                                    send_log(sender, "Disabled Sleep / Hibernation".into());
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("Error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Activate Webroot" => {
                            if service_number.is_empty() {
                                send_log(sender, "Webroot activation requires SO number".into());
                                send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                continue;
                            }
                            send_log(sender, "Fetching CPS keys...".into());
                            let so = service_number.clone();
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::tabs::tur_sheet::get_ticket::SendRequest::get_cps(so, client.clone()).await {
                                Ok(keys) => {
                                    let key = keys.get(0).cloned().unwrap_or_default();
                                    send_log(sender, format!("Webroot key: {}", key.webroot_key));
                                    match crate::utilities::scripts::antivirus::install_webroot(key.webroot_key, client, progress_tx).await {
                                        Ok(_) => {
                                            send_log(sender, "Webroot installed successfully".into());
                                            send_result(sender, &script.name, RemoteScriptStatus::Success);
                                        }
                                        Err(e) => {
                                            send_log(sender, format!("Webroot install error: {e}"));
                                            send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                        }
                                    }
                                }
                                Err(e) => {
                                    send_log(sender, format!("Failed to get CPS keys: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Activate SuperAnti" => {
                            if service_number.is_empty() {
                                send_log(sender, "SuperAnti activation requires SO number".into());
                                send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                continue;
                            }
                            let killed = crate::utilities::scripts::antivirus::kill_sas_processes();
                            send_log(sender, format!("Killed {killed} SAS processes"));
                            let so = service_number.clone();
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::tabs::tur_sheet::get_ticket::SendRequest::get_cps(so, client.clone()).await {
                                Ok(keys) => {
                                    let key = keys.get(0).cloned().unwrap_or_default();
                                    send_log(sender, format!("SuperAnti key: {}", key.superanti_key));
                                    let key_str = key.superanti_key.clone();
                                    match crate::utilities::scripts::antivirus::install_sas(key.superanti_key, client, progress_tx).await {
                                        Ok(_) => {
                                            send_log(sender, "SAS installed successfully".into());
                                            let killed = crate::utilities::scripts::antivirus::kill_sas_processes();
                                            send_log(sender, format!("Post-install killed {killed} SAS processes"));
                                            std::thread::sleep(std::time::Duration::from_secs(2));
                                            use crate::utilities::scripts::antivirus::sas_tasks::configure_sas_with_activation;
                                            match configure_sas_with_activation(&key_str) {
                                                Ok((upd, scan)) => {
                                                    send_log(sender, format!("SAS activated: update task {upd}, scan task {scan}"));
                                                    // Launch SAS with /REGCODE to trigger online
                                                    // registration — clears the "expired" banner
                                                    // and enables Real-Time Protection.
                                                    let regcode_arg = format!("/REGCODE:{}", key_str);
                                                    let sas_exe = r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe";
                                                    if std::path::Path::new(sas_exe).exists() {
                                                        match std::process::Command::new(sas_exe).arg(&regcode_arg).spawn() {
                                                            Ok(_) => send_log(sender, "SAS launched with /REGCODE for online registration".into()),
                                                            Err(e) => send_log(sender, format!("SAS /REGCODE launch error: {e}")),
                                                        }
                                                    }
                                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                                }
                                                Err(e) => {
                                                    send_log(sender, format!("SAS activation/settings error: {e}"));
                                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            send_log(sender, format!("SAS install error: {e}"));
                                            send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                        }
                                    }
                                }
                                Err(e) => {
                                    send_log(sender, format!("Failed to get CPS keys: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Activate SEB" => {
                            if service_number.is_empty() || customer_email.is_empty() {
                                send_log(sender, "SEB activation requires SO number and email".into());
                                send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                continue;
                            }
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::utilities::scripts::antivirus::install_supereasybackup(customer_email.clone(), client, progress_tx).await {
                                Ok(_) => {
                                    send_log(sender, "SEB installed successfully".into());
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("SEB install error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Install Windows Updates" => {
                            send_log(sender, "Checking internet before Windows Updates...".into());
                            match crate::utilities::windows::net_adapter::ensure_internet_connected().await {
                                Ok(_) => send_log(sender, "Internet confirmed".into()),
                                Err(e) => {
                                    send_log(sender, format!("No internet: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                    continue;
                                }
                            }
                            send_log(sender, "Starting Windows Updates (search + install)...".into());

                            let (update_tx, update_rx) = crossbeam::channel::unbounded();
                            let handle = std::thread::spawn(move || {
                                crate::utilities::windows::windows_update::install_windows_updates(
                                    update_tx, true, true,
                                )
                            });

                            loop {
                                match update_rx.recv_timeout(std::time::Duration::from_millis(250)) {
                                    Ok(event) => {
                                        use crate::utilities::windows::windows_update::WindowsUpdateEvent;
                                        match event {
                                            WindowsUpdateEvent::UpdateLogs(msg) => send_log(sender, msg),
                                            WindowsUpdateEvent::DownloadPercentage(pct) => {
                                                send_log(sender, format!("Download: {pct}%"));
                                            }
                                            WindowsUpdateEvent::InstallPercentage(pct) => {
                                                send_log(sender, format!("Install: {pct}%"));
                                            }
                                            WindowsUpdateEvent::ReturnedUpdates(updates) => {
                                                send_log(sender, format!("{} updates processed", updates.updates.len()));
                                                for u in &updates.updates {
                                                    send_log(sender, format!("  {} (installed: {})", u.title, u.is_installed));
                                                }
                                            }
                                        }
                                    }
                                    Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                                        if handle.is_finished() {
                                            while let Ok(event) = update_rx.try_recv() {
                                                use crate::utilities::windows::windows_update::WindowsUpdateEvent;
                                                match event {
                                                    WindowsUpdateEvent::UpdateLogs(msg) => send_log(sender, msg),
                                                    WindowsUpdateEvent::DownloadPercentage(pct) => send_log(sender, format!("Download: {pct}%")),
                                                    WindowsUpdateEvent::InstallPercentage(pct) => send_log(sender, format!("Install: {pct}%")),
                                                    WindowsUpdateEvent::ReturnedUpdates(updates) => {
                                                        send_log(sender, format!("{} updates processed", updates.updates.len()));
                                                    }
                                                }
                                            }
                                            break;
                                        }
                                    }
                                    Err(crossbeam::channel::RecvTimeoutError::Disconnected) => break,
                                }
                            }

                            match handle.join() {
                                Ok(Ok(_)) => {
                                    send_log(sender, "Windows Updates completed successfully".into());
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Ok(Err(e)) => {
                                    send_log(sender, format!("Windows Updates error: {e:?}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                                Err(_) => {
                                    send_log(sender, "Windows Updates thread panicked".into());
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Install LibreOffice" => {
                            let client = reqwest::Client::new();
                            let (progress_tx, _) = crossbeam::channel::unbounded();
                            match crate::utilities::scripts::programs::install_program(
                                "https://ninite.com/libreoffice/ninite.exe".into(), client, progress_tx
                            ).await {
                                Ok(_) => {
                                    send_log(sender, "LibreOffice installed".into());
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("LibreOffice install error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }
                        
                        "Disable Notifications" => if cfg!(target_os = "windows") {
                            let mut msgs = Vec::new();
                            let mut ok = true;
                            macro_rules! try_reg {
                                ($fn:expr, $label:expr) => {
                                    match $fn() {
                                        Ok(r) => msgs.push(format!("{}: {:?}", $label, r)),
                                        Err(e) => { msgs.push(format!("{}: {e}", $label)); ok = false; }
                                    }
                                }
                            }
                            use crate::utilities::windows::registry::*;
                            try_reg!(disable_notifications, "notifications");
                            try_reg!(disable_lockscreen_notifications, "lockscreen_notifications");
                            try_reg!(disable_content_delivery_allowed, "content_delivery");
                            try_reg!(disable_silent_installed_apps_enabled, "silent_apps");
                            try_reg!(disable_subscribed_content_enabled, "subscribed_content");
                            try_reg!(disable_system_pane_suggestions_enabled, "system_suggestions");
                            try_reg!(disable_account_notifications, "account_notifications");
                            try_reg!(enable_more_pins_layout, "more_pins_layout");
                            try_reg!(disable_start_account_notifications, "start_account_notifications");
                            try_reg!(disable_recent_items_tracking, "recent_items");
                            try_reg!(remove_chat_from_taskbar, "chat_taskbar");
                            for m in &msgs { send_log(sender, m.clone()); }
                            send_result(sender, &script.name, if ok { RemoteScriptStatus::Success } else { RemoteScriptStatus::Failed });
                        }

                        "Unpin Copilot" => {
                            match crate::utilities::windows::registry::disable_copilot() {
                                Ok(results) => {
                                    for r in &results { send_log(sender, r.clone()); }
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("Error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Align Taskbar to left" => {
                            match crate::utilities::windows::registry::align_taskbar_left() {
                                Ok(msgs) => {
                                    for m in &msgs { send_log(sender, m.trim().to_string()); }
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("Error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Change SuperAntiSpyware settings" => {
                            let sas_exe = std::path::Path::new(r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe");
                            if !sas_exe.exists() {
                                send_log(sender, "SAS not installed".into());
                                send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                continue;
                            }
                            let killed = crate::utilities::scripts::antivirus::kill_sas_processes();
                            send_log(sender, format!("Killed {killed} SAS processes"));
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            match crate::utilities::scripts::antivirus::sas_tasks::configure_sas_scheduled_tasks() {
                                Ok((update_guid, scan_guid)) => {
                                    send_log(sender, format!("SAS update task: {update_guid}"));
                                    send_log(sender, format!("SAS scan task: {scan_guid}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("Error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Is Windows Activated?" => {
                            match crate::terminal_mode::tabs::script_categories::check_windows_activation() {
                                Ok(status) => {
                                    let msg = if status.license_status == 1 { "Windows is activated" } else { "Windows is NOT activated" };
                                    send_log(sender, msg.into());
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("Error: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Windows Version" => {
                            let ver = sysinfo::System::long_os_version().unwrap_or_default();
                            send_log(sender, format!("Windows Version: {ver}"));
                            send_result(sender, &script.name, RemoteScriptStatus::Success);
                        }

                        "Is SuperEasyBackup installed?" | "Is Webroot installed?" | "Is SuperAntiSpyware installed?" => {
                            let search_term = match script.name.as_str() {
                                "Is SuperEasyBackup installed?" => "supereasybackup",
                                "Is Webroot installed?" => "webroot",
                                "Is SuperAntiSpyware installed?" => "superantispyware",
                                _ => "",
                            };
                            match crate::utilities::scripts::programs::InstalledProgram::get_installed_programs() {
                                Ok(programs) => {
                                    let found = programs.iter().any(|p| {
                                        let dn = p.display_name.clone().unwrap_or_default().to_lowercase();
                                        let pub_ = p.publisher.clone().unwrap_or_default().to_lowercase();
                                        dn.contains(search_term) || pub_.contains(search_term)
                                    });
                                    if found {
                                        send_log(sender, format!("{} found", script.name.trim_end_matches('?')));
                                    } else {
                                        send_log(sender, format!("{} NOT found", script.name.trim_end_matches('?')));
                                    }
                                    send_result(sender, &script.name, RemoteScriptStatus::Success);
                                }
                                Err(e) => {
                                    send_log(sender, format!("Error querying programs: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                }
                            }
                        }

                        "Disable proxy settings" | "Disable Startup Apps" | "Run Tron"
                        | "Run SuperAntiSpyware Scan" | "Run Webroot Scan" | "Data Transfer"
                        | "Any Recent Blue Screens?" | "When Was The Last Service Date?"
                        | "Are there scheduled tasks for it?" | "Is Hibernation/Sleep enabled?"
                        | "Check Updates" | "Run Prechecks" | "Run Junkware Category" => {
                            send_log(sender, format!("'{}' not yet implemented for remote execution", script.name));
                            send_result(sender, &script.name, RemoteScriptStatus::Failed);
                        }

                        _ => {
                            if let Some(content) = &script.content {
                                send_log(sender, format!("Running custom script: {}", script.name));
                                let ext = if script.name.ends_with(".bat") || script.name.ends_with(".cmd") {
                                    "bat"
                                } else {
                                    "ps1"
                                };
                                let temp_dir = std::env::temp_dir();
                                let script_file = temp_dir.join(format!("mastertech_custom_{}.{}", uuid::Uuid::new_v4(), ext));
                                if let Err(e) = std::fs::write(&script_file, content) {
                                    send_log(sender, format!("Failed to write script: {e}"));
                                    send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                    continue;
                                }

                                let output = if ext == "ps1" {
                                    tokio::process::Command::new("powershell")
                                        .args(["-ExecutionPolicy", "Bypass", "-File", &script_file.to_string_lossy()])
                                        .output()
                                        .await
                                } else {
                                    tokio::process::Command::new("cmd")
                                        .args(["/C", &script_file.to_string_lossy()])
                                        .output()
                                        .await
                                };

                                let _ = std::fs::remove_file(&script_file);

                                match output {
                                    Ok(out) => {
                                        let stdout = String::from_utf8_lossy(&out.stdout);
                                        let stderr = String::from_utf8_lossy(&out.stderr);
                                        if !stdout.is_empty() {
                                            for line in stdout.lines() {
                                                send_log(sender, line.to_string());
                                            }
                                        }
                                        if !stderr.is_empty() {
                                            for line in stderr.lines() {
                                                send_log(sender, format!("[stderr] {}", line));
                                            }
                                        }
                                        if out.status.success() {
                                            send_result(sender, &script.name, RemoteScriptStatus::Success);
                                        } else {
                                            send_log(sender, format!("Exit code: {:?}", out.status.code()));
                                            send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                        }
                                    }
                                    Err(e) => {
                                        send_log(sender, format!("Execution error: {e}"));
                                        send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                    }
                                }
                            } else if script.category == "Junkware Removal" {
                                send_log(sender, format!("Attempting to uninstall: {}", script.name));
                                match crate::utilities::scripts::programs::InstalledProgram::get_by_name(&script.name) {
                                    Ok(Some(program)) => {
                                        let _ = program.uninstall();
                                        send_log(sender, format!("Uninstall initiated for {}", script.name));
                                        send_result(sender, &script.name, RemoteScriptStatus::Success);
                                    }
                                    Ok(None) => {
                                        send_log(sender, format!("{} not found / already removed", script.name));
                                        send_result(sender, &script.name, RemoteScriptStatus::Success);
                                    }
                                    Err(e) => {
                                        send_log(sender, format!("Error: {e}"));
                                        send_result(sender, &script.name, RemoteScriptStatus::Failed);
                                    }
                                }
                            } else {
                                send_log(sender, format!("Unknown script: {}", script.name));
                                send_result(sender, &script.name, RemoteScriptStatus::Failed);
                            }
                        }
                    }
                }

                let complete = Cmd::RemoteScriptsComplete;
                if let Ok(payload) = encode_to_vec(&complete, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::RunScriptContent { filename, content } => {
                log::info!("RunScriptContent: filename={filename}");
                let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();

                let send_log = |sender: &mut ClientTransport, msg: String| {
                    if let Ok(payload) = encode_to_vec(&Cmd::RemoteScriptLog(msg), standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                };

                send_log(sender, format!("Running script: {filename}"));

                let output = match ext.as_str() {
                    "ps1" => {
                        std::process::Command::new("powershell")
                            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &content])
                            .output()
                    }
                    "bat" | "cmd" => {
                        std::process::Command::new("cmd")
                            .args(["/C", &content])
                            .output()
                    }
                    _ => {
                        send_log(sender, format!("Unsupported script type: .{ext}"));
                        return;
                    }
                };

                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if !stdout.is_empty() {
                            send_log(sender, stdout.to_string());
                        }
                        if !stderr.is_empty() {
                            send_log(sender, format!("[stderr] {stderr}"));
                        }
                        send_log(sender, format!("Script {filename} exited with code: {}", out.status));
                    }
                    Err(e) => {
                        send_log(sender, format!("Failed to run script {filename}: {e}"));
                    }
                }
            }

            Cmd::LoadWasmPlugin { plugin_id, wasm_bytes } => {
                let size = wasm_bytes.len();
                log::info!("Received remote WASM plugin '{plugin_id}' ({size} bytes)");
                let tx = displays::plugins::wasm_load_sender();
                let result_cmd = if tx.try_send((plugin_id.clone(), wasm_bytes)).is_ok() {
                    Cmd::LoadWasmPluginResult {
                        plugin_id,
                        success: true,
                        message: format!("Plugin queued for loading ({size} bytes)"),
                    }
                } else {
                    Cmd::LoadWasmPluginResult {
                        plugin_id,
                        success: false,
                        message: "WASM load channel full or disconnected".to_string(),
                    }
                };
                if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::SetFrameCapture { enabled } => {
                log::info!("SetFrameCapture received: enabled={enabled}");
                let tx = displays::plugins::frame_capture_sender();
                let _ = tx.try_send(enabled);
            }

            Cmd::CallRemotePluginTool { request_id, plugin_id, tool_name, args_json } => {
                log::info!("CallRemotePluginTool: {plugin_id}::{tool_name} req={request_id}");
                let call_tx = displays::plugins::remote_tool_call_sender();
                let _ = call_tx.try_send((request_id.clone(), plugin_id.clone(), tool_name.clone(), args_json));
                let result_rx = displays::plugins::remote_tool_result_receiver();
                let mut result: Option<(bool, String)> = None;
                for _ in 0..1200 {
                    if let Ok((rid, success, rjson)) = result_rx.try_recv() {
                        if rid == request_id {
                            result = Some((success, rjson));
                            break;
                        }
                    }
                    // Yield to tokio so the PluginManager background task can run and process the call
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                let (success, result_json) = result.unwrap_or((
                    false,
                    "PluginManager did not process the call within 12 seconds".to_string(),
                ));
                let result_cmd = Cmd::RemotePluginToolResult {
                    request_id,
                    plugin_id,
                    tool_name,
                    success,
                    result_json,
                };
                if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                    sender.send(WsMessage::Binary(payload));
                }
            }

            Cmd::DirectFileTransfer { filename, chunk_index, total_chunks, data } => {
                log::info!("DirectFileTransfer: {filename} chunk {chunk_index}/{total_chunks} ({} bytes)", data.len());
                let entry = self.file_transfer_buffers
                    .entry(filename.clone())
                    .or_insert_with(|| (total_chunks, Vec::new()));
                entry.1.push((chunk_index, data));

                if entry.1.len() as u32 == total_chunks {
                    let (_, mut chunks) = self.file_transfer_buffers.remove(&filename).unwrap();
                    chunks.sort_by_key(|(idx, _)| *idx);
                    let full_data: Vec<u8> = chunks.into_iter().flat_map(|(_, d)| d).collect();
                    let size = full_data.len();

                    let transfer_dir = std::env::var("USERPROFILE")
                        .map(|p| std::path::PathBuf::from(p).join("Desktop"))
                        .unwrap_or_else(|_| std::env::temp_dir());
                    let _ = std::fs::create_dir_all(&transfer_dir);
                    let save_path = transfer_dir.join(&filename);
                    let result_cmd = match std::fs::write(&save_path, &full_data) {
                        Ok(()) => {
                            log::info!("File saved: {} ({size} bytes)", save_path.display());
                            Cmd::DirectFileTransferResult {
                                filename,
                                success: true,
                                message: format!("Saved to {} ({size} bytes)", save_path.display()),
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to save file {}: {e}", save_path.display());
                            Cmd::DirectFileTransferResult {
                                filename,
                                success: false,
                                message: format!("Write failed: {e}"),
                            }
                        }
                    };
                    if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                }
            }

            Cmd::None => {},
            // ── Remote self-update (terminal-mode path) ──────────────────
            Cmd::MastertechSelfUpdateChunk { chunk_index, total_chunks, data } => {
                log::info!(
                    "[self-update] chunk {}/{} ({} bytes) via terminal WS",
                    chunk_index + 1,
                    total_chunks,
                    data.len(),
                );
                if let Some(bytes) = self.self_update_buffer.push(chunk_index, total_chunks, data) {
                    log::info!("[self-update] all {} chunks received — applying…", total_chunks);
                    let (success, message) = crate::remote_self_update::apply_and_relaunch(bytes);
                    log::info!("[self-update] result: success={success} message={message}");
                    let result_cmd = displays::Cmd::MastertechSelfUpdateResult { success, message };
                    if let Ok(payload) = encode_to_vec(&result_cmd, standard()) {
                        sender.send(WsMessage::Binary(payload));
                    }
                    if success {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        std::process::exit(0);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Convert HBITMAP to PNG bytes (Windows only)
#[cfg(target_os = "windows")]
pub fn hbitmap_to_png_bytes(
    hbmp: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Result<Vec<u8>, String> {
    use windows::Win32::Graphics::Gdi::*;
    
    let mut bmp = BITMAP::default();
    if unsafe { GetObjectW(
        HGDIOBJ(hbmp.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut _ as *mut _),
    ) } == 0
    {
        let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
        return Err("GetObjectW failed".into());
    }
    
    let width = bmp.bmWidth as i32;
    let height = bmp.bmHeight as i32;
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: Default::default(),
    };
    
    let stride = (width * 4) as usize;
    let mut buffer = vec![0u8; stride * height as usize];
    let hdc: HDC = unsafe { CreateCompatibleDC(None) };
    
    if hdc.0.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
        return Err("CreateCompatibleDC failed".into());
    }
    
    let _old = unsafe { SelectObject(hdc, HGDIOBJ(hbmp.0)) };
    let got = unsafe { GetDIBits(
        hdc,
        hbmp,
        0,
        height as u32,
        Some(buffer.as_mut_ptr() as *mut _),
        &mut bi as *mut _,
        DIB_RGB_COLORS,
    ) };
    
    let _ = unsafe { DeleteDC(hdc) };
    let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
    
    if got == 0 {
        return Err("GetDIBits failed".into());
    }
    
    // Convert BGRA to RGBA
    for px in buffer.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    
    let img = image::RgbaImage::from_raw(width as u32, height as u32, buffer)
        .ok_or("rgba from raw failed")?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    
    Ok(png)
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

pub async fn create_client(mut client: ConnectedClient) -> anyhow::Result<ConnectedClient> {
    client.connected = true;

    // Fetch the existing row first so we can honor `customer_locked`. The
    // OA3 product-key lookup below resolves to the *original* Windows
    // license purchaser; for used machines that the shop has resold the
    // friendly_name from this lookup is wrong and admins manually re-link
    // via the admin console, which sets `customer_locked = true`. We must
    // not clobber that here on every reconnect.
    let existing_row = query_id::<ConnectedClient>(
        CONNECTED_CLIENT_TABLE.to_string(),
        client.id.clone(),
    )
    .await;
    log::info!("websockets -> query_id: {existing_row:?}");

    let existing: Option<ConnectedClient> = match &existing_row {
        Ok(opt) => opt.clone(),
        Err(_) => None,
    };
    let locked = existing.as_ref().map(|c| c.customer_locked).unwrap_or(false);

    // Carry the lock + admin-set linkage forward across the upsert so we
    // never accidentally reset them when the local client builds a
    // fresh `ConnectedClient` from scratch on startup.
    if let Some(prev) = existing.as_ref() {
        client.customer_locked = prev.customer_locked;
        if locked {
            client.friendly_name = prev.friendly_name.clone();
            client.customer = prev.customer.clone();
        }
    }

    // Attempt to lookup customer by OA3 serial number (Windows only).
    // Skipped entirely when `customer_locked` is true.
    #[cfg(target_os = "windows")]
    if !locked {
        use crate::filesystem::oa_serial::{get_oa_style_serial, to_oa3_13digit};
        use crate::filesystem::customer_lookup::lookup_customer_by_serial;

        match get_oa_style_serial() {
            Ok(raw_serial) => {
                log::info!("websockets -> Raw OA serial: {}", raw_serial);

                match to_oa3_13digit(&raw_serial) {
                    Ok(serial13) => {
                        log::info!("websockets -> 13-digit serial: {}", serial13);

                        match lookup_customer_by_serial(&serial13).await {
                            Ok(customer_string) => {
                                log::info!("websockets -> Customer found: {}", customer_string);
                                client.friendly_name = Some(customer_string);
                            }
                            Err(e) => {
                                log::warn!("websockets -> Customer lookup failed: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("websockets -> Failed to convert serial to 13-digit: {:?}", e);
                    }
                }
            }
            Err(e) => {
                log::warn!("websockets -> Failed to get OA serial: {:?}", e);
            }
        }
    } else {
        log::info!(
            "websockets -> create_client: customer_locked is true; \
             skipping OA-serial customer lookup"
        );
    }

    let check_id_existence = check_id_existence(
        CONNECTED_CLIENT_TABLE.to_string(),
        client.id.clone(),
    )
    .await;

    log::info!("websockets -> check_id_existence: {check_id_existence:?}");

    if let Ok(Some(_)) = existing_row {
        log::info!("WE HAVE A CLIENT");
        if client.friendly_name.is_some() {
            let res: Option<ConnectedClient> = DATABASE
                .upsert(client.id.clone())
                .content(client.clone())
                .await?
                .take();
            log::info!("websockets -> Updated existing client with friendly_name: {res:?}");
        }
    } else {
        let res: Option<ConnectedClient> = DATABASE
            .upsert(client.id.clone())
            .content(client.clone())
            .await?
            .take();

        log::info!("websockets -> Upsert: {res:?}");
    }
    Ok(client)
}