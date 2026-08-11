//! Root-only approval modal for SurrealQL mutations submitted over MCP.
//!
//! `query_surrealdb` stays read-only; anything that writes lands in the
//! `sql_approval` table as a pending row and blocks there until a Root
//! operator decides here. The requesting process polls the row and runs the
//! statement itself once the status reads `approved`.
//!
//! Gating lives in two places on purpose. This module refuses to render for
//! a non-Root user, and the live stream that feeds it is only spawned for
//! Root (see `ui_data::mod::load_data`), so a non-Root console never even
//! subscribes to the queue. Requests raised by other users still reach Root
//! through the table's CREATE event, which mints an `Approval` notification
//! for every Root account — the same path the company-wide `Admin`
//! notification uses.

use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::sql_approval::{ApprovalStatus, SqlApproval};
use database::schema::{RecordId, RecordIdExt};
use eframe::egui::{self, Align, Grid, Id, Layout, Modal, RichText, ScrollArea, TextEdit};

/// Outcome of a decision write, surfaced back on the UI thread.
pub enum DecisionResult {
    Done(RecordId),
    Failed(RecordId, String),
}

/// Pending queue plus the operator's in-flight denial note.
pub struct SqlApprovalQueue {
    pending: Vec<SqlApproval>,
    /// Denial reason being typed for the front request.
    deny_reason: String,
    /// Requests with a decision write in flight; buttons stay disabled.
    in_flight: Vec<RecordId>,
    tx: Sender<DecisionResult>,
    rx: Receiver<DecisionResult>,
    last_error: Option<String>,
}

impl Default for SqlApprovalQueue {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            pending: Vec::new(),
            deny_reason: String::new(),
            in_flight: Vec::new(),
            tx,
            rx,
            last_error: None,
        }
    }
}

impl SqlApprovalQueue {
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Replaces the queue with a fetched snapshot (initial fill / resync).
    pub fn set_pending(&mut self, rows: Vec<SqlApproval>) {
        self.pending = rows
            .into_iter()
            .filter(|r| r.status_enum() == ApprovalStatus::Pending)
            .collect();
        self.pending.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    }

    /// Folds one live-query event in. A row that has left `pending` — decided
    /// here, decided on another Root console, or expired — drops out.
    pub fn apply_update(&mut self, row: SqlApproval) {
        let id = row.id.clone();
        self.in_flight.retain(|p| p != &id);
        if row.status_enum() == ApprovalStatus::Pending {
            match self.pending.iter_mut().find(|r| r.id == id) {
                Some(existing) => *existing = row,
                None => {
                    self.pending.push(row);
                    self.pending.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                }
            }
        } else {
            self.remove(&id);
        }
    }

    pub fn apply_delete(&mut self, row: SqlApproval) {
        self.remove(&row.id);
    }

    fn remove(&mut self, id: &RecordId) {
        if self.pending.first().map(|r| &r.id) == Some(id) {
            self.deny_reason.clear();
        }
        self.pending.retain(|r| &r.id != id);
        self.in_flight.retain(|p| p != id);
    }

