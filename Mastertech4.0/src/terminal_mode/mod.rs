use ratatui::{crossterm::{ event::{DisableMouseCapture, EnableMouseCapture}, execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},}, layout::{Constraint, Direction, Layout}};
use tabs::{assistant::AssistantTab, logger::Logger, login::LoginTab, menu_bar::Tab, service_form::ServiceFormTab, settings::SettingsTab, stress::StressTab, tasks::TasksTab, webconsole::WebconsoleTab, MenuBar, NcduTab, ScriptsTab, SysinfoTab};
use systems::{communication_system::Message, data_system::DataSystem, notification_system::Notification, render_system::RenderSystem, widget_render_system::WidgetRenderer};
use std::{cell::RefCell, io, rc::Rc, sync::{Arc, Mutex}, time::{Duration, Instant}};
use events::{action_handler::{get_event_receiver, EventManager}, EventHandler};
use widgets::splash_screen::{SplashConfig, SplashScreen};
use websockets::TerminalWebsocketClient;
use crossbeam::channel::unbounded;
use context::TerminalContext;
use widgets::HandleWidget;
use data::LocalTermEvent;
use ratatui::prelude::*;
use reqwest::Client;
use styling::THEME;

pub mod systems;
pub mod widgets;
pub mod tabs;
pub mod events;
pub mod styling;
pub mod fx;
pub mod data;
pub mod context;
pub mod websockets;
pub mod modals;
pub mod embedded;

static SPLASH_CONFIG: SplashConfig = SplashConfig {
    image_data: include_bytes!("../assets/masterlogoV3.png"),
    sha256sum: None,
    render_steps: 8,
    use_colors: true,
};

pub struct TerminalApp<'a> {
    logger: Logger,
    menu_bar: Rc<RefCell<MenuBar<'a>>>,
    scripts_tab: Rc<RefCell<ScriptsTab<'a>>>,
    service_tab: Rc<RefCell<ServiceFormTab<'a>>>,
    ncdu_tab: Rc<RefCell<NcduTab>>,
    tasks_tab: Rc<RefCell<TasksTab<'a>>>,
    stress_tab: Rc<RefCell<StressTab>>,
    sysinfo_tab: SysinfoTab,
    login_tab: Rc<RefCell<LoginTab<'a>>>,
    webconsole_tab: Rc<RefCell<WebconsoleTab<'a>>>,
    settings_tab: Rc<RefCell<SettingsTab<'a>>>,
    assistant_tab: Rc<RefCell<AssistantTab<'a>>>,
    event_handler: EventHandler,
    event_manager: EventManager<'a>,
    ctx: Arc<Mutex<TerminalContext>>,
    render_system: Arc<RenderSystem>,
    data_system: Arc<DataSystem>,
    manual_connect_rx: tokio::sync::mpsc::UnboundedReceiver<bool>,
    plugin_manager: Arc<std::sync::RwLock<displays::plugins::PluginManager>>,
}

pub async fn run_terminal_mode() -> anyhow::Result<(), anyhow::Error> {
    log::info!("STARTING TERM MODE");
    enable_raw_mode()?;
    log::info!("Hooking StdOut");
    let mut stdout = io::stdout();
    execute!(
        stdout, 
        EnterAlternateScreen, 
        EnableMouseCapture
    )?;
    log::info!("Creating Crossterm backend");
    let backend = CrosstermBackend::new(stdout);
    log::info!("Creating Terminal");
    let mut terminal = Terminal::new(backend)?;

    let mut app = TerminalApp::default();

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
        log::error!("Err: {:?}", err);
    }

    Ok(())
}

impl Default for TerminalApp<'_> {
    fn default() -> Self {
        Self::build(EventHandler::new())
    }
}

