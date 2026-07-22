//! Card UI for AI hands-on handoff tasks in the My Tasks "AI Tasks" column.
//!
//! Two roles render the same data differently: the assigned tech gets an
//! open interactive checklist; the requesting operator gets a done-state
//! review card with Open Diagnostics / Resume Session / Close actions.

use crate::modals::tabs::ai_checklist_panel::{ai_checklist_progress, display_ai_checklist};
use crate::ui_tools::{icons, theme};
use crate::TaskUiActions;
use crossbeam::channel::Sender;
use database::schema::{AiTask, AiTaskItem, AiTaskStatus, LiveTaskPayload, RecordIdExt, TaskNotePayload, User};
use eframe::egui::{Button, CollapsingHeader, ComboBox, Frame, Margin, RichText, ScrollArea, Shadow, Ui, Vec2, Widget};

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

        let mut frame = Frame::default()
            .fill(style.visuals.extreme_bg_color)
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(5))
            .corner_radius(eframe::egui::CornerRadius::same(15))
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
                        ui.style_mut().spacing.button_padding.x = 6.0;
                        ui.style_mut().spacing.button_padding.y = 4.0;

                        // Jump straight to the diagnostics hub for this handoff.
                        if Button::new(icons::DIAGNOSTICS)
                            .min_size(Vec2::new(25.0, 20.0))
                            .ui(ui)
                            .on_hover_text("Open Diagnostics tab")
                            .clicked()
                        {
                            let _ = tx.try_send(TaskUiActions::OpenTaskDiagnostics {
                                task_id: task.task_ref.clone(),
                                session: Some(task.session_ref.clone()),
                            });
                        }

                        if Button::new("⮫")
                            .min_size(Vec2::new(25.0, 20.0))
                            .ui(ui)
                            .on_hover_text("Open Task Modal")
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
                        let label = if count > 0 {
                            let color = if has_unread {
                                eframe::egui::Color32::from_rgb(250, 100, 80)
                            } else {
                                style.visuals.warn_fg_color
                            };
                            RichText::new(format!("{count} 💬")).color(color)
                        } else {
                            RichText::new("  💬")
                        };
                        if Button::new(label)
                            .min_size(Vec2::new(25.0, 20.0))
                            .ui(ui)
                            .on_hover_text("Open Task Notes")
                            .clicked()
                        {
                            let _ = tx.try_send(TaskUiActions::OpenChatModal((
                                task.task_ref.clone(),
                                notes.clone(),
                                Some(task.service_number.clone()),
                            )));
                        }

                        // Remaining width: robot + one-line truncated headline
                        // (always "{customer} - {service#}  —  {summary}") so a
                        // long title can never grow the card / column.
                        let headline = format!(
                            "{} - {}  —  {}",
                            task.customer_name, task.service_number, task.title
                        );
                        ui.with_layout(
                            eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                            |ui| {
                                ui.add(
                                    eframe::egui::Label::new(
                                        RichText::new(format!("{} {}", icons::ROBOT, headline))
                                            .strong(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&headline);
                            },
                        );
                    },
                );
            });
            if self.linked_task.is_none() {
                ui.label(RichText::new("(task deleted)").weak().small().italics());
            }

            ui.separator();

            ui.horizontal(|ui| {
                // Assignee combo — reassignment clears the attention ack.
                let assignee_name = user_name(store_users, &task.assignee);
                ui.push_id(("ai_assignee", &key), |ui| {
                    ComboBox::from_id_salt(("ai_assignee_cb", &key))
                        .selected_text(assignee_name)
                        .show_ui(ui, |ui| {
                            for u in store_users.iter().filter(|u| u.is_active()) {
                                if ui.selectable_label(
                                    u.get_id() == task.assignee,
                                    u.get_name(),
                                ).clicked() && u.get_id() != task.assignee {
                                    let _ = tx.try_send(TaskUiActions::ReassignAiTask {
                                        ai_task_id: task.id.clone(),
                                        assignee: u.get_id(),
                                    });
                                }
                            }
                        });
                });

                ui.label(
                    RichText::new(format!(
                        "requested by {}",
                        user_name(store_users, &task.requested_by)
                    ))
                    .weak()
                    .small(),
                );

                if task.status == AiTaskStatus::AwaitingFollowup {
                    ui.label(
                        RichText::new("• AWAITING OPERATOR")
                            .color(theme::warn(ui))
                            .strong()
                            .small(),
                    );
                }
            });

            ui.separator();

            let (checked, total) = ai_checklist_progress(ui, &self.items);

            if self.in_grace && done {
                ui.label(
                    RichText::new(format!(
                        "{} All done — {} notified",
                        icons::CHECK,
                        user_name(store_users, &task.requested_by)
                    ))
                    .color(theme::success(ui))
                    .strong(),
                );
            }

            let interactive = task.status != AiTaskStatus::Closed
                && (current_user.get_id() == task.assignee
                    || current_user.get_id() == task.requested_by);
            let default_open = self.role == AiCardRole::AssignedTech;
            CollapsingHeader::new(format!("Checklist ({checked}/{total})"))
                .id_salt(("ai_checklist", &key))
                .default_open(default_open)
                .show_unindented(ui, |ui| {
                    // Height-capped even when open so a long checklist can't
                    // balloon the card/column.
                    ScrollArea::vertical()
                        .id_salt(("ai_checklist_scroll", &key))
                        .max_height(220.0)
                        .show(ui, |ui| {
                            display_ai_checklist(ui, task, &self.items, store_users, interactive, tx);
                        });
                });

            // Operator review actions.
            if self.role == AiCardRole::Operator {
                ui.separator();
                ui.horizontal(|ui| {
                    if Button::new(crate::ui_tools::icons::menu_item(
                        icons::DIAGNOSTICS,
                        "Open diagnostics",
                    ))
                    .small()
                    .ui(ui)
                    .clicked()
                    {
                        let _ = tx.try_send(TaskUiActions::OpenTaskDiagnostics {
                            task_id: task.task_ref.clone(),
                            session: Some(task.session_ref.clone()),
                        });
                    }
                    if let Some(cs) = task.connection_string.as_ref().filter(|c| !c.is_empty()) {
                        if Button::new(crate::ui_tools::icons::menu_item(
                            icons::MONITOR,
                            "Resume session",
                        ))
                        .small()
                        .ui(ui)
                        .clicked()
                        {
                            let _ = tx.try_send(TaskUiActions::OpenAdminConsole(cs.clone()));
                        }
                    }
                    if Button::new(RichText::new("Close").weak())
                        .small()
                        .ui(ui)
                        .on_hover_text("Accept the handback and close this AI task")
                        .clicked()
                    {
                        let _ = tx.try_send(TaskUiActions::CloseAiTask(task.id.clone()));
                    }
                });
            }
        }

        let response = frame.allocate_space(ui);
        if self.in_grace && done {
            frame.frame.stroke = eframe::egui::Stroke::new(1.2, theme::success(ui));
        } else if response.hovered() {
            frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
            frame.frame.shadow = style.visuals.window_shadow;
        } else {
            frame.frame.stroke = style.visuals.widgets.open.bg_stroke;
        }
        frame.paint(ui);
    }
}
