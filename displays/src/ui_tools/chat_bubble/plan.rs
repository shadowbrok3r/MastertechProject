//! The agent's `update_plan` checklist: its steps, a one-line summary, and the panel pinned above the composer.

use database::schema::{Plan, PlanStatus};
use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::{
    CornerRadius, Frame, Id, Label, Margin, ScrollArea, Stroke, TextFormat, Ui, text::LayoutJob,
};

use super::{ChatStyle, wrap_width};
use crate::ui_tools::icons;

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
    if let Some(why) = &plan.explanation {
        ui.add(Label::new(eframe::egui::RichText::new(why).font(style.small.clone()).color(style.weak)).wrap());
        ui.add_space(2.0);
    }
    for step in &plan.steps {
        step_line(ui, style, step.status, &step.step);
    }
}

fn step_line(ui: &mut Ui, style: &ChatStyle, status: PlanStatus, step: &str) {
    let (glyph, glyph_ink, ink, strike) = match status {
        PlanStatus::Completed => (icons::PLAN_DONE, style.secondary, style.weak, true),
        PlanStatus::InProgress => (icons::PLAN_ACTIVE, style.primary, style.strong, false),
        PlanStatus::Pending => (icons::PLAN_PENDING, style.weak, style.text, false),
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width(ui);
    job.append(glyph, 0.0, TextFormat { font_id: style.body.clone(), color: glyph_ink, ..Default::default() });
    job.append(
        step,
        8.0,
        TextFormat {
            font_id: style.body.clone(),
            color: ink,
            strikethrough: if strike { Stroke::new(1.0, ink) } else { Stroke::NONE },
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.add(Label::new(galley));
}

/// The session's current plan over the composer, open while steps remain; the steps scroll past `max_height`.
pub fn pinned_plan(ui: &mut Ui, style: &ChatStyle, id: Id, plan: &Plan, max_height: f32) {
    let state = CollapsingState::load_with_default_open(ui.ctx(), id.with(plan.complete()), !plan.complete());
    let open = state.is_open();
    Frame::new()
        .corner_radius(CornerRadius::same(style.radius))
        .inner_margin(Margin::symmetric(8, 4))
        .stroke(Stroke::new(1.0, style.rim))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            state
                .show_header(ui, |ui| {
                    let mut job = LayoutJob::default();
                    let fmt = |color| TextFormat { font_id: style.label.clone(), color, ..Default::default() };
                    job.append(icons::PLAN, 0.0, fmt(style.secondary));
                    job.append("Plan", 6.0, fmt(style.strong));
                    let detail = if open { plan_progress(plan) } else { plan_summary(plan) };
                    job.append(&detail, 8.0, TextFormat { font_id: style.small.clone(), color: style.weak, ..Default::default() });
                    ui.add(Label::new(job).truncate());
                })
                .body(|ui| {
                    ScrollArea::vertical()
                        .id_salt(id.with("steps"))
                        .max_height(max_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| plan_steps(ui, style, plan));
                });
        });
}
