//! Shared AI-task checklist widget — the single render path used by both
//! the AI task card and the diagnostics tab's AI Handoff panel.

use crate::TaskUiActions;
use crate::ui_tools::theme;
use crossbeam::channel::Sender;
use database::schema::{AiTask, AiTaskItem, RecordIdExt, User};
use eframe::egui::{Checkbox, ProgressBar, RichText, Ui, Widget};

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

/// Fraction + thin progress bar. Returns (checked, total).
pub fn ai_checklist_progress(ui: &mut Ui, items: &[AiTaskItem]) -> (usize, usize) {
    let total = items.len();
    let checked = items.iter().filter(|i| i.checked).count();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{checked}/{total}"))
                .strong()
                .color(if checked == total && total > 0 {
                    theme::success(ui)
                } else {
                    ui.style().visuals.text_color()
                }),
        );
        let frac = if total > 0 { checked as f32 / total as f32 } else { 0.0 };
        ProgressBar::new(frac).desired_height(6.0).desired_width(140.0).ui(ui);
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
        ui.label(RichText::new("No checklist items").weak().small());
        return;
    }
    let mut sorted: Vec<&AiTaskItem> = items.iter().collect();
    sorted.sort_by_key(|i| i.position);

    for item in sorted {
        ui.push_id(("ai_item", item.id.key_string()), |ui| {
            ui.horizontal(|ui| {
                let mut checked = item.checked;
                let response = ui.add_enabled(interactive, Checkbox::new(&mut checked, ""));
                if response.changed() {
                    let _ = tx.try_send(TaskUiActions::ToggleAiCheckItem {
                        ai_task_id: ai_task.id.clone(),
                        item_id: item.id.clone(),
                        checked,
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
                            ui.label(
                                RichText::new(format!(
                                    "{} {} {}",
                                    crate::ui_tools::icons::CHECK,
                                    user_name(store_users, by),
                                    short_time(at)
                                ))
                                .weak()
                                .small()
                                .color(theme::success(ui)),
                            );
                        }
                    } else {
                        ui.label(text);
                    }
                });
            });
        });
    }
}
