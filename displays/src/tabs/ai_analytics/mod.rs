//! AI diagnostics ROI dashboard: outcome funnel, measured AI cost against the
//! technician pay band, turnaround by task origin, and the counters that bound
//! every one of those numbers.
//!
//! Turnaround is reported in open-store hours (10-19 local, closed Sunday) as
//! well as wall clock; comeback windows stay in calendar days because a
//! customer's machine fails on customer time.

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use database::schema::{RoiSummary, TurnaroundStats, TECH_RATE_HIGH, TECH_RATE_LOW};
use eframe::egui::{Align, Layout, RichText, ScrollArea, Ui};
use egui_plot::{Bar, BarChart, Legend, Plot};
use web_time::Instant;

use crate::ui_tools::{icons, info_card, theme};
use crate::{PlatformSpawner, Spawner};

const LOOKBACKS: [u32; 4] = [30, 60, 90, 180];
const WINDOWS: [u32; 3] = [30, 60, 90];
const OPEN_HOURS_PER_DAY: f64 = 9.0;

pub struct AiAnalytics {
    summary: Option<RoiSummary>,
    lookback_days: u32,
    window_days: u32,
    loading: bool,
    status: String,
    last_poll: Option<Instant>,
    tx: Sender<Result<RoiSummary, String>>,
    rx: Receiver<Result<RoiSummary, String>>,
}

impl Default for AiAnalytics {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            summary: None,
            lookback_days: 90,
            window_days: 30,
            loading: false,
            status: String::new(),
            last_poll: None,
            tx,
            rx,
        }
    }
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

    fn drain(&mut self) {
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
        if self.loading {
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
            self.outcome_panel(ui, &summary);
            self.cost_panel(ui, &summary);
            self.turnaround_panel(ui, &summary);
            self.gaps_panel(ui, &summary);
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
                        "{} staff/test machines, {} sessions with no computer link",
                        summary.outcome.excluded_internal, summary.outcome.no_computer
                    ),
                );

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
}
