//! Order QC panel: lookup + status gate + items/serials + spec check +
//! tech sign-off + comments + photo check + status advance + QC submission.
//!
//! All backend traffic flows through `database::orders::QcBackend`, keeping
//! this panel renderer-agnostic enough to re-mount under a terminal-mode
//! front end later: state lives in [`OrderPanel`], async results arrive on
//! a channel as [`PanelMsg`].

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::orders::{
    gate::GateOutcome, persist_qc_report, BackendKind, BuildSpec, GateDecision, OrderComment,
    OrderKey, OrderKind, PhotoCheck, QcBackend, QcChecklist, QcOrder, QcReportPayload,
    TechIdentity,
};
use database::schema::{RecordId, RecordIdExt, RunResult, TICKET_TABLE};
use eframe::egui::{
    self, Align, Color32, CollapsingHeader, Frame, Layout, Margin, RichText, ScrollArea, TextEdit,
};
use egui_phosphor::regular as p;
use stress_runner::RunVerdict;
use stress_kit::telemetry::TelemetrySnapshot;

use crate::spec_check::{collect_detected, compare, CheckStatus, SpecCheckReport};

const GOOD: Color32 = Color32::from_rgb(61, 185, 157);
const CAUTION: Color32 = Color32::from_rgb(180, 140, 50);

pub struct OrderSession {
    pub backend: QcBackend,
    pub order: QcOrder,
    pub spec: BuildSpec,
    pub gate: GateDecision,
    pub comments: Vec<OrderComment>,
    pub photos: PhotoCheck,
}

struct LoadedOrder {
    order: QcOrder,
    spec: BuildSpec,
    gate: GateDecision,
    comments: Vec<OrderComment>,
    photos: PhotoCheck,
}

enum PanelMsg {
    Loaded(Box<LoadedOrder>),
    LoadFailed(String),
    Auth { signoff: bool, result: Result<TechIdentity, String> },
    CommentPosted(Box<Result<OrderComment, String>>),
    CommentsRefreshed(Result<Vec<OrderComment>, String>),
    Advanced(Result<i64, String>),
    Submitted(Result<String, String>),
}

pub struct OrderPanel {
    key_input: String,
    busy: bool,
    error: Option<String>,
    session: Option<OrderSession>,

    tech_email: String,
    tech_password: String,
    tech: Option<TechIdentity>,
    auth_busy: bool,
    auth_error: Option<String>,

    signoff_email: String,
    signoff_password: String,
    signoff: Option<TechIdentity>,
    signoff_busy: bool,
    signoff_error: Option<String>,

    comment_input: String,
    comment_busy: bool,
    comment_error: Option<String>,

    spec_report: Option<SpecCheckReport>,

    checklist: QcChecklist,
    report_notes: String,
    submit_busy: bool,
    submit_result: Option<(bool, String)>,
    advance_busy: bool,
    advance_result: Option<(bool, String)>,

    tx: Sender<PanelMsg>,
    rx: Receiver<PanelMsg>,
}

impl Default for OrderPanel {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            key_input: String::new(),
            busy: false,
            error: None,
            session: None,
            tech_email: String::new(),
            tech_password: String::new(),
            tech: None,
            auth_busy: false,
            auth_error: None,
            signoff_email: String::new(),
            signoff_password: String::new(),
            signoff: None,
            signoff_busy: false,
            signoff_error: None,
            comment_input: String::new(),
            comment_busy: false,
            comment_error: None,
            spec_report: None,
            checklist: QcChecklist::default(),
            report_notes: String::new(),
            submit_busy: false,
            submit_result: None,
            advance_busy: false,
            advance_result: None,
            tx,
            rx,
        }
    }
}