impl<'a> TerminalApp<'a> {
    fn build(event_handler: EventHandler) -> Self {
        let client = Client::new();
        // Create channels explicitly for communication between Data and Render systems
        let (data_to_render_tx, data_to_render_rx) = unbounded::<Box<dyn Message>>();
        let (render_to_data_tx, render_to_data_rx) = unbounded::<Box<dyn Message>>();
        let (manual_connect_tx, manual_connect_rx) = tokio::sync::mpsc::unbounded_channel();

        // Global App Context, passed through most widgets / event handlers / 'Systems' via Arc 
        let ctx = Arc::new(Mutex::new(TerminalContext::new(render_to_data_tx.clone(), data_to_render_tx.clone())));

        // Create systems separately
        let render_system: Arc<RenderSystem> = Arc::new(RenderSystem::new(render_to_data_tx, data_to_render_rx, ctx.clone()));
        let data_system: Arc<DataSystem> = Arc::new(DataSystem::new(data_to_render_tx, render_to_data_rx));

        // Create a global event channel.
        let mut event_manager = EventManager::new(get_event_receiver());
        let service_tab = Rc::new(RefCell::new(ServiceFormTab::new(client.clone(), ctx.clone())));
        let tasks_tab = Rc::new(RefCell::new(TasksTab::new(client.clone(), ctx.clone())));
        let ncdu_tab = Rc::new(RefCell::new(NcduTab::new(ctx.clone())));

        let scripts_tab = Rc::new(
            RefCell::new(
                ScriptsTab::new(
                    client.clone(), 
                    ctx.clone()
                )
            )
        );
        
        let login_tab = Rc::new(RefCell::new(LoginTab::new(client.clone(), ctx.clone())));
        let webconsole_tab = Rc::new(RefCell::new(WebconsoleTab::new(client.clone(), ctx.clone())));
        let settings_tab = Rc::new(RefCell::new(SettingsTab::new(ctx.clone())));
        let assistant_tab = Rc::new(RefCell::new(AssistantTab::new()));

        let sysinfo_tab = SysinfoTab::new();
        let stress_tab = Rc::new(RefCell::new(StressTab::new()));
        let menu_bar = Rc::new(RefCell::new(MenuBar::new(ctx.clone(), manual_connect_tx)));
        
        // Register the ServiceFormTab with the event manager.
        // Here we clone the Rc so both ServiceTab and the EventManager share it.
        event_manager.register_handler(service_tab.clone());
        event_manager.register_handler(scripts_tab.clone());
        event_manager.register_handler(login_tab.clone());
        event_manager.register_handler(webconsole_tab.clone());
        event_manager.register_handler(menu_bar.clone());
        event_manager.register_handler(tasks_tab.clone());
        event_manager.register_handler(settings_tab.clone());

        let plugin_manager = Arc::new(std::sync::RwLock::new(
            displays::plugins::PluginManager::new(),
        ));

        Self {
            ctx,
            menu_bar,
            login_tab,
            tasks_tab,
            stress_tab,
            scripts_tab,
            service_tab,
            sysinfo_tab,
            webconsole_tab,
            settings_tab,
            assistant_tab,
            ncdu_tab,
            data_system,
            render_system,
            event_manager,
            logger: Logger::new(),
            event_handler,
            manual_connect_rx,
            plugin_manager,
        }
    }
}

