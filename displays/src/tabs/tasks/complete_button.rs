//! Two-click complete control. The button's border doubles as a progress ring
//! traced clockwise from top-centre:
//!
//! - one click arms it and the ring fills over [`ARM_WINDOW`]; letting it fill
//!   disarms without writing anything,
//! - a second click stages the completion and the ring restarts, now tracking
//!   [`super::pending::COMMIT_DELAY`],
//! - when that fills the write has landed and the border goes solid green.
//!
//! Uncompleting is a single click: reopening a task by accident is cheap.

use eframe::egui::{
    Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};
use web_time::Duration;

use crate::tabs::tasks::pending;
use crate::ui_tools::icons;
use database::schema::{LiveTaskPayload, RecordIdExt, TaskField};

/// Time a first click stays armed, waiting for the confirming second click.
pub const ARM_WINDOW: Duration = Duration::from_millis(1500);

const BUTTON_SIZE: Vec2 = Vec2::new(25.0, 20.0);
const RING_WIDTH: f32 = 1.6;
/// Segments per rounded corner when tracing the perimeter.
const CORNER_SEGMENTS: usize = 6;

/// What the control is currently showing.
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// Open, idle.
    Open,
    /// One click landed; `progress` of the arm window has elapsed.
    Arming(f32),
    /// Completion staged; `progress` of the commit delay has elapsed.
    Committing(f32),
    /// Written and complete.
    Complete,
}

/// Draws the control for `task` and returns the response.
///
/// Returns `Some(completed)` when the operator has settled on a new completion
/// state this frame, for the caller to stage.
pub fn complete_button(ui: &mut Ui, task: &LiveTaskPayload) -> (Response, Option<bool>) {
    let key = task.id.key_string();
    let armed_id = Id::new(("task_complete_arm", &key));

    let (rect, response) = ui.allocate_exact_size(BUTTON_SIZE, Sense::click());

    // egui's temp store needs Default, so the arm time is the frame clock in
    // seconds rather than an Instant.
    let now = ui.input(|i| i.time);
    let arm_window = ARM_WINDOW.as_secs_f64();
    let armed_at: Option<f64> = ui
        .data_mut(|d| d.get_temp::<f64>(armed_id))
        // An expired arm is dropped so a stale click can never confirm later.
        .filter(|t| now - t < arm_window);

    let staged = pending::get(&key);
    let staging_completion = staged
        .as_ref()
        .filter(|e| e.fields.contains(&TaskField::Completed))
        .cloned();

    let mut request: Option<bool> = None;

    if response.clicked() {
        if task.completed {
            // Reopening needs no confirmation.
            ui.data_mut(|d| d.remove_temp::<f64>(armed_id));
            request = Some(false);
        } else if armed_at.is_some() {
            ui.data_mut(|d| d.remove_temp::<f64>(armed_id));
            request = Some(true);
        } else {
            ui.data_mut(|d| d.insert_temp(armed_id, now));
        }
    }

    // Re-read after the click so the ring reflects this frame's state.
    let armed_at: Option<f64> = ui
        .data_mut(|d| d.get_temp::<f64>(armed_id))
        .filter(|t| now - t < arm_window);

    let phase = if let Some(edit) = &staging_completion {
        if edit.staged.completed {
            Phase::Committing(edit.progress())
        } else {
            Phase::Open
        }
    } else if request == Some(true) {
        // Staged this frame; the pending entry lands after the caller stages it.
        Phase::Committing(0.0)
    } else if let Some(at) = armed_at {
        Phase::Arming((((now - at) / arm_window) as f32).clamp(0.0, 1.0))
    } else if task.completed {
        Phase::Complete
    } else {
        Phase::Open
    };

    // An in-flight ring needs continuous repaints to animate.
    if matches!(phase, Phase::Arming(_) | Phase::Committing(_)) {
        ui.ctx().request_repaint();
    }

    paint(ui, rect, phase, response.hovered());

    let hover = match phase {
        Phase::Arming(_) => "Click again to complete",
        Phase::Committing(_) => "Completing — undo from the notification",
        Phase::Complete => "Complete. Click to reopen",
        Phase::Open => "Click twice to complete",
    };

    (response.on_hover_text(hover), request)
}

