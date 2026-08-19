//! AI diagnostics ROI dashboard: outcome funnel, measured AI cost against the
//! technician pay band, turnaround by task origin, and the counters that bound
//! every one of those numbers.
//!
//! Turnaround is reported in open-store hours (10-19 local, closed Sunday) as
//! well as wall clock; comeback windows stay in calendar days because a
//! customer's machine fails on customer time.

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use database::schema::{
    OutcomeOverride, RoiSummary, SessionOutcomeRow, TurnaroundStats, TECH_RATE_HIGH,
    TECH_RATE_LOW,
};
use eframe::egui::{Align, Button, Layout, RichText, ScrollArea, TextEdit, Ui};
use egui_plot::{Bar, BarChart, Legend, Plot};
use web_time::Instant;

use crate::ui_tools::{icons, info_card, theme};
use crate::{PlatformSpawner, Spawner};

const LOOKBACKS: [u32; 4] = [30, 60, 90, 180];
const WINDOWS: [u32; 3] = [30, 60, 90];
const OPEN_HOURS_PER_DAY: f64 = 9.0;
/// Sessions offered for correction, newest first; older ones nobody remembers.
const CORRECTABLE_LIMIT: usize = 20;

pub struct AiAnalytics {
    summary: Option<RoiSummary>,
    lookback_days: u32,
    window_days: u32,
    loading: bool,
    status: String,
    last_poll: Option<Instant>,
    tx: Sender<Result<RoiSummary, String>>,
    rx: Receiver<Result<RoiSummary, String>>,
    /// Session id whose correction editor is open.
    editing: Option<String>,
    draft_verdict: Option<OutcomeOverride>,
    draft_reason: String,
    write_busy: bool,
    write_status: String,
    write_tx: Sender<Result<String, String>>,
    write_rx: Receiver<Result<String, String>>,
}

impl Default for AiAnalytics {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        let (write_tx, write_rx) = crossbeam::channel::unbounded();
        Self {
            summary: None,
            lookback_days: 90,
            window_days: 30,
            loading: false,
            status: String::new(),
            last_poll: None,
            tx,
            rx,
            editing: None,
            draft_verdict: None,
            draft_reason: String::new(),
            write_busy: false,
            write_status: String::new(),
            write_tx,
            write_rx,
        }
    }
}

/// Elapsed time against the rollup's own clock, so the UI needs none.
fn ago(now: i64, then: i64) -> String {
    let secs = (now - then).max(0);
    if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

fn verdict_at(row: &SessionOutcomeRow, window: u32) -> &str {
    row.per_window
        .iter()
        .find(|(w, _)| *w == window)
        .map(|(_, v)| v.as_str())
        .unwrap_or("indeterminate")
}

fn hours(secs: i64) -> String {
    format!("{:.1}h", secs as f64 / 3600.0)
}

fn open_days(secs: i64) -> String {
    format!("{:.1} open days", secs as f64 / 3600.0 / OPEN_HOURS_PER_DAY)
}

fn pct(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".into();
    }
    format!("{:.0}%", 100.0 * part as f64 / whole as f64)
}

impl AiAnalytics {
    fn refresh(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.last_poll = Some(Instant::now());
        let (tx, days) = (self.tx.clone(), self.lookback_days);
        PlatformSpawner::spawn(async move {
            let _ = tx.send(RoiSummary::compute(days).await.map_err(|e| e.to_string()));
        });
    }

    /// Runs a write off the UI thread and reports it on the write channel.
    fn spawn_write(
        &mut self,
        done: String,
        fut: impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        self.write_busy = true;
        self.write_status = "writing...".to_string();
        let tx = self.write_tx.clone();
        PlatformSpawner::spawn(async move {
            let _ = tx.send(fut.await.map(|()| done).map_err(|e| e.to_string()));
        });
    }

    fn operator() -> String {
        crate::get_current_user_from_auth()
            .map(|u| u.get_email().to_string())
            .filter(|e| !e.trim().is_empty())
            .unwrap_or_else(|| "unknown operator".to_string())
    }

