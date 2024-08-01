use crate::{app_state::MastertechContext, filesystem::system_info::generate_client_id, tabs::file_browser::{read_folder, FileBrowser}};
use database::{schema::{utilities::{deserialize_command, serialize_system_info}, ClientId, Cmd, ComputerId, ConnectedClient, SystemInformation, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE}, DATABASE};
use eframe::{egui::{Align, Button, Color32, Direction, Frame, Key, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget}, epaint::Shadow};
use std::{env, path::{Path, PathBuf}, process::Stdio, sync::Arc, time::{Duration, Instant}};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::{ChildStdin, Command}, spawn, sync::Mutex, time::sleep};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use crate::filesystem::system_info::get_sysinfo;
use crossbeam::channel::{Receiver, Sender};
use anyhow::{Result, Error};
use surrealdb::sql::Thing;
use bincode::serialize;
use tracing::info;

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
                let client_hash = generate_client_id(self.computer_data.hostname.clone(), self.computer_data.cpu.trim().to_string());
                let url_string = format!("{}:{}", self.computer_data.hostname.clone(), client_hash.split_at(9).0);
                self.url = Some(format!("wss://sock.master-tech.app/websocket?room_id={}&role=client",  url_string.clone()));
                let computer_id = &self.computer_data.id.clone().unwrap_or( // i need to first check if a computer exists with a customer id or something..
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
                    connected: true,
                    ..Default::default()
                };

                info!("Client: {:?}", connected_client);

                let tx = self.connected_clients_tx.clone();
                spawn(async move {

                    let res: Result<Vec<ConnectedClient>, surrealdb::Error> = DATABASE
                        .query("CREATE connected_client CONTENT $content")
                        .bind(("content", connected_client.clone()))
                        .await
                        .unwrap().take(0);

                    match res{
                        Ok(data) => tx.try_send(data.clone()).unwrap(),
                        Err(e) => info!("Error Creating Client: {e:?}"),
                    }
                });

                if let Some(url) = &self.url{

                    info!("self.url: {}", url.clone());
                    let ctx = ui.ctx().clone();
                    let wakeup = move || ctx.request_repaint(); // wake up UI thread on new message

                    match ewebsock::connect_with_wakeup(url, Default::default(), wakeup) {
                        Ok((mut ws_sender, ws_receiver)) => {
                            ws_sender.send(ewebsock::WsMessage::Text("Client Connected!".to_string()));
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
            if !self.error.is_empty() {
                TopBottomPanel::top("error").show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::RED, format!("Error: {}", &self.error));
                    });
                });
            }
            if let Some(frontend) = &mut self.frontend {
                let connected = frontend.initialize_websocket(ui);
                if !connected{ } // if let Some(db) =  { spawn(async move { }); }
            }
        });
    }
}

pub struct WebConsoleFrontend {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,

    pub tx: Sender<Vec<u8>>,
    pub rx: Receiver<Vec<u8>>,
    pub command_tx: Sender<Vec<u8>>,
    pub command_rx: Receiver<Vec<u8>>,

