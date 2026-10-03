//! The agent's `update_plan` checklist: its steps, a one-line summary, and the card floating over the transcript.

use database::schema::{Plan, PlanStatus, PlanStep};
use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::{
    Align, Align2, Area, Button, Color32, CornerRadius, CursorIcon, Frame, Id, Label, Layout,
    Margin, Rect, RichText, ScrollArea, Sense, Spinner, Stroke, TextFormat, Ui, UiBuilder, pos2,
    text::LayoutJob, vec2,
};

use super::ChatStyle;
use crate::ui_tools::{glass_backdrop, icons, theme};

/// Gap between the card and the edges of the transcript it floats over.
const CARD_GAP: f32 = 8.0;
const HEADER_H: f32 = 24.0;
const BAR_W: f32 = 72.0;
const BAR_H: f32 = 4.0;
/// Width of the status glyph column in a step row.
const GLYPH_W: f32 = 18.0;
/// Width kept free on the header's right for the chevron and the close button.
const HEADER_TAIL_W: f32 = 52.0;
/// Share of the transcript height the open step list may cover.
const STEPS_SHARE: f32 = 0.45;

/// Steps done out of the total, e.g. "2 of 5 done".
pub fn plan_progress(plan: &Plan) -> String {
    format!("{} of {} done", plan.done(), plan.steps.len())
}

/// Progress and the step being worked on, e.g. "2 of 5 done · Scans".
pub fn plan_summary(plan: &Plan) -> String {
    match plan.current() {
        Some(step) => format!("{} · {}", plan_progress(plan), step.step),
        None => plan_progress(plan),
    }
}

/// The explanation, then one line per step with its status glyph.
pub fn plan_steps(ui: &mut Ui, style: &ChatStyle, plan: &Plan) {
    explanation(ui, style, plan);
    for step in &plan.steps {
        step_row(ui, style, step, false);
    }
}

fn explanation(ui: &mut Ui, style: &ChatStyle, plan: &Plan) {
    if let Some(why) = &plan.explanation {
        ui.add(
            Label::new(
                RichText::new(why)
                    .font(style.small.clone())
                    .color(style.weak),
            )
            .wrap(),
        );
        ui.add_space(2.0);
    }
}

/// A status glyph column, then the step wrapped under itself; `spin` turns the step in progress into a spinner.
fn step_row(ui: &mut Ui, style: &ChatStyle, step: &PlanStep, spin: bool) {
    let (glyph, glyph_ink, ink, strike) = match step.status {
        PlanStatus::Completed => (icons::PLAN_DONE, theme::success(ui), style.weak, true),
        PlanStatus::InProgress => (icons::PLAN_ACTIVE, style.primary, style.strong, false),
        PlanStatus::Pending => (icons::PLAN_PENDING, style.weak, style.text, false),
    };
    let row_h = ui.ctx().fonts_mut(|f| f.row_height(&style.body));
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (slot, _) = ui.allocate_exact_size(vec2(GLYPH_W, row_h), Sense::hover());
        if spin && step.status == PlanStatus::InProgress {
            let size = (row_h - 4.0).max(8.0);
            ui.put(
                Rect::from_center_size(slot.center(), vec2(size, size)),
                Spinner::new().size(size).color(glyph_ink),
            );
        } else {
            ui.painter().text(
                slot.center(),
                Align2::CENTER_CENTER,
                glyph,
                style.body.clone(),
                glyph_ink,
            );
        }
        let mut job = LayoutJob::default();
        job.wrap.max_width = ui.available_width().max(48.0);
        job.append(
            &step.step,
            0.0,
            TextFormat {
                font_id: style.body.clone(),
                color: ink,
                strikethrough: if strike {
                    Stroke::new(1.0, ink)
                } else {
                    Stroke::NONE
                },
                ..Default::default()
            },
        );
        let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
        ui.add(Label::new(galley).selectable(false));
    });
}

