use displays::remote_viewer::ratagui::TerminalEvent;
use ratatui::{crossterm::{ event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind}, execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},}, layout::{Constraint, Direction, Flex, Layout}};
use systems::{communication_system::Message, data_system::DataSystem, notification_system::Notification, render_system::RenderSystem, widget_render_system::WidgetRenderer};
use tabs::{logger::Logger, login::LoginTab, service_form::ServiceFormTab, tasks::TasksTab, MenuBar, ScriptsTab, SysinfoTab, Tab};
use events::{action_handler::{get_event_receiver, EventManager}, EventHandler};
use database::WS_CLIENT_URL;
use std::{cell::RefCell, io, rc::Rc, sync::{Arc, Mutex}, time::{Duration, Instant}};
use ratatui_splash_screen::{SplashConfig, SplashScreen};
use crate::filesystem::system_info::get_sysinfo_no_gpu;
use crossbeam::channel::unbounded;
// use base64::{engine::general_purpose, Engine as _};
use fx::{effect::UniqueEffectId, EffectStage};
use context::TerminalContext;
// use tachyonfx::CellFilter;
use widgets::HandleWidget;
// use styling::CATPPUCCIN;
use ratatui::prelude::*;
use reqwest::Client;

pub mod systems;
pub mod widgets;
pub mod tabs;
pub mod events;
pub mod styling;
pub mod fx;
pub mod data;
pub mod context;
pub mod ncdu;
pub mod websockets;

static SPLASH_CONFIG: SplashConfig = SplashConfig {
    image_data: include_bytes!("../assets/masterlogoV2.png"),
    sha256sum: None,
    render_steps: 30,
    use_colors: true,
};

static SPLASH_CONFIG2: SplashConfig = SplashConfig {
    image_data: include_bytes!("../assets/pcllogo.png"),
    sha256sum: None,
    render_steps: 30,
    use_colors: true,
};

pub struct TerminalApp<'a> {
    logger: Logger,
    menu_bar: MenuBar<'a>,
    scripts_tab: Rc<RefCell<ScriptsTab<'a>>>,
    service_tab: Rc<RefCell<ServiceFormTab<'a>>>,
    tasks_tab: Rc<RefCell<TasksTab>>,
    sysinfo_tab: SysinfoTab,
    login_tab: Rc<RefCell<LoginTab<'a>>>,
    effect_stage: EffectStage<UniqueEffectId>,
    first_run: bool,
    event_handler: EventHandler,
    event_manager: EventManager<'a>,
    ctx: Arc<Mutex<TerminalContext>>,
    render_system: Arc<RenderSystem>,
    data_system: Arc<DataSystem>,
    // buffer_tx: tokio::sync::mpsc::UnboundedSender<Buffer>,
    // buffer_rx: tokio::sync::mpsc::UnboundedReceiver<Buffer>,
    cached_buffer: Option<Buffer>, // Existing from previous solution
}

impl Default for TerminalApp <'_>{
    fn default() -> Self {
        // Create the channel for Buffer messages.
        // let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();

        let client = Client::new();
        // Create channels explicitly for communication between Data and Render systems
        let (data_to_render_tx, data_to_render_rx) = unbounded::<Box<dyn Message>>();
        let (render_to_data_tx, render_to_data_rx) = unbounded::<Box<dyn Message>>();

        // Global App Context, passed through most widgets / event handlers / 'Systems' via Arc 
        let ctx = Arc::new(Mutex::new(TerminalContext::new(render_to_data_tx.clone(), data_to_render_tx.clone())));

        // Create systems separately
        let render_system: Arc<RenderSystem> = Arc::new(RenderSystem::new(render_to_data_tx, data_to_render_rx, ctx.clone()));
        let data_system: Arc<DataSystem> = Arc::new(DataSystem::new(data_to_render_tx, render_to_data_rx));

        // Create a global event channel.
        let mut event_manager = EventManager::new(get_event_receiver());
        let service_tab = Rc::new(RefCell::new(ServiceFormTab::new(client.clone(), ctx.clone())));
        let tasks_tab = Rc::new(RefCell::new(TasksTab::new(client.clone(), ctx.clone())));
        let scripts_tab = Rc::new(RefCell::new(ScriptsTab::new(client.clone(), ctx.clone())));
        let login_tab = Rc::new(RefCell::new(LoginTab::new(client.clone(), ctx.clone())));
        let menu_bar = MenuBar::new(ctx.clone());
        
        // Register the ServiceFormTab with the event manager.
        // Here we clone the Rc so both ServiceTab and the EventManager share it.
        event_manager.register_handler(service_tab.clone());
        event_manager.register_handler(scripts_tab.clone());
        event_manager.register_handler(login_tab.clone());

        Self {
            logger: Logger::new(),
            menu_bar,
            scripts_tab,
            service_tab,
            sysinfo_tab: SysinfoTab::new(),
            login_tab,
            effect_stage: EffectStage::default(),
            event_handler: EventHandler::new(),
            first_run: true,
            event_manager,
            ctx,
            render_system,
            data_system,
            tasks_tab,
            // buffer_tx,
            // buffer_rx,
            cached_buffer: None,
        }
    }
}