    fn apply_override(&mut self, row: &SessionOutcomeRow) {
        let Some(id) = database::schema::record_id_from_string(
            database::schema::DIAGNOSTIC_SESSION_TABLE,
            &row.session_id,
        ) else {
            self.write_status = format!("unreadable session id {}", row.session_id);
            return;
        };
        let verdict = self.draft_verdict;
        let reason = self.draft_reason.trim().to_string();
        let by = Self::operator();
        let done = match verdict {
            Some(v) => format!("{} recorded as {}", row.hostname, v.as_str()),
            None => format!("{} left to inference", row.hostname),
        };
        self.editing = None;
        self.draft_reason.clear();
        self.draft_verdict = None;
        self.spawn_write(done, async move {
            database::schema::set_outcome_override(&id, verdict, Some(&reason), &by).await
        });
    }

    fn clear_override(&mut self, row: &SessionOutcomeRow) {
        let Some(id) = database::schema::record_id_from_string(
            database::schema::DIAGNOSTIC_SESSION_TABLE,
            &row.session_id,
        ) else {
            self.write_status = format!("unreadable session id {}", row.session_id);
            return;
        };
        let done = format!("{} override cleared", row.hostname);
        self.editing = None;
        self.spawn_write(done, async move {
            database::schema::set_outcome_override(&id, None, None, "").await
        });
    }

