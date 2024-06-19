
use std::{collections::HashMap, fmt::Display};

use egui::{Key, ScrollArea, TextEdit, Ui, Widget};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use log::info;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    terminal::Frame,
    text::{Line, Span, Text},
    widgets::{block::{Position, Title}, Block, Borders, List, ListItem, Paragraph},
};
use ratatui::symbols::border;
use serde::{Deserialize, Serialize};

pub enum InputMode {
    Normal,
    Editing,
}

// ----------------------------------------------------------------------------

pub struct TerminalFrontend {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    pub events: Vec<WsEvent>,
    pub text_to_send: String,
    /// Current value of the input box
    pub input: String,
    /// Position of cursor in the editor area.
    pub character_index: usize,
    /// Current input mode
    pub input_mode: InputMode,
    /// History of recorded messages
    pub messages: Vec<String>,
}

impl TerminalFrontend {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        Self {
            ws_sender,
            ws_receiver,
            events: Default::default(),
            text_to_send: String::new(),
            input: String::new(),
            input_mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
        }
    }
    
    pub fn ui(&mut self, ui: &mut Ui) { // , area: Rect, frame: &mut Frame
        
        
        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }

        // let mut text: String = String::new();
        ScrollArea::vertical()
            .show(ui, |ui| 
        {
            for event in &self.events {
                match event{
                    WsEvent::Message(msg) => {
                        match msg{
                            WsMessage::Binary(bin) => {
                                info!("Binary data: {bin:#?}");
                                let sysinfo = deserialize_system_info(bin);
                                ui.label(format!("{sysinfo}"));

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
                    // WsEvent::Error(_) => todo!(),
                    _ => {}
                }
                
            }
        });
        

        let block = Block::default().on_black().bg(Color::Black)
            .title(Title::from("MasterTech Web Console").alignment(Alignment::Center))
            .title(
                Title::from("X")
                    .alignment(Alignment::Center)
                    .position(Position::Bottom),
            )
            .borders(Borders::ALL)
            .border_set(border::THICK)
            .white().bg(Color::Black);

        

        
        // let para = Paragraph::new(text)
        //     .left_aligned()
        //     .block(block)
        //     .cyan()
            // .on_black();

        

        // frame.render_widget(para, area);
        ui.vertical_centered(|ui| {
            let text_edit = TextEdit::singleline(&mut self.text_to_send).hint_text("Send command").ui(ui);
            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            if text_edit.lost_focus() && key_press
            {
                text_edit.request_focus();
                self.ws_sender
                    .send(WsMessage::Text(std::mem::take(&mut self.text_to_send)));
            }
        });
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
    pub cpu_clock: u64,
    /// Live system temps
    pub component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: u64,
    /// Total RAM
    pub total_memory: u64,
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