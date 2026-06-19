//! Order QC tab: lookup + recent picker, order header, gate + advance, items &
//! serials + federated history, spec check, sign-off, structured checklist +
//! submit, comments, and Auto-Provision. Mirrors the egui `OrderPanel` 1:1; all
//! backend traffic flows through `database::orders::QcBackend`.

mod render;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crossbeam::channel::{unbounded, Receiver, Sender};

use database::orders::checklist::{ChecklistKind, ItemStatus};
use database::orders::{
    persist_qc_failures, persist_qc_report, BackendKind, ChecklistState, OrderComment, OrderKey,
    OrderKind, OrderSummary, QcBackend, QcReportPayload, SerialHistorySummary, TechIdentity,
};
use database::schema::{RecordId, RecordIdExt, RunResult, TICKET_TABLE};

use mtech_tui::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use mtech_tui::styling::Theme;
use mtech_tui::widgets::{
    button::Button, click_zones::ClickZones, dropdown_menu::DropdownMenu, input_field::InputField,
    ButtonType, HandleWidget,
};
use ratatui::{
    crossterm::event::{KeyEvent, MouseEvent},
    layout::Rect,
    prelude::Backend,
    Frame,
};

use crate::order_panel::OrderSession;
use crate::provisioning::Company;
use crate::spec_check::{collect_detected, compare, SpecCheckReport};
use crate::terminal_mode::context::QcContext;

// Widget ids — discrete actions go through the event bus.
const LOOKUP_ID: &str = "OqcLookup";
const LOAD_ID: &str = "OqcLoad";
const BACK_ID: &str = "OqcBack";
const RECENT_REFRESH_ID: &str = "OqcRecentRefresh";
const ADVANCE_ID: &str = "OqcAdvance";
const SPEC_RUN_ID: &str = "OqcSpecRun";
const TECH_EMAIL_ID: &str = "OqcTechEmail";
const TECH_PASS_ID: &str = "OqcTechPass";
const TECH_SIGNIN_ID: &str = "OqcTechSignin";
const SIGNOFF_EMAIL_ID: &str = "OqcSignoffEmail";
const SIGNOFF_PASS_ID: &str = "OqcSignoffPass";
const SIGNOFF_SIGNIN_ID: &str = "OqcSignoffSignin";
const MKT_EMAIL_ID: &str = "OqcMktEmail";
const MKT_PASS_ID: &str = "OqcMktPass";
const MKT_SIGNIN_ID: &str = "OqcMktSignin";
const EXEC_EMAIL_ID: &str = "OqcExecEmail";
const EXEC_PASS_ID: &str = "OqcExecPass";
const EXEC_SIGNIN_ID: &str = "OqcExecSignin";
const COMMENT_ID: &str = "OqcComment";
const COMMENT_SEND_ID: &str = "OqcCommentSend";
const COMMENT_REFRESH_ID: &str = "OqcCommentRefresh";
const REPORT_NOTES_ID: &str = "OqcReportNotes";
const SUBMIT_ID: &str = "OqcSubmit";
const PROV_COMPANY_ID: &str = "OqcProvCompany";
const PROV_DMI_TOOL_ID: &str = "OqcProvDmiTool";

/// Sub-view selected by `[`/`]` or number keys.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum View {
    List,
    Order,
    SignOff,
    Report,
    Comments,
    Provision,
}

impl View {
    const ORDER_VIEWS: [View; 5] = [
        View::Order,
        View::SignOff,
        View::Report,
        View::Comments,
        View::Provision,
    ];

    fn label(self) -> &'static str {
        match self {
            View::List => "List",
            View::Order => "Order",
            View::SignOff => "Sign-off",
            View::Report => "Report",
            View::Comments => "Comments",
            View::Provision => "Provision",
        }
    }
}

/// Which signature slot an authentication targets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthRole {
    Tech,
    Signoff,
    Marketing,
    Executive,
}

struct LoadedOrder {
    order: database::orders::QcOrder,
    spec: database::orders::BuildSpec,
    gate: database::orders::GateDecision,
    comments: Vec<OrderComment>,
    photos: database::orders::PhotoCheck,
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
}

