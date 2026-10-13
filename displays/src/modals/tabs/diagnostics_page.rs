//! Diagnostics tab — shows every `DiagnosticSession` linked to this task or
//! to the same computer the ticket references, side-by-side with the
//! customer's check-in notes so the tech can compare reported symptoms
//! against AI-recorded findings.

use database::schema::{AiTask, AiTaskItem, AiTaskStatus, DiagnosticEntry, DiagnosticSession, RecordId, RecordIdExt, User};
use eframe::egui::{
    Align, Color32, Grid, Layout, RichText, ScrollArea, Ui, Vec2,
};
use crate::modals::tabs::ai_checklist_panel::{
    ai_checklist_progress, can_close_ai_task, display_ai_checklist,
};
use crate::ui_tools::{
    controls::{
        self, BarKind, ButtonKind, Icon, IconLabel, Metrics, Section, SpinnerSize, TextButton,
    },
    glass_card,
    icons::{self, svg},
    theme,
};
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

/// A [`Section`] with its icon tinted `tone`; returns whether the header was clicked and the body's result.
pub(crate) fn status_section<R>(
    ui: &mut Ui,
    salt: &str,
    icon: Icon,
    tone: Color32,
    title: &str,
    default_open: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> (bool, Option<R>) {
    let (header, body) = Section::new(salt, title)
        .icon(icon)
        .icon_tone(tone)
        .default_open(default_open)
        .show_with_header(ui, add);
    (header.clicked(), body)
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
    let m = Metrics::get(ui.ctx());

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
                ui.add_space(m.gap.y);
            }
        }

        Section::new("Check-in Notes", "Check-in Notes")
            .icon(svg::NOTE)
            .default_open(false)
            .show(ui, |ui| {
                if checkin_notes.trim().is_empty() {
                    controls::icon_label(ui, svg::INFO, "No check-in notes recorded.");
                } else {
                    ScrollArea::vertical()
                        .id_salt("diag_checkin_scroll")
                        .max_height(notes_h)
                        .show(ui, |ui| {
                            ui.label(checkin_notes);
                        });
                }
            });

        ui.add_space(m.gap.y);

        // Fill nearly all remaining vertical space with the sessions list.
        let list_h = ui.available_height() * 0.99;
        ui.allocate_ui_with_layout(
            Vec2::new(right_w, list_h),
            Layout::top_down(Align::LEFT),
            |ui| {
                controls::bar(ui, BarKind::Header, |ui| {
                    controls::icon_label(
                        ui,
                        svg::MICROSCOPE,
                        &format!("Diagnostic Sessions ({})", sessions.len()),
                    );
                    if loading {
                        controls::spinner(ui, SpinnerSize::Sm);
                    }
                    if show_refresh {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if controls::icon_text_button(ui, svg::ARROW_CLOCKWISE, "Refresh")
                                .on_hover_text("Re-fetch sessions and entries")
                                .clicked()
                            {
                                refresh_clicked = true;
                            }
                        });
                    }
                });

                if let Some(err) = error {
                    controls::status_label(ui, svg::WARNING_CIRCLE, err, theme::error(ui));
                }

                if sessions.is_empty() && !loading {
                    controls::empty_state(
                        ui,
                        svg::MICROSCOPE,
                        "No diagnostics yet",
                        "No diagnostics recorded for this task or computer yet.",
                        &[],
                    );
                    return;
                }

                ui.add_space(m.pad.y);
                let mark_ctx: Option<(&Sender<TaskUiActions>, String)> =
                    ai_ctx.as_ref().and_then(|ai| {
                        ai.ui_actions_tx.map(|tx| {
                            (
                                tx,
                                ai.current_user
                                    .map(|u| u.get_name().to_string())
                                    .unwrap_or_else(|| "operator".to_string()),
                            )
                        })
                    });
                ScrollArea::vertical()
                    .id_salt("diag_sessions_scroll")
                    .auto_shrink([false; 2])
                    .max_height(ui.available_height() * 0.99)
                    .show(ui, |ui| {
                        for (idx, view) in sessions.iter().enumerate() {
                            let is_selected = selected
                                .as_ref()
                                .is_some_and(|s| s == &view.session.id);
                            render_session(ui, view, idx, is_selected, mark_ctx.as_ref(), |id| {
                                *selected = Some(id);
                            });
                            ui.add_space(m.gap.y);
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
        AiTaskStatus::Open => ("IN PROGRESS", theme::info(ui)),
        AiTaskStatus::AwaitingFollowup => ("AWAITING OPERATOR", theme::warn(ui)),
        AiTaskStatus::Closed => ("CLOSED", theme::weak_text(ui)),
    };
    let user_name = |id: &RecordId| -> String {
        ctx.store_users
            .iter()
            .find(|u| u.get_id() == *id)
            .map(|u| u.get_name().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    glass_card::group(ui, |ui| {
        // Hard width bound: label wrapping must never dictate layout.
        ui.set_max_width(ui.available_width().clamp(320.0, 690.0));
        ui.horizontal(|ui| {
            controls::icon_label(ui, svg::ROBOT, &task.title);
            controls::badge(ui, chip_text, chip_color);
        });
        let handoff = format!(
            "{} {} {}",
            user_name(&task.requested_by),
            icons::ARROW_RIGHT,
            user_name(&task.assignee)
        );
        IconLabel::new(svg::USERS, &handoff)
            .small(true)
            .tone(theme::weak_text(ui))
            .show(ui);
        if let Some(theory) = task
            .current_theory
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            theory_label(ui, theory);
            if let Some(next) = task
                .theory_next_step
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                ui.label(RichText::new(next).weak().small());
            }
        }
        let _ = ai_checklist_progress(ui, items);

        let interactive = task.status != AiTaskStatus::Closed
            && ctx.ui_actions_tx.is_some()
            && ctx
                .current_user
                .map(|u| u.get_id() == task.assignee || u.get_id() == task.requested_by)
                .unwrap_or(false);
        if let Some(tx) = ctx.ui_actions_tx {
            Section::new(format!("ai_checklist_collapse_{}", task.id.key_string()), "Checklist")
                .icon(svg::LIST_CHECKS)
                .default_open(false)
                .show(ui, |ui| {
                    display_ai_checklist(ui, task, items, ctx.store_users, interactive, tx);
                });
        }

        if let (Some(tx), Some(user)) = (ctx.ui_actions_tx, ctx.current_user)
            && can_close_ai_task(task, &user.get_id(), items)
        {
            controls::hairline(ui);
            if controls::icon_text_button(ui, svg::CHECK_CIRCLE, "Accept & close")
                .on_hover_text("Close this AI task and record the outcome in the session log")
                .clicked()
            {
                let _ = tx.try_send(TaskUiActions::CloseAiTask(task.id.clone()));
            }
        }

        if TextButton::new("Show linked session")
            .kind(ButtonKind::Ghost)
            .icon(svg::CARET_DOWN)
            .show(ui)
            .clicked()
        {
            *selected = Some(task.session_ref.clone());
        }
    });
}

pub(crate) fn render_session(
    ui: &mut Ui,
    view: &DiagnosticSessionView,
    idx: usize,
    selected: bool,
    mark_ctx: Option<&(&Sender<TaskUiActions>, String)>,
    mut on_select: impl FnMut(RecordId),
) {
    let m = Metrics::get(ui.ctx());
    let session = &view.session;
    let started = format_datetime(&session.started_at);
    let stale_days = session.stale_days();
    let theory = session
        .current_theory
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    // Truncated into the header so the verdict reads without expanding.
    let header = format!(
        "{} • {} • {}{}{}",
        started,
        session.ran_by(),
        session.hostname,
        stale_days
            .map(|d| format!("  {} STALE — open {d}d", icons::STATUS_WARN))
            .unwrap_or_default(),
        theory
            .map(|t| {
                let short: String = t.chars().take(70).collect();
                let cut = if t.chars().count() > 70 { "…" } else { "" };
                format!("   {} {short}{cut}", icons::LIGHTBULB)
            })
            .unwrap_or_default(),
    );

    let (status_icon, status_color) = match session.status.as_str() {
        "open" => (svg::PULSE, theme::info(ui)),
        "resolved" | "closed" => (svg::CHECK_CIRCLE, theme::success(ui)),
        "escalated" => (svg::WARNING, theme::warn(ui)),
        _ => (svg::CIRCLE, theme::weak_text(ui)),
    };

    let salt = format!("diag_session_{idx}_{}", session.id.key_string());
    let (clicked, _) = status_section(
        ui,
        &salt,
        status_icon,
        status_color,
        &header,
        idx == 0 || selected,
        |ui| {
            if let Some(t) = theory {
                theory_label(ui, t);
                if let Some(next) = session
                    .theory_next_step
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    ui.label(RichText::new(format!("Next: {next}")).weak());
                }
                let meta = match (
                    session.theory_confidence.as_deref(),
                    session.theory_updated_at.as_ref(),
                ) {
                    (Some(c), Some(at)) => {
                        Some(format!("{c} confidence • updated {}", format_datetime(at)))
                    }
                    (Some(c), None) => Some(format!("{c} confidence")),
                    (None, Some(at)) => Some(format!("updated {}", format_datetime(at))),
                    (None, None) => None,
                };
                if let Some(line) = meta {
                    ui.label(RichText::new(line).weak().small());
                }
                controls::hairline(ui);
            }
            Grid::new(format!("diag_meta_grid_{idx}"))
                .num_columns(2)
                .striped(false)
                .show(ui, |ui| {
                    ui.label(RichText::new("Status").weak());
                    match stale_days {
                        Some(d) => {
                            let text = format!("{} — open {d} days, never closed", session.status);
                            controls::status_label(ui, svg::WARNING, &text, theme::warn(ui))
                        }
                        None => {
                            controls::status_label(ui, status_icon, &session.status, status_color)
                        }
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
                    if let Some(diag) = session.diagnosed_at.as_ref() {
                        ui.label(RichText::new("Diagnosed").weak());
                        ui.label(format!(
                            "{} by {}",
                            format_datetime(diag),
                            session.diagnosed_by.as_deref().unwrap_or("unknown"),
                        ));
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
                            IconLabel::new(svg::TAG, &session.tags.join(", ")).show(ui);
                        });
                        ui.end_row();
                    }
                });

            if session.diagnosed_at.is_none() {
                if let Some((tx, by)) = mark_ctx {
                    if controls::icon_text_button(ui, svg::STETHOSCOPE, "Mark diagnosed")
                        .on_hover_text(
                            "Stamp the diagnosis-complete milestone now; remediation can \
                             continue and the session stays open",
                        )
                        .clicked()
                    {
                        let _ = tx.try_send(TaskUiActions::MarkDiagnosed {
                            session_id: session.id.clone(),
                            by: by.clone(),
                        });
                    }
                }
            }

            if let Some(summary) = session.summary.as_deref() {
                if !summary.trim().is_empty() {
                    ui.add_space(m.pad.y);
                    controls::icon_label(ui, svg::CLIPBOARD_TEXT, "Summary");
                    ui.label(summary);
                }
            }

            ui.add_space(m.gap.y);
            controls::icon_label(
                ui,
                svg::LIST_BULLETS,
                &format!("Entries ({})", view.entries.len()),
            );
            controls::hairline(ui);

            if view.entries.is_empty() {
                controls::icon_label(ui, svg::INFO, "No entries recorded in this session.");
            } else {
                for (eidx, entry) in view.entries.iter().enumerate() {
                    render_entry(ui, entry, eidx);
                    ui.add_space(m.pad.y / 2.0);
                }
            }
        },
    );

    if clicked {
        on_select(session.id.clone());
    }
}

