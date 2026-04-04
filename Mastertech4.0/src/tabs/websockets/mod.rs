use eframe::{egui::{Align, Button, CentralPanel, Color32, Context, Direction, Frame, Id, Key, Layout, Margin, Rect, RichText, ScrollArea, Sense, Shape, Stroke, TextEdit, Ui, Vec2, Widget}, epaint::Shadow};
use database::{schema::{utilities::{compress_data, query_id}, ConnectedClient, Record, SystemInformation, CONNECTED_CLIENT_TABLE}, DATABASE};
use egui::TextBuffer;
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, process::{Child, ChildStdin, Command}, spawn, sync::Mutex, time::sleep};
use crate::{app_state::MastertechContext, filesystem::system_info::generate_client_id, tabs::file_browser::read_folder};
use std::{env, path::Path, process::Stdio, sync::{atomic::Ordering, Arc}, time::{Duration, Instant}};
use displays::{channel_manager::ChannelManager, deserialize_command, serialize_system_info, virtual_filesystem::FileSystem, Cmd, EGUI_INPUT_TAG, FileSystemAction};
use displays::plugins::EguiInputEvent;
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use crate::filesystem::system_info::get_sysinfo;
use crossbeam::channel::{Receiver, Sender};
use bincode::{config::standard, serde::*};
use database::schema::RecordId;
use anyhow::{Result, Error};
use log::{error, info};

impl MastertechContext{
    pub fn websockets(&mut self, ui: &mut Ui) {
        if !self.show_ws_viewport.load(Ordering::Relaxed) {
            eframe::egui::Panel::top("Client Top Panel").show_inside(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Connect").clicked() {
                        self.connect(ui.ctx().clone());
                    }

                    ui.add_space(5.);
                    if ui.add_enabled(
                        !self.client_friendly_name.is_empty(), 
                        Button::new("Update Name")
                    )
                        .clicked() 
                    {
                        let name = self.client_friendly_name.clone();
                        let client = self.client_uuid.clone();
                        spawn(async move {
                            let _update_client = DATABASE.query("UPDATE $client SET friendly_name = $name")
                                .bind(("name", name))
                                .bind(("client", client))
                                .await?;
    
                            info!("websockets -> update_client: {_update_client:?}");
                            Ok::<(), Error>(())
                        });
                    }

                    ui.add_space(5.);
                    TextEdit::singleline(&mut self.client_friendly_name)
                        .hint_text("Client Name")
                        .margin(Margin::symmetric(10, 6))
                        .ui(ui);
                });
            });

            if !self.error.is_empty() {
                eframe::egui::Panel::bottom("error").show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::RED, format!("Error: {}", &self.error));
                    });
                });
            }

            CentralPanel::default().show_inside(ui, |ui| {
                if let Some(ref mut frontend) = self.frontend {
                    let connected = frontend.initialize_websocket(ui);
                    if !connected{ 
                        if let Some(url) = &self.url{
                            // std::thread::sleep(Duration::from_secs(10));
                            info!("websockets -> Trying to reconnect");
                            self.make_ws_connection(&url.to_string(), ui.ctx().clone(), self.client_uuid.clone());
                        }
                    }
                }
            });
        }
    }

    pub fn connect(&mut self, ctx: Context) {
        let client_hash = generate_client_id(
            self.computer_data.hostname.clone(), 
            self.computer_data.cpu.trim().to_string()
        );

        let computer_id = &self.computer_data.id.clone();

        info!("websockets -> self.client_uuid.clone(): {:?}", self.client_uuid.clone());

        let cust_id = if self.customer_data.name.is_empty() {
            Some(self.customer_data.id.clone())
        } else {
            None
        };

        let connected_client = ConnectedClient {
            id: self.client_uuid.clone(),
            client_hash,
            connected: false,
            assigned_user: Some(self.shared_ctx.current_user.as_ref().cloned().unwrap_or_default().get_id()),
            connection_string: self.client_title.clone(),
            customer: cust_id,
            computer: Some(computer_id.clone()),
            ..Default::default()
        };

        let uuid = self.client_uuid.clone();

        info!("websockets -> uuid: {:?}", connected_client.id.clone());

        spawn(async move {
            let query_id = query_id::<ConnectedClient>(CONNECTED_CLIENT_TABLE.to_string(), uuid.clone()).await?;
            info!("websockets -> query_id: {query_id:?}");
            let res: Option<Record> = DATABASE
                .create(uuid.clone())
                .content::<ConnectedClient>(connected_client)
                .await?;

            info!("websockets -> Upsert: {res:?}");
            Ok::<(), Error>(())
        });

        if let Some(url) = &self.url{
            self.make_ws_connection(&url.to_string(), ctx, self.client_uuid.clone());
        }
    }

    pub fn make_ws_connection(&mut self, url: &String, ctx: Context, client_id: RecordId) {
        info!("websockets -> self.url: {}", url.clone());
        let ctx = ctx.clone();
        let wakeup = move || ctx.request_repaint(); // wake up UI thread on new message

        match ewebsock::connect_with_wakeup(url, Default::default(), wakeup) {
            Ok((mut ws_sender, ws_receiver)) => {
                info!("websockets -> Connected to websocket server");
                ws_sender.send(ewebsock::WsMessage::Text("Client Connected!".to_string()));
                
                if self.frontend.is_none() {
                    self.frontend = Some(WebConsoleFrontend::new(
                        ws_sender,
                        ws_receiver,
                        self.egui_input_tx.clone(),
                    ));
                }

                spawn(async move {
                    let _update_client = DATABASE.query("UPDATE $client SET connected = true, last_update = time::now()")
                        .bind(("client", client_id.clone()))
                        .await?;

                    info!("websockets -> update_client: {_update_client:?}");
                    Ok::<(), Error>(())
                });

                self.error.clear();
            }
            Err(error) => {
                info!("websockets -> Failed to connect to {:?}: {}", &self.url, error);
                spawn(async move {
                    let _update_client = DATABASE.query("UPDATE $client SET connected = false, last_update = time::now()")
                        .bind(("client", client_id.clone()))
                        .await?;

                    info!("websockets -> update_client: {_update_client:?}");
                    Ok::<(), Error>(())
                });
                self.error = error;
            }
        };
    }
}
pub struct WebConsoleFrontend {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    pub egui_input_tx: Option<Sender<EguiInputEvent>>,

    pub tx: Sender<Vec<u8>>,
    pub rx: Receiver<Vec<u8>>,
    pub command_tx: Sender<Vec<u8>>,
    pub command_rx: Receiver<Vec<u8>>,
    pub interactive_input: (Sender<String>, Receiver<String>),

    pub events: Vec<WsEvent>,
    pub input: String,
    pub messages: Vec<String>,
    pub command: Cmd,
    pub send_specs: bool,
    pub history: Vec<String>,
    pub connected: bool,
    pub timeout_counter: Instant,
    pub process: Arc<Mutex<Option<ChildStdin>>>,
    pub explorer: FileSystem, 
    live_stats: bool,
    file_transfer_buffers: std::collections::HashMap<String, (u32, Vec<(u32, Vec<u8>)>)>,
    /// Pending remote plugin tool calls awaiting PluginManager response: (request_id, plugin_id, tool_name)
    pending_plugin_calls: Vec<(String, String, String)>,
}

