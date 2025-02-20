use std::cell::RefCell;
use ratatui::crossterm::event::KeyEvent;
use ratatui::{prelude::*, widgets::*};
use tui_logger::*;
use log::*;
pub use ratatui::crossterm::event::KeyCode as Key;

pub struct Logger {
    mode: LoggerMode,
    states: RefCell<Vec<TuiWidgetState>>,
    tab_names: Vec<&'static str>,
    selected_tab: usize,
    progress_counter: Option<u16>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum LoggerMode {
    #[default]
    Run,
    Quit,
}

//// Example for simple customized formatter
struct MyLogFormatter {}
impl LogFormatter for MyLogFormatter {
    fn min_width(&self) -> u16 {
        4
    }
    fn format(&self, _width: usize, evt: &ExtLogRecord) -> Vec<Line> {
        let mut lines = vec![];
        match evt.level {
            log::Level::Error => {
                let st = Style::new().red().bold();
                let sp = Span::styled("======", st);
                let mayday = Span::from(" MAYDAY MAYDAY ".to_string());
                let sp2 = Span::styled("======", st);
                lines.push(Line::from(vec![sp, mayday, sp2]).alignment(Alignment::Center));
                lines.push(
                    Line::from(format!("{}: {}", evt.level, evt.msg)).alignment(Alignment::Center),
                );
            }
            _ => {
                lines.push(Line::from(format!("{}: {}", evt.level, evt.msg)));
            }
        };

        match evt.level {
            log::Level::Error => {
                let st = Style::new().blue().bold();
                let sp = Span::styled("======", st);
                let mayday = Span::from(" MAYDAY SEEN ? ".to_string());
                let sp2 = Span::styled("======", st);
                lines.push(Line::from(vec![sp, mayday, sp2]).alignment(Alignment::Center));
            }
            _ => {}
        };
        lines
    }
}


impl Logger {
    pub fn new() -> Logger {
        let states = vec![
            TuiWidgetState::new().set_default_display_level(LevelFilter::Info),
            TuiWidgetState::new().set_default_display_level(LevelFilter::Info),
            TuiWidgetState::new().set_default_display_level(LevelFilter::Info),
            TuiWidgetState::new().set_default_display_level(LevelFilter::Info),
        ];

        // Adding this line had provoked the bug as described in issue #69
        // let states = states.into_iter().map(|s| s.set_level_for_target("some::logger", LevelFilter::Off)).collect();
        let tab_names = vec!["State 1", "State 2", "State 3", "State 4"];
        Logger {
            mode: LoggerMode::Run,
            states: RefCell::new(states),
            tab_names,
            selected_tab: 0,
            progress_counter: None,
        }
    }

    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        debug!(target: "Logger", "Handling UI event: {:?}", key_event);
        let mut states = self.states.borrow_mut();
        let Some(state) = states.get_mut(self.selected_tab) else {return false;};

        match key_event.code.into() {
            Key::Char('q') => self.mode = LoggerMode::Quit,
            // Key::Char('\t') => self.next_tab(),
            // Key::Tab => self.next_tab(),
            Key::Char(' ') => state.transition(TuiWidgetEvent::SpaceKey),
            Key::Esc => state.transition(TuiWidgetEvent::EscapeKey),
            Key::PageUp => state.transition(TuiWidgetEvent::PrevPageKey),
            Key::PageDown => state.transition(TuiWidgetEvent::NextPageKey),
            Key::Up => state.transition(TuiWidgetEvent::UpKey),
            Key::Down => state.transition(TuiWidgetEvent::DownKey),
            Key::Left => state.transition(TuiWidgetEvent::LeftKey),
            Key::Right => state.transition(TuiWidgetEvent::RightKey),
            Key::Char('+') => state.transition(TuiWidgetEvent::PlusKey),
            Key::Char('-') => state.transition(TuiWidgetEvent::MinusKey),
            Key::Char('h') => state.transition(TuiWidgetEvent::HideKey),
            Key::Char('f') => state.transition(TuiWidgetEvent::FocusKey),
            _ => (),
        }
        false
    }


    fn _next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % self.tab_names.len();
    }

    pub fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {     
        self.render(area, f.buffer_mut());
    }
}


impl WidgetRef for &mut Logger {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let progress_height = if self.progress_counter.is_some() {
            3
        } else {
            0
        };
        let [tabs_area, smart_area, main_area, progress_area, help_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(50),
            Constraint::Fill(30),
            Constraint::Length(progress_height),
            Constraint::Length(3),
        ])
        .areas(area);
        // show two TuiWidgetState side-by-side
        let [left, right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(main_area);

        Tabs::new(self.tab_names.iter().cloned())
            .block(Block::default().title("States").borders(Borders::ALL))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .select(self.selected_tab)
            .render(tabs_area, buf);

        let states = self.states.borrow();
        let state = &states[self.selected_tab];

        TuiLoggerSmartWidget::default()
            .style_error(Style::default().fg(Color::Red))
            .style_debug(Style::default().fg(Color::Green))
            .style_warn(Style::default().fg(Color::Yellow))
            .style_trace(Style::default().fg(Color::Magenta))
            .style_info(Style::default().fg(Color::Cyan))
            .output_separator(':')
            .output_timestamp(Some("%H:%M:%S".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
            .output_target(true)
            .output_file(true)
            .output_line(true)
            .state(&state)
            .render(smart_area, buf);

        // An example of filtering the log output. The left TuiLoggerWidget is filtered to only show
        // log entries for the "Logger" target. The right TuiLoggerWidget shows all log entries.
        let filter_state = TuiWidgetState::new()
            .set_default_display_level(LevelFilter::Off)
            .set_level_for_target("Logger", LevelFilter::Debug)
            .set_level_for_target("background-task", LevelFilter::Info);
        let mut _formatter: Option<Box<dyn LogFormatter>> = None;
        _formatter = Some(Box::new(MyLogFormatter {}));

        TuiLoggerWidget::default()
            .block(Block::bordered().title("Filtered Logs"))
            .output_separator('|')
            .output_timestamp(Some("%F %H:%M:%S%.3f".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Long))
            .output_target(false)
            .output_file(false)
            .output_line(false)
            .style(Style::default().fg(Color::White))
            .state(&filter_state)
            .render(left, buf);

        TuiLoggerWidget::default()
            .block(Block::bordered().title("Unfiltered Logs"))
            .opt_formatter(_formatter)
            .output_separator('|')
            .output_timestamp(Some("%F %H:%M:%S%.3f".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Long))
            .output_target(false)
            .output_file(false)
            .output_line(false)
            .style(Style::default().fg(Color::White))
            .render(right, buf);

        if let Some(percent) = self.progress_counter {
            Gauge::default()
                .block(Block::bordered().title("progress-task"))
                .gauge_style((Color::White, Modifier::ITALIC))
                .percent(percent)
                .render(progress_area, buf);
        }
        if area.width > 40 {
            Text::from(vec![
                "Q: Quit | Tab: Switch state | ↑/↓: Select target | f: Focus target".into(),
                "←/→: Display level | +/-: Filter level | Space: Toggle hidden targets".into(),
                "h: Hide target selector | PageUp/Down: Scroll | Esc: Cancel scroll".into(),
            ])
            .style(Color::Gray)
            .centered()
            .render(help_area, buf);
        }
    }
}

