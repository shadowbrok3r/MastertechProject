
use egui::{Key, TextEdit, Ui, Widget};
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
    
    pub fn ui(&mut self, ui: &mut Ui, area: Rect, frame: &mut Frame) {
        
        
        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }

        let mut text: String = String::new();
        for event in &self.events {
            match event{
                WsEvent::Message(msg) => {
                    match msg{
                        WsMessage::Binary(bin) => {
                            text = format!("{bin:?}");
                        },
                        WsMessage::Text(txt) => {
                            text = txt.clone();
                        },
                        _ => {}
                    }
                },
                // WsEvent::Error(_) => todo!(),
                _ => {}
            }
            
        }

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

        
        for event in &self.events {
            ui.label(format!("{event:?}"));
        }
        
        let para = Paragraph::new(text)
            .left_aligned()
            .block(block)
            .cyan()
            .on_black();

        

        frame.render_widget(para, area);
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

