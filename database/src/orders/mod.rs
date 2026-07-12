//! Order-backend abstraction for bench QC (QCWizard parity port).
//!
//! PCL stays on PrestaShop while Xidax moves to Shopify, so qc-app talks to
//! orders only through [`OrderBackend`]. Status gating is evaluated on
//! PrestaShop-legacy status ids, which the Shopify status metaobjects carry
//! as `legacy_id` — one gate table works for both backends.

pub mod checklist;
pub mod checklist_verify;
pub mod gate;
pub mod prestashop_backend;
pub mod shopify_backend;
pub mod spec_check;

pub use checklist::{ChecklistKind, ChecklistState, ItemStatus, QcFailure};
pub use gate::{GateDecision, GateOutcome};
pub use prestashop_backend::PrestashopBackend;
pub use shopify_backend::ShopifyBackend;
pub use spec_check::{CheckStatus, DetectedDisk, DetectedHardware, SpecCheckReport, SpecCheckRow};

use crate::SurrealValue;
use crate::schema::{RecordId, RecordIdExt};
use serde::{Deserialize, Serialize};

pub const QC_REPORT_TABLE: &str = "qc_report";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    Prestashop,
    Shopify,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prestashop => "prestashop",
            Self::Shopify => "shopify",
        }
    }
}

/// Order lookup key. Shape decides the backend: PS ids start with `2`,
/// Everest documents with `5`, Xidax build serials with `XBS-`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderKey {
    Prestashop(String),
    Everest(String),
    ShopifyOrderNumber(String),
    BuildSerial(String),
}

impl OrderKey {
    /// `#` prefix forces Shopify. Bare digits route by QCWizard shape:
    /// 6+ digits on `2` → PS, 7+ digits on `5` → Everest, rest → Shopify.
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let upper = trimmed.to_uppercase();
        if upper.starts_with("XBS-") {
            return Some(Self::BuildSerial(upper));
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim();
            return (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
                .then(|| Self::ShopifyOrderNumber(rest.to_string()));
        }
        if trimmed.chars().all(|c| c.is_ascii_digit()) {
            return match trimmed.chars().next() {
                Some('2') if trimmed.len() >= 6 => Some(Self::Prestashop(trimmed.to_string())),
                Some('5') if trimmed.len() >= 7 => Some(Self::Everest(trimmed.to_string())),
                _ => Some(Self::ShopifyOrderNumber(trimmed.to_string())),
            };
        }
        None
    }

    pub fn backend(&self) -> BackendKind {
        match self {
            Self::Prestashop(_) | Self::Everest(_) => BackendKind::Prestashop,
            Self::ShopifyOrderNumber(_) | Self::BuildSerial(_) => BackendKind::Shopify,
        }
    }

    pub fn display(&self) -> &str {
        match self {
            Self::Prestashop(s) | Self::Everest(s) | Self::ShopifyOrderNumber(s) | Self::BuildSerial(s) => s,
        }
    }
}

/// Order workflow class. PS repairs are `id_order_type == "4"`; Xidax order
/// types carry a `legacy_id` on the `xidax_order_type` metaobject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OrderKind {
    #[default]
    Sales,
    Repair,
    Service,
    Other,
}

impl OrderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sales => "Sales",
            Self::Repair => "Repair",
            Self::Service => "Service",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusInfo {
    pub legacy_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QcOrderItem {
    pub row_id: String,
    pub product_id: String,
    pub name: String,
    pub reference: String,
    pub quantity: f64,
    pub unit_price: String,
    pub serials: Vec<String>,
}

impl QcOrderItem {
    pub fn serial_attached(&self) -> bool {
        self.serials.iter().any(|s| !s.trim().is_empty())
    }
}

/// PS `order_config` row subset relevant to QC (techs, config state).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderConfigInfo {
    pub id: String,
    pub name: String,
    pub id_config: String,
    pub builder_employee: Option<String>,
    pub qc_employee: Option<String>,
    pub state_legacy_id: Option<i64>,
}

/// Service-order device intake fields (repairs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub device_name: String,
    pub device_mfg: String,
    pub device_model: String,
    pub device_serial: String,
    pub physical_damage: String,
    pub check_in_notes: String,
    pub intake_notes: String,
}

