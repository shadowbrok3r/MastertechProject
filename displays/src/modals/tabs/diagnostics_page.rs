//! Diagnostics tab — shows every `DiagnosticSession` linked to this task or
//! to the same computer the ticket references, side-by-side with the
//! customer's check-in notes so the tech can compare reported symptoms
//! against AI-recorded findings.

use database::schema::{AiTask, AiTaskItem, AiTaskStatus, DiagnosticEntry, DiagnosticSession, RecordId, RecordIdExt, User};
use eframe::egui::{
    Button, CollapsingHeader, Color32, Frame, Grid, Margin, RichText, ScrollArea, Spinner, Ui, Vec2,
    Widget,
};
use crate::modals::tabs::ai_checklist_panel::{
    ai_checklist_progress, can_close_ai_task, display_ai_checklist,
};
use crate::ui_tools::{icons, theme};
use crate::TaskUiActions;
use crossbeam::channel::Sender;
use serde::{Deserialize, Serialize};

/// One session paired with all of its entries. Built by
/// `TaskModal::kickoff_diagnostics_load` from `DiagnosticSession::get_full`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticSessionView {
    pub session: DiagnosticSession,
    pub entries: Vec<DiagnosticEntry>,
}

/// AI handoff context for the pinned checklist panels. `None` renders the
/// page without them (the Admin Console per-client popup).
pub struct DiagnosticsPageAiCtx<'a> {
    pub ai_views: &'a [(AiTask, Vec<AiTaskItem>)],
    pub store_users: &'a [User],
    pub current_user: Option<&'a User>,
    pub ui_actions_tx: Option<&'a Sender<TaskUiActions>>,
}

/// Render the diagnostics page. Two columns: left = check-in notes from
/// the ticket, right = scrollable list of diagnostic sessions with their
/// entries inline. Returns true when the operator clicked Refresh.
pub fn display_diagnostics_page(
    ui: &mut Ui,
    avail_size: Vec2,
    sessions: &[DiagnosticSessionView],
    loading: bool,
    error: Option<&str>,
    selected: &mut Option<RecordId>,
    checkin_notes: &str,
    ai_ctx: Option<DiagnosticsPageAiCtx<'_>>,
    show_refresh: bool,
) -> bool {
    let mut refresh_clicked = false;

    let total_w = avail_size.x.max(700.0);
    let left_w = (total_w * 0.32).clamp(220.0, 320.0);
    let right_w = (total_w - left_w - 12.0).max(380.0);
    // Cap the check-in notes so a long expanded note can't grow the modal;
    // the sessions list below fills whatever vertical space remains.
    let notes_h = 120.0;

    ui.vertical_centered_justified(|ui| {
        // AI handoff panels live inside the vertical scope — the outer tab
        // ui is a LeftToRight layout that collapses label wrap widths.
        if let Some(ai) = ai_ctx.as_ref() {
            for (task, items) in ai.ai_views.iter() {
                render_ai_handoff_panel(ui, task, items, ai, selected);
                ui.add_space(6.0);
            }
        }

        ui.collapsing("Check-in Notes", |ui| {
            if checkin_notes.trim().is_empty() {
                ui.colored_label(
                    theme::weak_text(ui),
                    "No check-in notes recorded.",
                );
            } else {
                ScrollArea::vertical()
                    .id_salt("diag_checkin_scroll")
                    .max_height(notes_h)
                    .show(ui, |ui| {
                        ui.label(checkin_notes);
                    });
            }
        });

        ui.add_space(8.0);

        // Fill nearly all remaining vertical space with the sessions list.
        let list_h = ui.available_height() * 0.99;
        ui.allocate_ui_with_layout(
            Vec2::new(right_w, list_h),
            eframe::egui::Layout::top_down(eframe::egui::Align::LEFT),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Diagnostic Sessions ({})", sessions.len()))
                            .strong()
                            .size(14.0),
                    );
                    if loading {
                        Spinner::new().size(14.0).ui(ui);
                    }
                    if show_refresh
                        && ui
                            .small_button(format!("{} Refresh", icons::REFRESH))
                            .on_hover_text("Re-fetch sessions and entries")
                            .clicked()
                    {
                        refresh_clicked = true;
                    }
                });

                if let Some(err) = error {
                    ui.colored_label(theme::error(ui), err);
                }

                ui.separator();

                if sessions.is_empty() && !loading {
                    ui.colored_label(
                        theme::weak_text(ui),
                        "No diagnostics recorded for this task or computer yet.",
                    );
                    return;
                }

                ScrollArea::vertical()
                    .id_salt("diag_sessions_scroll")
                    .auto_shrink([false; 2])
                    .max_height(ui.available_height() * 0.99)
                    .show(ui, |ui| {
                        for (idx, view) in sessions.iter().enumerate() {
                            let is_selected = selected
                                .as_ref()
                                .is_some_and(|s| s == &view.session.id);
                            render_session(ui, view, idx, is_selected, |id| {
                                *selected = Some(id);
                            });
                            ui.add_space(6.0);
                        }
                    });
            },
        );
    });

    refresh_clicked
}

