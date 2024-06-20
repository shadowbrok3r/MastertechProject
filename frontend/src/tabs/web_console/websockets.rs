
use std::{collections::{HashMap, VecDeque}, fmt::Display};
use egui::{Align, Color32, Direction, Key, Layout, Rounding, ScrollArea, TextEdit, Ui, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use log::info;

use crate::utilities::ColumnLayout;

use super::charts::LinePlot;

// use ratatui::{
//     layout::{Alignment, Constraint, Layout, Rect},
//     style::{Color, Modifier, Style, Stylize},
//     terminal::Frame,
//     text::{Line, Span, Text},
//     widgets::{block::{Position, Title}, Block, Borders, List, ListItem, Paragraph},
// }, symbols::border;

pub enum InputMode {
    Normal,
    Editing,
}

// ----------------------------------------------------------------------------

pub struct TerminalFrontend {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    pub events: Vec<WsEvent>,
    pub input: String,

    pub messages: Vec<String>,
    pub cpu_clock: VecDeque<f32>,
    pub component_temps: VecDeque<HashMap<String, f32>>,
    pub cpu_percentage: VecDeque<f32>,

    pub column_names: Vec<String>,
    // pub search_inputs: HashMap<String, String>,
}


impl TerminalFrontend {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        Self {
            ws_sender,
            ws_receiver,
            events: Default::default(),
            input: String::new(),
            messages: Vec::new(),

            cpu_clock: VecDeque::new(),
            component_temps: VecDeque::new(),
            cpu_percentage: VecDeque::new(),

            column_names: Vec::new()
        }
    }
    
    pub fn ui(&mut self, ui: &mut Ui) { // , area: Rect, frame: &mut Frame
        
        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }
        let mut system_info: Option<SystemInformation> = None;

        for event in &self.events {
            match event{
                WsEvent::Message(msg) => {
                    match msg{
                        WsMessage::Binary(bin) => {
                            let sys = deserialize_system_info(bin);
                            system_info = Some(deserialize_system_info(bin));

                            if self.cpu_percentage.len() < 30
                            || self.component_temps.len() < 30 
                            || self.cpu_clock.len() < 30 {
                                self.cpu_percentage.push_back(sys.cpu_percentage);
                                self.component_temps.push_back(sys.component_temps);
                                self.cpu_clock.push_back(sys.cpu_clock);
                            } else {
                                self.cpu_percentage.pop_front();
                                self.cpu_percentage.push_back(sys.cpu_percentage);
                                self.component_temps.pop_front();
                                self.component_temps.push_back(sys.component_temps);
                                self.cpu_clock.pop_front();
                                self.cpu_clock.push_back(sys.cpu_clock);
                            }
                            // ui.label(format!("{sysinfo}"));

                        },
                        WsMessage::Text(txt) => {
                            info!("Text data: {txt:#?}");
                            // text = txt.clone();
                            ui.label(txt);
                        },
                        WsMessage::Unknown(unknown) => {
                            info!("unknown data: {unknown:#?}");
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
        }  
        // let block = Block::default().on_black().bg(Color::Black)
        //     .title(Title::from("MasterTech Web Console").alignment(Alignment::Center))
        //     .title(
        //         Title::from("X")
        //             .alignment(Alignment::Center)
        //             .position(Position::Bottom),
        //     )
        //     .borders(Borders::ALL)
        //     .border_set(border::THICK)
        //     .white().bg(Color::Black);
        // let para = Paragraph::new(text)
        //     .left_aligned()
        //     .block(block)
        //     .cyan()
            // .on_black();
    }
}

impl ColumnLayout for TerminalFrontend {
    fn layout_cols(
        &mut self,
        ui: &mut egui::Ui
    ){
        ui.style_mut().visuals.window_rounding = Rounding::same(10.0);
        
        let column_width = Size::exact(450.0);
    
        ScrollArea::horizontal()
            .show_viewport(ui, |ui, _|
        {
            let x: f32 = ui.available_height() - 40.0;
            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(egui::Align::Center))
                .size(Size::exact(30.0))
                .size(Size::exact(5.0))
                .size(Size::exact(x))
                .vertical(|mut strip| 
            {
                strip
                    .strip(|strip| 
                {
                    strip
                        .sizes(column_width, self.column_names.len())
                        .horizontal( |strip| self.headers(strip));
                });
                strip.empty();
                strip
                    .strip(|strip| 
                {
                    strip
                        .sizes(column_width, self.column_names.len())
                        .horizontal( |mut strip| 
                    {
                        // self.task_columns(
                        //     strip.borrow_mut(),
                        // );
                    });
                });
            });
        });
    }

    fn columns(&mut self, s: &mut egui_extras::Strip) {
        // if let Some(sysinfo) = system_info{
        //     // let x_val = sysinfo.cpu_clock as f32;
        //     info!("length: {}", self.cpu_percentage.len());
        //     let percentages = self.cpu_percentage.make_contiguous().to_owned();
        //     let other = self.cpu_clock.make_contiguous().to_owned();
        //     LinePlot::new(&[0.0], &percentages.as_slice()).ui(ui);
        // }
        // // frame.render_widget(para, area);
        
        // let text_edit = TextEdit::singleline(&mut self.input).hint_text("Send command").ui(ui);
        // let key_press = ui.input(|i| i.key_pressed(Key::Enter));
        // if text_edit.lost_focus() && key_press
        // {
        //     text_edit.request_focus();
        //     self.ws_sender
        //         .send(WsMessage::Text(std::mem::take(&mut self.input)));
        // }
    }

    fn headers(&mut self, s: egui_extras::Strip) {
        todo!()
    }
}


pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    bincode::serialize(system_info).expect("Failed to serialize SystemInformation")
}


pub fn deserialize_system_info(bytes: &[u8]) -> SystemInformation {
    bincode::deserialize(bytes).expect("Failed to deserialize SystemInformation")
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