
use egui::Ui;
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
    
    pub fn ui(&mut self, ui: &mut Ui, frame: &mut Frame, area: Rect) {
        if ui.text_edit_singleline(&mut self.text_to_send).lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.text_to_send)));
        }

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

        let block = Block::default()
            .title(Title::from("MasterTech Web Console").alignment(Alignment::Center))
            .title(
                Title::from("X")
                    .alignment(Alignment::Center)
                    .position(Position::Bottom),
            )
            .borders(Borders::ALL)
            .border_set(border::THICK)
            .white().bg(Color::Black);

        let para = Paragraph::new(text)
            .centered()
            .block(block)
            .cyan()
            .on_black();

        // for event in &self.events {
        //     ui.label(format!("{event:?}"));
        // }
        frame.render_widget(para, area);
    }
}


pub fn ui(f: &mut Frame, app: &TerminalFrontend) {
    let vertical = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
    ]);
    let [help_area, input_area, messages_area] = vertical.areas(f.size());

    let (msg, style) = match app.input_mode {
        InputMode::Normal => (
            vec![
                "Press ".into(),
                "q".bold(),
                " to exit, ".into(),
                "e".bold(),
                " to start editing.".bold(),
            ],
            Style::default().add_modifier(Modifier::RAPID_BLINK),
        ),
        InputMode::Editing => (
            vec![
                "Press ".into(),
                "Esc".bold(),
                " to stop editing, ".into(),
                "Enter".bold(),
                " to record the message".into(),
            ],
            Style::default(),
        ),
    };
    let text = Text::from(Line::from(msg)).patch_style(style);
    let help_message = Paragraph::new(text);
    f.render_widget(help_message, help_area);

    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(Block::bordered().title("Input"));
    f.render_widget(input, input_area);
    match app.input_mode {
        InputMode::Normal =>
            // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
            {}

        InputMode::Editing => {
            // Make the cursor visible and ask ratatui to put it at the specified coordinates after
            // rendering
            #[allow(clippy::cast_possible_truncation)]
            f.set_cursor(
                // Draw the cursor at the current position in the input field.
                // This position is can be controlled via the left and right arrow key
                input_area.x + app.character_index as u16 + 1,
                // Move one line down, from the border to the input line
                input_area.y + 1,
            );
        }
    }

    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let content = Line::from(Span::raw(format!("{i}: {m}")));
            ListItem::new(content)
        })
        .collect();
    let messages = List::new(messages).block(Block::bordered().title("Messages"));
    f.render_widget(messages, messages_area);
}