/// Pinned AI Handoff panel — one per ai_task on this task, above the
/// check-in notes. Same shared checklist widget as the AI task card.
fn render_ai_handoff_panel(
    ui: &mut Ui,
    task: &AiTask,
    items: &[AiTaskItem],
    ctx: &DiagnosticsPageAiCtx<'_>,
    selected: &mut Option<RecordId>,
) {
    let (chip_text, chip_color) = match task.status {
        AiTaskStatus::Open => ("• IN PROGRESS", theme::info(ui)),
        AiTaskStatus::AwaitingFollowup => ("• AWAITING OPERATOR", theme::warn(ui)),
        AiTaskStatus::Closed => ("• CLOSED", theme::weak_text(ui)),
    };
    let user_name = |id: &RecordId| -> String {
        ctx.store_users
            .iter()
            .find(|u| u.get_id() == *id)
            .map(|u| u.get_name().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    Frame::default()
        .fill(ui.style().visuals.faint_bg_color)
        .stroke(ui.style().visuals.window_stroke)
        .inner_margin(Margin::same(8))
        .corner_radius(ui.style().visuals.menu_corner_radius)
        .show(ui, |ui| {
            // Hard width bound: label wrapping must never dictate layout.
            ui.set_max_width(ui.available_width().clamp(320.0, 690.0));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} {}", icons::ROBOT, task.title)).strong(),
                );
                ui.label(RichText::new(chip_text).color(chip_color).strong().small());
            });
            ui.label(
                RichText::new(format!(
                    "{} {} {}",
                    user_name(&task.requested_by),
                    icons::ARROW_RIGHT,
                    user_name(&task.assignee)
                ))
                .weak()
                .small(),
            );
            let _ = ai_checklist_progress(ui, items);

            let interactive = task.status != AiTaskStatus::Closed
                && ctx.ui_actions_tx.is_some()
                && ctx
                    .current_user
                    .map(|u| u.get_id() == task.assignee || u.get_id() == task.requested_by)
                    .unwrap_or(false);
            if let Some(tx) = ctx.ui_actions_tx {
                // Collapsed by default and height-capped even when open, so a
                // long checklist can't push the diagnostic sessions list
                // below it out of view.
                CollapsingHeader::new(RichText::new("Checklist").small())
                    .id_salt(format!("ai_checklist_collapse_{}", task.id.key_string()))
                    .default_open(false)
                    .show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt(format!("ai_checklist_scroll_{}", task.id.key_string()))
                            .max_height(220.0)
                            .show(ui, |ui| {
                                display_ai_checklist(ui, task, items, ctx.store_users, interactive, tx);
                            });
                    });
            }

            if let (Some(tx), Some(user)) = (ctx.ui_actions_tx, ctx.current_user) {
                if can_close_ai_task(task, &user.get_id(), items) {
                    ui.separator();
                    if Button::new(icons::menu_item(icons::CHECK, "Accept & close"))
                        .small()
                        .ui(ui)
                        .on_hover_text(
                            "Close this AI task and record the outcome in the session log",
                        )
                        .clicked()
                    {
                        let _ = tx.try_send(TaskUiActions::CloseAiTask(task.id.clone()));
                    }
                }
            }

            if ui
                .link(RichText::new(format!("show linked session {}", icons::CARET_DOWN)).small())
                .clicked()
            {
                *selected = Some(task.session_ref.clone());
            }
        });
}

