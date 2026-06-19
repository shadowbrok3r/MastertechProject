use mtech_tui::styling::{APP_BACKGROUND, THEME};
use mtech_tui::widgets::HandleWidget;
use ratatui::{
    layout::Rect,
    prelude::Backend,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

/// Read-only mirror of the egui Settings tab: the compile-time fleet
/// orchestrator config + current reporting status.
#[derive(Default)]
pub struct SettingsTab;

impl<'a> HandleWidget<'a> for SettingsTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let url = database::orchestrator_url();
        let active_label = if cfg!(debug_assertions) {
            "ORCHESTRATOR_URL_DEV (debug build)"
        } else {
            "ORCHESTRATOR_URL (release build)"
        };
        let key_style = Style::default().fg(THEME.text_muted);

        let mut lines: Vec<Line> = vec![
            Line::from("Fleet orchestrator").style(THEME.title()),
            Line::from(
                "Picked from .env at compile time. Rebuild after editing \
                 ORCHESTRATOR_URL / ORCHESTRATOR_URL_DEV to change.",
            )
            .style(key_style),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Active key:    ", key_style),
                Span::raw(active_label),
            ]),
        ];

        if url.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Resolved URL:  ", key_style),
                Span::styled("(empty — reporting disabled)", Style::default().fg(THEME.warning)),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from("Reporting disabled (env var unset)").style(key_style));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Resolved URL:  ", key_style),
                Span::raw(url.to_string()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(format!("Reporting target → {url}")).style(key_style));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(false))
            .title("Settings");
        f.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(THEME.text).bg(APP_BACKGROUND)),
            area,
        );
    }
}
