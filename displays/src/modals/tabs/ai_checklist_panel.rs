//! Shared AI-task checklist widget — the single render path used by both
//! the AI task card and the diagnostics tab's AI Handoff panel.

use crate::TaskUiActions;
use crate::ui_tools::{
    controls::{self, IconBox, IconButton, IconLabel, Metrics},
    icons::svg,
    theme,
};
use crossbeam::channel::Sender;
use database::schema::{AiTask, AiTaskItem, AiTaskStatus, RecordId, RecordIdExt, User};
use eframe::egui::{RichText, Sense, Ui, Vec2};

/// Short "3:41 PM" form of a Surreal datetime.
fn short_time(dt: &database::schema::Datetime) -> String {
    chrono::DateTime::parse_from_rfc3339(&dt.to_string())
        .map(|t| t.with_timezone(&chrono::Local).format("%-I:%M %p").to_string())
        .unwrap_or_default()
}

fn user_name(store_users: &[User], id: &database::schema::RecordId) -> String {
    store_users
        .iter()
        .find(|u| u.get_id() == *id)
        .map(|u| u.get_name().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// True when `user_id` may close `task`: the requester on any handback, the
/// assignee once every item is checked.
pub fn can_close_ai_task(task: &AiTask, user_id: &RecordId, items: &[AiTaskItem]) -> bool {
    task.status == AiTaskStatus::AwaitingFollowup
        && (*user_id == task.requested_by
            || (*user_id == task.assignee
                && !items.is_empty()
                && items.iter().all(|i| i.checked)))
}

/// Fraction + thin progress bar. Returns (checked, total).
pub fn ai_checklist_progress(ui: &mut Ui, items: &[AiTaskItem]) -> (usize, usize) {
    let total = items.len();
    let checked = items.iter().filter(|i| i.checked).count();
    ui.horizontal(|ui| {
        let done = checked == total && total > 0;
        let icon = if done { svg::CHECK_CIRCLE } else { svg::LIST_CHECKS };
        let ink = if done { theme::success(ui) } else { theme::weak_text(ui) };
        let side = Metrics::get(ui.ctx()).icon_sm;
        let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
        icon.paint(ui, slot, side, ink);
        ui.label(
            RichText::new(format!("{checked}/{total}"))
                .strong()
                .color(if done {
                    theme::success(ui)
                } else {
                    ui.style().visuals.text_color()
                }),
        );
        let frac = if total > 0 { checked as f32 / total as f32 } else { 0.0 };
        let tone = if done { theme::success(ui) } else { theme::primary(ui) };
        controls::meter(ui, frac, 140.0, tone);
    });
    (checked, total)
}

/// Render the checklist rows. `interactive` gates the checkboxes; every
/// toggle routes through `TaskUiActions::ToggleAiCheckItem` so the card,
/// column, and modal all mutate the same SharedContext state.
pub fn display_ai_checklist(
    ui: &mut Ui,
    ai_task: &AiTask,
    items: &[AiTaskItem],
    store_users: &[User],
    interactive: bool,
    tx: &Sender<TaskUiActions>,
) {
    if items.is_empty() {
        IconLabel::new(svg::LIST_CHECKS, "No checklist items")
            .small(true)
            .tone(theme::weak_text(ui))
            .show(ui);
        return;
    }
    let mut sorted: Vec<&AiTaskItem> = items.iter().collect();
    // Checked items sink below the open ones; position orders within each group.
    sorted.sort_by_key(|i| (i.checked, i.position));

    for item in sorted {
        ui.push_id(("ai_item", item.id.key_string()), |ui| {
            ui.horizontal(|ui| {
                let (icon, tip) = if item.checked {
                    (svg::CHECK_SQUARE, "Mark not done")
                } else {
                    (svg::SQUARE, "Mark done")
                };
                let mut check = IconButton::new(icon, IconBox::Sm).tip(tip).enabled(interactive);
                if item.checked {
                    check = check.tint(theme::success(ui));
                }
                if check.show(ui).clicked() {
                    let _ = tx.try_send(TaskUiActions::ToggleAiCheckItem {
                        ai_task_id: ai_task.id.clone(),
                        item_id: item.id.clone(),
                        checked: !item.checked,
                    });
                }
                // Clamped wrap width — a collapsed available_width (sizing
                // pass / horizontal parent) must never produce 1-char lines.
                let wrap_w = (ui.available_width() - 8.0).clamp(160.0, 640.0);
                ui.vertical(|ui| {
                    ui.set_max_width(wrap_w);
                    let text = RichText::new(&item.text);
                    if item.checked {
                        ui.label(text.weak());
                        if let (Some(by), Some(at)) = (&item.checked_by, &item.checked_at) {
                            let stamp = format!("{} {}", user_name(store_users, by), short_time(at));
                            IconLabel::new(svg::CHECK, &stamp)
                                .small(true)
                                .tone(theme::success(ui))
                                .show(ui);
                        }
                    } else {
                        ui.label(text);
                    }
                });
            });
        });
    }
}