pub struct OrderQcTab<'a> {
    ctx: Arc<Mutex<QcContext>>,
    pub(crate) view: View,

    key_field: InputField<'a>,
    busy: bool,
    error: Option<String>,
    pub(crate) session: Option<OrderSession>,

    recent: Option<Result<Vec<OrderSummary>, String>>,
    recent_busy: bool,
    pub(crate) recent_sel: usize,

    resolved: Option<OrderSummary>,
    resolve_busy: bool,
    resolve_attempted: bool,
    board_serial: Option<String>,

    tech_email: InputField<'a>,
    tech_pass: InputField<'a>,
    tech: Option<TechIdentity>,
    auth_busy: bool,
    auth_error: Option<String>,

    signoff_email: InputField<'a>,
    signoff_pass: InputField<'a>,
    pub(crate) signoff: Option<TechIdentity>,
    signoff_busy: bool,
    signoff_error: Option<String>,

    pub(crate) is_influencer: bool,
    marketing_email: InputField<'a>,
    marketing_pass: InputField<'a>,
    pub(crate) marketing: Option<TechIdentity>,
    marketing_busy: bool,
    marketing_error: Option<String>,
    executive_email: InputField<'a>,
    executive_pass: InputField<'a>,
    pub(crate) executive: Option<TechIdentity>,
    executive_busy: bool,
    executive_error: Option<String>,

    comment_field: InputField<'a>,
    comment_busy: bool,
    comment_error: Option<String>,

    spec_report: Option<SpecCheckReport>,
    spec_pending: bool,

    serial_history: HashMap<String, Result<SerialHistorySummary, String>>,
    serial_busy: Option<String>,

    pub(crate) checklist: ChecklistState,
    checklist_kind: ChecklistKind,
    verify_pending: bool,
    prior_signoff: Option<String>,
    air_cooled: bool,
    blocked_keys: Vec<String>,
    report_notes: InputField<'a>,
    submit_busy: bool,
    submit_result: Option<(bool, String)>,
    advance_busy: bool,
    advance_result: Option<(bool, String)>,

    /// Keyboard focus index into the active sub-view's field/item list.
    pub(crate) focus: usize,
    /// Active text-entry field id; keystrokes route here when set.
    active_field: Option<WidgetId>,

    prov_company: Option<Company>,
    prov_busy: bool,
    prov_dmi_tool: InputField<'a>,
    prov_dmi_confirm: bool,
    prov_log: Vec<(String, bool, String)>,

    // Buttons (built once).
    load_btn: Button<'a>,
    back_btn: Button<'a>,
    recent_refresh_btn: Button<'a>,
    advance_btn: Button<'a>,
    spec_run_btn: Button<'a>,
    tech_signin_btn: Button<'a>,
    signoff_signin_btn: Button<'a>,
    mkt_signin_btn: Button<'a>,
    exec_signin_btn: Button<'a>,
    comment_send_btn: Button<'a>,
    comment_refresh_btn: Button<'a>,
    submit_btn: Button<'a>,
    prov_company_menu: DropdownMenu,
    prov_company_open: bool,

    zones: ClickZones,

    tx: Sender<PanelMsg>,
    rx: Receiver<PanelMsg>,
}

