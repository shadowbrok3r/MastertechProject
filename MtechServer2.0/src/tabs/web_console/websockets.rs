use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use database::{schema::{ConnectedClient, Node, Record, CONNECTED_CLIENT_TABLE}, DATABASE};
use regex::Regex;
use core::f32;
use std::{collections::{HashMap, VecDeque}, fmt::Display};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use displays::{channel_manager::ChannelManager, virtual_filesystem::{FileSystem, FileSystemActionHandler}, Cmd, FileSystemAction};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use egui_extras::{syntax_highlighting::{highlight, CodeTheme}, Size, StripBuilder};
use surrealdb::Response;
use bincode::serialize;
use web_time::Instant;
use log::{error, info};

use super::charts::LinePlot;

pub trait ClientHandler { 
    fn connect(&mut self);
    fn export_logs(&mut self, history: Vec<String>);
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

    pub events: Vec<WsEvent>,
    pub input: String,
    pub messages: Vec<String>,

    pub cpu_clock: VecDeque<f32>,
    pub temps: VecDeque<HashMap<String, f32>>,
    pub cpu_percentage: VecDeque<f32>,
    pub ram_usage: VecDeque<f32>,

    pub sysinfo: Option<SystemInformation>,
    pub history: Vec<String>,
    pub loading: bool,
    pub timeout_counter: Instant,
    pub toolbox: FileSystem,
    pub state: WsDisplayState,
    pub explorer: FileSystem,
    pub path_edit: String,
    pub current_path: String,
    pub interactive: bool,
    pub history_idx: usize,
    pub my_history: Vec<String>
}

