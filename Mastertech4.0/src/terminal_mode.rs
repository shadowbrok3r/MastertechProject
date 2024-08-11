#[cfg(feature="term")]
use {
    ratatui::{
        backend::{Backend, CrosstermBackend},
        layout::{Constraint, Direction, Layout},
        style::{Color, Style},
        widgets::{Block, Borders, Paragraph},
        Terminal,
        crossterm::{
            event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton, MouseEvent, MouseEventKind},
            execute,
            terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        }
    },
    tui_textarea::{Input, Key, TextArea}
};
use {database::schema::{CustomerData, TicketData}, ratatui::{buffer::Buffer, layout::{Position, Rect, Size}, text::Line, widgets::Widget, Frame}, std::{io, ops::ControlFlow, rc::Rc, time::Duration}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    Selected,
    Active,
}

#[derive(Debug, Clone, Copy)]
struct Theme {
    text: Color,
    background: Color,
    highlight: Color,
    shadow: Color,
}

const DEFAULT_THEME: Theme = Theme {
    text: Color::Cyan,
    background: Color::Black,
    highlight: Color::Magenta,
    shadow: Color::DarkGray,
};

enum InputMode {
    Normal,
    Editing,
}

/// App holds the state of the application
struct App {
    
    pub ticket_data: TicketData,

    pub customer_data: CustomerData,
    /// Position of cursor in the editor area.
    character_index: usize,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Vec<String>,
}

impl App {
    fn new() -> Self {
        Self {
            ticket_data: TicketData::default(),
            customer_data: CustomerData::default(),
            input_mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
        }
    }

    // fn move_cursor_left(&mut self) {
    //     let cursor_moved_left = self.character_index.saturating_sub(1);
    //     self.character_index = self.clamp_cursor(cursor_moved_left);
    // }

    // fn move_cursor_right(&mut self) {
    //     let cursor_moved_right = self.character_index.saturating_add(1);
    //     self.character_index = self.clamp_cursor(cursor_moved_right);
    // }

    // fn enter_char(&mut self, new_char: char) {
    //     let index = self.byte_index();
    //     self.input.insert(index, new_char);
    //     self.move_cursor_right();
    // }

    // /// Returns the byte index based on the character position.
    // ///
    // /// Since each character in a string can be contain multiple bytes, it's necessary to calculate
    // /// the byte index based on the index of the character.
    // fn byte_index(&self) -> usize {
    //     self.input
    //         .char_indices()
    //         .map(|(i, _)| i)
    //         .nth(self.character_index)
    //         .unwrap_or(self.input.len())
    // }

    // fn delete_char(&mut self) {
    //     let is_not_cursor_leftmost = self.character_index != 0;
    //     if is_not_cursor_leftmost {
    //         // Method "remove" is not used on the saved text for deleting the selected char.
    //         // Reason: Using remove on String works on bytes instead of the chars.
    //         // Using remove would require special care because of char boundaries.

    //         let current_index = self.character_index;
    //         let from_left_to_current_index = current_index - 1;

    //         // Getting all characters before the selected character.
    //         let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
    //         // Getting all characters after selected character.
    //         let after_char_to_delete = self.input.chars().skip(current_index);

    //         // Put all characters together except the selected one.
    //         // By leaving the selected one out, it is forgotten and therefore deleted.
    //         self.input = before_char_to_delete.chain(after_char_to_delete).collect();
    //         self.move_cursor_left();
    //     }
    // }

    // fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
    //     new_cursor_pos.clamp(0, self.input.chars().count())
    // }

    // fn reset_cursor(&mut self) {
    //     self.character_index = 0;
    // }

    // fn submit_message(&mut self) {
    //     self.messages.push(self.input.clone());
    //     self.input.clear();
    //     self.reset_cursor();
    // }
}


