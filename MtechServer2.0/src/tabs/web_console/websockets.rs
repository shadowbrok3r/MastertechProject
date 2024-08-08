
use std::{collections::{HashMap, VecDeque}, fmt::Display};
use bincode::serialize;
use database::{schema::{Cmd, ConnectedClient, Record, User, CONNECTED_CLIENT_TABLE}, DATABASE};
use eframe::egui::{epaint::Shadow, Align, Button, CollapsingHeader, Color32, Direction, Frame, Key, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, Ui, Vec2, Widget};
use egui_extras::{Size, Strip};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use log::info;
use surrealdb::Response;
use wasm_bindgen_futures::spawn_local;
use web_time::Instant;

use crate::tabs::toolbox::storage_api::FileSystem;

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
    // pub client: ConnectedClient,
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
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
    pub file_system: FileSystem,
    pub client_name: String,
    pub state: WsDisplayState,
    pub explorer: FileSystem
}

impl WebSocketClient{
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, client_name: String, file_system: FileSystem) -> Self {
        Self{
            ws_sender,
            ws_receiver,
            events: Default::default(),
            input: String::new(),
            messages: Vec::new(),

            sysinfo: None,
            cpu_clock: VecDeque::new(),
            cpu_percentage: VecDeque::new(),
            ram_usage: VecDeque::new(),
            history: Vec::new(),
            temps: VecDeque::new(),
            loading: false, 
            timeout_counter: Instant::now(),
            file_system,
            client_name,
            state: WsDisplayState::Shell,
            explorer: FileSystem::new()
        }
    }
    
    pub fn handle_events(&mut self) {
        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }

        // if self.timeout_counter.elapsed().as_secs() > 10 {
        //     info!("Its been over 10 seconds since last ping");
        // }

        // info!("Timer: {:?}", self.timeout_counter.elapsed().as_secs());

        for event in &self.events {
            match event{
                WsEvent::Message(msg) => {
                    // self.connected = true;
                    match msg{
                        WsMessage::Binary(bin) => {
                            // info!("Binary: {bin:?}");
                            
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
                                if let Cmd::DirContents(paths) = cmd{
                                    self.explorer.build_file_system(paths);
                                }

                            } else{ 
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
                        WsMessage::Ping(_bytes) => {
                            self.loading = false;
                            info!("Ping");
                            self.timeout_counter = Instant::now();
                            
                        },
                        WsMessage::Pong(_bytes) => {
                            info!("Pong");
                            self.timeout_counter = Instant::now();
                        },
                        _ => {}
                    }
                },
                WsEvent::Opened => {
                    // self.connected = true;
                    self.history.push("Connection Opened".to_string());
                },
                WsEvent::Closed => {
                    self.history.push("Connection Closed".to_string());
                    // self.connected = false;
                },
                WsEvent::Error(e) => {
                    // self.connected = false;
                    self.history.push(e.clone());
                },
            }
        }
        self.events.clear();
    }
    
    pub fn show(&mut self, mut strip: Strip, name: String) {
        self.handle_events();

        strip.strip(|strip| 
        {
            strip.sizes(Size::remainder(), 4)
                .horizontal(|mut s| 
            {
                s.cell(|ui|{
                    if Button::new("ToolBox").ui(ui).clicked(){
                        self.state = WsDisplayState::ToolBox;
                    }
                });
                
                s.cell(|ui|{
                    if Button::new("Explorer").ui(ui).clicked(){
                        self.state = WsDisplayState::Explorer;
                        match serialize(&Cmd::ReadDir("current".to_string())){
                            Ok(bytes) => {
                                self.ws_sender.send(WsMessage::Binary(bytes));
                            },
                            Err(e) => self.history.push(e.to_string()),
                        }
                    }
                });

                s.cell(|ui|{
                    if Button::new("Live Data").ui(ui).clicked(){
                        self.state = WsDisplayState::LiveStats;
                        self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::LiveData)));
                    }
                });
                s.cell(|ui|{
                    if Button::new("Shell").ui(ui).clicked(){
                        self.state = WsDisplayState::Shell;
                    }
                });
            });
        });

        strip.strip(|strip| 
        {
            strip.sizes(Size::remainder(), 6)
                .horizontal(|mut s| 
            {
                s.cell(|ui|{
                    if Button::new("Tuneup").ui(ui).clicked(){
                        // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Tuneup)));
                        // self.history.push(format!("You\nCommand::Tuneup"));
                    }
                });
                s.cell(|ui|{
                    if Button::new("CPS").ui(ui).clicked(){
                        // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Cps)));
                        // self.history.push(format!("You\nCommand::Cps\nChecking current antivirus"));
                        self.input = "SELECT * FROM Win32_OperatingSystem".to_string();
                    }
                });
                s.cell(|ui|{
                    if Button::new("SFC").ui(ui).clicked(){
                        // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::SfcScan)));
                        // self.history.push(format!("You\nCommand::SfcScan"));
                        self.input = "sfc /scannow".to_string();
                    }
                });
                s.cell(|ui|{
                    if Button::new("Dism").ui(ui).clicked(){
                        // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::DismScan)));
                        // self.history.push(format!("You\nCommand::DismScan"));
                        self.input = "dism /online /cleanup-image /scanhealth\ndism /online /cleanup-image /checkhealth\ndism /online /cleanup-image /restorehealth".to_string();
                    }
                });
                s.cell(|ui|{
                    if Button::new("Chkdsk").ui(ui).clicked(){
                        // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::ChkDsk)));
                        // self.history.push(format!("You\nCommand::ChkDsk"));
                        self.input = "chkdsk /f /x /r".to_string();
                        
                    }
                });
                s.cell(|ui|{
                    if Button::new("Mbr2Gpt").ui(ui).clicked(){
                        // self.ws_sender.send(WsMessage::Binary(serialize_command(&Cmd::Mbr2Gpt)));
                        // self.history.push(format!("You\nCommand::Mbr2Gpt"));
                        self.input = "mbr2gpt /Convert /AllowFullOS /disk:0".to_string();
                    }
                });
            });
        });

        strip.cell(|ui | 
        {
            match self.state {
                WsDisplayState::LiveStats => self.show_live_stats(ui, name.clone()),
                WsDisplayState::Explorer => self.show_explorer(ui),
                WsDisplayState::ToolBox => self.show_tool_box(ui),
                WsDisplayState::Shell => self.show_shell(ui, name.clone()),
            }
        });
    }

    fn show_live_stats(&mut self, ui: &mut Ui, name: String) {
        let client_id = ui.make_persistent_id(format!("client_id {:?}", name.clone()));
        let client_header = CollapsingHeader::new("Live Stats").id_source(client_id);

        client_header.show_background(true).show_unindented(ui, |ui| 
        {
            ui.add_space(10.0);
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
                    let width = ui.available_width() / 3.0;
                    let mut cpu_usage_plot = LinePlot::new(&[0.0], &percentages.as_slice(), width);
                    let mut cpu_clock_plot = LinePlot::new(&[0.0], &clocks.as_slice(), width);
                    let mut ram_usage_plot = LinePlot::new(&[0.0], &ram.as_slice(), width);
            
                    ui.horizontal(|ui| {

                        // temps_plot.ui(ui, "System Temps", temps_plot.line("System Temps (°C)", Color32::from_rgb(255, 69, 0)));
                        cpu_usage_plot.ui(ui, "CPU Usage", cpu_usage_plot.line("CPU(%)", Color32::from_rgb(170, 10, 150)));
                        cpu_clock_plot.ui(ui, "CPU Clock", cpu_clock_plot.line("CPU (MHz)", Color32::from_rgb(21, 232, 165)));
                        ram_usage_plot.ui(ui, "RAM Usage", ram_usage_plot.line("RAM (MB)", Color32::from_rgb(0, 191, 255)));
                    });
                }
            });
            ui.add_space(10.0);
        });
    }

    fn show_explorer(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            self.explorer.display(ui);
            let new_dir = self.explorer.enter_directory.clone();
            if !new_dir.is_empty(){
                info!("New directory: {:?}", new_dir);
                match serialize(&Cmd::ReadDir(new_dir.clone())){
                    Ok(bytes) => {
                        self.ws_sender.send(WsMessage::Binary(bytes));
                    },
                    Err(e) => self.history.push(e.to_string()),
                }
            }
        });
    }

    fn show_tool_box(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            self.file_system.display(ui);
            let new_dir = self.file_system.enter_directory.clone();
            if !new_dir.is_empty(){
                info!("New directory: {:?}", new_dir);
                match serialize(&Cmd::ReadDir(new_dir.clone())){
                    Ok(bytes) => {
                        self.ws_sender.send(WsMessage::Binary(bytes));
                    },
                    Err(e) => self.history.push(e.to_string()),
                }
            }
        });
    }

    fn show_shell(&mut self, ui: &mut Ui, name: String) {
        let client_id = ui.make_persistent_id(format!("history {:?}", name.clone()));
        let scroll = CollapsingHeader::new("Shell").id_source(client_id);

        scroll.show_background(true).show_unindented(ui, |ui| 
        {
            ui.allocate_ui(Vec2::new(ui.available_width(), ui.available_height() - 20.0), |ui| {
                ScrollArea::vertical()
                    .animated(true)
                    .max_height(ui.available_height())
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
            });

            ui.vertical_centered_justified(|ui: &mut eframe::egui::Ui| {
                let text_edit = TextEdit::singleline(&mut self.input).hint_text("USE WISELY").ui(ui);
                let key_press = ui.input(|i| i.key_pressed(Key::Enter));
                if text_edit.lost_focus() && key_press {
                    self.loading = true;
                    text_edit.request_focus();
                    self.history.push(format!("You\n{}", self.input.clone()));
                    self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));
                }
            });
        });
    }
}



impl ClientHandler for ConnectedClient {
    fn connect(&mut self) { }

    fn export_logs(&mut self, history: Vec<String>) {
        let id = self.id.clone().unwrap().0;
        spawn_local(async move {
            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("history", history.clone()).await.unwrap();
            let query = "UPDATE $id SET command_history = $history";
            let update_history: Result<Response, surrealdb::Error> = DATABASE
                .query(query)
                .await;

            info!("History: {update_history:#?}");
        });
     }

     fn delete_client(&mut self) {
        let id = self.id.clone().unwrap().0;
        spawn_local(async move {
            let update_history: Result<Option<Record>, surrealdb::Error> = DATABASE
                .delete((CONNECTED_CLIENT_TABLE, id.id))
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