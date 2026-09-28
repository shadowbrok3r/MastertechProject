//! Serde types for the Xidax Build Management API (`/api/v1/*`).
//!
//! Field names mirror the camelCase JSON the Remix app returns; structs stay
//! permissive (`default` everywhere) because the server adds fields freely.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schema::deserializer::deserialize_to_string;

// ─── Envelope ───────────────────────────────────────────────────────────────

/// `{ok:true,data}` / `{ok:false,error:{code,message}}` wire envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct Envelope {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<EnvelopeError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvelopeError {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
    /// Machine-readable context, e.g. `legacyOrderId` on `legacy_order_not_migrated`.
    #[serde(default)]
    pub details: Option<Value>,
}

// ─── GET /orders (queue) ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueuePayload {
    pub orders: Vec<QueueOrder>,
    pub reps: Vec<QueueRep>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueOrder {
    /// Shopify Order GID.
    pub id: String,
    /// `"#1020"`.
    pub name: String,
    pub created_at: Option<String>,
    pub customer: QueueCustomer,
    pub build_name: String,
    pub build_serial: Option<String>,
    pub status: Option<QueueStatus>,
    pub expected_serials: i64,
    pub attached_serials: i64,
    pub eta_date: Option<String>,
    pub sales_rep: Option<String>,
    pub sales_rep_code: Option<String>,
    pub pull_priority: i64,
    pub awaiting_parts: bool,
    /// `"corporate" | "prebuilt" | "custom" | "other"`.
    pub order_type: String,
    /// Which brand's store the order belongs to — `"xidax"` or `"pcl"`.
    pub entity: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct QueueCustomer {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct QueueStatus {
    pub gid: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct QueueRep {
    pub code: String,
    pub name: String,
}

/// Workflow buckets `GET /orders?bucket=` documents. The server ignores the
/// parameter — verified 2026-08-29, an unknown bucket returns the full list
/// unchanged — so callers must filter the response themselves.
pub const QUEUE_BUCKETS: &[&str] = &[
    "to_pull",
    "building",
    "qc",
    "ready_to_ship",
    "shipped",
    "other",
];

// ─── GET /orders/{id} (build detail) ────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildDetail {
    pub order: Option<DetailOrder>,
    pub config: Option<DetailConfig>,
    pub line_items: Vec<DetailLineItem>,
    pub current_status: Option<StatusRef>,
    pub legal_transitions: Vec<LegalTransition>,
    pub prechecks: Vec<Precheck>,
    pub build_photos: Vec<BuildPhoto>,
    pub timer: Option<BuildTimer>,
    pub all_statuses: Vec<StatusRef>,
    pub order_type: Option<DetailOrderType>,
    pub order_reference: Option<String>,
    pub shipping_damage_reported: Option<bool>,
    pub pool_unit: Option<DetailPoolUnit>,
    /// `xidax_installed_serial` GIDs attached to the order.
    pub installed_serials: Vec<String>,
    pub service_details: Option<Value>,
    pub order_details: Option<OrderDetailsBlock>,
    /// Every build on the order; `active` marks the one in view.
    pub configs: Vec<ResolvedConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailOrder {
    /// Shopify Order GID.
    pub id: String,
    /// `"#1020"`.
    pub name: String,
    pub customer: Option<DetailCustomer>,
    pub shipping_address: Option<Value>,
    pub note: Option<String>,
    pub cancelled_at: Option<String>,
    pub cancel_reason: Option<String>,
    pub financial_status: Option<String>,
    pub fulfillment_status: Option<String>,
    pub pull_priority: Option<i64>,
    pub client_ip: Option<String>,
    pub sales_rep: Option<DetailSalesRep>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DetailCustomer {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DetailSalesRep {
    pub name: String,
    pub code: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailConfig {
    pub build_serial: Option<String>,
    pub build_name: Option<String>,
    pub build_template: Option<String>,
    pub system_type: Option<String>,
    pub cec: Option<Value>,
    pub notes: Option<Value>,
    pub customer_notes: Option<String>,
    pub estimated_ship_date: Option<String>,
    /// Slot picks; objects carry `slot`, `product_name`, `sku`, `quantity`, …
    pub selection: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailLineItem {
    /// Shopify LineItem GID.
    pub id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub qty: i64,
    /// Canonical slot: `"processor"`, `"ram"`, `"storage-m2"`, …
    pub slot: String,
    pub sku: Option<String>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub product_handle: Option<String>,
    pub is_pool_product: Option<bool>,
    pub expected_serials: i64,
    pub serials: Vec<InstalledSerial>,
    pub required_for_build: Option<bool>,
    pub slot_handle: Option<String>,
    pub substitution: Option<Value>,
    /// `"original" | "extra"`.
    pub kind: Option<String>,
    pub addition: Option<Value>,
    /// Build this line belongs to; null on order-edit additions.
    pub config_id: Option<String>,
    /// Cart-time pointer, diagnostics only; never a join key.
    pub cart_config_gid: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstalledSerial {
    /// `xidax_installed_serial` metaobject GID.
    pub metaobject_gid: String,
    pub serial: String,
    pub scanned_at: Option<String>,
    pub scanned_by: Option<String>,
    /// `"reserved" | "pending" | "failed" | "unknown" | "detached"`.
    pub reservation_status: String,
    pub odoo_lot_id: Option<String>,
    /// `xidax_order_config` GID; null on migrated PrestaShop serials.
    pub order_config_gid: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusRef {
    /// `xidax_order_status` metaobject GID.
    pub gid: String,
    pub name: String,
    pub color: String,
    pub legacy_id: i64,
    pub locked: Option<bool>,
    /// `"awaiting_parts" | "building" | "qc" | "preparing" | "shipped" | ""`.
    pub bucket: Option<String>,
    pub production_locked: Option<bool>,
    pub edit_locked: Option<bool>,
    pub shipped: Option<bool>,
    pub paid: Option<bool>,
    pub retired: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LegalTransition {
    pub gid: String,
    pub name: String,
    pub color: String,
    pub prechecks: Vec<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Precheck {
    pub name: String,
    pub ok: bool,
    pub missing: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildPhoto {
    pub gid: String,
    pub url: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildTimer {
    pub pull_started_at: Option<String>,
    pub pull_paused_at: Option<String>,
    pub pull_completed_at: Option<String>,
    pub pull_by_staff: Option<String>,
    pub build_started_at: Option<String>,
    pub build_paused_at: Option<String>,
    pub build_completed_at: Option<String>,
    pub build_by_staff: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailOrderType {
    pub gid: String,
    pub handle: String,
    pub legacy_id: i64,
    pub name: String,
    pub prefix: Option<String>,
    pub is_default: Option<bool>,
    /// Handled by the repair centre.
    pub is_service: Option<bool>,
    pub flags: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailPoolUnit {
    pub id: String,
    pub handle: String,
    pub build_serial: Option<String>,
    pub warehouse_location: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderDetailsBlock {
    pub customer_reference: Option<String>,
    pub external_po_number: Option<String>,
    pub store_id: Option<String>,
    pub sales_rep: Option<OrderDetailsRep>,
    pub split_reps: Vec<OrderDetailsRep>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderDetailsRep {
    pub employee_id: String,
    pub name: String,
}

// ─── PATCH /orders/{id} ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderDetailsPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_po_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sales_rep_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_rep_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type_gid: Option<String>,
}

// ─── POST /orders/{id}/advance ──────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_status_gid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_status_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

/// Advance result carries its own inner `ok` distinct from the envelope.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AdvanceResult {
    pub ok: bool,
    pub current_status: Option<StatusRef>,
    pub precheck_failed: Option<bool>,
    pub error: Option<String>,
}

// ─── POST /orders/{id}/scan-serial ──────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSerialRequest {
    pub line_item_id: String,
    pub serial: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanSerialResult {
    pub ok: bool,
    /// `"reserved" | "pending"` on success.
    pub reservation_status: Option<String>,
    pub odoo_lot_id: Option<String>,
    pub warning: Option<String>,
    pub line_item: Option<Value>,
    /// `"no_sku" | "not_found" | "reserved" | "no_stock" | "read_only" | "odoo_unreachable"`.
    pub reason: Option<String>,
    pub can_force: Option<bool>,
    pub error: Option<String>,
}

// ─── POST /orders/{id}/detach-serial ────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachSerialRequest {
    pub serial_metaobject_gid: String,
    pub reason: String,
    /// `"qc_reject" | "rma_bin" | "manual_remove" | "warehouse_stock"`.
    pub disposition: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetachSerialResult {
    pub ok: bool,
    pub removed: Option<String>,
    pub odoo_released: Option<bool>,
    pub warning: Option<String>,
}

// ─── POST /orders/{id}/ship ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShipRequest {
    pub carrier: String,
    pub tracking: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_customer: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShipResult {
    pub ok: bool,
    pub fulfillment_id: Option<String>,
}

// ─── GET /statuses ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StatusesPayload {
    pub statuses: Vec<WorkflowStatus>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkflowStatus {
    pub gid: String,
    pub handle: String,
    pub legacy_id: i64,
    pub name: String,
    pub color: String,
    /// `"awaiting_parts" | "building" | "qc" | "preparing" | "shipped" | ""`.
    pub bucket: String,
    pub production_locked: bool,
    pub edit_locked: bool,
    pub shipped: bool,
    pub paid: bool,
    /// Displayable but refused (422) as an `advance` target.
    pub retired: bool,
    /// Pipeline position from 1; 0 is unordered.
    pub display_order: i64,
    /// `xidax_order_type` GIDs this status applies to; empty is unrestricted.
    pub applicable_order_types: Vec<String>,
}

// ─── GET /serials/{serial} ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SerialHistory {
    pub serial: String,
    pub found: bool,
    pub sources: Option<Value>,
    pub shopify: Option<SerialShopify>,
    pub odoo: Option<SerialOdoo>,
    pub prestashop: Vec<SerialPrestashop>,
    pub active_recall: bool,
    pub batch_rma_count: i64,
    pub history: Vec<SerialHistoryEvent>,
    pub elapsed_ms: Option<i64>,
    pub cached: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SerialShopify {
    pub metaobject_gid: String,
    pub serial: String,
    pub installed_at: Option<String>,
    pub installed_by_staff: Option<String>,
    pub disposition: Option<String>,
    pub defect_reason: Option<String>,
    pub detached_at: Option<String>,
    pub odoo_lot_id: Option<String>,
    pub order: Option<SerialShopifyOrder>,
    pub variant: Option<SerialShopifyVariant>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SerialShopifyOrder {
    pub gid: String,
    pub name: Option<String>,
    pub created_at: Option<String>,
    pub customer: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SerialShopifyVariant {
    pub gid: Option<String>,
    pub sku: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SerialOdoo {
    pub lot_id: i64,
    pub name: String,
    pub product_id: Option<i64>,
    pub product_name: Option<String>,
    pub create_date: Option<String>,
    pub r#ref: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SerialPrestashop {
    pub id_order_serial: String,
    pub id_order: Option<String>,
    pub id_order_detail: Option<String>,
    pub id_product: Option<String>,
    pub id_odoo_sl: Option<String>,
    pub serial_number: String,
    pub date_created: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SerialHistoryEvent {
    /// `"shopify" | "odoo" | "prestashop"`.
    pub source: String,
    pub kind: String,
    pub at: Option<String>,
    pub label: String,
    pub r#ref: Option<String>,
}

// ─── /staff ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StaffPayload {
    pub staff: Vec<StaffMember>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StaffMember {
    pub id: String,
    pub name: String,
    /// `"floor" | "manager"`.
    pub role: String,
    pub active: bool,
    pub nfc_id: Option<String>,
    pub has_pin: bool,
    pub has_qr: bool,
    /// The address itself is write-only.
    pub has_email: bool,
    pub created_at: Option<String>,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateStaffRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nfc_id: Option<String>,
    /// Unique across staff (409 `email_taken`); enables self-serve PIN reset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateStaffResult {
    pub staff: Option<StaffMember>,
    /// Returned once at creation; encode in the badge QR.
    pub qr_token: Option<String>,
}

// ─── GET /dashboard-metrics ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DashboardMetrics {
    pub as_of: String,
    pub kpis: DashboardKpis,
    pub pipeline: Vec<PipelineStage>,
    pub exceptions: Vec<Value>,
    pub tech_throughput: Vec<Value>,
    pub shipped_trend: Vec<ShippedTrendDay>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DashboardKpis {
    pub shipped_today: i64,
    pub shipped_today_goal: i64,
    pub in_build: i64,
    pub in_qc: i64,
    pub ready_to_ship: i64,
    pub stuck48h: i64,
    pub overdue_ship_by: i64,
    pub open_rmas: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PipelineStage {
    pub stage_gid: String,
    pub stage_name: String,
    pub color: String,
    pub count: i64,
    pub median_age_hours: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShippedTrendDay {
    pub date: String,
    pub count: i64,
}

// ─── GET /pool-summary ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PoolSummaryPayload {
    pub summary: Vec<PoolModelSummary>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolModelSummary {
    pub model_sku: String,
    pub available: i64,
    pub pending: i64,
    pub assigned: i64,
    pub shipped: i64,
    pub other: i64,
    pub total_active: i64,
}

// ─── /prebuilt-units ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePrebuiltUnitRequest {
    pub model_sku: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warehouse_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_order_gid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrebuiltUnitRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warehouse_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_serial: Option<String>,
    /// `"new" | "refurbished"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

// ─── /orders/{id}/assign-pool-unit ──────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignPoolUnitRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_sku: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_gid: Option<String>,
}

// ─── /orders/{id}/release-pool-unit ─────────────────────────────────────────

/// Where a released unit lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolReleaseDisposition {
    #[default]
    Restock,
    Missing,
    SoldElsewhere,
    Defective,
    Debuild,
    Lost,
    Stolen,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePoolUnitRequest {
    /// Read from the order's `pool_unit` metafield when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_gid: Option<String>,
    pub disposition: PoolReleaseDisposition,
    /// Required (3+ characters) for every disposition except `Restock`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Carries its own `ok`; a refused release (already shipped, held elsewhere) is `ok: false`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolReleaseResult {
    pub ok: bool,
    pub unit: Option<PoolUnit>,
    pub error: Option<String>,
    /// `"release"`, or `"unlink"` when only the order's stale link was cleared.
    pub mode: Option<String>,
    pub note: Option<String>,
    pub odoo_transfer: Option<Value>,
    pub expansion: Option<Value>,
    pub warnings: Vec<String>,
    pub journal_id: Option<String>,
}

/// `xidax_prebuilt_unit` metaobject.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolUnit {
    pub id: String,
    pub handle: String,
    pub model_sku: String,
    pub build_serial: String,
    pub warehouse_location: String,
    /// `building | qc_pending | available | assigned | shipped | returned_pending_qc | …`.
    pub status: String,
    /// `"new" | "refurbished"`.
    pub condition: String,
    pub brand: String,
    pub built_at: Option<String>,
    pub built_by: Option<String>,
    pub assigned_order_gid: Option<String>,
    pub draft_order_gid: Option<String>,
    pub serial_chain_gids: Vec<String>,
    pub notes: String,
    /// 1 (urgent) to 5 (low).
    pub pull_priority: i64,
}

// ─── /debuilds ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDebuildRequest {
    pub order_id: String,
    /// `"return" | "discontinued" | "cancelled" | "damage" | "other"`.
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[cfg(test)]
mod fixture_tests {
    //! Deserialize real (sanitized) API captures from 2026-06-12 so the wire
    //! contract is enforced by CI, not by hand-written sample JSON.

    use super::*;

    fn data(envelope_json: &str) -> Value {
        let env: Envelope = serde_json::from_str(envelope_json).unwrap();
        assert!(env.ok, "fixture envelope not ok: {:?}", env.error);
        env.data.unwrap()
    }

    #[test]
    fn live_serial_1234_parses() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/serial-1234-live.json")).unwrap();
        let data = raw.get("data").cloned().unwrap();
        if let Err(e) = serde_json::from_value::<SerialHistory>(data) {
            panic!("SerialHistory decode failed: {e}");
        }
    }

    #[test]
    fn live_orders_qc_fixture_parses() {
        let payload: QueuePayload =
            serde_json::from_value(data(include_str!("fixtures/orders-qc.json"))).unwrap();
        // Live-derived fixture; assert non-empty rather than an exact count.
        assert!(!payload.orders.is_empty());
        let with_build_serial = payload
            .orders
            .iter()
            .filter(|o| o.build_serial.as_deref().is_some_and(|s| !s.is_empty()))
            .count();
        assert!(with_build_serial > 0, "expected some build serials in the qc bucket");
        assert!(payload.orders.iter().all(|o| o.id.starts_with("gid://shopify/Order/")));
    }

    #[test]
    fn live_order_detail_fixture_parses() {
        let detail: BuildDetail =
            serde_json::from_value(data(include_str!("fixtures/order-detail.json"))).unwrap();
        let status = detail.current_status.as_ref().unwrap();
        assert_eq!(status.legacy_id, 4);
        assert_eq!(status.shipped, Some(true));
        assert!(!detail.installed_serials.is_empty());
        assert!(detail.installed_serials[0].starts_with("gid://shopify/Metaobject/"));
        // Line items carry structured serial scans.
        let scanned: Vec<_> = detail
            .line_items
            .iter()
            .flat_map(|li| &li.serials)
            .collect();
        assert!(!scanned.is_empty());
        assert!(scanned[0].serial.contains('-'));
        assert_eq!(detail.prechecks[0].name, "serials_attached");
        assert!(detail.timer.as_ref().unwrap().pull_completed_at.is_some());
        // Config selection entries are snake_case objects.
        let sel = detail.config.unwrap().selection.unwrap();
        let first = &sel.as_array().unwrap()[0];
        assert!(first.get("slot_handle").is_some());
        assert!(first.get("product_name").is_some());
    }

    #[test]
    fn live_serial_history_fixture_parses() {
        let hist: SerialHistory =
            serde_json::from_value(data(include_str!("fixtures/serial-history.json"))).unwrap();
        assert!(hist.found);
        let shopify = hist.shopify.unwrap();
        assert_eq!(shopify.order.unwrap().name.as_deref(), Some("#1003"));
        assert_eq!(shopify.variant.unwrap().sku.as_deref(), Some("MB/X670/GODLIKE"));
        assert_eq!(hist.history[0].kind, "installed");
    }

    #[test]
    fn live_statuses_fixture_parses() {
        let payload: StatusesPayload =
            serde_json::from_value(data(include_str!("fixtures/statuses.json"))).unwrap();
        assert!(payload.statuses.len() > 30);
        // The Xidax store carries the classic PrestaShop legacy-id space.
        let by_id = |id: i64| payload.statuses.iter().find(|s| s.legacy_id == id);
        assert_eq!(by_id(71).unwrap().name, "QC & Burn-in");
        assert_eq!(by_id(67).unwrap().name, "Preparing to Ship");
        assert_eq!(by_id(4).unwrap().name, "Shipped");
        // Planned bench ids from master plan W7 are NOT live yet: 109 is
        // absent and 43 is a repair status, so the gate also admits 71.
        // When this assert flips, the store seeded the planned ids — revisit
        // XIDAX_BENCH_STATUSES / XIDAX_BENCH_TARGET in orders/gate.rs.
        assert!(by_id(109).is_none());
        assert_ne!(by_id(43).unwrap().name, "Burn-in");
    }

    #[test]
    fn live_pool_summary_fixture_parses() {
        let payload: PoolSummaryPayload =
            serde_json::from_value(data(include_str!("fixtures/pool-summary.json"))).unwrap();
        assert_eq!(payload.summary.len(), 3);
        assert!(payload.summary.iter().any(|m| m.model_sku == "x6-rtx5090-apex"));
        assert!(payload.summary[0].total_active >= payload.summary[0].available);
    }
}

#[cfg(test)]
mod comment_envelope_tests {
    use super::*;

    /// The created comment arrives wrapped. Every `XbmComment` field defaults,
    /// so deserialising the envelope straight into it yields a blank comment
    /// and no error — the shape has to be unwrapped explicitly.
    #[test]
    fn created_comment_is_unwrapped_from_its_envelope() {
        let data = serde_json::json!({
            "comment": {
                "id": "cmtezqxyl000hfx84cbgvzuse",
                "orderGid": "gid://shopify/Order/7280155787490",
                "type": "note",
                "body": "Mastertech write test",
                "author": "api:mastertech",
                "visibility": "internal",
                "source": "api-key"
            }
        });

        let envelope: CommentEnvelope = serde_json::from_value(data.clone()).unwrap();
        assert_eq!(envelope.comment.id, "cmtezqxyl000hfx84cbgvzuse");
        assert_eq!(envelope.comment.author, "api:mastertech");
        assert_eq!(envelope.comment.kind, "note");

        // The bug this guards: the wrong target type parses without error.
        let flattened: XbmComment = serde_json::from_value(data).unwrap();
        assert!(flattened.id.is_empty(), "envelope must not parse as a comment");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_payload_parses() {
        let raw = serde_json::json!({
            "orders": [{
                "id": "gid://shopify/Order/123",
                "name": "#1020",
                "createdAt": "2026-06-01T10:00:00Z",
                "customer": { "name": "Jane Doe", "email": "j@x.com" },
                "buildName": "Apex X-10",
                "buildSerial": "XBS-1020",
                "status": { "gid": "gid://shopify/Metaobject/9", "name": "In QC", "color": "#888" },
                "expectedSerials": 8,
                "attachedSerials": 8,
                "etaDate": null,
                "salesRep": null,
                "salesRepCode": null,
                "pullPriority": 3,
                "awaitingParts": false,
                "orderType": "custom"
            }],
            "reps": [{ "code": "JD", "name": "John Drake" }]
        });
        let payload: QueuePayload = serde_json::from_value(raw).unwrap();
        assert_eq!(payload.orders.len(), 1);
        let order = &payload.orders[0];
        assert_eq!(order.name, "#1020");
        assert_eq!(order.build_serial.as_deref(), Some("XBS-1020"));
        assert_eq!(order.status.as_ref().unwrap().name, "In QC");
        assert_eq!(payload.reps[0].code, "JD");
    }

    #[test]
    fn build_detail_parses() {
        let raw = serde_json::json!({
            "order": {
                "id": "gid://shopify/Order/123",
                "name": "#1020",
                "customer": { "name": "Jane", "email": "j@x.com", "phone": "555" },
                "note": "leave at door",
                "cancelledAt": null,
                "financialStatus": "PAID",
                "pullPriority": 1
            },
            "config": {
                "buildSerial": "XBS-1020",
                "buildName": "Apex X-10",
                "selection": [
                    { "slot": "processor", "product_name": "Ryzen 9 9950X", "sku": "CPU-9950X", "quantity": 1 },
                    { "slot": "ram", "product_name": "64GB DDR5-6000", "quantity": 2 }
                ]
            },
            "lineItems": [{
                "id": "gid://shopify/LineItem/456",
                "title": "Ryzen 9 9950X",
                "qty": 1,
                "slot": "processor",
                "sku": "CPU-9950X",
                "expectedSerials": 1,
                "serials": [{
                    "metaobjectGid": "gid://shopify/Metaobject/77",
                    "serial": "SN123",
                    "scannedAt": "2026-06-10T12:00:00Z",
                    "scannedBy": "gid://staff/1",
                    "reservationStatus": "reserved",
                    "odooLotId": "LOT-9"
                }]
            }],
            "currentStatus": { "gid": "gid://shopify/Metaobject/9", "name": "In QC", "color": "#888", "legacyId": 109 },
            "legalTransitions": [{ "gid": "gid://shopify/Metaobject/10", "name": "Preparing to Ship", "color": "#0a0", "prechecks": [] }],
            "prechecks": [{ "name": "serials_attached", "ok": true }],
            "buildPhotos": [{ "gid": "gid://shopify/MediaImage/1", "url": "https://cdn/x.jpg", "width": 800, "height": 600 }],
            "allStatuses": [{ "gid": "gid://shopify/Metaobject/9", "name": "In QC", "color": "#888", "legacyId": 109, "locked": false }],
            "orderReference": "X0001020",
            "installedSerials": ["gid://shopify/Metaobject/77"],
            "orderDetails": { "customerReference": "PO-9", "splitReps": [] }
        });
        let detail: BuildDetail = serde_json::from_value(raw).unwrap();
        assert_eq!(detail.order.as_ref().unwrap().name, "#1020");
        assert_eq!(detail.current_status.as_ref().unwrap().legacy_id, 109);
        assert_eq!(detail.line_items[0].serials[0].serial, "SN123");
        assert_eq!(detail.build_photos.len(), 1);
        assert_eq!(detail.config.as_ref().unwrap().build_serial.as_deref(), Some("XBS-1020"));
        let selection = detail.config.unwrap().selection.unwrap();
        assert_eq!(selection.as_array().unwrap().len(), 2);
    }

    #[test]
    fn statuses_and_staff_parse() {
        let statuses: StatusesPayload = serde_json::from_value(serde_json::json!({
            "statuses": [{
                "gid": "gid://shopify/Metaobject/9", "legacyId": 76, "name": "Preparing to Ship",
                "color": "#0a0", "bucket": "preparing",
                "productionLocked": false, "editLocked": false, "shipped": false, "paid": true
            }]
        }))
        .unwrap();
        assert_eq!(statuses.statuses[0].legacy_id, 76);

        let staff: StaffPayload = serde_json::from_value(serde_json::json!({
            "staff": [{
                "id": "u1", "name": "Logan", "role": "manager", "active": true,
                "nfcId": null, "hasPin": true, "hasQr": false,
                "createdAt": "2026-01-01T00:00:00Z", "lastLoginAt": null
            }]
        }))
        .unwrap();
        assert_eq!(staff.staff[0].name, "Logan");
        assert!(staff.staff[0].has_pin);
    }

    #[test]
    fn advance_and_scan_results_parse() {
        let advance: AdvanceResult = serde_json::from_value(serde_json::json!({
            "ok": true,
            "currentStatus": { "gid": "g", "name": "Preparing to Ship", "color": "#0a0", "legacyId": 76 }
        }))
        .unwrap();
        assert!(advance.ok);
        assert_eq!(advance.current_status.unwrap().legacy_id, 76);

        let denied: AdvanceResult = serde_json::from_value(serde_json::json!({
            "ok": false, "precheckFailed": true, "error": "serials missing"
        }))
        .unwrap();
        assert!(!denied.ok);
        assert_eq!(denied.precheck_failed, Some(true));

        let scan: ScanSerialResult = serde_json::from_value(serde_json::json!({
            "ok": false, "reason": "no_stock", "canForce": true, "error": "no Odoo stock"
        }))
        .unwrap();
        assert_eq!(scan.reason.as_deref(), Some("no_stock"));
        assert_eq!(scan.can_force, Some(true));
    }

    #[test]
    fn serial_history_parses() {
        let history: SerialHistory = serde_json::from_value(serde_json::json!({
            "serial": "SN123",
            "found": true,
            "shopify": {
                "metaobjectGid": "gid://shopify/Metaobject/77",
                "serial": "SN123",
                "order": { "gid": "gid://shopify/Order/123", "name": "#1020" }
            },
            "odoo": { "lotId": 9, "name": "LOT-9" },
            "prestashop": [],
            "activeRecall": false,
            "batchRmaCount": 0,
            "history": [{ "source": "shopify", "kind": "installed", "at": null, "label": "Installed on #1020" }]
        }))
        .unwrap();
        assert!(history.found);
        assert_eq!(history.odoo.unwrap().lot_id, 9);
        assert_eq!(history.history[0].source, "shopify");
    }
}

// ─── GET /orders/resolve ────────────────────────────────────────────────────

/// One build on a resolved order.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolvedConfig {
    pub gid: String,
    pub config_id: String,
    pub build_name: String,
    pub legacy_config_id: Option<String>,
    pub status: String,
    /// Set only by `GET /orders/{id}`, on the build the response is narrowed to.
    pub active: bool,
}

/// `GET /orders/resolve`. The server decides which reading of the scanned
/// string won; `matched_by` reports it, and `search` means a fuzzy fallback
/// matched and the operator should confirm before loading.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolveResult {
    pub order_gid: String,
    pub name: String,
    pub order_number: String,
    /// PrestaShop `id_order`; null for orders born in Shopify.
    pub legacy_order_id: Option<String>,
    pub config_gid: Option<String>,
    pub config_id: Option<String>,
    pub configs: Vec<ResolvedConfig>,
    pub matched_by: String,
}

impl ResolveResult {
    /// True when the match came from the fuzzy fallback rather than an exact id.
    pub fn is_fuzzy(&self) -> bool {
        self.matched_by == "search"
    }
}

/// Narrows `GET /orders/{id}` to one build; the server derives the other key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigRef<'a> {
    /// `xidax_order_config.config_id`, or the legacy PrestaShop `id_order_config`.
    Id(&'a str),
    /// `xidax_order_config` metaobject GID, as `/orders/resolve` returns it.
    Gid(&'a str),
}

impl ConfigRef<'_> {
    pub(super) fn query_pair(self) -> (&'static str, String) {
        match self {
            Self::Id(id) => ("configId", id.to_string()),
            Self::Gid(gid) => ("configGid", gid.to_string()),
        }
    }
}

// ─── /orders/{id}/comments ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct XbmComment {
    pub id: String,
    pub order_gid: String,
    pub order_config_gid: Option<String>,
    /// `note | qc | stress_fail | asset_tag | system`.
    #[serde(rename = "type")]
    pub kind: String,
    pub body: String,
    pub author_staff_id: Option<String>,
    pub author: String,
    pub visibility: String,
    pub source: Option<String>,
    pub created_at: Option<String>,
}

/// `POST /orders/{id}/comments` wraps the created row in a `comment` key.
/// Every field on `XbmComment` defaults, so deserialising the envelope
/// straight into it silently yields a blank comment instead of an error.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommentEnvelope {
    pub comment: XbmComment,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommentsPayload {
    pub comments: Vec<XbmComment>,
    pub next_before: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentKind {
    Note,
    Qc,
    StressFail,
    AssetTag,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentVisibility {
    Internal,
    Customer,
}

impl CommentVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Customer => "customer",
        }
    }
}

/// `GET /orders/{id}/comments` filters.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommentsQuery<'a> {
    /// This build's comments plus the order-level ones.
    pub config_gid: Option<&'a str>,
    /// Server default 50, max 200.
    pub limit: Option<u32>,
    /// Keyset cursor: the previous page's `next_before`.
    pub before: Option<&'a str>,
    pub visibility: Option<CommentVisibility>,
}

impl CommentsQuery<'_> {
    pub(super) fn query(self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if let Some(gid) = self.config_gid {
            query.push(("configGid", gid.to_string()));
        }
        if let Some(limit) = self.limit {
            query.push(("limit", limit.to_string()));
        }
        if let Some(before) = self.before {
            query.push(("before", before.to_string()));
        }
        if let Some(visibility) = self.visibility {
            query.push(("visibility", visibility.as_str().to_string()));
        }
        query
    }
}

/// `POST /orders/{id}/comments` body.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewComment {
    /// Max 4000 characters.
    pub body: String,
    /// Server default `note`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<CommentKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_config_gid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<CommentVisibility>,
    /// Must match the staff token sent with the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_staff_id: Option<String>,
}

// ─── /orders/{id}/qc ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QcRunDoc {
    pub items: std::collections::HashMap<String, bool>,
    pub status: String,
    pub notes: Option<String>,
    pub tech_email: Option<String>,
    pub updated_at: Option<String>,
}

// ─── POST /staff/authenticate ───────────────────────────────────────────────

/// Which floor credential is being exchanged.
#[derive(Debug, Clone, Copy)]
pub enum StaffAuthMethod<'a> {
    Pin { staff_id: &'a str, pin: &'a str },
    Qr { token: &'a str },
    Nfc { nfc_id: &'a str },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StaffAuth {
    pub staff_id: String,
    pub name: String,
    pub staff_token: String,
    pub expires_at: Option<String>,
    pub ttl_seconds: Option<u64>,
    pub permissions: Vec<String>,
}

impl StaffAuth {
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }
}

// ─── Secrets ────────────────────────────────────────────────────────────────

/// Serializes verbatim; `Debug` never prints the value.
#[derive(Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() {
            "\"\""
        } else {
            "<redacted>"
        })
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_to_string(deserializer).map(Self)
    }
}

/// `true`, `"true"`, `"1"` or `1`; anything else is false.
fn lenient_bool<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Bool(b) => b,
        Value::Number(n) => n.as_i64() == Some(1),
        Value::String(s) => matches!(s.trim(), "true" | "1"),
        _ => false,
    })
}

// ─── /orders/{id}/service ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceRecord {
    pub order_gid: String,
    pub details: ServiceDetails,
}

/// Repair intake document (ps_order_service port); unset fields are empty.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServiceDetails {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_name: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_mfg: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_model: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_serial: String,
    pub device_password: Secret,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_power_supply: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id_status_service: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub other_hardware_software: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub physical_damage: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub check_in_notes: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub intake_notes: String,
    #[serde(deserialize_with = "lenient_bool")]
    pub data_transfer_status: bool,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub qc_tech: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub qc_signoff: String,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
}

/// `PATCH /orders/{id}/service`; only the set fields are merged.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ServicePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_mfg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_password: Option<Secret>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_power_supply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_status_service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_hardware_software: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_damage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_in_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intake_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_transfer_status: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qc_tech: Option<String>,
    /// Requires a staff token holding `qc.perform`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qc_signoff: Option<String>,
}

// ─── /oa3/attach ────────────────────────────────────────────────────────────

/// `GET /oa3/attach`: the Windows line a scanned machine carries.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Oa3AttachTarget {
    /// A prebuilt pool unit (`PB-…`) with no order behind it.
    pub is_pool_unit: bool,
    pub order_gid: Option<String>,
    /// Order name, or the unit's build serial.
    pub name: String,
    pub unit_gid: Option<String>,
    pub build_serial: Option<String>,
    pub model_sku: Option<String>,
    pub unit_status: Option<String>,
    pub config_id: Option<String>,
    pub configs: Vec<ResolvedConfig>,
    /// `pool_unit`, or the `/orders/resolve` reading that won.
    pub matched_by: String,
    /// Empty on a build with no Windows line.
    pub os_lines: Vec<Oa3OsLine>,
    pub already_injected: bool,
    pub injected_keys: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Oa3OsLine {
    pub line_item_id: String,
    pub sku: Option<String>,
    pub title: String,
    pub quantity: Option<i64>,
    /// Keys already recorded on the line.
    pub serials: Vec<String>,
    /// Set on a pool unit, where it is the join key.
    pub variant_gid: Option<String>,
}

/// Hardware as the bench reads it from WMI.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Oa3Hardware {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processor_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_size_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_speed_mhz: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_storage_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motherboard_product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motherboard_serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bios_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bios_serial: Option<String>,
}