#[derive(Debug, Clone, Default)]
pub struct QcOrder {
    pub backend: Option<BackendKind>,
    pub key: Option<OrderKey>,
    /// PS order id or Shopify `legacyResourceId`.
    pub id: String,
    /// Shopify order GID when applicable.
    pub gid: Option<String>,
    /// PS `reference` / Shopify `name` (`#1234`).
    pub reference: String,
    pub customer_name: String,
    pub kind: OrderKind,
    pub status: StatusInfo,
    pub items: Vec<QcOrderItem>,
    pub total_paid: String,
    pub everest_doc: Option<String>,
    pub parent_order_id: Option<String>,
    pub id_customer: Option<String>,
    pub build_serial: Option<String>,
    pub config: Option<OrderConfigInfo>,
    pub service_info: Option<ServiceInfo>,
    pub note: Option<String>,
    /// Source PS order kept for spec extraction.
    pub raw_prestashop: Option<crate::schema::prestashop::Order>,
    /// Raw `xidax_order_config` metaobject nodes kept for spec extraction.
    pub shopify_configs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriveSpec {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlotPick {
    pub slot: String,
    pub name: String,
}

/// Expected hardware parsed from the order configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildSpec {
    pub model: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub motherboard: Option<String>,
    pub os: Option<String>,
    pub drives: Vec<DriveSpec>,
    pub extra: Vec<SlotPick>,
    pub device_serial: String,
    pub device_mfg: String,
}

impl BuildSpec {
    pub fn is_empty(&self) -> bool {
        self.cpu.is_empty()
            && self.gpu.is_empty()
            && self.ram.is_empty()
            && self.drives.is_empty()
            && self.extra.is_empty()
    }
}

/// Authenticated QC technician.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TechIdentity {
    pub id_employee: String,
    pub name: String,
    pub email: String,
    /// PrestaShop employee profile id (e.g. 26 Marketing / 15 Executive);
    /// gates the influencer sign-off. Empty on Shopify (no role equivalent).
    pub id_profile: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderComment {
    pub id: String,
    pub author: String,
    pub author_employee_id: Option<String>,
    pub body: String,
    pub created_at: String,
    pub private: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PhotoCheck {
    pub present: bool,
    pub count: usize,
}

/// Lightweight order row for list/picker views. Carries enough to render a
/// row and reload the full [`QcOrder`] by key via [`OrderSummary::lookup_input`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderSummary {
    pub backend: Option<BackendKind>,
    /// PS order id or Shopify `legacyResourceId` (gid tail).
    pub id: String,
    pub gid: Option<String>,
    /// PS `reference` / Shopify `name` (`#1234`).
    pub reference: String,
    pub customer_name: String,
    pub status: StatusInfo,
    /// Build/config name when known.
    pub model: String,
    pub build_serial: Option<String>,
    /// ISO-8601 creation timestamp (lexicographically sortable).
    pub created_at: Option<String>,
    /// Backend order class (`custom`, `prebuilt`, …).
    pub order_type: String,
    pub expected_serials: i64,
    pub attached_serials: i64,
}

impl OrderSummary {
    /// Lookup string that round-trips through [`OrderKey::parse`] to reload
    /// the full order: bare PS id for PrestaShop, else `#`-prefixed Shopify
    /// order number, else build serial.
    pub fn lookup_input(&self) -> String {
        if self.backend == Some(BackendKind::Prestashop) && !self.id.is_empty() {
            return self.id.clone();
        }
        let number = self.reference.trim_start_matches('#').trim();
        if !number.is_empty() {
            format!("#{number}")
        } else {
            self.build_serial.clone().unwrap_or_default()
        }
    }
}

/// Backend-neutral federated serial history (Shopify + Odoo + PrestaShop),
/// flattened to what a bench tech needs: where the part lives now, its Odoo
/// lot state, prior allocations, and recall/RMA red flags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SerialHistorySummary {
    pub serial: String,
    pub found: bool,
    pub active_recall: bool,
    pub batch_rma_count: i64,
    /// Current Shopify install, e.g. `"#1003 (Jane Doe)"`.
    pub current_order: Option<String>,
    pub disposition: Option<String>,
    /// Odoo lot one-liner: `"name — product"`.
    pub odoo_lot: Option<String>,
    /// Count of prior PrestaShop order allocations for this serial.
    pub prestashop_allocations: usize,
    /// Human-readable flags worth surfacing in red (recall, double-alloc, …).
    pub flags: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<crate::xbm::SerialHistory> for SerialHistorySummary {
    fn from(h: crate::xbm::SerialHistory) -> Self {
        let mut flags = Vec::new();
        if h.active_recall {
            flags.push("ACTIVE RECALL on this part".to_string());
        }
        if h.batch_rma_count > 0 {
            flags.push(format!("{} batch-RMA sibling(s)", h.batch_rma_count));
        }
        let current_order = h.shopify.as_ref().and_then(|s| s.order.as_ref()).map(|o| {
            match o.customer.as_deref() {
                Some(c) if !c.is_empty() => format!("{} ({c})", o.name.clone().unwrap_or_default()),
                _ => o.name.clone().unwrap_or_default(),
            }
        });
        let disposition = h
            .shopify
            .as_ref()
            .and_then(|s| s.disposition.clone())
            .filter(|d| !d.trim().is_empty());
        if let Some(d) = disposition.as_deref() {
            if matches!(d, "qc_reject" | "rma_bin") {
                flags.push(format!("previously dispositioned: {d}"));
            }
        }
        let odoo_lot = h.odoo.as_ref().map(|l| match l.product_name.as_deref() {
            Some(p) if !p.is_empty() => format!("{} — {p}", l.name),
            _ => l.name.clone(),
        });
        // A serial live on a Shopify order AND carrying prior PS allocations is
        // a double-allocation smell worth flagging.
        if h.shopify.is_some() && !h.prestashop.is_empty() {
            flags.push("also allocated in PrestaShop — verify not double-installed".to_string());
        }
        Self {
            serial: h.serial,
            found: h.found,
            active_recall: h.active_recall,
            batch_rma_count: h.batch_rma_count,
            current_order,
            disposition,
            odoo_lot,
            prestashop_allocations: h.prestashop.len(),
            flags,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
pub struct SpecDiffSummary {
    pub component: String,
    pub expected: String,
    pub detected: String,
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
pub struct SpecCheckSummary {
    pub matched: bool,
    pub diffs: Vec<SpecDiffSummary>,
}

/// One stress stage line inside the bench QC payload (`xidax_qc.bench.stages`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
pub struct QcStageBrief {
    pub label: String,
    pub throughput: f64,
    pub unit: String,
    /// `"pass"` / `"fail"` / `"unscored"`.
    pub result: String,
}

/// Bench QC result pushed to the order backend and persisted to SurrealDB.
/// Mirrors the planned `xidax_qc.bench` metafield contract.
#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
pub struct QcReportPayload {
    pub order_key: String,
    pub order_id: String,
    pub backend: String,
    /// `passed` / `failed` / `aborted`.
    pub verdict: String,
    pub preset: Option<String>,
    pub machine_id: Option<String>,
    pub tech: Option<String>,
    pub tech_employee_id: Option<String>,
    pub signoff_tech: Option<String>,
    pub signoff_employee_id: Option<String>,
    pub duration_secs: f64,
    pub whea_delta: i64,
    pub tdr_delta: i64,
    pub stressor_errors: i64,
    pub cpu_max_c: Option<f64>,
    pub gpu_max_c: Option<f64>,
    pub spec_check: Option<SpecCheckSummary>,
    /// `stress_test_run` record backing this verdict.
    pub run_ref: Option<String>,
    /// Section-based checklist snapshot.
    #[serde(default)]
    pub checklist: ChecklistState,
    /// `"BuildQC"` / `"Repair"`.
    #[serde(default)]
    pub checklist_type: String,
    #[serde(default)]
    pub is_influencer: bool,
    pub marketing_employee_id: Option<String>,
    pub executive_employee_id: Option<String>,
    /// One entry per failed checklist item.
    #[serde(default)]
    pub failures: Vec<QcFailure>,
    /// Motherboard/system serial this machine reported, for telemetry backfill.
    pub board_serial: Option<String>,
    pub notes: String,
    /// Per-stage stress results for the backing run.
    #[serde(default)]
    pub stages: Vec<QcStageBrief>,
}

impl QcReportPayload {
    /// Human-readable summary used for backend comment pushes.
    pub fn summary_text(&self) -> String {
        let mut out = format!(
            "BENCH QC {} — order {} ({})\n",
            self.verdict.to_uppercase(),
            self.order_id,
            self.preset.as_deref().unwrap_or("no preset"),
        );
        out.push_str(&format!(
            "Errors: WHEA {} | TDR {} | stressor {}\n",
            self.whea_delta, self.tdr_delta, self.stressor_errors
        ));
        for s in &self.stages {
            out.push_str(&format!(
                "  Stage {}: {} ({:.1} {})\n",
                s.label,
                s.result.to_uppercase(),
                s.throughput,
                s.unit
            ));
        }
        if let Some(c) = self.cpu_max_c {
            out.push_str(&format!("CPU max {c:.1}C "));
        }
        if let Some(g) = self.gpu_max_c {
            out.push_str(&format!("GPU max {g:.1}C"));
        }
        out.push('\n');
        if let Some(spec) = &self.spec_check {
            if spec.matched {
                out.push_str("Spec check: MATCHED\n");
            } else {
                out.push_str("Spec check: MISMATCH\n");
                for d in &spec.diffs {
                    out.push_str(&format!("  {}: expected '{}' detected '{}'\n", d.component, d.expected, d.detected));
                }
            }
        }
        if !self.failures.is_empty() {
            out.push_str(&format!("Checklist failures ({}):\n", self.failures.len()));
            for f in &self.failures {
                out.push_str(&format!("  §{} {}: {}\n", f.section_number, f.item_text, f.note));
            }
        }
        if let Some(tech) = &self.tech {
            out.push_str(&format!("Tech: {tech}\n"));
        }
        if let Some(so) = &self.signoff_tech {
            out.push_str(&format!("Sign-off: {so}\n"));
        }
        if self.is_influencer {
            out.push_str("Influencer build — Marketing + Executive sign-off recorded.\n");
        }
        if let Some(run) = &self.run_ref {
            out.push_str(&format!("Run: {run}\n"));
        }
        if !self.notes.trim().is_empty() {
            out.push_str(&format!("Notes: {}\n", self.notes.trim()));
        }
        out
    }
}

/// SurrealDB row wrapping a submitted bench QC report.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct QcReportRecord {
    pub id: RecordId,
    pub created_at: crate::schema::Datetime,
    pub report: QcReportPayload,
}

/// Persist a bench QC report. Independent of any backend push so SurrealDB
/// keeps the record even when the order backend write fails.
pub async fn persist_qc_report(report: &QcReportPayload) -> anyhow::Result<RecordId> {
    let id = crate::schema::random_record_id(QC_REPORT_TABLE);
    let record = QcReportRecord {
        id: id.clone(),
        created_at: chrono::Utc::now().into(),
        report: report.clone(),
    };
    let created: Option<QcReportRecord> = crate::db()
        .create(QC_REPORT_TABLE)
        .content(record)
        .await?;
    created
        .map(|r| r.id)
        .ok_or_else(|| anyhow::anyhow!("qc_report insert returned no record"))
}

pub const QC_REPORT_FAILURE_TABLE: &str = "qc_report_failure";

/// SurrealDB row for one failed checklist item, linked to its report.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct QcReportFailureRecord {
    pub id: RecordId,
    pub report_ref: String,
    pub created_at: crate::schema::Datetime,
    pub failure: QcFailure,
}

/// Persist one row per failed checklist item, linked to the report record.
/// Independent of `persist_qc_report` so a failure-row error never loses the
/// report. Returns the number of rows written.
pub async fn persist_qc_failures(report_ref: &RecordId, report: &QcReportPayload) -> anyhow::Result<usize> {
    let now: crate::schema::Datetime = chrono::Utc::now().into();
    let report_ref = report_ref.key_string();
    let mut written = 0;
    for failure in &report.failures {
        let record = QcReportFailureRecord {
            id: crate::schema::random_record_id(QC_REPORT_FAILURE_TABLE),
            report_ref: report_ref.clone(),
            created_at: now.clone(),
            failure: failure.clone(),
        };
        let created: Option<QcReportFailureRecord> = crate::db()
            .create(QC_REPORT_FAILURE_TABLE)
            .content(record)
            .await?;
        if created.is_some() {
            written += 1;
        }
    }
    Ok(written)
}

/// Backend contract for bench QC (master plan §6.3, extended with the
/// identity / comments / photo legs of the gap matrix).
pub trait OrderBackend {
    fn backend_kind(&self) -> BackendKind;
    fn find_order(&self, key: &OrderKey) -> impl std::future::Future<Output = anyhow::Result<QcOrder>> + Send;
    fn build_spec(&self, order: &QcOrder) -> impl std::future::Future<Output = anyhow::Result<BuildSpec>> + Send;
    fn status_gate(&self, order: &QcOrder) -> GateDecision;
    fn advance_status(&self, order: &QcOrder, to_legacy_id: i64) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
    fn submit_qc(&self, order: &QcOrder, report: &QcReportPayload) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
    fn authenticate_tech(&self, email: &str, password: &str) -> impl std::future::Future<Output = anyhow::Result<TechIdentity>> + Send;
    fn fetch_comments(&self, order: &QcOrder) -> impl std::future::Future<Output = anyhow::Result<Vec<OrderComment>>> + Send;
    fn post_comment(&self, order: &QcOrder, tech: &TechIdentity, body: &str) -> impl std::future::Future<Output = anyhow::Result<OrderComment>> + Send;
    fn check_build_photos(&self, order: &QcOrder) -> impl std::future::Future<Output = anyhow::Result<PhotoCheck>> + Send;
}

/// Concrete backend dispatch. Picks the implementation from the key shape;
/// the UI can override with an explicit choice.
#[derive(Debug, Clone)]
pub enum QcBackend {
    Prestashop(PrestashopBackend),
    Shopify(ShopifyBackend),
}

impl QcBackend {
    /// Standalone Shopify backend for staff-roster lookups without an order key.
    pub fn shopify() -> Self {
        Self::Shopify(ShopifyBackend::from_env())
    }

    pub fn for_key(key: &OrderKey) -> Self {
        match key.backend() {
            BackendKind::Prestashop => Self::Prestashop(PrestashopBackend::new()),
            BackendKind::Shopify => Self::Shopify(ShopifyBackend::from_env()),
        }
    }

    pub fn backend_kind(&self) -> BackendKind {
        match self {
            Self::Prestashop(b) => b.backend_kind(),
            Self::Shopify(b) => b.backend_kind(),
        }
    }

    pub async fn find_order(&self, key: &OrderKey) -> anyhow::Result<QcOrder> {
        match self {
            Self::Prestashop(b) => b.find_order(key).await,
            Self::Shopify(b) => b.find_order(key).await,
        }
    }

    pub async fn build_spec(&self, order: &QcOrder) -> anyhow::Result<BuildSpec> {
        match self {
            Self::Prestashop(b) => b.build_spec(order).await,
            Self::Shopify(b) => b.build_spec(order).await,
        }
    }

    pub fn status_gate(&self, order: &QcOrder) -> GateDecision {
        match self {
            Self::Prestashop(b) => b.status_gate(order),
            Self::Shopify(b) => b.status_gate(order),
        }
    }

    pub async fn advance_status(&self, order: &QcOrder, to_legacy_id: i64) -> anyhow::Result<()> {
        match self {
            Self::Prestashop(b) => b.advance_status(order, to_legacy_id).await,
            Self::Shopify(b) => b.advance_status(order, to_legacy_id).await,
        }
    }

    pub async fn submit_qc(&self, order: &QcOrder, report: &QcReportPayload) -> anyhow::Result<()> {
        match self {
            Self::Prestashop(b) => b.submit_qc(order, report).await,
            Self::Shopify(b) => b.submit_qc(order, report).await,
        }
    }

    pub async fn authenticate_tech(&self, email: &str, password: &str) -> anyhow::Result<TechIdentity> {
        match self {
            Self::Prestashop(b) => b.authenticate_tech(email, password).await,
            Self::Shopify(b) => b.authenticate_tech(email, password).await,
        }
    }

    pub async fn fetch_comments(&self, order: &QcOrder) -> anyhow::Result<Vec<OrderComment>> {
        match self {
            Self::Prestashop(b) => b.fetch_comments(order).await,
            Self::Shopify(b) => b.fetch_comments(order).await,
        }
    }

    pub async fn post_comment(&self, order: &QcOrder, tech: &TechIdentity, body: &str) -> anyhow::Result<OrderComment> {
        match self {
            Self::Prestashop(b) => b.post_comment(order, tech, body).await,
            Self::Shopify(b) => b.post_comment(order, tech, body).await,
        }
    }

    pub async fn check_build_photos(&self, order: &QcOrder) -> anyhow::Result<PhotoCheck> {
        match self {
            Self::Prestashop(b) => b.check_build_photos(order).await,
            Self::Shopify(b) => b.check_build_photos(order).await,
        }
    }

    /// Federated serial history. Shopify routes through the XBM
    /// `/serials/{serial}` endpoint; PrestaShop has no equivalent wired here.
    pub async fn serial_history(&self, serial: &str) -> anyhow::Result<SerialHistorySummary> {
        match self {
            Self::Shopify(b) => b.serial_history(serial).await,
            Self::Prestashop(_) => Err(anyhow::anyhow!(
                "Serial federation for PCL runs through xidax-lookup, not the bench order backend."
            )),
        }
    }

    /// Newest orders in the build-intake statuses (Order Placed / Ready to
    /// Build), capped at `limit`. Shopify reads the XBM build queue;
    /// PrestaShop has no equivalent wired here.
    pub async fn recent_orders(&self, limit: usize) -> anyhow::Result<Vec<OrderSummary>> {
        match self {
            Self::Shopify(b) => b.recent_orders(limit).await,
            Self::Prestashop(_) => Err(anyhow::anyhow!(
                "Recent-order listing is wired for the Shopify bench queue only."
            )),
        }
    }

    /// Reverse-lookup the order a serial is installed on (no full fetch).
    pub async fn resolve_by_serial(&self, serial: &str) -> anyhow::Result<Option<OrderSummary>> {
        match self {
            Self::Shopify(b) => b.resolve_by_serial(serial).await,
            Self::Prestashop(b) => b.resolve_by_serial(serial).await,
        }
    }
}

/// Try a set of hardware serials against both backends (Shopify first), first
/// hit wins. Each backend yields `None` on miss or when unconfigured, so this
/// is safe to call on any bench. Used to auto-prefill the order from the
/// motherboard serial without a manual scan.
pub async fn resolve_any(serials: &[String]) -> Option<OrderSummary> {
    let shopify = ShopifyBackend::from_env();
    let prestashop = PrestashopBackend::new();
    for serial in serials.iter().filter(|s| !s.trim().is_empty()) {
        if let Ok(Some(summary)) = shopify.resolve_by_serial(serial).await {
            return Some(summary);
        }
        if let Ok(Some(summary)) = prestashop.resolve_by_serial(serial).await {
            return Some(summary);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_key_routing_matches_qcwizard() {
        assert_eq!(OrderKey::parse("212345"), Some(OrderKey::Prestashop("212345".into())));
        assert_eq!(OrderKey::parse("51234567"), Some(OrderKey::Everest("51234567".into())));
        assert_eq!(OrderKey::parse("#1042"), Some(OrderKey::ShopifyOrderNumber("1042".into())));
        assert_eq!(OrderKey::parse("xbs-1042"), Some(OrderKey::BuildSerial("XBS-1042".into())));
        assert_eq!(OrderKey::parse("   "), None);
        assert_eq!(OrderKey::parse("abc"), None);
    }

    #[test]
    fn short_numbers_route_to_shopify_not_everest() {
        // A 4-digit Shopify order number starting with 5 or 2 must not be
        // mistaken for an Everest doc / PS id.
        assert_eq!(OrderKey::parse("5123"), Some(OrderKey::ShopifyOrderNumber("5123".into())));
        assert_eq!(OrderKey::parse("2042"), Some(OrderKey::ShopifyOrderNumber("2042".into())));
        assert_eq!(OrderKey::parse("#51234567"), Some(OrderKey::ShopifyOrderNumber("51234567".into())));
    }

    #[test]
    fn order_key_backend_split() {
        assert_eq!(OrderKey::parse("212345").unwrap().backend(), BackendKind::Prestashop);
        assert_eq!(OrderKey::parse("51234567").unwrap().backend(), BackendKind::Prestashop);
        assert_eq!(OrderKey::parse("1042").unwrap().backend(), BackendKind::Shopify);
        assert_eq!(OrderKey::parse("XBS-1042").unwrap().backend(), BackendKind::Shopify);
    }
}
