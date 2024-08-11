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
use {database::schema::{CustomerData, TicketData}, ratatui::{buffer::Buffer, crossterm::event::KeyModifiers, layout::{Position, Rect, Size}, style::Stylize, text::{Line, Span}, widgets::{BorderType, Widget}, Frame}, std::{io, ops::ControlFlow, rc::Rc, time::Duration}};

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
    /// Current input mode
    input_mode: InputMode,
    // History of recorded messages
    // messages: Vec<String>,
}

impl App {
    fn new() -> Self {
        Self {
            ticket_data: TicketData::default(),
            customer_data: CustomerData::default(),
            input_mode: InputMode::Normal,
            // messages: Vec::new(),
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
    let mut focused_index = 0;  // Track which text field is focused

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
                if handle_key_event(
                    key, &mut app,  
                    &mut button_states, 
                    &mut selected_button,
                ).is_break() {
                    break;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    if let KeyCode::Enter = key.code {
                        button_states[0] = State::Active;
                    }
                }
                match key.code{
                    KeyCode::Tab => {
                        log::info!("index: {:?}", focused_index);
                        if focused_index < text_areas.len() - 1 {
                            focused_index += 1;
                        }
                    }
                    KeyCode::BackTab => {
                        log::info!("index: {:?}", focused_index);
                        if focused_index > 0 {
                            focused_index -= 1;
                        }
                    },
                    _ => {}
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
    recommendations: TextArea
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
        _ => {

        },
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


pub enum FieldStatus {
    Valid,
    Invalid,
}

impl Into<String> for Field<'_> {
    fn into(self) -> String {
        self.fd.val.to_string()
    }
}

/// A reference to a specific field's data in a form that also indicates whether or not it's valid.
pub struct Field<'a> {
    fd: FieldData<'a>,
    status: FieldStatus,
}

impl<'a> Field<'a> {
    pub(crate) fn valid(name: &'a str, val: &'a str) -> Field<'a> {
        Self {
            fd: FieldData { name, val },
            status: FieldStatus::Valid,
        }
    }

    pub(crate) fn invalid(name: &'a str, val: &'a str) -> Field<'a> {
        Self {
            fd: FieldData { name, val },
            status: FieldStatus::Invalid,
        }
    }

    /// Name of the underlying field.
    pub fn name(&self) -> &str {
        self.fd.name
    }

    /// Value of the underlying field.
    pub fn value(&self) -> &str {
        self.fd.val
    }

    /// Returns `true` if the underlying field is currently valid.
    pub fn is_valid(&self) -> bool {
        match self.status {
            FieldStatus::Valid => true,
            FieldStatus::Invalid => false,
        }
    }
}

#[derive(Clone)]
struct FieldData<'a> {
    name: &'a str,
    val: &'a str,
}

type FormFieldStatus<'a> = Vec<Field<'a>>;
/// Enumerates possible states of a [`Form`]s currently selected field.
#[derive(PartialEq)]
pub enum FormSelection {
    /// No field selected
    NoSelection,
    /// Hovered, but not receiving text input
    Hovered(usize),
    /// Receiving text input
    Active(usize),
}

pub(crate) struct FieldBuffer {
    name: String,
    val: String,
}

impl From<Vec<(&str, &str)>> for Form {
    fn from(value: Vec<(&str, &str)>) -> Self {
        Self {
            fields: value
                .into_iter()
                .map(|(d_name, d_val)| FieldBuffer {
                    name: d_name.to_string(),
                    val: d_val.to_string(),
                })
                .collect(),
            ..Default::default()
        }
    }
}

impl From<Vec<&str>> for Form {
    fn from(value: Vec<&str>) -> Self {
        Self {
            fields: value
                .into_iter()
                .map(|d_name| FieldBuffer {
                    name: d_name.to_string(),
                    val: String::new(),
                })
                .collect(),
            ..Default::default()
        }
    }
}

impl From<Vec<FieldBuffer>> for Form {
    fn from(value: Vec<FieldBuffer>) -> Self {
        Self {
            fields: value,
            ..Default::default()
        }
    }
}

