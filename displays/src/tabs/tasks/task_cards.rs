use chrono::{DateTime, Utc};
use crossbeam::channel::Sender;
use database::schema::{LiveTaskPayload, Priority, RecordIdExt, Status, TaskNotePayload, User};
use eframe::egui::{
    Align, Color32, Frame, Id, Layout, Margin, Shadow, TextStyle, TextWrapMode, Ui, vec2,
};
use log::info;

use crate::modals::task_modal::ModalAction;
use crate::tabs::tasks::{interactable, pending};
use crate::ui_tools::{
    controls::{self, IconBox, IconButton, Section},
    glass_card::FixedWidthFrame,
    icons::svg,
    theme,
    tokens::Tokens,
};
use crate::{Displayable, Interaction, TaskUiActions};

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

        let frame = Frame::default()
            .fill(style.visuals.extreme_bg_color)
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(4))
            .corner_radius(Tokens::get(ui.ctx()).radius_lg)
            .stroke(style.visuals.widgets.open.bg_stroke)
            .shadow(Shadow::NONE);
        let width = ui.available_width();
        let mut card = FixedWidthFrame::begin(ui, frame, width);

        {
            let ui = &mut card.content_ui;
            ui.set_min_height(67.0);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Two clicks to complete, one to reopen; the write is staged
                    // and batched, never issued here.
                    let _ = self.interact_completed(ui);

                    let button = controls::icon_button(
                        ui,
                        svg::ARROW_SQUARE_OUT,
                        IconBox::Md,
                        false,
                        "Open Task Modal",
                    );
                    if button.clicked() {
                        let _ = tx.try_send(TaskUiActions::OpenTaskModal(self.to_owned()));
                    }
                    if button.secondary_clicked() {
                        info!("Secondary clicked, opening viewport");
                        let _ = tx.try_send(TaskUiActions::OpenViewport(self.to_owned()));
                    }

                    let count = notes.len();
                    // Unread: a note from someone else newer than last_read.
                    let has_unread = {
                        let current_user_id = user.get_id();
                        notes.iter().any(|note| {
                            let not_self = note.user != current_user_id;
                            let created: chrono::DateTime<chrono::Utc> = note.created_at.into();
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
                    let tip = if has_unread {
                        "Task notes — new since you last opened this task"
                    } else {
                        "Open Task Notes"
                    };
                    let notes_button = IconButton::new(svg::CHAT_TEXT, IconBox::Md)
                        .tint(tone)
                        .tip(tip)
                        .show(ui);
                    controls::count_badge(ui, notes_button.rect, count, tone);
                    if notes_button.clicked() {
                        let _ = tx.try_send(TaskUiActions::OpenChatModal((
                            self.id.clone(),
                            notes.clone(),
                            self.service_number.clone(),
                        )));
                    }

                    let name_w = ui.available_width();
                    let _ = interactable::task_name_edit(self, ui, name_w);
                });
            });

            controls::hairline(ui);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let _ = ui.scope(|ui| self.interact_due_date(ui));

                    if recommendation_badge(ui, recommendations).clicked() {
                        let _ = tx.try_send(TaskUiActions::OpenTaskModalAtPage {
                            task: self.to_owned(),
                            page: ModalAction::DiagnosticsPage,
                        });
                    }

                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let [assignee_w, priority_w, status_w] = combo_widths(ui);
                        let key = self.id.key_string();
                        ui.push_id(format!("Assignee {key}"), |ui| {
                            truncating_slot(ui, assignee_w, |ui| {
                                interactable::assignee_combo(
                                    self,
                                    ui,
                                    store_users,
                                    user,
                                    assignee_w,
                                )
                            });
                        });
                        ui.push_id(format!("Priority {key}"), |ui| {
                            truncating_slot(ui, priority_w, |ui| {
                                interactable::priority_combo(self, ui, priority_w)
                            });
                        });
                        ui.push_id(format!("Status {key}"), |ui| {
                            truncating_slot(ui, status_w, |ui| {
                                interactable::status_combo(self, user, ui, status_w)
                            });
                        });
                    });
                });
            });

            ui.vertical(|ui| {
                let task_descrip_header = ui.make_persistent_id(format!("task_description {:?}",self.id.clone()));

                // Set while the description changed after the tech last opened the task.
                let unseen_edit = description_edit
                    .as_ref()
                    .filter(|(edited_at, _)| last_read.map(|lr| *edited_at > lr).unwrap_or(true));
                let mut section = Section::new("task_description", "Task Description")
                    .id(ui.make_persistent_id(Id::unique(task_descrip_header)))
                    .icon(svg::NOTE_PENCIL)
                    .default_open(false);
                if unseen_edit.is_some() {
                    let tone = unread_tone(ui);
                    section = section.title_tone(tone).icon_tone(tone);
                }
                let (header, open) = section.header_response(ui);
                if open {
                    let description_w = ui.available_width();
                    let _ = interactable::task_description_edit(self, ui, description_w);
                }

                if let Some((edited_at, editor)) = unseen_edit {
                    header.on_hover_text(format!(
                        "Description edited by {editor} on {} — the Task History tab shows what changed",
                        edited_at.format("%m/%d/%Y %H:%M")
                    ));
                }
            });
        }

        let response = card.allocate_space(ui);
        if response.hovered() {
            card.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
            card.frame.shadow = style.visuals.window_shadow;
        }
        card.paint(ui);
    }
}

