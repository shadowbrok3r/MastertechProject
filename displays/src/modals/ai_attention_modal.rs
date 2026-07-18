//! Blocking attention/review popups for AI tasks.
//!
//! Queue-driven: `SharedContext.ai_popup_queue` collects popups from the
//! ai_task live stream; `handle_modals` pumps one at a time into
//! `ai_popup_modal` and routes the outcome.

use database::schema::{AiTask, RecordIdExt, User};
use eframe::egui::{Context, Id, Modal, RichText};
use crate::ui_tools::{icons, theme};

#[derive(Clone, Debug, PartialEq)]
pub enum AiPopupKind {
    /// New/reopened checklist for the assigned tech.
    TechAttention,
    /// Completed checklist awaiting the requesting operator.
    OperatorReview,
}

#[derive(Clone, Debug)]
pub struct AiPopup {
    pub kind: AiPopupKind,
    pub ai_task: AiTask,
    pub item_count: usize,
}

pub enum AiAttentionOutcome {
    /// Open the task modal on the Diagnostics tab + acknowledge.
    ViewNow(AiPopup),
    /// Acknowledge only; the card/badge keep it visible.
    Later(AiPopup),
}

pub struct AiAttentionModal {
    pub popup: AiPopup,
    pub queue_remaining: usize,
}

impl AiAttentionModal {
    fn user_name(store_users: &[User], id: &database::schema::RecordId) -> String {
        store_users
            .iter()
            .find(|u| u.get_id() == *id)
            .map(|u| u.get_name().to_string())
            .unwrap_or_else(|| "another user".to_string())
    }

    pub fn show(&self, ctx: &Context, store_users: &[User]) -> Option<AiAttentionOutcome> {
        let mut outcome: Option<AiAttentionOutcome> = None;
        let task = &self.popup.ai_task;
        let modal = Modal::new(Id::new(("ai_attention_modal", task.id.key_string())))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.vertical_centered(|ui| {
                    match self.popup.kind {
                        AiPopupKind::TechAttention => {
                            ui.heading(format!("{} Requires your attention", icons::ROBOT));
                            ui.separator();
                            ui.add_space(8.);
                            ui.label(
                                RichText::new(format!(
                                    "Computer for {} - {} requires your attention",
                                    task.customer_name, task.service_number
                                ))
                                .size(16.0)
                                .strong(),
                            );
                            ui.add_space(4.);
                            ui.label(
                                RichText::new(format!(
                                    "{} checklist items · requested by {}",
                                    self.popup.item_count,
                                    Self::user_name(store_users, &task.requested_by)
                                ))
                                .weak()
                                .small(),
                            );
                        }
                        AiPopupKind::OperatorReview => {
                            ui.heading(format!("{} AI task ready for review", icons::ROBOT));
                            ui.separator();
                            ui.add_space(8.);
                            ui.label(
                                RichText::new(format!(
                                    "{} — {} - {}",
                                    task.title, task.customer_name, task.service_number
                                ))
                                .size(16.0)
                                .strong(),
                            );
                            ui.add_space(4.);
                            let completed = task
                                .completed_at
                                .as_ref()
                                .map(|d| d.to_string())
                                .unwrap_or_default();
                            ui.label(
                                RichText::new(format!(
                                    "All {} steps completed by {} {}",
                                    self.popup.item_count,
                                    Self::user_name(store_users, &task.assignee),
                                    completed
                                ))
                                .weak()
                                .small(),
                            );
                        }
                    }
                    if self.queue_remaining > 0 {
                        ui.add_space(2.);
                        ui.label(
                            RichText::new(format!(
                                "+{} more waiting in your AI Tasks column",
                                self.queue_remaining
                            ))
                            .weak()
                            .small(),
                        );
                    }
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        let view_label = match self.popup.kind {
                            AiPopupKind::TechAttention => "View now",
                            AiPopupKind::OperatorReview => "Review now",
                        };
                        let view = ui.button(
                            RichText::new(view_label).color(theme::success(ui)).strong(),
                        );
                        if view.clicked() {
                            outcome = Some(AiAttentionOutcome::ViewNow(self.popup.clone()));
                        }
                        if ui.button("Later").clicked() {
                            outcome = Some(AiAttentionOutcome::Later(self.popup.clone()));
                        }
                    });
                });
            });

        // ESC / outside click = Later — blocking, not imprisoning.
        if outcome.is_none() && modal.should_close() {
            outcome = Some(AiAttentionOutcome::Later(self.popup.clone()));
        }
        outcome
    }
}
