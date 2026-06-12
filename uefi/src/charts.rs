//! Time-series ring buffers and a glyph-proof line chart.
//!
//! Mirrors the terminal-mode task-manager charts (sliding time window,
//! auto-scaled y bounds, corner labels) but paints the trace as
//! background-colored space cells instead of Canvas marker glyphs: firmware
//! fonts routinely lack Braille/half-block characters (vendor consoles and
//! OVMF draw blanks), while a colored cell background renders everywhere.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Widget};

use crate::styling::{APP_BACKGROUND, CATPPUCCIN, THEME};

/// Sliding-window sample ring; `t` is seconds from an arbitrary epoch.
pub struct History {
    window: f64,
    pts: VecDeque<(f64, f64)>,
}

impl History {
    pub fn new(window_secs: f64) -> Self {
        Self {
            window: window_secs,
            pts: VecDeque::new(),
        }
    }

    pub fn push(&mut self, t: f64, v: f64) {
        self.pts.push_back((t, v));
        let cutoff = t - self.window * 1.25;
        while self.pts.front().is_some_and(|p| p.0 < cutoff) {
            self.pts.pop_front();
        }
    }

    pub fn latest(&self) -> Option<f64> {
        self.pts.back().map(|p| p.1)
    }

    pub fn clear(&mut self) {
        self.pts.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    pub fn window_secs(&self) -> f64 {
        self.window
    }

    fn points_since(&self, t0: f64) -> Vec<(f64, f64)> {
        self.pts.iter().copied().filter(|p| p.0 >= t0).collect()
    }
}

/// Compact number for axis/footer labels: 1.23G, 45.6M, 789k, 12.3.
pub fn fmt_mag(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{:.2}G", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e3 {
        format!("{:.1}k", v / 1e3)
    } else if a >= 10.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Line chart over the history's window ending at `now`.
pub fn line_chart(
    title: String,
    unit: &'static str,
    color: Color,
    hist: &History,
    now: f64,
    y_floor: Option<f64>,
) -> CellChart {
    let lower = now - hist.window_secs();
    let pts = hist.points_since(lower);
    let latest = pts.last().map(|p| p.1).unwrap_or(0.0);
    let mut y_max = pts.iter().fold(0.0_f64, |m, p| m.max(p.1));
    if let Some(f) = y_floor {
        y_max = y_max.max(f);
    }
    let y_max = if y_max <= 0.0 { 1.0 } else { y_max * 1.15 };
    CellChart {
        title,
        unit,
        color,
        floor: y_floor,
        lower,
        now,
        y_max,
        latest,
        pts,
    }
}

/// Chart widget that fills cell backgrounds along the trace.
pub struct CellChart {
    title: String,
    unit: &'static str,
    color: Color,
    floor: Option<f64>,
    lower: f64,
    now: f64,
    y_max: f64,
    latest: f64,
    pts: Vec<(f64, f64)>,
}

impl CellChart {
    fn col(&self, t: f64, w: u16) -> i32 {
        let span = (self.now - self.lower).max(1e-9);
        (((t - self.lower) / span) * (w.saturating_sub(1)) as f64).round() as i32
    }

    fn row(&self, v: f64, h: u16) -> i32 {
        let frac = (v / self.y_max).clamp(0.0, 1.0);
        ((1.0 - frac) * (h.saturating_sub(1)) as f64).round() as i32
    }
}

impl Widget for CellChart {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(THEME.border_idle()).bg(APP_BACKGROUND))
            .style(Style::default().bg(APP_BACKGROUND))
            .title(format!(" {} ", self.title));
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width < 6 || inner.height < 2 {
            return;
        }
        let (w, h) = (inner.width, inner.height);

        // Pass-floor reference: dashed ASCII so it reads as a guide, not data.
        if let Some(f) = self.floor {
            let row = self.row(f, h);
            if (0..h as i32).contains(&row) {
                let y = inner.y + row as u16;
                for x in (0..w).step_by(2) {
                    buf[(inner.x + x, y)]
                        .set_char('-')
                        .set_fg(CATPPUCCIN.surface2);
                }
            }
        }

        // Trace: vertical-fill between consecutive samples per column.
        for seg in self.pts.windows(2) {
            let [a, b] = seg else { continue };
            let (c0, r0) = (self.col(a.0, w), self.row(a.1, h));
            let (c1, r1) = (self.col(b.0, w), self.row(b.1, h));
            let steps = (c1 - c0).abs().max((r1 - r0).abs()).max(1);
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let c = (c0 as f64 + (c1 - c0) as f64 * t).round() as i32;
                let r = (r0 as f64 + (r1 - r0) as f64 * t).round() as i32;
                if (0..w as i32).contains(&c) && (0..h as i32).contains(&r) {
                    buf[(inner.x + c as u16, inner.y + r as u16)]
                        .set_char(' ')
                        .set_bg(self.color);
                }
            }
        }

        // Corner labels over the trace (fg text on panel background).
        let label = |buf: &mut Buffer, x: u16, y: u16, s: &str, fg: Color| {
            buf.set_string(x, y, s, Style::default().fg(fg).bg(APP_BACKGROUND));
        };
        label(buf, inner.x, inner.y, &fmt_mag(self.y_max), CATPPUCCIN.overlay1);
        label(
            buf,
            inner.x,
            inner.y + h - 1,
            "0",
            CATPPUCCIN.overlay1,
        );
        if !self.pts.is_empty() {
            let cur = format!("{} {}", fmt_mag(self.latest), self.unit);
            let x = inner.x + w.saturating_sub(cur.len() as u16 + 1);
            label(buf, x, inner.y, &cur, CATPPUCCIN.text);
        }
    }
}
