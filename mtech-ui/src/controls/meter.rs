//! Progress bars, sliders and segmented choices.

use std::ops::RangeInclusive;

use eframe::egui::{
    Color32, CursorIcon, Key, Rect, Response, Role, Sense, Stroke, StrokeKind, TextStyle, Ui, Vec2,
    WidgetInfo, emath::Numeric, lerp, pos2, remap_clamp, vec2,
};

use super::ink_on;
use super::{
    Icon, State, ValueField, focus_ring, kit, medium_font, mix, paint_box, state_of, text_galley,
    whole,
};

/// Height of a thin progress track.
const THIN_H: f32 = 4.0;
/// Inset of a progress bar's box from the top and bottom of its row.
const BAR_INSET_Y: f32 = 3.0;
/// Progress-bar text size.
const BAR_TEXT_PT: f32 = 11.0;
/// Share of the bar a moving indeterminate segment covers.
const SWEEP_W: f32 = 0.3;
/// Seconds an indeterminate segment takes to cross the bar.
const SWEEP_SECS: f32 = 1.4;
/// Slider track height.
const TRACK_H: f32 = 4.0;
/// Slider knob radius.
const KNOB_R: f32 = 7.0;
/// Width of a slider's value field.
const SLIDER_VALUE_W: f32 = 64.0;
/// Steps across a slider's range the arrow keys move.
const KEY_STEPS: f64 = 100.0;
/// Horizontal padding inside a segment.
const SEGMENT_PAD_X: f32 = 10.0;
/// Gap between a segment's icon and label.
const SEGMENT_ICON_GAP: f32 = 6.0;
/// Inset of the selected segment's wash from the outer box.
const SEGMENT_INSET: f32 = 2.0;
/// Share of the tone in a selected toned segment's fill and border.
const SEGMENT_TONE_FILL: f32 = 0.22;
const SEGMENT_TONE_EDGE: f32 = 0.6;

/// A horizontal progress bar, determinate or sweeping.
#[must_use = "call `show` to draw the bar"]
pub struct ProgressBar<'a> {
    fraction: Option<f32>,
    width: Option<f32>,
    text: Option<&'a str>,
    percentage: bool,
    tone: Option<Color32>,
    thin: bool,
}

impl<'a> ProgressBar<'a> {
    /// `fraction` done, clamped to 0..=1.
    pub fn new(fraction: f32) -> Self {
        Self {
            fraction: Some(fraction.clamp(0.0, 1.0)),
            width: None,
            text: None,
            percentage: false,
            tone: None,
            thin: false,
        }
    }

    /// A segment sweeping across the bar for work of unknown length.
    pub fn indeterminate() -> Self {
        Self {
            fraction: None,
            ..Self::new(0.0)
        }
    }

    /// Outer width; the available width by default.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Text centred in the bar.
    pub fn text(mut self, text: &'a str) -> Self {
        self.text = Some(text);
        self
    }

    /// Shows the percentage done when no text is set.
    pub fn percentage(mut self, percentage: bool) -> Self {
        self.percentage = percentage;
        self
    }

    /// Fill colour in place of the primary role.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }

    /// A 4 px track with no text, allocated at its own height.
    pub fn thin(mut self, thin: bool) -> Self {
        self.thin = thin;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let (t, m) = kit(ui);
        let width = self.width.unwrap_or_else(|| ui.available_width()).max(1.0);
        let height = if self.thin { THIN_H } else { m.control_h };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let tone = self.tone.unwrap_or(t.primary);
        let percent = self
            .percentage
            .then(|| self.fraction.map(|f| format!("{:.0}%", f * 100.0)))
            .flatten();
        let text = self.text.or(percent.as_deref());
        response.widget_info(|| {
            WidgetInfo::labeled(
                Role::ProgressIndicator,
                ui.is_enabled(),
                text.unwrap_or("Progress"),
            )
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let track = if self.thin {
            rect
        } else {
            rect.shrink2(vec2(0.0, BAR_INSET_Y))
        };
        let radius = if self.thin {
            (THIN_H / 2.0) as u8
        } else {
            t.radius_sm
        };
        let edge = if self.thin {
            Color32::TRANSPARENT
        } else {
            t.field_border
        };
        let trough = if self.thin { t.field_border } else { t.field };
        paint_box(ui, track, radius, trough, edge);

        let done = match self.fraction {
            Some(f) => Rect::from_min_size(track.min, vec2(track.width() * f, track.height())),
            None => {
                ui.ctx().request_repaint();
                let time = ui.input(|i| i.time) as f32;
                let phase = (time / SWEEP_SECS).fract();
                let w = track.width() * SWEEP_W;
                let x = lerp((track.left() - w)..=track.right(), phase);
                Rect::from_min_max(
                    pos2(x.max(track.left()), track.top()),
                    pos2((x + w).min(track.right()), track.bottom()),
                )
            }
        };
        if done.width() > 0.0 {
            ui.painter().rect_filled(done, radius, tone);
        }
        if let (Some(text), false) = (text, self.thin) {
            let galley = text_galley(ui, text, medium_font_sized_small(ui));
            let pos = (track.center() - galley.size() / 2.0).round();
            let clip = ui.clip_rect();
            let dark = ui.painter().with_clip_rect(clip.intersect(done));
            dark.galley(pos, galley.clone(), ink_on(tone));
            let rest = Rect::from_min_max(pos2(done.right(), track.top()), track.max);
            let light = ui.painter().with_clip_rect(clip.intersect(rest));
            light.galley(pos, galley.clone(), t.text);
            if done.left() > track.left() {
                let head = Rect::from_min_max(track.min, pos2(done.left(), track.bottom()));
                let lead = ui.painter().with_clip_rect(clip.intersect(head));
                lead.galley(pos, galley, t.text);
            }
        }
        response
    }
}

/// The progress-bar text face.
fn medium_font_sized_small(ui: &Ui) -> eframe::egui::FontId {
    super::medium_font_sized(ui, BAR_TEXT_PT)
}

/// A full-width progress bar with `text` centred in it.
pub fn progress_bar(ui: &mut Ui, fraction: f32, text: &str) -> Response {
    let bar = ProgressBar::new(fraction);
    if text.is_empty() {
        bar.show(ui)
    } else {
        bar.text(text).show(ui)
    }
}

/// A thin `width`-wide meter filled `fraction` in `tone`.
pub fn meter(ui: &mut Ui, fraction: f32, width: f32, tone: Color32) -> Response {
    ProgressBar::new(fraction)
        .width(width)
        .tone(tone)
        .thin(true)
        .show(ui)
}

/// A horizontal slider over a numeric range, optionally followed by a value field.
#[must_use = "call `show` to draw the slider"]
pub struct Slider<'a, N: Numeric> {
    value: &'a mut N,
    range: RangeInclusive<N>,
    width: Option<f32>,
    step: Option<f64>,
    value_suffix: Option<&'a str>,
}

impl<'a, N: Numeric> Slider<'a, N> {
    pub fn new(value: &'a mut N, range: RangeInclusive<N>) -> Self {
        Self {
            value,
            range,
            width: None,
            step: None,
            value_suffix: None,
        }
    }

    /// Track width; [`super::Metrics::slider_w`] by default.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Rounds picked values to multiples of `step`.
    pub fn step(mut self, step: f64) -> Self {
        self.step = (step > 0.0).then_some(step);
        self
    }

    /// Adds a value field with this unit suffix after the track.
    pub fn value(mut self, suffix: &'a str) -> Self {
        self.value_suffix = Some(suffix);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let (t, m) = kit(ui);
        let width = self.width.unwrap_or(m.slider_w).max(2.0 * KNOB_R + 2.0);
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(width, m.control_h), Sense::click_and_drag());
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
        let x_range = (rect.left() + KNOB_R)..=(rect.right() - KNOB_R);
        let snap = |v: f64| {
            let v = match self.step {
                Some(step) => lo + ((v - lo) / step).round() * step,
                None => v,
            };
            let v = v.clamp(lo.min(hi), lo.max(hi));
            if N::INTEGRAL { v.round() } else { v }
        };