    pub events: Vec<WsEvent>,
    pub input: String,
    pub messages: Vec<String>,
    pub command: Cmd,
    pub send_specs: bool,
    pub history: Vec<String>,
    pub connected: bool,
    pub timeout_counter: Instant,
    pub process: Arc<Mutex<Option<ChildStdin>>>
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
            input: String::new(),
            messages: Vec::new(),
            command: Cmd::None,
            history: Vec::new(),
            send_specs: false,
            connected: false,
            timeout_counter: Instant::now(),
            process: Arc::new(Mutex::new(None)),
        }
    }

    pub fn handle_events(&mut self) -> bool{
        let mut connected = false;

        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }
        
        if let Ok(sysinfo) = &mut self.rx.try_recv(){
            self.ws_sender.send(WsMessage::Binary(std::mem::take(sysinfo)));
        }
        
        if let Ok(cmd_output) = &mut self.command_rx.try_recv(){
            self.ws_sender.send(WsMessage::Binary(std::mem::take(cmd_output)));
        }

        if self.timeout_counter.elapsed().as_secs() > 10 {
            info!("Its been over 10 seconds since last ping");
        }



        for event in &self.events{
            match event{
                WsEvent::Message(msg) => {
                    connected = true;
                    match msg{
                        WsMessage::Binary(bin) => {
                            self.history.push(format!("{:?}", deserialize_command(&bin.clone())));
                            let cmd = deserialize_command(&bin.clone());
                            info!("297Cmd: {bin:?}");
                            match cmd{
                                Cmd::LiveData => {
                                    let tx = self.tx.clone();
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let connected = self.connected.clone();
                                    spawn(async move { 
                                        match live_computer_stats(tx.clone(), connected).await{
                                            Ok(_) => drop(tx),
                                            Err(e) => info!("Error with live data {e:?}"),
                                        }
                                    });
                                },
                                Cmd::Tuneup => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let _tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");
                                    
                                    // spawn(async move {
                                    //     handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                                    // });
                                },
                                Cmd::Cps => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");
                                    spawn(async move {
                                        handle_command_payload("SELECT * FROM Win32_OperatingSystem".to_string(), tx.clone()).await.unwrap();
                                    });
                                },
                                Cmd::Qc => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");
                                    spawn(async move {
                                        handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                                    });
                                },
                                Cmd::SfcScan => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");
                                    
                                    spawn(async move {
                                        handle_command_payload("sfc /scannow".to_string(), tx.clone()).await.unwrap();
                                    });
                                },
                                Cmd::DismScan => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");

                                    spawn(async move {
                                        handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                                    });
                                },
                                Cmd::ChkDsk => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let _tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");
                                    
                                    // spawn(async move {
                                    //     handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                                    // });
                                },
                                Cmd::Mbr2Gpt => {
                                    self.history.push(format!("Cmd: {:?}", cmd));
                                    let _tx = self.tx.clone();
                                    info!("Cmd: {cmd:?}");
                                    // spawn(async move {
                                    //     handle_command_payload("chkdsk ".to_string(), tx.clone()).await.unwrap();
                                    // });
                                },
                                Cmd::ReadDir(path) => {
                                    info!("READING DIR");
                                    let current_path = env::current_dir().unwrap_or_default();
                                    let contents = if path == "current" {
                                        let paths = read_folder(&current_path, 2, false);
                                        info!("Current paths: {:?}", paths.clone());
                                        paths
                                    } else {
                                        let p: PathBuf = Path::new(path.as_str()).to_path_buf();
                                        if p.is_dir() {
                                            let paths = read_folder(&p, 2, false);
                                            info!("Paths: {:?}", paths.clone());
                                            paths
                                        } else {
                                            let paths = read_folder(&current_path, 2, false);
                                            info!("Paths: {:?}", paths.clone());
                                            paths
                                        }
                                    };

                                    let mut strings = Vec::new();
                                    for x in contents {
                                        strings.push(x.to_string_lossy().to_string());
                                    }
                                    let payload = serialize(&Cmd::DirContents(strings));
                                    let _x = serialize(&FileBrowser::new());
                                    match payload {
                                        Ok(bytes) => {
                                            self.ws_sender.send(WsMessage::Binary(bytes));
                                            
                                        },
                                        Err(e) => info!("Error serializing paths: {e:?}"),
                                    }
                                }
                                Cmd::Quit => {
                                    self.connected = false;
                                }
                                _ => {
                                    // self.history.push(format!("Raw Binary: {:?}", bin));
                                },
                            }
                        },
                        WsMessage::Text(txt) => {
                            self.history.push(format!("Raw Command: {}", txt.clone()));
                            let tx = self.command_tx.clone();
                            let text = txt.clone();
                            let process = Arc::clone(&self.process);
                            spawn(async move {
                                process_command(text.clone(), tx.clone(), process).await;
                            });
                        },
                        WsMessage::Ping(_x) => {
                            // info!("Got a Ping: {x:?}");
                            self.timeout_counter = Instant::now();
                        },
                        WsMessage::Pong(_x) => {
                            // info!("Got a Pong: {x:?}");
                            self.timeout_counter = Instant::now();
                        },
                        _ => ()
                        
                    }
                },
                WsEvent::Opened => {
                    connected = true;
                },
                WsEvent::Closed => {
                    connected = false;

                },
                WsEvent::Error(e) => {
                    connected = false;
                    self.history.push(e.clone())
                },
            }
        }
        self.events.clear();
        connected
    }

    pub fn initialize_websocket(&mut self, ui: &mut Ui) -> bool {
        ui.vertical_centered(|ui | ui.heading("Received events:"));
        ui.separator();
        let connected = self.handle_events();

        ScrollArea::vertical()
            .animated(true)
            .max_height(ui.available_height() - 5.0)
            .max_width(f32::INFINITY)
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| 
        {
            ui.set_width(ui.available_width());
            let max_msg_width = ui.available_width() / 2.5;
            let fixed_height = 50.0;
            let min_width = 200.0;

            for item in self.history.iter(){
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
                                
                                let mut b_panel_marg = Margin::default();
                                b_panel_marg.top = 3.0;

                                let color = Color32::from_rgb(10,10,12);

                                let note_frame = Frame::none().fill(color)
                                    .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
                                    .inner_margin(Margin::symmetric(6.0, 10.0)).rounding(rnding);

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
                                    let text: (&str, &str) = item.split_once(":").unwrap_or(("Raw Binary", ""));
                                    let cmd = text.1;
                                    (
                                        RichText::new("Raw Binary Payload").strong().monospace().color(Color32::LIGHT_BLUE),
                                        RichText::new(cmd).strong().monospace()
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
                                        ui.label(txt);
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
            let text_edit = TextEdit::singleline(&mut self.input).hint_text("Send Message").ui(ui);
            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            if text_edit.lost_focus() && key_press {
                text_edit.request_focus();
                self.history.push(format!("You\n{}", self.input.clone()));
                self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));
            }
        });

      connected
    }

    
}

