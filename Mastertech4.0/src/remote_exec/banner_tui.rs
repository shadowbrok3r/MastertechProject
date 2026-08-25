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

/// Rows the single-line fallback occupies when the full banner will not fit.
const COMPACT_HEIGHT: u16 = 1;

/// Columns below which even the compact line cannot name who is connected.
const MIN_BANNER_WIDTH: u16 = 16;

/// Banner height for `area`, or the reason it cannot be painted there.
fn fit(area: Rect) -> Result<u16, String> {
    if area.width < MIN_BANNER_WIDTH || area.height < COMPACT_HEIGHT {
        return Err(format!(
            "the client's terminal is {}x{}, too small to paint the consent banner (needs at least {}x{}); resize the client's terminal window",
            area.width, area.height, MIN_BANNER_WIDTH, COMPACT_HEIGHT
        ));
    }
    Ok(if area.height > BANNER_HEIGHT {
        BANNER_HEIGHT
    } else {
        COMPACT_HEIGHT
    })
}

/// Splits `area` into (banner, rest). Returns the whole area as `rest` when
/// nothing is armed, or when the banner does not fit; the latter files a block
/// report so the refusal names the size instead of reading as a dead UI.
pub fn split(area: Rect) -> (Option<Rect>, Rect) {
    if gate::banner_info().is_none() {
        return (None, area);
    }
    let height = match fit(area) {
        Ok(height) => height,
        Err(why) => {
            gate::note_banner_blocked(why);
            return (None, area);
        }
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(height), Constraint::Fill(1)])
        .split(area);
    (Some(chunks[0]), chunks[1])
}

/// Paints the banner and stamps the gate. Stamps only once the area is known to
/// hold the banner, so a frame that cannot show it does not admit work.
pub fn render(f: &mut Frame, area: Rect) {
    let Some(info) = gate::banner_info() else {
        return;
    };
    let height = match fit(area) {
        Ok(height) => height,
        Err(why) => {
            gate::note_banner_blocked(why);
            return;
        }
    };
    gate::stamp_banner();

    if height == COMPACT_HEIGHT {
        render_compact(f, area, &info);
        return;
    }

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

/// One-line form for a terminal too short for the bordered banner.
fn render_compact(f: &mut Frame, area: Rect, info: &gate::BannerInfo) {
    let accent = THEME.warning;
    let mut spans = vec![
        Span::styled(
            "REMOTE CONTROL",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", info.tech), Style::default().fg(THEME.text)),
    ];
    let running = registry::running_count();
    if running > 0 {
        spans.push(Span::styled(
            format!("{running} running "),
            Style::default().fg(accent),
        ));
    }
    if super::screen_is_live() {
        spans.push(Span::styled(
            "SCREEN ",
            Style::default().fg(THEME.error).add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        "F12 ends",
        Style::default().fg(THEME.text_muted),
    ));
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(THEME.bg)),
        area,
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Serialises against the shared gate and arms it for the test.
    fn armed() -> std::sync::MutexGuard<'static, ()> {
        let held = gate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        gate::disarm();
        gate::arm("s".into(), "tech".into(), "diag".into(), "why".into(), 600);
        held
    }

    #[test]
    fn a_terminal_too_small_names_the_size_instead_of_failing_silently() {
        let _g = armed();
        let area = Rect::new(0, 0, 8, 1);
        assert_eq!(split(area), (None, area));
        let err = gate::check_admits_job().unwrap_err();
        assert!(err.contains("too small"), "{err}");
        assert!(err.contains("8x1"), "the refusal must name the size: {err}");
        gate::disarm();
    }

    #[test]
    fn a_zero_height_frame_does_not_stamp() {
        let _g = armed();
        let area = Rect::new(0, 0, 80, 0);
        assert_eq!(split(area), (None, area));
        assert!(gate::check_admits_job().is_err());
        gate::disarm();
    }

    #[test]
    fn a_full_size_frame_paints_and_stamps() {
        let _g = armed();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| {
                let (banner, rest) = split(f.area());
                let banner = banner.expect("the banner fits an 80x24 terminal");
                assert_eq!(banner.height, BANNER_HEIGHT);
                assert_eq!(rest.height, 20);
                render(f, banner);
            })
            .unwrap();
        assert!(gate::check_admits_job().is_ok());
        gate::disarm();
    }

    #[test]
    fn a_short_frame_paints_the_compact_line_and_stamps() {
        let _g = armed();
        let mut terminal = Terminal::new(TestBackend::new(60, 3)).unwrap();
        terminal
            .draw(|f| {
                let (banner, rest) = split(f.area());
                let banner = banner.expect("the compact banner fits a 60x3 terminal");
                assert_eq!(banner.height, COMPACT_HEIGHT);
                assert_eq!(rest.height, 2);
                render(f, banner);
            })
            .unwrap();
        assert!(
            gate::check_admits_job().is_ok(),
            "a painted compact banner must admit work"
        );
        gate::disarm();
    }

    #[test]
    fn nothing_is_reserved_while_the_gate_is_disarmed() {
        let _g = gate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        gate::disarm();
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(split(area), (None, area));
    }
}
