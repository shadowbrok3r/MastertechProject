use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Context, Direction, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::{schema::{ConnectedClient, Node, Record, SystemInformation, CONNECTED_CLIENT_TABLE}, DATABASE};
use regex::Regex;
use core::f32;
use std::collections::{HashMap, VecDeque};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use displays::{channel_manager::ChannelManager, virtual_filesystem::{FileSysHelper, FileSystem}, Cmd, FileSystemAction};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use surrealdb::Response;
use bincode::serialize;
use web_time::Instant;
use log::info;

use super::charts::LinePlot;

pub trait ClientHandler { 
    fn connect(&mut self);
    fn export_logs(&mut self, history: Vec<History>);
    fn delete_client(&mut self);
}

pub enum ClientConnection{
    ClientUrl(String),
    Disconnect(String)
}

pub enum WsDisplayState {
    LiveStats,
    Explorer,
    Shell,
    ToolBox
}

#[derive(Clone)]
struct WebSocketHelperDelegate {
    tx: Sender<Cmd>
}

impl WebSocketHelperDelegate {
    fn new(tx: Sender<Cmd>) -> Self {
        Self { tx }
    }
}

impl FileSysHelper for WebSocketHelperDelegate {
    fn handle_filesystem_action(&mut self, action: &FileSystemAction) {
        log::warn!("FileSysHelper for WebSocketHelperDelegate -> Action -> {action:?}");
        let _ = self.tx.try_send(Cmd::FileSystemAction(action.clone()));
    }
}


pub struct WebSocketClient {
    pub client: ConnectedClient,

    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    /// Commands that we are SENDING to Mastertech
    send_cmd_tx: Sender<Cmd>, 
    /// Commands that we are SENDING to Mastertech
    send_cmd_rx: Receiver<Cmd>,
    /// Commands that we are RECEIVING from Mastertech
    receive_cmd_tx: Sender<Cmd>,
    /// Commands that we are RECEIVING from Mastertech
    receive_cmd_rx: Receiver<Cmd>,
    /// Sending / Receiving of UI state
    display_state_channel: (Sender<WsDisplayState>, Receiver<WsDisplayState>),

    pub input: String,
    pub messages: Vec<String>,

    pub cpu_clock: VecDeque<f32>,
    pub temps: VecDeque<HashMap<String, f32>>,
    pub cpu_percentage: VecDeque<f32>,
    pub ram_usage: VecDeque<f32>,

    pub sysinfo: Option<SystemInformation>,
    pub history: Vec<History>,
    pub loading: bool,
    pub timeout_counter: Instant,
    pub toolbox: FileSystem,
    pub state: WsDisplayState,
    pub explorer: FileSystem,
    pub interactive: bool,
    pub history_idx: usize,
    helper_delegate: WebSocketHelperDelegate,
    /// Accumulates fragments of messages
    buffer: String,     
    my_history: Vec<History>,
    notifications: i32,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct History {
    from: String,
    message: String,
    timestamp: String
}

lazy_static::lazy_static! {
    static ref TRON_COMPLETE_REGEX: Regex = Regex::new("Tron.*complete").unwrap();
}


impl WebSocketClient{
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, client: ConnectedClient, toolbox: FileSystem) -> Self {
        let display_state_channel = <WsDisplayState>::create_unbounded_channel();
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        let helper_delegate = WebSocketHelperDelegate::new(send_cmd_tx.clone());
        let mut explorer = FileSystem::new();
        explorer.helper_delegate = Some(Box::new(helper_delegate.clone()));
        
        Self {
            client,
            ws_sender,
            ws_receiver,
            send_cmd_tx, 
            send_cmd_rx,
            receive_cmd_tx, 
            receive_cmd_rx,
            display_state_channel,
            timeout_counter: Instant::now(),
            toolbox,
            state: WsDisplayState::Shell,
            explorer,
            interactive: false,
            helper_delegate,
            input: Default::default(),
            messages: Default::default(),
            cpu_clock: Default::default(),
            temps: Default::default(),
            cpu_percentage: Default::default(),
            ram_usage: Default::default(),
            sysinfo: Default::default(),
            history: Default::default(),
            loading: Default::default(),
            history_idx: Default::default(),
            buffer: Default::default(),
            my_history: Default::default(),
            notifications: Default::default(),
        }
    }
    
