//! Reads the `zeroclaw_audit` trail written by ZeroClaw's webhook-audit hook.
//!
//! Rows arrive out-of-band: the hook POSTs each matching tool call to the axum
//! orchestrator's `/api/v1/audit/zeroclaw`, which appends to an append-only
//! table. Nothing is fetched until the tab is drawn.

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{Align, Color32, DragValue, Layout, RichText, ScrollArea, TextEdit, Ui};
use egui_extras::{Column, TableBuilder};
use web_time::Instant;

use crate::ui_tools::hex_json;
use crate::{PlatformSpawner, Spawner};

const ROW_HEIGHT: f32 = 20.0;
const HEADER_HEIGHT: f32 = 22.0;
const AUTO_REFRESH_SECS: u64 = 15;
/// Probe beats every 5 min; three missed beats reads as down.
const HEALTH_STALE_SECS: i64 = 900;

#[derive(Debug, Clone, Default)]
pub struct AuditRow {
    pub id: String,
    pub created_at: String,
    pub tool: String,
    pub success: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub args: Option<serde_json::Value>,
}

impl AuditRow {
    fn from_value(v: &serde_json::Value) -> Self {
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        Self {
            id: v
                .get("id")
                .map(|i| i.as_str().map(str::to_string).unwrap_or_else(|| i.to_string()))
                .unwrap_or_default(),
            created_at: text("created_at"),
            tool: text("tool"),
            success: v.get("success").and_then(serde_json::Value::as_bool).unwrap_or(false),
            duration_ms: v.get("duration_ms").and_then(serde_json::Value::as_u64).unwrap_or(0),
            error: v
                .get("error")
                .and_then(|e| e.as_str())
                .filter(|e| !e.trim().is_empty())
                .map(str::to_string),
            args: v.get("args").filter(|a| !a.is_null()).cloned(),
        }
    }

    /// Clock portion of the timestamp; the date is rarely useful in a live feed.
    fn clock(&self) -> &str {
        self.created_at
            .split_once('T')
            .map(|(_, t)| &t[..t.len().min(8)])
            .unwrap_or(&self.created_at)
    }
}

/// Latest `zeroclaw_health` heartbeat; a stale one reads as down.
#[derive(Debug, Clone, Default)]
struct HealthRow {
    status: String,
    detail: Option<String>,
    cron_errors: u64,
    age_secs: i64,
}

impl HealthRow {
    fn from_value(v: &serde_json::Value) -> Self {
        Self {
            status: v.get("status").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            detail: v
                .get("detail")
                .and_then(|d| d.as_str())
                .filter(|d| !d.trim().is_empty())
                .map(str::to_string),
            cron_errors: v.get("cron_errors").and_then(serde_json::Value::as_u64).unwrap_or(0),
            age_secs: v.get("age_secs").and_then(serde_json::Value::as_i64).unwrap_or(i64::MAX),
        }
    }
}

pub struct AgentAudit {
    rows: Vec<AuditRow>,
    health: Option<HealthRow>,
    limit: usize,
    filter_tool: String,
    failures_only: bool,
    selected: Option<String>,
    auto_refresh: bool,
    last_poll: Option<Instant>,
    loading: bool,
    status: String,
    tx: Sender<Result<(Option<HealthRow>, Vec<AuditRow>), String>>,
    rx: Receiver<Result<(Option<HealthRow>, Vec<AuditRow>), String>>,
}

impl Default for AgentAudit {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            rows: Vec::new(),
            health: None,
            limit: 200,
            filter_tool: String::new(),
            failures_only: false,
            selected: None,
            auto_refresh: false,
            last_poll: None,
            loading: false,
            status: String::new(),
            tx,
            rx,
        }
    }
}

/// Newest audit rows plus the health heartbeat, one round trip.
async fn fetch(
    limit: usize,
    tool: String,
    failures_only: bool,
) -> Result<(Option<HealthRow>, Vec<AuditRow>), String> {
    let mut clauses: Vec<&str> = Vec::new();
    if failures_only {
        clauses.push("success == false");
    }
    if !tool.is_empty() {
        clauses.push("tool != NONE AND string::contains(tool, $tool)");
    }
    let where_sql =
        if clauses.is_empty() { String::new() } else { format!("WHERE {} ", clauses.join(" AND ")) };
    let sql = format!(
        "SELECT * FROM zeroclaw_audit {where_sql}ORDER BY created_at DESC LIMIT {limit}"
    );

    let mut res = database::db()
        .query(sql)
        // Age computed server-side so client clock skew never colors the strip.
        .query(
            "SELECT status, detail, cron_errors, \
             time::unix(time::now()) - time::unix(checked_at) AS age_secs FROM zeroclaw_health",
        )
        .bind(("tool", tool))
        .await
        .map_err(|e| e.to_string())?;
    let rows: Vec<serde_json::Value> = res.take(0).map_err(|e| e.to_string())?;
    let health = res
        .take::<Vec<serde_json::Value>>(1)
        .ok()
        .and_then(|h| h.first().map(HealthRow::from_value));
    Ok((health, rows.iter().map(AuditRow::from_value).collect()))
}

