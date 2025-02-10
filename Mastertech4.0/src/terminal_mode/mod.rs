use ratatui::{
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event
        },
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols,
    text::Line,
    widgets::{Block, Borders, Tabs},
};
use tabs::{service_order::render_tur_sheet_tab, Tab};
use tui_scrollview::ScrollViewState;
use database::schema::{prestashop_schema, TicketData};
use crossbeam::channel::{self, Receiver, Sender};
use widgets::{json_viewer::JsonWidget, button::{Button, State}};
use ratatui::prelude::*;
use serde_json::Value;
use colors::{C_CYAN, C_DEEPPINK, C_MEDIUMSLATEBLUE, TURQUOISE};
use tui_input::Input;
use std::io;

pub mod widgets;
pub mod colors;
pub mod tabs;
pub mod events;


pub struct App<'a> {
    input: Input,
    logs: Vec<String>,
    _ticket_data: TicketData,

    // JSON
    pub json_widget: JsonWidget,
    json_scroll_state: ScrollViewState,

    // Tab
    selected_tab: Tab,

    // Prestashop
    prestashop_api_tx: Sender<prestashop_schema::PrestashopPayload>,
    prestashop_api_rx: Receiver<prestashop_schema::PrestashopPayload>,

    buttons: Vec<Button<'a>>,
    //////////////////////////////////
    // Button areas for TUR Sheet
    //////////////////////////////////
    get_ticket_button_area: Option<Rect>,
    submit_ticket_button_area: Option<Rect>,
    get_ticket_button_state: State,
    submit_ticket_button_state: State,

    //////////////////////////////////
    // Button areas for Scripts tab
    //////////////////////////////////
    tuneup_button_area: Option<Rect>,
    qc_button_area: Option<Rect>,
    tuneup_button_state: State,
    qc_button_state: State,
}

impl Default for App <'_>{
    fn default() -> Self {
        let (prestashop_api_tx, prestashop_api_rx) = channel::unbounded();
        Self {
            input: Input::default(),
            logs: Vec::new(),
            _ticket_data: Default::default(),
            prestashop_api_tx,
            prestashop_api_rx,
            json_widget: JsonWidget::default(),
            json_scroll_state: ScrollViewState::default(),
            selected_tab: Tab::TurSheet,
            get_ticket_button_area: None,
            submit_ticket_button_area: None,
            get_ticket_button_state: State::Normal,
            submit_ticket_button_state: State::Normal,
            tuneup_button_area: None,
            qc_button_area: None,
            tuneup_button_state: State::Normal,
            qc_button_state: State::Normal,
            buttons: Vec::new(),
        }
    }
}

impl <'a> App <'a> {
    fn new() -> Self {
        Self::default()
    }

    fn log_message(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }

    fn log_json(&mut self, value: Value) {
        self.json_widget = JsonWidget::new(value);
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

        let _read_events = match event::read()? {
            Event::Key(key_event) => app.handle_key_event(key_event),
            Event::Mouse(mouse_event) => app.handle_mouse_event(mouse_event),
            Event::Resize(_, _) => Ok(()),
            _ => Ok(())
        };
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
                .border_style(Style::default().fg(C_MEDIUMSLATEBLUE))
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(C_DEEPPINK))
        .divider(symbols::DOT)
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

    // (1) Tuneup
    let tuneup_button = Button::new(Line::from("Tuneup"), chunks[0])
        .theme(TURQUOISE)
        .state(app.tuneup_button_state);
    f.render_widget(tuneup_button, chunks[0]);
    // app.tuneup_button_area = Some(chunks[0]);

    // (2) QC
    let qc_button = Button::new(Line::from("QC"), chunks[1])
        .theme(TURQUOISE)
        .state(app.qc_button_state);
    f.render_widget(qc_button, chunks[1]);
    // app.qc_button_area = Some(chunks[1]);
}

fn render_system_info_tab<B: Backend>(_app: &mut App, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("System Info")
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(block, area);
}

fn render_extra_tab<B: Backend>(_app: &mut App, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Extra")
        .border_style(Style::default().fg(C_CYAN));

    f.render_widget(block, area);
}
