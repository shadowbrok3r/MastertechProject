use crossbeam::channel::{self, Receiver, Sender};
use database::schema::{prestashop_schema, TicketData};
use ratatui::prelude::*;
use ratatui::{
    buffer::Buffer,
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
            MouseButton, MouseEventKind,
        },
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget},
};
use serde::Serialize;
use tui_tree_widget::TreeItem;
use std::fmt::Debug;
use std::{error::Error, io};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

/// ------------------------------
/// Custom Button widget
/// ------------------------------
#[derive(Debug, Clone)]
struct Button<'a> {
    label: Line<'a>,
    theme: Theme,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
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

// We'll define a single theme matching our desired turquoise color scheme.
// You can adjust highlight/shadow as you like.
const TURQUOISE: Theme = Theme {
    text: Color::Black,
    background: Color::Rgb(72, 209, 204), // mediumturquoise
    highlight: Color::Rgb(102, 239, 234), // lighten slightly for highlight
    shadow: Color::Rgb(42, 179, 174),     // darken slightly for shadow
};

impl<'a> Button<'a> {
    pub fn new<T: Into<Line<'a>>>(label: T) -> Self {
        Button {
            label: label.into(),
            theme: TURQUOISE,
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

    /// Helper method to get the right colors based on the current state.
    const fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme;
        match self.state {
            State::Normal => (t.background, t.text, t.shadow, t.highlight),
            State::Selected => (t.highlight, t.text, t.shadow, t.highlight),
            State::Active => (t.background, t.text, t.highlight, t.shadow),
        }
    }
}

impl<'a> Widget for Button<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        // Fill area with background + text color.
        buf.set_style(area, Style::default().bg(background).fg(text));

        // If there's room, draw top highlight line.
        if area.height > 2 {
            let top_str = "▔".repeat(area.width as usize);
            buf.set_string(
                area.x,
                area.y,
                top_str,
                Style::default().fg(highlight).bg(background),
            );
        }
        // If there's room, draw bottom shadow line.
        if area.height > 1 {
            let bot_str = "▁".repeat(area.width as usize);
            buf.set_string(
                area.x,
                area.y + area.height - 1,
                bot_str,
                Style::default().fg(shadow).bg(background),
            );
        }

        // Center the label.
        let label_x = area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2;
        let label_y = area.y + (area.height.saturating_sub(1)) / 2;
        buf.set_line(label_x, label_y, &self.label, area.width);
    }
}

/// ------------------------------
/// Main App Code
/// ------------------------------
struct App {
    input: Input,
    logs: Vec<String>,
    /// The rectangular area of the 'Get Ticket' button, updated each frame so we can detect clicks.
    button_area: Option<Rect>,
    /// Track the button's state (Normal, Selected, Active, etc.)
    button_state: State,
    /// Ticket information
    ticket_data: TicketData,

    tree: Vec<TreeItem<'static, &'static str>>,
    prestashop_api_tx: Sender<prestashop_schema::PrestashopPayload>,
    prestashop_api_rx: Receiver<prestashop_schema::PrestashopPayload>
}

impl App {
    fn new() -> Self {
        let (prestashop_api_tx, prestashop_api_rx) = channel::unbounded();
        Self {
            input: Input::default(),
            logs: Vec::new(),
            button_area: None,
            button_state: State::Normal,
            ticket_data: Default::default(),
            prestashop_api_tx,
            prestashop_api_rx,
            tree: Vec::new()
        }
    }

    fn log_message(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }

    fn log_json<T: Serialize + Clone + Debug>(&mut self, message: T) {

    }
    
    fn get_ticket(&self, service_number: &str) {
        let tx = self.prestashop_api_tx.clone();
        let input = service_number.to_string();
        log::info!("Getting payload with {input}");
        if !input.is_empty() {
            tokio::spawn(async move {
                let prestashop_order = database::schema::utilities::get_prestashop_payload(&input).await?;
                // log::info!("prestashop_order: {prestashop_order:#?}");
                tx.try_send(prestashop_order)?;
                Ok::<(), anyhow::Error>(())
            });
        }
    }

    fn receive_ticket(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(data) = self.prestashop_api_rx.try_recv() {
            // let out = serde_json::to_string_pretty(&data).unwrap();
            let output = serde_json::to_string(&data)?;
            self.log_message(&output);
            
            log::info!("{output}");
            // self.ticket_data = TicketData {
            //     id: todo!(),
            //     created_at: todo!(),
            //     customer: todo!(),
            //     computer: todo!(),
            //     service_number: todo!(),
            //     checkin_rep: todo!(),
            //     sales_rep: todo!(),
            //     checkin_notes: todo!(),
            //     tech: todo!(),
            //     salesman: todo!(),
            //     terms: todo!(),
            //     ticket_total: todo!(),
            //     doc_alias: todo!(),
            //     current_antivirus: todo!(),
            //     hardware_test_results: todo!(),
            // }
        }
        Ok(())
    }
}

