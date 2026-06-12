//! Xidax Build Management API client (`https://build-mgmt.xidax.com/api/v1`).
//!
//! Auth: `Authorization: Bearer xbm_<40 hex>` per-consumer key with csv scopes
//! (read | write | workflow). Every response uses the envelope
//! `{ok:true,data}` / `{ok:false,error:{code,message}}`; order ids accept bare
//! numerics or full Shopify GIDs. Rate limit: 120 req/min per key.
//!
//! This is the sanctioned Shopify surface for bench/floor machines — they hold
//! one `xbm_` key instead of an Admin token, and side-effectful actions
//! (advance, scan/detach serial, ship) run their Odoo/email legs server-side.

pub mod types;

use serde::de::DeserializeOwned;
use serde_json::Value;

pub use types::*;

use crate::{XBM_API_KEY, XBM_API_URL};

/// API error: transport, or a decoded `{code,message}` envelope error.
#[derive(Debug, thiserror::Error)]
pub enum XbmError {
    #[error("XBM API not configured — set XBM_API_KEY in .env and rebuild")]
    NotConfigured,
    #[error("XBM transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("XBM {status} {code}: {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
        /// Seconds from the `Retry-After` header on 429s.
        retry_after_secs: Option<u64>,
    },
    #[error("XBM response decode failed: {0}")]
    Decode(String),
}

impl XbmError {
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Api { status: 429, .. })
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Api { status: 404, .. })
    }
}

