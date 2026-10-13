//! Blocking attention/review popups for AI tasks.
//!
//! Queue-driven: `SharedContext.ai_popup_queue` collects popups from the
//! ai_task live stream; `handle_modals` pumps one at a time into
//! `ai_popup_modal` and routes the outcome.

use database::schema::{AiTask, RecordIdExt, User};
use eframe::egui::{Context, Id, RichText};
use crate::ui_tools::{
    controls::{self, IconLabel, Metrics, ModalWidth, Role},
    icons::svg,
    theme,
};

/// Size of the popup's lead line.
const LEAD_PT: f32 = 15.0;

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
        let task = &self.popup.ai_task;
        let (title, view_label) = match self.popup.kind {
            AiPopupKind::TechAttention => ("Requires your attention", "View now"),
            AiPopupKind::OperatorReview => ("AI task ready for review", "Review now"),
        };
        let buttons = [(Role::Primary, view_label), (Role::Cancel, "Later")];
        let shown = controls::modal(
            ctx,
            Id::new(("ai_attention_modal", task.id.key_string())),
            svg::ROBOT,
            title,
            ModalWidth::Narrow,
            &buttons,
            |ui| {
                let m = Metrics::get(ui.ctx());
                ui.vertical_centered(|ui| {
                    match self.popup.kind {
                        AiPopupKind::TechAttention => {
                            ui.label(
                                RichText::new(format!(
                                    "Computer for {} - {} requires your attention",
                                    task.customer_name, task.service_number
                                ))
                                .size(LEAD_PT)
                                .strong(),
                            );
                            ui.add_space(m.pad.y);
                            let detail = format!(
                                "{} checklist items · requested by {}",
                                self.popup.item_count,
                                Self::user_name(store_users, &task.requested_by)
                            );
                            IconLabel::new(svg::LIST_CHECKS, &detail)
                                .small(true)
                                .tone(theme::weak_text(ui))
                                .show(ui);
                        }
                        AiPopupKind::OperatorReview => {
                            ui.label(
                                RichText::new(format!(
                                    "{} — {} - {}",
                                    task.title, task.customer_name, task.service_number
                                ))
                                .size(LEAD_PT)
                                .strong(),
                            );
                            ui.add_space(m.pad.y);
                            let completed = task
                                .completed_at
                                .as_ref()
                                .map(|d| d.to_string())
                                .unwrap_or_default();
                            let detail = format!(
                                "All {} steps completed by {} {}",
                                self.popup.item_count,
                                Self::user_name(store_users, &task.assignee),
                                completed
                            );
                            IconLabel::new(svg::CHECK_CIRCLE, &detail)
                                .small(true)
                                .tone(theme::weak_text(ui))
                                .icon_tone(theme::success(ui))
                                .show(ui);
                        }
                    }
                    if self.queue_remaining > 0 {
                        ui.add_space(m.pad.y / 2.0);
                        let waiting = format!(
                            "+{} more waiting in your AI Tasks column",
                            self.queue_remaining
                        );
                        IconLabel::new(svg::QUEUE, &waiting)
                            .small(true)
                            .tone(theme::weak_text(ui))
                            .show(ui);
                    }
                });
            },
        );

        if shown.clicked == Some(0) {
            return Some(AiAttentionOutcome::ViewNow(self.popup.clone()));
        }
        // Escape, an outside click or Later acknowledges only.
        shown
            .should_close
            .then(|| AiAttentionOutcome::Later(self.popup.clone()))
    }
}