pub fn run_terminal_mode() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new();
    let res = run_app(&mut terminal, app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        log::info!("{:?}", err);
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        app.receive_ticket();
        terminal.draw(|f| ui::<B>(f, &mut app))?;

        match event::read()? {
            Event::Key(key_event) => {
                // Exit on Ctrl + C
                if key_event.code == KeyCode::Char('c')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return Ok(());
                }
                // Send the keystroke to our input field
                app.input.handle_event(&Event::Key(key_event));

                // If user presses Enter, treat it like clicking Get Ticket
                if key_event.code == KeyCode::Enter {
                    let user_input = app.input.value();
                    app.get_ticket(user_input);
                    app.log_message(&format!("(Enter) 'Get Ticket' with input: {}", user_input));
                }
            }
            Event::Mouse(mouse_event) => {
                match mouse_event.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Detect if click is inside the button area
                        if let Some(area) = app.button_area {
                            if mouse_event.column >= area.x
                                && mouse_event.column < area.x + area.width
                                && mouse_event.row >= area.y
                                && mouse_event.row < area.y + area.height
                            {
                                // Button was clicked, handle the input
                                let user_input = app.input.value();
                                app.log_message(&format!(
                                    "(Click) 'Get Ticket' with input: {}",
                                    user_input
                                ));

                                // Toggle the button's state to Active momentarily
                                app.button_state = State::Active;
                            } else {
                                // If we click outside, revert button to Normal
                                app.button_state = State::Normal;
                            }
                        }
                    }
                    MouseEventKind::Moved => {
                        // If hovered over the button, set to Selected
                        if let Some(area) = app.button_area {
                            if mouse_event.column >= area.x
                                && mouse_event.column < area.x + area.width
                                && mouse_event.row >= area.y
                                && mouse_event.row < area.y + area.height
                            {
                                app.button_state = State::Selected;
                            } else {
                                app.button_state = State::Normal;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn ui<B: Backend>(f: &mut Frame, app: &mut App) {
    // Top-level layout: input area + button row, then logs
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(f.area());

    // Horizontal layout for input box and custom Button side by side
    let input_and_button = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(chunks[0]);

    // (A) Render the input box
    // We'll use (10,10,14) for the background, keep the rest as is.

    let width = input_and_button[0].width.saturating_sub(2);
    let scroll_offset = app.input.visual_scroll(width as usize);

    let input_widget = Paragraph::new(app.input.value())
        .style(
            Style::default().fg(Color::Rgb(255, 20, 147)), // deeppink text
        )
        .scroll((0, scroll_offset as u16))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Input")
                .border_style(
                    Style::default().fg(Color::Rgb(147, 112, 219)), // mediumpurple border
                )
                .style(Style::default().bg(Color::Rgb(10, 10, 14))),
        );

    f.render_widget(input_widget, input_and_button[0]);

    // Place the cursor so user can see where they're typing.
    let pos = Position::new(
        input_and_button[0].x
        + ((app.input.visual_cursor()).max(scroll_offset) - scroll_offset) as u16
        + 1,
        input_and_button[0].y + 1
    );

    f.set_cursor_position(pos);

    // (B) Render the custom Button widget instead of a normal Block.
    // We'll use the same color theme from before (TURQUOISE) via the new custom widget.
    let get_ticket_button = Button::new(Line::from("Get Ticket"))
        .theme(TURQUOISE)
        .state(app.button_state);

    // Render the button.
    f.render_widget(get_ticket_button, input_and_button[1]);
    // Store the button area for click detection.
    app.button_area = Some(input_and_button[1]);

    // (C) Render the logs
    // We'll use background (8,8,12) for a deeper cosmic look.

    let items: Vec<ListItem> = app
        .logs
        .iter()
        .map(|log| {
            ListItem::new(log.clone()).style(
                Style::default().fg(Color::Rgb(224, 255, 255)), // lightcyan text
            )
        })
        .collect();

    
    let logs_list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Logs")
            .border_style(
                Style::default().fg(Color::Rgb(0, 255, 127)), // springgreen border
            )
            .style(Style::default().bg(Color::Rgb(8, 8, 12))),
    );

    f.render_widget(logs_list, chunks[1]);
}