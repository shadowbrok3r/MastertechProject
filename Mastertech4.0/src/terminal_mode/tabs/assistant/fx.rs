//! tachyonfx animations drawn over the assistant tab after each redraw.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Color;
use tachyonfx::{
    CellFilter, Duration, Effect, EffectManager, Interpolatable, Interpolation, Motion, RefRect, fx,
};

use crate::terminal_mode::styling::THEME;

/// Longest step fed to the effects in one redraw.
const MAX_STEP_MS: u128 = 100;
/// Cells per second the status shimmer travels.
const SHIMMER_SPEED: f32 = 28.0;
/// Half-width in cells of the shimmer band.
const SHIMMER_BAND: f32 = 5.0;
/// Cells per second the light runs around a working border.
const BORDER_SPEED: f32 = 34.0;
/// Cells of fading tail behind the light on a working border.
const BORDER_TAIL: f32 = 14.0;

/// Slots for the tab's effects; adding to a slot replaces what runs there.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum FxKey {
    #[default]
    Border,
    Status,
    Transcript,
    Row(String),
    Picker,
    PickerDim,
    Plan,
    PlanStep(usize),
    Approval,
    ApprovalPulse,
    Note,
    Unseen,
}

#[derive(Default)]
pub struct Fx {
    manager: EffectManager<FxKey>,
    last: Option<Instant>,
}

impl Fx {
    pub fn add(&mut self, key: FxKey, effect: Effect) {
        self.manager.add_unique_effect(key, effect);
    }

    pub fn cancel(&mut self, key: FxKey) {
        self.manager.cancel_unique_effect(key);
    }

    /// Advances every effect by the time since the last call and draws it into `buf`.
    pub fn process(&mut self, buf: &mut Buffer, area: Rect) {
        let now = Instant::now();
        let step = self
            .last
            .map_or(0, |t| now.duration_since(t).as_millis().min(MAX_STEP_MS))
            as u32;
        self.last = Some(now);
        self.manager
            .process_effects(Duration::from_millis(step), buf, area);
    }
}

/// `color` moved `t` of the way to `to`; colours other than RGB are left alone.
fn toward(color: Color, to: Color, t: f32) -> Color {
    match color {
        Color::Rgb(..) => color.lerp(&to, t.clamp(0.0, 1.0)),
        other => other,
    }
}

/// A bright band sweeping over the text in `area` again and again.
pub fn shimmer(area: RefRect, highlight: Color) -> Effect {
    let sweep = fx::effect_fn_buf(0u32, u32::MAX, move |elapsed, ctx, buf| {
        *elapsed = elapsed.wrapping_add(ctx.last_tick.as_millis());
        let a = ctx.area;
        if a.width == 0 {
            return;
        }
        let period = f32::from(a.width) + SHIMMER_BAND * 2.0;
        let head = (*elapsed as f32 / 1000.0 * SHIMMER_SPEED) % period - SHIMMER_BAND;
        for x in a.left()..a.right() {
            let t = 1.0 - ((f32::from(x - a.x) - head).abs() / SHIMMER_BAND).min(1.0);
            if t <= 0.0 {
                continue;
            }
            for y in a.top()..a.bottom() {
                if let Some(cell) = buf.cell_mut((x, y))
                    && cell.symbol() != " "
                {
                    let fg = toward(cell.fg, highlight, t * 0.9);
                    cell.set_fg(fg);
                }
            }
        }
    });
    fx::dynamic_area(area, sweep)
}

/// Cells of `a`'s outline clockwise from its top-left corner.
fn perimeter(a: Rect) -> impl Iterator<Item = (u16, u16)> {
    let (l, t, r, b) = (a.left(), a.top(), a.right() - 1, a.bottom() - 1);
    let top = (l..=r).map(move |x| (x, t));
    let right = (t + 1..b).map(move |y| (r, y));
    let bottom = (l..=r).rev().map(move |x| (x, b));
    let left = (t + 1..b).rev().map(move |y| (l, y));
    top.chain(right).chain(bottom).chain(left)
}

/// A light with a fading tail running around the outline of `area`.
pub fn running_border(area: RefRect, glow: Color) -> Effect {
    let run = fx::effect_fn_buf(0u32, u32::MAX, move |elapsed, ctx, buf| {
        *elapsed = elapsed.wrapping_add(ctx.last_tick.as_millis());
        let a = ctx.area;
        if a.width < 2 || a.height < 2 {
            return;
        }
        let len = 2.0 * (f32::from(a.width) + f32::from(a.height)) - 4.0;
        let head = (*elapsed as f32 / 1000.0 * BORDER_SPEED) % len;
        for (i, pos) in perimeter(a).enumerate() {
            let behind = (head - i as f32).rem_euclid(len);
            let t = 1.0 - behind / BORDER_TAIL;
            if t > 0.0
                && let Some(cell) = buf.cell_mut(pos)
            {
                let fg = toward(cell.fg, glow, t);
                cell.set_fg(fg);
            }
        }
    });
    fx::dynamic_area(area, run)
}

