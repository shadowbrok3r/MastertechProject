//! Card UI for AI hands-on handoff tasks in the My Tasks "AI Tasks" column.
//!
//! Two roles render the same data differently: the assigned tech gets an
//! open interactive checklist; the requesting operator gets a done-state
//! review card. The Open Diagnostics / Resume Session / Close action row
//! renders for whoever may close the handback.

use crate::modals::tabs::ai_checklist_panel::{
    ai_checklist_progress, can_close_ai_task, display_ai_checklist,
};
use crate::ui_tools::controls::{
    self, ButtonKind, IconBox, IconButton, IconLabel, MenuButton, Metrics, Section, TextButton,
    Tokens,
};
use crate::ui_tools::icons::svg;
use crate::ui_tools::theme;
use crate::TaskUiActions;
use crossbeam::channel::Sender;
use database::schema::{AiTask, AiTaskItem, AiTaskStatus, LiveTaskPayload, RecordIdExt, TaskNotePayload, User, assignable_users};
use eframe::egui::{Frame, Margin, RichText, Shadow, Ui};

#[derive(Clone, PartialEq)]
pub enum AiCardRole {
    AssignedTech,
    Operator,
}

#[derive(Clone)]
pub struct AiTaskCardView {
    pub ai_task: AiTask,
    pub items: Vec<AiTaskItem>,
    pub linked_task: Option<LiveTaskPayload>,
    pub role: AiCardRole,
    pub in_grace: bool,
}