/// `POST /oa3/attach` body; only for an injection the technician confirmed.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Oa3AttachRequest {
    /// Scanned value, verbatim.
    #[serde(rename = "ref")]
    pub reference: String,
    pub key: String,
    /// False gets 409 `cbr_required` unless the key is RDPK.
    pub cbr_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_rdpk: Option<bool>,
    /// Required when the order has more than one Windows line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
    /// `HOME | HOMEA | HOMEPREC | PRO | PROA | PROPREC | RDPK`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_edition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware: Option<Oa3Hardware>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Oa3AttachResult {
    pub ok: bool,
    pub is_pool_unit: bool,
    pub order_gid: Option<String>,
    pub unit_gid: Option<String>,
    pub name: String,
    pub line_item_id: String,
    pub sku: Option<String>,
    pub key: String,
    /// Keys already on the line before this attach.
    pub replacing: Option<Vec<String>>,
    pub ordered_sku: Option<String>,
    /// The key's own Odoo product, when Odoo resolved it.
    pub consumed_sku: Option<String>,
    /// Order path: `"reserved" | "pending"`.
    pub reservation_status: Option<String>,
    pub odoo_lot_id: Option<String>,
    /// Pool-unit path only.
    pub serial_gid: Option<String>,
    pub lot_id: Option<String>,
    pub warning: Option<String>,
    pub warnings: Vec<String>,
}