/// Colours and glyph for each phase, plus the partial or full border ring.
fn paint(ui: &mut Ui, rect: Rect, phase: Phase, hovered: bool) {
    let visuals = ui.style().visuals.clone();
    let green = Color32::from_rgba_premultiplied(51, 255, 189, 200);
    let pink = Color32::from_rgba_premultiplied(255, 51, 153, 200);
    let corner = CornerRadius::same(4);

    let bg = if hovered {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    ui.painter().rect_filled(rect, corner, bg);

    let (glyph, glyph_color, ring_color, progress) = match phase {
        Phase::Open => (icons::CLOSE, pink, pink, None),
        Phase::Arming(p) => (icons::CHECK, visuals.warn_fg_color, visuals.warn_fg_color, Some(p)),
        Phase::Committing(p) => (icons::CHECK, green, green, Some(p)),
        Phase::Complete => (icons::CHECK, green, green, None),
    };

    match progress {
        // Idle: plain border.
        None => {
            ui.painter().rect_stroke(
                rect,
                corner,
                Stroke::new(0.7, ring_color),
                eframe::egui::StrokeKind::Inside,
            );
        }
        Some(p) => {
            // Unfilled remainder stays visible so the button keeps its outline.
            ui.painter().rect_stroke(
                rect,
                corner,
                Stroke::new(0.7, visuals.widgets.inactive.bg_stroke.color),
                eframe::egui::StrokeKind::Inside,
            );
            let filled = perimeter_prefix(rect, corner.nw as f32, p);
            if filled.len() >= 2 {
                ui.painter().add(Shape::line(
                    filled,
                    Stroke::new(RING_WIDTH, ring_color),
                ));
            }
        }
    }

    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(11.0),
        glyph_color,
    );
}

/// The first `fraction` of the rounded-rect border, traced clockwise from
/// top-centre. Returns the polyline to stroke.
fn perimeter_prefix(rect: Rect, radius: f32, fraction: f32) -> Vec<Pos2> {
    let path = perimeter(rect, radius);
    if path.len() < 2 || fraction <= 0.0 {
        return Vec::new();
    }
    if fraction >= 1.0 {
        return path;
    }

    let total: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    let target = total * fraction;

    let mut out = vec![path[0]];
    let mut walked = 0.0;
    for pair in path.windows(2) {
        let seg = pair[0].distance(pair[1]);
        if walked + seg >= target {
            let t = if seg > 0.0 { (target - walked) / seg } else { 0.0 };
            out.push(pair[0] + (pair[1] - pair[0]) * t);
            break;
        }
        walked += seg;
        out.push(pair[1]);
    }
    out
}

