//! Bug report tab: submit a GitHub issue from within qc-app. Resolves the
//! submitter against the Shopify staff roster so the issue names who to ask.

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::orders::{QcBackend, TechIdentity};
use eframe::egui;
use mtech_ui::github::{build_github_issue_body, create_new_issue};

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

        ui.heading("Bug report");
        ui.label(
            egui::RichText::new("Files a GitHub issue. Recent app logs are attached automatically.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Employee name or ID");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.employee_input)
                    .hint_text("e.g. Kellie Boisse or 12345")
                    .desired_width(220.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = ui
                .add_enabled(
                    !self.busy && !self.employee_input.trim().is_empty(),
                    egui::Button::new("Look up"),
                )
                .clicked();
            if clicked || (enter && !self.employee_input.trim().is_empty()) {
                self.kick_off_lookup();
            }
        });
        if let Some(id) = &self.resolved {
            ui.colored_label(
                egui::Color32::from_rgb(50, 160, 90),
                format!("{} — employee #{}", id.name, id.id_employee),
            );
        }

        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.title)
                .hint_text("Issue title")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .hint_text("Describe the bug…")
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );

        ui.add_space(8.0);
        let can_submit =
            !self.busy && !self.title.trim().is_empty() && !self.description.trim().is_empty();
        if ui
            .add_enabled(can_submit, egui::Button::new("Submit bug report"))
            .clicked()
        {
            self.kick_off_submit();
        }

        if self.busy {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working…");
            });
        }
        if !self.status.is_empty() {
            ui.add_space(4.0);
            let color = if self.status.contains("failed") {
                egui::Color32::LIGHT_RED
            } else if self.status.contains("submitted") {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::LIGHT_BLUE
            };
            ui.colored_label(color, &self.status);
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
                let name = if typed.is_empty() { "unknown".to_string() } else { typed };
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
