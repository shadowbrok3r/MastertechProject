use std::sync::{Arc, Mutex};

use mtech_tui::styling::{APP_BACKGROUND, THEME};
use mtech_tui::widgets::HandleWidget;
use ratatui::{
    layout::{Constraint, Rect},
    prelude::Backend,
    style::Style,
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::terminal_mode::context::QcContext;

/// Minimal hardware view: per-core CPU usage from the shared telemetry
/// snapshot. Fleshed out to the full 8-table monitor in its own phase.
pub struct HardwareTab {
    ctx: Arc<Mutex<QcContext>>,
}

impl HardwareTab {
    pub fn new(ctx: Arc<Mutex<QcContext>>) -> Self {
        Self { ctx }
    }
}

impl<'a> HandleWidget<'a> for HardwareTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(false))
            .title("Hardware — CPU cores");

        let cores = self
            .ctx
            .lock()
            .ok()
            .and_then(|c| c.snapshot.as_ref().map(|s| s.cores.clone()));

        let Some(cores) = cores else {
            f.render_widget(
                Paragraph::new("Waiting for first telemetry tick…")
                    .block(block)
                    .style(Style::default().fg(THEME.text_muted).bg(APP_BACKGROUND)),
                area,
            );
            return;
        };

        let rows: Vec<Row> = cores
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let pct = c.usage_pct;
                let color = if pct < 70.0 {
                    THEME.success
                } else if pct < 90.0 {
                    THEME.warning
                } else {
                    THEME.error
                };
                Row::new(vec![
                    Cell::from(format!("Core {i}")),
                    Cell::from(format!("{pct:5.1}%")).style(Style::default().fg(color)),
                ])
            })
            .collect();

        let table = Table::new(rows, [Constraint::Length(10), Constraint::Length(10)])
            .block(block)
            .style(Style::default().fg(THEME.text).bg(APP_BACKGROUND));
        f.render_widget(table, area);
    }
}
