//! Consent banner for the terminal-mode client.
//!
//! Same interlock as the egui banner: painting it stamps the gate, and a gate
//! that has not been stamped in the last two seconds refuses new jobs. Terminal
//! mode needs its own because a client that boots straight into the TUI would
//! otherwise run jobs with nothing on screen saying so.

use displays::ui_tools::tui_theme::THEME;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::{gate, registry};

/// Rows the banner occupies when armed.
const BANNER_HEIGHT: u16 = 4;

/// Splits `area` into (banner, rest). Returns the whole area as `rest` when
/// nothing is armed.
pub fn split(area: Rect) -> (Option<Rect>, Rect) {
    if gate::banner_info().is_none() || area.height <= BANNER_HEIGHT {
        return (None, area);
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(BANNER_HEIGHT), Constraint::Fill(1)])
        .split(area);
    (Some(chunks[0]), chunks[1])
}

/// Paints the banner and stamps the gate.
pub fn render(f: &mut Frame, area: Rect) {
    let Some(info) = gate::banner_info() else {
        return;
    };
    gate::stamp_banner();

    let running = registry::running_count();
    let accent = THEME.warning;

    let mut headline = vec![
        Span::styled(
            "REMOTE CONTROL ACTIVE",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{} is connected to this computer", info.tech),
            Style::default().fg(THEME.text),
        ),
    ];
    if running > 0 {
        headline.push(Span::styled(
            format!(
                "  ({running} command{} running)",
                if running == 1 { "" } else { "s" }
            ),
            Style::default().fg(accent),
        ));
    }
    if super::screen_is_live() {
        headline.push(Span::styled(
            "  VIEWING YOUR SCREEN",
            Style::default().fg(THEME.error).add_modifier(Modifier::BOLD),
        ));
    }

    let body = vec![
        Line::from(headline),
        Line::from(vec![Span::styled(
            format!("Reason: {}", info.reason),
            Style::default().fg(THEME.text_muted),
        )]),
        Line::from(vec![
            Span::styled(
                "F12",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " end remote session now",
                Style::default().fg(THEME.text_muted),
            ),
            Span::styled(
                format!("     {} left", fmt_remaining(info.expires_in_secs)),
                Style::default().fg(THEME.text_muted),
            ),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .style(Style::default().bg(THEME.bg));

    f.render_widget(Paragraph::new(body).block(block), area);
}

/// Revoke the lease and terminate anything running under it.
pub fn end_session() {
    let killed = registry::cancel_all();
    gate::disarm();
    log::warn!("[remote_exec] session ended from the client TUI; {killed} job(s) terminated");
}

fn fmt_remaining(secs: u64) -> String {
    match secs {
        s if s >= 3600 => format!("{}h {}m", s / 3600, (s % 3600) / 60),
        s if s >= 60 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}
