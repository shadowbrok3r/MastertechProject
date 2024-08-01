use tui_input::Input;
use ewebsock::{WsEvent, WsReceiver, WsSender};
// use eframe::egui::{Key, Ui};
// use log::info;
// use ratatui::{
    //     layout::{Alignment, Constraint, Layout, Rect},
    //     style::{Color, Modifier, Style, Stylize},
    //     terminal::Frame,
    //     text::{Line, Span, Text},
    //     widgets::{block::{Position, Title}, Block, Borders, List, ListItem, Paragraph},
    // };
    // use ratatui::symbols::border;

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
    /// Position of cursor in the editor area.
    pub character_index: usize,
    /// Current value of the input box
    pub input: Input,
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
            input: Input::default(),
            input_mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
        }
    }
}