pub async fn run_terminal_mode() -> anyhow::Result<(), anyhow::Error> {
    // // Set max_log_level to Trace
    // tui_logger::init_logger(log::LevelFilter::Info).unwrap();
    // // Set default level for unknown targets to Trace
    // tui_logger::set_default_level(log::LevelFilter::Info);

    let log_level = log::LevelFilter::Info;
    let log_file = std::fs::File::create("terminal_output.log").unwrap();
    simplelog::WriteLogger::init(
        log_level,
        simplelog::Config::default(),
        log_file
    ).unwrap();

    log::info!("STARTING TERM MODE");
    enable_raw_mode()?;
    log::info!("Hooking StdOut");
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    log::info!("Creating Crossterm backend");
    let backend = CrosstermBackend::new(stdout);
    log::info!("Creating Terminal");
    let mut terminal = Terminal::new(backend)?;

    let mut app = TerminalApp::default();
    log::info!("Retrieving sysinfo");
    if let Ok(sysinfo) = get_sysinfo_no_gpu().await {
        app.sysinfo_tab.set_sysinfo(sysinfo);
    }

    log::info!("Running app");

    let first_run = app.first_run();

    log::info!("First Run Results: {first_run:?}");

    let res = app.ui(&mut terminal).await;
    
    disable_raw_mode()?;

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    terminal.show_cursor()?;

    if let Err(err) = res {
        log::info!("ERR: {:?}", err);
    }

    Ok(())
}