/// A wrapped semibold theory line after a lightbulb.
fn theory_label(ui: &mut Ui, theory: &str) {
    IconLabel::new(svg::LIGHTBULB, theory)
        .strong(true)
        .icon_tone(theme::warn(ui))
        .wrap(true)
        .show(ui);
}

fn render_entry(ui: &mut Ui, entry: &DiagnosticEntry, idx: usize) {
    let cat_str = entry.category.as_str();
    let (cat_icon, cat_color) = match cat_str {
        "finding" => (svg::MAGNIFYING_GLASS, theme::warn(ui)),
        "action" => (svg::WRENCH, theme::info(ui)),
        "error" => (svg::X_CIRCLE, theme::error(ui)),
        "security_alert" => (svg::SHIELD_WARNING, theme::primary(ui)),
        "performance_note" => (svg::GAUGE, theme::secondary(ui)),
        "recommendation" => (svg::LIGHTBULB, theme::success(ui)),
        _ => (svg::DOT, theme::weak_text(ui)),
    };

    IconLabel::new(svg::CLOCK, &format_datetime(&entry.timestamp))
        .small(true)
        .tone(theme::weak_text(ui))
        .show(ui);
    ui.horizontal_wrapped(|ui| {
        IconLabel::new(cat_icon, &format!("[{cat_str}]"))
            .tone(cat_color)
            .show(ui);
        ui.label(RichText::new(&entry.title).strong());
    });

    if !entry.detail.trim().is_empty() {
        ui.label(&entry.detail);
    }

    if !entry.plugins_used.is_empty() {
        ui.horizontal_wrapped(|ui| {
            IconLabel::new(svg::PUZZLE_PIECE, "Plugins:")
                .small(true)
                .tone(theme::weak_text(ui))
                .show(ui);
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
        Section::new(format!("diag_entry_data_{idx}"), "data")
            .icon(svg::BRACKETS_CURLY)
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