impl AgentAudit {
    fn refresh(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.last_poll = Some(Instant::now());
        let (tx, limit) = (self.tx.clone(), self.limit);
        let (tool, failures_only) = (self.filter_tool.trim().to_string(), self.failures_only);
        PlatformSpawner::spawn(async move {
            let _ = tx.send(fetch(limit, tool, failures_only).await);
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.loading = false;
            match msg {
                Ok((health, rows)) => {
                    self.status = format!("{} events", rows.len());
                    self.health = health;
                    self.rows = rows;
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.drain();
        // First draw is the first fetch, so a closed tab costs nothing.
        if self.last_poll.is_none() {
            self.refresh();
        }
        if self.auto_refresh
            && !self.loading
            && self.last_poll.is_some_and(|t| t.elapsed() >= Duration::from_secs(AUTO_REFRESH_SECS))
        {
            self.refresh();
        }
        if self.loading || self.auto_refresh {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        match &self.health {
            None => {
                ui.label(RichText::new("zeroclaw health: no probe data").weak());
            }
            Some(h) if h.status == "ok" && h.age_secs <= HEALTH_STALE_SECS => {
                ui.label(
                    RichText::new(format!("zeroclaw ok · {}s ago", h.age_secs))
                        .color(Color32::from_rgb(120, 200, 140)),
                );
            }
            Some(h) => {
                let text = if h.age_secs == i64::MAX {
                    "zeroclaw STALE: heartbeat unreadable".to_string()
                } else if h.age_secs > HEALTH_STALE_SECS {
                    format!("zeroclaw STALE: no heartbeat for {}m", h.age_secs / 60)
                } else {
                    format!(
                        "zeroclaw DOWN: {} cron error(s){}",
                        h.cron_errors,
                        h.detail.as_deref().map(|d| format!(" — {d}")).unwrap_or_default()
                    )
                };
                ui.label(RichText::new(text).color(Color32::from_rgb(220, 120, 120)));
            }
        }
        ui.separator();

        let mut requery = false;
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                requery = true;
            }
            ui.checkbox(&mut self.auto_refresh, "Auto");
            if ui.checkbox(&mut self.failures_only, "Failures only").changed() {
                requery = true;
            }
            ui.label("Tool:");
            if ui
                .add(TextEdit::singleline(&mut self.filter_tool).desired_width(180.0))
                .lost_focus()
            {
                requery = true;
            }
            ui.label("Limit:");
            if ui.add(DragValue::new(&mut self.limit).range(20..=2000).speed(10)).changed() {
                requery = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if self.loading { "loading...".to_string() } else { self.status.clone() };
                ui.label(RichText::new(text).weak());
            });
        });
        if requery {
            self.refresh();
        }
        ui.separator();

        if self.rows.is_empty() && !self.loading {
            ui.label(
                RichText::new(
                    "No audit events. Enable hooks.builtin.webhook_audit on the agent host and \
                     point its url at /api/v1/audit/zeroclaw.",
                )
                .weak(),
            );
            return;
        }

        let detail = self
            .selected
            .as_ref()
            .and_then(|id| self.rows.iter().find(|r| &r.id == id))
            .cloned();
        let body_h = if detail.is_some() { ui.available_height() * 0.55 } else { ui.available_height() };

        let mut clicked: Option<String> = None;
        ScrollArea::horizontal().id_salt("agent_audit_table").show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .max_scroll_height(body_h)
                .column(Column::exact(70.0))
                .column(Column::initial(240.0).at_least(120.0))
                .column(Column::exact(60.0))
                .column(Column::exact(70.0))
                .column(Column::remainder().at_least(120.0))
                .header(HEADER_HEIGHT, |mut h| {
                    for label in ["Time", "Tool", "Result", "ms", "Error"] {
                        h.col(|ui| {
                            ui.label(RichText::new(label).strong());
                        });
                    }
                })
                .body(|body| {
                    body.rows(ROW_HEIGHT, self.rows.len(), |mut row| {
                        let r = &self.rows[row.index()];
                        row.set_selected(self.selected.as_deref() == Some(r.id.as_str()));
                        row.col(|ui| {
                            ui.label(RichText::new(r.clock()).weak());
                        });
                        row.col(|ui| {
                            ui.label(&r.tool);
                        });
                        row.col(|ui| {
                            let (text, color) = if r.success {
                                ("ok", Color32::from_rgb(120, 200, 140))
                            } else {
                                ("failed", Color32::from_rgb(220, 120, 120))
                            };
                            ui.label(RichText::new(text).color(color));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(r.duration_ms.to_string()).weak());
                        });
                        row.col(|ui| {
                            ui.label(
                                RichText::new(r.error.as_deref().unwrap_or(""))
                                    .color(Color32::from_rgb(220, 120, 120)),
                            );
                        });
                        if row.response().clicked() {
                            clicked = Some(r.id.clone());
                        }
                    });
                });
        });

        if let Some(id) = clicked {
            self.selected = if self.selected.as_deref() == Some(id.as_str()) { None } else { Some(id) };
        }

        if let Some(r) = detail {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&r.tool).strong());
                ui.label(RichText::new(format!("· {}", r.created_at)).weak());
                ui.label(RichText::new(format!("· {}ms", r.duration_ms)).weak());
                if let Some(e) = &r.error {
                    ui.label(RichText::new(format!("· {e}")).color(Color32::from_rgb(220, 120, 120)));
                }
            });
            match &r.args {
                Some(args) => {
                    ScrollArea::vertical()
                        .id_salt("agent_audit_args")
                        .show(ui, |ui| hex_json::json_tree(ui, "agent_audit_args_tree", args));
                }
                None => {
                    ui.label(
                        RichText::new("No args recorded (set include_args = true on the hook).")
                            .weak(),
                    );
                }
            }
        }
    }
}