fn user_name(store_users: &[User], id: &database::schema::RecordId) -> String {
    store_users
        .iter()
        .find(|u| u.get_id() == *id)
        .map(|u| u.get_name().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

impl AiTaskCardView {
    pub fn display(
        &self,
        ui: &mut Ui,
        current_user: &User,
        store_users: &[User],
        notes: Vec<TaskNotePayload>,
        last_read: Option<chrono::DateTime<chrono::Utc>>,
        tx: &Sender<TaskUiActions>,
    ) {
        let style = ui.style().clone();
        let task = &self.ai_task;
        let key = task.id.key_string();
        let done = self.items.len() > 0 && self.items.iter().all(|i| i.checked);

        let m = Metrics::get(ui.ctx());
        let mut frame = Frame::default()
            .fill(style.visuals.extreme_bg_color)
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(4))
            .corner_radius(eframe::egui::CornerRadius::same(Tokens::get(ui.ctx()).radius_md))
            .shadow(Shadow::NONE)
            .begin(ui);

        {
            let ui = &mut frame.content_ui;
            let available = ui.available_width();
            const CARD_MARGIN: f32 = 8.0;
            ui.set_width(available - CARD_MARGIN * 2.0);
            ui.set_min_height(67.0);

            ui.horizontal(|ui| {
                ui.with_layout(
                    eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                    |ui| {
                        // Jump straight to the diagnostics hub for this handoff.
                        if controls::icon_button(ui, svg::MICROSCOPE, IconBox::Sm, false, "Open Diagnostics tab")
                            .clicked()
                        {
                            let _ = tx.try_send(TaskUiActions::OpenTaskDiagnostics {
                                task_id: task.task_ref.clone(),
                                session: Some(task.session_ref.clone()),
                            });
                        }

                        if controls::icon_button(ui, svg::ARROW_SQUARE_OUT, IconBox::Sm, false, "Open Task Modal")
                            .clicked()
                        {
                            let _ = tx.try_send(TaskUiActions::OpenTaskModalById(
                                task.task_ref.clone(),
                            ));
                        }

                        // Real task's notes — unread dot matches task_cards.
                        let count = notes.len();
                        let has_unread = {
                            let me = current_user.get_id();
                            notes.iter().any(|note| {
                                let created: chrono::DateTime<chrono::Utc> =
                                    note.created_at.clone().into();
                                note.user != me
                                    && last_read.map(|lr| created > lr).unwrap_or(true)
                            })
                        };
                        let tone = if has_unread { theme::error(ui) } else { theme::warn(ui) };
                        let mut notes_button =
                            IconButton::new(svg::CHAT_TEXT, IconBox::Sm).tip("Open Task Notes");
                        if count > 0 {
                            notes_button = notes_button.tint(tone);
                        }
                        let notes_resp = notes_button.show(ui);
                        controls::count_badge(ui, notes_resp.rect, count, tone);
                        if notes_resp.clicked() {
                            let _ = tx.try_send(TaskUiActions::OpenChatModal((
                                task.task_ref.clone(),
                                notes.clone(),
                                Some(task.service_number.clone()),
                            )));
                        }

                        // One-line headline cut at the remaining width.
                        let headline = format!(
                            "{} - {}  —  {}",
                            task.customer_name, task.service_number, task.title
                        );
                        ui.with_layout(
                            eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                            |ui| {
                                IconLabel::new(svg::ROBOT, &headline)
                                    .icon_tone(theme::accent_secondary(ui))
                                    .strong(true)
                                    .truncate(true)
                                    .show(ui);
                            },
                        );
                    },
                );
            });
            if self.linked_task.is_none() {
                IconLabel::new(svg::TRASH, "Task deleted")
                    .tone(theme::faint_text(ui))
                    .small(true)
                    .show(ui);
            }

            controls::hairline(ui);

            ui.horizontal(|ui| {
                // Assignee combo — reassignment clears the attention ack.
                let assignee_name = user_name(store_users, &task.assignee);
                ui.push_id(("ai_assignee", &key), |ui| {
                    MenuButton::new(&assignee_name)
                        .icon(svg::USER_GEAR)
                        .tip("Reassign this AI task")
                        .show(ui, |ui| {
                            let roster = crate::get_database_users();
                            for u in assignable_users(&roster, current_user.get_store()) {
                                if controls::menu_check_item(
                                    ui,
                                    svg::USER,
                                    u.get_name(),
                                    u.get_id() == task.assignee,
                                ).clicked() && u.get_id() != task.assignee {
                                    let _ = tx.try_send(TaskUiActions::ReassignAiTask {
                                        ai_task_id: task.id.clone(),
                                        assignee: u.get_id(),
                                    });
                                }
                            }
                        });
                });

                ui.add(svg::USER_CIRCLE.image(m.icon_sm).tint(theme::faint_text(ui)));
                ui.label(
                    RichText::new(format!(
                        "requested by {}",
                        user_name(store_users, &task.requested_by)
                    ))
                    .weak()
                    .small(),
                );

                if task.status == AiTaskStatus::AwaitingFollowup {
                    controls::badge(ui, "AWAITING OPERATOR", theme::warn(ui));
                }
            });

            if let Some(theory) = task
                .current_theory
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                controls::hairline(ui);
                IconLabel::new(svg::LIGHTBULB, theory)
                    .icon_tone(theme::warn(ui))
                    .strong(true)
                    .wrap(true)
                    .show(ui);
                if let Some(next) = task
                    .theory_next_step
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                {
                    ui.label(RichText::new(next).weak().small());
                }
            }

            let (checked, total) = ai_checklist_progress(ui, &self.items);

            if self.in_grace && done {
                ui.horizontal(|ui| {
                    let color = theme::success(ui);
                    ui.add(svg::CHECK_CIRCLE.image(m.icon_sm).tint(color));
                    ui.label(
                        RichText::new(format!(
                            "All done — {} notified",
                            user_name(store_users, &task.requested_by)
                        ))
                        .color(color)
                        .strong(),
                    );
                });
            }

            let interactive = task.status != AiTaskStatus::Closed
                && (current_user.get_id() == task.assignee
                    || current_user.get_id() == task.requested_by);
            let default_open = self.role == AiCardRole::AssignedTech;
            let checklist_title = format!("Checklist ({checked}/{total})");
            if Section::new(("ai_checklist", &key), &checklist_title)
                .icon(svg::LIST_CHECKS)
                .default_open(default_open)
                .header(ui)
            {
                display_ai_checklist(ui, task, &self.items, store_users, interactive, tx);
            }

            // Handback actions — requester always, assignee once complete.
            if can_close_ai_task(task, &current_user.get_id(), &self.items) {
                controls::hairline(ui);
                ui.horizontal_wrapped(|ui| {
                    if controls::icon_text_button(ui, svg::MICROSCOPE, "Open diagnostics").clicked() {
                        let _ = tx.try_send(TaskUiActions::OpenTaskDiagnostics {
                            task_id: task.task_ref.clone(),
                            session: Some(task.session_ref.clone()),
                        });
                    }
                    if let Some(cs) = task.connection_string.as_ref().filter(|c| !c.is_empty())
                        && controls::icon_text_button(ui, svg::MONITOR, "Resume session").clicked()
                    {
                        let _ = tx.try_send(TaskUiActions::OpenAdminConsole(cs.clone()));
                    }
                    if TextButton::new("Accept & close")
                        .kind(ButtonKind::Primary)
                        .icon(svg::CHECK)
                        .show(ui)
                        .on_hover_text("Close this AI task and record the outcome in the session log")
                        .clicked()
                    {
                        let _ = tx.try_send(TaskUiActions::CloseAiTask(task.id.clone()));
                    }
                });
            }
        }

        let response = frame.allocate_space(ui);
        if self.in_grace && done {
            frame.frame.stroke = eframe::egui::Stroke::new(1.0, theme::success(ui));
        } else if response.hovered() {
            frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
            frame.frame.shadow = style.visuals.window_shadow;
        } else {
            frame.frame.stroke = style.visuals.widgets.open.bg_stroke;
        }
        frame.paint(ui);
    }
}
