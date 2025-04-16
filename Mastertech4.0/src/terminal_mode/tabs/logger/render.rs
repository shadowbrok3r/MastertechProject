use ratatui::crossterm::event::KeyEvent;
use ratatui::{prelude::*, widgets::*};
use tui_logger::*;
use log::*;
pub use ratatui::crossterm::event::KeyCode as Key;
use crate::terminal_mode::tabs::logger::LoggerMode;
use crate::terminal_mode::widgets::HandleWidget;

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
            .output_file(false)
            .output_line(true)
            .state(&state)
            .render(smart_area, buf);

        let mut _formatter: Option<Box<dyn LogFormatter>> = None;
        _formatter = Some(Box::new(MyLogFormatter {}));

        TuiLoggerWidget::default()
            .block(Block::bordered().title("Unfiltered Logs"))
            .opt_formatter(_formatter)
            .output_separator('|')
            .output_timestamp(Some("%F %H:%M:%S%.3f".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
            .output_target(false)
            .output_file(false)
            .output_line(true)
            .style(Style::default().fg(Color::White))
            .render(bottom, buf);

        if let Some(percent) = self.progress_counter {
            Gauge::default()
                .block(Block::bordered().title("progress-task"))
                .gauge_style((Color::White, Modifier::ITALIC))
                .percent(percent)
                .render(progress_area, buf);
        }
        if area.width > 120 {
            Text::from(vec![
                "Q: Quit | Tab: Switch state | ↑/↓: Select target | f: Focus target | ←/→: Display level | +/-: Filter level".into(),
                "Space: Toggle hidden targets | h: Hide target selector | PageUp/Down: Scroll | Esc: Cancel scroll".into(),
            ])
            .style(Color::Gray)
            .centered()
            .render(help_area, buf);
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
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
    
    fn handle_mouse_event(&self, mouse_event: &crossterm::event::MouseEvent) { 
        let mut states = self.states.borrow_mut();
        let Some(state) = states.get_mut(self.selected_tab) else {return;};

        match mouse_event.kind {
            crossterm::event::MouseEventKind::ScrollUp => state.transition(TuiWidgetEvent::PrevPageKey),
            crossterm::event::MouseEventKind::ScrollDown => state.transition(TuiWidgetEvent::NextPageKey),
            _ => {}
        }
    }
}

