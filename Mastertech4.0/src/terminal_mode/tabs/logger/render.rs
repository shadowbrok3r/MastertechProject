use ratatui::crossterm::event::KeyEvent;
use ratatui::{prelude::*, widgets::*};
use tui_logger::*;
use log::*;
pub use ratatui::crossterm::event::KeyCode as Key;
use crate::terminal_mode::tabs::logger::LoggerMode;
use crate::terminal_mode::widgets::HandleWidget;
use crate::terminal_mode::styling::{CATPPUCCIN, THEME};

use super::{Logger, MyLogFormatter};

impl <'a>HandleWidget<'a> for Logger {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let buf = f.buffer_mut();
        let progress_height = if self.progress_counter.is_some() {
            3
        } else {
            0
        };
        let [tabs_area, smart_area, bottom, progress_area, help_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(80),
            Constraint::Fill(10),
            Constraint::Length(progress_height),
            Constraint::Length(2),
        ])
        .areas(area);

        Tabs::new(self.tab_names.iter().cloned())
            .block(Block::default().title("States").title_style(THEME.title()).borders(Borders::ALL).border_style(Style::default().fg(THEME.tertiary)))
            .highlight_style(Style::default().fg(THEME.accent).add_modifier(Modifier::REVERSED))
            .select(self.selected_tab)
            .render(tabs_area, buf);

        let states = self.states.borrow();
        let state = &states[self.selected_tab];

        TuiLoggerSmartWidget::default()
            .style_error(Style::default().fg(CATPPUCCIN.red))
            .style_debug(Style::default().fg(CATPPUCCIN.green))
            .style_warn(Style::default().fg(CATPPUCCIN.yellow))
            .style_trace(Style::default().fg(CATPPUCCIN.mauve))
            .style_info(Style::default().fg(CATPPUCCIN.sky))
            .output_separator(':')
            .output_timestamp(Some("%H:%M:%S".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
            .output_target(true)
            .output_file(false)
            .output_line(true)
            .state(&state)
            .render(smart_area, buf);

        let mut _formatter: Option<Box<dyn LogFormatter>> = None;
        _formatter = Some(Box::new(MyLogFormatter {}));

        // Flash copy confirmation in the title for a few seconds.
        let unfiltered_title = match self.copied_feedback {
            Some((at, lines)) if at.elapsed().as_secs() < 4 => {
                format!("Unfiltered Logs — Copied {lines} lines to clipboard ✔")
            }
            _ => "Unfiltered Logs".to_string(),
        };

        TuiLoggerWidget::default()
            .block(Block::bordered().title(unfiltered_title).title_style(THEME.title()).border_style(Style::default().fg(THEME.tertiary)))
            .opt_formatter(_formatter)
            .output_separator('|')
            .output_timestamp(Some("%F %H:%M:%S%.3f".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
            .output_target(false)
            .output_file(false)
            .output_line(true)
            .style(Style::default().fg(CATPPUCCIN.text))
            .render(bottom, buf);

        if let Some(percent) = self.progress_counter {
            Gauge::default()
                .block(Block::bordered().title("progress-task").title_style(THEME.title()))
                .gauge_style((THEME.accent, Modifier::ITALIC))
                .percent(percent)
                .render(progress_area, buf);
        }
        if area.width > 120 {
            Text::from(vec![
                "Q: Quit | Tab: Switch state | ↑/↓: Select target | f: Focus target | ←/→: Display level | +/-: Filter level".into(),
                "c: Copy all logs | Space: Toggle hidden targets | h: Hide target selector | PageUp/Down: Scroll | Esc: Cancel scroll".into(),
            ])
            .style(CATPPUCCIN.subtext0)
            .centered()
            .render(help_area, buf);
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        debug!(target: "Logger", "Handling UI event: {:?}", key_event);
        if key_event.code == Key::Char('c') {
            self.copy_all_logs();
            return false;
        }
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
    
    fn handle_mouse_event(&self, mouse_event: &ratatui::crossterm::event::MouseEvent) { 
        let mut states = self.states.borrow_mut();
        let Some(state) = states.get_mut(self.selected_tab) else {return;};

        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollUp => state.transition(TuiWidgetEvent::PrevPageKey),
            ratatui::crossterm::event::MouseEventKind::ScrollDown => state.transition(TuiWidgetEvent::NextPageKey),
            _ => {}
        }
    }
}

