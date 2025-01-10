use eframe::{egui::{Align, Button, CentralPanel, Color32, Context, Direction, Frame, Id, Key, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget}, epaint::Shadow};
use database::{schema::{utilities::{compress_data, query_id}, ConnectedClient, Record, SystemInformation, CONNECTED_CLIENT_TABLE}, DATABASE};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, process::{Child, ChildStdin, Command}, spawn, sync::Mutex, time::sleep};
use crate::{app_state::MastertechContext, filesystem::system_info::generate_client_id, tabs::file_browser::read_folder};
use std::{env, path::Path, process::Stdio, sync::{atomic::Ordering, Arc}, time::{Duration, Instant}};
use displays::{channel_manager::ChannelManager, deserialize_command, serialize_system_info, virtual_filesystem::FileSystem, Cmd, FileSystemAction};
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use crate::filesystem::system_info::get_sysinfo;
use crossbeam::channel::{Receiver, Sender};
use surrealdb::{RecordId, Response};
use anyhow::{Result, Error};
use bincode::serialize;
use log::{error, info};

impl MastertechContext{
    pub fn websockets(&mut self, ui: &mut Ui) {
        if !self.show_ws_viewport.load(Ordering::Relaxed) {
            TopBottomPanel::top("Client Top Panel").show_inside(ui, |ui| {
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
                            let update_client: Response = DATABASE.query("UPDATE $client SET friendly_name = $name")
                                .bind(("name", name))
                                .bind(("client", client))
                                .await?;
    
                            info!("websockets -> update_client: {update_client:?}");
                            Ok::<(), Error>(())
                        });
                    }

                    ui.add_space(5.);
                    TextEdit::singleline(&mut self.client_friendly_name)
                        .hint_text("Client Name")
                        .margin(Margin::symmetric(10., 6.))
                        .ui(ui);
                });
            });

            if !self.error.is_empty() {
                TopBottomPanel::bottom("error").show_inside(ui, |ui| {
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
            assigned_user: Some(self.shared_ctx.current_user.as_ref().cloned().unwrap_or_default().id),
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
                .content(connected_client)
                .await?
                .take();

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
                    self.frontend = Some(WebConsoleFrontend::new(ws_sender, ws_receiver));
                }

                spawn(async move {
                    let update_client: Response = DATABASE.query("UPDATE $client SET connected = true, last_update = time::now()")
                        .bind(("client", client_id.clone()))
                        .await?;

                    info!("websockets -> update_client: {update_client:?}");
                    Ok::<(), Error>(())
                });

                self.error.clear();
            }
            Err(error) => {
                info!("websockets -> Failed to connect to {:?}: {}", &self.url, error);
                spawn(async move {
                    let update_client: Response = DATABASE.query("UPDATE $client SET connected = false, last_update = time::now()")
                        .bind(("client", client_id.clone()))
                        .await?;

                    info!("websockets -> update_client: {update_client:?}");
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
    live_stats: bool
}

impl WebConsoleFrontend {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        let (tx, rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let (command_tx, command_rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let interactive_input = String::create_unbounded_channel();
        
        Self {
            ws_sender, ws_receiver,
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
            live_stats: false
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
                Err(e) => info!("Error compressing data: {e:?}"),
            }
        }
        
        while let Ok(cmd_output) = &mut self.command_rx.try_recv(){
            self.ws_sender.send(WsMessage::Binary(std::mem::take(cmd_output)));
        }

        // if self.timeout_counter.elapsed().as_secs() > 10 { info!("websockets -> Its been over 10 seconds since last ping"); }

        for event in self.events.clone() {
            match event{
                WsEvent::Message(msg) => {
                    connected = true;
                    match msg{
                        WsMessage::Binary(bin) => {
                            self.history.push(format!("{:?}", deserialize_command(&bin.clone())));
                            let cmd: Cmd = deserialize_command(&bin.clone());
                            info!("websockets -> Binary Message: {cmd:?}");
                            self.handle_command(cmd);
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
            Cmd::Tuneup => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let _tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");
                
                // spawn(async move {
                //     handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                // });
            },
            Cmd::Cps => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");
                spawn(async move {
                    handle_command_payload("SELECT * FROM Win32_OperatingSystem".to_string(), tx.clone()).await.unwrap();
                });
            },
            Cmd::Qc => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");
                spawn(async move {
                    handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                });
            },
            Cmd::SfcScan => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");
                
                spawn(async move {
                    handle_command_payload("sfc /scannow".to_string(), tx.clone()).await.unwrap();
                });
            },
            Cmd::DismScan => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");

                spawn(async move {
                    handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                });
            },
            Cmd::ChkDsk => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let _tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");
                
                // spawn(async move {
                //     handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                // });
            },
            Cmd::Mbr2Gpt => {
                self.history.push(format!("Cmd: {:?}", cmd));
                let _tx = self.tx.clone();
                info!("websockets -> Cmd: {cmd:?}");
                // spawn(async move {
                //     handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                // });
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
    
                        let payload = serialize(
                            &Cmd::FileSystemAction(FileSystemAction::GetNode(node))
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
                        let payload = serialize(
                            &Cmd::FileSystemAction(FileSystemAction::PreviewedFile(file))
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
            _ => {},
            // Cmd::Command => todo!(),
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
                    let rnding = Rounding {
                        ne: if is_message_from_myself { 0.0 } else { rounding },
                        nw: if is_message_from_myself { rounding } else { 0.0 },
                        se: rounding,
                        sw: rounding,
                    };

                    let response = Frame::none()
                        .rounding(rnding)
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
                                shadow.blur = 3.0;
                                shadow.spread = 3.0;
                                shadow.color = Color32::from_rgb(40,36,40);
                                let color = Color32::from_rgb(10,10,12);

                                let note_frame = Frame::none().fill(color)
                                    .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke)
                                    .inner_margin(Margin::same(0.)).rounding(rnding);

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
                                        let mut layouter = |ui: &Ui, string: &str, wrap_width: f32| {
                                            let mut layout_job: eframe::egui::text::LayoutJob =
                                                highlight(ui.ctx(), ui.style(), &CodeTheme::dark(12.), string, "bash".into()); // || "zsh".into()
                                            layout_job.wrap.max_width = wrap_width;
                                            ui.fonts(|f| f.layout_job(layout_job))
                                        };
                                        TextEdit::singleline(&mut txt.text())
                                            .id_salt(Id::new(format!("{item:?}-{count:?}")))
                                            .frame(false)
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
            
            let mut layouter = |ui: &Ui, string: &str, wrap_width: f32| {
                let mut layout_job =
                    highlight(ui.ctx(), ui.style(), &theme, string, "bash".into()); // || "zsh".into()
                layout_job.wrap.max_width = wrap_width;
                ui.fonts(|f| f.layout_job(layout_job))
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

async fn handle_linux_cmd(
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

