use context::TerminalContext;
use fx::{effect::{ outline_selected_cells, UniqueEffectId}, EffectStage};
use ratatui_splash_screen::{SplashConfig, SplashScreen};
use tabs::{logger::Logger, login::LoginTab, MenuBar, ScriptsTab, ServiceTab, SysinfoTab, Tab};
use styling::CATPPUCCIN;
use tachyonfx::CellFilter;
use widgets::HandleWidget;
use events::{action_handler::{get_event_receiver, EventManager}, EventHandler};
use ratatui::prelude::*;
use std::{cell::RefCell, io, rc::Rc, sync::{Arc, Mutex}};
use ratatui::{
    crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    }, layout::{Constraint, Direction, Flex, Layout},
};

use crate::filesystem::system_info::get_sysinfo_no_gpu;

pub mod widgets;
pub mod tabs;
pub mod events;
pub mod styling;
pub mod fx;
pub mod data;
pub mod context;

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

// impl <'a> TerminalApp <'a> { fn log_message(&mut self, message: &str) { self.logs.push(message.to_string()); } }

pub struct TerminalApp<'a> { // logs: Vec<String>,
    logger: Logger,
    menu_bar: MenuBar<'a>,
    scripts_tab: Rc<RefCell<ScriptsTab<'a>>>,
    service_tab: ServiceTab<'a>,
    sysinfo_tab: SysinfoTab,
    login_tab: LoginTab <'a>,
    effect_stage: EffectStage<UniqueEffectId>,
    first_run: bool,
    event_handler: EventHandler,
    event_manager: EventManager<'a>,
    ctx: Arc<Mutex<TerminalContext>>
}

impl Default for TerminalApp <'_>{
    fn default() -> Self {
        // Create a global event channel.
        let mut event_manager = EventManager::new(get_event_receiver());
        let service_tab = ServiceTab::new();
        let scripts_tab = Rc::new(RefCell::new(ScriptsTab::new()));
        // Register the ServiceFormWidget with the event manager.
        // Here we clone the Rc so both ServiceTab and the EventManager share it.
        event_manager.register_handler(service_tab.service_form_widget.clone());
        event_manager.register_handler(scripts_tab.clone());
        let ctx = Arc::new(Mutex::new(TerminalContext::default()));

        Self {
            logger: Logger::new(),
            menu_bar: MenuBar::new(),
            scripts_tab,
            service_tab,
            sysinfo_tab: SysinfoTab::new(),
            login_tab: LoginTab::new(ctx.clone()),
            effect_stage: EffectStage::default(),
            event_handler: EventHandler::new(),
            first_run: true,
            event_manager,
            ctx
        }
    }
}

