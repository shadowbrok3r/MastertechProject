//! Diagnostics tab — shows every `DiagnosticSession` linked to this task or
//! to the same computer the ticket references, side-by-side with the
//! customer's check-in notes so the tech can compare reported symptoms
//! against AI-recorded findings.

use database::schema::{DiagnosticEntry, DiagnosticSession, RecordId, RecordIdExt};
use eframe::egui::{
    CollapsingHeader, Color32, Grid, RichText, ScrollArea, Spinner, Ui, Vec2, Widget,
};
use crate::ui_tools::theme;
use serde::{Deserialize, Serialize};

/// One session paired with all of its entries. Built by
/// `TaskModal::kickoff_diagnostics_load` from `DiagnosticSession::get_full`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticSessionView {
    pub session: DiagnosticSession,
    pub entries: Vec<DiagnosticEntry>,
}

/// Render the diagnostics page. Two columns: left = check-in notes from
/// the ticket, right = scrollable list of diagnostic sessions with their
/// entries inline.
pub fn display_diagnostics_page(
    ui: &mut Ui,
    avail_size: Vec2,
    sessions: &[DiagnosticSessionView],
    loading: bool,
    error: Option<&str>,
    selected: &mut Option<RecordId>,
    checkin_notes: &str,
) {
    let total_w = avail_size.x.max(700.0);
    let left_w = (total_w * 0.32).clamp(220.0, 320.0);
    let right_w = (total_w - left_w - 12.0).max(380.0);
    // Cap the check-in notes so a long expanded note can't grow the modal;
    // the sessions list below fills whatever vertical space remains.
    let notes_h = 120.0;

    ui.vertical_centered_justified(|ui| {
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
    let header = format!(
        "{} • {} • {}",
        started,
        session.tech.as_deref().unwrap_or("(unknown tech)"),
        session.hostname,
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
                    ui.colored_label(status_color, &session.status);
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
