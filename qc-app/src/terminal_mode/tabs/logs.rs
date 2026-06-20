use std::cell::{Cell, RefCell};

use mtech_tui::styling::{APP_BACKGROUND, THEME};
use mtech_tui::widgets::{click_zones::ClickZones, HandleWidget, SHORTCUT_SET};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind},
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Backend,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Minimum log level shown; lower-severity lines are filtered out.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MinLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl MinLevel {
    fn as_str(self) -> &'static str {
        match self {
            MinLevel::Trace => "TRACE",
            MinLevel::Debug => "DEBUG",
            MinLevel::Info => "INFO",
            MinLevel::Warn => "WARN",
            MinLevel::Error => "ERROR",
        }
    }

    fn next(self) -> Self {
        match self {
            MinLevel::Trace => MinLevel::Debug,
            MinLevel::Debug => MinLevel::Info,
            MinLevel::Info => MinLevel::Warn,
            MinLevel::Warn => MinLevel::Error,
            MinLevel::Error => MinLevel::Trace,
        }
    }
}

/// Severity of a rendered log line, detected from its level token.
fn line_level(line: &str) -> MinLevel {
    if line.contains("ERROR") {
        MinLevel::Error
    } else if line.contains("WARN") {
        MinLevel::Warn
    } else if line.contains("INFO") {
        MinLevel::Info
    } else if line.contains("TRACE") {
        MinLevel::Trace
    } else {
        MinLevel::Debug
    }
}

/// Read-only log viewer fed by the global `egui_logger` ring buffer (populated
/// in terminal mode too, since the logger is installed regardless of frontend).
/// Tails the buffer; Up/Down/PageUp/PageDown or the mouse wheel scroll history.
pub struct LogsTab {
    scroll_back: Cell<usize>,
    min_level: Cell<MinLevel>,
    status: RefCell<Option<String>>,
    pending: RefCell<Option<String>>,
    zones: ClickZones,
}

impl Default for LogsTab {
    fn default() -> Self {
        Self {
            scroll_back: Cell::new(0),
            min_level: Cell::new(MinLevel::Debug),
            status: RefCell::new(None),
            pending: RefCell::new(None),
            zones: ClickZones::default(),
        }
    }
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

impl LogsTab {
    fn cycle_level(&self) {
        self.min_level.set(self.min_level.get().next());
        self.scroll_back.set(0);
    }

    fn copy_all(&self) {
        let text = mtech_ui::egui_logger::get_logs_as_string(None, true, false);
        let msg = match mtech_tui::data::log_capture::copy_text(text) {
            Ok(()) => "copied all logs to clipboard".to_string(),
            Err(e) => format!("copy failed: {e}"),
        };
        *self.status.borrow_mut() = Some(msg);
    }

    fn dump_to_file(&self) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let name = format!("qc-app-log-{secs}.txt");
        let path = std::env::current_dir().unwrap_or_default().join(&name);
        let body = mtech_ui::egui_logger::get_logs_as_string(None, true, false);
        let msg = match std::fs::write(&path, body) {
            Ok(()) => format!("wrote {}", path.display()),
            Err(e) => format!("write failed: {e}"),
        };
        *self.status.borrow_mut() = Some(msg);
    }

    fn apply_pending(&self) {
        let action = self.pending.borrow_mut().take();
        match action.as_deref() {
            Some("level") => self.cycle_level(),
            Some("copy") => self.copy_all(),
            Some("dump") => self.dump_to_file(),
            _ => {}
        }
    }
}

impl<'a> HandleWidget<'a> for LogsTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.zones.begin();
        if let Some(id) = self.zones.take() {
            *self.pending.borrow_mut() = Some(id);
        }
        self.apply_pending();

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(1)])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Logs");

        let min = self.min_level.get();
        let text = mtech_ui::egui_logger::get_logs_as_string(None, true, false);
        let lines: Vec<&str> = text.lines().filter(|l| line_level(l) >= min).collect();
        let inner_h = rows[0].height.saturating_sub(2) as usize;
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
            rows[0],
        );

        let status = self.status.borrow().clone().unwrap_or_default();
        let prefix = format!("level: {}  ", min.as_str());
        let level_tok = "[l]evel";
        let copy_tok = "[c]opy";
        let dump_tok = "[d]ump";
        let footer = format!(
            "{prefix}{level_tok}  {copy_tok}  {dump_tok}  ↑↓ scroll   {status}"
        );
        let y = rows[1].y;
        let mut x = rows[1].x + prefix.len() as u16;
        for (tok, id) in [(level_tok, "level"), (copy_tok, "copy"), (dump_tok, "dump")] {
            self.zones.add(Rect { x, y, width: tok.len() as u16, height: 1 }, id.to_string());
            x += tok.len() as u16 + 2;
        }
        f.render_widget(
            Paragraph::new(footer).style(Style::default().fg(THEME.text_muted)),
            rows[1],
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
            KeyCode::Char('l') => self.cycle_level(),
            KeyCode::Char('c') => self.copy_all(),
            KeyCode::Char('d') => self.dump_to_file(),
            _ => return false,
        }
        true
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let back = self.scroll_back.get();
        match mouse_event.kind {
            MouseEventKind::ScrollUp => self.scroll_back.set(back.saturating_add(3)),
            MouseEventKind::ScrollDown => self.scroll_back.set(back.saturating_sub(3)),
            _ => self.zones.on_mouse(mouse_event),
        }
    }
}