/// The plan as a frosted card floating over the bottom of `over`; returns the height it covers.
pub fn floating_plan(
    ui: &Ui,
    style: &ChatStyle,
    scope: Id,
    plan_key: &str,
    plan: &Plan,
    over: Rect,
    busy: bool,
) -> f32 {
    let ctx = ui.ctx();
    let dismiss_id = scope.with(("plan_dismissed", plan_key));
    if ctx
        .data(|d| d.get_temp::<bool>(dismiss_id))
        .unwrap_or(false)
    {
        return 0.0;
    }
    let parent = ui.layer_id();
    let left = over.left() + CARD_GAP;
    let right = over.right() - CARD_GAP - ui.spacing().scroll.allocated_width();
    let width = (right - left).max(160.0);
    let steps_max = (over.height() * STEPS_SHARE).clamp(64.0, 380.0);
    let area = Area::new(scope.with("floating_plan"))
        .order(parent.order)
        .fixed_pos(pos2(left, over.bottom() - CARD_GAP))
        .pivot(Align2::LEFT_BOTTOM)
        .constrain_to(over)
        .movable(false)
        .fade_in(false);
    let layer = area.layer();
    let shown = area.show(ctx, |ui| {
        let frame = card_frame(ui);
        let inner_w = width - frame.total_margin().sum().x;
        glass_backdrop::glass_frame(ui, "plan_card", frame, |ui| {
            ui.set_width(inner_w);
            card(ui, style, scope, dismiss_id, plan, steps_max, busy);
        });
    });
    ctx.set_sublayer(parent, layer);
    shown.response.rect.height() + CARD_GAP
}

/// The theme's window surface; opaque when no frost sits behind a translucent fill.
fn card_frame(ui: &Ui) -> Frame {
    let v = ui.visuals();
    let frosted = glass_backdrop::is_available() && glass_backdrop::params(ui.ctx()).is_visible();
    let fill = if frosted || v.window_fill.a() == 255 {
        v.window_fill
    } else {
        opaque(v.window_fill, v.panel_fill)
    };
    Frame::window(ui.style())
        .fill(fill)
        .inner_margin(Margin::symmetric(10, 6))
}

/// `over` composited onto `base`.
fn opaque(over: Color32, base: Color32) -> Color32 {
    let inv = 1.0 - f32::from(over.a()) / 255.0;
    let ch = |o: u8, b: u8| (f32::from(o) + f32::from(b) * inv).round().min(255.0) as u8;
    Color32::from_rgb(
        ch(over.r(), base.r()),
        ch(over.g(), base.g()),
        ch(over.b(), base.b()),
    )
}

fn card(
    ui: &mut Ui,
    style: &ChatStyle,
    scope: Id,
    dismiss_id: Id,
    plan: &Plan,
    steps_max: f32,
    busy: bool,
) {
    let mut state = CollapsingState::load_with_default_open(
        ui.ctx(),
        scope.with(("plan_open", plan.complete())),
        !plan.complete(),
    );
    header(ui, style, &mut state, dismiss_id, plan);
    state.show_body_unindented(ui, |ui| {
        hairline(ui, style);
        explanation(ui, style, plan);
        ScrollArea::vertical()
            .id_salt(scope.with("plan_steps"))
            .max_height(steps_max)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for step in &plan.steps {
                    step_row(ui, style, step, busy);
                }
            });
    });
    state.store(ui.ctx());
}

/// Icon, title, progress bar and the current step; a click anywhere on it folds the steps.
fn header(
    ui: &mut Ui,
    style: &ChatStyle,
    state: &mut CollapsingState,
    dismiss_id: Id,
    plan: &Plan,
) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if response.clicked() {
        state.toggle(ui);
    }
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(vec2(4.0, 1.0)),
            CornerRadius::same(style.radius),
            style.rim.gamma_multiply(0.5),
        );
    }
    let complete = plan.complete();
    let done_ink = theme::success(ui);
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 6.0;
    let (glyph, glyph_ink) = if complete {
        (icons::PLAN_DONE, done_ink)
    } else {
        (icons::PLAN, style.secondary)
    };
    row.add(
        Label::new(
            RichText::new(glyph)
                .font(style.label.clone())
                .color(glyph_ink),
        )
        .selectable(false),
    );
    row.add(
        Label::new(
            RichText::new("Plan")
                .font(style.label.clone())
                .color(style.strong),
        )
        .selectable(false),
    );
    let progress = format!("{}/{}", plan.done(), plan.steps.len());
    row.add(
        Label::new(
            RichText::new(progress)
                .font(style.small.clone())
                .color(style.weak),
        )
        .selectable(false),
    );
    progress_bar(
        &mut row,
        style,
        plan,
        if complete { done_ink } else { style.secondary },
    );

    let detail = match plan.current() {
        Some(step) => step.step.as_str(),
        None => "All steps done",
    };
    let room = (row.available_width() - HEADER_TAIL_W).max(0.0);
    let ink = if complete { style.weak } else { style.text };
    row.allocate_ui_with_layout(
        vec2(room, HEADER_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add(
                Label::new(RichText::new(detail).font(style.small.clone()).color(ink))
                    .truncate()
                    .selectable(false),
            );
        },
    );

    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let close = ui
            .add(
                Button::new(
                    RichText::new(icons::CLOSE)
                        .font(style.small.clone())
                        .color(style.weak),
                )
                .frame(false),
            )
            .on_hover_text("Hide until the plan changes");
        if close.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(dismiss_id, true));
        }
        let chevron = if state.is_open() {
            icons::PLAN_COLLAPSE
        } else {
            icons::PLAN_EXPAND
        };
        ui.add(
            Label::new(
                RichText::new(chevron)
                    .font(style.small.clone())
                    .color(style.weak),
            )
            .selectable(false),
        );
    });
}