impl<'a> OrderQcTab<'a> {
    pub fn new(ctx: Arc<Mutex<QcContext>>) -> Self {
        let (tx, rx) = unbounded();
        let key_field = InputField::new("Order lookup", WidgetId(LOOKUP_ID.to_string()));
        let tech_pass = InputField::new("Password", WidgetId(TECH_PASS_ID.to_string()));
        tech_pass.input.borrow_mut().set_mask_char('*');
        let signoff_pass = InputField::new("Password", WidgetId(SIGNOFF_PASS_ID.to_string()));
        signoff_pass.input.borrow_mut().set_mask_char('*');
        let marketing_pass = InputField::new("Password", WidgetId(MKT_PASS_ID.to_string()));
        marketing_pass.input.borrow_mut().set_mask_char('*');
        let executive_pass = InputField::new("Password", WidgetId(EXEC_PASS_ID.to_string()));
        executive_pass.input.borrow_mut().set_mask_char('*');

        Self {
            ctx,
            view: View::List,
            key_field,
            busy: false,
            error: None,
            session: None,
            recent: None,
            recent_busy: false,
            recent_sel: 0,
            resolved: None,
            resolve_busy: false,
            resolve_attempted: false,
            board_serial: None,
            tech_email: InputField::new("Email / name", WidgetId(TECH_EMAIL_ID.to_string())),
            tech_pass,
            tech: None,
            auth_busy: false,
            auth_error: None,
            signoff_email: InputField::new("Email", WidgetId(SIGNOFF_EMAIL_ID.to_string())),
            signoff_pass,
            signoff: None,
            signoff_busy: false,
            signoff_error: None,
            is_influencer: false,
            marketing_email: InputField::new("Marketing email", WidgetId(MKT_EMAIL_ID.to_string())),
            marketing_pass,
            marketing: None,
            marketing_busy: false,
            marketing_error: None,
            executive_email: InputField::new("Executive email", WidgetId(EXEC_EMAIL_ID.to_string())),
            executive_pass,
            executive: None,
            executive_busy: false,
            executive_error: None,
            comment_field: InputField::new("Private staff note", WidgetId(COMMENT_ID.to_string())),
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
            report_notes: InputField::new("QC notes", WidgetId(REPORT_NOTES_ID.to_string())),
            submit_busy: false,
            submit_result: None,
            advance_busy: false,
            advance_result: None,
            focus: 0,
            active_field: None,
            prov_company: None,
            prov_busy: false,
            prov_dmi_tool: InputField::new("DMI tool path", WidgetId(PROV_DMI_TOOL_ID.to_string())),
            prov_dmi_confirm: false,
            prov_log: Vec::new(),
            load_btn: Button::new("Load", WidgetId(LOAD_ID.to_string())).theme(Theme::ACCENT),
            back_btn: Button::new("Back to list", WidgetId(BACK_ID.to_string()))
                .theme(Theme::TERTIARY),
            recent_refresh_btn: Button::new("Refresh", WidgetId(RECENT_REFRESH_ID.to_string()))
                .theme(Theme::TERTIARY),
            advance_btn: Button::new("Advance", WidgetId(ADVANCE_ID.to_string()))
                .theme(Theme::ACCENT),
            spec_run_btn: Button::new("Run spec check", WidgetId(SPEC_RUN_ID.to_string()))
                .theme(Theme::TERTIARY),
            tech_signin_btn: Button::new("Sign in", WidgetId(TECH_SIGNIN_ID.to_string()))
                .theme(Theme::ACCENT),
            signoff_signin_btn: Button::new("Sign in", WidgetId(SIGNOFF_SIGNIN_ID.to_string()))
                .theme(Theme::ACCENT),
            mkt_signin_btn: Button::new("Sign in", WidgetId(MKT_SIGNIN_ID.to_string()))
                .theme(Theme::ACCENT),
            exec_signin_btn: Button::new("Sign in", WidgetId(EXEC_SIGNIN_ID.to_string()))
                .theme(Theme::ACCENT),
            comment_send_btn: Button::new("Send", WidgetId(COMMENT_SEND_ID.to_string()))
                .theme(Theme::ACCENT),
            comment_refresh_btn: Button::new("Refresh", WidgetId(COMMENT_REFRESH_ID.to_string()))
                .theme(Theme::TERTIARY),
            submit_btn: Button::new("Submit QC report", WidgetId(SUBMIT_ID.to_string()))
                .theme(Theme::ACCENT),
            prov_company_menu: DropdownMenu::new(),
            prov_company_open: false,
            zones: ClickZones::default(),
            tx,
            rx,
        }
    }