        let current = self.value.to_f64();
        let mut next = current;
        if let Some(pos) = response.interact_pointer_pos()
            && (response.is_pointer_button_down_on() || response.clicked())
        {
            let share = remap_clamp(pos.x, x_range.clone(), 0.0..=1.0) as f64;
            next = snap(lo + (hi - lo) * share);
        }
        if response.has_focus() {
            let key_step = self.step.unwrap_or_else(|| {
                if N::INTEGRAL {
                    1.0
                } else {
                    (hi - lo) / KEY_STEPS
                }
            });
            let dir = ui.input(|i| {
                (i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp)) as i32
                    - (i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown)) as i32
            });
            if dir != 0 {
                next = snap(current + dir as f64 * key_step);
            }
        }
        if next != current {
            *self.value = N::from_f64(next);
            response.mark_changed();
        }
        let value = self.value.to_f64();
        response
            .widget_info(|| WidgetInfo::labeled(Role::Slider, ui.is_enabled(), format!("{value}")));
        let response = response.on_hover_cursor(CursorIcon::PointingHand);

        if ui.is_rect_visible(rect) {
            let (state, focused) = state_of(ui, &response);
            let share = if hi == lo {
                0.0
            } else {
                ((self.value.to_f64() - lo) / (hi - lo)).clamp(0.0, 1.0) as f32
            };
            let knob_x = lerp(x_range.clone(), share);
            let cy = rect.center().y;
            let track = Rect::from_min_max(
                pos2(*x_range.start(), cy - TRACK_H / 2.0),
                pos2(*x_range.end(), cy + TRACK_H / 2.0),
            );
            let radius = (TRACK_H / 2.0) as u8;
            ui.painter().rect_filled(track, radius, t.field_border);
            let filled = Rect::from_min_max(track.min, pos2(knob_x, track.bottom()));
            let fill = if ui.is_enabled() {
                t.primary
            } else {
                t.text_faint
            };
            ui.painter().rect_filled(filled, radius, fill);
            let knob = pos2(knob_x, cy);
            let ring = match state {
                State::Rest => Stroke::new(1.0, t.field_border),
                State::Hovered | State::Pressed => Stroke::new(2.0, t.primary_border),
            };
            ui.painter().circle(knob, KNOB_R, Color32::WHITE, ring);
            if focused {
                let ring_rect = Rect::from_center_size(knob, Vec2::splat(2.0 * KNOB_R));
                focus_ring(ui, ring_rect, KNOB_R as u8, &t);
            }
        }

        match self.value_suffix {
            Some(suffix) => {
                let field = ValueField::new(self.value, self.range)
                    .suffix(suffix)
                    .width(SLIDER_VALUE_W)
                    .show(ui);
                if field.changed() {
                    let mut response = response;
                    response.mark_changed();
                    return response;
                }
                response
            }
            None => response,
        }
    }
}

/// A slider `width` wide over `range`.
pub fn slider<N: Numeric>(
    ui: &mut Ui,
    value: &mut N,
    range: RangeInclusive<N>,
    width: f32,
) -> Response {
    Slider::new(value, range).width(width).show(ui)
}

/// One choice of a [`segmented`] control.
#[derive(Clone, Debug)]
pub struct Segment<'a, V> {
    value: V,
    label: &'a str,
    icon: Option<Icon>,
    tone: Option<Color32>,
}