// ─── /oa3/injection-log ─────────────────────────────────────────────────────

/// `POST /oa3/injection-log` body: one injection attempt, failures included.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Oa3InjectionReport {
    /// One per attempt; a replay returns the stored verdict.
    pub client_event_id: String,
    /// COA / firmware key id; a full product key is a 400.
    pub pkid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanned_ref: Option<String>,
    /// At most the last 5 characters of the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key_last5: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_firmware_key: Option<bool>,
    /// Edition the bench decided, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_edition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oa3_verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_rdpk: Option<bool>,
    /// A CBR (Report.xml) exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_present: Option<bool>,
    /// `LicensablePartNumber` from OA3.cfg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku_part_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware: Option<Oa3Hardware>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Oa3InjectionReceipt {
    pub id: String,
    /// `client_event_id` was already recorded.
    pub duplicate: bool,
    pub pkid: String,
    pub derived_tier: Option<String>,
    pub expected_sku: Option<String>,
    /// False when the bench's tier differs from the one the server derives.
    pub tier_agrees: bool,
    /// Other motherboards this PKID was reported against.
    pub prior_boards: Vec<Oa3PriorBoard>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Oa3PriorBoard {
    pub motherboard_serial: Option<String>,
    pub created_at: Option<String>,
    pub scanned_ref: Option<String>,
}