impl WebConsoleFrontend {
    pub fn new(
        ws_sender: WsSender,
        ws_receiver: WsReceiver,
        egui_input_tx: Option<Sender<EguiInputEvent>>,
    ) -> Self {
        let (tx, rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let (command_tx, command_rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let interactive_input = String::create_unbounded_channel();
        
        Self {
            ws_sender, ws_receiver, egui_input_tx,
            command_tx, command_rx,
            tx, rx,
            events: Default::default(),
            input: String::new(),
            messages: Vec::new(),
            command: Cmd::None,
            history: Vec::new(),
            send_specs: false,
            connected: true,
            timeout_counter: Instant::now(),
            process: Arc::new(Mutex::new(None)),
            explorer: FileSystem::new(),
            interactive_input,
            live_stats: false,
            file_transfer_buffers: std::collections::HashMap::new(),
            pending_plugin_calls: Vec::new(),
        }
    }

    pub fn receive(&mut self) -> bool{
        let mut connected = true;

        while let Some(event) = self.ws_receiver.try_recv() { self.events.push(event); }
        
        if let Ok(sysinfo) = &mut self.rx.try_recv(){
            match compress_data(sysinfo.as_slice()) {
                Ok(mut compressed) => {
                    info!("Compressed data: {}\nOriginal: {}", compressed.len(), sysinfo.len());
                    self.ws_sender.send(WsMessage::Binary(std::mem::take(&mut compressed)));
                },
                Err(e) => log::error!("Error compressing data: {e:?}"),
            }
        }
        
        while let Ok(cmd_output) = &mut self.command_rx.try_recv() {
            self.ws_sender.send(WsMessage::Binary(std::mem::take(cmd_output)));
        }

        // Drain pending remote plugin tool call results (non-blocking; avoids main-thread deadlock)
        if !self.pending_plugin_calls.is_empty() {
            let result_rx = displays::plugins::remote_tool_result_receiver();
            let mut resolved = Vec::new();
            while let Ok((rid, success, rjson)) = result_rx.try_recv() {
                if let Some(pos) = self.pending_plugin_calls.iter().position(|(id, _, _)| id == &rid) {
                    let (request_id, plugin_id, tool_name) = self.pending_plugin_calls.remove(pos);
                    let result_cmd = displays::Cmd::RemotePluginToolResult {
                        request_id,
                        plugin_id,
                        tool_name,
                        success,
                        result_json: rjson,
                    };
                    if let Ok(payload) = bincode::serde::encode_to_vec(&result_cmd, bincode::config::standard()) {
                        self.ws_sender.send(ewebsock::WsMessage::Binary(payload));
                    }
                    resolved.push(rid);
                }
            }
        }

        // if self.timeout_counter.elapsed().as_secs() > 10 { info!("websockets -> Its been over 10 seconds since last ping"); }

        for event in self.events.clone() {
            match event{
                WsEvent::Message(msg) => {
                    connected = true;
                    match msg{
                        WsMessage::Binary(bin) => {
                            if bin.first().copied() == Some(EGUI_INPUT_TAG) {
                                match &self.egui_input_tx {
                                    None => {
                                        error!(
                                            target: "egui_remote",
                                            "[client_ws] EGUI_INPUT binary ({} bytes) but egui_input_tx is None — frame capture bridge not wired",
                                            bin.len()
                                        );
                                    }
                                    Some(tx) => {
                                        match bincode::serde::decode_from_slice::<EguiInputEvent, _>(
                                            &bin[1..],
                                            standard(),
                                        ) {
                                            Ok((ev, _)) => {
                                                let loud = matches!(
                                                    &ev,
                                                    EguiInputEvent::PointerButton { .. }
                                                        | EguiInputEvent::PointerLeave
                                                        | EguiInputEvent::Scroll { .. }
                                                        | EguiInputEvent::Key { .. }
                                                        | EguiInputEvent::Text(_)
                                                );
                                                if loud {
                                                    error!(
                                                        target: "egui_remote",
                                                        "[client_ws] decoded + enqueue: {ev:?}"
                                                    );
                                                } else {
                                                    log::debug!(
                                                        target: "egui_remote",
                                                        "[client_ws] decoded PointerMoved (enqueue)"
                                                    );
                                                }
                                                if let Err(e) = tx.try_send(ev) {
                                                    error!(
                                                        target: "egui_remote",
                                                        "[client_ws] try_send to egui_input_tx failed (channel full?): {e}"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                error!(
                                                    target: "egui_remote",
                                                    "[client_ws] bincode decode EguiInputEvent failed: {e} (payload {} bytes)",
                                                    bin.len().saturating_sub(1)
                                                );
                                            }
                                        }
                                    }
                                }
                                continue;
                            }
                            match decode_from_slice::<Cmd, _>(&bin, standard()) {
                                Ok((cmd, _)) => {
                                    self.history.push(format!("{cmd:?}"));
                                    info!("websockets -> Binary Message: {cmd:?}");
                                    self.handle_command(cmd);
                                }
                                Err(e) => {
                                    log::debug!("websockets -> Ignoring non-Cmd binary ({} bytes): {e}", bin.len());
                                }
                            }
                        },
                        WsMessage::Text(txt) => {
                            info!("websockets -> Got txt from websocket connection: {:?}", txt.clone());
                            self.history.push(format!("Raw Command: {}", txt.clone()));
                            let tx = self.command_tx.clone();
                            let text = txt.clone();
                            // let process = Arc::clone(&self.process);
                            spawn(async move {
                                let input_tx: std::result::Result<tokio::sync::mpsc::Sender<String>, Error>  = handle_command_payload(text.clone(), tx.clone()).await;
                                match input_tx {
                                    Ok(_tx) => { },
                                    Err(e) => log::warn!("Error with command payload: {e:?}"),
                                }
                                // process_command(text.clone(), tx.clone(), process).await;
                            });
                        },
                        _ => ()
                    }
                },
                WsEvent::Opened => {
                    connected = true;
                    self.history.push("Connection Opened".to_string())
                },
                WsEvent::Closed => {
                    connected = false;
                    self.history.push("Connection Closed".to_string())
                },
                WsEvent::Error(e) => {
                    connected = false;
                    info!("websockets -> {e:?}");
                    self.history.push(e.clone())
                },
            }
        }
        self.events.clear();
        connected
    }

    fn handle_command(&mut self, cmd: Cmd) {
        match cmd{
            Cmd::LiveData => {
                if self.live_stats == true {
                    
                } else {
                    self.live_stats = true;
                    let tx = self.tx.clone();
                    self.history.push(format!("Cmd: {:?}", cmd));
                    let connected = self.connected.clone();
                    spawn(async move { 
                        match live_computer_stats(tx.clone(), connected).await{
                            Ok(_) => drop(tx),
                            Err(e) => error!("Error with live data {e:?}"),
                        }
                    });
                }
            },
            Cmd::FileSystemAction(FileSystemAction::RequestNewContents(new_path)) => {
                let path = if new_path == "current" {
                    let current_path = env::current_dir().unwrap_or_default();
                    info!("websockets -> Current_path: {current_path:?}");
                    current_path
                } else {
                    Path::new(&new_path).to_path_buf()
                };
                if path.is_dir() {
                    let paths = read_folder(&path, 1, false);
                    // info!("websockets -> Paths: {:?}", paths.clone());
                    if paths.len() > 0 {
                        let node = self.explorer.build_virtual_file_system(path, paths);
                        info!("websockets -> Node: {:?}", node);
    
                        let payload = encode_to_vec(
                            &Cmd::FileSystemAction(FileSystemAction::GetNode(node)),
                            standard()
                        );
        
                        match payload {
                            Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                            Err(e) => error!("Error serializing paths: {e:?}"),
                        }
                    }
                } else { self.ws_sender.send(WsMessage::Text(format!("{new_path} is not a directory"))); }
            },
            Cmd::FileSystemAction(FileSystemAction::Execute(path)) => {
                let tx = self.tx.clone();
                let p = path.clone();
                let interactive_rx = self.interactive_input.1.clone();
                info!("websockets -> executing: {path:?}");
                spawn(async move {
                    let x = handle_windows_cmd_interactive(p, tx, interactive_rx).await;
                    info!("websockets -> x: {x:?}");
                });
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
                            Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                            Err(e) => error!("Error serializing paths: {e:?}"),
                        };
                    },
                    Err(e) => {let _ = self.tx.try_send(format!("Error with file preview: {e:?}").as_bytes().to_vec());},
                };
            }
            Cmd::FileSystemAction(FileSystemAction::Delete(path)) => {
                let tx = self.tx.clone();
                info!("websockets -> deleting: {path:?}");
                spawn(async move {
                    let path = Path::new(&path);
                    if !path.is_dir() {
                        let remove_dir = tokio::fs::remove_dir_all(path).await;
                        match remove_dir {
                            Ok(_) => tx.try_send("Removed Directory".as_bytes().to_vec()),
                            Err(e) => tx.try_send(format!("Error removing path: {e:?}").as_bytes().to_vec()),
                        }
                    } else {
                        let remove_file = tokio::fs::remove_file(path).await;
                        match remove_file {
                            Ok(_) => tx.try_send("Removed Path".as_bytes().to_vec()),
                            Err(e) => tx.try_send(format!("Error removing path: {e:?}").as_bytes().to_vec()),
                        }
                    }
                });
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
                let tx = self.interactive_input.0.clone();
                std::thread::spawn(move || {
                    tx.send(cmd).unwrap();
                });
            },
            Cmd::ReadEvents => {
                
            },
            Cmd::QuitInteractive => {
                let _ = self.interactive_input.0.try_send("quit".to_string());
            },
            Cmd::Quit => { self.connected = false; }
            Cmd::LoadWasmPlugin { plugin_id, wasm_bytes } => {
                let size = wasm_bytes.len();
                log::info!("Received remote WASM plugin '{plugin_id}' ({size} bytes) via egui WS");
                let tx = displays::plugins::wasm_load_sender();
                let _ = tx.try_send((plugin_id, wasm_bytes));
            }
            Cmd::SetFrameCapture { enabled } => {
                log::info!("SetFrameCapture received via egui WS: enabled={enabled}");
                let tx = displays::plugins::frame_capture_sender();
                let _ = tx.try_send(enabled);
            }
            Cmd::CallRemotePluginTool { request_id, plugin_id, tool_name, args_json } => {
                log::info!("CallRemotePluginTool via egui WS: {plugin_id}::{tool_name} req={request_id}");
                let call_tx = displays::plugins::remote_tool_call_sender();
                let _ = call_tx.try_send((request_id.clone(), plugin_id.clone(), tool_name.clone(), args_json));
                // Store as pending; result is flushed non-blockingly each receive() frame
                self.pending_plugin_calls.push((request_id, plugin_id, tool_name));
            }
            Cmd::DirectFileTransfer { filename, chunk_index, total_chunks, data } => {
                log::info!("DirectFileTransfer via egui WS: {filename} chunk {chunk_index}/{total_chunks} ({} bytes)", data.len());
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
                    if let Ok(payload) = bincode::serde::encode_to_vec(&result_cmd, bincode::config::standard()) {
                        self.ws_sender.send(ewebsock::WsMessage::Binary(payload));
                    }
                }
            }
            Cmd::ListDirectory(path_str) => {
                log::info!("websockets -> Listing directory: {}", path_str);
                let target_path = if path_str == "current" {
                    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
                } else {
                    #[cfg(target_os = "windows")]
                    let expanded = {
                        let s = path_str.clone();
                        // Expand basic env vars like %USERPROFILE%
                        let mut result = s.clone();
                        for (key, val) in std::env::vars() {
                            let pattern = format!("%{}%", key);
                            if result.to_lowercase().contains(&pattern.to_lowercase()) {
                                result = result.replace(&pattern, &val);
                            }
                        }
                        result
                    };
                    #[cfg(not(target_os = "windows"))]
                    let expanded = path_str.clone();
                    Path::new(&expanded).to_path_buf()
                };

                let mut entries: Vec<displays::RemoteDirEntry> = Vec::new();
                let resolved_path = target_path.to_string_lossy().to_string();

                if target_path.is_dir() {
                    if let Ok(dir_iter) = std::fs::read_dir(&target_path) {
                        for entry in dir_iter.flatten() {
                            let path = entry.path();
                            let name = entry.file_name().to_string_lossy().to_string();
                            let is_directory = path.is_dir();
                            let size = if is_directory { None } else { entry.metadata().ok().map(|m| m.len()) };
                            let modified = entry.metadata().ok()
                                .and_then(|m| m.modified().ok())
                                .map(|t| {
                                    let datetime: chrono::DateTime<chrono::Local> = t.into();
                                    datetime.to_rfc3339()
                                });
                            entries.push(displays::RemoteDirEntry {
                                name, path: path.to_string_lossy().to_string(),
                                is_directory, size, modified,
                            });
                        }
                    }
                }

                let response = Cmd::DirectoryListing(entries, Some(resolved_path));
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    self.ws_sender.send(WsMessage::Binary(payload));
                }
            }
            Cmd::GetDrives => {
                log::info!("websockets -> Getting drives");
                use sysinfo::Disks;
                let disks = Disks::new_with_refreshed_list();
                let drives: Vec<String> = disks.iter()
                    .filter_map(|disk| disk.mount_point().to_str().map(|s| s.to_string()))
                    .collect();
                let response = Cmd::DriveList(drives);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    self.ws_sender.send(WsMessage::Binary(payload));
                }
            }
            Cmd::DownloadRemoteFile(path_str) => {
                log::info!("websockets -> Download request for: {}", path_str);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let path = Path::new(&path_str);
                    if !path.is_file() {
                        return;
                    }
                    let metadata = match std::fs::metadata(path) {
                        Ok(m) => m,
                        Err(_) => return,
                    };
                    if metadata.len() > 100 * 1024 * 1024 { return; }

                    if let Ok(data) = std::fs::read(path) {
                        const CHUNK_SIZE: usize = 4 * 1024 * 1024;
                        if data.len() > CHUNK_SIZE {
                            let chunks: Vec<&[u8]> = data.chunks(CHUNK_SIZE).collect();
                            let total_chunks = chunks.len();
                            for (i, chunk) in chunks.into_iter().enumerate() {
                                let is_last = i == total_chunks - 1;
                                let response = Cmd::FileChunk(chunk.to_vec(), is_last);
                                if let Ok(payload) = encode_to_vec(&response, standard()) {
                                    let _ = tx.try_send(payload);
                                }
                            }
                        } else {
                            let response = Cmd::FileChunk(data, true);
                            if let Ok(payload) = encode_to_vec(&response, standard()) {
                                let _ = tx.try_send(payload);
                            }
                        }
                    }
                });
            }
            Cmd::ExecuteRemoteFile(path_str) => {
                log::info!("websockets -> Execute request for: {}", path_str);
                #[cfg(target_os = "windows")]
                { let _ = std::process::Command::new("cmd").args(["/c", "start", "", &path_str]).spawn(); }
                #[cfg(target_os = "macos")]
                { let _ = std::process::Command::new("open").arg(&path_str).spawn(); }
                #[cfg(target_os = "linux")]
                { let _ = std::process::Command::new("xdg-open").arg(&path_str).spawn(); }
            }
            Cmd::PreviewRemoteFile(path_str) => {
                log::info!("websockets -> Preview request for: {}", path_str);
                let tx = self.command_tx.clone();
                let ps = path_str.clone();
                spawn(async move {
                    let path = Path::new(&ps);
                    if !path.is_file() { return; }
                    if let Ok(meta) = std::fs::metadata(path) {
                        if meta.len() > 5 * 1024 * 1024 { return; }
                    }
                    let content = std::fs::read_to_string(path)
                        .unwrap_or_else(|_| std::fs::read(path).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default());
                    let response = Cmd::FilePreviewContent(ps, content);
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::SaveRemoteFile(path_str, content) => {
                log::info!("websockets -> Save file request: {}", path_str);
                let (success, message) = match std::fs::write(&path_str, &content) {
                    Ok(_) => (true, format!("File saved: {}", path_str)),
                    Err(e) => (false, format!("Failed to save: {}", e)),
                };
                let response = Cmd::SaveResult(success, message);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    self.ws_sender.send(WsMessage::Binary(payload));
                }
            }
            Cmd::UploadToClient(dest_path, data) => {
                log::info!("websockets -> Upload to client: {} ({} bytes)", dest_path, data.len());
                let (success, message) = match std::fs::write(&dest_path, &data) {
                    Ok(_) => (true, format!("File saved: {}", dest_path)),
                    Err(e) => (false, format!("Failed to save: {}", e)),
                };
                let response = Cmd::SaveResult(success, message);
                if let Ok(payload) = encode_to_vec(&response, standard()) {
                    self.ws_sender.send(WsMessage::Binary(payload));
                }
            }
            Cmd::RequestThumbnail(path_str) => {
                log::info!("websockets -> Thumbnail request for: {}", path_str);
                let tx = self.command_tx.clone();
                spawn(async move {
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
                        unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); }
                        let path = Path::new(&path_str);
                        let result: Result<Vec<u8>, String> = (|| -> Result<Vec<u8>, String> {
                            unsafe {
                                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                                let shell_item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>)
                                    .map_err(|e| format!("SHCreateItemFromParsingName: {e}"))?;
                                let factory: IShellItemImageFactory = shell_item.cast().map_err(|e| format!("cast: {e}"))?;
                                let hbmp: HBITMAP = factory.GetImage(SIZE { cx: 256, cy: 256 }, SIIGBF(0)).map_err(|e| format!("GetImage: {e}"))?;
                                crate::terminal_mode::websockets::hbitmap_to_png_bytes(hbmp)
                            }
                        })();
                        if let Ok(png_bytes) = result {
                            let response = Cmd::ThumbnailResponse(path_str, png_bytes);
                            if let Ok(payload) = encode_to_vec(&response, standard()) {
                                let _ = tx.try_send(payload);
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        if let Ok(img) = image::open(&path_str) {
                            let thumb = img.thumbnail(256, 256);
                            let mut buf = Vec::new();
                            if thumb.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).is_ok() {
                                let response = Cmd::ThumbnailResponse(path_str, buf);
                                if let Ok(payload) = encode_to_vec(&response, standard()) {
                                    let _ = tx.try_send(payload);
                                }
                            }
                        }
                    }
                });
            }
            Cmd::RebootSystem { persist_mastertech, terminal_mode } => {
                log::info!("websockets -> Reboot system command received (persist={})", persist_mastertech);
                spawn(async move {
                    #[cfg(target_os = "windows")]
                    {
                        if persist_mastertech {
                            let exe_path = std::env::current_exe().unwrap_or_default();
                            let command = if terminal_mode {
                                format!("\"{}\" -t", exe_path.to_string_lossy())
                            } else {
                                format!("\"{}\"", exe_path.to_string_lossy())
                            };
                            let _ = tokio::process::Command::new("schtasks")
                                .args(["/Create", "/TN", "MastertechAutoRestart", "/TR", &command, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F"])
                                .output().await;
                        }
                        let _ = tokio::process::Command::new("shutdown")
                            .args(["/r", "/t", "5", "/c", "Mastertech remote reboot requested"])
                            .output().await;
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let _ = tokio::process::Command::new("sudo").args(["shutdown", "-r", "+1"]).output().await;
                    }
                });
            }
            Cmd::ShutdownSystem => {
                log::info!("websockets -> Shutdown system command received");
                spawn(async move {
                    #[cfg(target_os = "windows")]
                    { let _ = tokio::process::Command::new("shutdown").args(["/s", "/t", "5", "/c", "Mastertech remote shutdown"]).output().await; }
                    #[cfg(not(target_os = "windows"))]
                    { let _ = tokio::process::Command::new("sudo").args(["shutdown", "-h", "+1"]).output().await; }
                });
            }
            Cmd::LockWorkstation => {
                log::info!("websockets -> Lock workstation command received");
                spawn(async move {
                    #[cfg(target_os = "windows")]
                    { let _ = tokio::process::Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]).output().await; }
                    #[cfg(target_os = "linux")]
                    { let _ = tokio::process::Command::new("loginctl").args(["lock-session"]).output().await; }
                    #[cfg(target_os = "macos")]
                    { let _ = tokio::process::Command::new("pmset").args(["displaysleepnow"]).output().await; }
                });
            }
            Cmd::LogOffUser => {
                log::info!("websockets -> Log off user command received");
                spawn(async move {
                    #[cfg(target_os = "windows")]
                    { let _ = tokio::process::Command::new("shutdown").args(["/l"]).output().await; }
                    #[cfg(not(target_os = "windows"))]
                    { let _ = tokio::process::Command::new("pkill").args(["-KILL", "-u", &whoami::username().unwrap_or_default()]).output().await; }
                });
            }
            Cmd::KillProcess(pid) => {
                log::info!("websockets -> Killing process {}", pid);
                spawn(async move {
                    #[cfg(target_os = "windows")]
                    { let _ = tokio::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output().await; }
                    #[cfg(not(target_os = "windows"))]
                    { let _ = tokio::process::Command::new("kill").args(["-9", &pid.to_string()]).output().await; }
                });
            }
            Cmd::OpenProcessInExplorer(path_str) => {
                log::info!("websockets -> Opening process location: {}", path_str);
                #[cfg(target_os = "windows")]
                {
                    let target_path = Path::new(&path_str);
                    if target_path.exists() {
                        let dir_path = if target_path.is_file() {
                            target_path.parent().unwrap_or(target_path).to_path_buf()
                        } else {
                            target_path.to_path_buf()
                        };
                        let _ = std::process::Command::new("explorer.exe").arg(dir_path).spawn();
                    }
                }
            }
            Cmd::ReadEventLog { log_name, max_entries, level_filter } => {
                log::info!("websockets -> Reading event log: {} (max: {}, filter: {:?})", log_name, max_entries, level_filter);
                let tx = self.command_tx.clone();
                spawn(async move {
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
                        .output().await;

                    let mut entries = Vec::new();
                    if let Ok(out) = output {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                            for obj in json_array {
                                entries.push(displays::EventLogEntry {
                                    level: obj.get("LevelDisplayName").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                    time: obj.get("TimeCreated").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    source: obj.get("ProviderName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    event_id: obj.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                    message: obj.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                });
                            }
                        } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                            entries.push(displays::EventLogEntry {
                                level: obj.get("LevelDisplayName").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                time: obj.get("TimeCreated").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                source: obj.get("ProviderName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                event_id: obj.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                message: obj.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            });
                        }
                    }

                    let response = Cmd::EventLogResponse(entries);
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ListServices => {
                log::info!("websockets -> Listing services");
                let tx = self.command_tx.clone();
                spawn(async move {
                    let ps_cmd = "Get-CimInstance Win32_Service | Select-Object Name,DisplayName,State,StartMode,ProcessId | ConvertTo-Json -Compress";
                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", ps_cmd])
                        .output().await;

                    let mut services = Vec::new();
                    if let Ok(out) = output {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                            for obj in json_array {
                                services.push(displays::WindowsService {
                                    name: obj.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    display_name: obj.get("DisplayName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    status: obj.get("State").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                    start_type: obj.get("StartMode").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                    pid: obj.get("ProcessId").and_then(|v| v.as_u64()).map(|p| p as u32),
                                });
                            }
                        }
                    }

                    let response = Cmd::ServiceListResponse(services);
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ControlService { name, action } => {
                log::info!("websockets -> Service control: {} - {:?}", name, action);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let ps_cmd = match &action {
                        displays::ServiceActionType::Start => format!("Start-Service -Name '{}' -ErrorAction Stop; 'OK'", name),
                        displays::ServiceActionType::Stop => format!("Stop-Service -Name '{}' -Force -ErrorAction Stop; 'OK'", name),
                        displays::ServiceActionType::Restart => format!("Restart-Service -Name '{}' -Force -ErrorAction Stop; 'OK'", name),
                        displays::ServiceActionType::SetStartType(start_type) => format!("Set-Service -Name '{}' -StartupType '{}' -ErrorAction Stop; 'OK'", name, start_type),
                    };
                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output().await;

                    let (success, message) = match output {
                        Ok(out) if out.status.success() => (true, format!("Action completed: {:?}", action)),
                        Ok(out) => (false, String::from_utf8_lossy(&out.stderr).to_string()),
                        Err(e) => (false, format!("Failed to execute: {}", e)),
                    };

                    let response = Cmd::ServiceActionResponse { name, success, message };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ListScheduledTasks { folder } => {
                log::info!("websockets -> Listing scheduled tasks (folder: {:?})", folder);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let folder_filter = folder.as_deref().unwrap_or("\\");
                    let ps_cmd = format!(
                        r#"$tasks = Get-ScheduledTask -TaskPath '{}*' -ErrorAction SilentlyContinue; $results = @(); foreach($t in $tasks) {{ $info = $null; try {{ $info = Get-ScheduledTaskInfo -TaskName $t.TaskName -TaskPath $t.TaskPath -ErrorAction SilentlyContinue }} catch {{}}; $triggers = @(); foreach($tr in $t.Triggers) {{ $triggers += $tr.CimClass.CimClassName }}; $actions = @(); foreach($a in $t.Actions) {{ $actions += $a.Execute }}; $results += @{{ Name=$t.TaskName; Path=$t.TaskPath; State=$t.State.ToString(); LastRun=if($info){{$info.LastRunTime.ToString('o')}}else{{'Never'}}; NextRun=if($info){{$info.NextRunTime.ToString('o')}}else{{'N/A'}}; Description=$t.Description; Triggers=$triggers; Actions=$actions }} }}; $results | ConvertTo-Json -Compress -Depth 3"#,
                        folder_filter
                    );
                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output().await;

                    let mut tasks = Vec::new();
                    if let Ok(out) = output {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let parse_task = |obj: &serde_json::Value| -> displays::ScheduledTask {
                            displays::ScheduledTask {
                                name: obj.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                path: obj.get("Path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                state: obj.get("State").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                last_run: obj.get("LastRun").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                next_run: obj.get("NextRun").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                description: obj.get("Description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                triggers: obj.get("Triggers").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect()).unwrap_or_default(),
                                actions: obj.get("Actions").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|a| a.as_str().map(|s| s.to_string())).collect()).unwrap_or_default(),
                            }
                        };
                        if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                            for obj in &json_array { tasks.push(parse_task(obj)); }
                        } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                            tasks.push(parse_task(&obj));
                        }
                    }

                    let response = Cmd::ScheduledTaskListResponse(tasks);
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ToggleScheduledTask { path, enable } => {
                log::info!("websockets -> {} task: {}", if enable { "Enable" } else { "Disable" }, path);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let ps_cmd = if enable {
                        format!("Enable-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path)
                    } else {
                        format!("Disable-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path)
                    };
                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output().await;

                    let (success, message) = match output {
                        Ok(out) if out.status.success() => (true, format!("Task {}", if enable { "enabled" } else { "disabled" })),
                        Ok(out) => (false, String::from_utf8_lossy(&out.stderr).to_string()),
                        Err(e) => (false, format!("Failed: {}", e)),
                    };
                    let response = Cmd::ScheduledTaskActionResponse { success, message };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::RunScheduledTask(path) => {
                log::info!("websockets -> Running task: {}", path);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let ps_cmd = format!("Start-ScheduledTask -TaskName '{}' -ErrorAction Stop; 'OK'", path);
                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output().await;

                    let (success, message) = match output {
                        Ok(out) if out.status.success() => (true, "Task started".to_string()),
                        Ok(out) => (false, String::from_utf8_lossy(&out.stderr).to_string()),
                        Err(e) => (false, format!("Failed: {}", e)),
                    };
                    let response = Cmd::ScheduledTaskActionResponse { success, message };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ListRegistryKeys(path) => {
                log::info!("websockets -> Listing registry keys: {}", path);
                let tx = self.command_tx.clone();
                let p = path.clone();
                spawn(async move {
                    let ps_cmd = format!(
                        r#"$subkeys = @(); $values = @(); try {{ Get-ChildItem -Path 'Registry::{p}' -ErrorAction Stop | ForEach-Object {{ $subkeys += @{{ Name=$_.PSChildName; Path=$_.Name; SubkeyCount=(Get-ChildItem -Path $_.PSPath -ErrorAction SilentlyContinue | Measure-Object).Count; ValueCount=(Get-ItemProperty -Path $_.PSPath -ErrorAction SilentlyContinue | Get-Member -MemberType NoteProperty | Where-Object {{ $_.Name -notmatch '^PS' }} | Measure-Object).Count }} }}; $props = Get-ItemProperty -Path 'Registry::{p}' -ErrorAction SilentlyContinue; if($props) {{ $props | Get-Member -MemberType NoteProperty | Where-Object {{ $_.Name -notmatch '^PS' }} | ForEach-Object {{ $n = $_.Name; $v = $props.$n; $kind = (Get-Item -Path 'Registry::{p}' -ErrorAction SilentlyContinue).GetValueKind($n); $values += @{{ Name=$n; Kind=$kind.ToString(); Data=[string]$v }} }} }} }} catch {{ }}; @{{ Subkeys=$subkeys; Values=$values }} | ConvertTo-Json -Compress -Depth 4"#
                    );
                    let output = tokio::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &ps_cmd])
                        .output().await;

                    let mut subkeys = Vec::new();
                    let mut values = Vec::new();
                    if let Ok(out) = output {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&stdout) {
                            if let Some(sk_arr) = obj.get("Subkeys").and_then(|v| v.as_array()) {
                                for sk in sk_arr {
                                    subkeys.push(displays::RegistryKeyInfo {
                                        name: sk.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                        path: sk.get("Path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                        subkey_count: sk.get("SubkeyCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                        value_count: sk.get("ValueCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                    });
                                }
                            }
                            if let Some(val_arr) = obj.get("Values").and_then(|v| v.as_array()) {
                                for val in val_arr {
                                    values.push(displays::RegistryValueEntry {
                                        name: val.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                        kind: val.get("Kind").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                                        data: val.get("Data").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    });
                                }
                            }
                        }
                    }

                    let response = Cmd::RegistryKeyResponse { path: p, subkeys, values };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::BackupRegistryKey(path) => {
                log::info!("websockets -> Backing up registry key: {}", path);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                    let backup_filename = format!("reg_backup_{}_{}.reg", path.replace('\\', "_").replace('/', "_"), timestamp);
                    let backup_dir = std::env::temp_dir().join("mastertech_reg_backups");
                    let _ = std::fs::create_dir_all(&backup_dir);
                    let backup_path = backup_dir.join(&backup_filename);
                    let backup_path_str = backup_path.to_string_lossy().to_string();

                    let output = tokio::process::Command::new("reg")
                        .args(["export", &path, &backup_path_str, "/y"])
                        .output().await;

                    let (success, message) = match output {
                        Ok(out) if out.status.success() => (true, format!("Backup saved to {}", backup_path_str)),
                        Ok(out) => (false, String::from_utf8_lossy(&out.stderr).to_string()),
                        Err(e) => (false, format!("Failed to backup: {}", e)),
                    };
                    let response = Cmd::RegistryBackupResponse { success, backup_path: backup_path_str, message };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::CommitRegistryEdits(edits) => {
                log::info!("websockets -> Committing {} registry edits", edits.len());
                let tx = self.command_tx.clone();
                spawn(async move {
                    let mut all_success = true;
                    let mut messages = Vec::new();
                    for edit in &edits {
                        let ps_cmd = match edit {
                            displays::RegistryEdit::SetValue { path, name, kind, data } => {
                                let reg_type = match kind.as_str() {
                                    "REG_DWORD" | "DWord" => "DWord",
                                    "REG_QWORD" | "QWord" => "QWord",
                                    "REG_BINARY" | "Binary" => "Binary",
                                    "REG_MULTI_SZ" | "MultiString" => "MultiString",
                                    "REG_EXPAND_SZ" | "ExpandString" => "ExpandString",
                                    _ => "String",
                                };
                                format!("Set-ItemProperty -Path 'Registry::{}' -Name '{}' -Value '{}' -Type {} -ErrorAction Stop; 'OK'", path, name, data, reg_type)
                            }
                            displays::RegistryEdit::DeleteValue { path, name } => {
                                format!("Remove-ItemProperty -Path 'Registry::{}' -Name '{}' -ErrorAction Stop; 'OK'", path, name)
                            }
                            displays::RegistryEdit::CreateKey { path } => {
                                format!("New-Item -Path 'Registry::{}' -Force -ErrorAction Stop | Out-Null; 'OK'", path)
                            }
                            displays::RegistryEdit::DeleteKey { path } => {
                                format!("Remove-Item -Path 'Registry::{}' -Recurse -Force -ErrorAction Stop; 'OK'", path)
                            }
                        };
                        let output = tokio::process::Command::new("powershell")
                            .args(["-NoProfile", "-Command", &ps_cmd])
                            .output().await;
                        match output {
                            Ok(out) if !out.status.success() => {
                                all_success = false;
                                messages.push(format!("Failed: {}", String::from_utf8_lossy(&out.stderr)));
                            }
                            Err(e) => { all_success = false; messages.push(format!("Error: {}", e)); }
                            _ => {}
                        }
                    }
                    let message = if all_success { format!("All {} edit(s) applied successfully", edits.len()) } else { messages.join("; ") };
                    let response = Cmd::RegistryEditResponse { success: all_success, message };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ListStartupApps => {
                log::info!("websockets -> ListStartupApps");
                let tx = self.command_tx.clone();
                spawn(async move {
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
                        .output().await;

                    let apps: Vec<displays::StartupApp> = match output {
                        Ok(out) if out.status.success() => {
                            let stdout = String::from_utf8_lossy(&out.stdout);
                            let trimmed = stdout.trim();
                            if trimmed.is_empty() || trimmed == "null" { Vec::new() }
                            else {
                                serde_json::from_str::<Vec<displays::StartupApp>>(trimmed)
                                    .or_else(|_| serde_json::from_str::<displays::StartupApp>(trimmed).map(|s| vec![s]))
                                    .unwrap_or_default()
                            }
                        }
                        _ => Vec::new(),
                    };

                    let response = Cmd::StartupAppsResponse(apps);
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::ToggleStartupApp { name, registry_path, enable } => {
                log::info!("websockets -> ToggleStartupApp: {} -> enable={}", name, enable);
                let tx = self.command_tx.clone();
                spawn(async move {
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
                        .output().await;

                    let (success, message) = match output {
                        Ok(out) if out.status.success() => (true, format!("'{}' {}", name, if enable { "enabled" } else { "disabled" })),
                        Ok(out) => (false, format!("Failed to toggle '{}': {}", name, String::from_utf8_lossy(&out.stderr))),
                        Err(e) => (false, format!("Error: {}", e)),
                    };
                    let response = Cmd::StartupAppActionResponse { success, message };
                    if let Ok(payload) = encode_to_vec(&response, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::GetRemoteScriptList => {
                log::info!("websockets -> GetRemoteScriptList");
                let builtin = |name: &str, cat: &str| displays::RemoteScriptItem {
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
                    self.ws_sender.send(WsMessage::Binary(payload));
                }
            }
            Cmd::RunRemoteScripts { scripts, service_number, customer_email: _ } => {
                log::info!("websockets -> RunRemoteScripts: {} scripts, SO={}", scripts.len(), service_number);
                let tx = self.command_tx.clone();
                spawn(async move {
                    let send_log = |tx: &Sender<Vec<u8>>, msg: String| {
                        let cmd = Cmd::RemoteScriptLog(msg);
                        if let Ok(payload) = encode_to_vec(&cmd, standard()) {
                            let _ = tx.try_send(payload);
                        }
                    };
                    let send_result = |tx: &Sender<Vec<u8>>, name: &str, status: displays::RemoteScriptStatus| {
                        let cmd = Cmd::RemoteScriptResult { name: name.to_string(), status };
                        if let Ok(payload) = encode_to_vec(&cmd, standard()) {
                            let _ = tx.try_send(payload);
                        }
                    };

                    for script in &scripts {
                        send_log(&tx, format!("Starting: {}", script.name));

                        if let Some(content) = &script.content {
                            send_log(&tx, format!("Running custom script: {}", script.name));
                            let ext = if script.name.ends_with(".bat") || script.name.ends_with(".cmd") { "bat" } else { "ps1" };
                            let script_file = std::env::temp_dir().join(format!("mastertech_custom_{}.{}", uuid::Uuid::new_v4(), ext));
                            if let Err(e) = std::fs::write(&script_file, content) {
                                send_log(&tx, format!("Failed to write script: {e}"));
                                send_result(&tx, &script.name, displays::RemoteScriptStatus::Failed);
                                continue;
                            }
                            let output = if ext == "ps1" {
                                tokio::process::Command::new("powershell")
                                    .args(["-ExecutionPolicy", "Bypass", "-File", &script_file.to_string_lossy()])
                                    .output().await
                            } else {
                                tokio::process::Command::new("cmd")
                                    .args(["/C", &script_file.to_string_lossy()])
                                    .output().await
                            };
                            let _ = std::fs::remove_file(&script_file);
                            match output {
                                Ok(out) => {
                                    let stdout = String::from_utf8_lossy(&out.stdout);
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    if !stdout.is_empty() { for line in stdout.lines() { send_log(&tx, line.to_string()); } }
                                    if !stderr.is_empty() { for line in stderr.lines() { send_log(&tx, format!("[stderr] {}", line)); } }
                                    if out.status.success() {
                                        send_result(&tx, &script.name, displays::RemoteScriptStatus::Success);
                                    } else {
                                        send_result(&tx, &script.name, displays::RemoteScriptStatus::Failed);
                                    }
                                }
                                Err(e) => {
                                    send_log(&tx, format!("Execution error: {e}"));
                                    send_result(&tx, &script.name, displays::RemoteScriptStatus::Failed);
                                }
                            }
                        } else {
                            send_log(&tx, format!("'{}' not yet implemented for remote execution in egui mode", script.name));
                            send_result(&tx, &script.name, displays::RemoteScriptStatus::Failed);
                        }
                    }

                    let complete = Cmd::RemoteScriptsComplete;
                    if let Ok(payload) = encode_to_vec(&complete, standard()) {
                        let _ = tx.try_send(payload);
                    }
                });
            }
            Cmd::RunScriptContent { filename, content } => {
                log::info!("RunScriptContent: filename={filename}");
                let tx = self.command_tx.clone();
                let fname = filename.clone();
                spawn(async move {
                    let send_log = |tx: &Sender<Vec<u8>>, msg: String| {
                        if let Ok(payload) = encode_to_vec(&Cmd::RemoteScriptLog(msg), standard()) {
                            let _ = tx.try_send(payload);
                        }
                    };
                    let ext = fname.rsplit('.').next().unwrap_or("").to_lowercase();
                    send_log(&tx, format!("Running script: {fname}"));
                    let output = match ext.as_str() {
                        "ps1" => std::process::Command::new("powershell").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &content]).output(),
                        "bat" | "cmd" => std::process::Command::new("cmd").args(["/C", &content]).output(),
                        _ => { send_log(&tx, format!("Unsupported script type: .{ext}")); return; }
                    };
                    match output {
                        Ok(out) => {
                            let stdout = String::from_utf8_lossy(&out.stdout);
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            if !stdout.is_empty() { send_log(&tx, stdout.to_string()); }
                            if !stderr.is_empty() { send_log(&tx, format!("[stderr] {stderr}")); }
                            send_log(&tx, format!("Script {fname} exited with code: {}", out.status));
                        }
                        Err(e) => { send_log(&tx, format!("Failed to run script {fname}: {e}")); }
                    }
                });
            }
            Cmd::None => {}
            _ => {
                log::debug!("websockets -> Unhandled command in egui mode: {:?}", std::mem::discriminant(&cmd));
            },
        }
    }

    pub fn initialize_websocket(&mut self, ui: &mut Ui) -> bool {
        self.connected = self.receive();
        let theme = CodeTheme::dark(12.);
        ScrollArea::vertical()
            .animated(true)
            .max_height(ui.available_height() - 5.0)
            .max_width(f32::INFINITY)
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| 
        {
            ui.set_width(ui.available_width());
            let max_msg_width = ui.available_width() - 15.0;
            let fixed_height = 50.0;
            let min_width = 200.0;
            let mut count = 0;
            for item in self.history.iter(){
                count += 1;
                let is_message_from_myself = if item.contains("You"){
                    true
                } else { false };

                // Messages from the user are right-aligned.
                let layout = if is_message_from_myself {
                    Layout::top_down(Align::Max)
                } else {
                    Layout::top_down(Align::Min)
                };

                let msg_color = if is_message_from_myself {
                    ui.style().visuals.widgets.inactive.bg_fill
                } else {
                    ui.style().visuals.widgets.active.weak_bg_fill
                };

                ui.with_layout(layout, |ui| {
                    ui.set_max_width(max_msg_width);

                    let rounding = 8.0;
                    let margin = 8.0;
                    
                    // ui.set_min_width(min_width);
                    let rnding = eframe::egui::CornerRadius {
                        ne: if is_message_from_myself { 0 } else { rounding as u8 },
                        nw: if is_message_from_myself { rounding as u8 } else { 0 },
                        se: rounding as u8,
                        sw: rounding as u8,
                    };

                    let response = Frame::new()
                        .corner_radius(rnding)
                        .inner_margin(margin)
                        .outer_margin(margin)
                        .fill(msg_color)
                        .show(ui, |ui| {
                            ui.set_min_height(fixed_height);  // Set the fixed height for the message box
                            ui.set_min_width(min_width / 2.5);
                            // Use a vertical layout to stack the name and message content
                            ui.with_layout(Layout::top_down(Align::Min), |ui| 
                            {

                                let mut shadow = Shadow::default();
                                shadow.blur = 3;
                                shadow.spread = 3;
                                shadow.color = Color32::from_rgb(40,36,40);
                                let color = Color32::from_rgb(10,10,12);

                                let note_frame = Frame::new().fill(color)
                                    .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke)
                                    .inner_margin(Margin::same(0)).corner_radius(rnding);

                                let (from, txt) = if item.contains("Cmd"){
                                    let text: (&str, &str) = item.split_once(":").unwrap_or(("Command", ""));
                                    let cmd = text.1;
                                    (
                                        RichText::new("Cmd").strong().monospace().color(Color32::LIGHT_BLUE),
                                        RichText::new(cmd).strong().monospace()
                                    )
                                } else if item.contains("Raw Command"){
                                    let text: (&str, &str) = item.split_once(":").unwrap_or(("Raw Command", ""));
                                    let cmd = text.1;
                                    (
                                        RichText::new("Raw Command").strong().monospace().color(Color32::LIGHT_BLUE),
                                        RichText::new(cmd).strong().monospace()
                                    )
                                }else if item.contains("You"){
                                    let text: (&str, &str) = item.split_once("\n").unwrap_or(("Raw Command", ""));
                                    let cmd = text.1;
                                    (
                                        RichText::new("You").strong().monospace().color(Color32::LIGHT_BLUE),
                                        RichText::new(cmd).strong().monospace()
                                    )
                                }else {
                                    (
                                        RichText::new("Raw Binary Payload").strong().monospace().color(Color32::LIGHT_BLUE),
                                        RichText::new(item).strong().monospace()
                                    )
                                };
                                

                                if is_message_from_myself {
                                    ui.with_layout(Layout::from_main_dir_and_cross_align(
                                        Direction::RightToLeft,
                                        Align::Min,
                                    ), |ui| {
                                        ui.add_space(8.0);
                                        Button::new(from).fill(Color32::TRANSPARENT).min_size(Vec2::new(30.0, 20.0)).sense(Sense::hover()).ui(ui);
                                        
                                    });
                                }else{
                                    ui.with_layout(Layout::from_main_dir_and_cross_align(
                                        Direction::LeftToRight,
                                        Align::Min,
                                    ), |ui| {
                                        ui.add_space(8.0);
                                        Button::new(from).fill(Color32::TRANSPARENT).min_size(Vec2::new(30.0, 20.0)).sense(Sense::hover()).ui(ui);
                                    });
                                }
                                note_frame.show(ui, |ui| {
                                    ui.with_layout(Layout::from_main_dir_and_cross_align(
                                        Direction::TopDown,
                                        Align::Center,
                                    ), |ui| {
                                        ui.set_width(ui.available_width());
                                        let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap_width: f32| {
                                            let mut layout_job: eframe::egui::text::LayoutJob =
                                                highlight(ui.ctx(), ui.style(), &CodeTheme::dark(12.), buf.as_str(), "bash".into()); // || "zsh".into()
                                            layout_job.wrap.max_width = wrap_width;
                                            ui.fonts_mut(|f| f.layout_job(layout_job))
                                        };
                                        TextEdit::singleline(&mut txt.text())
                                            .id_salt(Id::new(format!("{item:?}-{count:?}")))
                                            .frame(egui::Frame::NONE)
                                            .layouter(&mut layouter)
                                            .min_size(Vec2::new(ui.available_size_before_wrap().x / 1.1, 30.))
                                            .ui(ui);
                                    });
                                });
                        });
                    })
                    .response;

                    let points = if !is_message_from_myself {
                        let top = response.rect.left_top() + Vec2::splat(margin);
                        let arrow_rect =
                            Rect::from_two_pos(top, top + Vec2::new(-rounding, rounding));

                        vec![
                            arrow_rect.left_top(),
                            arrow_rect.right_top(),
                            arrow_rect.right_bottom(),
                        ]
                    } else {
                        let top = response.rect.right_top() + Vec2::new(-margin, margin);
                        let arrow_rect =
                            Rect::from_two_pos(top, top + Vec2::new(rounding, rounding));

                        vec![
                            arrow_rect.left_top(),
                            arrow_rect.right_top(),
                            arrow_rect.left_bottom(),
                        ]
                    };

                    ui.painter()
                        .add(Shape::convex_polygon(points, msg_color, Stroke::NONE));

                });
            };
        });

        ui.vertical_centered_justified(|ui| {
            // let mut theme = CodeTheme::from_memory(ui.ctx());
            // ui.collapsing("Theme", |ui| {
            //     ui.group(|ui| {
            //         theme.ui(ui);
            //         theme.clone().store_in_memory(ui.ctx());
            //     });
            // });
            
            let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap_width: f32| {
                let mut layout_job =
                    highlight(ui.ctx(), ui.style(), &theme, buf.as_str(), "bash".into()); // || "zsh".into()
                layout_job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(layout_job))
            };

            let text_edit = TextEdit::singleline(&mut self.input).hint_text("Send Message").layouter(&mut layouter).ui(ui);
            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            if text_edit.lost_focus() && key_press {
                text_edit.request_focus();
                self.history.push(format!("You\n{}", self.input.clone()));
                self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));
            }
        });

        self.connected  
    }
}

async fn live_computer_stats(tx: Sender<Vec<u8>>, connected: bool) -> Result<(), Error>{
    while connected {
        let systeminfo: SystemInformation = get_sysinfo().await?;
        sleep(Duration::from_secs_f32(0.4)).await;
        info!("websockets -> {systeminfo:?}");
        tx.try_send(serialize_system_info(&systeminfo))?;
    }
    #[allow(unreachable_code)]
    Ok(())
}

async fn handle_command_payload(string_payload: String, tx: Sender<Vec<u8>>) -> Result<tokio::sync::mpsc::Sender<String>, Error>  { 
    // #[cfg(target_os="windows")]{ return handle_windows_cmd(string_payload, tx.clone()).await?; }
    if cfg!(target_os="windows") { 
        let _ = handle_windows_cmd(string_payload.clone(), tx.clone()).await?;
    }
    Ok(handle_linux_cmd(string_payload, tx.clone()).await?)
}

async fn handle_windows_cmd(command_payload: String, tx: Sender<Vec<u8>>) -> Result<ChildStdin, Error> {
    use tokio::{process::{Child, ChildStdin}, time::Instant};

    let start = Instant::now();
    info!("websockets -> Executing command: {}", command_payload);
    let mut process: Child = Command::new("cmd")
        .arg("/C")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Create a Tokio stream for stdout
    let mut stdout = process.stdout.take().expect("Failed to get stdout");
    // Create a Tokio stream for stderr
    let mut stderr = process.stderr.take().expect("Failed to get stderr");
    let stdin: ChildStdin = process.stdin.take().expect("Failed to open stdin");
    
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
    info!("websockets -> output: {:?}", output);
    let duration = start.elapsed();
    info!("websockets -> Command executed in {:?}", duration);
    let tx_clone = tx.clone();
    if !output.status.success() {
        info!("websockets -> output status not successfull");
        tx_clone.send(output.stderr).ok();
    }

    Ok(stdin)
}

async fn handle_windows_cmd_interactive(
    command_payload: String, 
    tx: Sender<Vec<u8>>,
    rx: Receiver<String>
) ->  Result<(), Error> {

    let mut process: Child = Command::new("cmd")
        .arg("/C")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Create a Tokio stream for stdout / stderr
    let stdout = process.stdout.take().expect("Failed to get stdout");
    let stderr = process.stderr.take().expect("Failed to get stderr");
    let mut stdin: ChildStdin = process.stdin.take().expect("Failed to open stdin");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    // Ensure the child process is spawned in the runtime so it can
    // make progress on its own while we await for any output.
    tokio::spawn(async move {
        let status = process.wait().await.expect("child process encountered an error");
        info!("websockets -> child status was: {}", status);
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = stderr_reader.next_line().await? {
            tx_clone.send(line.into_bytes()).ok();
        }
        Ok::<(), Error>(())
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = stdout_reader.next_line().await? {
            tx_clone.try_send(line.into_bytes()).ok();
        }
        Ok::<(), Error>(())
    });
    
    tokio::spawn(async move {
        while let Ok(input) = rx.recv() {
            if input != "quit".to_string() {
                if let Err(e) = stdin.write_all(input.as_bytes()).await {
                    info!("websockets -> Failed to write to stdin: {}", e);
                    break;
                }
            } else { break; }
        }
    });

    Ok(())
}

// async fn process_command(text: String, tx: Sender<Vec<u8>>, process: Arc<Mutex<Option<ChildStdin>>>) {
//     let mut process = process.lock().await;
//     if let Some(ref mut stdin) = *process {
//         if text == "quit".to_string(){
//             // drop(child.stdin);
//         } else {
//             info!("websockets -> We have stdin!!");
//             let input = text.clone();
//             match stdin.write_all(input.as_bytes()).await {
//                 Ok(_) => info!("websockets -> Wrote to stdin"),
//                 Err(e) => error!("Error writing to stdin: {e:?}"),
//             }
//             match stdin.flush().await {
//                 Ok(_) => info!("websockets -> Flushed stdin"),
//                 Err(e) => error!("Error flushing stdin: {:?}", e),
//             }
//         }
//     } else {
//         info!("websockets -> No stdin yet");
//         match handle_command_payload(text.clone(), tx.clone()).await {
//             Ok(stdin) => {
//                 *process = Some(stdin);
//             }
//             Err(e) => error!("Error running command: {e:?}"),
//         }
//     }
// }

pub async fn handle_linux_cmd(
    command_payload: String, 
    tx: Sender<Vec<u8>>
) -> Result<tokio::sync::mpsc::Sender<String>, Error> {
    let mut process: Child = Command::new("sh")
        .arg("-c")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = process.stdout.take().expect("Failed to get stdout");
    let stderr = process.stderr.take().expect("Failed to get stderr");
    let stdin: ChildStdin = process.stdin.take().expect("Failed to open stdin");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    // Use a channel to allow sending input to the stdin of the process
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(32);

    let tx_clone = tx.clone();
    // let mut idle_timeout = sleep(Duration::from_secs(10));
    // pin!(idle_timeout);
    let t = input_tx.clone();
    tokio::spawn(async move {
        // Process both stdout and stderr
        loop {
            let in_tx = input_tx.clone();
            tokio::select! {
                stdout_line = stdout_reader.next_line() => {
                    if let Ok(Some(line)) = stdout_line {
                        if line.contains("Enter") || line.contains("Password:") {
                            // Command is asking for input; handle interactively
                            info!("Detected interactive prompt: {}", line);
                            in_tx.send("YourInputHere\n".to_string()).await?;
                        } else {
                            tx_clone.send(line.into_bytes())?;
                        }
                    }
                }
                stderr_line = stderr_reader.next_line() => {
                    if let Ok(Some(line)) = stderr_line {
                        if line.contains("Enter") || line.contains("Password:") {
                            info!("Detected interactive prompt: {}", line);
                            input_tx.send("YourInputHere\n".to_string()).await?;
                        } else {
                            tx_clone.send(line.into_bytes())?;
                        }
                    }
                }
                // _ = &mut idle_timeout => {
                //     // No output within the timeout duration
                //     input_tx.send("DefaultInput\n".to_string()).await?;
                // }
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), Error>(())
    });

    // Spawn a task to handle stdin input using `input_rx`
    tokio::spawn(async move {
        let mut stdin = stdin; // Move `stdin` into this task
        while let Some(input) = input_rx.recv().await {
            if let Err(e) = stdin.write_all(input.as_bytes()).await {
                error!("Failed to write to stdin: {:?}", e);
                break;
            }
            // Ensure each input is flushed after writing
            if let Err(e) = stdin.flush().await {
                error!("Failed to flush stdin: {:?}", e);
                break;
            }
        }
    });
    
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        // Wait for the child process to complete
        if let Ok(status) = process.wait().await {
            info!("Process exited with status: {status}");
            tx_clone.send("DONE".to_string().into_bytes())?;
        }
        Ok::<(), Error>(())
    });

    Ok(t)
}

