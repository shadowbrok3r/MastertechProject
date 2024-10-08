#[cfg(feature="term")]
use {
    ratatui::{
        backend::{Backend, CrosstermBackend},
        layout::{Constraint, Direction, Layout},
        style::{Color, Style},
        widgets::{Block, Borders, Paragraph},
        buffer::Buffer, crossterm::event::KeyModifiers, layout::{Position, Rect, Size}, style::Stylize, text::{Line, Span}, widgets::{BorderType, Widget}, 
        Frame,
        Terminal,
        crossterm::{
            event::{self, DisableMouseCapture, KeyEvent, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton, MouseEvent, MouseEventKind},
            execute,
            terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        }
    },
    tui_textarea::{Input, Key, TextArea},
    database::schema::{CustomerData, TicketData}, 
    std::{io, ops::ControlFlow, rc::Rc, time::Duration}
};
use {colors::{BLUE, GREEN, RED}, terminal_widgets::Button};

pub mod colors;
pub mod terminal_widgets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Normal,
    Selected,
    Active,
}

enum InputMode {
    Normal,
    Editing,
}

struct App <'a>{
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub form: Vec<&'a str>,
    input_mode: InputMode,
    should_quit: bool,
}

impl  <'a> App  <'a>{
    fn new() -> Self {
        Self {
            ticket_data: TicketData::default(),
            customer_data: CustomerData::default(),
            form: vec![ "Service #", "Customer Name", "Phone Number 1", "Phone Number 2", "Assignee", "Tech" ],
            input_mode: InputMode::Normal,
            should_quit: false,
        }
    }
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

    let mut fields = TextArea::default();
    let mut style = Style::default();
    style.fg = Some(Color::Cyan);
    fields.set_cursor_line_style(style);

    // let mut text_areas: Vec<TextArea> = vec![ service.clone(), customer_name.clone(), customer_phone.clone(), customer_phone2.clone(), assignee.clone(), tech.clone()];

    let mut text_areas = Vec::new();

    for field in app.form.iter() {
        fields.set_block(Block::default().borders(Borders::ALL).title(*field));
        text_areas.push(fields.clone());
    }
    let mut checkin_notes = TextArea::default();
    let mut recommendations = TextArea::default();
    checkin_notes.set_cursor_line_style(Style::default());
    recommendations.set_cursor_line_style(Style::default());
    checkin_notes.set_block(Block::default().borders(Borders::ALL).title("Checkin Notes"));
    recommendations.set_block(Block::default().borders(Borders::ALL).title("Recommendations"));
    let mut focused_index = 0;  // Track which text field is focused

    loop {
        let _draw_res = terminal.draw(|f| ui(f, button_states, &app, text_areas.clone(), checkin_notes.clone(), recommendations.clone()));
        if app.should_quit {
            break;
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if key.kind != event::KeyEventKind::Press {
                    continue;
                }
                if handle_key_event(key, &mut app,  &mut button_states, &mut selected_button).is_break() {
                     break; 
                }
                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Right) => {
                        log::info!("index: {:?}", focused_index);
                        if focused_index < text_areas.len() - 1 {
                            focused_index += 1;
                        }
                    }
                    (KeyModifiers::CONTROL, KeyCode::Left) => {
                        log::info!("index: {:?}", focused_index);
                        if focused_index > 0 {
                            focused_index -= 1;
                        }
                    }
                    (KeyModifiers::SHIFT, KeyCode::Left) => {
                        log::info!("index: {:?}", focused_index);
                        if button_states[selected_button] == State::Active {
                            button_states[selected_button] = State::Normal;
                        } else {
                            button_states[selected_button] = State::Active;
                        }
                    }
                    (KeyModifiers::SHIFT, KeyCode::Right) => {
                        log::info!("index: {:?}", focused_index);
                        if button_states[selected_button] == State::Active {
                            button_states[selected_button] = State::Normal;
                        } else {
                            button_states[selected_button] = State::Active;
                        }
                    }
                    _ => {}
                }
                match key.code{
                    KeyCode::Esc => break,
                    KeyCode::Char(' ') => {
                        if button_states[selected_button] == State::Active {
                            button_states[selected_button] = State::Normal;
                        } else {
                            button_states[selected_button] = State::Active;
                        }
                    }
                    _ => {},
                }
                // Only pass input to the focused text area
                if focused_index < text_areas.len() {
                    text_areas[focused_index].input_without_shortcuts(key);
                } else if focused_index == text_areas.len() {
                    checkin_notes.input_without_shortcuts(key);
                } else if focused_index == text_areas.len() + 1 {
                    recommendations.input_without_shortcuts(key);
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
    recommendations: TextArea,
) {
    // Create main layout
    let size = frame.area();
    let vertical_chunks: Rc<[Rect]> = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(35),
            Constraint::Percentage(35),
            Constraint::Percentage(10),
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
        Constraint::Length(20),
        Constraint::Length(20),
        Constraint::Length(20),
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

    match key.code{
        KeyCode::Esc => return ControlFlow::Break(()),
        // KeyCode::Left | KeyCode::Char('h') => {
        //     button_states[*selected_button] = State::Normal;
        //     *selected_button = selected_button.saturating_sub(1);
        //     button_states[*selected_button] = State::Selected;
        // }
        // KeyCode::Right | KeyCode::Char('l') => {
        //     button_states[*selected_button] = State::Normal;
        //     *selected_button = selected_button.saturating_add(1).min(2);
        //     button_states[*selected_button] = State::Selected;
        // }
        KeyCode::Char(' ') => {
            if button_states[*selected_button] == State::Active {
                button_states[*selected_button] = State::Normal;
            } else {
                button_states[*selected_button] = State::Active;
            }
        }
        _ => {},
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