impl<'a, V> Segment<'a, V> {
    pub fn new(value: V, label: &'a str) -> Self {
        Self {
            value,
            label,
            icon: None,
            tone: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Colour of this segment's wash, icon and label while selected.
    pub fn tone(mut self, tone: Color32) -> Self {
        self.tone = Some(tone);
        self
    }
}

/// Joined choices; a click picks one into `value` and sets `changed()`.
pub fn segmented<V: PartialEq + Clone>(
    ui: &mut Ui,
    value: &mut V,
    segments: &[Segment<'_, V>],
) -> Response {
    let (t, m) = kit(ui);
    let font = medium_font(ui, TextStyle::Button);
    let galleys: Vec<_> = segments
        .iter()
        .map(|s| text_galley(ui, s.label, font.clone()))
        .collect();
    let widths: Vec<f32> = segments
        .iter()
        .zip(&galleys)
        .map(|(s, g)| {
            let icon_w = s.icon.map_or(0.0, |_| m.icon_sm + SEGMENT_ICON_GAP);
            whole(g.size().x + icon_w + 2.0 * SEGMENT_PAD_X)
        })
        .collect();
    let total = widths.iter().sum::<f32>().max(m.control_h);
    let (rect, mut response) = ui.allocate_exact_size(vec2(total, m.control_h), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_box(ui, rect, t.radius_sm, t.field, t.field_border);
    }
    let selected = segments.iter().position(|s| s.value == *value);
    let mut picked = None;
    let mut x = rect.left();
    for (i, ((segment, galley), w)) in segments.iter().zip(galleys).zip(&widths).enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, rect.height()));
        x += w;
        let hit = ui.interact(cell, response.id.with(i), Sense::click());
        let on = selected == Some(i);
        hit.widget_info(|| {
            WidgetInfo::selected(Role::RadioButton, ui.is_enabled(), on, segment.label)
        });
        if hit.clicked() {
            picked = Some(i);
        }
        if !ui.is_rect_visible(cell) {
            continue;
        }
        let (state, focused) = state_of(ui, &hit);
        let wash = cell.shrink(SEGMENT_INSET);
        let inner_r = t.radius_sm.saturating_sub(SEGMENT_INSET as u8);
        let (ink, icon_ink) = if on {
            let (fill, edge, ink) = match segment.tone {
                Some(tone) => (
                    mix(t.field, tone, SEGMENT_TONE_FILL),
                    mix(t.field, tone, SEGMENT_TONE_EDGE),
                    tone,
                ),
                None => super::selected_colors(&t),
            };
            ui.painter().rect(
                wash,
                inner_r,
                fill,
                Stroke::new(1.0, edge),
                StrokeKind::Inside,
            );
            (ink, segment.tone.unwrap_or(t.primary))
        } else {
            let (fill, ink) = match state {
                State::Rest => (Color32::TRANSPARENT, t.text_dim),
                State::Hovered => (t.hover, t.text),
                State::Pressed => (t.pressed, t.text),
            };
            ui.painter().rect_filled(wash, inner_r, fill);
            (ink, ink)
        };
        let icon_w = segment.icon.map_or(0.0, |_| m.icon_sm + SEGMENT_ICON_GAP);
        let left = (cell.center().x - (galley.size().x + icon_w) / 2.0).round();
        if let Some(icon) = segment.icon {
            let slot = Rect::from_min_size(
                pos2(left, cell.center().y - m.icon_sm / 2.0),
                Vec2::splat(m.icon_sm),
            );
            icon.paint(ui, slot, m.icon_sm, icon_ink);
        }
        let y = (cell.center().y - galley.size().y / 2.0).round();
        ui.painter().galley(pos2(left + icon_w, y), galley, ink);
        if focused {
            focus_ring(ui, cell, t.radius_sm, &t);
        }
        let next_on = selected == Some(i + 1);
        if i + 1 < segments.len() && !on && !next_on {
            let sx = ui.painter().round_to_pixel_center(cell.right());
            let span = (cell.top() + 2.0 * SEGMENT_INSET)..=(cell.bottom() - 2.0 * SEGMENT_INSET);
            ui.painter()
                .vline(sx, span, Stroke::new(1.0, t.field_border));
        }
    }
    if let Some(i) = picked
        && Some(i) != selected
    {
        *value = segments[i].value.clone();
        response.mark_changed();
    }
    response
}
