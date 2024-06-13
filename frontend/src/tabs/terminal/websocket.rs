
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
            self.ws_sender
                .send(WsMessage::Text(std::mem::take(&mut self.text_to_send)));
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

/*
    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
    }

    /// Returns the byte index based on the character position.
    ///
    /// Since each character in a string can be contain multiple bytes, it's necessary to calculate
    /// the byte index based on the index of the character.
    fn byte_index(&mut self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            // Method "remove" is not used on the saved text for deleting the selected char.
            // Reason: Using remove on String works on bytes instead of the chars.
            // Using remove would require special care because of char boundaries.

            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            // Getting all characters before the selected character.
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            // Getting all characters after selected character.
            let after_char_to_delete = self.input.chars().skip(current_index);

            // Put all characters together except the selected one.
            // By leaving the selected one out, it is forgotten and therefore deleted.
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn submit_message(&mut self) {
        self.messages.push(self.input.clone());
        self.input.clear();
        self.reset_cursor();
    }

*/
// pub fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: TerminalFrontend) -> io::Result<()> {
//     terminal.draw(|f| ui(f, &app))?;
//     if let Event::Key(key) = event::read()? {
//         match app.input_mode {
//             InputMode::Normal => match key.code {
//                 KeyCode::Char('e') => {
//                     app.input_mode = InputMode::Editing;
//                 }
//                 KeyCode::Char('q') => {
//                     return Ok(());
//                 }
//                 _ => {}
//             },
//             InputMode::Editing if key.kind == KeyEventKind::Press => match key.code {
//                 KeyCode::Enter => app.submit_message(),
//                 KeyCode::Char(to_insert) => {
//                     app.enter_char(to_insert);
//                 }
//                 KeyCode::Backspace => {
//                     app.delete_char();
//                 }
//                 KeyCode::Left => {
//                     app.move_cursor_left();
//                 }
//                 KeyCode::Right => {
//                     app.move_cursor_right();
//                 }
//                 KeyCode::Esc => {
//                     app.input_mode = InputMode::Normal;
//                 }
//                 _ => {}
//             },
//             InputMode::Editing => {}
//         }
//     }
//     Ok(())
// }

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