/// A widget to display data in a collection of fields, and allow editing of a currently selected
/// field.
///
/// # Example
///
/// ```
/// # use tui_form_widget::{Form, FormSelection};
/// let mut form = Form::new(&["A", "B", "C"], |field| !field.is_empty());
///
/// // all fields remain valid until form is submitted.
/// assert_eq!(form.status().iter().all(|field| field.is_valid()), true);
///
/// // fields will now be invalid after submitting.
/// form.submit();
/// assert_eq!(form.status().iter().all(|field| field.is_valid()), false);
///
/// form.select(FormSelection::Active(0));
/// form.append_selection('a');
/// assert!(form.status()[0].is_valid());
/// ```
pub struct Form {
    selected: FormSelection,
    pub fields: Vec<FieldBuffer>,
    pub submitted: bool,
    validation_fn: Box<dyn Fn(&str) -> bool + 'static>,
    pub default_field_style: Style,
    pub invalid_field_style: Style,
    pub hovered_field_style: Style,
    pub active_field_style: Style,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            selected: FormSelection::NoSelection,
            fields: Vec::new(),
            submitted: false,
            validation_fn: Box::new(|f| !f.is_empty()),
            default_field_style: Style::default(),
            invalid_field_style: Style::default().red().bold(),
            hovered_field_style: Style::default().cyan(),
            active_field_style: Style::default().cyan().bold(),
        }
    }
}

impl Form {
    /// Create a new [`Form`] from a slice of field titles and a validator function.
    /// `validation_fn` is used to mark fields as either valid or invalid when `.status()` is called.
    pub fn new(fields: &[&str], validation_fn: impl Fn(&str) -> bool + 'static) -> Self {
        let fields = fields
            .iter()
            .map(|&title| FieldBuffer {
                name: title.to_string(),
                val: String::new(),
            })
            .collect();

        Self {
            fields,
            validation_fn: Box::new(validation_fn),
            ..Default::default()
        }
    }

    /// Returns a tui [`Widget`](ratatui::widgets::Widget) to be used for rendering with
    /// [`render_frame`][ratatui::terminal::Frame::render_widget].
    pub fn widget(&self) -> impl Widget + '_ {
        Renderer::new(self)
    }

    /// Change current selection of the form.
    pub fn select(&mut self, s: FormSelection) {
        self.selected = s;
    }

    /// Get current selection state of the form.
    pub fn selected(&self) -> &FormSelection {
        &self.selected
    }

    /// Submits form and returns status of fields.
    pub fn submit(&mut self) -> FormFieldStatus {
        self.submitted = true;
        self.status()
    }

    /// Returns the state of all fields in the form. Uses a [`Field`] struct to indicate whether or
    /// not each field's buffer is valid.
    pub fn status(&self) -> FormFieldStatus {
        if self.submitted {
            self.fields
                .iter()
                .map(|fb| {
                    if (self.validation_fn)(&fb.val) {
                        Field::valid(&fb.name, &fb.val)
                    } else {
                        Field::invalid(&fb.name, &fb.val)
                    }
                })
                .collect()
        } else {
            self.fields
                .iter()
                .map(|fb| Field::valid(&fb.name, &fb.val))
                .collect()
        }
    }

    /// Handle default input for the form.
    pub fn input(&mut self, key: KeyCode) {
        if let FormSelection::Active(i) = self.selected {
            match key {
                KeyCode::Enter => self.next_field(),
                KeyCode::Esc => self.select(FormSelection::Hovered(i)),
                KeyCode::Backspace => self.pop_field(i),
                KeyCode::Char(ch) => self.append_field(ch, i),
                _ => {}
            }
        } else {
            match key {
                KeyCode::Esc => self.select(FormSelection::NoSelection),
                KeyCode::Char('j') => self.next_field(),
                KeyCode::Char('k') => self.prev_field(),
                KeyCode::Enter => {
                    if let FormSelection::Hovered(i) = self.selected {
                        self.selected = FormSelection::Active(i)
                    } else {
                        self.selected = FormSelection::Active(0)
                    }
                }
                _ => {}
            }
        }
    }

    fn pop_field(&mut self, field: usize) {
        self.fields[field].val.pop();
    }

    fn append_field(&mut self, ch: char, field: usize) {
        self.fields[field].val.push(ch)
    }

    /// Append a char to the active field (if one is)
    pub fn append_selection(&mut self, ch: char) {
        match self.selected() {
            FormSelection::NoSelection => {}
            FormSelection::Hovered(_) => {}
            FormSelection::Active(i) => self.append_field(ch, *i),
        }
    }

    /// Remove a char frome the active field (if one is)
    pub fn pop_selection(&mut self) {
        match self.selected() {
            FormSelection::NoSelection => {}
            FormSelection::Hovered(_) => {}
            FormSelection::Active(i) => self.pop_field(*i),
        }
    }

    /// De(select / activate) current field
    pub fn deselect(&mut self) {
        self.selected = FormSelection::NoSelection
    }

    /// Move to next field. Retains previous hovered or activated state.
    pub fn next_field(&mut self) {
        self.selected = match self.selected {
            FormSelection::NoSelection => FormSelection::Hovered(0),
            FormSelection::Hovered(i) => {
                FormSelection::Hovered((i + 1).rem_euclid(self.fields.len()))
            }
            FormSelection::Active(i) => {
                FormSelection::Active((i + 1).rem_euclid(self.fields.len()))
            }
        }
    }

    /// Move to previous field. Retains previous hovered or activated state.
    pub fn prev_field(&mut self) {
        self.selected = match self.selected {
            FormSelection::NoSelection => FormSelection::Hovered(0),
            FormSelection::Hovered(i) => {
                let i = if i == 0 { self.fields.len() - 1 } else { i - 1 };
                FormSelection::Hovered(i)
            }
            FormSelection::Active(i) => {
                let i = if i == 0 { self.fields.len() - 1 } else { i - 1 };
                FormSelection::Active(i)
            }
        }
    }

    /// Set whether the Form has been submitted
    pub fn submitted(&mut self, submitted: bool) {
        self.submitted = submitted;
    }

    /// Set style for the active field.
    pub fn active_field_style(&mut self, style: Style) {
        self.active_field_style = style;
    }

    /// Set style for any invalid fields.
    pub fn invalid_field_style(&mut self, style: Style) {
        self.invalid_field_style = style;
    }

    /// Set the style for the hovered field.
    pub fn hovered_field_style(&mut self, style: Style) {
        self.hovered_field_style = style;
    }

    /// Set style for a valid, unselected field.
    pub fn default_field_style(&mut self, style: Style) {
        self.default_field_style = style;
    }
}