    /// `(service_order, tech)` context applied to stress runs while an order
    /// session is open.
    fn run_context(&self) -> Option<(RecordId, String)> {
        let session = self.session.as_ref()?;
        let service = RecordId::new(TICKET_TABLE, session.order.id.clone());
        let tech = self.tech.as_ref().map(|t| t.name.clone()).unwrap_or_default();
        Some((service, tech))
    }

    /// Drain async results, kick off the load-time auto runs, and publish the
    /// order context into the shared blackboard. Called every frame.
    pub fn tick(&mut self) {
        self.drain_messages();

        if let Some(id) = self.zones.take() {
            self.on_zone_click(&id);
        }

        if self.session.is_none() {
            if !self.resolve_attempted && !self.resolve_busy {
                self.start_resolve_from_hardware();
            }
            if self.recent.is_none() && !self.recent_busy {
                self.start_recent();
            }
        } else {
            self.run_spec_check_auto();
            self.run_verify_auto();
        }

        if let Ok(mut ctx) = self.ctx.lock() {
            ctx.order_context = self.run_context();
        }
    }

    fn save_worksheet(&self) {
        if let Some(s) = self.session.as_ref() {
            let machine = crate::reporting::machine_id();
            crate::checklist_store::save(&s.order.id, &machine, &self.checklist, false, "");
        }
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                PanelMsg::Loaded(loaded) => self.on_loaded(*loaded),
                PanelMsg::LoadFailed(e) => {
                    self.busy = false;
                    self.error = Some(e);
                }
                PanelMsg::Auth { role, result } => self.on_auth(role, result),
                PanelMsg::CommentPosted(result) => {
                    self.comment_busy = false;
                    match *result {
                        Ok(comment) => {
                            self.comment_field.set_text("");
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
                    self.recent_sel = 0;
                }
                PanelMsg::ProvDone { step, result } => {
                    self.prov_busy = false;
                    match result {
                        Ok(msg) => self.prov_log.push((step, true, msg)),
                        Err(e) => self.prov_log.push((step, false, e)),
                    }
                }
                PanelMsg::SerialResolved(found) => {
                    self.resolve_busy = false;
                    if let Some(summary) = found {
                        if self.session.is_none() && self.key_field.get_raw_text().trim().is_empty()
                        {
                            self.key_field.set_text(&summary.lookup_input());
                        }
                        self.resolved = Some(summary);
                    }
                }
            }
        }
    }

    fn on_loaded(&mut self, loaded: LoadedOrder) {
        self.busy = false;
        self.error = None;
        let LoadedOrder { order, spec, gate, comments, photos } = loaded;
        let backend = order
            .key
            .as_ref()
            .map(QcBackend::for_key)
            .unwrap_or_else(|| QcBackend::for_key(&OrderKey::Prestashop(order.id.clone())));
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
        self.report_notes.set_text("");
        self.submit_result = None;
        self.advance_result = None;
        self.signoff = None;
        self.is_influencer = false;
        self.marketing = None;
        self.executive = None;
        self.prov_company = None;
        self.prov_log.clear();
        self.prov_dmi_confirm = false;
        self.view = View::Order;
        self.focus = 0;
        self.active_field = None;
    }

    fn on_auth(&mut self, role: AuthRole, result: Result<TechIdentity, String>) {
        match role {
            AuthRole::Tech => {
                self.auth_busy = false;
                match result {
                    Ok(t) => {
                        self.tech = Some(t);
                        self.auth_error = None;
                        self.tech_pass.set_text("");
                    }
                    Err(e) => self.auth_error = Some(e),
                }
            }
            AuthRole::Signoff => {
                self.signoff_busy = false;
                match result {
                    Ok(t) => {
                        self.signoff = Some(t);
                        self.signoff_error = None;
                        self.signoff_pass.set_text("");
                    }
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
        }
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
                let msg = format!(
                    "{} is not authorized for this signature (profile {profile}).",
                    identity.name
                );
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
                self.marketing_pass.set_text("");
            }
            AuthRole::Executive => {
                self.executive = Some(identity);
                self.executive_error = None;
                self.executive_pass.set_text("");
            }
            _ => {}
        }
    }

