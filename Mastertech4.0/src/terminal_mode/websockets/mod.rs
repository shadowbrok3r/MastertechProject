use database::{schema::{utilities::{check_id_existence, query_id}, ConnectedClient, CONNECTED_CLIENT_TABLE}, DATABASE, WS_CLIENT_URL, WS_CLIENT_URL_LOCAL};
use displays::{deserialize_command, remote_viewer::{encode_buffer_with_timestamp, ratagui::TerminalEvent}, serialize_system_info, tabs::admin_console::client_action::ClientHandler, Cmd, FileSystemAction, RemoteDirEntry};
use crate::{filesystem::{get_client_hash, system_info::get_sysinfo_no_gpu}, tabs::file_browser::read_folder};
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
            Cmd::None => {},
            _ => {}
        }
    }
}

/// Convert HBITMAP to PNG bytes (Windows only)
#[cfg(target_os = "windows")]
fn hbitmap_to_png_bytes(
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