    pub fn receive(&mut self, ctx: &Context) {
        self.explorer.receive(ctx);
        // if self.timeout_counter.elapsed().as_secs() > 10 { info!("Its been over 10 seconds since last ping"); }
        // info!("Timer: {:?}", self.timeout_counter.elapsed().as_secs());

        if let Some(event) = &self.ws_receiver.try_recv() {
            ctx.request_repaint();
            match event{
                WsEvent::Message(msg) => {
                    match msg{
                        WsMessage::Binary(bin) => {
                            if let Some(sysinfo) = deserializer::<SystemInformation>(bin){
                                info!("Got sysinfo");
                                self.loading = false;
                                let normalized_cpu_clock = normalize(sysinfo.cpu_clock, 0.0, 100.0); // Example range for CPU clock
                                // let normalized_cpu_percentage = normalize(sysinfo.cpu_percentage, 0.0, 100.0);
                                let total_ram = if sysinfo.total_memory > 0.0 { (sysinfo.used_memory / sysinfo.total_memory)*100.0 } else { 0.0 };
                                // let normalized_ram_usage = normalize(total_ram, 0.0, 100.0); // Example range for RAM usage
                                self.cpu_percentage.push_back(sysinfo.cpu_percentage);
                                self.cpu_clock.push_back(normalized_cpu_clock);
                                self.ram_usage.push_back(total_ram);
                                self.sysinfo = Some(sysinfo);
                                // info!("normalized_ram_usage: {normalized_ram_usage:?}\nLen: {:?}", self.cpu_percentage.len());
                            } else if let Some(cmd) = deserializer::<Cmd>(bin){
                                let _ = self.receive_cmd_tx.try_send(cmd);
                            } else{ 
                                if self.interactive {
                                    let msg = String::from_utf8_lossy(&bin).to_string();
                                    if TRON_COMPLETE_REGEX.is_match(&msg) {
                                        self.interactive = false;
                                    }
                                }

                                if bin.len() > 0 {
                                    self.loading = false;
                                    let msg = String::from_utf8_lossy(&bin).to_string();
                                    info!("Binary Msg: {msg}");

                                    // Check if the incoming message is "DONE"
                                    if msg.eq("DONE") {
                                        // Push the buffered content as a new history entry
                                        if !self.buffer.is_empty() {
                                            self.history.push(History {
                                                from: "Client".to_string(),
                                                message: self.buffer.clone(),
                                                timestamp: chrono::Local::now().to_rfc3339(),
                                            });
                                            self.buffer.clear(); // Clear the buffer after processing
                                        }
                                    } else {
                                        // Append the incoming message to the buffer with a newline
                                        self.buffer.push_str(&msg);
                                        self.buffer.push('\n');
                                    }
                                }
                            }
                        },
                        WsMessage::Text(txt) => {
                            self.loading = false;
                            info!("Text data: {txt:#?}");
                        
                            // Append the incoming text to the buffer
                            self.buffer.push_str(&txt);
                        
                            // Process the buffer for complete lines
                            while let Some(pos) = self.buffer.find('\n') {
                                // Extract the complete line up to the newline character
                                let line = self.buffer.drain(..=pos).collect::<String>().trim_end().to_string();
                        
                                // Create a new history entry for the extracted line
                                let history = History {
                                    from: "Client".to_string(),
                                    message: line,
                                    timestamp: chrono::Local::now().to_rfc3339(),
                                };
                        
                                // Add to history
                                self.history.push(history);
                                self.notifications += 1;
                            }
                        },
                        _ => {}
                    }
                },
                _ => {
                    self.history.push(History { 
                        from: "Client".to_string(), 
                        message: format!("{event:?}"), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    });
                    self.notifications += 1;
                },
            }
        }
        
        if let Ok(state) = self.display_state_channel.1.try_recv() {
            ctx.request_repaint();
            self.state = state;
        }

        // Here we will handle commands we are going to SEND to Mastertech
        if let Ok(command) = self.send_cmd_rx.try_recv() {
            ctx.request_repaint();
            match command {
                Cmd::FileSystemAction(ref action) => {
                    match action {
                        FileSystemAction::EnterDirectory(directory) => {
                            info!("web_console/websockets.rs -> EnterDirectory -> {directory:?}\nweb_console/websockets.rs -> EnterDirectory -> Root: {:?}", self.explorer.root);
                            info!("Prefix before double clicking folder: {}", self.explorer.current_prefix);
                            self.explorer.double_click_folder(&directory);
                            info!("After: {}", self.explorer.current_prefix);
                        },
                        FileSystemAction::GetNode(new_node) => {
                            log::info!("web_console/websockets.rs -> GetNode -> Root: {:?}", self.explorer.root); // {new_node:?}
                            if let Node::Folder(prefix, _) = new_node {
                                if &self.explorer.current_prefix == "current" {
                                    self.explorer.current_prefix = prefix.clone();
                                }
                                info!("web_console/websockets.rs -> Current prefix: {}\nNew prefix: {}", self.explorer.current_prefix, prefix);
                            }
                            let insert_node = self.explorer.insert_node(new_node.clone());
                            info!("web_console/websockets.rs -> InsertNode -> {insert_node:?}");
                        },
                        FileSystemAction::RequestNewContents(directory) => {
                            log::info!("web_console/websockets.rs -> RequestNewContents -> {directory}");
                            info!("ACTION TO SEND: {command:?}");
                            self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                        }
                        FileSystemAction::Execute(label) => { 
                            self.explorer.execute_file = label.clone(); 
                            if !label.is_empty() {
                                self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                                self.interactive = true;
                                self.history.push(History { 
                                    from: "Client".to_string(), 
                                    message: "Switching to interactive mode".to_string(), 
                                    timestamp:  chrono::Local::now().to_rfc3339()
                                });
                                self.notifications += 1;
                                let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                            }
                        },
                        FileSystemAction::Select((modifiers, label)) => {
                            if self.explorer.selected_items.borrow().contains(label) {
                                // If the item was already selected, deselect it
                                self.explorer.selected_items.borrow_mut().remove(label);
                            } 
                            if modifiers.ctrl { 
                                self.explorer.selected_items.borrow_mut().insert(label.clone());
                            } else { // If the control key is not down, clear previous selection and select the current item
                                self.explorer.selected_items.borrow_mut().clear();
                                self.explorer.selected_items.borrow_mut().insert(label.clone());
                            }
                            
                            
                            self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                        },
                        FileSystemAction::ExpandDirectory(directory) => self.explorer.expand_folder(&directory),
                        FileSystemAction::NavigateHome => {
                            info!("web_console/websockets.rs -> NavigateHome");
                            // self.explorer.navigation_stack.clear();
                            // self.explorer.current_prefix.clear();
                        }
                        // FileSystemAction::CopyToClient(_) => todo!(),
                        // FileSystemAction::CopyFromClient(_) => todo!(),
                        // FileSystemAction::Delete(_) => todo!(),
                        FileSystemAction::PreviewedFile(file) => {
                            self.explorer.previewed_file = Some(file.to_string());
                        },
                        _ => {
                            self.ws_sender.send(WsMessage::Binary(serialize_command(&command)));
                        }
                    }
                }
                Cmd::Quit => {
                    self.interactive = false;
                    self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Quit)));
                },
                _ => self.ws_sender.send(WsMessage::Binary(serialize_command(&command)))
            }
        }

        // Here we will handle commands we receive from Mastertech
        if let Ok(command) = self.receive_cmd_rx.try_recv() {
            ctx.request_repaint();
            if let Cmd::FileSystemAction(file_system_action) = command {
                self.helper_delegate.handle_filesystem_action(&file_system_action);
            }
            
        }
    }
    
    pub fn show(&mut self, ui: &mut Ui) { // , add_contents: impl FnOnce(&mut Ui)
        self.receive(ui.ctx());
        ui.set_min_height(600.);

        // let exact_height = match self.state {
        //     WsDisplayState::Shell => .,
        //     _ => 
        // };

        TopBottomPanel::top(Id::new(format!("ClientTopPanel-{}", self.client.client_hash)))
        .exact_height(26.)
        // .frame(top_frame)
        .show_inside(ui, |ui| 
        {
            ui.add_space(2.);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| { 
                    let btn_color = ui.style().visuals.error_fg_color;
                    if Button::new(RichText::new("My Tools").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                    }

                    if Button::new(RichText::new("Explorer").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Explorer);
                        self.notifications = 0;
                        // if we are already in an interactive mode, then we dont want to quit that session,
                        if !self.interactive {
                            if self.explorer.current_prefix.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory("current".to_string())));
                            } else {
                                let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory(self.explorer.current_prefix.clone())));
                            }
                        }
                    }

                    if Button::new(RichText::new("Charts").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::LiveStats);
                        let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                    }

                    let notifs = if let WsDisplayState::Shell = self.state {
                        format!("Shell")
                    } else {
                        if self.notifications > 0 {
                            format!("Shell   {}", self.notifications)
                        } else {
                            format!("Shell")
                        }
                    };

                    if Button::new(RichText::new(notifs).color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                    }

                    if self.interactive {
                        if Button::new(RichText::new("Quit").color(Color32::RED)).ui(ui).clicked(){
                            self.send_cmd_tx.try_send(Cmd::Quit);
                        }
                    }
                });

                if let WsDisplayState::Shell = self.state {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| { 
                        self.command_shell_menu(ui);
                    });
                }
            });
            ui.add_space(2.);
        });


        match self.state {
            WsDisplayState::LiveStats => self.show_live_stats(ui),
            WsDisplayState::Explorer => ui.group(|ui| self.explorer.display(ui)).inner,
            WsDisplayState::ToolBox => ui.group(|ui| self.toolbox.display(ui)).inner,
            WsDisplayState::Shell => self.show_shell(ui),
        };
    }

    fn command_shell_menu(&mut self, ui: &mut Ui) {
        if Button::new("Tuneup").ui(ui).clicked(){
            info!("web_console -> websockets.rs -> Tuneup clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::Tuneup);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Tuneup)));
            // self.history.push(format!("You\nCommand::Tuneup"));
        }
        
        if Button::new("CPS").ui(ui).clicked(){
            info!("web_console -> websockets.rs -> CPS clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::Cps);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Cps)));
            // self.history.push(format!("You\nCommand::Cps\nChecking current antivirus"));
            self.input = "SELECT * FROM Win32_OperatingSystem".to_string();
        }

        if Button::new("SFC").ui(ui).clicked(){
            info!("web_console -> websockets.rs -> SFC clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::SfcScan);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::SfcScan)));
            // self.history.push(format!("You\nCommand::SfcScan"));
            self.input = "sfc /scannow".to_string();
        }

        if Button::new("Dism").ui(ui).clicked(){
            info!("web_console -> websockets.rs -> Dism clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::DismScan);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::DismScan)));
            // self.history.push(format!("You\nCommand::DismScan"));
            self.input = "dism /online /cleanup-image /scanhealth\ndism /online /cleanup-image /checkhealth\ndism /online /cleanup-image /restorehealth".to_string();
        }

        if Button::new("Chkdsk").ui(ui).clicked(){
            info!("web_console -> websockets.rs -> Chkdsk clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::ChkDsk);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::ChkDsk)));
            // self.history.push(format!("You\nCommand::ChkDsk"));
            self.input = "chkdsk /f /x /r".to_string();
            
        }

        if Button::new("Mbr2Gpt").ui(ui).clicked(){
            info!("web_console -> websockets.rs -> Mbr2Gpt clicked");
            let _ = self.send_cmd_tx.try_send(Cmd::Mbr2Gpt);
            // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Mbr2Gpt)));
            // self.history.push(format!("You\nCommand::Mbr2Gpt"));
            self.input = "mbr2gpt /Convert /AllowFullOS /disk:0".to_string();
        }
    }

    fn show_live_stats(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            if let Some(sysinfo) = &self.sysinfo {
                // let normalized_temps: Vec<f32> = sysinfo.component_temps.values().map(|&temp| normalize(temp, 0.0, 100.0)).collect();

                if self.cpu_percentage.len() < 30
                    // || self.component_temps.len() < 30
                    || self.cpu_clock.len() < 30
                    || self.ram_usage.len() < 30 {

                    // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                } else {
                    self.cpu_percentage.pop_front();
                    self.cpu_percentage.push_back(sysinfo.cpu_percentage);
                    self.cpu_clock.pop_front();
                    self.cpu_clock.push_back(sysinfo.cpu_clock);
                    self.ram_usage.pop_front();
                    self.ram_usage.push_back(sysinfo.used_memory);
                    // self.component_temps.pop_front();
                    // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                }

                if self.cpu_percentage.len() > 50
                    || self.cpu_clock.len() > 50
                    || self.ram_usage.len() > 50 {
                    // || self.component_temps.len() < 30 
                    self.cpu_percentage.clear();
                    self.cpu_clock.clear();
                    self.ram_usage.clear();
                }


                let percentages = self.cpu_percentage.make_contiguous().to_owned();
                let clocks = self.cpu_clock.make_contiguous().to_owned();
                // let temps = self.component_temps.make_contiguous().to_owned();
                let ram = self.ram_usage.make_contiguous().to_owned();
        
                // info!("\nsysinfo: CPU %: {percentages:?}, \nCPU Clock: {clocks:?}, \nRAM usage: {ram:?}");
                // let temps_plot = LinePlot::new(&[0.0], &temps.as_slice());
                let width = ui.available_width() - 50.0;
                // self.timeout_counter.elapsed().as_secs()
                let mut cpu_usage_plot = LinePlot::new(&[0.0], &percentages.as_slice(), width);
                let mut cpu_clock_plot = LinePlot::new(&[0.0], &clocks.as_slice(), width);
                let mut ram_usage_plot = LinePlot::new(&[0.0], &ram.as_slice(), width);
        
                ui.vertical_centered_justified(|ui| {

                    // temps_plot.ui(ui, "System Temps", temps_plot.line("System Temps (°C)", Color32::from_rgb(255, 69, 0)));
                    cpu_usage_plot.ui(ui, "CPU Usage", cpu_usage_plot.line("CPU(%)", Color32::from_rgb(170, 10, 150)));
                    cpu_clock_plot.ui(ui, "CPU Clock", cpu_clock_plot.line("CPU (MHz)", Color32::from_rgb(21, 232, 165)));
                    ram_usage_plot.ui(ui, "RAM Usage", ram_usage_plot.line("RAM (MB)", Color32::from_rgb(0, 191, 255)));
                });
            }
        });
        ui.add_space(10.0);
    }

    fn show_shell(&mut self, ui: &mut Ui) {
        let b_panel_marg = Margin::symmetric(5., 10.);

        let id = ui.auto_id_with(format!("Chat {:?}", self.client.client_hash));

        TopBottomPanel::bottom(id)
            .default_height(ui.available_height()/1.2)
            // .resizable(false)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            
            let theme = CodeTheme::from_memory(ui.ctx(), ui.style());
            let style = ui.style_mut();
            let default_rounding = Rounding::same(2.0);
            style.visuals.widgets.inactive.rounding = default_rounding;
            style.visuals.widgets.active.rounding = default_rounding;
            style.visuals.widgets.hovered.rounding = default_rounding;
            
            let mut layouter = |ui: &Ui, string: &str, _: f32| {
                let mut layout_job =
                    highlight(ui.ctx(), &ui.style(), &theme, string, "bash".into()); // || "zsh".into()
                layout_job.wrap.max_width = ui.available_width()/1.1;
                ui.fonts(|f| f.layout_job(layout_job))
            };

            ui.add_space(3.);

            let text_edit = TextEdit::singleline(&mut self.input)
                .hint_text("Use Wisely..")
                .margin(Margin::symmetric(10., 4.))
                .desired_width(ui.available_width())
                .desired_rows(4)
                .layouter(&mut layouter)
                .ui(ui);
            

            
            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            let up_press = ui.input(|i| i.key_pressed(Key::ArrowUp));
            let down_press = ui.input(|i| i.key_pressed(Key::ArrowDown));
            let copy_key = ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::C)));

            if copy_key && text_edit.has_focus() {
                self.input.clear();
            }

            if down_press {
                if self.history_idx <= self.my_history.len() {
                    self.history_idx += 1;
                }
                if let Some(history) = self.my_history.get(self.history_idx){
                    self.input = history.message.clone();
                }
            } 
            if up_press {
                if self.history_idx > 0 {
                    self.history_idx -= 1;
                }
                if let Some(history) = self.my_history.get(self.history_idx){
                    self.input = history.message.clone();
                }
            }

            if text_edit.lost_focus() && key_press && !self.interactive{
                self.loading = true;
                text_edit.request_focus();

                self.history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                self.notifications += 1;

                self.my_history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));

            } else if text_edit.lost_focus() && key_press && self.interactive { 
                text_edit.request_focus();
                self.history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });
                self.notifications += 1;

                self.my_history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                match serialize(&Cmd::InteractiveInput(std::mem::take(&mut self.input))){
                    Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                    Err(e) => self.history.push(History { 
                        from: "Client".to_string(), 
                        message: e.to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    }),
                } 
            }
        
        });

        let central_panel_frame = Frame::none().fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
            .stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
            .inner_margin(Margin::same(6.0));

        // info!("avail_size: {:?}", avail_size);
        CentralPanel::default()
            .frame(central_panel_frame)
            .show_inside(ui, |ui| 
        {
        // ui.allocate_ui(Vec2::new(avail_size.x, avail_size.y), |ui| {
            let id = Id::new(format!("scroll_area-{:?}", self.client.client_hash));
            ScrollArea::vertical()
                .id_salt(id)
                .animated(true)
                .max_width(f32::INFINITY)
                // .max_height(400.)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {
                ui.set_width(ui.available_width());
                let max_msg_width = ui.available_width() / 1.2;
                let fixed_height = 50.0;
                let mut count = 0;

                // Start with the history as the base for combined messages
                let mut combined_messages = self.history.clone(); // Clone history only once

                // If there's a buffer, add it as a temporary entry
                if !self.buffer.is_empty() {
                    if let Some(last) = combined_messages.last_mut() {
                        // Temporarily append the buffer to the last client message if applicable
                        if last.from == "Client" {
                            last.message.push_str(&self.buffer);
                        } else {
                            combined_messages.push(History {
                                from: "Client".to_string(),
                                message: self.buffer.clone(),
                                timestamp: chrono::Local::now().to_rfc3339(),
                            });
                        }
                    } else {
                        // If no messages exist, the buffer is the first entry
                        combined_messages.push(History {
                            from: "Client".to_string(),
                            message: self.buffer.clone(),
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                    }
                }

                // Render combined messages
                for item in &combined_messages {
                    count += 1;
                    let is_message_from_myself = if item.from.eq("You"){ true } else { false };
    
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
                                ui.set_max_width(max_msg_width);
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
    
                                    let (from, txt) = if item.from.eq("You"){
                                        (
                                            RichText::new("Command Sent:").strong().monospace().color(Color32::LIGHT_BLUE),
                                            RichText::new(item.message.clone()).strong().monospace()
                                        )
                                    }else {
                                        (
                                            RichText::new("Client Response:").strong().monospace().color(Color32::LIGHT_BLUE),
                                            RichText::new(item.message.clone()).strong().monospace()
                                        )
                                    };
                                    
    
                                    if is_message_from_myself {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::RightToLeft,
                                            Align::Min,
                                        ), |ui| {
                                            Button::new(from)
                                                .fill(Color32::TRANSPARENT)
                                                .min_size(Vec2::new(30.0, 20.0))
                                                .frame(false)
                                                .sense(Sense::hover())
                                                .ui(ui);

                                            ui.add_space(max_msg_width / 1.1);

                                            let copy_btn = Button::new(RichText::new("🗐").weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui)
                                                .on_hover_text(RichText::new("Copy Command"));

                                            if copy_btn.clicked(){
                                                ui.ctx().copy_text(item.message.to_string());
                                            }
                                        });
                                    } else {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::LeftToRight,
                                            Align::Min,
                                        ), |ui| {
                                            Button::new(from)
                                                .fill(Color32::TRANSPARENT)
                                                .min_size(Vec2::new(30.0, 20.0))
                                                .frame(false)
                                                .sense(Sense::hover())
                                                .ui(ui);

                                            ui.add_space(max_msg_width / 1.1);
                                            let btn = Button::new(RichText::new("🗐").small().weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if btn.clicked(){
                                                ui.ctx().copy_text(item.message.clone());
                                            }
                                        });
                                    }
                                    note_frame.show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            let style = ui.style_mut();
                                            style.visuals.widgets.inactive.rounding = Rounding::same(2.0);
                                            let mut layouter = |ui: &Ui, string: &str, _: f32| {
                                                let mut layout_job: eframe::egui::text::LayoutJob =
                                                    highlight(ui.ctx(), ui.style(), &CodeTheme::dark(12.), string, "bash".into()); // || "zsh".into()
                                                layout_job.wrap.max_width = ui.available_width()/1.1;
                                                ui.fonts(|f| f.layout_job(layout_job))
                                            };

                                            TextEdit::singleline(&mut txt.text())
                                                .id_salt(format!("TextEdit-{:?}-{:?}-{:?}", self.client.client_hash, count, item.message.clone()))
                                                .frame(false)
                                                .layouter(&mut layouter)
                                                .min_size(Vec2::new(ui.available_width(), 30.))
                                                .ui(ui);
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

                // After rendering, process the buffer
                if self.buffer.ends_with("DONE") {
                    if let Some(last) = self.history.last_mut() {
                        if last.from == "Client" {
                            last.message.push_str(&self.buffer);
                        } else {
                            self.history.push(History {
                                from: "Client".to_string(),
                                message: self.buffer.clone(),
                                timestamp: chrono::Local::now().to_rfc3339(),
                            });
                        }
                    } else {
                        self.history.push(History {
                            from: "Client".to_string(),
                            message: self.buffer.clone(),
                            timestamp: chrono::Local::now().to_rfc3339(),
                        });
                    }
                    self.buffer.clear();
                }
            });
        });

    }

}