    // ---- async starts ----

    fn start_load(&mut self) {
        let input = self.key_field.get_raw_text();
        let Some(key) = OrderKey::parse(&input) else {
            self.error = Some(
                "Enter a PS order (2…), Everest doc (5…), Shopify order # or XBS- serial.".into(),
            );
            return;
        };
        self.busy = true;
        self.error = None;
        let backend = QcBackend::for_key(&key);
        let tx = self.tx.clone();
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
        });
    }

    fn start_recent(&mut self) {
        self.recent_busy = true;
        let backend = QcBackend::shopify();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = backend.recent_orders(10).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::RecentLoaded(result));
        });
    }

    fn start_resolve_from_hardware(&mut self) {
        self.resolve_attempted = true;
        let serials = crate::hardware_id::read_machine_serials();
        self.board_serial = serials.first().cloned();
        if serials.is_empty() {
            return;
        }
        self.resolve_busy = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let found = database::orders::resolve_any(&serials).await;
            let _ = tx.send(PanelMsg::SerialResolved(found));
        });
    }

    fn spawn_prov<F>(&mut self, step: &str, f: F)
    where
        F: FnOnce() -> anyhow::Result<String> + Send + 'static,
    {
        self.prov_busy = true;
        let step = step.to_string();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = f().map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::ProvDone { step, result });
        });
    }

    fn auth_inputs(&self, role: AuthRole) -> (String, String) {
        match role {
            AuthRole::Tech => (self.tech_email.get_raw_text(), self.tech_pass.get_raw_text()),
            AuthRole::Signoff => {
                (self.signoff_email.get_raw_text(), self.signoff_pass.get_raw_text())
            }
            AuthRole::Marketing => {
                (self.marketing_email.get_raw_text(), self.marketing_pass.get_raw_text())
            }
            AuthRole::Executive => {
                (self.executive_email.get_raw_text(), self.executive_pass.get_raw_text())
            }
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

    fn start_auth(&mut self, role: AuthRole) {
        let Some(backend) = self.session.as_ref().map(|s| s.backend.clone()) else { return };
        let (email, password) = self.auth_inputs(role);
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
        tokio::spawn(async move {
            let result = backend
                .authenticate_tech(email.trim(), &password)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::Auth { role, result });
        });
    }

    fn start_post_comment(&mut self) {
        let Some(session) = self.session.as_ref() else { return };
        let Some(tech) = self.tech.clone() else {
            self.comment_error = Some("Sign in before posting comments.".into());
            return;
        };
        let body = self.comment_field.get_raw_text().trim().to_string();
        if body.is_empty() {
            return;
        }
        self.comment_busy = true;
        self.comment_error = None;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result =
                backend.post_comment(&order, &tech, &body).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::CommentPosted(Box::new(result)));
        });
    }

    fn start_refresh_comments(&mut self) {
        let Some(session) = self.session.as_ref() else { return };
        self.comment_busy = true;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = backend.fetch_comments(&order).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::CommentsRefreshed(result));
        });
    }

    fn start_advance(&mut self, to: i64) {
        let Some(session) = self.session.as_ref() else { return };
        self.advance_busy = true;
        self.advance_result = None;
        let backend = session.backend.clone();
        let order = session.order.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = backend
                .advance_status(&order, to)
                .await
                .map(|_| to)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::Advanced(result));
        });
    }

    fn start_serial_history(&mut self, serial: String) {
        let Some(session) = self.session.as_ref() else { return };
        self.serial_busy = Some(serial.clone());
        let backend = session.backend.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = backend.serial_history(&serial).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::SerialHistory { serial, result });
        });
    }

    fn start_submit(&mut self) {
        let (last_verdict, preset) = {
            let guard = self.ctx.lock();
            match guard {
                Ok(g) => (g.last_verdict.clone(), g.last_preset.clone()),
                Err(_) => (None, None),
            }
        };
        let last_verdict = last_verdict.as_ref();

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
            self.submit_result =
                Some((false, "Repair orders need the second sign-off before submitting.".into()));
            return;
        }
        if self.is_influencer && (self.marketing.is_none() || self.executive.is_none()) {
            self.submit_result = Some((
                false,
                "Influencer build needs both Marketing and Executive signatures.".into(),
            ));
            return;
        }
        if !self.checklist.is_complete() {
            self.submit_result = Some((
                false,
                "Checklist incomplete — every applicable item needs Pass / Fail / N/A (Fails need a note)."
                    .into(),
            ));
            return;
        }

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
                    (v.summary.test_errors.max(v.summary.memory_errors) + v.summary.disk_io_errors)
                        as i64
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
            notes: self.report_notes.get_raw_text(),
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

        crate::checklist_store::save(
            &order.id,
            &machine,
            &self.checklist,
            true,
            &verdict_summary(&payload),
        );

        self.submit_busy = true;
        self.submit_result = None;
        let tx = self.tx.clone();
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
                (Err(db), Ok(())) => {
                    Err(format!("Backend push OK but SurrealDB save failed: {db:#}"))
                }
                (Err(db), Err(push)) => Err(format!(
                    "SurrealDB save failed: {db:#}; backend push failed: {push:#}"
                )),
            };
            let _ = tx.send(PanelMsg::Submitted(result));
        });
    }

    // ---- auto runs (load-time once-flags) ----

    fn run_spec_check_auto(&mut self) {
        if !self.spec_pending {
            return;
        }
        let Some(session) = self.session.as_ref() else { return };
        if session.spec.is_empty() {
            return;
        }
        let spec = session.spec.clone();
        let snapshot = self.ctx.lock().ok().and_then(|g| g.snapshot.clone());
        if let Some(snap) = snapshot.filter(|s| s.is_populated()) {
            let hw = collect_detected(&snap);
            self.spec_report = Some(compare(&spec, &hw));
            self.spec_pending = false;
        }
    }

    fn run_spec_check_now(&mut self) {
        let Some(session) = self.session.as_ref() else { return };
        let spec = session.spec.clone();
        if spec.is_empty() {
            return;
        }
        let snapshot = self.ctx.lock().ok().and_then(|g| g.snapshot.clone());
        if let Some(snap) = snapshot {
            let hw = collect_detected(&snap);
            self.spec_report = Some(compare(&spec, &hw));
            self.spec_pending = false;
        }
    }

    fn run_verify_auto(&mut self) {
        if !self.verify_pending {
            return;
        }
        let (cpu, gpu) = {
            let guard = self.ctx.lock();
            match guard {
                Ok(g) => (
                    g.last_verdict.as_ref().and_then(|v| v.summary.max_temp_c).map(f64::from),
                    g.last_verdict.as_ref().and_then(|v| v.summary.max_gpu_temp_c).map(f64::from),
                ),
                Err(_) => (None, None),
            }
        };
        let probe = crate::checklist_verify::WmiProbe::new();
        crate::checklist_verify::apply(&mut self.checklist, &probe, cpu, gpu);
        self.verify_pending = false;
        self.save_worksheet();
    }

    // ---- shared helpers used by render + key handling ----

    pub(crate) fn is_repair(&self) -> bool {
        self.session.as_ref().map(|s| s.order.kind == OrderKind::Repair).unwrap_or(false)
    }

    pub(crate) fn is_shopify(&self) -> bool {
        self.session.as_ref().map(|s| s.backend.backend_kind() == BackendKind::Shopify).unwrap_or(false)
    }

    fn back_to_list(&mut self) {
        self.session = None;
        self.error = None;
        self.view = View::List;
        self.focus = 0;
        self.active_field = None;
    }

    fn switch_view(&mut self, delta: i32) {
        if self.session.is_none() {
            return;
        }
        let order = View::ORDER_VIEWS;
        let cur = order.iter().position(|v| *v == self.view).unwrap_or(0) as i32;
        let n = order.len() as i32;
        let next = ((cur + delta) % n + n) % n;
        self.view = order[next as usize];
        self.focus = 0;
        self.active_field = None;
    }

    /// Toggle the focused checklist item to the next status, or set explicitly.
    fn set_checklist_status(&mut self, key: &str, status: ItemStatus) {
        self.checklist.set_status(key, status);
        self.save_worksheet();
    }

    fn toggle_air_cooled(&mut self) {
        self.air_cooled = !self.air_cooled;
        self.checklist.set_air_cooled(self.air_cooled);
        self.save_worksheet();
    }

    /// Dispatch a clicked zone id to the same internal method the keyboard uses.
    fn on_zone_click(&mut self, id: &str) {
        // Company dropdown is modal: clicks pick a row, anything else closes it.
        if self.prov_company_open {
            if let Some(rest) = id.strip_prefix("menu:") {
                if let Ok(idx) = rest.parse::<usize>() {
                    if let Some(c) = Company::ALL.get(idx) {
                        self.prov_company = Some(*c);
                    }
                }
            }
            self.prov_company_open = false;
            return;
        }

        if let Some(rest) = id.strip_prefix("view:") {
            self.set_view_by_name(rest);
        } else if let Some(rest) = id.strip_prefix("recent:") {
            if let Ok(i) = rest.parse::<usize>() {
                self.load_recent(i);
            }
        } else if let Some(rest) = id.strip_prefix("item:") {
            if let Ok(i) = rest.parse::<usize>() {
                self.serial_history_for_item(i);
            }
        } else if id == "air" {
            self.toggle_air_cooled();
        } else if let Some(rest) = id.strip_prefix("chk:") {
            if let Some((key, verb)) = rest.rsplit_once(':') {
                let status = match verb {
                    "pass" => ItemStatus::Pass,
                    "fail" => ItemStatus::Fail,
                    "na" => ItemStatus::Na,
                    _ => return,
                };
                self.set_checklist_status(key, status);
            }
        } else if let Some(kind) = id.strip_prefix("prov:") {
            self.prov_step(kind);
        }
    }

    /// Set the active sub-view from a view-bar zone id; ignores order-views
    /// when no session is loaded.
    fn set_view_by_name(&mut self, name: &str) {
        let target = match name {
            "list" => View::List,
            "order" => View::Order,
            "signoff" => View::SignOff,
            "report" => View::Report,
            "comments" => View::Comments,
            "provision" => View::Provision,
            _ => return,
        };
        if target != View::List && self.session.is_none() {
            return;
        }
        self.view = target;
        self.focus = 0;
        self.active_field = None;
    }

    /// Select recent order `i` and load it, mirroring the keyboard Enter path.
    fn load_recent(&mut self, i: usize) {
        if let Some(Ok(orders)) = self.recent.as_ref() {
            if let Some(o) = orders.get(i) {
                self.recent_sel = i;
                let input = o.lookup_input();
                self.key_field.set_text(&input);
                self.start_load();
            }
        }
    }

    /// Look up serial history for the serials on the item at index `i`.
    fn serial_history_for_item(&mut self, i: usize) {
        if !self.is_shopify() {
            return;
        }
        let serials: Vec<String> = self
            .session
            .as_ref()
            .and_then(|s| s.order.items.get(i))
            .map(|item| item.serials.clone())
            .unwrap_or_default();
        for serial in serials {
            self.start_serial_history(serial);
        }
    }
}