pub async fn run_terminal_mode() -> anyhow::Result<(), anyhow::Error> {
    // Set max_log_level to Trace
    // tui_logger::init_logger(log::LevelFilter::Info).unwrap();
    // // Set default level for unknown targets to Trace
    // tui_logger::set_default_level(log::LevelFilter::Info);

    let log_level = log::LevelFilter::Info;
    let log_file = std::fs::File::create("output.log").unwrap();
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

    let res = run_app(&mut terminal, app);

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

fn run_app<'a, B: Backend>(terminal: &mut Terminal<B>, mut app: TerminalApp<'a>) -> anyhow::Result<(), anyhow::Error> {
    // render splash screen
    log::info!("Running splash");
    let mut splash_screen = SplashScreen::new(SPLASH_CONFIG)?;
    log::info!("Running splash 2");
    let mut splash_screen2 = SplashScreen::new(SPLASH_CONFIG2)?;
    log::info!("Entering main loop");
    loop {
        if let Ok(events) = app.event_handler.next() {
            let current_tab = app.menu_bar.current_tab.borrow().clone();
            match events {
                events::Event::Key(key_event) => {
                    let ctrl_key = key_event.modifiers.contains(KeyModifiers::CONTROL);
                    match key_event.code {
                        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                            log::info!("Quitting");
                            break;
                        }
                        _ => {
                            if ctrl_key {
                                match key_event.code {
                                    // We'll let left/right arrow change tabs
                                    KeyCode::Right => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                        log::info!("Current tab: {current_tab:?}");
                                        match current_tab {
                                            Tab::TurSheet => app.menu_bar.set_active_tab(Tab::Scripts),
                                            Tab::Scripts => app.menu_bar.set_active_tab(Tab::SystemInfo),
                                            Tab::SystemInfo => app.menu_bar.set_active_tab(Tab::Logs),
                                            Tab::Logs => app.menu_bar.set_active_tab(Tab::Login),
                                            Tab::Login => app.menu_bar.set_active_tab(Tab::TurSheet),
                                        };
                                    }
                                    KeyCode::Left => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                        match current_tab {
                                            Tab::TurSheet => app.menu_bar.set_active_tab(Tab::Login),
                                            Tab::Scripts => app.menu_bar.set_active_tab(Tab::TurSheet),
                                            Tab::SystemInfo => app.menu_bar.set_active_tab(Tab::Scripts),
                                            Tab::Logs => app.menu_bar.set_active_tab(Tab::SystemInfo),
                                            Tab::Login => app.menu_bar.set_active_tab(Tab::Logs),
                                        };
                                    }
                                    _ => {}
                                }
                            }

                            // Now dispatch key event to the active widget, and only one widget:
                            let consumed = match current_tab {
                                Tab::TurSheet => app.service_tab.handle_key_event(key_event),
                                Tab::Scripts => app.scripts_tab.borrow_mut().handle_key_event(key_event),
                                Tab::SystemInfo => app.service_tab.handle_key_event(key_event),
                                Tab::Logs => app.logger.handle_key_event(key_event),
                                Tab::Login => app.login_tab.handle_key_event(key_event),
                            };

                            if consumed {}
                        }
                    };
                },
                events::Event::Mouse(mouse_event) => {
                    app.menu_bar.handle_mouse_event(&mouse_event);
                     match current_tab {
                        Tab::TurSheet => app.service_tab.handle_mouse_event(&mouse_event),
                        Tab::Scripts => app.scripts_tab.borrow_mut().handle_mouse_event(&mouse_event),
                        Tab::SystemInfo => app.service_tab.handle_mouse_event(&mouse_event),
                        Tab::Logs => {}
                        Tab::Login => app.login_tab.handle_mouse_event(&mouse_event),
                    };
                },
                events::Event::Error => log::info!("Error in event loop"),
                events::Event::Tick => {}
            }
        }

        terminal.draw(|f| {
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
                
                app.event_manager.process_events();
                if let Ok(mut lock) = app.ctx.lock() {
                    lock.receive();
                }
                // top-level layout has a row for tabs, then main content
                // let bg = Block::default().style(Style::default().bg(Color::Rgb(8, 8, 12)));
                // f.render_widget(bg, f.area());
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
                        Constraint::Percentage(20),
                        Constraint::Percentage(60),
                        Constraint::Percentage(20),
                    ]).split(outer_chunks[0]);

                let tab_area = center_horizontal(tab_layout[1], tab_layout[1].width);
                let main_content_area = outer_chunks[1];
                
                if app.first_run {
                    app.first_run = false;
                    // let effect = selected_category(CATPPUCCIN.flamingo, tab_area);
                    // let effect2 = open_category(CATPPUCCIN.peach, tab_area);
                    // let effect2 = outline_selected_cells(
                    //     &mut app.menu_bar.effect_stage, 
                    //     tab_area.as_size(),
                    //     CATPPUCCIN.blue,
                    //     CellFilter::All
                    // );

                    // let effect4 = outline_selected_cells(
                    //     &mut app.menu_bar.effect_stage, 
                    //     main_content_area.as_size(),
                    //     CATPPUCCIN.lavender,
                    //     CellFilter::FgColor(CATPPUCCIN.lavender)
                    // );

                    // let effect5 = outline_selected_cells(
                    //     &mut app.menu_bar.effect_stage, 
                    //     main_content_area.as_size(),
                    //     CATPPUCCIN.teal,
                    //     CellFilter::FgColor(CATPPUCCIN.teal)
                    // );

                    let effect1 = outline_selected_cells(
                        &mut app.menu_bar.effect_stage, 
                        main_content_area.as_size(),
                        CATPPUCCIN.maroon,
                        CellFilter::FgColor(CATPPUCCIN.maroon)
                    );

                    // app.menu_bar.effect_stage.add_effect(effect2);
                    // app.service_tab.effect_stage.add_effect(effect3);
                    app.effect_stage.add_effect(effect1);
                    // app.sysinfo_tab.effect_stage.add_effect(effect3);
                    // app.sysinfo_tab.effect_stage.add_effect(effect4);
                    // app.sysinfo_tab.effect_stage.add_effect(effect5);
                    // app.sysinfo_tab.effect_stage.add_effect(effect3);
                    // app.service_tab.effect_stage.add_effect(effect5);
                }

                app.menu_bar.draw::<B>(f, tab_area);

                // match app.ctx.state {
                //     AppState::Authenticated(page) => {},
                //     AppState::NoAuth(reason) => {
                //         if reason.to_string().contains("Already connected") && app.ctx.current_user.is_some() {
                //             log::info!("Already connected");
                //             let _ = app.ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                //         } else {
                //             let _ = app.ctx.app_state_tx.try_send(AppState::Login);
                //         }
                //     },
                //     AppState::Login => {
                //         // app.menu_bar.current_tab.replace(Tab::)
                //     },
                //     _ => {}
                // }
                let buf = &mut Buffer::empty(Rect::ZERO);

                // (2) Render Main content area depends on which tab is selected
                match *app.menu_bar.current_tab.borrow() {
                    Tab::TurSheet => app.service_tab.draw::<B>(f, main_content_area),
                    Tab::Scripts => app.scripts_tab.borrow_mut().draw::<B>(f, main_content_area),
                    Tab::SystemInfo => app.sysinfo_tab.draw::<B>(f, main_content_area),
                    Tab::Login => app.login_tab.draw::<B>(f, main_content_area),
                    Tab::Logs => {
                        buf.merge(f.buffer_mut());
                        // logger.render(main_content_area, buf);
                        app.logger.draw::<B>(f, main_content_area);
                    },
                }

                // app.service_tab.service_form_widget.receive();

                // ----- Process TachyonFX Effects -----
                // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
                // let fx_duration = tachyonfx::Duration::from_millis(16);
                // Process all effects added to our effect_stage. They will update and render onto f's buffer.
                // app.menu_bar.effect_stage.process_effects(fx_duration, f.buffer_mut(), tab_area);
                // app.service_tab.effect_stage.process_effects(fx_duration, f.buffer_mut(), main_content_area);
                // app.sysinfo_tab.effect_stage.process_effects(fx_duration, f.buffer_mut(), main_content_area);
                // let area = f.area();
                // app.effect_stage.process_effects(fx_duration, f.buffer_mut(), main_content_area);
            }
        })?;
    }
    Ok(())
}

fn center_horizontal(area: Rect, width: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    area
}