async fn live_computer_stats(tx: Sender<Vec<u8>>, _connected: bool) -> Result<(), Error>{
    loop {
        sleep(Duration::from_secs(4)).await;
        let systeminfo: SystemInformation = get_sysinfo().await?;
        info!("{systeminfo:?}");
        tx.send(serialize_system_info(&systeminfo))?;
        // if app.lock().await.finish { break; }
    }
    Ok(())
}
async fn handle_command_payload(string_payload: String, tx: Sender<Vec<u8>>) -> Result<ChildStdin, Error>  { 
    // #[cfg(target_os="windows")]{ return handle_windows_cmd(string_payload, tx.clone()).await?; }
    if cfg!(target_os="windows") { Ok(handle_windows_cmd(string_payload, tx.clone()).await?) }
    else { Ok(handle_linux_cmd(string_payload, tx.clone()).await?) }
}

async fn handle_windows_cmd(command_payload: String, tx: Sender<Vec<u8>>) -> Result<ChildStdin, Error> {
    use tokio::{process::{Child, ChildStdin}, time::Instant};

    let start = Instant::now();
    info!("Executing command: {}", command_payload);
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
    
    // let mut stdout_stream = BufReader::new(stdout).lines();
    // let mut stderr_stream = BufReader::new(stderr).lines();
    // Use a loop to select between stdout and stderr streams
    // loop {
    //     select! {
    //         Ok(line) = stdout_stream.next_line() => match line {
    //             Some(line) => {
    //                 // tx_clone.send(stderr_buf).ok();
    //                 info!("stdout: {}", line);
    //             },
    //             None => break,
    //         },
    //         Ok(line) = stderr_stream.next_line() => match line {
    //             Some(line) => {
    //                 // tx_clone.send(stderr_buf).ok();
    //                 info!("stderr: {}", line);
    //             },
    //             None => break,
    //         },
    //         else => break, // Exit the loop when both streams are exhausted
    //     }
    // }

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
    info!("output: {:?}", output);
    let duration = start.elapsed();
    info!("Command executed in {:?}", duration);
    let tx_clone = tx.clone();
    if !output.status.success() {
        info!("output status not successfull");
        tx_clone.send(output.stderr).ok();
    }

    Ok(stdin)
}

async fn process_command(text: String, tx: Sender<Vec<u8>>, process: Arc<Mutex<Option<ChildStdin>>>) {
    let mut process = process.lock().await;
    if let Some(ref mut stdin) = *process {
        if text == "quit".to_string(){
            // drop(child.stdin);
        } else {
            info!("We have stdin!!");
            let input = text.clone();
            match stdin.write_all(input.as_bytes()).await {
                Ok(_) => info!("Wrote to stdin"),
                Err(e) => info!("error writing to stdin: {e:?}"),
            }
            match stdin.flush().await {
                Ok(_) => info!("Flushed stdin"),
                Err(e) => info!("Error flushing stdin: {:?}", e),
            }
        }
    } else {
        info!("No stdin yet");
        match handle_command_payload(text.clone(), tx.clone()).await {
            Ok(stdin) => {
                *process = Some(stdin);
            }
            Err(e) => info!("error running command: {e:?}"),
        }
    }
}

async fn handle_linux_cmd(command_payload: String, tx: Sender<Vec<u8>>) -> Result<ChildStdin, Error> {
    info!("Executing command: {}", command_payload);
    let mut process = Command::new("sh")
        .arg("-c")
        .arg(&command_payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;


    // Handle stdout and stderr
    let mut stdout = process.stdout.take().expect("Failed to open stdout");
    let mut stderr = process.stderr.take().expect("Failed to open stderr");
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
    let tx_clone = tx.clone();
    if !output.status.success() {
        tx_clone.send(output.stderr).ok();
    }

    Ok(stdin)
}