/// The text in `area` fading in from the background.
pub fn fade_in(area: RefRect, ms: u32) -> Effect {
    fx::dynamic_area(
        area,
        fx::fade_from_fg(THEME.bg, (ms, Interpolation::QuadOut)),
    )
}

/// `area` revealed top to bottom.
pub fn drop_in(area: Rect) -> Effect {
    fx::sweep_in(
        Motion::UpToDown,
        area.height.max(1),
        0,
        THEME.bg,
        (220, Interpolation::QuadOut),
    )
    .with_area(area)
}

/// `area` revealed left to right.
pub fn wipe_in(area: RefRect, ms: u32) -> Effect {
    fx::dynamic_area(
        area,
        fx::sweep_in(
            Motion::LeftToRight,
            24,
            0,
            THEME.bg,
            (ms, Interpolation::QuadOut),
        ),
    )
}

/// Everything outside `keep` dimmed until cancelled.
pub fn dim_except(keep: Rect) -> Effect {
    fx::never_complete(fx::darken(
        Some(0.55),
        Some(0.4),
        (180, Interpolation::QuadOut),
    ))
    .with_filter(CellFilter::Not(Box::new(CellFilter::Area(keep))))
}

/// The outline of `area` brightening and dimming until cancelled.
pub fn pulse_outline(area: RefRect) -> Effect {
    let pulse = fx::ping_pong(fx::hsl_shift_fg(
        [0.0, 0.0, 24.0],
        (650, Interpolation::SineInOut),
    ));
    fx::dynamic_area(
        area,
        fx::repeating(pulse).with_filter(CellFilter::Outer(Margin::new(1, 1))),
    )
}

/// The text in `area` brightening and dimming until cancelled.
pub fn pulse_text(area: RefRect) -> Effect {
    let pulse = fx::ping_pong(fx::hsl_shift_fg(
        [0.0, 0.0, 28.0],
        (550, Interpolation::SineInOut),
    ));
    fx::dynamic_area(area, fx::repeating(pulse))
}

/// `area` lit in `fg` on `bg`, easing back to its own colours.
pub fn flash(area: RefRect, fg: Color, bg: Color, ms: u32) -> Effect {
    fx::dynamic_area(area, fx::fade_from(fg, bg, (ms, Interpolation::QuadOut)))
}

/// The text in `area` starting in `color` and easing back to its own.
pub fn tint_in(area: RefRect, color: Color, ms: u32) -> Effect {
    fx::dynamic_area(area, fx::fade_from_fg(color, (ms, Interpolation::QuadOut)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn filled(area: Rect, fg: Color) -> Buffer {
        let mut buf = Buffer::empty(area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)]
                    .set_symbol("x")
                    .set_style(Style::default().fg(fg).bg(Color::Rgb(0, 0, 0)));
            }
        }
        buf
    }

    #[test]
    fn the_outline_runs_once_round_without_repeating_a_cell() {
        let a = Rect::new(2, 3, 5, 4);
        let cells: Vec<_> = perimeter(a).collect();
        assert_eq!(cells.len(), 2 * (5 + 4) - 4);
        let unique: std::collections::HashSet<_> = cells.iter().collect();
        assert_eq!(unique.len(), cells.len());
        assert_eq!(cells.first(), Some(&(2, 3)));
        assert!(
            cells
                .iter()
                .all(|&(x, y)| x == 2 || x == 6 || y == 3 || y == 6)
        );
    }

    #[test]
    fn the_shimmer_lights_only_cells_near_its_band() {
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = filled(area, Color::Rgb(100, 100, 100));
        let rect = RefRect::new(area);
        let mut fx = Fx::default();
        fx.add(FxKey::Status, shimmer(rect, Color::Rgb(255, 255, 255)));
        fx.process(&mut buf, area);
        fx.last = Some(Instant::now() - std::time::Duration::from_millis(60));
        fx.process(&mut buf, area);
        let lit = (0..40)
            .filter(|&x| buf[(x, 0)].fg != Color::Rgb(100, 100, 100))
            .count();
        assert!(
            lit > 0 && lit <= (SHIMMER_BAND as usize) * 2 + 1,
            "{lit} cells lit"
        );
    }

    #[test]
    fn a_dim_leaves_the_kept_rect_alone() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = filled(area, Color::Rgb(200, 200, 200));
        let keep = Rect::new(2, 1, 4, 2);
        let mut fx = Fx::default();
        fx.add(FxKey::PickerDim, dim_except(keep));
        fx.process(&mut buf, area);
        fx.last = Some(Instant::now() - std::time::Duration::from_millis(100));
        fx.process(&mut buf, area);
        fx.last = Some(Instant::now() - std::time::Duration::from_millis(100));
        fx.process(&mut buf, area);
        assert_eq!(buf[(3, 2)].fg, Color::Rgb(200, 200, 200));
        assert_ne!(buf[(0, 0)].fg, Color::Rgb(200, 200, 200));
    }
}