/// `GET /oa3/injection-log` filters.
#[derive(Debug, Clone, Copy, Default)]
pub struct Oa3InjectionQuery<'a> {
    pub pkid: Option<&'a str>,
    pub motherboard_serial: Option<&'a str>,
    /// `Some(false)` lists the rows where bench and server tiers disagree.
    pub tier_agrees: Option<bool>,
    /// Server default 50, max 250.
    pub limit: Option<u32>,
}

impl Oa3InjectionQuery<'_> {
    pub(super) fn query(self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if let Some(pkid) = self.pkid {
            query.push(("pkid", pkid.to_string()));
        }
        if let Some(serial) = self.motherboard_serial {
            query.push(("motherboardSerial", serial.to_string()));
        }
        if let Some(agrees) = self.tier_agrees {
            query.push(("tierAgrees", agrees.to_string()));
        }
        if let Some(limit) = self.limit {
            query.push(("limit", limit.to_string()));
        }
        query
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Oa3InjectionsPayload {
    pub injections: Vec<Oa3Injection>,
}

/// One `OA3Injection` ledger row.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Oa3Injection {
    pub id: String,
    pub client_event_id: String,
    pub scanned_ref: Option<String>,
    pub pkid: String,
    pub product_key_last5: Option<String>,
    pub has_firmware_key: bool,
    pub tool_edition: Option<String>,
    /// `home | pro`.
    pub tool_family: Option<String>,
    /// `standard | advanced | copilot`.
    pub tool_tier: Option<String>,
    pub derived_tier: Option<String>,
    pub expected_sku: Option<String>,
    pub tier_agrees: bool,
    pub oa3_verdict: Option<String>,
    pub ship_ready: bool,
    pub is_rdpk: bool,
    pub report_present: bool,
    pub sku_part_number: Option<String>,
    pub app_version: Option<String>,
    pub machine_name: Option<String>,
    pub processor_name: Option<String>,
    pub memory_size_gb: Option<i64>,
    pub memory_speed_mhz: Option<i64>,
    pub total_storage_gb: Option<i64>,
    pub motherboard_product: Option<String>,
    pub motherboard_serial: Option<String>,
    pub bios_version: Option<String>,
    pub bios_serial: Option<String>,
    /// When the bench injected; `created_at` is when the row arrived.
    pub injected_at: Option<String>,
    pub created_at: Option<String>,
}