pub fn run_terminal_mode() -> anyhow::Result<(), anyhow::Error> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let app = App::new();
    let res = run_app(&mut terminal, app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        log::info!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    let mut selected_button: usize = 0;
    let mut button_states = [State::Selected, State::Normal, State::Normal];

    let mut checkin_notes = TextArea::default();
    let mut recommendations = TextArea::default();
    let mut service = TextArea::default();
    let mut customer_name = TextArea::default();
    let mut customer_phone = TextArea::default();
    let mut customer_phone2 = TextArea::default();
    let mut assignee = TextArea::default();
    let mut tech = TextArea::default();

    service.set_cursor_line_style(Style::default());
    customer_name.set_cursor_line_style(Style::default());
    customer_phone.set_cursor_line_style(Style::default());
    customer_phone2.set_cursor_line_style(Style::default());
    assignee.set_cursor_line_style(Style::default());
    tech.set_cursor_line_style(Style::default());

    service.set_block(Block::default().borders(Borders::ALL).title("Service #"));
    customer_name.set_block(Block::default().borders(Borders::ALL).title("Customer Name"));
    customer_phone.set_block(Block::default().borders(Borders::ALL).title("Phone Number 1"));
    customer_phone2.set_block(Block::default().borders(Borders::ALL).title("Phone Number 2"));
    assignee.set_block(Block::default().borders(Borders::ALL).title("Assignee"));
    tech.set_block(Block::default().borders(Borders::ALL).title("Tech"));

    let mut text_areas: Vec<TextArea> = vec![
        service.clone(),
        customer_name.clone(),
        customer_phone.clone(),
        customer_phone2.clone(),
        assignee.clone(),
        tech.clone(),
    ];
    
    checkin_notes.set_cursor_line_style(Style::default());
    recommendations.set_cursor_line_style(Style::default());
    checkin_notes.set_block(Block::default().borders(Borders::ALL).title("Checkin Notes"));
    recommendations.set_block(Block::default().borders(Borders::ALL).title("Recommendations"));

    loop {
        terminal.draw(|f| ui(f, button_states, &app, text_areas.clone(), checkin_notes.clone(), recommendations.clone()))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if key.kind != event::KeyEventKind::Press {
                    continue;
                }
                recommendations.input(key);
                checkin_notes.input(key);
                for text_input in text_areas.iter_mut() {
                    text_input.input(key);
                }
                if handle_key_event(key, &mut app,  &mut button_states, &mut selected_button).is_break() {
                    break;
                }
            }
            Event::Mouse(mouse) => {
                handle_mouse_event(mouse, &mut button_states, &mut selected_button);
            },
            _ => {}
        }
    }
    Ok(())
}

fn ui(
    frame: &mut Frame, 
    states: [State; 3],
    app: &App, 
    text_areas: Vec<TextArea>,
    checkin_notes: TextArea,
    recommendations: TextArea
) {

    // let checkin_notes_in = checkin_notes.input(app.ticket_data.checkin_notes);
    // // recommendations.input(app.ticket_data.r)
    // let service_in = service.input(app.ticket_data.service_number);
    // let customer_name_in = customer_name.input(app.customer_data.name);
    // let customer_phone_in = customer_phone.input(app.customer_data.phone_number);
    // let customer_phone2_in = customer_phone2.input(app.customer_data.phone_number_2);
    // assignee.input(app.ticket_data.checkin_notes)
    // tech.input(app.ticket_data.checkin_notes)

    // Create main layout
    let size = frame.area();
    let vertical_chunks: Rc<[Rect]> = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(20),
        ].as_ref())
        .split(size);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ].as_ref())
        .split(vertical_chunks[0]);

    let top_left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ].as_ref())
        .split(top_chunks[0]);

    let top_right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ].as_ref())
        .split(top_chunks[1]);

    let bottom_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ].as_ref())
        .split(vertical_chunks[3]);

    let button_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ].as_ref())
        .split(bottom_chunks[1]);


    for (i, area) in top_left_chunks.iter().chain(top_right_chunks.iter()).enumerate() {
        frame.render_widget(&text_areas[i], *area);
    }

    // Render large text areas
    frame.render_widget(&checkin_notes.to_owned(), vertical_chunks[1]);
    frame.render_widget(&recommendations.to_owned(), vertical_chunks[2]);
    render_buttons(frame, button_layout[1], states);
}

fn render_buttons(frame: &mut Frame<'_>, area: Rect, states: [State; 3]) {
    let horizontal = Layout::horizontal([
        Constraint::Length(15),
        Constraint::Length(15),
        Constraint::Length(15),
        Constraint::Min(0), // ignore remaining space
    ]);
    let [red, green, blue, _] = horizontal.areas(area);

    frame.render_widget(Button::new("Get Keys").theme(RED).state(states[0]), red);
    frame.render_widget(Button::new("Pull Ticket").theme(GREEN).state(states[1]), green);
    frame.render_widget(Button::new("Submit").theme(BLUE).state(states[2]), blue);
}