/// Assignee, priority and status widths filling the row; the assignee takes what is left.
fn combo_widths(ui: &Ui) -> [f32; 3] {
    let spacing = ui.spacing();
    let chrome = spacing.icon_spacing + spacing.icon_width + 2.0 * spacing.button_padding.x;
    let font = TextStyle::Button.resolve(ui.style());
    let text_w = |label: &str| {
        ui.painter()
            .layout_no_wrap(label.to_owned(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    };
    let priority = Priority::VALUES
        .iter()
        .map(|p| text_w(p.as_str()))
        .fold(0.0, f32::max)
        + chrome;
    let status = Status::VALUES
        .iter()
        .map(|s| text_w(s.as_str()))
        .fold(0.0, f32::max)
        + chrome;
    let wants = [priority.ceil(), status.ceil()];
    let room = (ui.available_width() - 2.0 * spacing.item_spacing.x).max(0.0);
    let level = fill_level(room, wants).floor();
    let [priority, status] = wants.map(|want| want.min(level));
    [room - priority - status, priority, status]
}

/// The level `c` at which `c + min(wants[0], c) + min(wants[1], c)` equals `room`.
fn fill_level(room: f32, wants: [f32; 2]) -> f32 {
    let [low, high] = if wants[0] <= wants[1] {
        wants
    } else {
        [wants[1], wants[0]]
    };
    if room / 3.0 <= low {
        room / 3.0
    } else if (room - low) / 2.0 <= high {
        (room - low) / 2.0
    } else {
        room - low - high
    }
}

/// Runs `add` in a slot exactly `width` wide whose text truncates instead of widening it.
fn truncating_slot<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width);
            ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
            add(ui)
        },
    )
    .inner
}

pub fn date_colors(ui: &mut Ui, due_date: DateTime<Utc>, _complete: bool) -> Color32 {
    let current_date = Utc::now().date_naive();
    let due_date_naive = due_date.date_naive();
    // 3 days in seconds
    let three_days_secs = 3 * 24 * 60 * 60;

    let current_secs = current_date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    let due_secs = due_date_naive
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

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
            created_at: created
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
                .into(),
            updated_at: updated.map(|u| u.parse::<chrono::DateTime<chrono::Utc>>().unwrap().into()),
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
            &[
                item("2026-01-01T00:00:00Z", None),
                item("2026-01-03T00:00:00Z", None),
            ],
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

#[cfg(test)]
mod combo_width_tests {
    use super::fill_level;

    #[test]
    fn the_fill_level_uses_the_whole_row() {
        let wants = [88.0, 96.0];
        for room in [40.0, 170.0, 250.0, 290.0, 600.0] {
            let level = fill_level(room, wants);
            let used = level + wants[0].min(level) + wants[1].min(level);
            assert!(
                (used - room).abs() < 1e-3,
                "room {room}: level {level} fills {used}"
            );
        }
        assert!(
            fill_level(290.0, wants) >= 96.0,
            "both stock labels fit a 290 pt row"
        );
        assert_eq!(fill_level(290.0, wants), fill_level(290.0, [96.0, 88.0]));
    }
}