// ─── POST /oa3/baseboards ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseboardSighting {
    /// Baseboard product string as WMI reports it.
    pub product_wmi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobile_chassis: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// `skipped` with a `reason` for a placeholder product string.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaseboardReceipt {
    pub skipped: bool,
    pub reason: Option<String>,
    pub product_wmi: Option<String>,
    /// `pending | skipped | covered | ignored`.
    pub status: Option<String>,
    pub injection_count: Option<i64>,
}

// ─── POST /client-errors ────────────────────────────────────────────────────

/// Unhandled exception from a floor app; each text field is capped at 20,000 characters.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientErrorReport {
    pub app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanned_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_exception_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hresult: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at_utc: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientErrorReceipt {
    pub id: String,
    /// Count for this `(app, fingerprint)`; 1 on first sight.
    pub occurrences: i64,
    pub fingerprint: String,
}

// ─── GET /app-versions ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppVersion {
    pub name: String,
    /// Dotted integers, e.g. `2026.9.8.1301`.
    pub version: String,
    /// Lowercase hex digest of the published binary.
    pub sha256: String,
    /// The build must not be used.
    pub retired: bool,
    pub artifact_path: Option<String>,
    pub updated_at: Option<String>,
}

// ─── GET /skus/resolve ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SkuResolution {
    pub sku: String,
    pub product_title: String,
    pub variant_title: String,
    pub vendor: String,
    /// Shopify product status, e.g. `ACTIVE`.
    pub status: String,
    /// Present only when the product's build has a populated DMI profile.
    pub build: Option<SkuBuildProfile>,
}

/// Per-model SMBIOS values from the product's `xidax_build`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SkuBuildProfile {
    pub handle: String,
    pub name: String,
    pub dmi_manufacturer: String,
    pub dmi_system_product: String,
    pub dmi_system_sku: String,
    pub dmi_system_family: String,
}

// ─── GET /builds/cec-label ──────────────────────────────────────────────────

/// How `GET /builds/cec-label` treats the stored `cec_model` snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CecLabelMode {
    /// Replay the snapshot; a first print stores one.
    #[default]
    Replay,
    /// Recompute and re-snapshot.
    Refresh,
    /// Recompute without writing the snapshot.
    DryRun,
}