fn render_session(
    ui: &mut Ui,
    view: &DiagnosticSessionView,
    idx: usize,
    selected: bool,
    mut on_select: impl FnMut(RecordId),
) {
    let session = &view.session;
    let started = format_datetime(&session.started_at);
    let stale_days = session.stale_days();
    let header = format!(
        "{} • {} • {}{}",
        started,
        session.tech.as_deref().unwrap_or("(unknown tech)"),
        session.hostname,
        stale_days
            .map(|d| format!("  {} STALE — open {d}d", icons::STATUS_WARN))
            .unwrap_or_default(),
    );

    let status_color = match session.status.as_str() {
        "open" => Color32::from_rgb(42, 195, 222),
        "resolved" | "closed" => Color32::from_rgb(100, 200, 100),
        "escalated" => Color32::from_rgb(255, 200, 50),
        _ => Color32::GRAY,
    };

    let resp = CollapsingHeader::new(RichText::new(header).strong().size(13.0))
        .id_salt(format!("diag_session_{idx}_{}", session.id.key_string()))
        .default_open(idx == 0 || selected)
        .show(ui, |ui| {
            Grid::new(format!("diag_meta_grid_{idx}"))
                .num_columns(2)
                .striped(false)
                .show(ui, |ui| {
                    ui.label(RichText::new("Status").weak());
                    match stale_days {
                        Some(d) => ui.colored_label(
                            theme::warn(ui),
                            format!("{} — open {d} days, never closed", session.status),
                        ),
                        None => ui.colored_label(status_color, &session.status),
                    };
                    ui.end_row();
                    ui.label(RichText::new("Connection").weak());
                    ui.label(&session.connection_string);
                    ui.end_row();
                    if let Some(name) = session.customer_name.as_deref() {
                        ui.label(RichText::new("Customer").weak());
                        ui.label(name);
                        ui.end_row();
                    }
                    if let Some(end) = session.ended_at.as_ref() {
                        ui.label(RichText::new("Ended").weak());
                        ui.label(format_datetime(end));
                        ui.end_row();
                    }
                    if !session.tags.is_empty() {
                        ui.label(RichText::new("Tags").weak());
                        ScrollArea::horizontal()
                        .id_salt("diag_sessions_tags_scroll")
                        .auto_shrink([false, true])
                        .max_width(ui.available_width().max(500.))
                        .show(ui, |ui| {
                            ui.label(session.tags.join(", "));
                        });
                        ui.end_row();
                    }
                });

            if let Some(summary) = session.summary.as_deref() {
                if !summary.trim().is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Summary").strong());
                    ui.label(summary);
                }
            }

            ui.add_space(6.0);
            ui.label(RichText::new(format!("Entries ({})", view.entries.len())).strong());
            ui.separator();

            if view.entries.is_empty() {
                ui.colored_label(
                    theme::weak_text(ui),
                    "No entries recorded in this session.",
                );
            } else {
                for (eidx, entry) in view.entries.iter().enumerate() {
                    render_entry(ui, entry, eidx);
                    ui.add_space(2.0);
                }
            }
        });

    if resp.header_response.clicked() {
        on_select(session.id.clone());
    }
}

fn render_entry(ui: &mut Ui, entry: &DiagnosticEntry, idx: usize) {
    let cat_str = entry.category.as_str();
    let cat_color = match cat_str {
        "finding" => theme::warn(ui),
        "action" => theme::info(ui),
        "error" => theme::error(ui),
        "security_alert" => Color32::from_rgb(255, 80, 200),
        "performance_note" => Color32::from_rgb(200, 150, 255),
        "recommendation" => theme::success(ui),
        _ => Color32::GRAY,
    };

    ui.label(RichText::new(format_datetime(&entry.timestamp)).weak().small());
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(cat_color, format!("[{}]", cat_str));
        ui.label(RichText::new(&entry.title).strong());
    });

    if !entry.detail.trim().is_empty() {
        ui.label(&entry.detail);
    }

    if !entry.plugins_used.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Plugins:").weak().small());
            for usage in entry.plugins_used.iter() {
                ui.label(
                    RichText::new(format!("{}::{}", usage.plugin_id, usage.tool_name))
                        .small()
                        .color(theme::weak_text(ui)),
                );
            }
        });
    }

    if let Some(data) = entry.data.as_ref() {
        let pretty = serde_json::to_string_pretty(data)
            .unwrap_or_else(|_| data.to_string());
        CollapsingHeader::new(RichText::new("data").weak().small())
            .id_salt(format!("diag_entry_data_{idx}"))
            .default_open(false)
            .show(ui, |ui| {
                ui.code(pretty);
            });
    }
}

fn format_datetime(dt: &database::schema::Datetime) -> String {
    chrono::DateTime::<chrono::Utc>::from(*dt)
        .format("%m/%d/%Y %H:%M")
        .to_string()
}