impl ClientHandler for ConnectedClient {
    fn connect(&mut self) { }

    fn export_logs(&mut self, history: Vec<History>) {
        let id = self.id.clone();
        spawn_local(async move {
            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("history", Some(history.clone())).await.unwrap();
            let query = "UPDATE $id SET command_history += $history";
            let update_history: Result<Response, surrealdb::Error> = DATABASE
                .query(query)
                .await;

            info!("History Response: {update_history:?}");
            info!("History: {:#?}", history.clone());
        });
     }

    fn delete_client(&mut self) {
        let id = self.id.clone();
        spawn_local(async move {
            let update_history: Result<Option<Record>, surrealdb::Error> = DATABASE
                .delete((CONNECTED_CLIENT_TABLE, id.key().to_string()))
                .await;

            info!("History: {update_history:#?}");
        });
     }
}

pub fn serialize_system_info(system_info: &SystemInformation) -> Option<Vec<u8>> {
    if let Ok(data) = bincode::serialize(system_info){
        Some(data)
    } else { None }
}

pub fn deserialize_system_info(bytes: &[u8]) -> Option<SystemInformation> {
    if let Ok(data) = bincode::deserialize(bytes){
        Some(data)
    } else { None }
}

pub fn deserializer<T: Serialize + for<'a> Deserialize<'a> + 'static >(bytes: &[u8]) -> Option<T> {
    if let Ok(data) = bincode::deserialize(bytes){
        Some(data)
    } else { None }
}

fn normalize(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}

pub fn deserialize_command(bytes: &[u8]) -> Option<Cmd> {
    if let Ok(cmd) = bincode::deserialize(bytes){
        Some(cmd)
    }else{ None }
}

pub fn serialize_command(bytes: &Cmd) -> Vec<u8> {
    bincode::serialize(bytes).expect("Failed to deserialize Cmd")
}