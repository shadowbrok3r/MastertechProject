//! Serde types for the Xidax Build Management API (`/api/v1/*`).
//!
//! Field names mirror the camelCase JSON the Remix app returns; structs stay
//! permissive (`default` everywhere) because the server adds fields freely.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Workflow buckets accepted by `GET /orders?bucket=`.
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
    pub legacy_id: i64,
    pub name: String,
    pub color: String,
    /// `"awaiting_parts" | "building" | "qc" | "preparing" | "shipped" | ""`.
    pub bucket: String,
    pub production_locked: bool,
    pub edit_locked: bool,
    pub shipped: bool,
    pub paid: bool,
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