    fn set_internal(&mut self, computer_key: &str, internal: bool) {
        let Some(id) = database::schema::record_id_from_string(
            database::schema::COMPUTER_TABLE,
            computer_key,
        ) else {
            self.write_status = format!("unreadable computer id {computer_key}");
            return;
        };
        let done = if internal {
            format!("{computer_key} flagged as a staff machine")
        } else {
            format!("{computer_key} is a customer machine again")
        };
        self.spawn_write(done, async move {
            database::schema::set_computer_internal(&id, internal).await
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.write_rx.try_recv() {
            self.write_busy = false;
            match msg {
                Ok(done) => {
                    self.write_status = done;
                    self.refresh();
                },
                Err(e) => self.write_status = format!("write failed: {e}"),
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            self.loading = false;
            match msg {
                Ok(summary) => {
                    self.status = format!(
                        "{} sessions scored, {} cost rows",
                        summary.outcome.sessions_considered, summary.ai_usage_rows
                    );
                    self.summary = Some(summary);
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
        if self.loading || self.write_busy {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        let mut requery = false;
        ui.horizontal(|ui| {
            if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
                requery = true;
            }
            ui.separator();
            ui.label("Lookback:");
            for days in LOOKBACKS {
                if ui.selectable_label(self.lookback_days == days, format!("{days}d")).clicked() {
                    self.lookback_days = days;
                    requery = true;
                }
            }
            ui.separator();
            ui.label("Comeback window:");
            for days in WINDOWS {
                if ui.selectable_label(self.window_days == days, format!("{days}d")).clicked() {
                    self.window_days = days;
                }
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

        let Some(summary) = self.summary.clone() else {
            ui.label(
                RichText::new(if self.loading {
                    "Computing fleet ROI..."
                } else {
                    "No data yet. Refresh to compute."
                })
                .weak(),
            );
            return;
        };

        ScrollArea::vertical().id_salt("ai_analytics_body").show(ui, |ui| {
            self.shelf_panel(ui, &summary);
            self.outcome_panel(ui, &summary);
            self.corrections_panel(ui, &summary);
            self.cost_panel(ui, &summary);
            self.turnaround_panel(ui, &summary);
            self.gaps_panel(ui, &summary);
        });
    }

    /// Triage verdicts from the twice-daily Check-in Shelf sweep.
    fn shelf_panel(&self, ui: &mut Ui, summary: &RoiSummary) {
        if summary.shelf_candidates.is_empty() {
            return;
        }
        let stale = summary
            .shelf_candidates
            .iter()
            .filter_map(|c| c.swept_age_secs)
            .min()
            .is_some_and(|age| age > 24 * 3600);
        let subtitle = if stale {
            "Last sweep was over a day ago - the shelf has moved since."
        } else {
            "Scored by the twice-daily sweep before anyone plugs the machine in. Score is a sort order for attention, not a measurement."
        };
        info_card::section_card(ui, icons::ROBOT, "Waiting on the shelf", Some(subtitle), |ui| {
            for c in &summary.shelf_candidates {
                ui.horizontal(|ui| {
                    let color = if c.score >= 50 {
                        theme::accent(ui)
                    } else if c.score >= 25 {
                        theme::warn(ui)
                    } else {
                        theme::weak_text(ui)
                    };
                    info_card::badge(ui, &format!("{}", c.score), color);
                    ui.label(RichText::new(format!("#{}", c.service_number)).strong());
                    if let Some(store) = &c.store {
                        ui.label(RichText::new(store).weak());
                    }
                    if let Some(h) = c.waiting_open_hours {
                        ui.label(RichText::new(format!("{h:.0}h open")).weak());
                    }
                });
                ui.label(RichText::new(&c.reason).weak());
                ui.add_space(4.0);
            }
        });
    }

    fn outcome_panel(&self, ui: &mut Ui, summary: &RoiSummary) {
        let window = self.window_days;
        info_card::section_card(
            ui,
            icons::CHECK,
            "Did the fix stick?",
            Some("A diagnostic counts as fixed only when the same computer has not come back, and only once the window has fully elapsed."),
            |ui| {
                let Some(b) = summary.outcome.overall.iter().find(|b| b.window_days == window) else {
                    ui.label(RichText::new("no bucket for this window").weak());
                    return;
                };
                let decided = b.confirmed_fixed + b.comeback;
                ui.horizontal_wrapped(|ui| {
                    info_card::badge(ui, &format!("{} fixed", b.confirmed_fixed), theme::success(ui));
                    info_card::badge(ui, &format!("{} came back", b.comeback), theme::error(ui));
                    info_card::badge(
                        ui,
                        &format!("{} undecided", b.indeterminate),
                        theme::weak_text(ui),
                    );
                });
                ui.add_space(4.0);
                info_card::kv_row(
                    ui,
                    "Comeback rate",
                    &format!("{} of {decided} decided ({})", b.comeback, pct(b.comeback, decided)),
                );
                info_card::kv_row(
                    ui,
                    "Undecided",
                    &format!(
                        "{} of {} scored - window has not elapsed yet",
                        b.indeterminate, summary.outcome.sessions_considered
                    ),
                );
                info_card::kv_row(
                    ui,
                    "Excluded",
                    &format!(
                        "{} staff/test machines, {} sessions with no computer link, {} set aside by hand",
                        summary.outcome.excluded_internal,
                        summary.outcome.no_computer,
                        summary.outcome.excluded_override
                    ),
                );
                if b.overridden > 0 {
                    info_card::kv_row(
                        ui,
                        "Human verdicts",
                        &format!(
                            "{} of {} came from a person, not from the orders",
                            b.overridden, summary.outcome.sessions_considered
                        ),
                    );
                }

                ui.add_space(6.0);
                let (fixed_color, comeback_color) = (theme::success(ui), theme::error(ui));
                let bars: Vec<Bar> = summary
                    .outcome
                    .overall
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        Bar::new(i as f64, b.confirmed_fixed as f64)
                            .name(format!("{}d fixed", b.window_days))
                    })
                    .collect();
                let comebacks: Vec<Bar> = summary
                    .outcome
                    .overall
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        Bar::new(i as f64 + 0.35, b.comeback as f64)
                            .name(format!("{}d comeback", b.window_days))
                    })
                    .collect();
                Plot::new("outcome_windows")
                    .height(140.0)
                    .legend(Legend::default())
                    .show_axes([false, true])
                    .show(ui, |plot| {
                        plot.bar_chart(BarChart::new("fixed", bars).width(0.3).color(fixed_color));
                        plot.bar_chart(
                            BarChart::new("comeback", comebacks).width(0.3).color(comeback_color),
                        );
                    });
                ui.label(
                    RichText::new(
                        "Escalated sessions are scored separately; inside that group \"fixed\" means \
                         \"no return within the window\", not \"we verified the repair\".",
                    )
                    .weak(),
                );
            },
        );
    }

    fn cost_panel(&self, ui: &mut Ui, summary: &RoiSummary) {
        info_card::section_card(
            ui,
            icons::CHART,
            "What it cost",
            Some("AI cost is measured from the usage ledger at API-equivalent rates. Technician labor is the audited active-minute floor priced across the pay band."),
            |ui| {
                info_card::kv_row(
                    ui,
                    &format!("AI spend (last {}d)", summary.window_days),
                    &format!("${:.2} across {} sessions", summary.ai_cost_usd, summary.ai_usage_rows),
                );
                if summary.ai_usage_rows > 0 {
                    info_card::kv_row(
                        ui,
                        "AI per session",
                        &format!("${:.2}", summary.ai_cost_usd / summary.ai_usage_rows as f64),
                    );
                }
                info_card::kv_row(
                    ui,
                    "Technician active time",
                    &format!(
                        "{} min ({:.1}h) of audited edits",
                        summary.tech_active_minutes,
                        summary.tech_active_minutes as f64 / 60.0
                    ),
                );
                info_card::kv_row(
                    ui,
                    &format!("Labor at ${TECH_RATE_LOW:.0}-${TECH_RATE_HIGH:.0}/hr"),
                    &format!("${:.2} - ${:.2}", summary.tech_labor_low_usd, summary.tech_labor_high_usd),
                );
                if summary.ai_usage_rows == 0 {
                    ui.label(
                        RichText::new(
                            "No AI cost rows in this window. Only Claude Code sessions report usage; \
                             Claude Desktop work is invisible to the ledger.",
                        )
                        .color(theme::warn(ui)),
                    );
                }
            },
        );
    }

    fn turnaround_panel(&self, ui: &mut Ui, summary: &RoiSummary) {
        info_card::section_card(
            ui,
            icons::STATUS_QUEUED,
            "Turnaround by origin",
            Some("Check-in to first completion. Open hours count only 10-19 local, Monday through Saturday."),
            |ui| {
                let row = |ui: &mut Ui, label: &str, s: &TurnaroundStats| {
                    let wall = s.median_wall_secs.map(hours).unwrap_or_else(|| "n/a".into());
                    let biz = s.median_business_secs.map(hours).unwrap_or_else(|| "n/a".into());
                    let biz_days =
                        s.median_business_secs.map(open_days).unwrap_or_else(|| "n/a".into());
                    info_card::kv_row(
                        ui,
                        label,
                        &format!(
                            "n={} | median {biz} open ({biz_days}) | {wall} wall | {} within one open day",
                            s.n,
                            pct(s.within_one_open_day, s.n)
                        ),
                    );
                };
                row(ui, "AI-origin tasks", &summary.ai_turnaround);
                row(ui, "Tech-origin tasks", &summary.tech_turnaround);

                let pairs = [
                    ("AI", &summary.ai_turnaround),
                    ("Tech", &summary.tech_turnaround),
                ];
                let bars: Vec<Bar> = pairs
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (name, s))| {
                        s.median_business_secs.map(|secs| {
                            Bar::new(i as f64, secs as f64 / 3600.0).name(format!("{name} median"))
                        })
                    })
                    .collect();
                let bar_color = theme::accent(ui);
                if !bars.is_empty() {
                    Plot::new("turnaround_by_origin")
                        .height(130.0)
                        .legend(Legend::default())
                        .show_axes([false, true])
                        .show(ui, |plot| {
                            plot.bar_chart(
                                BarChart::new("median open hours", bars).width(0.4).color(bar_color),
                            );
                        });
                }
                if summary.ai_turnaround.n == 0 {
                    ui.label(
                        RichText::new(
                            "No AI-origin tasks completed in this window - task.origin is stamped \
                             only when a diagnostic session creates or links the task.",
                        )
                        .color(theme::warn(ui)),
                    );
                }
            },
        );
    }

    fn gaps_panel(&self, ui: &mut Ui, summary: &RoiSummary) {
        let g = &summary.gaps;
        info_card::section_card(
            ui,
            icons::STATUS_WARN,
            "What we cannot see yet",
            Some("Every number above is a floor. These counters are the reason."),
            |ui| {
                info_card::kv_row(
                    ui,
                    "Sessions still open",
                    &format!("{} of {} ({})", g.sessions_open, g.sessions_total, pct(g.sessions_open, g.sessions_total)),
                );
                info_card::kv_row(
                    ui,
                    "Sessions with no task or order link",
                    &format!("{} - unreachable from a service number", g.sessions_unlinked),
                );
                info_card::kv_row(
                    ui,
                    "Closed without a diagnosis milestone",
                    &format!("{} - turnaround cannot see when the root cause landed", g.sessions_without_diagnosed),
                );
                info_card::kv_row(
                    ui,
                    "Service orders with no computer",
                    &format!("{} - cannot participate in comeback detection", g.orders_without_computer),
                );
                info_card::kv_row(
                    ui,
                    "Tasks with no origin stamp",
                    &format!("{} in the lookback window", g.tasks_without_origin),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Not measured here: comebacks that went to another shop, customers who gave \
                         up, and per-store status hygiene (that needs the PrestaShop history sweep).",
                    )
                    .weak(),
                );
            },
        );
    }

    /// Ground-truth controls: inference only ever sees new service orders, so a
    /// tech who knows the real result has to be able to say so.
    fn corrections_panel(&mut self, ui: &mut Ui, summary: &RoiSummary) {
        let Some(sessions) = summary.outcome.sessions.as_ref() else {
            return;
        };
        let window = self.window_days;
        let now = summary.outcome.computed_at_unix;
        let mut recent: Vec<&SessionOutcomeRow> = sessions.iter().collect();
        recent.sort_by_key(|r| std::cmp::Reverse(r.ended_at_unix.unwrap_or(0)));
        let shown = recent.len().min(CORRECTABLE_LIMIT);
        let empty: Vec<SessionOutcomeRow> = Vec::new();
        let excluded = summary.outcome.excluded_sessions.as_ref().unwrap_or(&empty);

        info_card::section_card(
            ui,
            icons::EDIT,
            "Correct a verdict",
            Some("A correction is recorded against your name with your reason, and the panel above counts how many verdicts came from people rather than from data."),
            |ui| {
                if !self.write_status.is_empty() {
                    ui.label(RichText::new(&self.write_status).weak());
                    ui.add_space(4.0);
                }
                ui.label(
                    RichText::new(format!(
                        "{shown} most recently ended of {} scored sessions",
                        recent.len()
                    ))
                    .weak(),
                );
                ui.add_space(4.0);
                for row in recent.into_iter().take(CORRECTABLE_LIMIT) {
                    self.session_row(ui, row, window, now);
                }

                if !excluded.is_empty() {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(RichText::new("Set aside by hand").strong());
                    for row in excluded {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&row.hostname).strong());
                            if let Some(by) = &row.override_by {
                                ui.label(RichText::new(by.as_str()).weak());
                            }
                            if ui.button("Count it again").clicked() {
                                self.clear_override(row);
                            }
                        });
                        if let Some(reason) = &row.override_reason {
                            ui.label(RichText::new(reason.as_str()).weak());
                        }
                    }
                }

                if summary.outcome.internal_computer_count > 0 {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(RichText::new("Staff and test machines").strong());
                    ui.label(
                        RichText::new(format!(
                            "{} machines are flagged; every session on them is excluded, past and                              future. Only the {} that cost a session here are listed.",
                            summary.outcome.internal_computer_count,
                            summary.outcome.internal_computers.len()
                        ))
                        .weak(),
                    );
                    for key in &summary.outcome.internal_computers {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(key.as_str()).monospace());
                            if ui.button("Treat as a customer machine").clicked() {
                                self.set_internal(key, false);
                            }
                        });
                    }
                }
            },
        );
    }

    fn session_row(&mut self, ui: &mut Ui, row: &SessionOutcomeRow, window: u32, now: i64) {
        let (success, error, weak, accent) =
            (theme::success(ui), theme::error(ui), theme::weak_text(ui), theme::accent(ui));
        let verdict = verdict_at(row, window);
        let (label, color) = match verdict {
            "confirmed_fixed" => ("fixed", success),
            "comeback" => ("came back", error),
            _ => ("undecided", weak),
        };
        let editing = self.editing.as_deref() == Some(row.session_id.as_str());
        let mut toggle = false;
        ui.horizontal(|ui| {
            info_card::badge(ui, label, color);
            ui.label(RichText::new(&row.hostname).strong());
            ui.label(RichText::new(&row.status).weak());
            if let Some(end) = row.ended_at_unix {
                ui.label(RichText::new(ago(now, end)).weak());
            }
            if row.outcome_override.is_some() {
                info_card::badge(ui, "by hand", accent);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if editing { "Close" } else { "Correct" };
                if ui.button(format!("{} {text}", icons::EDIT)).clicked() {
                    toggle = true;
                }
            });
        });
        if let Some(cb) = &row.comeback {
            ui.label(
                RichText::new(format!(
                    "same computer returned on #{} after {} days",
                    cb.service_number, cb.days_after_end
                ))
                .weak(),
            );
        }
        if let (Some(by), Some(reason)) = (&row.override_by, &row.override_reason) {
            ui.label(RichText::new(format!("{by}: {reason}")).weak());
        }
        if toggle {
            self.editing = if editing { None } else { Some(row.session_id.clone()) };
            self.draft_verdict = row.outcome_override;
            self.draft_reason = row.override_reason.clone().unwrap_or_default();
        }
        if editing && !toggle {
            self.editor(ui, row);
        }
        ui.add_space(4.0);
    }

    fn editor(&mut self, ui: &mut Ui, row: &SessionOutcomeRow) {
        let busy = self.write_busy;
        let has_override = row.outcome_override.is_some();
        let computer = row.computer.clone();
        let mut action: Option<Action> = None;
        ui.indent("correction_editor", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(self.draft_verdict.is_none(), "Leave it to the data").clicked()
                {
                    self.draft_verdict = None;
                }
                for v in OutcomeOverride::ALL {
                    if ui.selectable_label(self.draft_verdict == Some(v), v.label()).clicked() {
                        self.draft_verdict = Some(v);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Why:");
                ui.add(
                    TextEdit::singleline(&mut self.draft_reason)
                        .desired_width(360.0)
                        .hint_text("what you know that the service orders do not show"),
                );
            });
            let needs_reason = self.draft_verdict.is_some() && self.draft_reason.trim().is_empty();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !busy && !needs_reason,
                        Button::new(format!("{} Apply", icons::CHECK)),
                    )
                    .clicked()
                {
                    action = Some(Action::Apply);
                }
                if has_override && ui.add_enabled(!busy, Button::new("Clear override")).clicked() {
                    action = Some(Action::Clear);
                }
                if let Some(key) = computer.as_deref() {
                    if ui
                        .add_enabled(
                            !busy,
                            Button::new(format!("{} This is a staff machine", icons::DESKTOP)),
                        )
                        .on_hover_text("Excludes every session on this computer, past and future")
                        .clicked()
                    {
                        action = Some(Action::Internal(key.to_string()));
                    }
                }
            });
            if needs_reason {
                ui.label(RichText::new("A correction needs a reason.").weak());
            }
        });
        match action {
            Some(Action::Apply) => self.apply_override(row),
            Some(Action::Clear) => self.clear_override(row),
            Some(Action::Internal(key)) => self.set_internal(&key, true),
            None => {},
        }
    }
}

/// Deferred so a click can borrow the editor's own state first.
enum Action {
    Apply,
    Clear,
    Internal(String),
}