impl OrderPanel {
    /// `(service_order, tech)` context applied to stress runs while an order
    /// session is open.
    pub fn run_context(&self) -> Option<(RecordId, String)> {
        let session = self.session.as_ref()?;
        let service = RecordId::new(TICKET_TABLE, session.order.id.clone());
        let tech = self.tech.as_ref().map(|t| t.name.clone()).unwrap_or_default();
        Some((service, tech))
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                PanelMsg::Loaded(loaded) => {
                    self.busy = false;
                    self.error = None;
                    let LoadedOrder { order, spec, gate, comments, photos } = *loaded;
                    let backend = order
                        .key
                        .as_ref()
                        .map(QcBackend::for_key)
                        .unwrap_or_else(|| QcBackend::for_key(&OrderKey::Prestashop(order.id.clone())));
                    self.session = Some(OrderSession { backend, order, spec, gate, comments, photos });
                    self.spec_report = None;
                    self.checklist = QcChecklist::default();
                    self.report_notes.clear();
                    self.submit_result = None;
                    self.advance_result = None;
                    self.signoff = None;
                }
                PanelMsg::LoadFailed(e) => {
                    self.busy = false;
                    self.error = Some(e);
                }
                PanelMsg::Auth { signoff, result } => {
                    if signoff {
                        self.signoff_busy = false;
                        match result {
                            Ok(t) => {
                                self.signoff = Some(t);
                                self.signoff_error = None;
                                self.signoff_password.clear();
                            }
                            Err(e) => self.signoff_error = Some(e),
                        }
                    } else {
                        self.auth_busy = false;
                        match result {
                            Ok(t) => {
                                self.tech = Some(t);
                                self.auth_error = None;
                                self.tech_password.clear();
                            }
                            Err(e) => self.auth_error = Some(e),
                        }
                    }
                }
                PanelMsg::CommentPosted(result) => {
                    self.comment_busy = false;
                    match *result {
                        Ok(comment) => {
                            self.comment_input.clear();
                            self.comment_error = None;
                            if let Some(s) = self.session.as_mut() {
                                s.comments.push(comment);
                            }
                        }
                        Err(e) => self.comment_error = Some(e),
                    }
                }
                PanelMsg::CommentsRefreshed(result) => {
                    self.comment_busy = false;
                    match result {
                        Ok(comments) => {
                            if let Some(s) = self.session.as_mut() {
                                s.comments = comments;
                            }
                        }
                        Err(e) => self.comment_error = Some(e),
                    }
                }
                PanelMsg::Advanced(result) => {
                    self.advance_busy = false;
                    match result {
                        Ok(to) => {
                            self.advance_result = Some((true, format!("Status advanced to {to}.")));
                            if let Some(s) = self.session.as_mut() {
                                s.order.status.legacy_id = to;
                                s.order.status.name = database::orders::gate::status_display(to, "");
                                s.gate = s.backend.status_gate(&s.order);
                            }
                        }
                        Err(e) => self.advance_result = Some((false, e)),
                    }
                }
                PanelMsg::Submitted(result) => {
                    self.submit_busy = false;
                    self.submit_result = Some(match result {
                        Ok(msg) => (true, msg),
                        Err(e) => (false, e),
                    });
                }
            }
        }
    }

    fn start_load(&mut self, ctx: &egui::Context) {
        let Some(key) = OrderKey::parse(&self.key_input) else {
            self.error = Some("Enter a PS order (2…), Everest doc (5…), Shopify order # or XBS- serial.".into());
            return;
        };
        self.busy = true;
        self.error = None;
        let backend = QcBackend::for_key(&key);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = async {
                let order = backend.find_order(&key).await?;
                let spec = backend.build_spec(&order).await.unwrap_or_default();
                let gate = backend.status_gate(&order);
                let comments = backend.fetch_comments(&order).await.unwrap_or_default();
                let photos = backend.check_build_photos(&order).await.unwrap_or_default();
                Ok::<_, anyhow::Error>(LoadedOrder { order, spec, gate, comments, photos })
            }
            .await;
            let _ = match result {
                Ok(loaded) => tx.send(PanelMsg::Loaded(Box::new(loaded))),
                Err(e) => tx.send(PanelMsg::LoadFailed(format!("{e:#}"))),
            };
            ctx.request_repaint();
        });
    }

    fn start_auth(&mut self, ctx: &egui::Context, signoff: bool) {
        let Some(session) = self.session.as_ref() else { return };
        let (email, password) = if signoff {
            (self.signoff_email.clone(), self.signoff_password.clone())
        } else {
            (self.tech_email.clone(), self.tech_password.clone())
        };
        // Shopify identity is a roster name match; no PIN verification exists yet.
        let needs_password = session.backend.backend_kind() == BackendKind::Prestashop;
        if email.trim().is_empty() || (needs_password && password.is_empty()) {
            let err = Some(if needs_password {
                "Email and password required.".to_string()
            } else {
                "Tech name required.".to_string()
            });
            if signoff { self.signoff_error = err } else { self.auth_error = err }
            return;
        }
        if signoff { self.signoff_busy = true } else { self.auth_busy = true }
        let backend = session.backend.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend
                .authenticate_tech(email.trim(), &password)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::Auth { signoff, result });
            ctx.request_repaint();
        });
    }

    fn start_post_comment(&mut self, ctx: &egui::Context) {
        let Some(session) = self.session.as_ref() else { return };
        let Some(tech) = self.tech.clone() else {
            self.comment_error = Some("Sign in before posting comments.".into());
            return;
        };
        let body = self.comment_input.trim().to_string();
        if body.is_empty() {
            return;
        }
        self.comment_busy = true;
        self.comment_error = None;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend
                .post_comment(&order, &tech, &body)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::CommentPosted(Box::new(result)));
            ctx.request_repaint();
        });
    }

    fn start_refresh_comments(&mut self, ctx: &egui::Context) {
        let Some(session) = self.session.as_ref() else { return };
        self.comment_busy = true;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend
                .fetch_comments(&order)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::CommentsRefreshed(result));
            ctx.request_repaint();
        });
    }

    fn start_advance(&mut self, ctx: &egui::Context, to: i64) {
        let Some(session) = self.session.as_ref() else { return };
        self.advance_busy = true;
        self.advance_result = None;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend
                .advance_status(&order, to)
                .await
                .map(|_| to)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::Advanced(result));
            ctx.request_repaint();
        });
    }

    fn start_submit(&mut self, ctx: &egui::Context, last_verdict: Option<&RunVerdict>, preset: Option<String>) {
        let Some(session) = self.session.as_ref() else { return };
        let Some(tech) = self.tech.clone() else {
            self.submit_result = Some((false, "Sign in before submitting the QC report.".into()));
            return;
        };
        if session.order.kind == OrderKind::Repair && self.signoff.is_none() {
            self.submit_result = Some((false, "Repair orders need the second sign-off before submitting.".into()));
            return;
        }

        let verdict = last_verdict
            .map(|v| match v.result {
                RunResult::Pass => "passed",
                RunResult::Fail => "failed",
                RunResult::Aborted => "aborted",
                RunResult::Inconclusive => "inconclusive",
                RunResult::InProgress => "in_progress",
            })
            .unwrap_or("not_run")
            .to_string();

        let payload = QcReportPayload {
            order_key: session.order.key.as_ref().map(|k| k.display().to_string()).unwrap_or_default(),
            order_id: session.order.id.clone(),
            backend: session.backend.backend_kind().as_str().to_string(),
            verdict,
            preset,
            machine_id: Some(crate::reporting::machine_id()),
            tech: Some(tech.name.clone()),
            tech_employee_id: Some(tech.id_employee.clone()),
            signoff_tech: self.signoff.as_ref().map(|t| t.name.clone()),
            signoff_employee_id: self.signoff.as_ref().map(|t| t.id_employee.clone()),
            duration_secs: last_verdict.map(|v| v.duration_secs).unwrap_or(0.0),
            whea_delta: last_verdict.map(|v| v.summary.whea_delta_count as i64).unwrap_or(0),
            tdr_delta: last_verdict.map(|v| v.summary.tdr_count as i64).unwrap_or(0),
            stressor_errors: last_verdict
                .map(|v| {
                    (v.summary.test_errors.max(v.summary.memory_errors)
                        + v.summary.disk_io_errors) as i64
                })
                .unwrap_or(0),
            cpu_max_c: last_verdict.and_then(|v| v.summary.max_temp_c).map(f64::from),
            gpu_max_c: last_verdict.and_then(|v| v.summary.max_gpu_temp_c).map(f64::from),
            spec_check: self.spec_report.as_ref().map(|r| r.summary()),
            run_ref: last_verdict.map(|v| format!("stress_test_run:{}", v.run_id.key_string())),
            checklist: self.checklist,
            notes: self.report_notes.clone(),
            stages: last_verdict
                .map(|v| {
                    v.stage_outcomes
                        .iter()
                        .map(|o| database::orders::QcStageBrief {
                            label: o.summary.label.clone(),
                            throughput: o.summary.avg_throughput.unwrap_or(0.0),
                            unit: o.summary.throughput_unit.clone(),
                            result: match (&o.verdict, o.summary.had_error) {
                                (Some(v), _) if v.pass => "pass".to_string(),
                                (Some(_), _) => "fail".to_string(),
                                (None, true) => "fail".to_string(),
                                (None, false) => "unscored".to_string(),
                            },
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };

        self.submit_busy = true;
        self.submit_result = None;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let persisted = persist_qc_report(&payload).await;
            let pushed = backend.submit_qc(&order, &payload).await;
            let result = match (persisted, pushed) {
                (Ok(id), Ok(())) => Ok(format!(
                    "QC report saved ({}) and pushed to {}.",
                    id.key_string(),
                    backend.backend_kind().as_str()
                )),
                (Ok(id), Err(e)) => Ok(format!(
                    "QC report saved ({}); backend push failed: {e:#}",
                    id.key_string()
                )),
                (Err(db), Ok(())) => Err(format!("Backend push OK but SurrealDB save failed: {db:#}")),
                (Err(db), Err(push)) => Err(format!("SurrealDB save failed: {db:#}; backend push failed: {push:#}")),
            };
            let _ = tx.send(PanelMsg::Submitted(result));
            ctx.request_repaint();
        });
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<&TelemetrySnapshot>,
        last_verdict: Option<&RunVerdict>,
        last_preset: Option<String>,
    ) {
        self.drain_messages();
        let ctx = ui.ctx().clone();

        // Lookup bar
        ui.horizontal(|ui| {
            ui.label(RichText::new(p::MAGNIFYING_GLASS).size(16.0));
            let response = ui.add(
                TextEdit::singleline(&mut self.key_input)
                    .hint_text("PS order (2…) · Everest (5…) · Shopify # · XBS-…")
                    .desired_width(220.0),
            );
            let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Load order").clicked() || submitted) && !self.busy {
                self.start_load(&ctx);
            }
            if self.busy {
                ui.spinner();
            }
            if let Some(tech) = self.tech.as_ref() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.colored_label(GOOD, format!("{} {}", p::USER, tech.name));
                });
            }
        });
        if let Some(e) = self.error.as_ref() {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }

        if self.session.is_none() {
            ui.add_space(12.0);
            ui.label(
                RichText::new(
                    "Load an order to begin QC: gate check, items + serials, spec verification, \
                     sign-off, comments, and report submission.",
                )
                .weak(),
            );
            return;
        }

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.ui_order_header(ui);
            self.ui_gate_banner(ui, &ctx);
            CollapsingHeader::new(format!("{} Items & serials", p::PACKAGE))
                .default_open(true)
                .show(ui, |ui| self.ui_items(ui));
            CollapsingHeader::new(format!("{} Spec check", p::CPU))
                .default_open(true)
                .show(ui, |ui| self.ui_spec_check(ui, snapshot));
            CollapsingHeader::new(format!("{} Sign-off", p::USER))
                .default_open(false)
                .show(ui, |ui| self.ui_sign_off(ui, &ctx));
            CollapsingHeader::new(format!("{} Order comments", p::CHAT))
                .default_open(false)
                .show(ui, |ui| self.ui_comments(ui, &ctx));
            CollapsingHeader::new(format!("{} QC report", p::CLIPBOARD_TEXT))
                .default_open(false)
                .show(ui, |ui| self.ui_report(ui, &ctx, last_verdict, last_preset));
            ui.add_space(24.0);
        });
    }

    fn ui_order_header(&mut self, ui: &mut egui::Ui) {
        let Some(session) = self.session.as_ref() else { return };
        let order = &session.order;
        let photos = &session.photos;

        Frame::default()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(6.0)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Order {}", order.id)).strong().size(16.0));
                    if !order.reference.is_empty() && order.reference != order.id {
                        ui.label(RichText::new(format!("({})", order.reference)).weak());
                    }
                    ui.label(RichText::new(order.kind.as_str()).monospace().color(CAUTION));
                    if let Some(backend) = order.backend {
                        ui.label(RichText::new(backend.as_str()).monospace().weak());
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if !order.customer_name.is_empty() {
                        ui.label(format!("{} {}", p::USER, order.customer_name));
                    }
                    if !order.total_paid.is_empty() {
                        ui.label(format!("Total ${}", order.total_paid));
                    }
                    if let Some(doc) = order.everest_doc.as_ref() {
                        ui.label(format!("Everest {doc}"));
                    }
                    if let Some(serial) = order.build_serial.as_ref() {
                        ui.label(RichText::new(serial).monospace());
                    }
                    if let Some(parent) = order.parent_order_id.as_ref() {
                        ui.label(format!("Parent {parent}"));
                    }
                });
                if let Some(config) = order.config.as_ref() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Config: {}", config.name)).weak());
                        if let Some(b) = config.builder_employee.as_ref() {
                            ui.label(RichText::new(format!("Builder #{b}")).weak().small());
                        }
                        if let Some(q) = config.qc_employee.as_ref() {
                            ui.label(RichText::new(format!("QC #{q}")).weak().small());
                        }
                    });
                }
                if let Some(svc) = order.service_info.as_ref() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Device: {} {} {} (SN {})",
                                svc.device_mfg, svc.device_name, svc.device_model, svc.device_serial
                            ))
                            .weak(),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    // Build-photo presence check (PS order_image / Shopify build_photos).
                    if photos.present {
                        ui.colored_label(GOOD, format!("{} {} build photo(s)", p::CAMERA, photos.count));
                    } else {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("{} No build photo on order — upload before sign-off", p::CAMERA),
                        );
                    }
                });
            });
    }

    fn ui_gate_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(session) = self.session.as_ref() else { return };
        let gate = session.gate.clone();
        let backend = session.order.backend;

        let (color, icon) = match gate.outcome {
            GateOutcome::GoodToMove { .. } => (GOOD, p::CHECK_CIRCLE),
            GateOutcome::RefuseToMove => (ui.visuals().error_fg_color, p::X_CIRCLE),
            GateOutcome::Neutral => (CAUTION, p::WARNING),
        };

        ui.add_space(4.0);
        Frame::default()
            .fill(color.gamma_multiply(0.12))
            .stroke((1.0, color))
            .corner_radius(6.0)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, RichText::new(icon).size(18.0));
                    ui.colored_label(
                        color,
                        RichText::new(format!("{} ({})", gate.status_name, gate.status_legacy_id)).strong(),
                    );
                    ui.label(RichText::new(&gate.message).small());
                });
                if let Some(target) = gate.advance_target() {
                    ui.horizontal(|ui| {
                        let label = format!(
                            "Advance to {} ({target})",
                            database::orders::gate::status_display(target, "")
                        );
                        let can_advance = !self.advance_busy
                            && backend == Some(database::orders::BackendKind::Prestashop);
                        if ui.add_enabled(can_advance, egui::Button::new(label)).clicked() {
                            self.start_advance(ctx, target);
                        }
                        if backend == Some(database::orders::BackendKind::Shopify) {
                            ui.label(RichText::new("Advance flows through the Worker (W7).").weak().small());
                        }
                        if self.advance_busy {
                            ui.spinner();
                        }
                    });
                }
                if let Some((ok, msg)) = self.advance_result.as_ref() {
                    let c = if *ok { GOOD } else { ui.visuals().error_fg_color };
                    ui.colored_label(c, msg);
                }
            });
        ui.add_space(4.0);
    }

    fn ui_items(&mut self, ui: &mut egui::Ui) {
        let Some(session) = self.session.as_ref() else { return };
        let items = &session.order.items;
        if items.is_empty() {
            ui.label(RichText::new("No line items on this order.").weak());
            return;
        }

        let attached = items.iter().filter(|i| i.serial_attached()).count();
        ui.label(
            RichText::new(format!("{attached}/{} items have serials attached", items.len()))
                .small()
                .weak(),
        );

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::exact(22.0))
            .column(egui_extras::Column::remainder().clip(true))
            .column(egui_extras::Column::auto().at_least(60.0))
            .column(egui_extras::Column::exact(34.0))
            .column(egui_extras::Column::auto().at_least(90.0))
            .header(20.0, |mut header| {
                header.col(|_| {});
                header.col(|ui| { ui.strong("Item"); });
                header.col(|ui| { ui.strong("Ref"); });
                header.col(|ui| { ui.strong("Qty"); });
                header.col(|ui| { ui.strong("Serial"); });
            })
            .body(|mut body| {
                for item in items {
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            if item.serial_attached() {
                                ui.colored_label(GOOD, p::CHECK_CIRCLE);
                            } else {
                                ui.colored_label(CAUTION, p::CIRCLE_DASHED);
                            }
                        });
                        row.col(|ui| {
                            ui.label(&item.name);
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&item.reference).monospace().small());
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}", item.quantity));
                        });
                        row.col(|ui| {
                            if item.serials.is_empty() {
                                ui.label(RichText::new("—").weak());
                            } else {
                                ui.label(RichText::new(item.serials.join(", ")).monospace().small());
                            }
                        });
                    });
                }
            });
    }

    fn ui_spec_check(&mut self, ui: &mut egui::Ui, snapshot: Option<&TelemetrySnapshot>) {
        let Some(session) = self.session.as_ref() else { return };
        let spec = session.spec.clone();

        if spec.is_empty() {
            ui.label(RichText::new("No hardware spec could be derived from this order.").weak());
        } else {
            ui.horizontal_wrapped(|ui| {
                if !spec.model.is_empty() {
                    ui.label(RichText::new(&spec.model).strong());
                }
            });
        }

        ui.horizontal(|ui| {
            let enabled = snapshot.map(|s| s.is_populated()).unwrap_or(false) && !spec.is_empty();
            if ui
                .add_enabled(enabled, egui::Button::new(format!("{} Run spec check", p::ARROW_CLOCKWISE)))
                .clicked()
            {
                if let Some(snap) = snapshot {
                    let hw = collect_detected(snap);
                    self.spec_report = Some(compare(&spec, &hw));
                }
            }
            if let Some(report) = self.spec_report.as_ref() {
                if report.matched() {
                    ui.colored_label(GOOD, format!("{} Spec matches detected hardware", p::CHECK_CIRCLE));
                } else {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} {} mismatch(es)", p::X_CIRCLE, report.mismatch_count()),
                    );
                }
            }
        });

        let Some(report) = self.spec_report.as_ref() else {
            // Expected-spec preview before the first check.
            if !spec.is_empty() {
                for (label, value) in [
                    ("CPU", spec.cpu.as_str()),
                    ("GPU", spec.gpu.as_str()),
                    ("RAM", spec.ram.as_str()),
                ] {
                    if !value.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).strong().small());
                            ui.label(RichText::new(value).small());
                        });
                    }
                }
                for drive in &spec.drives {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Storage ({})", drive.kind)).strong().small());
                        ui.label(RichText::new(&drive.name).small());
                    });
                }
            }
            return;
        };

        // Expected vs detected, colored per row like the merge-modal diff.
        for row in &report.rows {
            let (status_color, detected_fill) = match row.status {
                CheckStatus::Match => (GOOD, Color32::from_rgb(30, 60, 55)),
                CheckStatus::Mismatch => (ui.visuals().error_fg_color, Color32::from_rgb(60, 30, 50)),
                CheckStatus::NotDetected => (CAUTION, Color32::from_rgb(55, 45, 25)),
                CheckStatus::NotSpecified => (ui.visuals().weak_text_color(), ui.visuals().faint_bg_color),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&row.component).strong().small());
                ui.colored_label(status_color, RichText::new(row.status.label()).small().monospace());
            });
            ui.horizontal(|ui| {
                let half = (ui.available_width() - 40.0) / 2.0;
                Frame::default()
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(4.0)
                    .inner_margin(Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.set_min_width(half);
                        ui.set_max_width(half);
                        let text = if row.expected.is_empty() { "(not on order)" } else { &row.expected };
                        ui.add(egui::Label::new(RichText::new(text).small()).wrap());
                    });
                ui.label("→");
                Frame::default()
                    .fill(detected_fill)
                    .corner_radius(4.0)
                    .inner_margin(Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.set_min_width(half);
                        ui.set_max_width(half);
                        let text = if row.detected.is_empty() { "(not detected)" } else { &row.detected };
                        ui.add(egui::Label::new(RichText::new(text).small()).wrap());
                    });
            });
            ui.add_space(4.0);
        }
    }

    fn ui_sign_off(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let is_repair = self
            .session
            .as_ref()
            .map(|s| s.order.kind == OrderKind::Repair)
            .unwrap_or(false);

        match self.tech.clone() {
            Some(tech) => {
                ui.horizontal(|ui| {
                    ui.colored_label(GOOD, format!("{} {} ({})", p::CHECK_CIRCLE, tech.name, tech.id_employee));
                    if ui.small_button("Sign out").clicked() {
                        self.tech = None;
                    }
                });
            }
            None => {
                let is_shopify = self
                    .session
                    .as_ref()
                    .map(|s| s.backend.backend_kind() == BackendKind::Shopify)
                    .unwrap_or(false);
                let (id_hint, secret_hint) = if is_shopify {
                    ("floor staff name", "PIN (unused)")
                } else {
                    ("employee email", "password")
                };
                ui.label(RichText::new("QC technician").strong().small());
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.tech_email)
                            .hint_text(id_hint)
                            .desired_width(170.0),
                    );
                    ui.add(
                        TextEdit::singleline(&mut self.tech_password)
                            .hint_text(secret_hint)
                            .password(true)
                            .desired_width(120.0),
                    );
                    if ui.add_enabled(!self.auth_busy, egui::Button::new("Sign in")).clicked() {
                        self.start_auth(ctx, false);
                    }
                    if self.auth_busy {
                        ui.spinner();
                    }
                });
                if let Some(e) = self.auth_error.as_ref() {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
            }
        }

        if is_repair {
            ui.add_space(6.0);
            ui.label(RichText::new("Repair orders need a second sign-off.").small().color(CAUTION));
            match self.signoff.clone() {
                Some(tech) => {
                    ui.horizontal(|ui| {
                        ui.colored_label(GOOD, format!("{} Sign-off: {} ({})", p::CHECK_CIRCLE, tech.name, tech.id_employee));
                        if ui.small_button("Clear").clicked() {
                            self.signoff = None;
                        }
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(&mut self.signoff_email)
                                .hint_text("sign-off email")
                                .desired_width(170.0),
                        );
                        ui.add(
                            TextEdit::singleline(&mut self.signoff_password)
                                .hint_text("password")
                                .password(true)
                                .desired_width(120.0),
                        );
                        if ui.add_enabled(!self.signoff_busy, egui::Button::new("Sign off")).clicked() {
                            self.start_auth(ctx, true);
                        }
                        if self.signoff_busy {
                            ui.spinner();
                        }
                    });
                    if let Some(e) = self.signoff_error.as_ref() {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                }
            }
        }
    }

    fn ui_comments(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let comments: Vec<OrderComment> = self
            .session
            .as_ref()
            .map(|s| s.comments.clone())
            .unwrap_or_default();
        let own_employee_id = self.tech.as_ref().map(|t| t.id_employee.clone());

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.comment_busy, egui::Button::new(format!("{} Refresh", p::ARROW_CLOCKWISE)))
                .clicked()
            {
                self.start_refresh_comments(ctx);
            }
            if self.comment_busy {
                ui.spinner();
            }
            ui.label(RichText::new(format!("{} comment(s)", comments.len())).weak().small());
        });

        ScrollArea::vertical()
            .id_salt("order_comments_scroll")
            .max_height(260.0)
            .stick_to_bottom(true)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for comment in &comments {
                    let own = own_employee_id.is_some()
                        && comment.author_employee_id == own_employee_id;
                    let layout = if own {
                        Layout::top_down(Align::Max)
                    } else {
                        Layout::top_down(Align::Min)
                    };
                    ui.with_layout(layout, |ui| {
                        let fill = if own {
                            ui.visuals().widgets.active.bg_fill
                        } else {
                            ui.visuals().widgets.active.weak_bg_fill
                        };
                        Frame::default()
                            .fill(fill)
                            .corner_radius(6.0)
                            .inner_margin(Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.set_max_width(ui.available_width() * 0.85);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&comment.author).strong().small());
                                    if comment.private {
                                        ui.label(RichText::new(p::LOCK).small())
                                            .on_hover_text("Private (staff only)");
                                    }
                                    if !comment.created_at.is_empty() {
                                        ui.label(RichText::new(&comment.created_at).weak().small());
                                    }
                                });
                                ui.add(egui::Label::new(RichText::new(&comment.body).small()).wrap());
                            });
                        ui.add_space(4.0);
                    });
                }
                if comments.is_empty() {
                    ui.label(RichText::new("No comments on this order.").weak());
                }
            });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let width = ui.available_width() - 70.0;
            ui.add(
                TextEdit::multiline(&mut self.comment_input)
                    .hint_text("Private staff note…")
                    .desired_rows(2)
                    .desired_width(width),
            );
            let can_send = !self.comment_busy && self.tech.is_some() && !self.comment_input.trim().is_empty();
            if ui
                .add_enabled(can_send, egui::Button::new(format!("{} Send", p::PAPER_PLANE_TILT)))
                .clicked()
            {
                self.start_post_comment(ctx);
            }
        });
        if self.tech.is_none() {
            ui.label(RichText::new("Sign in to post comments.").weak().small());
        }
        if let Some(e) = self.comment_error.as_ref() {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    fn ui_report(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        last_verdict: Option<&RunVerdict>,
        last_preset: Option<String>,
    ) {
        match last_verdict {
            Some(v) => {
                let (color, label) = match v.result {
                    RunResult::Pass => (GOOD, "PASS"),
                    RunResult::Fail => (ui.visuals().error_fg_color, "FAIL"),
                    _ => (CAUTION, v.result.as_str()),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Last stress run:").strong().small());
                    ui.colored_label(color, RichText::new(label).strong());
                    ui.label(
                        RichText::new(format!(
                            "{:.0}s · WHEA {} · TDR {} · mem/disk errs {}",
                            v.duration_secs,
                            v.summary.whea_delta_count,
                            v.summary.tdr_count,
                            v.summary.memory_errors + v.summary.disk_io_errors
                        ))
                        .small()
                        .weak(),
                    );
                });
            }
            None => {
                ui.label(
                    RichText::new("No stress run this session — report will submit as not_run.")
                        .small()
                        .color(CAUTION),
                );
            }
        }

        ui.add_space(6.0);
        egui::Grid::new("qc_checklist_grid")
            .num_columns(2)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                for pair in QcChecklist::LABELS.chunks(2) {
                    for (key, label) in pair {
                        if let Some(value) = self.checklist.field_mut(key) {
                            ui.checkbox(value, *label);
                        }
                    }
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        ui.add(
            TextEdit::multiline(&mut self.report_notes)
                .hint_text("QC notes…")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let can_submit = !self.submit_busy && self.tech.is_some();
            if ui
                .add_enabled(can_submit, egui::Button::new(format!("{} Submit QC report", p::CLIPBOARD_TEXT)))
                .clicked()
            {
                self.start_submit(ctx, last_verdict, last_preset.clone());
            }
            if self.submit_busy {
                ui.spinner();
            }
        });
        if self.tech.is_none() {
            ui.label(RichText::new("Sign in to submit.").weak().small());
        }
        if let Some((ok, msg)) = self.submit_result.as_ref() {
            let c = if *ok { GOOD } else { ui.visuals().error_fg_color };
            ui.colored_label(c, msg);
        }
    }
}