    /// Drains decision results so a failed write re-enables the buttons
    /// instead of leaving the request stuck behind a spinner.
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            match result {
                DecisionResult::Done(id) => {
                    self.remove(&id);
                }
                DecisionResult::Failed(id, err) => {
                    self.in_flight.retain(|p| p != &id);
                    self.last_error = Some(err);
                }
            }
        }
    }

    fn decide(&mut self, id: RecordId, approved: bool, deny_reason: Option<String>) {
        self.in_flight.push(id.clone());
        self.last_error = None;
        let tx = self.tx.clone();
        let decided_by = crate::get_current_user_from_auth().map(|u| u.get_id());
        PlatformSpawner::spawn(async move {
            let outcome = SqlApproval::decide(&id, approved, decided_by, deny_reason).await;
            let msg = match outcome {
                Ok(()) => DecisionResult::Done(id),
                Err(e) => DecisionResult::Failed(id, e.to_string()),
            };
            let _ = tx.try_send(msg);
        });
    }

    /// Renders the front request. No-op for a non-Root user or an empty queue.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if self.pending.is_empty() || !super::current_user_is_root() {
            return;
        }

        // Expiry is enforced on read as well as by the sweeper, so a modal
        // left open overnight cannot be approved into a stale write.
        let expired: Vec<RecordId> = self
            .pending
            .iter()
            .filter(|r| r.expires_at.is_some() && r.secs_remaining() == 0)
            .map(|r| r.id.clone())
            .collect();
        for id in expired {
            self.remove(&id);
        }
        let Some(req) = self.pending.first().cloned() else {
            return;
        };

        let queued = self.pending.len();
        let busy = self.in_flight.contains(&req.id);
        let destructive = req.kind_is_destructive();
        let mut approve = false;
        let mut deny = false;

        Modal::new(Id::new("sql_approval_modal")).show(ctx, |ui| {
            ui.set_width(680.0);

            ui.horizontal(|ui| {
                ui.heading(format!("{} SurrealQL Approval", icons::LOCK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if queued > 1 {
                        ui.label(
                            RichText::new(format!("{} queued", queued))
                                .color(theme::warn(ui))
                                .strong(),
                        );
                    }
                });
            });
            ui.separator();

            if destructive {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{} ", icons::STATUS_WARN))
                            .color(theme::error(ui))
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} can destroy or overwrite data.",
                            req.statement_kind.to_uppercase()
                        ))
                        .color(theme::error(ui))
                        .strong(),
                    );
                });
                ui.add_space(4.);
            }

            Grid::new("sql_approval_meta")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Requested by").strong());
                    ui.label(if req.requested_label.is_empty() {
                        "unknown".to_string()
                    } else {
                        req.requested_label.clone()
                    });
                    ui.end_row();

                    if !req.origin_host.is_empty() {
                        ui.label(RichText::new("Origin").strong());
                        ui.label(&req.origin_host);
                        ui.end_row();
                    }

                    ui.label(RichText::new("Statement").strong());
                    ui.label(req.statement_kind.to_uppercase());
                    ui.end_row();

                    if let Some(table) = req.target_table.as_ref() {
                        ui.label(RichText::new("Target table").strong());
                        ui.label(table);
                        ui.end_row();
                    }

                    ui.label(RichText::new("Impact").strong());
                    match req.impact_rows {
                        Some(n) => {
                            let color = if n == 0 {
                                theme::warn(ui)
                            } else if destructive && n > 1 {
                                theme::error(ui)
                            } else {
                                theme::info(ui)
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{} row{} match this statement",
                                    n,
                                    if n == 1 { "" } else { "s" }
                                ))
                                .color(color)
                                .strong(),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new(
                                    req.impact_note
                                        .clone()
                                        .unwrap_or_else(|| "preview unavailable".to_string()),
                                )
                                .color(theme::warn(ui)),
                            );
                        }
                    }
                    ui.end_row();

                    ui.label(RichText::new("Expires in").strong());
                    let secs = req.secs_remaining();
                    ui.label(
                        RichText::new(format!("{}m {:02}s", secs / 60, secs % 60)).color(
                            if secs < 60 {
                                theme::error(ui)
                            } else {
                                ui.visuals().weak_text_color()
                            },
                        ),
                    );
                    ui.end_row();
                });

            ui.add_space(6.);
            ui.label(RichText::new("Reason given").strong());
            ui.label(&req.reason);

            ui.add_space(6.);
            ui.label(RichText::new("Statement").strong());
            ScrollArea::vertical()
                .max_height(180.0)
                .id_salt("sql_approval_stmt")
                .show(ui, |ui| {
                    let mut stmt = req.statement.clone();
                    // Read-only TextEdit: monospace, selectable, copyable.
                    ui.add(
                        TextEdit::multiline(&mut stmt)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });

            if req.impact_rows.is_some() {
                if let Some(note) = req.impact_note.as_ref() {
                    ui.add_space(2.);
                    ui.label(
                        RichText::new(note)
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                }
            }

            ui.add_space(8.);
            ui.label(RichText::new("Denial note (optional)").strong());
            ui.add(
                TextEdit::singleline(&mut self.deny_reason)
                    .desired_width(f32::INFINITY)
                    .hint_text("Sent back to the requester when you deny"),
            );

            if let Some(err) = self.last_error.as_ref() {
                ui.add_space(4.);
                ui.label(
                    RichText::new(format!("{} {err}", icons::STATUS_ERR)).color(theme::error(ui)),
                );
            }

            ui.add_space(10.);
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if ui
                        .button(
                            RichText::new(format!("{} Deny", icons::STATUS_ERR))
                                .color(theme::error(ui)),
                        )
                        .clicked()
                    {
                        deny = true;
                    }
                    ui.add_space(6.);
                    if ui
                        .button(
                            RichText::new(format!("{} Approve & Run", icons::STATUS_ON))
                                .color(theme::success(ui)),
                        )
                        .clicked()
                    {
                        approve = true;
                    }
                });
                if busy {
                    ui.add_space(8.);
                    ui.spinner();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(req.id.key_string())
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                });
            });
        });

        if approve {
            self.decide(req.id.clone(), true, None);
        } else if deny {
            let note = if self.deny_reason.trim().is_empty() {
                None
            } else {
                Some(self.deny_reason.trim().to_string())
            };
            self.deny_reason.clear();
            self.decide(req.id.clone(), false, note);
        }

        // Countdown has to keep ticking while the modal sits idle.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}