#[derive(Clone)]
pub struct XbmClient {
    base_url: String,
    key: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for XbmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XbmClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl XbmClient {
    pub fn new(base_url: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            key: key.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Compile-time `.env` configuration (`XBM_API_URL`, `XBM_API_KEY`).
    pub fn from_env() -> Self {
        Self::new(XBM_API_URL, XBM_API_KEY)
    }

    /// `false` until an `xbm_` key is configured; every call errors then.
    pub fn configured(&self) -> bool {
        !self.base_url.is_empty() && self.key.starts_with("xbm_")
    }

    /// `gid://shopify/Order/123` → `123`; passthrough otherwise. Routes take
    /// bare numerics without URL-encoding headaches.
    pub fn order_path_id(order_id: &str) -> &str {
        order_id.rsplit('/').next().unwrap_or(order_id)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<T, XbmError> {
        if !self.configured() {
            return Err(XbmError::NotConfigured);
        }
        let url = format!("{}{}", self.base_url, path);
        let mut req = self
            .http
            .request(method, &url)
            .bearer_auth(&self.key)
            .query(query);
        if let Some(body) = body {
            req = req.json(body);
        }
        let response = req.send().await?;
        let status = response.status().as_u16();
        let retry_after_secs = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());
        let envelope: Envelope = response
            .json()
            .await
            .map_err(|e| XbmError::Decode(format!("{path}: {e}")))?;

        if !envelope.ok {
            let err = envelope.error.unwrap_or(EnvelopeError {
                code: "unknown".into(),
                message: format!("HTTP {status} with no error body"),
            });
            return Err(XbmError::Api {
                status,
                code: err.code,
                message: err.message,
                retry_after_secs,
            });
        }
        let data = envelope.data.unwrap_or(Value::Null);
        serde_json::from_value(data).map_err(|e| XbmError::Decode(format!("{path}: {e}")))
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, XbmError> {
        self.request(reqwest::Method::GET, path, query, None).await
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, XbmError> {
        self.request(reqwest::Method::POST, path, &[], Some(&body)).await
    }

    async fn patch<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, XbmError> {
        self.request(reqwest::Method::PATCH, path, &[], Some(&body)).await
    }

    async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T, XbmError> {
        self.request(reqwest::Method::DELETE, path, &[], None).await
    }

    // ─── Orders ─────────────────────────────────────────────────────────────

    /// `GET /orders`. `buckets` empty = server default active-floor set.
    pub async fn orders(
        &self,
        buckets: &[&str],
        sales_rep: Option<&str>,
        order_type: Option<&str>,
    ) -> Result<QueuePayload, XbmError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if !buckets.is_empty() {
            query.push(("bucket", buckets.join(",")));
        }
        if let Some(rep) = sales_rep {
            query.push(("salesRep", rep.to_string()));
        }
        if let Some(ot) = order_type {
            query.push(("orderType", ot.to_string()));
        }
        self.get("/orders", &query).await
    }

    /// `GET /orders/{id}` — full build detail + order-details block.
    pub async fn order_detail(&self, order_id: &str) -> Result<BuildDetail, XbmError> {
        self.get(&format!("/orders/{}", Self::order_path_id(order_id)), &[])
            .await
    }

    /// `PATCH /orders/{id}` — order-details fields (scope: write).
    pub async fn update_order_details(
        &self,
        order_id: &str,
        patch: &OrderDetailsPatch,
    ) -> Result<Value, XbmError> {
        self.patch(
            &format!("/orders/{}", Self::order_path_id(order_id)),
            serde_json::to_value(patch).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `POST /orders/{id}/advance` — workflow transition (scope: workflow).
    pub async fn advance_order(
        &self,
        order_id: &str,
        request: &AdvanceRequest,
    ) -> Result<AdvanceResult, XbmError> {
        self.post(
            &format!("/orders/{}/advance", Self::order_path_id(order_id)),
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `POST /orders/{id}/scan-serial` — attach a component serial; reserves
    /// the Odoo lot server-side (scope: workflow).
    pub async fn scan_serial(
        &self,
        order_id: &str,
        request: &ScanSerialRequest,
    ) -> Result<ScanSerialResult, XbmError> {
        self.post(
            &format!("/orders/{}/scan-serial", Self::order_path_id(order_id)),
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `POST /orders/{id}/detach-serial` — releases the Odoo lot (scope: workflow).
    pub async fn detach_serial(
        &self,
        order_id: &str,
        request: &DetachSerialRequest,
    ) -> Result<DetachSerialResult, XbmError> {
        self.post(
            &format!("/orders/{}/detach-serial", Self::order_path_id(order_id)),
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `POST /orders/{id}/ship` — fulfillment + tracking + workflow advance.
    pub async fn ship_order(
        &self,
        order_id: &str,
        request: &ShipRequest,
    ) -> Result<ShipResult, XbmError> {
        self.post(
            &format!("/orders/{}/ship", Self::order_path_id(order_id)),
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `POST /orders/{id}/assign-pool-unit` — FIFO+CAS pool claim.
    pub async fn assign_pool_unit(
        &self,
        order_id: &str,
        request: &AssignPoolUnitRequest,
    ) -> Result<Value, XbmError> {
        self.post(
            &format!("/orders/{}/assign-pool-unit", Self::order_path_id(order_id)),
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    // ─── Statuses / serials / staff ─────────────────────────────────────────

    /// `GET /statuses` — all workflow statuses with `legacy_id` mapping.
    pub async fn statuses(&self) -> Result<StatusesPayload, XbmError> {
        self.get("/statuses", &[]).await
    }

    /// `GET /serials/{serial}` — federated Shopify + Odoo + PrestaShop history.
    pub async fn serial_history(&self, serial: &str) -> Result<SerialHistory, XbmError> {
        self.get(&format!("/serials/{serial}"), &[]).await
    }

    /// `GET /staff`.
    pub async fn staff(&self, active: Option<bool>) -> Result<StaffPayload, XbmError> {
        let query: Vec<(&str, String)> = active
            .map(|a| vec![("active", a.to_string())])
            .unwrap_or_default();
        self.get("/staff", &query).await
    }

    /// `GET /staff/{id}`.
    pub async fn staff_member(&self, staff_id: &str) -> Result<StaffMember, XbmError> {
        self.get(&format!("/staff/{staff_id}"), &[]).await
    }

    /// `POST /staff` — `qr_token` is returned once (scope: write).
    pub async fn create_staff(
        &self,
        request: &CreateStaffRequest,
    ) -> Result<CreateStaffResult, XbmError> {
        self.post(
            "/staff",
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `PATCH /staff/{id}` (scope: write).
    pub async fn update_staff(&self, staff_id: &str, patch: Value) -> Result<Value, XbmError> {
        self.patch(&format!("/staff/{staff_id}"), patch).await
    }

    /// `DELETE /staff/{id}` — soft delete (scope: write).
    pub async fn delete_staff(&self, staff_id: &str) -> Result<Value, XbmError> {
        self.delete(&format!("/staff/{staff_id}")).await
    }

    // ─── Dashboard / pool ───────────────────────────────────────────────────

    /// `GET /dashboard-metrics` — floor KPI payload.
    pub async fn dashboard_metrics(&self) -> Result<DashboardMetrics, XbmError> {
        self.get("/dashboard-metrics", &[]).await
    }

    /// `GET /prebuilt-units`.
    pub async fn prebuilt_units(&self, model: Option<&str>) -> Result<Value, XbmError> {
        let query: Vec<(&str, String)> = model
            .map(|m| vec![("model", m.to_string())])
            .unwrap_or_default();
        self.get("/prebuilt-units", &query).await
    }

    /// `GET /prebuilt-units/{id}`.
    pub async fn prebuilt_unit(&self, unit_id: &str) -> Result<Value, XbmError> {
        self.get(&format!("/prebuilt-units/{unit_id}"), &[]).await
    }

    /// `POST /prebuilt-units` (scope: write).
    pub async fn create_prebuilt_unit(
        &self,
        request: &CreatePrebuiltUnitRequest,
    ) -> Result<Value, XbmError> {
        self.post(
            "/prebuilt-units",
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `PATCH /prebuilt-units/{id}` — bookkeeping only, no Odoo side effects.
    pub async fn update_prebuilt_unit(
        &self,
        unit_id: &str,
        request: &UpdatePrebuiltUnitRequest,
    ) -> Result<Value, XbmError> {
        self.patch(
            &format!("/prebuilt-units/{unit_id}"),
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `GET /pool-summary` — per-model rollup + replenishment targets.
    pub async fn pool_summary(&self) -> Result<PoolSummaryPayload, XbmError> {
        self.get("/pool-summary", &[]).await
    }

    // ─── Debuilds / vendors / POs / inventory (Value-level v1) ──────────────

    /// `GET /debuilds`.
    pub async fn debuilds(&self, status: Option<&str>) -> Result<Value, XbmError> {
        let query: Vec<(&str, String)> = status
            .map(|s| vec![("status", s.to_string())])
            .unwrap_or_default();
        self.get("/debuilds", &query).await
    }

    /// `POST /debuilds` (scope: write).
    pub async fn create_debuild(&self, request: &CreateDebuildRequest) -> Result<Value, XbmError> {
        self.post(
            "/debuilds",
            serde_json::to_value(request).map_err(|e| XbmError::Decode(e.to_string()))?,
        )
        .await
    }

    /// `GET /debuilds/{id}`.
    pub async fn debuild(&self, debuild_id: &str) -> Result<Value, XbmError> {
        self.get(&format!("/debuilds/{debuild_id}"), &[]).await
    }

    /// `PATCH /debuilds/{id}` (scope: write).
    pub async fn update_debuild(&self, debuild_id: &str, patch: Value) -> Result<Value, XbmError> {
        self.patch(&format!("/debuilds/{debuild_id}"), patch).await
    }

    /// `GET /vendors`.
    pub async fn vendors(&self, q: Option<&str>) -> Result<Value, XbmError> {
        let query: Vec<(&str, String)> =
            q.map(|q| vec![("q", q.to_string())]).unwrap_or_default();
        self.get("/vendors", &query).await
    }

    /// `POST /vendors` — idempotent upsert (scope: write).
    pub async fn upsert_vendor(&self, vendor: Value) -> Result<Value, XbmError> {
        self.post("/vendors", vendor).await
    }

    /// `POST /vendors/import-odoo` (scope: write).
    pub async fn import_vendors_from_odoo(&self) -> Result<Value, XbmError> {
        self.post("/vendors/import-odoo", Value::Null).await
    }

    /// `GET /purchase-orders`.
    pub async fn purchase_orders(
        &self,
        status: Option<&str>,
        q: Option<&str>,
    ) -> Result<Value, XbmError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(s) = status {
            query.push(("status", s.to_string()));
        }
        if let Some(q_str) = q {
            query.push(("q", q_str.to_string()));
        }
        self.get("/purchase-orders", &query).await
    }

    /// `GET /purchase-orders/{id}`.
    pub async fn purchase_order(&self, po_id: &str) -> Result<Value, XbmError> {
        self.get(&format!("/purchase-orders/{po_id}"), &[]).await
    }

    /// `POST /purchase-orders` (scope: write).
    pub async fn create_purchase_order(&self, body: Value) -> Result<Value, XbmError> {
        self.post("/purchase-orders", body).await
    }

    /// `POST /purchase-orders/{id}` lifecycle action (mark-ordered pushes to Odoo).
    pub async fn purchase_order_action(&self, po_id: &str, body: Value) -> Result<Value, XbmError> {
        self.post(&format!("/purchase-orders/{po_id}"), body).await
    }

    /// `POST /purchase-orders/{id}/receive` — accepted qty lands in Shopify inventory.
    pub async fn receive_purchase_order(&self, po_id: &str, body: Value) -> Result<Value, XbmError> {
        self.post(&format!("/purchase-orders/{po_id}/receive"), body).await
    }

    /// `GET /inventory-counts`.
    pub async fn inventory_counts(&self) -> Result<Value, XbmError> {
        self.get("/inventory-counts", &[]).await
    }

    /// `GET /inventory-counts/{id}`.
    pub async fn inventory_count(
        &self,
        count_id: &str,
        since: Option<&str>,
    ) -> Result<Value, XbmError> {
        let query: Vec<(&str, String)> = since
            .map(|s| vec![("since", s.to_string())])
            .unwrap_or_default();
        self.get(&format!("/inventory-counts/{count_id}"), &query).await
    }

    /// `POST /inventory-counts` (scope: write).
    pub async fn create_inventory_count(&self, body: Value) -> Result<Value, XbmError> {
        self.post("/inventory-counts", body).await
    }

    /// `POST /inventory-counts/{id}` state action: snapshot | close | reopen.
    pub async fn inventory_count_action(
        &self,
        count_id: &str,
        action: &str,
    ) -> Result<Value, XbmError> {
        self.post(
            &format!("/inventory-counts/{count_id}"),
            serde_json::json!({ "action": action }),
        )
        .await
    }

    /// `POST /inventory-counts/{id}/scans` (scope: write).
    pub async fn record_inventory_scan(
        &self,
        count_id: &str,
        serial: &str,
        scanned_by: Option<&str>,
    ) -> Result<Value, XbmError> {
        self.post(
            &format!("/inventory-counts/{count_id}/scans"),
            serde_json::json!({ "serial": serial, "scannedBy": scanned_by }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_path_id_handles_gid_and_bare() {
        assert_eq!(XbmClient::order_path_id("gid://shopify/Order/123"), "123");
        assert_eq!(XbmClient::order_path_id("123"), "123");
    }

    #[test]
    fn unconfigured_client_reports_unconfigured() {
        let client = XbmClient::new("https://build-mgmt.xidax.com/api/v1", "");
        assert!(!client.configured());
        let client = XbmClient::new("", "xbm_abc");
        assert!(!client.configured());
        let client = XbmClient::new("https://build-mgmt.xidax.com/api/v1/", "xbm_abc");
        assert!(client.configured());
    }

    #[test]
    fn envelope_error_decodes() {
        let envelope: Envelope = serde_json::from_value(serde_json::json!({
            "ok": false,
            "error": { "code": "rate_limited", "message": "slow down" }
        }))
        .unwrap();
        assert!(!envelope.ok);
        assert_eq!(envelope.error.unwrap().code, "rate_limited");
    }
}