/// A thin bar filled to the share of steps done, eased toward a new share.
fn progress_bar(ui: &mut Ui, style: &ChatStyle, plan: &Plan, fill: Color32) {
    let (rect, response) = ui.allocate_exact_size(vec2(BAR_W, BAR_H), Sense::hover());
    let share = plan.done() as f32 / plan.steps.len().max(1) as f32;
    let shown = ui.ctx().animate_value_with_time(response.id, share, 0.35);
    let radius = CornerRadius::same((BAR_H / 2.0) as u8);
    ui.painter().rect_filled(rect, radius, style.rim);
    if shown > 0.0 {
        let filled = Rect::from_min_size(
            rect.min,
            vec2(rect.width() * shown.clamp(0.0, 1.0), rect.height()),
        );
        ui.painter().rect_filled(filled, radius, fill);
    }
}

fn hairline(ui: &mut Ui, style: &ChatStyle) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, style.rim));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, RawInput};
    use serde_json::json;

    fn plan(steps: usize, done: usize) -> Plan {
        let steps: Vec<_> = (0..steps)
            .map(|i| {
                let status = match i.cmp(&done) {
                    std::cmp::Ordering::Less => "completed",
                    std::cmp::Ordering::Equal => "inProgress",
                    std::cmp::Ordering::Greater => "pending",
                };
                json!({ "step": format!("step {i} with a long description that wraps"), "status": status })
            })
            .collect();
        Plan::from_value(&json!({ "explanation": "Tune-up pass", "plan": steps })).expect("plan")
    }

    /// Runs one frame drawing the card over `over`; returns the covered height and the card's rect.
    fn frame(ctx: &Context, plan: &Plan, over: Rect) -> (f32, Option<Rect>) {
        let mut covered = 0.0;
        let scope = Id::new("plan_test");
        let mut out = ctx.run_ui(
            RawInput {
                screen_rect: Some(over.expand(40.0)),
                ..Default::default()
            },
            |ui| {
                let style = ChatStyle::from_ui(ui);
                covered = floating_plan(ui, &style, scope, "p1", plan, over, true);
            },
        );
        out.textures_delta.clear();
        let rect = ctx.memory(|m| m.area_rect(scope.with("floating_plan")));
        (covered, rect)
    }

    #[test]
    fn the_card_floats_inside_the_transcript_and_reports_what_it_covers() {
        let ctx = Context::default();
        let over = Rect::from_min_size(pos2(40.0, 40.0), vec2(600.0, 500.0));
        let open = plan(12, 3);
        for _ in 0..3 {
            frame(&ctx, &open, over);
        }
        let (covered, rect) = frame(&ctx, &open, over);
        let rect = rect.expect("the card has an area");
        assert!(over.contains_rect(rect), "{rect:?} leaves {over:?}");
        assert!(
            (rect.bottom() - (over.bottom() - CARD_GAP)).abs() < 1.0,
            "anchored to the bottom: {rect:?}"
        );
        assert!(
            covered > HEADER_H + CARD_GAP,
            "an open plan lists its steps: {covered}"
        );
        assert!(
            rect.height() <= over.height() * STEPS_SHARE + 120.0,
            "the step list scrolls: {rect:?}"
        );

        let done = plan(4, 4);
        for _ in 0..3 {
            frame(&ctx, &done, over);
        }
        let (folded, _) = frame(&ctx, &done, over);
        assert!(
            folded < covered,
            "a complete plan starts folded: {folded} vs {covered}"
        );
        assert!(folded >= HEADER_H, "{folded}");
    }

    #[test]
    fn a_dismissed_plan_covers_nothing_until_it_changes() {
        let ctx = Context::default();
        let over = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 400.0));
        let scope = Id::new("plan_test");
        ctx.data_mut(|d| d.insert_temp(scope.with(("plan_dismissed", "p1")), true));
        let (covered, _) = frame(&ctx, &plan(3, 1), over);
        assert_eq!(covered, 0.0);
    }

    #[test]
    fn summaries_name_progress_and_the_current_step() {
        let p = plan(3, 1);
        assert_eq!(plan_progress(&p), "1 of 3 done");
        assert!(plan_summary(&p).starts_with("1 of 3 done · step 1"));
        assert_eq!(plan_summary(&plan(2, 2)), "2 of 2 done");
    }
}
