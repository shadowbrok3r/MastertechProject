//! Order QC panel: lookup + status gate + items/serials + spec check +
//! tech sign-off + comments + photo check + status advance + QC submission.
//!
//! All backend traffic flows through `database::orders::QcBackend`, keeping
//! this panel renderer-agnostic enough to re-mount under a terminal-mode
//! front end later: state lives in [`OrderPanel`], async results arrive on
//! a channel as [`PanelMsg`].

use crossbeam::channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;

use database::orders::checklist::{ChecklistKind, ItemStatus};
use database::orders::{
    gate::GateOutcome, persist_qc_failures, persist_qc_report, BackendKind, BuildSpec,
    ChecklistState, GateDecision, OrderComment, OrderKey, OrderKind, OrderSummary, PhotoCheck,
    QcBackend, QcOrder, QcReportPayload, SerialHistorySummary, TechIdentity,
};
use database::schema::{RecordId, RecordIdExt, RunResult, TICKET_TABLE};
use eframe::egui::{
    self, Align, Color32, CollapsingHeader, Frame, Layout, Margin, RichText, ScrollArea, TextEdit,
};
use egui_phosphor::regular as p;
use stress_runner::RunVerdict;
use stress_kit::telemetry::TelemetrySnapshot;

use crate::driver_check::{DriverCheckRow, DriverStatus};
use crate::spec_check::{collect_detected, compare, CheckStatus, SpecCheckReport};

const GOOD: Color32 = Color32::from_rgb(61, 185, 157);
const CAUTION: Color32 = Color32::from_rgb(180, 140, 50);

/// Date portion of an ISO-8601 timestamp (`2026-06-15T17:54:35Z` → `2026-06-15`).
fn short_date(iso: Option<&str>) -> String {
    iso.map(|s| s.split('T').next().unwrap_or(s).to_string()).unwrap_or_default()
}

/// One-line summary stored on the signed-off worksheet.
fn verdict_summary(p: &QcReportPayload) -> String {
    format!("{} — {} failure(s)", p.verdict, p.failures.len())
}

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

/// Which signature slot an authentication targets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthRole {
    Tech,
    Signoff,
    Marketing,
    Executive,
}

enum PanelMsg {
    Loaded(Box<LoadedOrder>),
    LoadFailed(String),
    Auth { role: AuthRole, result: Result<TechIdentity, String> },
    CommentPosted(Box<Result<OrderComment, String>>),
    CommentsRefreshed(Result<Vec<OrderComment>, String>),
    Advanced(Result<i64, String>),
    Submitted(Result<String, String>),
    SerialHistory { serial: String, result: Result<SerialHistorySummary, String> },
    RecentLoaded(Result<Vec<OrderSummary>, String>),
    SerialResolved(Option<OrderSummary>),
    ProvDone { step: String, result: Result<String, String> },
    ProcStep { label: String, result: Result<String, String> },
    ProcDone,
    DmiRead(Result<crate::provisioning::dmi::DmiReadResult, String>),
    DriverCheck(Result<Vec<DriverCheckRow>, String>),
}

pub struct OrderPanel {
    key_input: String,
    busy: bool,
    error: Option<String>,
    session: Option<OrderSession>,

    /// Recent build-intake orders (Order Placed / Ready to Build) for the picker.
    recent: Option<Result<Vec<OrderSummary>, String>>,
    recent_busy: bool,
    /// How many recent orders to fetch; grows by 10 via "Load +10".
    recent_limit: usize,

    /// Order auto-detected from this machine's serial (prefill suggestion).
    resolved: Option<OrderSummary>,
    resolve_busy: bool,
    resolve_attempted: bool,
    /// First detected hardware serial, stamped onto the QC report for backfill.
    board_serial: Option<String>,

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

    /// Optional influencer-build second/third signatures (Marketing + Executive).
    is_influencer: bool,
    marketing_email: String,
    marketing_password: String,
    marketing: Option<TechIdentity>,
    marketing_busy: bool,
    marketing_error: Option<String>,
    executive_email: String,
    executive_password: String,
    executive: Option<TechIdentity>,
    executive_busy: bool,
    executive_error: Option<String>,

    comment_input: String,
    comment_busy: bool,
    comment_error: Option<String>,

    spec_report: Option<SpecCheckReport>,
    /// Runs the spec check once automatically after an order loads.
    spec_pending: bool,

    /// Federated serial history keyed by serial; `busy_serial` is the one in flight.
    serial_history: HashMap<String, Result<SerialHistorySummary, String>>,
    serial_busy: Option<String>,

    checklist: ChecklistState,
    checklist_kind: ChecklistKind,
    /// Runs checklist auto-verify (SMART/OA3/temps) once after an order loads.
    verify_pending: bool,
    /// Summary of a prior signed-off worksheet restored for this order, if any.
    prior_signoff: Option<String>,
    air_cooled: bool,
    /// Live items reset by sign-off re-verify that must be re-marked before submit.
    blocked_keys: Vec<String>,
    report_notes: String,
    submit_busy: bool,
    submit_result: Option<(bool, String)>,
    advance_busy: bool,
    advance_result: Option<(bool, String)>,

    /// Auto-Provision: company override, DMI tool path + confirm, step log.
    prov_company: Option<crate::provisioning::Company>,
    prov_busy: bool,
    prov_dmi_tool: String,
    prov_dmi_confirm: bool,
    prov_log: Vec<(String, bool, String)>,
    /// Procedure runner: selected kind, run-in-flight flag, VRChat asset tag.
    proc_kind: crate::provisioning::procedure::ProcedureKind,
    proc_running: bool,
    prov_asset_tag: String,
    cleanup_confirm: bool,
    /// Native SMBIOS read result for display.
    dmi_read: Option<crate::provisioning::dmi::DmiReadResult>,
    /// BIOS + OA3 key, read once per load.
    hw_info_pending: bool,
    bios_installed: Option<String>,
    bios_latest: Option<crate::provisioning::catalog_query::BiosInfo>,
    oa3_key: Option<String>,
    /// Per-part driver comparison (installed vs catalog target).
    driver_check: Option<Result<Vec<DriverCheckRow>, String>>,
    driver_check_busy: bool,

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
            recent: None,
            recent_busy: false,
            recent_limit: 10,
            resolved: None,
            resolve_busy: false,
            resolve_attempted: false,
            board_serial: None,
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
            is_influencer: false,
            marketing_email: String::new(),
            marketing_password: String::new(),
            marketing: None,
            marketing_busy: false,
            marketing_error: None,
            executive_email: String::new(),
            executive_password: String::new(),
            executive: None,
            executive_busy: false,
            executive_error: None,
            comment_input: String::new(),
            comment_busy: false,
            comment_error: None,
            spec_report: None,
            spec_pending: false,
            serial_history: HashMap::new(),
            serial_busy: None,
            checklist: ChecklistState::from_kind(ChecklistKind::BuildQc),
            checklist_kind: ChecklistKind::BuildQc,
            verify_pending: false,
            prior_signoff: None,
            air_cooled: false,
            blocked_keys: Vec::new(),
            report_notes: String::new(),
            submit_busy: false,
            submit_result: None,
            advance_busy: false,
            advance_result: None,
            prov_company: None,
            prov_busy: false,
            prov_dmi_tool: String::new(),
            prov_dmi_confirm: false,
            prov_log: Vec::new(),
            proc_kind: crate::provisioning::procedure::ProcedureKind::NewBuild,
            proc_running: false,
            prov_asset_tag: String::new(),
            cleanup_confirm: false,
            dmi_read: None,
            hw_info_pending: false,
            bios_installed: None,
            bios_latest: None,
            oa3_key: None,
            driver_check: None,
            driver_check_busy: false,
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