impl CecLabelMode {
    pub(super) fn query_pair(self) -> Option<(&'static str, String)> {
        match self {
            Self::Replay => None,
            Self::Refresh => Some(("refresh", "1".to_string())),
            Self::DryRun => Some(("dry", "1".to_string())),
        }
    }
}

/// CEC chassis-sticker data; keys are snake_case on the wire.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CecLabel {
    /// The order number, for PrestaShop parity.
    pub serial_number: String,
    /// What the barcode and text line encode, e.g. `3840-53147`.
    pub label_value: String,
    pub model_name: String,
    /// Empty when the component data cannot produce a model number.
    pub cec_model: String,
    /// `YYYYMMDD`, America/Denver.
    pub mfg_date: String,
    /// `xidax | pcl | bimbox`.
    pub brand: String,
    pub order_number: String,
    pub config_id: String,
    /// Replayed from the stored snapshot.
    pub snapshot: bool,
    /// False when `cec_model` is short a segment; see `cec_missing`.
    pub cec_complete: bool,
    pub cec_missing: Vec<CecMissingPart>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CecMissingPart {
    pub sku: String,
    #[serde(rename = "type")]
    pub kind: String,
}

// ─── /corp-deals ────────────────────────────────────────────────────────────

/// `POST /corp-deals` body; Σ(qty × price_cents) over `unit_lines` must equal `unit_price_cents`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCorpDeal {
    pub company_gid: String,
    pub company_location_gid: String,
    pub company_contact_gid: String,
    pub company_name: String,
    pub customer_po: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_ref: Option<String>,
    pub units: u32,
    pub unit_price_cents: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Server default `X-6`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sales_rep_id: Option<String>,
    pub unit_lines: Vec<CorpDealLine>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpDealLine {
    pub variant_gid: String,
    pub sku: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Configurator slot handle, or empty.
    pub slot: String,
    pub qty: u32,
    pub price_cents: i64,
}

/// `POST /corp-deals/{id}` actions; `CreateOrders`, `Release` and `Notify` need workflow scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpDealAction {
    CreateOrders,
    Release,
    BuyList,
    Notify,
    VerifyConfigs,
}

// ─── /rma ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RmaReport {
    OpenRtvAging,
    CreditNotDebited,
    DebitedNoCredit,
    CreditsUndocumented,
    RoundTripIntegrity,
    ReplacementChain,
}

impl RmaReport {
    pub const ALL: [Self; 6] = [
        Self::OpenRtvAging,
        Self::CreditNotDebited,
        Self::DebitedNoCredit,
        Self::CreditsUndocumented,
        Self::RoundTripIntegrity,
        Self::ReplacementChain,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenRtvAging => "open-rtv-aging",
            Self::CreditNotDebited => "credit-not-debited",
            Self::DebitedNoCredit => "debited-no-credit",
            Self::CreditsUndocumented => "credits-undocumented",
            Self::RoundTripIntegrity => "round-trip-integrity",
            Self::ReplacementChain => "replacement-chain",
        }
    }
}

/// `/rma/import-odoo` options, sent as query parameters.
#[derive(Debug, Clone, Copy, Default)]
pub struct RmaImportOptions<'a> {
    /// Also import settled RTVs (the full backfill).
    pub include_settled: bool,
    pub since: Option<&'a str>,
    /// Odoo pages to walk, 1–50; server default 20.
    pub max_pages: Option<u32>,
}

impl RmaImportOptions<'_> {
    pub(super) fn query(self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if self.include_settled {
            query.push(("includeSettled", "1".to_string()));
        }
        if let Some(since) = self.since {
            query.push(("since", since.to_string()));
        }
        if let Some(pages) = self.max_pages {
            query.push(("maxPages", pages.to_string()));
        }
        query
    }
}

// ─── /balance-invoices ──────────────────────────────────────────────────────

/// `op=send` body fields.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceSendRequest {
    /// Bare numeric id or Order GID.
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_message: Option<String>,
    /// Refuses a resend inside this many days; server default 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_interval_days: Option<u32>,
}

/// `op=sweep` body fields; `Default` is a dry run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceSweepRequest {
    /// False emails real customers.
    pub dry_run: bool,
    /// Server default 7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_interval_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// ISO date floor, or `"all"` for the whole order history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub include_legacy_imports: bool,
}

impl Default for BalanceSweepRequest {
    fn default() -> Self {
        Self {
            dry_run: true,
            min_interval_days: None,
            limit: None,
            since: None,
            include_legacy_imports: false,
        }
    }
}

// ─── /roles ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRole {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Permission keys; `"*"` (Full Access) is refused.
    pub permissions: Vec<String>,
}

#[cfg(test)]
mod endpoint_tests {
    //! Payloads mirror the build-management route handlers on origin/main, 2026-09-28.

    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_error_keeps_details() {
        let env: Envelope = serde_json::from_value(json!({
            "ok": false,
            "error": {
                "code": "legacy_order_not_migrated",
                "message": "Serial is installed on PrestaShop order 2145629",
                "details": { "legacyOrderId": "2145629", "serial": "SN1" }
            }
        }))
        .unwrap();
        let details = env.error.unwrap().details.unwrap();
        assert_eq!(details["legacyOrderId"], "2145629");
    }

    #[test]
    fn statuses_carry_retired_and_display_order() {
        let payload: StatusesPayload = serde_json::from_value(json!({
            "statuses": [{
                "gid": "gid://shopify/Metaobject/9", "handle": "status-71", "legacyId": 71,
                "name": "QC & Burn-in", "color": "#888", "bucket": "qc",
                "productionLocked": false, "editLocked": false, "shipped": false, "paid": true,
                "retired": true, "displayOrder": 7,
                "applicableOrderTypes": ["gid://shopify/Metaobject/3"]
            }]
        }))
        .unwrap();
        let status = &payload.statuses[0];
        assert!(status.retired);
        assert_eq!(status.display_order, 7);
        assert_eq!(status.handle, "status-71");
        assert_eq!(status.applicable_order_types.len(), 1);
    }

