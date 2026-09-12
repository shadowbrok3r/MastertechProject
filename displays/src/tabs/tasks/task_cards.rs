use eframe::egui::{Align, Button, CollapsingHeader, Color32, Frame, Layout, Margin, RichText, Shadow, TextFormat, TextStyle, Ui, Vec2, Widget, WidgetText, text::LayoutJob};
use database::schema::{LiveTaskPayload, RecordIdExt, TaskNotePayload, User};
use crossbeam::channel::Sender;
use chrono::{DateTime, Utc};
use log::info;

use crate::tabs::tasks::pending;
use crate::{Displayable, Interaction, TaskUiActions};
use crate::modals::task_modal::ModalAction;
use crate::ui_tools::{icons, theme};

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

/// AI-recommendation counterpart to the notes button: count, robot glyph and
/// an unread dot that fires on steps added or reworded since the last open.
fn recommendation_badge(ui: &mut Ui, style: &eframe::egui::Style, summary: RecommendationSummary) -> eframe::egui::Response {
    let unseen = summary.unseen > 0;
    let font = style
        .text_styles
        .get(&TextStyle::Button)
        .cloned()
        .unwrap_or_default();
    let text_color = if unseen {
        Color32::from_rgb(250, 100, 80)
    } else {
        style.visuals.warn_fg_color
    };
    let dot_color = if unseen { Color32::from_rgb(250, 100, 80) } else { Color32::TRANSPARENT };

    let mut job = LayoutJob::default();
    job.append(
        &format!("{} {} ", summary.total, icons::ROBOT),
        0.0,
        TextFormat { font_id: font.clone(), color: text_color, ..Default::default() },
    );
    job.append(
        "●",
        0.0,
        TextFormat { font_id: font, color: dot_color, ..Default::default() },
    );

    let hover = if unseen {
        format!(
            "{} recommendation(s), {} new or reworded since you last opened this task",
            summary.total, summary.unseen
        )
    } else {
        format!("{} recommendation(s)", summary.total)
    };

    Button::new(WidgetText::from(job))
        .min_size(Vec2::new(25.0, 20.0))
        .ui(ui)
        .on_hover_text(hover)
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
    ) {
        let style = ui.style().clone();

        // Show staged edits while the write is held; column placement still
        // uses the unstaged value, so the card keeps its position.
        pending::apply_staged(self);
        
        let mut frame = Frame::default()
            .fill(style.visuals.extreme_bg_color) // (Color32::from_rgb(14, 14, 18))
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(5))
            .corner_radius(eframe::egui::CornerRadius::same(15))
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

                ui.style_mut().spacing.button_padding.x = 6.0;
                ui.style_mut().spacing.button_padding.y = 4.0;

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

                // Always lay out the unread dot (transparent when read) so the button, and
                // therefore the card, keeps a constant width regardless of unread state.
                let txt: WidgetText = if count > 0 {
                    let button_font = style
                        .text_styles
                        .get(&TextStyle::Button)
                        .cloned()
                        .unwrap_or_default();
                    let text_color = if has_unread {
                        Color32::from_rgb(250, 100, 80)
                    } else {
                        style.visuals.warn_fg_color
                    };
                    let dot_color = if has_unread {
                        Color32::from_rgb(250, 100, 80)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let mut job = LayoutJob::default();
                    job.append(
                        &format!("{count} 💬 "),
                        0.0,
                        TextFormat { font_id: button_font.clone(), color: text_color, ..Default::default() },
                    );
                    job.append(
                        "●",
                        0.0,
                        TextFormat { font_id: button_font, color: dot_color, ..Default::default() },
                    );
                    job.into()
                } else {
                    RichText::new("  💬").color(Color32::WHITE).into()
                };

                if Button::new(txt)
                    .min_size(Vec2::new(25.0, 20.0))
                    .ui(ui)
                    .on_hover_text(if has_unread { "Task notes — new since you last opened this task" } else { "Open Task Notes" })
                    .clicked()
                {
                    let _ = tx.try_send(
                        TaskUiActions::OpenChatModal((
                            self.id.clone(),
                            notes.clone(),
                            self.service_number.clone()
                        )),
                    );
                }

                let button = Button::new("⮫")
                    .min_size(Vec2::new(25.0, 20.0))
                    .ui(ui)
                    .on_hover_text("Open Task Modal");
                    
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

            ui.separator();

            ui.horizontal(|ui: &mut Ui| {
                ui.push_id(format!("Assignee {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_assignee(ui, store_users, user);
                });

                ui.add_space(22.);
                
                ui.push_id(format!("Priority {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_priority(ui);
                });

                ui.add_space(22.);

                ui.push_id(format!("Status {}", self.id.key_string().clone()), |ui| {
                    let _ = self.interact_status(user, ui);
                });

                ui.add_space(22.);

                let _ = self.interact_due_date(ui);
            });

            ui.separator();

            ui.horizontal(|ui| {
                let task_descrip_header = ui.make_persistent_id(format!("task_description {:?}",self.id.clone()));
                let task_descrip_head = CollapsingHeader::new("Task Description").id_salt(task_descrip_header);
                task_descrip_head.show_unindented(ui, |ui| {
                    let _ = self.interact_task_description(ui);
                });

                if recommendations.total > 0 {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if recommendation_badge(ui, &style, recommendations).clicked() {
                            let _ = tx.try_send(TaskUiActions::OpenTaskModalAtPage {
                                task: self.to_owned(),
                                page: ModalAction::DiagnosticsPage,
                            });
                        }
                    });
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