impl <'a>TerminalApp<'a> {
    async fn ui<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> anyhow::Result<(), anyhow::Error> {
        // render splash screen
        let mut splash_screen = SplashScreen::new(SPLASH_CONFIG)?;
        let mut splash_screen2 = SplashScreen::new(SPLASH_CONFIG2)?;

        let notifications: Arc<Mutex<Vec<Notification>>> = self.render_system.notifications.clone();
        let ui_messages: Arc<Mutex<Vec<Box<dyn Message>>>> = self.render_system.ui_messages.clone();
        // Create a shutdown channel
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

        // Clone Arc for running background tasks
        let render_system_bg: Arc<RenderSystem> = Arc::clone(&self.render_system);
        let data_system_bg: Arc<DataSystem> = Arc::clone(&self.data_system);

        let mut join_handles = Vec::new();
        let shutdown_rx_data = shutdown_tx.subscribe();
        // Run DataSystem in the background
        join_handles.push(
            tokio::spawn(async move {
                data_system_bg.run(shutdown_rx_data).await;
            })
        );

        let shutdown_rx_render = shutdown_tx.subscribe();

        // Run RenderSystem in the background
        join_handles.push(
            tokio::spawn(async move {
                render_system_bg.run(shutdown_rx_render).await;
            })
        );

        let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();
        let (start_tx, mut start_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        join_handles.push(
            tokio::spawn(async move {
                let websocket_server = TerminalApp::start_websocket_sender(buffer_rx, start_tx.clone(), event_tx).await;
                log::info!("websocket_server: {websocket_server:?}");
            })
        );

        let mut last_sent = Instant::now(); // Changed: Added to throttle sending
        let send_interval = Duration::from_secs_f32(0.5); // Changed: ~3 FPS interval
        let can_start = &mut false;
        loop {
            if self.handle_events(None, None) { 
                // Signal shutdown
                shutdown_tx.send(())?;
                log::info!("Sent shutdown signal");
        
                // Wait for tasks to complete with a timeout
                for handle in join_handles {
                    handle.abort();
                }
                break; 
            }
            terminal.draw(|f| {
                // f.buffer_mut().clone();
                if !splash_screen.is_rendered() && !splash_screen2.is_rendered() {
                    let layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(1)
                    .constraints([
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ]).split(f.area());
                    f.render_widget(&mut splash_screen, layout[0]);
                    f.render_widget(&mut splash_screen2, layout[1]);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                } else {
                    // Process events from egui via WebSocket
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            TerminalEvent::MouseClick { x, y } => {
                                log::info!("Received mouse click at x={}, y={}", x, y);
                                let mouse_event = MouseEvent {
                                    kind: MouseEventKind::Down(MouseButton::Left),
                                    column: x,
                                    row: y,
                                    modifiers: KeyModifiers::NONE,
                                };
                                if self.handle_events(Some(mouse_event), None) {
                                    log::info!("Quit signal received from handle_events (mouse)");
                                }
                            }
                            TerminalEvent::KeyPress { code, modifiers } => {
                                log::info!("Received key press: code={:?}, modifiers={:?}", code, modifiers);
                                let key_event = KeyEvent {
                                    code,
                                    modifiers,
                                    kind: KeyEventKind::Press,
                                    state: KeyEventState::NONE,
                                };
                                if self.handle_events(None, Some(key_event)) {
                                    log::info!("Quit signal received from handle_events (key)");
                                }
                            }
                        }
                    }
                    if let Ok(start) = start_rx.try_recv() {
                        *can_start = start;
                    }
                    self.event_manager.process_events();
                    self.tasks_tab.borrow_mut().check_tasks();
                    if let Ok(mut ctx) = self.ctx.lock() {
                        ctx.receive();
                        match ctx.state {
                            crate::app_state::AppState::Authenticated(_) => {
                                if ctx.new_state {
                                    ctx.new_state = false;
                                    if let Ok(mut tab) = self.menu_bar.current_tab.try_borrow_mut() {
                                        *tab = Tab::TurSheet;
                                        self.menu_bar.login_tab.set_label("Logout".to_string());
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
            
                    let area = f.area();
                    f.buffer_mut().set_style(area, Style::default().bg(Color::Rgb(8, 8, 12)));
            
                    let layout = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Percentage(8), // for tabs
                            Constraint::Percentage(92),// rest of content
                        ]);
            
                    let outer_chunks = layout.split(f.area());
            
                    let tab_layout = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(10),
                            Constraint::Percentage(80),
                            Constraint::Percentage(10),
                        ]).split(outer_chunks[0]);
            
                    let tab_area = center_horizontal(tab_layout[1], tab_layout[1].width);

                    let main_content_area = outer_chunks[1];
                    
                    if self.first_run {
            
                        // let effect1 = outline_selected_cells(
                        //     &mut self.menu_bar.effect_stage, 
                        //     main_content_area.as_size(),
                        //     CATPPUCCIN.maroon,
                        //     CellFilter::FgColor(CATPPUCCIN.maroon)
                        // );
            
                        // self.effect_stage.add_effect(effect1);
                        if let Ok(notifs) = notifications.lock() {
                            if let Some(notification) = notifs.last() {
                                let a = notification.notification_area::<B>(f);
                                notification.render_effects(&mut self.effect_stage, a);
                            }
                        }
                    }
            
                    self.menu_bar.draw::<B>(f, tab_area);
            
                    let buf = &mut Buffer::empty(Rect::ZERO);
            
                    // (2) Render Main content area depends on which tab is selected
                    match *self.menu_bar.current_tab.borrow() {
                        Tab::TurSheet => self.service_tab.borrow_mut().draw::<B>(f, main_content_area),
                        Tab::Scripts => self.scripts_tab.borrow_mut().draw::<B>(f, main_content_area),
                        Tab::Tasks => self.tasks_tab.borrow_mut().draw::<B>(f, main_content_area),
                        Tab::SystemInfo => self.sysinfo_tab.draw::<B>(f, main_content_area),
                        Tab::Login => self.login_tab.borrow_mut().draw::<B>(f, main_content_area),
                        Tab::Logs => {
                            buf.merge(f.buffer_mut());
                            self.logger.draw::<B>(f, main_content_area);
                        },
                    }
                    // Render notifications (synchronously) at top-right corner
                    if let Ok(mut notifs) = notifications.lock() {
                        notifs.retain(|notif| !notif.is_expired());
                        if let Some(notification) = notifs.last() {
                            let a = notification.notification_area::<B>(f);
                            notification.display::<B>(f);
                            self.effect_stage.process_effects(
                                tachyonfx::Duration::from_millis(16), 
                                f.buffer_mut(), 
                                a
                            );
                        }
                    }
                    // Synchronously render other UI messages
                    if let Ok(messages) = ui_messages.lock() {
                        for ui_message in messages.iter() {
                            ui_message.as_display().render_widget::<B>(f, area);
                        }
                    }
            
                    // Avoid cloning the buffer twice by reusing it
                    // let current_buffer = f.buffer_mut(); // Borrow mutably without cloning initially
                    // let should_send = if let Some(ref cached) = app.cached_buffer {
                    //     !cached.diff(current_buffer).is_empty() // Compare directly with borrowed buffer
                    // } else {
                    //     true // No cache yet, so send
                    // };

                    let now = Instant::now(); // Changed: Throttle buffer sending
                    if now.duration_since(last_sent) >= send_interval {
                        if *can_start {
                            let buffer_to_send = f.buffer_mut().clone();
                            let tx = buffer_tx.clone();
                            let count = f.count();
                            std::thread::scope(|s| {
                                s.spawn(|| {
                                    if let Err(e) = tx.send((count, buffer_to_send)) {
                                        log::warn!("Failed to send buffer: {:?}", e);
                                    }
                                });
                            });
                            last_sent = now;
                        }
                    }
                }
            })?;

        }
        Ok(())
    }
}

fn center_horizontal(area: Rect, width: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    area
}