pub struct Renderer<'a>(&'a Form);

impl<'a> Renderer<'a> {
    pub fn new(form: &'a Form) -> Self {
        Renderer(form)
    }
}

impl<'a> Widget for Renderer<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Block::new().title("Form").render(area, buf);
        let constraints: Vec<Constraint> = self
            .0
            .fields
            .iter()
            .map(|_| Constraint::Max(3))
            .chain([Constraint::Max(1)])
            .collect();

        let area = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        self.render_fields(area, buf);
    }
}

enum FieldRenderType {
    Normal,
    Invalid,
    Hovered,
    Active,
}

impl<'a> Renderer<'a> {
    fn render_fields(&self, area: Rc<[Rect]>, buf: &mut Buffer) {
        let fields = self.0.status();
        fields.iter().enumerate().for_each(|(i, field)| {
            let is_invalid = !field.is_valid() && self.0.submitted;
            let hovered = if let FormSelection::Hovered(f) = self.0.selected() {
                *f == i
            } else {
                false
            };

            let active = if let FormSelection::Active(f) = self.0.selected() {
                *f == i
            } else {
                false
            };

            let render_type = match (hovered, active, is_invalid) {
                (_, true, _) => FieldRenderType::Active,
                (true, false, _) => FieldRenderType::Hovered,
                (false, false, true) => FieldRenderType::Invalid,
                (false, false, false) => FieldRenderType::Normal,
            };
            self.render_field_gen(area[i], buf, field.value(), Some(field.name()), render_type);
        });
    }

    fn render_field_gen(
        &self,
        area: Rect,
        buf: &mut Buffer,
        content: &str,
        title: Option<&str>,
        fr: FieldRenderType,
    ) {
        match fr {
            FieldRenderType::Normal => self.render_field(area, buf, content, title),
            FieldRenderType::Invalid => self.render_field_invalid(area, buf, content, title),
            FieldRenderType::Hovered => self.render_field_hovered(area, buf, content, title),
            FieldRenderType::Active => self.render_field_active(area, buf, content, title),
        }
    }

    fn render_field(&self, area: Rect, buf: &mut Buffer, content: &str, title: Option<&str>) {
        Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(match title {
                        Some(t) => t,
                        None => "",
                    }),
            )
            .render(area, buf)
    }

    fn render_field_hovered(
        &self,
        area: Rect,
        buf: &mut Buffer,
        content: &str,
        title: Option<&str>,
    ) {
        Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.0.hovered_field_style)
                    .border_type(BorderType::Rounded)
                    .title(match title {
                        Some(t) => t,
                        None => "",
                    }),
            )
            .render(area, buf)
    }

    fn render_field_active(
        &self,
        area: Rect,
        buf: &mut Buffer,
        content: &str,
        title: Option<&str>,
    ) {
        Paragraph::new(Line::from(vec![
            Span::raw(content),
            Span::styled(" ", Style::default().reversed()),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.0.active_field_style)
                .border_type(BorderType::Rounded)
                .title_style(self.0.active_field_style)
                .title(match title {
                    Some(t) => t,
                    None => "",
                }),
        )
        .render(area, buf)
    }

    fn render_field_invalid(
        &self,
        area: Rect,
        buf: &mut Buffer,
        content: &str,
        title: Option<&str>,
    ) {
        Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.0.invalid_field_style)
                    .border_type(BorderType::Rounded)
                    .title_style(self.0.invalid_field_style)
                    .title(match title {
                        Some(t) => t,
                        None => "",
                    }),
            )
            .render(area, buf)
    }
}