fn handle_key_event(
    key: event::KeyEvent,
    app: &mut App,
    button_states: &mut [State; 3],
    selected_button: &mut usize,
) -> ControlFlow<()> {
    // match app.input_mode {
    //     InputMode::Normal => match key.code {
    //         KeyCode::Char('e') => {
    //             app.input_mode = InputMode::Editing;
    //         }
    //         KeyCode::Char('q') => {
    //             return ControlFlow::Continue(());
    //         }
    //         _ => {}
    //     },
    //     InputMode::Editing if key.kind == KeyEventKind::Press => match key.code {
    //         KeyCode::Enter => app.submit_message(),
    //         KeyCode::Char(to_insert) => {
    //             app.enter_char(to_insert);
    //         }
    //         KeyCode::Backspace => {
    //             app.delete_char();
    //         }
    //         KeyCode::Left => {
    //             app.move_cursor_left();
    //         }
    //         KeyCode::Right => {
    //             app.move_cursor_right();
    //         }
    //         KeyCode::Esc => {
    //             app.input_mode = InputMode::Normal;
    //         }
    //         _ => {}
    //     },
    //     InputMode::Editing => {}
    // }
    
    match key.code {
        KeyCode::Esc => return ControlFlow::Break(()),
        KeyCode::Left | KeyCode::Char('h') => {
            button_states[*selected_button] = State::Normal;
            *selected_button = selected_button.saturating_sub(1);
            button_states[*selected_button] = State::Selected;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            button_states[*selected_button] = State::Normal;
            *selected_button = selected_button.saturating_add(1).min(2);
            button_states[*selected_button] = State::Selected;
        }
        KeyCode::Char(' ') => {
            if button_states[*selected_button] == State::Active {
                button_states[*selected_button] = State::Normal;
            } else {
                button_states[*selected_button] = State::Active;
            }
        }
        _ => (),
    }
    ControlFlow::Continue(())
}

fn handle_mouse_event(
    mouse: MouseEvent,
    button_states: &mut [State; 3],
    selected_button: &mut usize,
) {
        // if let Event::Mouse(mouse) = event::read()? {
        //     if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
        //         let pos = Position::from((mouse.column as u16, mouse.row as u16));
        //         for (i, area) in button_layout.iter().enumerate() {
        //             if area.contains(pos) {
        //                 button_states[i] = State::Active;
        //             }
        //         }
        //         if bottom_chunks[1].contains(pos) {
        //             button_states[2] = State::Active;
        //         }
        //     }
        // }
    match mouse.kind {
        MouseEventKind::Moved => {
            let old_selected_button = *selected_button;
            *selected_button = match mouse.column {
                x if x < 15 => 0,
                x if x < 30 => 1,
                _ => 2,
            };
            if old_selected_button != *selected_button {
                if button_states[old_selected_button] != State::Active {
                    button_states[old_selected_button] = State::Normal;
                }
                if button_states[*selected_button] != State::Active {
                    button_states[*selected_button] = State::Selected;
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if button_states[*selected_button] == State::Active {
                button_states[*selected_button] = State::Normal;
            } else {
                button_states[*selected_button] = State::Active;
            }
        }
        _ => (),
    }
}


const BLUE: Theme = Theme {
    text: Color::Rgb(16, 24, 48),
    background: Color::Rgb(48, 72, 144),
    highlight: Color::Rgb(64, 96, 192),
    shadow: Color::Rgb(32, 48, 96),
};

const RED: Theme = Theme {
    text: Color::Rgb(48, 16, 16),
    background: Color::Rgb(144, 48, 48),
    highlight: Color::Rgb(192, 64, 64),
    shadow: Color::Rgb(96, 32, 32),
};

const GREEN: Theme = Theme {
    text: Color::Rgb(16, 48, 16),
    background: Color::Rgb(48, 144, 48),
    highlight: Color::Rgb(64, 192, 64),
    shadow: Color::Rgb(32, 96, 32),
};

#[derive(Debug, Clone)]
struct Button<'a> {
    label: Line<'a>,
    theme: Theme,
    state: State,
}


/// A button with a label that can be themed.
impl<'a> Button<'a> {
    pub fn new<T: Into<Line<'a>>>(label: T) -> Self {
        Button {
            label: label.into(),
            theme: BLUE,
            state: State::Normal,
        }
    }

    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }
}

impl<'a> Widget for Button<'a> {
    #[allow(clippy::cast_possible_truncation)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        buf.set_style(area, Style::new().bg(background).fg(text));

        // render top line if there's enough space
        if area.height > 2 {
            buf.set_string(
                area.x,
                area.y,
                "▔".repeat(area.width as usize),
                Style::new().fg(highlight).bg(background),
            );
        }
        // render bottom line if there's enough space
        if area.height > 1 {
            buf.set_string(
                area.x,
                area.y + area.height - 1,
                "▁".repeat(area.width as usize),
                Style::new().fg(shadow).bg(background),
            );
        }
        // render label centered
        buf.set_line(
            area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2,
            area.y + (area.height.saturating_sub(1)) / 2,
            &self.label,
            area.width,
        );
    }
}

impl Button<'_> {
    const fn colors(&self) -> (Color, Color, Color, Color) {
        let theme = self.theme;
        match self.state {
            State::Normal => (theme.background, theme.text, theme.shadow, theme.highlight),
            State::Selected => (theme.highlight, theme.text, theme.shadow, theme.highlight),
            State::Active => (theme.background, theme.text, theme.highlight, theme.shadow),
        }
    }
}