    /// Persist the in-progress checklist worksheet for this order + machine.
    fn save_worksheet(&self) {
        if let Some(s) = self.session.as_ref() {
            let machine = crate::reporting::machine_id();
            crate::checklist_store::save(&s.order.id, &machine, &self.checklist, false, "");
        }
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
                    // Build the order-appropriate checklist and overlay any
                    // saved per-machine worksheet for this order.
                    let kind = ChecklistKind::for_order_kind(order.kind);
                    let mut checklist = ChecklistState::from_kind(kind);
                    let machine = crate::reporting::machine_id();
                    let mut prior_signoff = None;
                    if let Some(restored) = crate::checklist_store::restore(&order.id, &machine) {
                        checklist.restore_from(&restored.checklist);
                        if restored.signed_off {
                            prior_signoff = Some(restored.summary);
                        }
                    }
                    self.prior_signoff = prior_signoff;
                    self.checklist_kind = kind;
                    self.air_cooled = checklist
                        .sections
                        .iter()
                        .find(|s| s.title.starts_with("Liquid Cooling"))
                        .map(|s| !s.applicable)
                        .unwrap_or(false);
                    self.checklist = checklist;
                    self.session = Some(OrderSession { backend, order, spec, gate, comments, photos });
                    self.spec_report = None;
                    self.spec_pending = true;
                    self.verify_pending = true;
                    self.serial_history.clear();
                    self.serial_busy = None;
                    self.blocked_keys.clear();
                    self.report_notes.clear();
                    self.submit_result = None;
                    self.advance_result = None;
                    self.signoff = None;
                    self.is_influencer = false;
                    self.marketing = None;
                    self.executive = None;
                    self.prov_company = None;
                    self.prov_log.clear();
                    self.prov_dmi_confirm = false;
                    self.prov_asset_tag.clear();
                    self.cleanup_confirm = false;
                    self.proc_running = false;
                    self.dmi_read = None;
                    self.driver_check = None;
                    self.driver_check_busy = false;
                    self.hw_info_pending = true;
                    self.bios_installed = None;
                    self.bios_latest = None;
                    self.oa3_key = None;
                    self.recent_limit = 10;
                }
                PanelMsg::LoadFailed(e) => {
                    self.busy = false;
                    self.error = Some(e);
                }
                PanelMsg::Auth { role, result } => match role {
                    AuthRole::Tech => {
                        self.auth_busy = false;
                        match result {
                            Ok(t) => { self.tech = Some(t); self.auth_error = None; self.tech_password.clear(); }
                            Err(e) => self.auth_error = Some(e),
                        }
                    }
                    AuthRole::Signoff => {
                        self.signoff_busy = false;
                        match result {
                            Ok(t) => { self.signoff = Some(t); self.signoff_error = None; self.signoff_password.clear(); }
                            Err(e) => self.signoff_error = Some(e),
                        }
                    }
                    AuthRole::Marketing => {
                        self.marketing_busy = false;
                        match result {
                            Ok(t) => self.set_influencer_slot(role, t),
                            Err(e) => self.marketing_error = Some(e),
                        }
                    }
                    AuthRole::Executive => {
                        self.executive_busy = false;
                        match result {
                            Ok(t) => self.set_influencer_slot(role, t),
                            Err(e) => self.executive_error = Some(e),
                        }
                    }
                },
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
                PanelMsg::SerialHistory { serial, result } => {
                    if self.serial_busy.as_deref() == Some(serial.as_str()) {
                        self.serial_busy = None;
                    }
                    self.serial_history.insert(serial, result);
                }
                PanelMsg::RecentLoaded(result) => {
                    self.recent_busy = false;
                    self.recent = Some(result);
                }
                PanelMsg::ProvDone { step, result } => {
                    self.prov_busy = false;
                    match result {
                        Ok(msg) => self.prov_log.push((step, true, msg)),
                        Err(e) => self.prov_log.push((step, false, e)),
                    }
                }
                PanelMsg::ProcStep { label, result } => match result {
                    Ok(msg) => self.prov_log.push((label, true, msg)),
                    Err(e) => self.prov_log.push((label, false, e)),
                },
                PanelMsg::ProcDone => self.proc_running = false,
                PanelMsg::DmiRead(result) => match result {
                    Ok(r) => self.dmi_read = Some(r),
                    Err(e) => self.prov_log.push(("DMI read".into(), false, e)),
                },
                PanelMsg::DriverCheck(result) => {
                    self.driver_check_busy = false;
                    self.driver_check = Some(result);
                }
                PanelMsg::SerialResolved(found) => {
                    self.resolve_busy = false;
                    if let Some(summary) = found {
                        // Prefill the lookup field; the tech confirms with Load.
                        if self.session.is_none() && self.key_input.trim().is_empty() {
                            self.key_input = summary.lookup_input();
                        }
                        self.resolved = Some(summary);
                    }
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

    fn start_recent(&mut self, ctx: &egui::Context) {
        self.recent_busy = true;
        let backend = QcBackend::shopify();
        let limit = self.recent_limit;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend.recent_orders(limit).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::RecentLoaded(result));
            ctx.request_repaint();
        });
    }

    /// Read this machine's serials (sync, fast) then reverse-lookup the order
    /// across backends, prefilling the lookup field. Runs once per session.
    fn start_resolve_from_hardware(&mut self, ctx: &egui::Context) {
        self.resolve_attempted = true;
        let serials = crate::hardware_id::read_machine_serials();
        self.board_serial = serials.first().cloned();
        if serials.is_empty() {
            return;
        }
        self.resolve_busy = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let found = database::orders::resolve_any(&serials).await;
            let _ = tx.send(PanelMsg::SerialResolved(found));
            ctx.request_repaint();
        });
    }

    /// Run a blocking provisioning step on a worker thread; result → `prov_log`.
    fn spawn_prov<F>(&mut self, step: &str, ctx: &egui::Context, f: F)
    where
        F: FnOnce() -> anyhow::Result<String> + Send + 'static,
    {
        self.prov_busy = true;
        let step = step.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = f().map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::ProvDone { step, result });
            ctx.request_repaint();
        });
    }

    /// Run a resolved procedure step-by-step on a worker thread; each result → `prov_log`.
    fn spawn_procedure(
        &mut self,
        ctx: &egui::Context,
        steps: Vec<crate::provisioning::procedure::ResolvedStep>,
        sqlite: String,
    ) {
        self.proc_running = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for step in steps {
                let label = step.label();
                let result = step.run(&sqlite).map_err(|e| format!("{e:#}"));
                let _ = tx.send(PanelMsg::ProcStep { label, result });
                ctx.request_repaint();
            }
            let _ = tx.send(PanelMsg::ProcDone);
            ctx.request_repaint();
        });
    }

    /// DMI step inputs for a procedure run, gated on manufacturer + confirm + tool path.
    fn build_dmi_inputs(
        &self,
        order: &QcOrder,
        spec: &BuildSpec,
        company: crate::provisioning::Company,
        manifest: &crate::provisioning::CompanyManifest,
    ) -> Option<crate::provisioning::procedure::DmiInputs> {
        use crate::provisioning::dmi;
        if company.dmi_manufacturer().is_none() || dmi::is_threadripper(spec) {
            return None;
        }
        if !self.prov_dmi_confirm || self.prov_dmi_tool.trim().is_empty() {
            return None;
        }
        let serial = self.board_serial.clone().unwrap_or_default();
        let dctx = dmi::DmiContext::build(order, spec, manifest, "", &serial);
        Some(crate::provisioning::procedure::DmiInputs {
            tool: std::path::PathBuf::from(self.prov_dmi_tool.clone()),
            cmds: dmi::ami_commands(&dctx),
        })
    }

    /// Installed vs latest BIOS, with a link to the manufacturer page.
    fn ui_bios(&mut self, ui: &mut egui::Ui) {
        match self.bios_installed.as_deref() {
            Some(v) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Installed").strong().small());
                    ui.label(RichText::new(v).monospace().small());
                });
            }
            None => {
                ui.label(RichText::new("Installed BIOS version unavailable.").weak().small());
            }
        }
        match self.bios_latest.as_ref() {
            Some(bios) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Latest").strong().small());
                    if let Some(f) = bios.file_name.as_ref() {
                        ui.label(RichText::new(f).monospace().small());
                    }
                    if !bios.url_webpage.is_empty() {
                        ui.hyperlink_to("manufacturer page", &bios.url_webpage);
                    }
                });
            }
            None => {
                ui.label(RichText::new("No catalog BIOS entry for this board.").weak().small());
            }
        }
    }

    /// Per-part driver comparison: installed (WMI) vs catalog target + missing list.
    fn ui_driver_check(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.driver_check.is_none() && !self.driver_check_busy {
            self.start_driver_check(ctx);
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.driver_check_busy,
                    egui::Button::new(format!("{} Re-check drivers", p::ARROW_CLOCKWISE)),
                )
                .clicked()
            {
                self.driver_check = None;
                self.start_driver_check(ctx);
            }
            if self.driver_check_busy {
                ui.spinner();
                ui.label(RichText::new("scanning installed drivers…").weak().small());
            }
        });

        match self.driver_check.as_ref() {
            None => {}
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Some(Ok(rows)) if rows.is_empty() => {
                ui.label(RichText::new("No catalog driver mapping for this board.").weak().small());
            }
            Some(Ok(rows)) => {
                let missing: Vec<&str> = rows
                    .iter()
                    .filter(|r| r.status == DriverStatus::Missing)
                    .map(|r| r.category.as_str())
                    .collect();
                let outdated: Vec<&str> = rows
                    .iter()
                    .filter(|r| r.status == DriverStatus::Outdated)
                    .map(|r| r.category.as_str())
                    .collect();
                if missing.is_empty() && outdated.is_empty() {
                    ui.colored_label(GOOD, format!("{} All drivers present and current", p::CHECK_CIRCLE));
                } else {
                    if !missing.is_empty() {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("{} Missing: {}", p::X_CIRCLE, missing.join(", ")),
                        );
                    }
                    if !outdated.is_empty() {
                        ui.colored_label(
                            CAUTION,
                            format!("{} Outdated: {}", p::WARNING, outdated.join(", ")),
                        );
                    }
                }
                ui.add_space(4.0);
                egui_extras::TableBuilder::new(ui)
                    .id_salt("driver_check_table")
                    .striped(true)
                    .column(egui_extras::Column::initial(120.0).at_least(70.0).clip(true))
                    .column(egui_extras::Column::remainder().at_least(150.0).clip(true))
                    .column(egui_extras::Column::initial(150.0).at_least(90.0).clip(true))
                    .column(egui_extras::Column::exact(70.0))
                    .header(20.0, |mut h| {
                        h.col(|ui| { ui.strong("Part"); });
                        h.col(|ui| { ui.strong("Installed"); });
                        h.col(|ui| { ui.strong("Catalog target"); });
                        h.col(|ui| { ui.strong("Status"); });
                    })
                    .body(|mut body| {
                        for r in rows {
                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(RichText::new(&r.category).strong().small());
                                });
                                row.col(|ui| {
                                    let txt = match (&r.installed_name, &r.installed_version) {
                                        (Some(n), Some(v)) => format!("{n}  v{v}"),
                                        (Some(n), None) => n.clone(),
                                        _ => "—".to_string(),
                                    };
                                    ui.label(RichText::new(txt).small());
                                });
                                row.col(|ui| {
                                    let tgt = match (r.target_file.as_deref(), r.target_version.as_deref()) {
                                        (Some(f), Some(v)) => format!("{f}  (v{v})"),
                                        (Some(f), None) => f.to_string(),
                                        _ => "—".to_string(),
                                    };
                                    ui.label(RichText::new(tgt).monospace().small());
                                });
                                row.col(|ui| {
                                    let (c, t) = match r.status {
                                        DriverStatus::Installed => (GOOD, "installed"),
                                        DriverStatus::Outdated => (CAUTION, "OUTDATED"),
                                        DriverStatus::Missing => (ui.visuals().error_fg_color, "MISSING"),
                                        DriverStatus::NoTarget => (CAUTION, "info"),
                                    };
                                    ui.colored_label(c, RichText::new(t).small());
                                });
                            });
                        }
                    });
            }
        }
    }

    /// Gather installed drivers (WMI) + catalog targets (SQLite) on a worker thread.
    fn start_driver_check(&mut self, ctx: &egui::Context) {
        let product = {
            let Some(session) = self.session.as_ref() else { return };
            session
                .spec
                .motherboard
                .clone()
                .or_else(crate::hardware_id::read_baseboard_product)
                .unwrap_or_default()
        };
        self.driver_check_busy = true;
        let sqlite = crate::db::default_sqlite_path();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Vec<DriverCheckRow>, String> {
                let installed = crate::diagnostics::installed_drivers();
                let conn = crate::db::open_or_create(&sqlite).map_err(|e| format!("{e:#}"))?;
                let package = if product.is_empty() {
                    None
                } else {
                    crate::provisioning::catalog_query::package_drivers_for_baseboard(&conn, &product)
                        .map_err(|e| format!("{e:#}"))?
                }
                .unwrap_or_default();
                let mut gpu_targets = Vec::new();
                for code in crate::hardware_id::read_gpu_device_codes() {
                    if let Ok(Some(row)) =
                        crate::provisioning::catalog_query::gpu_driver_for_device(&conn, &code)
                    {
                        gpu_targets.push(crate::provisioning::catalog_query::TargetDriver {
                            file: row.file_name,
                            version: row.version,
                        });
                    }
                }
                Ok(crate::driver_check::build_driver_check(&installed, &package, &gpu_targets))
            })();
            let _ = tx.send(PanelMsg::DriverCheck(result));
            ctx.request_repaint();
        });
    }

    /// Validate + record an influencer signature, gating on PrestaShop profile
    /// (26 Marketing / 15 Executive). Shopify staff carry no profile → name-only.
    fn set_influencer_slot(&mut self, role: AuthRole, identity: TechIdentity) {
        let required = match role {
            AuthRole::Marketing => "26",
            AuthRole::Executive => "15",
            _ => return,
        };
        if let Some(profile) = identity.id_profile.as_deref() {
            if profile != required {
                let msg = format!("{} is not authorized for this signature (profile {profile}).", identity.name);
                match role {
                    AuthRole::Marketing => self.marketing_error = Some(msg),
                    AuthRole::Executive => self.executive_error = Some(msg),
                    _ => {}
                }
                return;
            }
        }
        match role {
            AuthRole::Marketing => {
                self.marketing = Some(identity);
                self.marketing_error = None;
                self.marketing_password.clear();
            }
            AuthRole::Executive => {
                self.executive = Some(identity);
                self.executive_error = None;
                self.executive_password.clear();
            }
            _ => {}
        }
    }

    fn auth_inputs(&self, role: AuthRole) -> (String, String) {
        match role {
            AuthRole::Tech => (self.tech_email.clone(), self.tech_password.clone()),
            AuthRole::Signoff => (self.signoff_email.clone(), self.signoff_password.clone()),
            AuthRole::Marketing => (self.marketing_email.clone(), self.marketing_password.clone()),
            AuthRole::Executive => (self.executive_email.clone(), self.executive_password.clone()),
        }
    }

    fn set_auth_busy(&mut self, role: AuthRole, busy: bool) {
        match role {
            AuthRole::Tech => self.auth_busy = busy,
            AuthRole::Signoff => self.signoff_busy = busy,
            AuthRole::Marketing => self.marketing_busy = busy,
            AuthRole::Executive => self.executive_busy = busy,
        }
    }

    fn set_auth_error(&mut self, role: AuthRole, err: Option<String>) {
        match role {
            AuthRole::Tech => self.auth_error = err,
            AuthRole::Signoff => self.signoff_error = err,
            AuthRole::Marketing => self.marketing_error = err,
            AuthRole::Executive => self.executive_error = err,
        }
    }

    fn start_auth(&mut self, ctx: &egui::Context, role: AuthRole) {
        let Some(backend) = self.session.as_ref().map(|s| s.backend.clone()) else { return };
        let (email, password) = self.auth_inputs(role);
        // Shopify identity is a roster name match; no PIN verification exists yet.
        let needs_password = backend.backend_kind() == BackendKind::Prestashop;
        if email.trim().is_empty() || (needs_password && password.is_empty()) {
            let err = Some(if needs_password {
                "Email and password required.".to_string()
            } else {
                "Name required.".to_string()
            });
            self.set_auth_error(role, err);
            return;
        }
        self.set_auth_busy(role, true);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend
                .authenticate_tech(email.trim(), &password)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::Auth { role, result });
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

    /// Run SysPrep file cleanup then advance the order to Ready to Ship.
    fn start_sysprep_cleanup(&mut self, ctx: &egui::Context) {
        let Some(session) = self.session.as_ref() else { return };
        self.prov_busy = true;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = crate::provisioning::cleanup::sysprep_cleanup(&order, &backend)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::ProvDone {
                step: "SysPrep cleanup + Ready to Ship".into(),
                result,
            });
            ctx.request_repaint();
        });
    }

    fn start_serial_history(&mut self, ctx: &egui::Context, serial: String) {
        let Some(session) = self.session.as_ref() else { return };
        self.serial_busy = Some(serial.clone());
        let backend = session.backend.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = backend.serial_history(&serial).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::SerialHistory { serial, result });
            ctx.request_repaint();
        });
    }

    fn start_submit(&mut self, ctx: &egui::Context, last_verdict: Option<&RunVerdict>, preset: Option<String>) {
        // Snapshot session-derived data, then release the borrow for the
        // mutable re-verify pass below.
        let Some(session) = self.session.as_ref() else { return };
        let order = session.order.clone();
        let backend = session.backend.clone();
        let backend_kind = backend.backend_kind();
        let order_key = order.key.as_ref().map(|k| k.display().to_string()).unwrap_or_default();
        let order_kind = order.kind;

        let Some(tech) = self.tech.clone() else {
            self.submit_result = Some((false, "Sign in before submitting the QC report.".into()));
            return;
        };
        if order_kind == OrderKind::Repair && self.signoff.is_none() {
            self.submit_result = Some((false, "Repair orders need the second sign-off before submitting.".into()));
            return;
        }
        if self.is_influencer && (self.marketing.is_none() || self.executive.is_none()) {
            self.submit_result = Some((false, "Influencer build needs both Marketing and Executive signatures.".into()));
            return;
        }
        if !self.checklist.is_complete() {
            self.submit_result = Some((false, "Checklist incomplete — every applicable item needs Pass / Fail / N/A (Fails need a note).".into()));
            return;
        }

        // Re-verify live auto items; reset any that drifted and block submit.
        let probe = crate::checklist_verify::WmiProbe::new();
        let stale = crate::checklist_verify::reverify_at_signoff(&mut self.checklist, &probe);
        if !stale.is_empty() {
            self.blocked_keys = stale.clone();
            self.submit_result = Some((
                false,
                format!("Changed since the auto-check — re-mark before sign-off: {}", stale.join(", ")),
            ));
            self.save_worksheet();
            return;
        }
        self.blocked_keys.clear();

        let machine = crate::reporting::machine_id();
        let failures = self.checklist.failures(&order.id);

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
            order_key,
            order_id: order.id.clone(),
            backend: backend_kind.as_str().to_string(),
            verdict,
            preset,
            machine_id: Some(machine.clone()),
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
            checklist: self.checklist.clone(),
            checklist_type: self.checklist_kind.as_str().to_string(),
            is_influencer: self.is_influencer,
            marketing_employee_id: self.marketing.as_ref().map(|t| t.id_employee.clone()),
            executive_employee_id: self.executive.as_ref().map(|t| t.id_employee.clone()),
            failures,
            board_serial: self.board_serial.clone(),
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

        // Persist the signed-off worksheet as the on-machine audit copy.
        crate::checklist_store::save(&order.id, &machine, &self.checklist, true, &verdict_summary(&payload));

        self.submit_busy = true;
        self.submit_result = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let persisted = persist_qc_report(&payload).await;
            let pushed = backend.submit_qc(&order, &payload).await;
            let result = match (persisted, pushed) {
                (Ok(id), pushed) => {
                    let failures_written = persist_qc_failures(&id, &payload).await.unwrap_or(0);
                    let mut msg = format!(
                        "QC report saved ({}, {failures_written} failure row(s)).",
                        id.key_string()
                    );
                    match pushed {
                        Ok(()) => msg.push_str(&format!(" Pushed to {}.", backend_kind.as_str())),
                        Err(e) => msg.push_str(&format!(" Backend push failed: {e:#}")),
                    }
                    Ok(msg)
                }
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
            if self.session.is_some()
                && ui.button(format!("{} Back to list", p::ARROW_LEFT)).clicked()
            {
                self.session = None;
                self.error = None;
                self.recent_limit = 10;
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
            // Auto-resolve the order from this machine's serial, once.
            if !self.resolve_attempted && !self.resolve_busy {
                self.start_resolve_from_hardware(&ctx);
            }
            if self.resolve_busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Detecting order from machine serial…").weak().small());
                });
            }
            if let Some(s) = self.resolved.clone() {
                Frame::default()
                    .fill(GOOD.gamma_multiply(0.12))
                    .stroke((1.0, GOOD))
                    .corner_radius(6.0)
                    .inner_margin(Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.colored_label(GOOD, RichText::new(p::DESKTOP).size(14.0));
                            ui.label(
                                RichText::new(format!(
                                    "Detected on this machine: {} {}",
                                    s.reference,
                                    if s.customer_name.is_empty() { String::new() } else { format!("({})", s.customer_name) }
                                ))
                                .small(),
                            );
                            if ui.button("Load detected").clicked() {
                                self.key_input = s.lookup_input();
                                self.start_load(&ctx);
                            }
                        });
                    });
                ui.add_space(6.0);
            }
            if self.recent.is_none() && !self.recent_busy {
                self.start_recent(&ctx);
            }
            self.ui_recent_orders(ui, &ctx);
            ui.add_space(12.0);
            ui.label(
                RichText::new(
                    "Or look up any order above. Loading begins QC: gate check, items + serials, \
                     spec verification, sign-off, comments, and report submission.",
                )
                .weak()
                .small(),
            );
            return;
        }

        // BIOS + OA3 key, read once per load.
        if self.hw_info_pending {
            self.hw_info_pending = false;
            self.bios_installed = crate::hardware_id::read_bios_version();
            self.oa3_key = crate::hardware_id::read_oa3_product_key();
            let product = self
                .session
                .as_ref()
                .and_then(|s| s.spec.motherboard.clone())
                .or_else(crate::hardware_id::read_baseboard_product)
                .unwrap_or_default();
            if !product.is_empty() {
                let path = crate::db::default_sqlite_path();
                if let Ok(conn) = crate::db::open_or_create(&path) {
                    self.bios_latest =
                        crate::provisioning::catalog_query::bios_info_for_baseboard(&conn, &product)
                            .ok()
                            .flatten();
                }
            }
        }

        // Order comments (top) + QC report (bottom) live in a right side panel.
        egui::Panel::right("qc_side_panel")
            .resizable(true)
            .default_size(400.0)
            .show_inside(ui, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    CollapsingHeader::new(format!("{} Order comments", p::CHAT))
                        .default_open(true)
                        .show(ui, |ui| self.ui_comments(ui, &ctx));
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    CollapsingHeader::new(format!("{} QC report", p::CLIPBOARD_TEXT))
                        .default_open(true)
                        .show(ui, |ui| self.ui_report(ui, &ctx, last_verdict, last_preset));
                });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // Order info (left) + sign-off card (right).
                ui.columns(2, |cols| {
                    self.ui_order_header(&mut cols[0]);
                    self.ui_sign_off(&mut cols[1], &ctx);
                });
                self.ui_gate_banner(ui, &ctx);
                CollapsingHeader::new(format!("{} Items & serials", p::PACKAGE))
                    .default_open(true)
                    .show(ui, |ui| self.ui_items(ui));
                CollapsingHeader::new(format!("{} Spec check", p::CPU))
                    .default_open(true)
                    .show(ui, |ui| self.ui_spec_check(ui, snapshot));
                CollapsingHeader::new(format!("{} BIOS", p::CPU))
                    .default_open(false)
                    .show(ui, |ui| self.ui_bios(ui));
                CollapsingHeader::new(format!("{} Driver check", p::DOWNLOAD_SIMPLE))
                    .default_open(false)
                    .show(ui, |ui| self.ui_driver_check(ui, &ctx));
                CollapsingHeader::new(format!("{} Auto-Provision", p::WRENCH))
                    .default_open(false)
                    .show(ui, |ui| self.ui_provision(ui, &ctx));
                ui.add_space(24.0);
            });
        });
    }

    /// Recent build-intake orders picker shown before an order is loaded.
    fn ui_recent_orders(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} Recent build-intake orders", p::LIST)).strong().size(15.0));
            if ui
                .add_enabled(!self.recent_busy, egui::Button::new(format!("{} Refresh", p::ARROW_CLOCKWISE)))
                .clicked()
            {
                self.start_recent(ctx);
            }
            if ui
                .add_enabled(!self.recent_busy, egui::Button::new(format!("{} Load +10", p::PLUS)))
                .clicked()
            {
                self.recent_limit += 10;
                self.start_recent(ctx);
            }
            if self.recent_busy {
                ui.spinner();
            }
        });
        ui.label(
            RichText::new("Last 10 Shopify orders in Order Placed or Ready to Build — click one to load it for QC.")
                .weak()
                .small(),
        );
        ui.add_space(6.0);

        let mut load: Option<String> = None;
        match self.recent.as_ref() {
            None => {
                if !self.recent_busy {
                    ui.label(RichText::new("Loading…").weak());
                }
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Some(Ok(orders)) if orders.is_empty() => {
                ui.label(RichText::new("No orders in Order Placed or Ready to Build right now.").weak());
            }
            Some(Ok(orders)) => {
                egui_extras::TableBuilder::new(ui)
                    .id_salt("recent_orders_table")
                    .striped(true)
                    .column(egui_extras::Column::exact(72.0))
                    .column(egui_extras::Column::initial(190.0).at_least(120.0).clip(true))
                    .column(egui_extras::Column::remainder().at_least(120.0).clip(true))
                    .column(egui_extras::Column::initial(150.0).at_least(90.0).clip(true))
                    .column(egui_extras::Column::exact(56.0))
                    .column(egui_extras::Column::exact(92.0))
                    .header(20.0, |mut header| {
                        header.col(|ui| { ui.strong("Order"); });
                        header.col(|ui| { ui.strong("Status"); });
                        header.col(|ui| { ui.strong("Customer"); });
                        header.col(|ui| { ui.strong("Build"); });
                        header.col(|ui| { ui.strong("Serials"); });
                        header.col(|ui| { ui.strong("Placed"); });
                    })
                    .body(|mut body| {
                        for order in orders {
                            body.row(22.0, |mut row| {
                                row.col(|ui| {
                                    let label =
                                        if order.reference.is_empty() { &order.id } else { &order.reference };
                                    if ui
                                        .add(egui::Button::new(RichText::new(label).monospace()).small())
                                        .on_hover_text("Load this order for QC")
                                        .clicked()
                                    {
                                        load = Some(order.lookup_input());
                                    }
                                });
                                row.col(|ui| { ui.label(RichText::new(&order.status.name).small()); });
                                row.col(|ui| { ui.label(RichText::new(&order.customer_name).small()); });
                                row.col(|ui| { ui.label(RichText::new(&order.model).small()); });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}/{}",
                                            order.attached_serials, order.expected_serials
                                        ))
                                        .small()
                                        .monospace(),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(RichText::new(short_date(order.created_at.as_deref())).small());
                                });
                            });
                        }
                    });
            }
        }

        if let Some(input) = load {
            self.key_input = input;
            self.start_load(ctx);
        }
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
                if let Some(key) = self.oa3_key.as_ref() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("OA3 key").weak().small());
                        ui.label(RichText::new(key).monospace().small());
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
        // Clone the rows so the table closures don't hold a borrow of `self`,
        // freeing it for the serial-history lookup + results render below.
        let items = session.order.items.clone();
        let is_shopify = session.order.backend == Some(BackendKind::Shopify);
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

        let unattached: Vec<_> = items.iter().filter(|i| !i.serial_attached()).collect();
        if !unattached.is_empty() {
            Frame::default()
                .fill(CAUTION.gamma_multiply(0.12))
                .stroke((1.0, CAUTION))
                .corner_radius(6.0)
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.colored_label(
                        CAUTION,
                        RichText::new(format!(
                            "{} {} item(s) not yet committed (no serial)",
                            p::WARNING,
                            unattached.len()
                        ))
                        .strong()
                        .small(),
                    );
                    for it in &unattached {
                        ui.label(RichText::new(format!("• {} ({})", it.name, it.reference)).small());
                    }
                });
            ui.add_space(4.0);
        }

        let mut lookup: Option<Vec<String>> = None;
        egui_extras::TableBuilder::new(ui)
            .id_salt("qc_items_table")
            .striped(true)
            .column(egui_extras::Column::exact(22.0))
            .column(egui_extras::Column::initial(150.0).at_least(80.0).clip(true))
            .column(egui_extras::Column::initial(130.0).at_least(110.0).clip(true))
            .column(egui_extras::Column::exact(34.0))
            .column(egui_extras::Column::remainder().at_least(110.0))
            .header(20.0, |mut header| {
                header.col(|_| {});
                header.col(|ui| { ui.strong("Item"); });
                header.col(|ui| { ui.strong("Ref"); });
                header.col(|ui| { ui.strong("Qty"); });
                header.col(|ui| { ui.strong("Serial"); });
            })
            .body(|mut body| {
                for item in &items {
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
                                return;
                            }
                            ui.horizontal(|ui| {
                                // Lookup button leftmost so it aligns across rows.
                                // Federated history only exists on the Shopify/XBM side.
                                if is_shopify
                                    && ui
                                        .small_button(p::MAGNIFYING_GLASS)
                                        .on_hover_text("Serial history (Shopify · Odoo · PrestaShop)")
                                        .clicked()
                                {
                                    lookup = Some(item.serials.clone());
                                }
                                ui.label(RichText::new(item.serials.join(", ")).monospace().small());
                            });
                        });
                    });
                }
            });

        self.ui_serial_history(ui);

        if let Some(serials) = lookup {
            let ctx = ui.ctx().clone();
            for serial in serials {
                self.start_serial_history(&ctx, serial);
            }
        }
    }

    /// Federated serial-history results gathered this session.
    fn ui_serial_history(&self, ui: &mut egui::Ui) {
        if self.serial_history.is_empty() && self.serial_busy.is_none() {
            return;
        }
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Serial history").strong().small());
        if let Some(busy) = self.serial_busy.as_ref() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(format!("looking up {busy}…")).small().weak());
            });
        }
        for (serial, result) in &self.serial_history {
            match result {
                Ok(h) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(serial).monospace().small().strong());
                        if !h.found {
                            ui.colored_label(CAUTION, RichText::new("not found in any system").small());
                        }
                        if let Some(order) = h.current_order.as_ref() {
                            ui.label(RichText::new(format!("installed on {order}")).small());
                        }
                        if let Some(lot) = h.odoo_lot.as_ref() {
                            ui.label(RichText::new(format!("Odoo: {lot}")).small().weak());
                        }
                        if h.prestashop_allocations > 0 {
                            ui.label(
                                RichText::new(format!("PS allocs: {}", h.prestashop_allocations)).small().weak(),
                            );
                        }
                    });
                    for flag in &h.flags {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            RichText::new(format!("{} {flag}", p::WARNING)).small(),
                        );
                    }
                }
                Err(e) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        RichText::new(format!("{serial}: {e}")).small(),
                    );
                }
            }
        }
    }

    fn ui_spec_check(&mut self, ui: &mut egui::Ui, snapshot: Option<&TelemetrySnapshot>) {
        let Some(session) = self.session.as_ref() else { return };
        let spec = session.spec.clone();

        // Auto-run once after load, as soon as telemetry is populated.
        if self.spec_pending && !spec.is_empty() {
            if let Some(snap) = snapshot.filter(|s| s.is_populated()) {
                let hw = collect_detected(snap);
                self.spec_report = Some(compare(&spec, &hw));
                self.spec_pending = false;
            }
        }

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
                ui.label(p::ARROW_RIGHT);
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

    fn ui_provision(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::provisioning::{self, Company};
        let Some((order, spec)) = self.session.as_ref().map(|s| (s.order.clone(), s.spec.clone())) else {
            return;
        };
        let company = self.prov_company.unwrap_or_else(|| Company::from_order(&order));
        let manifest = provisioning::load_manifest(company);
        let sqlite = crate::db::default_sqlite_path().to_string_lossy().to_string();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Company").small());
            egui::ComboBox::from_id_salt("prov_company")
                .selected_text(company.label())
                .show_ui(ui, |ui| {
                    for c in Company::ALL {
                        if ui.selectable_label(company == c, c.label()).clicked() {
                            self.prov_company = Some(c);
                        }
                    }
                });
            if self.prov_busy {
                ui.spinner();
            }
        });
        ui.label(RichText::new("OS-config steps run on click; DMI writes need the confirm box.").weak().small());

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Procedure").small());
            egui::ComboBox::from_id_salt("proc_kind")
                .selected_text(self.proc_kind.label())
                .show_ui(ui, |ui| {
                    for k in crate::provisioning::procedure::ProcedureKind::ALL {
                        if ui.selectable_label(self.proc_kind == k, k.label()).clicked() {
                            self.proc_kind = k;
                        }
                    }
                });
            let can_run = !self.proc_running && !self.prov_busy;
            if ui
                .add_enabled(can_run, egui::Button::new(format!("{} Run full procedure", p::PLAY)))
                .clicked()
            {
                let dmi_inputs = self.build_dmi_inputs(&order, &spec, company, &manifest);
                if dmi_inputs.is_none()
                    && company.dmi_manufacturer().is_some()
                    && !crate::provisioning::dmi::is_threadripper(&spec)
                {
                    self.prov_log.push((
                        "DMI".into(),
                        false,
                        "skipped — tick the confirm box and set the tool path to include DMI".into(),
                    ));
                }
                let steps = crate::provisioning::procedure::resolve(
                    self.proc_kind, &manifest, company, &spec, dmi_inputs, &self.prov_asset_tag,
                );
                self.spawn_procedure(ctx, steps, sqlite.clone());
            }
            if self.proc_running {
                ui.spinner();
            }
        });
        if company == Company::VrChat {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Asset tag").small());
                ui.add(
                    TextEdit::singleline(&mut self.prov_asset_tag)
                        .hint_text("VRChat asset tag")
                        .desired_width(160.0),
                );
            });
        }
        ui.separator();

        for step in &manifest.steps {
            match step.kind.as_str() {
                "core_isolation" => {
                    if ui.add_enabled(!self.prov_busy, egui::Button::new(format!("{} Enable core isolation", p::LOCK))).clicked() {
                        self.spawn_prov("Core isolation", ctx, provisioning::osconfig::enable_core_isolation);
                    }
                }
                "timezone" => {
                    if ui.add_enabled(!self.prov_busy, egui::Button::new(format!("{} Set timezone (MST)", p::CLOCK))).clicked() {
                        self.spawn_prov("Timezone", ctx, provisioning::osconfig::set_timezone_mountain);
                    }
                }
                "open_tools" => {
                    if ui.add_enabled(!self.prov_busy, egui::Button::new(format!("{} Open system tools", p::DESKTOP))).clicked() {
                        self.spawn_prov("Open tools", ctx, provisioning::osconfig::open_system_tools);
                    }
                }
                "chipset" => {
                    if ui.add_enabled(!self.prov_busy, egui::Button::new("Install chipset driver")).clicked() {
                        let path = sqlite.clone();
                        self.spawn_prov("Chipset driver", ctx, move || provisioning::install_chipset(&path));
                    }
                }
                "display" => {
                    if ui.add_enabled(!self.prov_busy, egui::Button::new("Install display driver")).clicked() {
                        let path = sqlite.clone();
                        self.spawn_prov("Display driver", ctx, move || provisioning::install_display(&path));
                    }
                }
                "dmi" => self.ui_dmi(ui, ctx, &order, &spec, company, &manifest),
                "branding" => {
                    ui.label(RichText::new("Branding (.bat) — later phase").weak().small());
                }
                _ => {}
            }
        }

        // Conditional software (manifest `when` DSL).
        let applicable: Vec<provisioning::manifest::SoftwareSpec> =
            provisioning::software::plan(&manifest, &spec).into_iter().cloned().collect();
        if !applicable.is_empty() {
            CollapsingHeader::new("Conditional software").id_salt("prov_software").show(ui, |ui| {
                if ui
                    .add_enabled(!self.proc_running && !self.prov_busy, egui::Button::new("Install all applicable"))
                    .clicked()
                {
                    let steps = applicable
                        .iter()
                        .cloned()
                        .map(crate::provisioning::procedure::ResolvedStep::Software)
                        .collect();
                    self.spawn_procedure(ctx, steps, sqlite.clone());
                }
                for s in &applicable {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&s.id).small());
                        ui.label(RichText::new(format!("when: {}", s.when)).weak().small());
                        if ui.add_enabled(!self.prov_busy, egui::Button::new("Install")).clicked() {
                            let spec_c = s.clone();
                            self.spawn_prov(&format!("Software: {}", s.id), ctx, move || {
                                provisioning::software::install(&spec_c)
                            });
                        }
                    });
                }
            });
        }

        // Vendor-specific + cleanup actions.
        CollapsingHeader::new("Vendor & cleanup").id_salt("prov_vendor").show(ui, |ui| {
            if matches!(company, Company::Bimbox) {
                if ui.add_enabled(!self.prov_busy, egui::Button::new("Remove At-Home Support")).clicked() {
                    self.spawn_prov("Remove At-Home Support", ctx, provisioning::vendor_steps::remove_at_home_support);
                }
                if ui.add_enabled(!self.prov_busy, egui::Button::new("Remove Edge favorites")).clicked() {
                    self.spawn_prov("Remove Edge favorites", ctx, provisioning::vendor_steps::remove_edge_favorites);
                }
            }
            if matches!(company, Company::VrChat) {
                let tag = self.prov_asset_tag.clone();
                let can = !self.prov_busy && !tag.trim().is_empty();
                if ui.add_enabled(can, egui::Button::new("Run VRChat installer")).clicked() {
                    self.spawn_prov("VRChat installer", ctx, move || {
                        provisioning::vendor_steps::install_vrchat_custom(&tag)
                    });
                }
            }
            ui.separator();
            ui.checkbox(&mut self.cleanup_confirm, "I'm ready to clean up this machine");
            let can_clean = self.cleanup_confirm && !self.prov_busy;
            if ui
                .add_enabled(can_clean, egui::Button::new(format!("{} SysPrep cleanup + Ready to Ship", p::WARNING)))
                .clicked()
            {
                self.start_sysprep_cleanup(ctx);
            }
            if ui.add_enabled(can_clean, egui::Button::new(format!("{} SysPrep cleanup (files)", p::WARNING))).clicked() {
                self.spawn_prov("SysPrep cleanup", ctx, provisioning::cleanup::sysprep_files_only);
            }
            if ui.add_enabled(can_clean, egui::Button::new(format!("{} Service cleanup", p::WARNING))).clicked() {
                self.spawn_prov("Service cleanup", ctx, provisioning::cleanup::service_cleanup);
            }
        });

        // System utilities.
        CollapsingHeader::new("Utilities").id_salt("prov_utils").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Wi-Fi settings").clicked() {
                    self.spawn_prov("Wi-Fi settings", ctx, provisioning::osconfig::open_wifi_settings);
                }
                if ui.button("Install share").clicked() {
                    self.spawn_prov("Install share", ctx, provisioning::osconfig::open_share_browser);
                }
                if ui.button("Windows Update").clicked() {
                    self.spawn_prov("Windows Update", ctx, provisioning::osconfig::open_windows_update);
                }
                if ui.button("Start menu fix").clicked() {
                    self.spawn_prov("Start menu fix", ctx, provisioning::osconfig::fix_start_menu);
                }
            });
        });

        if !self.prov_log.is_empty() {
            ui.add_space(4.0);
            for (s, ok, msg) in &self.prov_log {
                let c = if *ok { GOOD } else { ui.visuals().error_fg_color };
                ui.colored_label(c, RichText::new(format!("{s}: {msg}")).small());
            }
        }
    }

    fn ui_dmi(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        order: &QcOrder,
        spec: &BuildSpec,
        company: crate::provisioning::Company,
        manifest: &crate::provisioning::CompanyManifest,
    ) {
        use crate::provisioning::dmi;
        let serial = self.board_serial.clone().unwrap_or_default();
        let dctx = dmi::DmiContext::build(order, spec, manifest, "", &serial);
        let cmds = dmi::ami_commands(&dctx);

        CollapsingHeader::new("DMI / SMBIOS write").id_salt("prov_dmi").show(ui, |ui| {
            if dmi::is_threadripper(spec) {
                ui.colored_label(CAUTION, "Threadripper board — DMI write skipped (tool rejects it).");
                return;
            }
            if company.dmi_manufacturer().is_none() {
                ui.label(RichText::new("No manufacturer for this company — DMI skipped.").weak().small());
                return;
            }
            ui.label(RichText::new("Will run:").strong().small());
            ui.add(egui::Label::new(RichText::new(dmi::preview("AMIDEWIN527", &cmds)).monospace().small()));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tool path").small());
                ui.add(
                    TextEdit::singleline(&mut self.prov_dmi_tool)
                        .hint_text(r".\Installs\AMIDEWIN_527\AMIDEWIN527")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.checkbox(&mut self.prov_dmi_confirm, "I verified this order matches this machine");
            let can = self.prov_dmi_confirm && !self.prov_dmi_tool.trim().is_empty() && !self.prov_busy;
            if ui.add_enabled(can, egui::Button::new(format!("{} Write DMI", p::WARNING))).clicked() {
                let tool = std::path::PathBuf::from(self.prov_dmi_tool.clone());
                let cmds = cmds.clone();
                self.spawn_prov("DMI write", ctx, move || dmi::run(&tool, &cmds));
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.prov_busy, egui::Button::new(format!("{} Read SMBIOS", p::MAGNIFYING_GLASS)))
                    .clicked()
                {
                    let tx = self.tx.clone();
                    let ctx2 = ctx.clone();
                    std::thread::spawn(move || {
                        let result = dmi::read_smbios().map_err(|e| format!("{e:#}"));
                        let _ = tx.send(PanelMsg::DmiRead(result));
                        ctx2.request_repaint();
                    });
                }
                let can_clear =
                    self.prov_dmi_confirm && !self.prov_dmi_tool.trim().is_empty() && !self.prov_busy;
                if ui.add_enabled(can_clear, egui::Button::new(format!("{} Clear SMBIOS", p::WARNING))).clicked() {
                    let tool = std::path::PathBuf::from(self.prov_dmi_tool.clone());
                    let clear = dmi::ami_clear_commands();
                    self.spawn_prov("DMI clear", ctx, move || dmi::run(&tool, &clear));
                }
            });
            if let Some(r) = self.dmi_read.as_ref() {
                ui.add_space(2.0);
                ui.add(egui::Label::new(RichText::new(r.summary()).monospace().small()));
            }
        });
    }

    fn ui_sign_off(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let is_repair = self
            .session
            .as_ref()
            .map(|s| s.order.kind == OrderKind::Repair)
            .unwrap_or(false);
        let is_shopify = self
            .session
            .as_ref()
            .map(|s| s.backend.backend_kind() == BackendKind::Shopify)
            .unwrap_or(false);
        let secret_hint = if is_shopify { "PIN (unused)" } else { "password" };

        Frame::default()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(6.0)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("{} Sign-off", p::USER)).strong().size(16.0));
                ui.add_space(4.0);

                macro_rules! slot {
                    ($label:expr, $ident:expr, $email:expr, $pass:expr, $busy:expr, $err:expr, $role:expr, $hint:expr) => {{
                        ui.label(RichText::new($label).strong().small());
                        if let Some(t) = $ident.clone() {
                            ui.horizontal(|ui| {
                                ui.colored_label(GOOD, format!("{} {} ({})", p::CHECK_CIRCLE, t.name, t.id_employee));
                                if ui.small_button("Clear").clicked() {
                                    $ident = None;
                                }
                            });
                        } else {
                            ui.add(TextEdit::singleline($email).hint_text($hint).desired_width(f32::INFINITY));
                            ui.horizontal(|ui| {
                                ui.add(TextEdit::singleline($pass).hint_text(secret_hint).password(true).desired_width(120.0));
                                if ui.add_enabled(!$busy, egui::Button::new("Sign in")).clicked() {
                                    self.start_auth(ctx, $role);
                                }
                                if $busy {
                                    ui.spinner();
                                }
                            });
                            if let Some(e) = $err.as_ref() {
                                ui.colored_label(ui.visuals().error_fg_color, RichText::new(e).small());
                            }
                        }
                    }};
                }

                let tech_hint = if is_shopify { "floor staff name" } else { "employee email" };
                slot!("QC technician", self.tech, &mut self.tech_email, &mut self.tech_password,
                    self.auth_busy, self.auth_error, AuthRole::Tech, tech_hint);

                if is_repair {
                    ui.add_space(6.0);
                    ui.label(RichText::new("2nd sign-off (repair)").small().color(CAUTION));
                    slot!("Repair sign-off", self.signoff, &mut self.signoff_email, &mut self.signoff_password,
                        self.signoff_busy, self.signoff_error, AuthRole::Signoff, "sign-off email");
                }

                ui.add_space(6.0);
                ui.checkbox(&mut self.is_influencer, "Influencer build");
                if self.is_influencer {
                    slot!("Marketing", self.marketing, &mut self.marketing_email, &mut self.marketing_password,
                        self.marketing_busy, self.marketing_error, AuthRole::Marketing, "marketing email");
                    slot!("Executive", self.executive, &mut self.executive_email, &mut self.executive_password,
                        self.executive_busy, self.executive_error, AuthRole::Executive, "executive email");
                }
            });
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
        // Auto-verify provable items once after load (SMART/OA3/stress temps).
        if self.verify_pending {
            let probe = crate::checklist_verify::WmiProbe::new();
            let cpu = last_verdict.and_then(|v| v.summary.max_temp_c).map(f64::from);
            let gpu = last_verdict.and_then(|v| v.summary.max_gpu_temp_c).map(f64::from);
            crate::checklist_verify::apply(&mut self.checklist, &probe, cpu, gpu);
            self.verify_pending = false;
            self.save_worksheet();
        }

        // This order was already signed off on this machine — offer a fresh start.
        if let Some(summary) = self.prior_signoff.clone() {
            Frame::default()
                .fill(GOOD.gamma_multiply(0.12))
                .stroke((1.0, GOOD))
                .corner_radius(6.0)
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(GOOD, format!("{} Already signed off — {summary}", p::CHECK_CIRCLE));
                    });
                    if ui.button(format!("{} Start new sign-off", p::ARROW_CLOCKWISE)).clicked() {
                        if let Some(s) = self.session.as_ref() {
                            let machine = crate::reporting::machine_id();
                            crate::checklist_store::clear(&s.order.id, &machine);
                        }
                        self.checklist = ChecklistState::from_kind(self.checklist_kind);
                        self.prior_signoff = None;
                        self.verify_pending = true;
                        self.submit_result = None;
                    }
                });
            ui.add_space(6.0);
        }

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

        // Deferred edits so the render loop holds only an immutable borrow.
        enum CkEdit {
            Status(String, ItemStatus),
            Note(String, String),
            Value(String, String),
        }
        let mut edits: Vec<CkEdit> = Vec::new();
        let mut air_toggle: Option<bool> = None;
        let blocked: std::collections::HashSet<String> = self.blocked_keys.iter().cloned().collect();

        let (resolved, total) = self.checklist.open_count();
        ui.horizontal(|ui| {
            let mut air = self.air_cooled;
            if ui.checkbox(&mut air, "Air-cooled (skip Liquid Cooling)").changed() {
                air_toggle = Some(air);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let c = if resolved == total { GOOD } else { CAUTION };
                ui.colored_label(c, RichText::new(format!("{resolved}/{total}")).small());
            });
        });

        let active = self.checklist.first_incomplete();
        for (idx, sec) in self.checklist.sections.iter().enumerate() {
            let header = format!("§{} {}  ({})", sec.number, sec.title, sec.progress_text());
            let locked = active.map(|a| idx > a).unwrap_or(false);
            if locked {
                ui.label(RichText::new(format!("{} {header}", p::LOCK)).weak().small());
                continue;
            }
            let mut ch = CollapsingHeader::new(RichText::new(header).strong())
                .id_salt(format!("ck_sec_{}", sec.number));
            if active == Some(idx) {
                ch = ch.open(Some(true));
            }
            ch.show(ui, |ui| {
                if !sec.applicable {
                    ui.label(RichText::new("Section marked N/A (air-cooled).").weak().small());
                    return;
                }
                if !sec.notes.is_empty() {
                    ui.label(RichText::new(&sec.notes).weak().small());
                }
                for item in &sec.items {
                    let status = item.status();
                    ui.horizontal_wrapped(|ui| {
                        let pass = ui.selectable_label(status == ItemStatus::Pass, RichText::new("Pass").color(GOOD).small());
                        if pass.clicked() {
                            edits.push(CkEdit::Status(item.key.clone(), ItemStatus::Pass));
                        }
                        let fail = ui.selectable_label(status == ItemStatus::Fail, RichText::new("Fail").color(ui.visuals().error_fg_color).small());
                        if fail.clicked() {
                            edits.push(CkEdit::Status(item.key.clone(), ItemStatus::Fail));
                        }
                        let na = ui.selectable_label(status == ItemStatus::Na, RichText::new("N/A").weak().small());
                        if na.clicked() {
                            edits.push(CkEdit::Status(item.key.clone(), ItemStatus::Na));
                        }
                        let label_color = if blocked.contains(&item.key) {
                            CAUTION
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.label(RichText::new(&item.text).small().color(label_color));
                        if item.auto_verified() {
                            ui.label(RichText::new(format!("{} auto", p::CHECK_CIRCLE)).color(GOOD).small())
                                .on_hover_text(&item.evidence);
                        }
                    });
                    if item.show_note() {
                        let mut note = item.note.clone();
                        if ui
                            .add(TextEdit::singleline(&mut note).hint_text("note (required for Fail)").desired_width(f32::INFINITY))
                            .changed()
                        {
                            edits.push(CkEdit::Note(item.key.clone(), note));
                        }
                    }
                    if item.captures_value {
                        let mut value = item.value.clone();
                        if ui
                            .add(TextEdit::singleline(&mut value).hint_text("recorded value").desired_width(f32::INFINITY))
                            .changed()
                        {
                            edits.push(CkEdit::Value(item.key.clone(), value));
                        }
                    }
                }
            });
        }

        let changed = air_toggle.is_some() || !edits.is_empty();
        if let Some(a) = air_toggle {
            self.air_cooled = a;
            self.checklist.set_air_cooled(a);
        }
        for e in edits {
            match e {
                CkEdit::Status(k, s) => self.checklist.set_status(&k, s),
                CkEdit::Note(k, n) => self.checklist.set_note(&k, &n),
                CkEdit::Value(k, v) => self.checklist.set_value(&k, &v),
            }
        }
        if changed {
            self.save_worksheet();
        }

        ui.add_space(6.0);
        ui.add(
            TextEdit::multiline(&mut self.report_notes)
                .hint_text("QC notes…")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );

        ui.add_space(6.0);
        let complete = self.checklist.is_complete();
        ui.horizontal(|ui| {
            let can_submit = !self.submit_busy && self.tech.is_some() && complete;
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
        } else if !complete {
            ui.label(RichText::new("Finish every applicable item to submit.").weak().small());
        }
        if let Some((ok, msg)) = self.submit_result.as_ref() {
            let c = if *ok { GOOD } else { ui.visuals().error_fg_color };
            ui.colored_label(c, msg);
        }
    }
}