    #[test]
    fn build_detail_carries_config_attribution() {
        let detail: BuildDetail = serde_json::from_value(json!({
            "lineItems": [{
                "id": "gid://shopify/LineItem/1", "title": "RTX 5090", "qty": 1, "slot": "gpu",
                "configId": "53147", "cartConfigGid": "gid://shopify/Metaobject/404",
                "serials": [{
                    "metaobjectGid": "gid://shopify/Metaobject/77", "serial": "SN1",
                    "reservationStatus": "reserved", "odooLotId": null,
                    "orderConfigGid": "gid://shopify/Metaobject/5"
                }]
            }, {
                "id": "gid://shopify/LineItem/2", "title": "Extra fan", "qty": 1, "slot": "fans",
                "configId": null, "cartConfigGid": null, "serials": []
            }],
            "allStatuses": [{ "gid": "g", "name": "Old", "color": "#000", "legacyId": 2, "locked": false, "retired": true }],
            "configs": [
                { "gid": "gid://shopify/Metaobject/5", "configId": "53147", "buildName": "X-6",
                  "legacyConfigId": "53147", "status": "", "active": true },
                { "gid": "gid://shopify/Metaobject/6", "configId": "53148", "buildName": "X-6",
                  "legacyConfigId": null, "status": "", "active": false }
            ]
        }))
        .unwrap();
        assert_eq!(detail.line_items[0].config_id.as_deref(), Some("53147"));
        assert!(detail.line_items[1].config_id.is_none());
        assert_eq!(
            detail.line_items[0].serials[0].order_config_gid.as_deref(),
            Some("gid://shopify/Metaobject/5")
        );
        assert_eq!(detail.all_statuses[0].retired, Some(true));
        let active: Vec<_> = detail.configs.iter().filter(|c| c.active).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].config_id, "53147");
    }

    #[test]
    fn staff_reports_email_presence_only() {
        let staff: StaffMember = serde_json::from_value(json!({
            "id": "cmq89", "name": "Jane Tech", "role": "floor", "active": true,
            "hasPin": true, "hasQr": true, "hasEmail": true
        }))
        .unwrap();
        assert!(staff.has_email);

        let body = serde_json::to_value(CreateStaffRequest {
            name: "Jane Tech".into(),
            email: Some("jane@example.com".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body,
            json!({ "name": "Jane Tech", "email": "jane@example.com" })
        );
    }

    #[test]
    fn comment_body_uses_wire_names() {
        let body = serde_json::to_value(NewComment {
            body: "Burn-in failed at 3h".into(),
            kind: Some(CommentKind::StressFail),
            order_config_gid: Some("gid://shopify/Metaobject/5".into()),
            visibility: Some(CommentVisibility::Internal),
            actor_staff_id: None,
        })
        .unwrap();
        assert_eq!(
            body,
            json!({
                "body": "Burn-in failed at 3h",
                "type": "stress_fail",
                "orderConfigGid": "gid://shopify/Metaobject/5",
                "visibility": "internal"
            })
        );
    }

    #[test]
    fn release_request_and_results() {
        let body = serde_json::to_value(ReleasePoolUnitRequest {
            disposition: PoolReleaseDisposition::SoldElsewhere,
            reason: Some("Sold at the counter".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body,
            json!({ "disposition": "sold_elsewhere", "reason": "Sold at the counter" })
        );
        assert_eq!(
            serde_json::to_value(ReleasePoolUnitRequest::default()).unwrap(),
            json!({ "disposition": "restock" })
        );

        let released: PoolReleaseResult = serde_json::from_value(json!({
            "ok": true, "mode": "release",
            "unit": {
                "id": "gid://shopify/Metaobject/9", "handle": "pb-00042", "modelSku": "x6-rtx5090-apex",
                "buildSerial": "PB-00042", "warehouseLocation": "B-3-5", "status": "available",
                "condition": "new", "brand": "xidax", "builtAt": null, "builtBy": null,
                "assignedOrderGid": null, "draftOrderGid": null,
                "serialChainGids": ["gid://shopify/Metaobject/77"], "notes": "", "pullPriority": 3
            },
            "odooTransfer": { "ok": true, "summary": "moved 9 lots" },
            "warnings": [], "journalId": "cj1"
        }))
        .unwrap();
        let unit = released.unit.unwrap();
        assert_eq!(unit.status, "available");
        assert!(unit.assigned_order_gid.is_none());
        assert_eq!(unit.serial_chain_gids.len(), 1);

        let refused: PoolReleaseResult = serde_json::from_value(json!({
            "ok": false,
            "error": "Order has a fulfilled line that is not a component line — the machine has shipped.",
            "warnings": []
        }))
        .unwrap();
        assert!(!refused.ok);
        assert!(refused.error.unwrap().contains("shipped"));
    }

    #[test]
    fn service_details_tolerate_legacy_values_and_hide_the_password() {
        let record: ServiceRecord = serde_json::from_value(json!({
            "orderGid": "gid://shopify/Order/1",
            "details": {
                "device_name": "Laptop", "device_password": 1234, "id_status_service": 3,
                "data_transfer_status": "1", "qc_signoff": null, "device_pin": "0000",
                "updated_at": "2026-09-01T10:00:00Z"
            }
        }))
        .unwrap();
        let details = &record.details;
        assert_eq!(details.device_password.expose(), "1234");
        assert_eq!(details.id_status_service, "3");
        assert!(details.data_transfer_status);
        assert_eq!(details.qc_signoff, "");
        assert!(!format!("{details:?}").contains("1234"));

        let empty: ServiceRecord =
            serde_json::from_value(json!({ "orderGid": "gid://shopify/Order/2", "details": {} }))
                .unwrap();
        assert!(empty.details.device_password.is_empty());
        assert!(!empty.details.data_transfer_status);
    }

    #[test]
    fn service_patch_sends_snake_case_subset() {
        let patch = ServicePatch {
            device_password: Some(Secret::new("hunter2")),
            data_transfer_status: Some(false),
            intake_notes: Some("No charger".into()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            json!({ "device_password": "hunter2", "data_transfer_status": false, "intake_notes": "No charger" })
        );
        assert!(!format!("{patch:?}").contains("hunter2"));
    }

    #[test]
    fn oa3_attach_target_order_and_pool_readings() {
        let order: Oa3AttachTarget = serde_json::from_value(json!({
            "isPoolUnit": false, "orderGid": "gid://shopify/Order/7280155787490", "name": "#3840",
            "unitGid": null, "buildSerial": null, "modelSku": null, "unitStatus": null,
            "configId": "53147",
            "configs": [{ "gid": "gid://shopify/Metaobject/5", "configId": "53147", "buildName": "X-6",
                          "legacyConfigId": "53147", "status": "" }],
            "matchedBy": "config_pair",
            "osLines": [{ "lineItemId": "gid://shopify/LineItem/1", "sku": "SW/WIN11HMA/OA3",
                          "title": "Windows 11 Home", "serials": [], "variantGid": null }],
            "alreadyInjected": false, "injectedKeys": []
        }))
        .unwrap();
        assert!(!order.is_pool_unit);
        assert_eq!(order.os_lines[0].sku.as_deref(), Some("SW/WIN11HMA/OA3"));
        assert!(order.os_lines[0].quantity.is_none());

        let pool: Oa3AttachTarget = serde_json::from_value(json!({
            "isPoolUnit": true, "orderGid": null, "name": "PB-00042",
            "unitGid": "gid://shopify/Metaobject/9", "buildSerial": "PB-00042",
            "modelSku": "x6-rtx5090-apex", "unitStatus": "building", "configId": null, "configs": [],
            "matchedBy": "pool_unit",
            "osLines": [{ "lineItemId": "pool-os-1", "sku": "SW/WIN11PRO/OA3", "title": "Windows 11 Pro",
                          "quantity": 1, "serials": ["AAAAA-BBBBB-CCCCC-DDDDD-EEEEE"],
                          "variantGid": "gid://shopify/ProductVariant/3" }],
            "alreadyInjected": true, "injectedKeys": ["AAAAA-BBBBB-CCCCC-DDDDD-EEEEE"]
        }))
        .unwrap();
        assert!(pool.is_pool_unit && pool.already_injected);
        assert_eq!(pool.matched_by, "pool_unit");
        assert!(pool.order_gid.is_none());
        assert_eq!(
            pool.os_lines[0].variant_gid.as_deref(),
            Some("gid://shopify/ProductVariant/3")
        );
    }

    #[test]
    fn oa3_attach_request_uses_ref_and_cbr_flag() {
        let body = serde_json::to_value(Oa3AttachRequest {
            reference: "3840-53147".into(),
            key: "AAAAA-BBBBB-CCCCC-DDDDD-EEEEE".into(),
            cbr_present: true,
            tool_edition: Some("HOMEA".into()),
            hardware: Some(Oa3Hardware {
                processor_name: Some("AMD Ryzen 7 7800X3D".into()),
                memory_size_gb: Some(32),
                total_storage_gb: Some(2000),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body,
            json!({
                "ref": "3840-53147",
                "key": "AAAAA-BBBBB-CCCCC-DDDDD-EEEEE",
                "cbrPresent": true,
                "toolEdition": "HOMEA",
                "hardware": { "processorName": "AMD Ryzen 7 7800X3D", "memorySizeGb": 32, "totalStorageGb": 2000 }
            })
        );
    }

    #[test]
    fn oa3_attach_result_decodes_both_paths() {
        let order: Oa3AttachResult = serde_json::from_value(json!({
            "ok": true, "reservationStatus": "reserved", "odooLotId": "LOT-9",
            "isPoolUnit": false, "orderGid": "gid://shopify/Order/1", "unitGid": null, "name": "#3840",
            "lineItemId": "gid://shopify/LineItem/1", "sku": "SW/WIN11HMA/OA3",
            "key": "AAAAA-BBBBB-CCCCC-DDDDD-EEEEE", "replacing": null,
            "orderedSku": "SW/WIN11HMA/OA3", "consumedSku": "SW/WIN11HM/OA3",
            "warnings": ["Standard licence on Advanced hardware"]
        }))
        .unwrap();
        assert_eq!(order.consumed_sku.as_deref(), Some("SW/WIN11HM/OA3"));
        assert!(order.replacing.is_none());
        assert_eq!(order.warnings.len(), 1);

        let pool: Oa3AttachResult = serde_json::from_value(json!({
            "ok": true, "serialGid": "gid://shopify/Metaobject/88", "lotId": "",
            "isPoolUnit": true, "orderGid": null, "unitGid": "gid://shopify/Metaobject/9",
            "name": "PB-00042", "lineItemId": "pool-os-1", "sku": null, "key": "K",
            "replacing": ["OLD-KEY"], "orderedSku": null, "consumedSku": null, "warnings": []
        }))
        .unwrap();
        assert_eq!(
            pool.serial_gid.as_deref(),
            Some("gid://shopify/Metaobject/88")
        );
        assert_eq!(pool.replacing.unwrap(), vec!["OLD-KEY".to_string()]);
    }

    #[test]
    fn injection_report_and_receipt() {
        let body = serde_json::to_value(Oa3InjectionReport {
            client_event_id: "oa3iw-BENCH07-8f2c".into(),
            pkid: "3422292130751".into(),
            occurred_at_utc: Some("2026-08-24T18:02:11Z".into()),
            product_key_last5: Some("3V66T".into()),
            oa3_verdict: Some("ShipReady".into()),
            ship_ready: Some(true),
            hardware: Some(Oa3Hardware {
                motherboard_serial: Some("MB-1".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body,
            json!({
                "clientEventId": "oa3iw-BENCH07-8f2c",
                "pkid": "3422292130751",
                "occurredAtUtc": "2026-08-24T18:02:11Z",
                "productKeyLast5": "3V66T",
                "oa3Verdict": "ShipReady",
                "shipReady": true,
                "hardware": { "motherboardSerial": "MB-1" }
            })
        );

        let receipt: Oa3InjectionReceipt = serde_json::from_value(json!({
            "id": "cinj1", "tierAgrees": false, "derivedTier": "advanced", "expectedSku": "SW/WIN11HMA/OA3",
            "duplicate": false, "pkid": "3422292130751",
            "priorBoards": [{ "motherboardSerial": "MB-0", "createdAt": "2026-08-20T10:00:00.000Z",
                              "scannedRef": "2150535-138557" }],
            "warnings": ["This key was already reported against 1 other motherboard(s)."]
        }))
        .unwrap();
        assert!(!receipt.tier_agrees);
        assert_eq!(
            receipt.prior_boards[0].motherboard_serial.as_deref(),
            Some("MB-0")
        );

        let replay: Oa3InjectionReceipt = serde_json::from_value(json!({
            "id": "cinj1", "tierAgrees": true, "derivedTier": null, "expectedSku": null,
            "duplicate": true, "pkid": "3422292130751", "priorBoards": [], "warnings": []
        }))
        .unwrap();
        assert!(replay.duplicate);
    }

    #[test]
    fn injection_ledger_row_decodes() {
        let payload: Oa3InjectionsPayload = serde_json::from_value(json!({
            "injections": [{
                "id": "cinj1", "clientEventId": "oa3iw-BENCH07-8f2c", "scannedRef": "2150535-138557",
                "pkid": "3422292130751", "productKeyLast5": "3V66T", "hasFirmwareKey": true,
                "toolEdition": "HOMEA", "toolFamily": "home", "toolTier": "advanced",
                "derivedTier": "advanced", "expectedSku": "SW/WIN11HMA/OA3", "tierAgrees": true,
                "oa3Verdict": "ShipReady", "shipReady": true, "isRdpk": false, "reportPresent": true,
                "skuPartNumber": null, "appVersion": "2026.8.24.1", "machineName": "BENCH07",
                "processorName": "AMD Ryzen 7 7800X3D", "memorySizeGb": 32, "memorySpeedMhz": null,
                "totalStorageGb": 2000, "motherboardProduct": "B650", "motherboardSerial": "MB-1",
                "biosVersion": null, "biosSerial": null,
                "injectedAt": "2026-08-24T18:02:11.000Z", "createdAt": "2026-08-24T18:02:12.000Z"
            }]
        }))
        .unwrap();
        let row = &payload.injections[0];
        assert_eq!(row.tool_tier.as_deref(), Some("advanced"));
        assert_eq!(row.memory_size_gb, Some(32));
        assert!(row.report_present && row.tier_agrees);
    }

    #[test]
    fn baseboard_sighting_and_receipts() {
        let body = serde_json::to_value(BaseboardSighting {
            product_wmi: "PRO B650M-A WIFI (MS-7E26)".into(),
            manufacturer: Some("Micro-Star International Co., Ltd.".into()),
            mobile_chassis: Some(false),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(body["productWmi"], "PRO B650M-A WIFI (MS-7E26)");
        assert_eq!(body["mobileChassis"], false);

        let skipped: BaseboardReceipt =
            serde_json::from_value(json!({ "skipped": true, "reason": "generic_name" })).unwrap();
        assert!(skipped.skipped);
        assert_eq!(skipped.reason.as_deref(), Some("generic_name"));

        let counted: BaseboardReceipt = serde_json::from_value(json!({
            "skipped": false, "productWmi": "PRO B650M-A WIFI", "status": "covered", "injectionCount": 12
        }))
        .unwrap();
        assert_eq!(counted.status.as_deref(), Some("covered"));
        assert_eq!(counted.injection_count, Some(12));
    }

    #[test]
    fn client_error_and_app_version() {
        let body = serde_json::to_value(ClientErrorReport {
            app: "MasterTech".into(),
            app_version: Some("4.8.5".into()),
            exception_type: Some("panic".into()),
            stack_trace: Some("at main".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body,
            json!({ "app": "MasterTech", "appVersion": "4.8.5", "exceptionType": "panic", "stackTrace": "at main" })
        );

        let receipt: ClientErrorReceipt = serde_json::from_value(json!({
            "id": "cerr1", "occurrences": 4, "fingerprint": "9f86d081884c7d659a2feaa0c55ad015"
        }))
        .unwrap();
        assert_eq!(receipt.occurrences, 4);

        let version: AppVersion = serde_json::from_value(json!({
            "name": "OA3InjectionWrapper", "version": "2026.9.8.1301",
            "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "retired": false, "updatedAt": "2026-09-08T19:01:00.000Z"
        }))
        .unwrap();
        assert_eq!(version.version, "2026.9.8.1301");
        assert!(version.artifact_path.is_none());
    }

    #[test]
    fn sku_resolution_with_and_without_a_build() {
        let bare: SkuResolution = serde_json::from_value(json!({
            "sku": "CASE/X6", "productTitle": "X-6 Chassis", "variantTitle": "Default Title",
            "vendor": "Xidax", "status": "ACTIVE"
        }))
        .unwrap();
        assert!(bare.build.is_none());

        let bimbox: SkuResolution = serde_json::from_value(json!({
            "sku": "LAP/BB/165070TI", "productTitle": "BIMBOX Slim 16", "variantTitle": "Default Title",
            "vendor": "BIMBOX", "status": "ACTIVE",
            "build": { "handle": "bimbox-slim-16", "name": "BIMBOX Slim 16", "dmiManufacturer": "BIMBOX",
                       "dmiSystemProduct": "Slim 16", "dmiSystemSku": "BB-S16", "dmiSystemFamily": "Slim" }
        }))
        .unwrap();
        assert_eq!(bimbox.build.unwrap().dmi_system_family, "Slim");
    }

    #[test]
    fn cec_label_decodes_snake_case() {
        let label: CecLabel = serde_json::from_value(json!({
            "serial_number": "3840", "label_value": "3840-53147", "model_name": "X-Series",
            "cec_model": "", "mfg_date": "20260811", "brand": "xidax", "order_number": "3840",
            "config_id": "53147", "snapshot": false, "cec_complete": false,
            "cec_missing": [{ "sku": "PSU/1000W", "type": "psu" }],
            "warnings": ["psu PSU/1000W has no xidax_cec.model_number — cec_model is missing that segment."]
        }))
        .unwrap();
        assert_eq!(label.label_value, "3840-53147");
        assert!(!label.cec_complete);
        assert_eq!(label.cec_missing[0].kind, "psu");
    }

    #[test]
    fn back_office_bodies() {
        let deal = serde_json::to_value(NewCorpDeal {
            company_gid: "gid://shopify/Company/1".into(),
            company_location_gid: "gid://shopify/CompanyLocation/1".into(),
            company_contact_gid: "gid://shopify/CompanyContact/1".into(),
            company_name: "Acme".into(),
            customer_po: "PO-9".into(),
            units: 40,
            unit_price_cents: 150_000,
            unit_lines: vec![CorpDealLine {
                variant_gid: "gid://shopify/ProductVariant/1".into(),
                sku: "CPU/7800X3D".into(),
                slot: "processor".into(),
                qty: 1,
                price_cents: 150_000,
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(deal["unitPriceCents"], 150_000);
        assert_eq!(deal["unitLines"][0]["priceCents"], 150_000);
        assert!(deal.get("quoteRef").is_none());

        assert_eq!(
            serde_json::to_value(CorpDealAction::CreateOrders).unwrap(),
            "create-orders"
        );
        assert_eq!(
            serde_json::to_value(CorpDealAction::VerifyConfigs).unwrap(),
            "verify-configs"
        );

        assert_eq!(
            serde_json::to_value(BalanceSweepRequest::default()).unwrap(),
            json!({ "dryRun": true })
        );

        let paths: std::collections::HashSet<_> =
            RmaReport::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(paths.len(), RmaReport::ALL.len());
    }
}
