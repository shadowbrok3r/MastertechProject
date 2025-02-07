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
    symbols,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
};
use serde_json::Value;
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};
use std::io;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

pub mod json_widget;
pub mod button;

////////////////////////////////////
// Add color constants referencing the scheme the user provided
////////////////////////////////////
const DEEPPINK: Color = Color::Rgb(255, 20, 147);
const CYAN: Color = Color::Cyan;
const SPRINGGREEN: Color = Color::Rgb(0, 255, 127);
const MEDIUMSLATEBLUE: Color = Color::Rgb(123, 104, 238);
const DARKORANGE: Color = Color::Rgb(255, 140, 0);
// etc. You can define more from your color scheme as needed.
////////////////////////////////////
// Main App Code
////////////////////////////////////

enum Tab {
    TurSheet,
    Scripts,
    SystemInfo,
    Extra,
}

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
    /// Which tab is currently selected
    selected_tab: Tab,
    /// Track the state of any script buttons, etc.
    tuneup_button_state: State,
    qc_button_state: State,
}

impl Default for App {
    fn default() -> Self {
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
            selected_tab: Tab::TurSheet,
            tuneup_button_state: State::Normal,
            qc_button_state: State::Normal,
        }
    }
}

impl App {
    fn new() -> Self {
        Self::default()
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

                match key_event.code {
                    KeyCode::Enter => {
                        let user_input = app.input.value();
                        app.get_ticket(user_input);
                        app.log_message(&format!("(Enter) 'Get Ticket' with input: {}", user_input));
                    }

                    // We'll let left/right arrow change tabs, just as an example
                    KeyCode::Right => {
                        app.selected_tab = match app.selected_tab {
                            Tab::TurSheet => Tab::Scripts,
                            Tab::Scripts => Tab::SystemInfo,
                            Tab::SystemInfo => Tab::Extra,
                            Tab::Extra => Tab::TurSheet,
                        };
                    }
                    KeyCode::Left => {
                        app.selected_tab = match app.selected_tab {
                            Tab::TurSheet => Tab::Extra,
                            Tab::Scripts => Tab::TurSheet,
                            Tab::SystemInfo => Tab::Scripts,
                            Tab::Extra => Tab::SystemInfo,
                        };
                    }

                    KeyCode::Down => {
                        // Move highlight in JSON widget, etc.
                        app.json_widget.next_edit();
                    }
                    KeyCode::Up => {
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
    // top-level layout has a row for tabs, then main content
    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // for tabs
            Constraint::Min(1),    // rest of content
        ])
        .split(f.area());

    // (1) Render Tabs at top
    let titles = ["TUR Sheet", "Scripts", "System Info", "Extra"];
    let selected_idx = match app.selected_tab {
        Tab::TurSheet => 0,
        Tab::Scripts => 1,
        Tab::SystemInfo => 2,
        Tab::Extra => 3,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Tabs")
                // let's do a block color in mediumslateblue
                .border_style(Style::default().fg(MEDIUMSLATEBLUE))
        )
        // base style for all tabs: white text
        .style(Style::default().fg(Color::White))
        // highlight style for the selected tab
        .highlight_style(Style::default().fg(DEEPPINK))
        // use an ASCII symbol or a fancy symbol for divider
        .divider(symbols::DOT)
        // The new ratatui Tabs take an IntoIterator of &str, so no .padding(...) right now.
        .select(selected_idx);

    f.render_widget(tabs, outer_chunks[0]);

    // (2) Main content area depends on which tab is selected
    let main_area = outer_chunks[1];

    match app.selected_tab {
        Tab::TurSheet => {
            render_tur_sheet_tab::<B>(app, f, main_area);
        }
        Tab::Scripts => {
            render_scripts_tab::<B>(app, f, main_area);
        }
        Tab::SystemInfo => {
            render_system_info_tab::<B>(app, f, main_area);
        }
        Tab::Extra => {
            render_extra_tab::<B>(app, f, main_area);
        }
    }
}

////////////////////////////////
// TUR SHEET TAB with Input Field
////////////////////////////////
fn render_tur_sheet_tab<B: Backend>(app: &mut App, f: &mut Frame, area: Rect) {
    // We'll have a vertical layout:
    //   [ Input + Button ]
    //   [ Logs (Left) | JSON (Right) ]
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // For input+button row
            Constraint::Min(1),    // For logs/json
        ])
        .split(area);

    // (A) Input + Button row
    let input_button_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(80),
            Constraint::Percentage(20),
        ])
        .split(vertical_chunks[0]);

    // The input field
    let width = input_button_chunks[0].width.saturating_sub(2);
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

    f.render_widget(input_widget, input_button_chunks[0]);

    // Place cursor in input field
    let cursor_x = input_button_chunks[0].x
        + ((app.input.visual_cursor()).max(scroll_offset) - scroll_offset) as u16
        + 1;
    let cursor_y = input_button_chunks[0].y + 1;
    f.set_cursor(cursor_x, cursor_y);

    // The "Get Ticket" button
    let get_ticket_button = Button::new(Line::from("Get Ticket"))
        .theme(TURQUOISE)
        .state(app.button_state);

    f.render_widget(get_ticket_button, input_button_chunks[1]);
    // track this area for mouse clicks
    app.button_area = Some(input_button_chunks[1]);

    // (B) Logs + JSON viewer below
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(vertical_chunks[1]);

    // (B1) Logs on the left
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
            .border_style(Style::default().fg(SPRINGGREEN))
    );

    f.render_widget(logs_list, horizontal_chunks[0]);

    // (B2) JSON scrollable area on the right
    let text = app.json_widget.render_text();
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Json Viewer")
                .border_style(Style::default().fg(DEEPPINK))
        );

    let size = Size {
        width: horizontal_chunks[1].width,
        height: horizontal_chunks[1].height,
    };

    let mut scroll_view = ScrollView::new(size)
        .scrollbars_visibility(ScrollbarVisibility::Always);

    scroll_view.render_widget(paragraph, scroll_view.area());
    f.render_stateful_widget(scroll_view, horizontal_chunks[1], &mut app.json_scroll_state);
}

////////////////////////////////
// SCRIPTS TAB with Buttons
////////////////////////////////
fn render_scripts_tab<B: Backend>(app: &mut App, f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // First button: Tuneup
    let tuneup_button = Button::new(Line::from("Tuneup"))
        .theme(TURQUOISE)
        .state(app.tuneup_button_state);

    f.render_widget(tuneup_button, chunks[0]);

    // Second button: QC
    let qc_button = Button::new(Line::from("QC"))
        .theme(TURQUOISE)
        .state(app.qc_button_state);

    f.render_widget(qc_button, chunks[1]);
}

fn render_system_info_tab<B: Backend>(app: &mut App, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("System Info")
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(block, area);
}

fn render_extra_tab<B: Backend>(app: &mut App, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Extra")
        .border_style(Style::default().fg(DARKORANGE));

    f.render_widget(block, area);
}