impl WebSocketClient{
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, client: ConnectedClient, toolbox: FileSystem) -> Self {
        let display_state_channel = <WsDisplayState>::create_unbounded_channel();
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        
        Self{
            client,
            ws_sender,
            ws_receiver,
            send_cmd_tx, 
            send_cmd_rx,
            receive_cmd_tx, 
            receive_cmd_rx,
            
            events: Default::default(),
            input: String::new(),
            messages: Vec::new(),
            display_state_channel,
            sysinfo: None,
            cpu_clock: VecDeque::new(),
            cpu_percentage: VecDeque::new(),
            ram_usage: VecDeque::new(),
            history: Vec::new(),
            temps: VecDeque::new(),
            loading: false, 
            timeout_counter: Instant::now(),
            toolbox,
            state: WsDisplayState::Shell,
            explorer: FileSystem::new(),
            path_edit: String::new(),
            current_path: String::new(),
            interactive: false,
            history_idx: 0,
            my_history: Vec::new()
        }
    }
    
    pub fn receive(&mut self) {
        while let Some(event) = self.ws_receiver.try_recv() { self.events.push(event); }
        // if self.timeout_counter.elapsed().as_secs() > 10 { info!("Its been over 10 seconds since last ping"); }
        // info!("Timer: {:?}", self.timeout_counter.elapsed().as_secs());

        for event in &self.events {
            match event{
                WsEvent::Message(msg) => {
                    match msg{
                        WsMessage::Binary(bin) => {
                            if let Some(sysinfo) = deserializer::<SystemInformation>(bin){
                                // info!("Got sysinfo");
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
                                    let msg = String::from_utf8_lossy(&bin.clone()).to_string();
                                    let regex = Regex::new("Tron.*complete");
                                    match regex {
                                        Ok(r) => {
                                            if r.is_match(&msg) {
                                                self.interactive = false;

                                            }
                                        },
                                        Err(e) => error!("Error with regex: {e:?}"),
                                    }
                                }

                                if bin.len() > 0 {
                                    self.loading = false;
                                    self.history.push(String::from_utf8_lossy(&bin).to_string());
                                }
                            }
                        },
                        WsMessage::Text(txt) => {
                            self.loading = false;
                            info!("Text data: {txt:#?}");
                            self.history.push(txt.clone());
                        },
                        _ => {}
                    }
                },
                WsEvent::Opened => self.history.push("Connection Opened".to_string()),
                WsEvent::Closed => self.history.push("Connection Closed".to_string()),
                WsEvent::Error(e) => self.history.push(e.clone()),
            }
        }
        
        if let Ok(state) = self.display_state_channel.1.try_recv() {
            self.state = state;
        }

        // Here we will handle commands we are going to SEND to Mastertech
        if let Ok(command) = self.send_cmd_rx.try_recv() {
            match command {
                Cmd::LiveData => todo!(),
                Cmd::Command => todo!(),
                Cmd::Tuneup => todo!(),
                Cmd::Cps => todo!(),
                Cmd::Qc => todo!(),
                Cmd::SfcScan => todo!(),
                Cmd::DismScan => todo!(),
                Cmd::ChkDsk => todo!(),
                Cmd::Mbr2Gpt => todo!(),
                Cmd::TaskManager => todo!(),
                Cmd::UninstallProgram(_) => todo!(),
                Cmd::PullKeys(_) => todo!(),
                Cmd::PullTicket(_) => todo!(),
                Cmd::InteractiveInput(_) => todo!(),
                Cmd::CopyTools(_) => todo!(),
                Cmd::QuitInteractive => todo!(),
                Cmd::ReadEvents => todo!(),
                Cmd::FileSystemAction(ref action) => {
                    let handle_fs_action = |prefix: &str, tx: Sender<Node>, fs_action: FileSystemAction | {
                        self.ws_sender.send(WsMessage::Binary(serialize(&command).unwrap()));
                    }; // this needs to move into the 'receive cmd rx 
                    // and maybe this should return something i can use out here..

                    self.explorer.handle_filesystem_action(action, Some(Box::new(handle_fs_action)));

                    let execute = &self.explorer.execute_file;
                    if !execute.is_empty() {
                        match serialize(&Cmd::FileSystemAction(FileSystemAction::Execute(execute.clone()))){
                            Ok(bytes) => {

                                self.ws_sender.send(WsMessage::Binary(bytes));
                            },
                            Err(e) => self.history.push(e.to_string()),
                        } 
                    }
                    match serialize(&command){
                        Ok(bytes) => {
                            if let Cmd::FileSystemAction(FileSystemAction::Execute(ref file)) = command {
                                if !file.is_empty() {
                                    self.explorer.execute_file.clear();
                                    self.history.push("Switching to interactive mode".to_string());
                                    self.interactive = true;
                                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                                }
                            }
                            self.ws_sender.send(WsMessage::Binary(bytes));
                        },
                        Err(e) => self.history.push(e.to_string()),
                    };   
                }
                Cmd::Quit => {
                    self.interactive = false;
                    self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Quit)));
                },
                Cmd::None => todo!(),
                
                _ => {}
            }
        }

        // Here we will handle commands we receive from Mastertech
        if let Ok(command) = self.receive_cmd_rx.try_recv() {
            match command {
                Cmd::LiveData => todo!(),
                Cmd::Command => todo!(),
                Cmd::Tuneup => todo!(),
                Cmd::Cps => todo!(),
                Cmd::Qc => todo!(),
                Cmd::SfcScan => todo!(),
                Cmd::DismScan => todo!(),
                Cmd::ChkDsk => todo!(),
                Cmd::Mbr2Gpt => todo!(),
                Cmd::TaskManager => todo!(),
                Cmd::FileSystemAction(file_system_action) => todo!(),
                Cmd::UninstallProgram(_) => todo!(),
                Cmd::PullKeys(_) => todo!(),
                Cmd::PullTicket(_) => todo!(),
                Cmd::InteractiveInput(_) => todo!(),
                Cmd::CopyTools(_) => todo!(),
                Cmd::QuitInteractive => todo!(),
                Cmd::ReadEvents => todo!(),
                Cmd::Quit => todo!(),
                Cmd::None => todo!(),
            }
        }

        self.events.clear();
    }
    
    pub fn show(&mut self, ui: &mut Ui) { // , add_contents: impl FnOnce(&mut Ui)
        self.receive();
        ui.set_min_height(400.0);

        StripBuilder::new(ui)
            .size(Size::exact(25.0)) // .sizes(size, strip_count)
            .size(Size::exact(25.0))
            .size(Size::remainder().at_most(400.))
            .vertical(|mut strip| 
        {
            strip.strip(|strip| 
            {
                let count = if self.interactive { 5 } else { 4 };
                strip.sizes(Size::remainder(), count)
                    .horizontal(|mut s| 
                {
                    s.cell(|ui|{
                        if Button::new(RichText::new("ToolBox").color(Color32::LIGHT_RED)).ui(ui).clicked(){
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                        }
                    });
                    s.cell(|ui|{
                        if Button::new(RichText::new("Explorer").color(Color32::LIGHT_RED)).ui(ui).clicked(){
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Explorer);
                            // if we are already in an interactive mode, then we dont want to quit that session,
                            if !self.interactive {
                                if self.current_path.is_empty() {
                                    let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory("current".to_string())));
                                } else {
                                    let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory(self.explorer.current_prefix.clone())));
                                }
                            }
                        }
                    });
                    s.cell(|ui|{
                        if Button::new(RichText::new("Charts").color(Color32::LIGHT_RED)).ui(ui).clicked(){
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::LiveStats);
                            let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                        }
                    });
                    s.cell(|ui|{
                        if Button::new(RichText::new("Shell").color(Color32::LIGHT_RED)).ui(ui).clicked(){
                            let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                        }
                    });
                    if self.interactive && count == 5 {
                        s.cell(|ui|{
                            if Button::new(RichText::new("Quit").color(Color32::RED)).ui(ui).clicked(){
                                self.send_cmd_tx.try_send(Cmd::Quit);
                            }
                        });
                    }
                });
            });
            strip.strip(|strip| 
            {
                if let WsDisplayState::Shell = self.state {
                    strip.sizes(Size::remainder(), 6)
                        .horizontal(|mut s| 
                    {
                        s.cell(|ui|{
                            if Button::new("Tuneup").ui(ui).clicked(){
                                let _ = self.send_cmd_tx.try_send(Cmd::Tuneup);
                                // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Tuneup)));
                                // self.history.push(format!("You\nCommand::Tuneup"));
                            }
                        });
                        s.cell(|ui|{
                            if Button::new("CPS").ui(ui).clicked(){
                                let _ = self.send_cmd_tx.try_send(Cmd::Cps);
                                // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Cps)));
                                // self.history.push(format!("You\nCommand::Cps\nChecking current antivirus"));
                                self.input = "SELECT * FROM Win32_OperatingSystem".to_string();
                            }
                        });
                        s.cell(|ui|{
                            if Button::new("SFC").ui(ui).clicked(){
                                let _ = self.send_cmd_tx.try_send(Cmd::SfcScan);
                                // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::SfcScan)));
                                // self.history.push(format!("You\nCommand::SfcScan"));
                                self.input = "sfc /scannow".to_string();
                            }
                        });
                        s.cell(|ui|{
                            if Button::new("Dism").ui(ui).clicked(){
                                let _ = self.send_cmd_tx.try_send(Cmd::DismScan);
                                // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::DismScan)));
                                // self.history.push(format!("You\nCommand::DismScan"));
                                self.input = "dism /online /cleanup-image /scanhealth\ndism /online /cleanup-image /checkhealth\ndism /online /cleanup-image /restorehealth".to_string();
                            }
                        });
                        s.cell(|ui|{
                            if Button::new("Chkdsk").ui(ui).clicked(){
                                let _ = self.send_cmd_tx.try_send(Cmd::ChkDsk);
                                // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::ChkDsk)));
                                // self.history.push(format!("You\nCommand::ChkDsk"));
                                self.input = "chkdsk /f /x /r".to_string();
                                
                            }
                        });
                        s.cell(|ui|{
                            if Button::new("Mbr2Gpt").ui(ui).clicked(){
                                let _ = self.send_cmd_tx.try_send(Cmd::Mbr2Gpt);
                                // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Mbr2Gpt)));
                                // self.history.push(format!("You\nCommand::Mbr2Gpt"));
                                self.input = "mbr2gpt /Convert /AllowFullOS /disk:0".to_string();
                            }
                        });
                    });
                }
            });
            strip.cell(|ui | 
            {
                match self.state {
                    WsDisplayState::LiveStats => self.show_live_stats(ui),
                    WsDisplayState::Explorer => self.show_explorer(ui),
                    WsDisplayState::ToolBox => self.show_tool_box(ui),
                    WsDisplayState::Shell => self.show_shell(ui),
                }
            });
        });
    }

    fn show_live_stats(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            if let Some(sysinfo) = &self.sysinfo {
                let _normalized_temps: Vec<f32> = sysinfo.component_temps.values().map(|&temp| normalize(temp, 0.0, 100.0)).collect();

                // if self.cpu_percentage.len() < 30
                    // || self.component_temps.len() < 30
                    // || self.cpu_clock.len() < 30
                    // || self.ram_usage.len() < 30 {

                    // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                // } else {
                //     self.cpu_percentage.pop_front();
                //     self.cpu_percentage.push_back(normalized_cpu_percentage);
                //     self.cpu_clock.pop_front();
                //     self.cpu_clock.push_back(sysinfo.cpu_clock);
                //     self.ram_usage.pop_front();
                //     self.ram_usage.push_back(normalized_ram_usage);
                //     // self.component_temps.pop_front();
                //     // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                // }

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

    fn show_explorer(&mut self, ui: &mut Ui) {
        let id = Id::new(format!("file_browser_top-{:?}", self.client.client_hash));
        TopBottomPanel::top(id).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let response = TextEdit::singleline(&mut self.explorer.current_prefix)
                    .id(Id::new(format!("path_edit-{:?}", self.client.client_hash)))
                    .cursor_at_end(true)
                    .desired_width(ui.available_width() / 1.1)
                    .ui(ui);

                if response.lost_focus() {
                    info!("Lost focus on self.path_edit");
                    let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory(self.explorer.current_prefix.clone())));
                }
                let home_res = ui.button("🏠").on_hover_text("Home");
                if home_res.clicked(){
                    let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory("current".to_string())));
                }

                let parent_res = ui.button("⬆").on_hover_text("Parent Folder");
                if parent_res.clicked() {
                    // if self.explorer.navigate_up() {

                    // }
                    // let _ = self.send_cmd_tx.try_send(Cmd::EnterDirectory(self.explorer.current_prefix.clone()));
                }
            });
        });
        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            self
                .explorer
                .display_directory_contents(
                    ui, 
                    self
                        .explorer
                        .get_current_folder()
                        .unwrap_or(&self.explorer.root)
                );

            if self.explorer.open_folder {
                self.explorer.open_folder = false;
                let new_dir = self.explorer.current_prefix.clone();
                match serialize(&Cmd::FileSystemAction(FileSystemAction::EnterDirectory(new_dir.clone()))){
                    Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                    Err(e) => self.history.push(e.to_string()),
                }
            }


        });
    }

    fn show_tool_box(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            self.toolbox.display(ui);
        });
    }

    fn show_shell(&mut self, ui: &mut Ui) {
        let avail_size = ui.available_size();
        // info!("avail_size: {:?}", avail_size);
        ui.allocate_ui(Vec2::new(avail_size.x, avail_size.y), |ui| {
            let id = Id::new(format!("scroll_area-{:?}", self.client.client_hash));
            ScrollArea::vertical()
                .id_salt(id)
                .animated(true)
                .max_width(f32::INFINITY)
                .max_height(400.)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {
                ui.set_width(ui.available_width());
                let max_msg_width = ui.available_width() / 1.5;
                let fixed_height = 50.0;
                let mut count = 0;
                for item in self.history.iter(){
                    count += 1;
                    let is_message_from_myself = if item.contains("You"){ true } else { false };
    
                    // Messages from the user are right-aligned.
                    let layout = 
                        if is_message_from_myself { Layout::top_down(Align::Max)} 
                        else { Layout::top_down(Align::Min)};
    
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
                                ui.set_min_width(ui.available_size_before_wrap().x - 15.0);
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
    
                                    let (from, txt) = if item.contains("You"){
                                        let text: (&str, &str) = item.split_once("\n").unwrap_or(("", ""));
                                        let cmd = text.1;
                                        (
                                            RichText::new("Command Sent:").strong().monospace().color(Color32::LIGHT_BLUE),
                                            RichText::new(cmd).strong().monospace()
                                        )
                                    }else {
                                        (
                                            RichText::new("Client Response:").strong().monospace().color(Color32::LIGHT_BLUE),
                                            RichText::new(item).strong().monospace()
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
                                                .sense(Sense::hover())
                                                .ui(ui);

                                            ui.add_space(max_msg_width / 1.1);

                                            let btn = Button::new(RichText::new("🗐").small().weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if btn.clicked(){
                                                ui.ctx().copy_text(item.clone());
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
                                                .sense(Sense::hover())
                                                .ui(ui);
                                            ui.add_space(max_msg_width / 1.1);
                                            let btn = Button::new(RichText::new("🗐").small().weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if btn.clicked(){
                                                ui.ctx().copy_text(item.clone());
                                            }
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
                                                .id_salt(format!("TextEdit-{:?}-{:?}-{:?}", self.client.client_hash, count, item.clone()))
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
            // ui.add_space(avail_size.y);
            ui.vertical_centered_justified(|ui: &mut eframe::egui::Ui| {
                let mut theme = CodeTheme::from_memory(ui.ctx(), ui.style());
                ui.collapsing(format!("Theme-{:?}", self.client.client_hash), |ui| {
                    ui.group(|ui| {
                        theme.ui(ui);
                        theme.clone().store_in_memory(ui.ctx());
                    });
                });
                
                let mut layouter = |ui: &Ui, string: &str, wrap_width: f32| {
                    let mut layout_job =
                        highlight(ui.ctx(), ui.style(), &theme, string, "bash".into()); // || "zsh".into()
                    layout_job.wrap.max_width = wrap_width;
                    ui.fonts(|f| f.layout_job(layout_job))
                };
                let text_edit = TextEdit::singleline(&mut self.input).hint_text("USE WISELY").layouter(&mut layouter).ui(ui);
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
                        self.input = history.clone();
                    }
                } 
                if up_press {
                    if self.history_idx > 0 {
                        self.history_idx -= 1;
                    }
                    if let Some(history) = self.my_history.get(self.history_idx){
                        self.input = history.clone();
                    }
                }

                if text_edit.lost_focus() && key_press && !self.interactive{
                    self.loading = true;
                    text_edit.request_focus();
                    self.history.push(format!("You\n{}", self.input.clone()));
                    self.my_history.push(self.input.clone());
                    self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));
                } else if text_edit.lost_focus() && key_press && self.interactive { 
                    text_edit.request_focus();
                    self.history.push(format!("You\n{}", self.input.clone()));
                    self.my_history.push(self.input.clone());
                    match serialize(&Cmd::InteractiveInput(std::mem::take(&mut self.input))){
                        Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                        Err(e) => self.history.push(e.to_string()),
                    } 
                }
            });
        });

    }
}

impl ClientHandler for ConnectedClient {
    fn connect(&mut self) { }

    fn export_logs(&mut self, history: Vec<String>) {
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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SystemInformation {
    /// Live CPU usage as a percentaget
    pub cpu_percentage: f32,
    /// Live CPU clock speed
    pub cpu_clock: f32,
    /// Live system temps
    pub component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: f32,
    /// Total RAM
    pub total_memory: f32,
    /// Disk usage
    pub disks: String,
    /// Name of machine
    pub name: String,
    /// Kernel version
    pub kernel_version: String,
    /// OS version
    pub os_version: String,
    /// Hostname based on DNS
    pub hostname: String,
    /// Number of Physical CPU's
    pub number_of_cpus: String,

    pub network_interfaces: HashMap<String, String>,
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