/// One-line summary stored on the signed-off worksheet.
fn verdict_summary(p: &QcReportPayload) -> String {
    format!("{} — {} failure(s)", p.verdict, p.failures.len())
}

/// Date portion of an ISO-8601 timestamp.
pub(crate) fn short_date(iso: Option<&str>) -> String {
    iso.map(|s| s.split('T').next().unwrap_or(s).to_string()).unwrap_or_default()
}

impl<'a> ActionHandler for OrderQcTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("OrderQcTab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        [
            LOOKUP_ID,
            LOAD_ID,
            BACK_ID,
            RECENT_REFRESH_ID,
            ADVANCE_ID,
            SPEC_RUN_ID,
            TECH_EMAIL_ID,
            TECH_PASS_ID,
            TECH_SIGNIN_ID,
            SIGNOFF_EMAIL_ID,
            SIGNOFF_PASS_ID,
            SIGNOFF_SIGNIN_ID,
            MKT_EMAIL_ID,
            MKT_PASS_ID,
            MKT_SIGNIN_ID,
            EXEC_EMAIL_ID,
            EXEC_PASS_ID,
            EXEC_SIGNIN_ID,
            COMMENT_ID,
            COMMENT_SEND_ID,
            COMMENT_REFRESH_ID,
            REPORT_NOTES_ID,
            SUBMIT_ID,
            PROV_COMPANY_ID,
            PROV_DMI_TOOL_ID,
        ]
        .into_iter()
        .map(|s| WidgetId(s.to_string()))
        .collect()
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id } => {
                if self.managed_widget_ids().contains(widget_id) {
                    self.active_field = Some(widget_id.clone());
                }
            }
            WidgetEvent::ButtonClick { widget_id, .. } => match widget_id.0.as_str() {
                LOAD_ID => {
                    if !self.busy {
                        self.start_load();
                    }
                }
                BACK_ID => self.back_to_list(),
                RECENT_REFRESH_ID => {
                    if !self.recent_busy {
                        self.start_recent();
                    }
                }
                ADVANCE_ID => {
                    if let Some(target) =
                        self.session.as_ref().and_then(|s| s.gate.advance_target())
                    {
                        let ps = self.session.as_ref().and_then(|s| s.order.backend)
                            == Some(BackendKind::Prestashop);
                        if ps && !self.advance_busy {
                            self.start_advance(target);
                        }
                    }
                }
                SPEC_RUN_ID => self.run_spec_check_now(),
                TECH_SIGNIN_ID => self.start_auth(AuthRole::Tech),
                SIGNOFF_SIGNIN_ID => self.start_auth(AuthRole::Signoff),
                MKT_SIGNIN_ID => self.start_auth(AuthRole::Marketing),
                EXEC_SIGNIN_ID => self.start_auth(AuthRole::Executive),
                COMMENT_SEND_ID => self.start_post_comment(),
                COMMENT_REFRESH_ID => self.start_refresh_comments(),
                SUBMIT_ID => self.start_submit(),
                _ => {}
            },
            _ => {}
        }
    }
}

