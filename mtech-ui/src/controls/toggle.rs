//! Pill switch and checkbox, each with a trailing label.

use eframe::egui::{
    Color32, Rect, Response, Sense, TextStyle, Ui, Vec2, WidgetInfo, WidgetType, lerp, pos2, vec2,
};

use super::{State, focus_ring, kit, mix, paint_box, state_of, text_galley, whole};
use crate::icon_pack;

const TRACK_W: f32 = 30.0;
const TRACK_H: f32 = 17.0;
const KNOB_R: f32 = 6.5;
/// Gap between the track and the label.
const LABEL_GAP: f32 = 8.0;
/// Share of the text colour mixed into the off track on hover.
const HOVER_LIFT: f32 = 0.12;
/// Largest corner radius of a checkbox's box.
const BOX_RADIUS: u8 = 4;
/// Inset of the check mark inside its box.
const CHECK_INSET: f32 = 3.0;

/// A 30 × 17 switch in a [`super::Metrics::control_h`] row; clicking it or its label flips `on`.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (t, m) = kit(ui);
    let text =
        (!label.is_empty()).then(|| text_galley(ui, label, TextStyle::Body.resolve(ui.style())));
    let label_w = text.as_ref().map_or(0.0, |g| LABEL_GAP + g.size().x);
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(whole(TRACK_W + label_w), m.control_h), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let (state, focused) = state_of(ui, &response);
    let track = Rect::from_min_size(
        pos2(rect.left(), (rect.center().y - TRACK_H / 2.0).round()),
        vec2(TRACK_W, TRACK_H),
    );
    let how_on = ui.ctx().animate_bool(response.id, *on);
    let off = if state == State::Rest {
        t.field_border
    } else {
        mix(t.field_border, t.text, HOVER_LIFT)
    };
    let fill = mix(off, t.primary, how_on);
    ui.painter().rect_filled(track, TRACK_H / 2.0, fill);
    let x = lerp(
        (track.left() + TRACK_H / 2.0)..=(track.right() - TRACK_H / 2.0),
        how_on,
    );
    ui.painter()
        .circle_filled(pos2(x, track.center().y), KNOB_R, Color32::WHITE);
    if focused {
        focus_ring(ui, track, (TRACK_H / 2.0) as u8, &t);
    }
    if let Some(text) = text {
        let ink = if *on { t.text } else { t.text_dim };
        let pos = pos2(
            track.right() + LABEL_GAP,
            (rect.center().y - text.size().y / 2.0).round(),
        );
        ui.painter().galley(pos, text, ink);
    }
    response
}

/// A box in a [`super::Metrics::control_h`] row, checked while `on`; clicking the row flips it.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (t, m) = kit(ui);
    let side = m.icon_md;
    let text =
        (!label.is_empty()).then(|| text_galley(ui, label, TextStyle::Body.resolve(ui.style())));
    let label_w = text.as_ref().map_or(0.0, |g| LABEL_GAP + g.size().x);
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(whole(side + label_w), m.control_h), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let (state, focused) = state_of(ui, &response);
    let square = Rect::from_min_size(
        pos2(rect.left(), (rect.center().y - side / 2.0).round()),
        Vec2::splat(side),
    );
    let radius = t.radius_sm.min(BOX_RADIUS);
    let (fill, edge) = match (*on, state) {
        (true, State::Rest) => (t.primary, Color32::TRANSPARENT),
        (true, _) => (
            mix(t.primary, Color32::WHITE, HOVER_LIFT),
            Color32::TRANSPARENT,
        ),
        (false, State::Rest) => (t.field, t.field_border),
        (false, State::Hovered) => (t.hover, t.field_border),
        (false, State::Pressed) => (t.pressed, t.primary_border),
    };
    paint_box(ui, square, radius, fill, edge);
    if *on {
        let mark = side - 2.0 * CHECK_INSET;
        icon_pack::CHECK.paint(ui, square, mark, t.on_primary);
    }
    if focused {
        focus_ring(ui, square, radius, &t);
    }
    if let Some(text) = text {
        let pos = pos2(
            square.right() + LABEL_GAP,
            (rect.center().y - text.size().y / 2.0).round(),
        );
        ui.painter().galley(pos, text, t.text);
    }
    response
}
