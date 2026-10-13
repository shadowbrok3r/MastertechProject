//! Bug report tab: submit a GitHub issue from within qc-app. Resolves the
//! submitter against the Shopify staff roster so the issue names who to ask.

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::orders::{QcBackend, TechIdentity};
use eframe::egui;
use mtech_ui::controls::{self, ButtonKind, Metrics, TextButton, TextField};
use mtech_ui::github::{build_github_issue_body, create_new_issue};
use mtech_ui::icons::svg;
use mtech_ui::theme;

/// Width of the employee lookup field.
const EMPLOYEE_W: f32 = 220.0;

enum BugReportEvent {
    Resolved(TechIdentity),
    LookupFailed(String),
    Submitted,
    SubmitFailed(String),
}

pub struct BugReportPanel {
    title: String,
    description: String,
    employee_input: String,
    resolved: Option<TechIdentity>,
    status: String,
    busy: bool,
    channel: (Sender<BugReportEvent>, Receiver<BugReportEvent>),
}

impl Default for BugReportPanel {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            employee_input: String::new(),
            resolved: None,
            status: String::new(),
            busy: false,
            channel: unbounded(),
        }
    }
}

impl BugReportPanel {
    fn poll(&mut self) {
        while let Ok(ev) = self.channel.1.try_recv() {
            match ev {
                BugReportEvent::Resolved(id) => {
                    self.busy = false;
                    self.status = format!("Matched {} (employee #{})", id.name, id.id_employee);
                    self.resolved = Some(id);
                }
                BugReportEvent::LookupFailed(e) => {
                    self.busy = false;
                    self.resolved = None;
                    self.status = format!("Lookup failed: {e}");
                }
                BugReportEvent::Submitted => {
                    self.busy = false;
                    self.status = "Bug report submitted — thank you!".to_string();
                    self.title.clear();
                    self.description.clear();
                }
                BugReportEvent::SubmitFailed(e) => {
                    self.busy = false;
                    self.status = format!("Submit failed: {e}");
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        let m = Metrics::get(ui.ctx());

        controls::heading(ui, svg::BUG, "Bug report");
        ui.label(
            egui::RichText::new(
                "Files a GitHub issue. Recent app logs are attached automatically.",
            )
            .small()
            .weak(),
        );
        ui.add_space(m.gap.x);

        ui.horizontal(|ui| {
            ui.label("Employee name or ID");
            let resp = TextField::new(&mut self.employee_input)
                .hint("e.g. Kellie Boisse or 12345")
                .icon(svg::USER)
                .width(EMPLOYEE_W)
                .show(ui);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let can_look_up = !self.busy && !self.employee_input.trim().is_empty();
            let clicked = TextButton::new("Look up")
                .icon(svg::MAGNIFYING_GLASS)
                .enabled(can_look_up)
                .show(ui)
                .clicked();
            if clicked || (enter && !self.employee_input.trim().is_empty()) {
                self.kick_off_lookup();
            }
        });
        if let Some(id) = &self.resolved {
            let ok = theme::success(ui);
            controls::status_label(
                ui,
                svg::USER_CHECK,
                &format!("{} — employee #{}", id.name, id.id_employee),
                ok,
            );
        }

        ui.add_space(m.gap.x);
        TextField::new(&mut self.title)
            .hint("Issue title")
            .icon(svg::NOTE_PENCIL)
            .width(ui.available_width())
            .show(ui);
        ui.add_space(m.pad.y);
        controls::TextArea::new(&mut self.description)
            .hint("Describe the bug…")
            .rows(8)
            .show(ui);

        ui.add_space(m.gap.x);
        let can_submit =
            !self.busy && !self.title.trim().is_empty() && !self.description.trim().is_empty();
        let submit = TextButton::new("Submit bug report")
            .icon(svg::PAPER_PLANE_TILT)
            .kind(ButtonKind::Primary)
            .prominent(true)
            .enabled(can_submit)
            .show(ui);
        if submit.clicked() {
            self.kick_off_submit();
        }

        if self.busy {
            ui.add_space(m.pad.y);
            controls::loading_row(ui, "Working…");
        }
        if !self.status.is_empty() {
            ui.add_space(m.pad.y);
            let (icon, tone) = if self.status.contains("failed") {
                (svg::X_CIRCLE, theme::error(ui))
            } else if self.status.contains("submitted") {
                (svg::CHECK_CIRCLE, theme::success(ui))
            } else {
                (svg::INFO, theme::info(ui))
            };
            controls::status_label(ui, icon, self.status.as_str(), tone);
        }
    }

    fn kick_off_lookup(&mut self) {
        let input = self.employee_input.trim().to_string();
        let tx = self.channel.0.clone();
        self.busy = true;
        self.status = "Looking up employee…".to_string();
        tokio::spawn(async move {
            let backend = QcBackend::shopify();
            match backend.authenticate_tech(&input, "").await {
                Ok(id) => {
                    let _ = tx.send(BugReportEvent::Resolved(id));
                }
                Err(e) => {
                    let _ = tx.send(BugReportEvent::LookupFailed(format!("{e:#}")));
                }
            }
        });
    }

    fn kick_off_submit(&mut self) {
        let title = self.title.trim().to_string();
        let description = self.description.clone();
        let resolved = self.resolved.clone();
        let typed = self.employee_input.trim().to_string();
        let tx = self.channel.0.clone();
        self.busy = true;
        self.status = "Submitting…".to_string();

        let logs = mtech_ui::egui_logger::get_logs_for_issue();
        let (name, contact) = match resolved {
            Some(id) => {
                let contact = if id.email.is_empty() {
                    format!("employee #{}", id.id_employee)
                } else {
                    id.email
                };
                (id.name, contact)
            }
            None => {
                let name = if typed.is_empty() {
                    "unknown".to_string()
                } else {
                    typed
                };
                (name, "employee not resolved".to_string())
            }
        };
        let body = build_github_issue_body(&description, &name, &contact, &logs);

        tokio::spawn(async move {
            let client = reqwest::Client::new();
            match create_new_issue(title, body, client).await {
                Ok(res) => {
                    log::info!("bug report submitted: {res}");
                    let _ = tx.send(BugReportEvent::Submitted);
                }
                Err(e) => {
                    let _ = tx.send(BugReportEvent::SubmitFailed(format!("{e:#}")));
                }
            }
        });
    }
}
