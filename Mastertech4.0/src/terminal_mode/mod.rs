use events::EventHandler;
use fx::{effect::{led_kbd_border, open_category, selected_category, UniqueEffectId}, EffectStage};
use tabs::{MenuBar, ScriptsTab, ServiceTab, SysinfoTab, Tab};
use styling::{CATPPUCCIN, C_DEEPPINK};
use widgets::HandleWidget;
use std::io;
use ratatui::{
    crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    }, layout::{Constraint, Direction, Layout}, prelude::*, widgets::Block,
};

pub mod widgets;
pub mod tabs;
pub mod events;
pub mod styling;
pub mod fx;
// impl <'a> App <'a> { fn log_message(&mut self, message: &str) { self.logs.push(message.to_string()); } }

pub struct App<'a> { // logs: Vec<String>,
    menu_bar: MenuBar<'a>,
    scripts_tab: ScriptsTab<'a>,
    service_tab: ServiceTab<'a>,
    sysinfo_tab: SysinfoTab,
    effect_stage: EffectStage<UniqueEffectId>,
    first_run: bool,
    event_handler: EventHandler
}

impl Default for App <'_>{
    fn default() -> Self {
        Self {
            menu_bar: MenuBar::new(),
            scripts_tab: ScriptsTab::new(),
            service_tab: ServiceTab::new(),
            sysinfo_tab: SysinfoTab::new(),
            effect_stage: EffectStage::default(),
            event_handler: EventHandler::new(),
            first_run: true
        }
    }
}

pub(crate) async fn run_terminal_mode() -> anyhow::Result<(), anyhow::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::default();
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

fn run_app<'a, B: Backend>(terminal: &mut Terminal<B>, mut app: App<'a>) -> anyhow::Result<(), anyhow::Error> {
    loop {
        if let Ok(events) = app.event_handler.next() {
            match events {
                events::Event::Key(key_event) => {
                    match key_event.code {
                        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                            log::info!("Quitting");
                            break;
                        }
                        // We'll let left/right arrow change tabs
                        KeyCode::Right => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                            app.menu_bar.selected_tab = match app.menu_bar.selected_tab {
                                Tab::TurSheet => Tab::Scripts,
                                Tab::Scripts => Tab::SystemInfo,
                                Tab::SystemInfo => Tab::TurSheet,
                            };
                        }
                        KeyCode::Left => if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                            app.menu_bar.selected_tab = match app.menu_bar.selected_tab {
                                Tab::TurSheet => Tab::SystemInfo,
                                Tab::Scripts => Tab::TurSheet,
                                Tab::SystemInfo => Tab::Scripts,
                            };
                        }
                        _ => {}
                    };
                    match app.menu_bar.selected_tab() {
                        Tab::TurSheet => app.service_tab.handle_key_event(key_event),
                        Tab::Scripts => app.scripts_tab.handle_key_event(key_event),
                        Tab::SystemInfo => app.service_tab.handle_key_event(key_event),
                    };
                },
                events::Event::Mouse(mouse_event) => {
                    app.menu_bar.handle_mouse_event(mouse_event);
                    match app.menu_bar.selected_tab() {
                        Tab::TurSheet => app.service_tab.handle_mouse_event(mouse_event),
                        Tab::Scripts => app.scripts_tab.handle_mouse_event(mouse_event),
                        Tab::SystemInfo => app.service_tab.handle_mouse_event(mouse_event),
                    };
                },
                events::Event::Error => log::info!("Error in event loop"),
                events::Event::Tick => {}
            }
        }
        let _ = app.service_tab.receive_ticket();

        terminal.draw(|f| {
            // top-level layout has a row for tabs, then main content
            let bg = Block::default().style(Style::default().bg(Color::Rgb(8, 8, 12)));
            f.render_widget(bg, f.area());

            let layout = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3), // for tabs
                    Constraint::Min(1),    // rest of content
                ]);

            let outer_chunks = layout.split(f.area());

            let tab_area = outer_chunks[0];
            let main_content_area = outer_chunks[1];

            if app.first_run {
                app.first_run = false;
                // let effect = selected_category(CATPPUCCIN.flamingo, tab_area);
                let effect1 = selected_category(CATPPUCCIN.sapphire, main_content_area);
                let effect2 = open_category(CATPPUCCIN.peach, tab_area);
                let effect3 = led_kbd_border();
                app.menu_bar.effect_stage.add_effect(effect3);
                app.service_tab.effect_stage.add_effect(effect1);
                app.effect_stage.add_effect(effect2);
                // app.effect_stage.add_effect(effect1);        
            }

            app.menu_bar.draw::<B>(f, tab_area);
            // (2) Render Main content area depends on which tab is selected
            match app.menu_bar.selected_tab() {
                Tab::TurSheet => app.service_tab.draw::<B>(f, main_content_area),
                Tab::Scripts => app.scripts_tab.draw::<B>(f, main_content_area),
                Tab::SystemInfo => app.sysinfo_tab.draw::<B>(f, main_content_area)
            }

            // ----- Process TachyonFX Effects -----
            // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
            let fx_duration = tachyonfx::Duration::from_millis(16);
            // Process all effects added to our effect_stage. They will update and render onto f's buffer.
            app.menu_bar.effect_stage.process_effects(fx_duration, f.buffer_mut(), tab_area);
            app.service_tab.effect_stage.process_effects(fx_duration, f.buffer_mut(), main_content_area);
            let area = f.area();
            app.effect_stage.process_effects(fx_duration, f.buffer_mut(), area);
        })?;
    }
    Ok(())
}
