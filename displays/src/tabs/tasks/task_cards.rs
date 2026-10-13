use eframe::egui::{Color32, Frame, Id, Margin, Shadow, Ui};
use database::schema::{LiveTaskPayload, RecordIdExt, TaskNotePayload, User};
use crossbeam::channel::Sender;
use chrono::{DateTime, Utc};
use log::info;

use crate::tabs::tasks::pending;
use crate::{Displayable, Interaction, TaskUiActions};
use crate::modals::task_modal::ModalAction;
use crate::ui_tools::{
    controls::{self, IconBox, IconButton, Section},
    icons::svg,
    metrics::Metrics,
    tokens::Tokens,
    theme,
};

/// AI checklist activity for one task, as shown on its card.
#[derive(Clone, Copy, Default)]
pub struct RecommendationSummary {
    /// Checklist steps across every AI task attached to this task.
    pub total: usize,
    /// Steps added or reworded since the tech last opened the task.
    pub unseen: usize,
}

impl RecommendationSummary {
    /// Counts the items of one AI task against the task's last-read stamp.
    pub fn accumulate(
        &mut self,
        items: &[database::schema::AiTaskItem],
        last_read: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        self.total += items.len();
        self.unseen += items
            .iter()
            .filter(|item| {
                let touched: chrono::DateTime<chrono::Utc> = item
                    .updated_at
                    .clone()
                    .unwrap_or_else(|| item.created_at.clone())
                    .into();
                last_read.map(|lr| touched > lr).unwrap_or(true)
            })
            .count();
    }
}

/// Robot icon button with the recommendation count; primary while a step is unseen.
fn recommendation_badge(ui: &mut Ui, summary: RecommendationSummary) -> eframe::egui::Response {
    let t = Tokens::get(ui.ctx());
    let unseen = summary.unseen > 0;
    let tone = if unseen {
        t.primary
    } else if summary.total == 0 {
        t.text_faint
    } else {
        t.warn
    };

    let hover = if unseen {
        format!(
            "{} recommendation(s), {} new or reworded since you last opened this task",
            summary.total, summary.unseen
        )
    } else if summary.total == 0 {
        "No AI recommendations on this task".to_string()
    } else {
        format!("{} recommendation(s)", summary.total)
    };

    let response = IconButton::new(svg::ROBOT, IconBox::Md)
        .tint(tone)
        .tip(&hover)
        .show(ui);
    controls::count_badge(ui, response.rect, summary.total, tone);
    response
}

/// Unread-note and description-edit highlight.
fn unread_tone(ui: &Ui) -> Color32 {
    theme::primary(ui)
}

impl Displayable for LiveTaskPayload {
    fn display_cards(
        &mut self,
        ui: &mut Ui,
        user: &User,
        store_users: &Vec<User>,
        notes: Vec<TaskNotePayload>,
        tx: Sender<TaskUiActions>,
        last_read: Option<chrono::DateTime<chrono::Utc>>,
        recommendations: RecommendationSummary,
        description_edit: Option<(chrono::DateTime<chrono::Utc>, String)>,
    ) {
        let style = ui.style().clone();

        // Show staged edits while the write is held; column placement still
        // uses the unstaged value, so the card keeps its position.
        pending::apply_staged(self);

        let mut frame = Frame::default()
            .fill(style.visuals.extreme_bg_color)
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(4))
            .corner_radius(Tokens::get(ui.ctx()).radius_lg)
            .shadow(Shadow::NONE)
            .begin(ui);