impl <'a>TerminalApp<'a> {
    async fn ui<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> anyhow::Result<(), anyhow::Error> 
    where 
        <B as Backend>::Error: Send + Sync + 'static
    {
        let last_sent = &mut Instant::now();
        let send_interval = Duration::from_millis(30); 
        let can_start = &mut false;

        // render splash screen
        let mut splash_screen = SplashScreen::new(SPLASH_CONFIG)?;

        // Create a shutdown channel
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

        // Clone Arc for running background tasks
        let render_system_bg: Arc<RenderSystem> = Arc::clone(&self.render_system);
        let data_system_bg: Arc<DataSystem> = Arc::clone(&self.data_system);

        let mut join_handles = Vec::new();
        let shutdown_rx_data = shutdown_tx.subscribe();
        let shutdown_rx_websocket = shutdown_tx.subscribe();
        let shutdown_rx_render = shutdown_tx.subscribe();

        // Standalone TUI: serve the :9004 MCP so the Assistant tab's Claude Code
        // session has tools (the egui process normally starts this in logic()).
        let mgr_http = self.plugin_manager.clone();
        join_handles.push(tokio::spawn(async move {
            if let Err(e) = displays::plugins::mcp_bridge::run_plugin_mcp_server_http(mgr_http).await {
                log::error!("Plugin MCP HTTP server error: {e:?}");
            }
        }));

        // Spawn a background task to drain the WASM plugin load channel
        let pm_bg = self.plugin_manager.clone();
        let mut shutdown_rx_plugins = shutdown_tx.subscribe();
        join_handles.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx_plugins.recv() => break,
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                }
                if let Ok(mut mgr) = pm_bg.write() {
                    mgr.process_events();
                }
            }
        }));
        let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();
        let (start_tx, mut start_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<LocalTermEvent>();
        let (connection_state_tx, mut connection_state_rx) = tokio::sync::mpsc::unbounded_channel();
        
        // Run DataSystem in the background
        join_handles.push(
            tokio::spawn(async move {
                data_system_bg.run(shutdown_rx_data).await;
            })
        );

        // Run RenderSystem in the background
        join_handles.push(
            tokio::spawn(async move {
                render_system_bg.run(shutdown_rx_render).await;
            })
        );

        join_handles.push(
            tokio::spawn(async move {
                let websocket_server = // 
                TerminalWebsocketClient::new().start_websocket_sender(
                    buffer_rx, 
                    start_tx.clone(),
                    connection_state_tx,
                    event_tx, 
                    shutdown_rx_websocket,
                ).await;
                log::info!("websocket_server: {websocket_server:?}");
            })
        );

        // Start the direct-TCP admin listener so admins on the same LAN can
        // connect directly without going through the WS relay.  Terminal mode
        // uses the same tcp_listener infrastructure as Egui mode; the only
        // difference is we initiate it here instead of first_run.rs.
        let tcp_client_id = crate::filesystem::get_client_hash().id;
        join_handles.push(tokio::spawn(async move {
            // Give the WS sender a head start on creating / updating the DB
            // row before we try to publish local_ip + tcp_port to that row.
            let head_start = if crate::tcp_listener::is_self_update_child() {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(3)
            };
            tokio::time::sleep(head_start).await;
            crate::tcp_listener::spawn_direct_tcp_listener(tcp_client_id).await;
        }));
        loop {
            if self.handle_events(None, None) { 
                // Signal process-wide shutdown so the TCP accept loop also
                // exits cleanly (it waits on displays::wait_for_shutdown).
                displays::signal_shutdown();
                // Signal shutdown
                shutdown_tx.send(())?;
                log::info!("Sent shutdown signal");
        
                // Wait for tasks to complete with a timeout
                for handle in join_handles {
                    handle.abort();
                }
                break; 
            }
            
            if let Ok(start) = start_rx.try_recv() {
                *can_start = start;
            }

            if let Ok(start) = self.manual_connect_rx.try_recv() {
                *can_start = start;
                // *manual_start = start;
            }

            terminal.draw(|f: &mut Frame<'_>| {
                let area = f.area();
                // Apply consistent dark background across the entire frame
                // This ensures all areas, including gaps between widgets, have the same background
                f.buffer_mut().set_style(area, Style::new().bg(THEME.bg));
                if !splash_screen.is_rendered() {
                    Self::render_splash_screen(f, &mut splash_screen);
                } else {
                    // Process events from egui via WebSocket
                    while let Ok(event) = event_rx.try_recv() {
                        if let Ok(mouse) = TryFrom::try_from(event.clone()) {
                            if self.handle_events(Some(mouse), None) {
                                log::info!("Quit signal received from handle_events (mouse)");
                            }
                        } else if let Ok(key) = TryFrom::try_from(event) {
                            if self.handle_events(None, Some(key)) {
                                log::info!("Quit signal received from handle_events (key)");
                            }
                        }
                    }

                    if let Ok(connection_state) = connection_state_rx.try_recv() {
                        if let Ok(mut menu) = self.menu_bar.try_borrow_mut() {
                            menu.set_connection_state(connection_state);
                        }
                    }

                    self.render_frame::<B>(f);
                    Self::send_buffer(f, last_sent, send_interval, can_start, buffer_tx.clone());
                }
            })?;
        }
        Ok(())
    }

    /// Construct a `TerminalApp` for rendering inside egui through a
    /// `RataguiBackend`. Uses an inert event handler since input is injected
    /// via `handle_events`, skipping the crossterm/raw-mode terminal.
    pub fn new_embedded() -> Self {
        Self::build(EventHandler::new_inert())
    }

    /// Mark the embedded terminal as logged in using the egui process's already
    /// authenticated session. Seeds the user so `RenderSystem` fetches data over
    /// the shared global database; no new `Database::new()` connection is made.
    pub fn seed_authenticated_user(&self, user: database::schema::User) {
        if let Ok(ctx) = self.ctx.lock() {
            let _ = ctx
                .data_sender
                .send(Box::new(systems::communication_system::DataMessage(user)));
            let _ = ctx
                .app_state_tx
                .try_send(crate::AppState::Authenticated(displays::app_state::MainPages::Tasks));
        }
    }

    /// Spawn the data and render background systems against `shutdown_tx`.
    /// Omits the websocket sender and direct-TCP listener that the GUI process
    /// already owns.
    pub fn spawn_core_systems(
        &self,
        shutdown_tx: &tokio::sync::broadcast::Sender<()>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let data_system = self.data_system.clone();
        let data_shutdown = shutdown_tx.subscribe();
        let render_system = self.render_system.clone();
        let render_shutdown = shutdown_tx.subscribe();
        vec![
            tokio::spawn(async move { data_system.run(data_shutdown).await; }),
            tokio::spawn(async move { render_system.run(render_shutdown).await; }),
        ]
    }

    /// Render one frame of the TUI into `f`: menu bar, active tab content, and
    /// notification/message overlays. Shared by the standalone loop and the
    /// embedded egui driver.
    pub fn render_frame<B: Backend>(&mut self, f: &mut Frame) {
        let area = f.area();
        f.buffer_mut().set_style(area, Style::new().bg(THEME.bg));

        if let Ok(mut menu) = self.menu_bar.try_borrow_mut() {
            menu.check_active_tab();
        }
        self.event_manager.process_events();
        self.tasks_tab.borrow_mut().check_tasks();

        // Reserved above everything else: painting it is what admits RemoteExec jobs.
        let (banner_area, area) = crate::remote_exec::banner_tui::split(area);
        if let Some(banner_area) = banner_area {
            crate::remote_exec::banner_tui::render(f, banner_area);
        }

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Fill(1)]);
        let outer_chunks = layout.split(area);
        let tab_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(outer_chunks[0]);

        let _ = self.menu_bar::<B>(f, tab_layout, outer_chunks);
        let notifications = self.render_system.notifications.clone();
        let ui_messages = self.render_system.ui_messages.clone();
        self.render_systems::<B>(f, notifications, ui_messages);
    }

    fn menu_bar<B: Backend>(&mut self, f: &mut Frame, tab_layout: Rc<[Rect]>, outer_chunks: Rc<[Rect]>) -> anyhow::Result<(), anyhow::Error> {
        let tab_area = tab_layout[0]; // center_horizontal(tab_layout[1], tab_layout[1].width);
        let main_content_area = outer_chunks[1];
        let menu_bar = self.menu_bar.try_borrow_mut();

        if let Ok(mut menu_bar) = menu_bar {
            menu_bar.draw::<B>(f, tab_area);

            let buf = &mut Buffer::empty(Rect::ZERO);

            // Sysinfo polling only runs while the System tab is visible.
            if *menu_bar.current_tab.borrow() != Tab::SystemInfo {
                self.sysinfo_tab.stop_polling();
            }

            // (2) Render Main content area depends on which tab is selected
            match *menu_bar.current_tab.borrow() {
                Tab::TurSheet => self.service_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Scripts => self.scripts_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Tasks => self.tasks_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Stress => self.stress_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::SystemInfo => self.sysinfo_tab.draw::<B>(f, main_content_area),
                Tab::Login => self.login_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Webconsole => self.webconsole_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Settings => self.settings_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Assistant => self.assistant_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Ncdu => self.ncdu_tab.borrow_mut().draw::<B>(f, main_content_area),
                Tab::Logs => {
                    buf.merge(f.buffer_mut());
                    self.logger.draw::<B>(f, main_content_area);
                },
            }

            // Dropdown overlay paints on top of the content tab.
            menu_bar.draw_overlay(f);
        }
        Ok(())
    }

    fn render_splash_screen(f: &mut Frame, splash_screen: &mut SplashScreen) {
        let layout_cols = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ]).split(f.area());

        let layout_rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ]).split(layout_cols[1]);

        f.render_widget(splash_screen, layout_rows[1]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    fn render_systems<B: Backend>(
        &mut self, 
        f: &mut Frame,
        notifications: Arc<Mutex<Vec<Notification>>>, 
        ui_messages: Arc<Mutex<Vec<Box<dyn Message>>>>
    ) {
        let area = f.area();
        // Render notifications (synchronously) at top-right corner
        if let Ok(mut notifs) = notifications.lock() {
            notifs.retain(|notif| !notif.is_expired());
            if let Some(notification) = notifs.last() {
                notification.display::<B>(f);
            }
        }
        // Synchronously render other UI messages
        if let Ok(messages) = ui_messages.lock() {
            for ui_message in messages.iter() {
                ui_message.as_display().render_widget::<B>(f, area);
            }
        }
    }
}
