use ratatui::prelude::*;
use std::cell::RefCell;
use tui_logger::*;
pub mod render;

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
    fn min_width(&self) -> u16 { 4 }

    fn format(&'_ self, _width: usize, evt: &ExtLogRecord) -> Vec<Line<'_>> {
        let mut lines = vec![];
        match evt.level {
            log::Level::Error => {
                let st = Style::new().red().bold();
                let sp = Span::styled("======", st);
                let mayday = Span::from(" ERROR ".to_string());
                let sp2 = Span::styled("======", st);
                lines.push(
                    Line::from(vec![sp, mayday, sp2]).alignment(Alignment::Center)
                );
                lines.push(
                    Line::from(format!("{}: {}", evt.level, evt.msg())).alignment(Alignment::Center),
                );
            }
            _ => {
                lines.push(Line::from(format!("{}: {}", evt.level, evt.msg())));
                lines.push(Line::default());
            }
        };

        match evt.level {
            log::Level::Error => {
                let st = Style::new().blue().bold();
                let sp = Span::styled("======", st);
                let mayday = Span::from(" ERROR ? ".to_string());
                let sp2 = Span::styled("======", st);
                lines.push(Line::from(vec![sp, mayday, sp2]).alignment(Alignment::Center));
            }
            _ => {
                lines.push(Line::default());
            }
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

    fn _next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % self.tab_names.len();
    }
}