        {
            let ui = &mut frame.content_ui;
            let available = ui.available_width();
            const CARD_MARGIN: f32 = 8.0; // match the Frame::inner_margin you’re using
            ui.set_width(available - CARD_MARGIN * 2.0);


            // ui.set_max_height(300.);
            ui.set_min_height(67.0);

            ui.horizontal(|ui| {
                let _ = self.interact_task_name(ui);

                let mut count = 0;
                if !notes.is_empty() {
                    count = notes.len();
                }

                // Determine if there are unread notes (notes newer than last_read, not from current user)
                let has_unread = {
                    let current_user_id = user.get_id();
                    notes.iter().any(|note| {
                        let not_self = note.user != current_user_id;
                        let created: chrono::DateTime<chrono::Utc> = note.created_at.clone().into();
                        let unread = match last_read {
                            Some(lr) => created > lr,
                            None => true,
                        };
                        not_self && unread
                    })
                };

                let tone = if has_unread {
                    unread_tone(ui)
                } else if count > 0 {
                    style.visuals.warn_fg_color
                } else {
                    theme::text(ui)
                };
                let tip = if has_unread { "Task notes — new since you last opened this task" } else { "Open Task Notes" };
                let notes_button = IconButton::new(svg::CHAT_TEXT, IconBox::Md)
                    .tint(tone)
                    .tip(tip)
                    .show(ui);
                controls::count_badge(ui, notes_button.rect, count, tone);
                if notes_button.clicked() {
                    let _ = tx.try_send(
                        TaskUiActions::OpenChatModal((
                            self.id.clone(),
                            notes.clone(),
                            self.service_number.clone()
                        )),
                    );
                }

                let button = controls::icon_button(ui, svg::ARROW_SQUARE_OUT, IconBox::Md, false, "Open Task Modal");

                if button.clicked() {
                    let _ = tx.try_send(TaskUiActions::OpenTaskModal(self.to_owned()));
                }
                if button.secondary_clicked() {
                    info!("Secondary clicked, opening viewport");
                    let _ = tx.try_send(TaskUiActions::OpenViewport(self.to_owned()));
                }

                // Two clicks to complete, one to reopen; the write is staged
                // and batched, never issued here.
                let _ = self.interact_completed(ui);
            });

            controls::hairline(ui);

            ui.horizontal(|ui: &mut Ui| {
                let row_gap = Metrics::get(ui.ctx()).gap.x;
                ui.push_id(format!("Assignee {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_assignee(ui, store_users, user);
                });

                ui.add_space(row_gap);

                ui.push_id(format!("Priority {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_priority(ui);
                });

                ui.add_space(row_gap);

                ui.push_id(format!("Status {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_status(user, ui);
                });

                ui.add_space(row_gap);

                // Rendered at a fixed size even at zero, so a card with no
                // recommendations is exactly as wide as one with them.
                if recommendation_badge(ui, recommendations).clicked() {
                    let _ = tx.try_send(TaskUiActions::OpenTaskModalAtPage {
                        task: self.to_owned(),
                        page: ModalAction::DiagnosticsPage,
                    });
                }

                ui.add_space(row_gap);

                let _ = self.interact_due_date(ui);
            });

            ui.vertical(|ui| {
                let task_descrip_header = ui.make_persistent_id(format!("task_description {:?}",self.id.clone()));

                // Set while the description changed after the tech last opened the task.
                let unseen_edit = description_edit
                    .as_ref()
                    .filter(|(edited_at, _)| last_read.map(|lr| *edited_at > lr).unwrap_or(true));
                let mut section = Section::new("task_description", "Task Description")
                    .id(ui.make_persistent_id(Id::new(task_descrip_header)))
                    .icon(svg::NOTE_PENCIL)
                    .default_open(false);
                if unseen_edit.is_some() {
                    let tone = unread_tone(ui);
                    section = section.title_tone(tone).icon_tone(tone);
                }
                let (header, open) = section.header_response(ui);
                if open {
                    let _ = self.interact_task_description(ui);
                }

                if let Some((edited_at, editor)) = unseen_edit {
                    header.on_hover_text(format!(
                        "Description edited by {editor} on {} — the Task History tab shows what changed",
                        edited_at.format("%m/%d/%Y %H:%M")
                    ));
                }
            });

        }


        let response = frame.allocate_space(ui);
        if response.hovered() {
            frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
            frame.frame.shadow = style.visuals.window_shadow;
        } else {
            frame.frame.stroke = style.visuals.widgets.open.bg_stroke;
        }
        frame.paint(ui);
    }
}

pub fn date_colors(ui: &mut Ui, due_date: DateTime<Utc>, _complete: bool) -> Color32 {
    let current_date = Utc::now().date_naive();
    let due_date_naive = due_date.date_naive();
    // 3 days in seconds
    let three_days_secs = 3 * 24 * 60 * 60;

    let current_secs = current_date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    let due_secs = due_date_naive.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();

    if due_secs < current_secs {
        theme::error(ui)
    } else if due_secs <= current_secs + three_days_secs {
        theme::warn(ui)
    } else {
        theme::success(ui)
    }
}

#[cfg(test)]
mod recommendation_tests {
    use super::RecommendationSummary;
    use database::schema::AiTaskItem;

    fn item(created: &str, updated: Option<&str>) -> AiTaskItem {
        AiTaskItem {
            created_at: created.parse::<chrono::DateTime<chrono::Utc>>().unwrap().into(),
            updated_at: updated
                .map(|u| u.parse::<chrono::DateTime<chrono::Utc>>().unwrap().into()),
            ..Default::default()
        }
    }

    fn at(stamp: &str) -> chrono::DateTime<chrono::Utc> {
        stamp.parse().unwrap()
    }

    #[test]
    fn a_step_added_after_the_last_read_is_unseen() {
        let mut summary = RecommendationSummary::default();
        summary.accumulate(
            &[item("2026-01-01T00:00:00Z", None), item("2026-01-03T00:00:00Z", None)],
            Some(at("2026-01-02T00:00:00Z")),
        );
        assert_eq!(summary.total, 2);
        assert_eq!(summary.unseen, 1);
    }

    #[test]
    fn a_reworded_step_counts_as_unseen() {
        let mut summary = RecommendationSummary::default();
        summary.accumulate(
            &[item("2026-01-01T00:00:00Z", Some("2026-01-03T00:00:00Z"))],
            Some(at("2026-01-02T00:00:00Z")),
        );
        assert_eq!(summary.unseen, 1);
    }

    #[test]
    fn a_task_never_opened_has_every_step_unseen() {
        let mut summary = RecommendationSummary::default();
        summary.accumulate(&[item("2026-01-01T00:00:00Z", None)], None);
        assert_eq!(summary.unseen, 1);
    }
}
