use std::cell::Cell;

use mtech_tui::styling::{APP_BACKGROUND, THEME};
use mtech_tui::widgets::{HandleWidget, SHORTCUT_SET};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind},
    layout::Rect,
    prelude::Backend,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Read-only log viewer fed by the global `egui_logger` ring buffer (populated
/// in terminal mode too, since the logger is installed regardless of frontend).
/// Tails the buffer; Up/Down/PageUp/PageDown or the mouse wheel scroll history.
#[derive(Default)]
pub struct LogsTab {
    scroll_back: Cell<usize>,
}

fn colorize(line: &str) -> Line<'static> {
    let color = if line.contains("ERROR") {
        THEME.error
    } else if line.contains("WARN") {
        THEME.warning
    } else if line.contains("DEBUG") || line.contains("TRACE") {
        THEME.text_muted
    } else {
        THEME.text
    };
    Line::from(line.to_string()).style(Style::default().fg(color))
}

impl<'a> HandleWidget<'a> for LogsTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Logs");

        let text = mtech_ui::egui_logger::get_logs_as_string(Some(2000), true, false);
        let lines: Vec<&str> = text.lines().collect();
        let inner_h = area.height.saturating_sub(2) as usize;
        let total = lines.len();
        let max_back = total.saturating_sub(inner_h);
        let back = self.scroll_back.get().min(max_back);
        self.scroll_back.set(back);
        let end = total.saturating_sub(back);
        let start = end.saturating_sub(inner_h);
        let view: Vec<Line> = lines[start..end].iter().map(|l| colorize(l)).collect();

        f.render_widget(
            Paragraph::new(view)
                .block(block)
                .style(Style::default().bg(APP_BACKGROUND)),
            area,
        );
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        let back = self.scroll_back.get();
        match key_event.code {
            KeyCode::Up => self.scroll_back.set(back.saturating_add(1)),
            KeyCode::Down => self.scroll_back.set(back.saturating_sub(1)),
            KeyCode::PageUp => self.scroll_back.set(back.saturating_add(10)),
            KeyCode::PageDown => self.scroll_back.set(back.saturating_sub(10)),
            KeyCode::End => self.scroll_back.set(0),
            _ => return false,
        }
        true
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let back = self.scroll_back.get();
        match mouse_event.kind {
            MouseEventKind::ScrollUp => self.scroll_back.set(back.saturating_add(3)),
            MouseEventKind::ScrollDown => self.scroll_back.set(back.saturating_sub(3)),
            _ => {}
        }
    }
}
