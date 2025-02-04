use button::{Button, State, TURQUOISE};
use crossbeam::channel::{self, Receiver, Sender};
use database::schema::{prestashop_schema, TicketData};
use json_widget::JsonWidget;
use ratatui::prelude::*;
use ratatui::{
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
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use serde_json::Value;
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};
use std::io;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

pub mod json_widget;
pub mod button;



/// ------------------------------
/// Main App Code
/// ------------------------------

struct App {
    input: Input,
    logs: Vec<String>,
    button_area: Option<Rect>,
    button_state: State,
    _ticket_data: TicketData,
    pub json_widget: JsonWidget,
    prestashop_api_tx: Sender<prestashop_schema::PrestashopPayload>,
    prestashop_api_rx: Receiver<prestashop_schema::PrestashopPayload>,
    /// We'll keep a scroll state for the JSON viewer
    json_scroll_state: ScrollViewState,
}

impl App {
    fn new() -> Self {
        let (prestashop_api_tx, prestashop_api_rx) = channel::unbounded();
        Self {
            input: Input::default(),
            logs: Vec::new(),
            button_area: None,
            button_state: State::Normal,
            _ticket_data: Default::default(),
            prestashop_api_tx,
            prestashop_api_rx,
            json_widget: JsonWidget::default(),
            json_scroll_state: ScrollViewState::default(),
        }
    }

    fn log_message(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }

    fn log_json(&mut self, value: Value) {
        self.json_widget = JsonWidget::new(value)
    }

    fn get_ticket(&self, service_number: &str) {
        let tx = self.prestashop_api_tx.clone();
        let input = service_number.to_string();
        log::info!("Getting payload with {input}");
        if !input.is_empty() {
            tokio::spawn(async move {
                let prestashop_order = database::schema::utilities::get_prestashop_payload(&input).await?;
                tx.try_send(prestashop_order)?;
                Ok::<(), anyhow::Error>(())
            });
        }
    }

    fn receive_ticket(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(data) = self.prestashop_api_rx.try_recv() {
            self.log_message(&serde_json::to_string(&data)?);
            self.log_json(serde_json::to_value(&data)?);
        }
        Ok(())
    }
}

pub fn run_terminal_mode() -> Result<(), Box<dyn std::error::Error>> {
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
        let _ = app.receive_ticket();
        terminal.draw(|f| ui::<B>(f, &mut app))?;

        match event::read()? {
            Event::Key(key_event) => {
                // Exit on Ctrl + C
                if key_event.code == KeyCode::Char('c')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return Ok(());
                }

                // Send keystroke to input field
                app.input.handle_event(&Event::Key(key_event));

                // If user presses Enter, treat it like clicking Get Ticket
                match key_event.code {
                    KeyCode::Enter => {
                        let user_input = app.input.value();
                        app.get_ticket(user_input);
                        app.log_message(&format!("(Enter) 'Get Ticket' with input: {}", user_input));
                    }
                    // We'll also handle up/down to scroll in the JSON scrollview
                    KeyCode::Down => {
                        // Move highlight
                        app.json_widget.next_edit();
                        // Or scroll
                        // app.json_scroll_state.scroll_down(1);
                    }
                    KeyCode::Up => {
                        app.json_widget.prev_edit();
                        // app.json_scroll_state.scroll_up(1);
                    }
                    KeyCode::Right => {
                        // could do something else or next edit
                        app.json_widget.next_edit();
                    }
                    KeyCode::Left => {
                        app.json_widget.prev_edit();
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse_event) => {
                match mouse_event.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(area) = app.button_area {
                            if mouse_event.column >= area.x
                                && mouse_event.column < area.x + area.width
                                && mouse_event.row >= area.y
                                && mouse_event.row < area.y + area.height
                            {
                                let user_input = app.input.value();
                                app.log_message(&format!(
                                    "(Click) 'Get Ticket' with input: {}",
                                    user_input
                                ));
                                app.button_state = State::Active;
                            } else {
                                app.button_state = State::Normal;
                            }
                        }
                    }
                    MouseEventKind::Moved => {
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1)
        ])
        .split(f.area());

    let input_and_button = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(80),
            Constraint::Percentage(20)
        ])
        .split(chunks[0]);

    let json_view = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50)
        ])
        .split(chunks[1]);

    // (A) Render Input Box
    let width = input_and_button[0].width.saturating_sub(2);
    let scroll_offset = app.input.visual_scroll(width as usize);

    let input_widget = Paragraph::new(app.input.value())
        .style(Style::default().fg(Color::Rgb(255, 20, 147)))
        .scroll((0, scroll_offset as u16))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Input")
                .border_style(Style::default().fg(Color::Rgb(147, 112, 219)))
                .style(Style::default().bg(Color::Rgb(10, 10, 14)))
        );

    f.render_widget(input_widget, input_and_button[0]);

    // set cursor
    let cursor_x = input_and_button[0].x + ((app.input.visual_cursor()).max(scroll_offset) - scroll_offset) as u16 + 1;
    let cursor_y = input_and_button[0].y + 1;
    f.set_cursor_position(Position { x: cursor_x, y: cursor_y });

    // (B) Render Button
    let get_ticket_button = Button::new(Line::from("Get Ticket"))
        .theme(TURQUOISE)
        .state(app.button_state);

    f.render_widget(get_ticket_button, input_and_button[1]);
    app.button_area = Some(input_and_button[1]);

    // (C) Left side logs
    let items: Vec<ListItem> = app
        .logs
        .iter()
        .map(|log| {
            ListItem::new(log.clone()).style(
                Style::default().fg(Color::Rgb(224, 255, 255)),
            )
        })
        .collect();

    let logs_list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Logs")
            .border_style(Style::default().fg(Color::Rgb(0, 255, 127)))
            .style(Style::default().bg(Color::Rgb(8, 8, 12)))
    );

    f.render_widget(logs_list, json_view[0]);

    // (D) Right side: JSON scrollable area
    // We'll call 'render_text()' on the widget to get a styled Text object

    let text = app.json_widget.render_text();
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Json Viewer")
                .border_style(Style::default().fg(Color::Rgb(250, 8, 182)))
                .style(Style::default().bg(Color::Rgb(10, 10, 14)))
        );

    // We create a `Size` using the width/height of the allocated area:
    let size = Size {
        width: json_view[1].width,
        height: json_view[1].height,
    };

    // Wrap in a ScrollView from tui-scrollview
    let mut scroll_view = ScrollView::new(size)
        .scrollbars_visibility(ScrollbarVisibility::Always);

    scroll_view.render_widget(paragraph, scroll_view.area());

    f.render_stateful_widget(scroll_view, json_view[1], &mut app.json_scroll_state);
    // paragraph
}