/// Full rounded-rect border as a polyline, clockwise from top-centre and back.
fn perimeter(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius
        .min(rect.width() * 0.5)
        .min(rect.height() * 0.5)
        .max(0.0);
    let mid_x = rect.center().x;
    let mut pts = vec![Pos2::new(mid_x, rect.top())];

    // Screen y grows downward, so increasing angle sweeps clockwise.
    let arc = |center: Pos2, from: f32, to: f32, pts: &mut Vec<Pos2>| {
        if r <= 0.0 {
            return;
        }
        for i in 1..=CORNER_SEGMENTS {
            let a = from + (to - from) * (i as f32 / CORNER_SEGMENTS as f32);
            pts.push(center + Vec2::angled(a) * r);
        }
    };

    let (top, bottom, left, right) = (rect.top(), rect.bottom(), rect.left(), rect.right());
    let quarter = std::f32::consts::FRAC_PI_2;

    pts.push(Pos2::new(right - r, top));
    arc(Pos2::new(right - r, top + r), -quarter, 0.0, &mut pts);
    pts.push(Pos2::new(right, bottom - r));
    arc(Pos2::new(right - r, bottom - r), 0.0, quarter, &mut pts);
    pts.push(Pos2::new(left + r, bottom));
    arc(Pos2::new(left + r, bottom - r), quarter, 2.0 * quarter, &mut pts);
    pts.push(Pos2::new(left, top + r));
    arc(Pos2::new(left + r, top + r), 2.0 * quarter, 3.0 * quarter, &mut pts);
    pts.push(Pos2::new(mid_x, top));

    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), BUTTON_SIZE)
    }

    #[test]
    fn perimeter_starts_and_ends_at_top_centre() {
        let r = rect();
        let path = perimeter(r, 4.0);
        let mid_x = r.center().x;
        assert!((path[0].x - mid_x).abs() < 0.01);
        assert!((path[0].y - r.top()).abs() < 0.01);
        let last = *path.last().unwrap();
        assert!((last.x - mid_x).abs() < 0.01);
        assert!((last.y - r.top()).abs() < 0.01);
    }

    #[test]
    fn perimeter_first_leg_runs_clockwise_to_the_right() {
        let path = perimeter(rect(), 4.0);
        assert!(path[1].x > path[0].x, "first leg must head right along the top");
        assert!((path[1].y - path[0].y).abs() < 0.01);
    }

    #[test]
    fn prefix_length_scales_with_fraction() {
        let r = rect();
        let len = |f: f32| -> f32 {
            perimeter_prefix(r, 4.0, f)
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .sum()
        };
        let full = len(1.0);
        assert!(full > 0.0);
        assert!((len(0.5) / full - 0.5).abs() < 0.02);
        assert!((len(0.25) / full - 0.25).abs() < 0.02);
    }

    #[test]
    fn prefix_is_empty_at_zero_and_whole_at_one() {
        let r = rect();
        assert!(perimeter_prefix(r, 4.0, 0.0).is_empty());
        assert_eq!(
            perimeter_prefix(r, 4.0, 1.0).len(),
            perimeter(r, 4.0).len()
        );
    }

    /// The ring must sweep 12 -> 3 -> 6 -> 9 o'clock. Quarter-way along the
    /// perimeter is the right edge, half is bottom-centre, three-quarters is
    /// the left edge — anything else means it runs backwards or starts wrong.
    #[test]
    fn quarters_land_on_the_expected_clock_positions() {
        let r = rect();
        let tip = |f: f32| *perimeter_prefix(r, 4.0, f).last().unwrap();

        let q1 = tip(0.25);
        assert!(
            (q1.x - r.right()).abs() < 1.5 && q1.y > r.center().y - 2.0,
            "3 o'clock should sit on the right edge, got {q1:?}"
        );

        let q2 = tip(0.5);
        assert!(
            (q2.y - r.bottom()).abs() < 1.5 && (q2.x - r.center().x).abs() < 2.0,
            "6 o'clock should sit at bottom-centre, got {q2:?}"
        );

        let q3 = tip(0.75);
        assert!(
            (q3.x - r.left()).abs() < 1.5 && q3.y < r.center().y + 2.0,
            "9 o'clock should sit on the left edge, got {q3:?}"
        );
    }

    /// A partly-filled ring is one continuous run from the start, never a
    /// detached arc.
    #[test]
    fn prefix_is_a_contiguous_run_from_the_start() {
        let r = rect();
        let path = perimeter_prefix(r, 4.0, 0.4);
        let full = perimeter(r, 4.0);
        assert!(path.len() >= 2);
        assert_eq!(path[0], full[0], "must begin at top-centre");
        for (i, p) in path.iter().enumerate().take(path.len() - 1) {
            assert_eq!(*p, full[i], "point {i} diverges from the full perimeter");
        }
    }

    #[test]
    fn zero_radius_traces_a_plain_rectangle() {
        let r = rect();
        let path = perimeter(r, 0.0);
        // Top-centre, 4 corners, back to top-centre.
        assert_eq!(path.len(), 6);
        assert!(path.iter().all(|p| r.expand(0.01).contains(*p)));
    }
}