impl<'a> HandleWidget<'a> for OrderQcTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.render(f, area);
    }

    fn handle_mouse_event(&self, ev: &MouseEvent) {
        self.key_field.handle_mouse_event(ev);
        self.load_btn.handle_mouse_event(ev);
        self.back_btn.handle_mouse_event(ev);
        self.recent_refresh_btn.handle_mouse_event(ev);
        self.advance_btn.handle_mouse_event(ev);
        self.spec_run_btn.handle_mouse_event(ev);
        self.tech_email.handle_mouse_event(ev);
        self.tech_pass.handle_mouse_event(ev);
        self.tech_signin_btn.handle_mouse_event(ev);
        self.signoff_email.handle_mouse_event(ev);
        self.signoff_pass.handle_mouse_event(ev);
        self.signoff_signin_btn.handle_mouse_event(ev);
        self.marketing_email.handle_mouse_event(ev);
        self.marketing_pass.handle_mouse_event(ev);
        self.mkt_signin_btn.handle_mouse_event(ev);
        self.executive_email.handle_mouse_event(ev);
        self.executive_pass.handle_mouse_event(ev);
        self.exec_signin_btn.handle_mouse_event(ev);
        self.comment_field.handle_mouse_event(ev);
        self.comment_send_btn.handle_mouse_event(ev);
        self.comment_refresh_btn.handle_mouse_event(ev);
        self.report_notes.handle_mouse_event(ev);
        self.submit_btn.handle_mouse_event(ev);
        self.prov_dmi_tool.handle_mouse_event(ev);
        self.zones.on_mouse(ev);
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        self.handle_key(key)
    }
}
