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

/// Process-wide reqwest client; cloning shares its connection pool + TLS cache.
pub(crate) fn shared_http() -> reqwest::Client {
    crate::shared_http()
}

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
        /// The envelope's `error.details`, e.g. `legacyOrderId`.
        details: Option<Value>,
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

fn json_body<T: serde::Serialize>(value: &T) -> Result<Value, XbmError> {
    serde_json::to_value(value).map_err(|e| XbmError::Decode(e.to_string()))
}

#[derive(Clone)]
pub struct XbmClient {
    base_url: String,
    key: String,
    /// Target store. The API is multi-store and defaults to Xidax, so PC
    /// Laptops orders return nothing unless this is set.
    shop: String,
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
            shop: String::new(),
            http: shared_http(),
        }
    }

    /// Compile-time `.env` configuration (`XBM_API_URL`, `XBM_API_KEY`).
    pub fn from_env() -> Self {
        Self::new(XBM_API_URL, XBM_API_KEY).for_shop(crate::XBM_SHOP)
    }

    /// Target a specific store, e.g. `pclaptops`. An empty value leaves the
    /// server's default (Xidax) in place.
    pub fn for_shop(mut self, shop: &str) -> Self {
        self.shop = shop.trim().to_string();
        self
    }

    pub fn shop(&self) -> &str {
        &self.shop
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
        self.request_as(method, path, query, body, None).await
    }

    /// `staff_token` attributes the call to a technician instead of the API
    /// key's system identity. Required by the QC and comment write routes.
    async fn request_as<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        staff_token: Option<&str>,
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
        if !self.shop.is_empty() {
            req = req.query(&[("shop", self.shop.as_str())]);
        }
        if let Some(token) = staff_token {
            req = req.header("X-Staff-Token", token);
        }
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
                details: None,
            });
            return Err(XbmError::Api {
                status,
                code: err.code,
                message: err.message,
                retry_after_secs,
                details: err.details,
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

    /// Untyped GET, for the endpoints with no typed wrapper — the Odoo
    /// passthroughs and the rest of the surface absent from
    /// `/api/v1/openapi.json`.
    pub async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value, XbmError> {
        self.get(path, query).await
    }

    /// Untyped POST. Same reason as [`Self::get_json`].
    pub async fn post_json(&self, path: &str, body: Value) -> Result<Value, XbmError> {
        self.post(path, body).await
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

    /// `GET /orders/resolve?ref=`. The server tries every reading of `ref`
    /// (order number, build-sheet pair, reference, legacy PrestaShop id,
    /// serial) — pass what was scanned verbatim and never pre-parse it.
    pub async fn resolve(&self, reference: &str) -> Result<ResolveResult, XbmError> {
        self.get("/orders/resolve", &[("ref", reference.to_string())]).await
    }

    /// `GET /orders/{id}/comments`, newest first.
    pub async fn comments(
        &self,
        order_id: &str,
        config_gid: Option<&str>,
        limit: Option<u32>,
    ) -> Result<CommentsPayload, XbmError> {
        let filter = CommentsQuery {
            config_gid,
            limit,
            ..Default::default()
        };
        self.comments_with(order_id, filter).await
    }

    /// `GET /orders/{id}/comments` with paging and visibility filters.
    pub async fn comments_with(
        &self,
        order_id: &str,
        filter: CommentsQuery<'_>,
    ) -> Result<CommentsPayload, XbmError> {
        self.get(
            &format!("/orders/{}/comments", Self::order_path_id(order_id)),
            &filter.query(),
        )
        .await
    }

    /// `POST /orders/{id}/comments`. `actor_staff_id` must match the staff
    /// token, and the API rejects the pair being half-supplied.
    pub async fn post_comment(
        &self,
        order_id: &str,
        body: &str,
        staff_token: Option<&str>,
        actor_staff_id: Option<&str>,
    ) -> Result<XbmComment, XbmError> {
        let comment = NewComment {
            body: body.to_string(),
            kind: Some(CommentKind::Note),
            actor_staff_id: actor_staff_id.map(str::to_string),
            ..Default::default()
        };
        self.post_comment_with(order_id, &comment, staff_token)
            .await
    }

    /// `POST /orders/{id}/comments` with build scope, kind and visibility.
    pub async fn post_comment_with(
        &self,
        order_id: &str,
        comment: &NewComment,
        staff_token: Option<&str>,
    ) -> Result<XbmComment, XbmError> {
        let envelope: CommentEnvelope = self
            .request_as(
                reqwest::Method::POST,
                &format!("/orders/{}/comments", Self::order_path_id(order_id)),
                &[],
                Some(&json_body(comment)?),
                staff_token,
            )
            .await?;
        Ok(envelope.comment)
    }

    /// `GET /orders/{id}/qc`. Null when no run has been started.
    pub async fn qc_run(&self, order_id: &str) -> Result<Option<QcRunDoc>, XbmError> {
        self.get(&format!("/orders/{}/qc", Self::order_path_id(order_id)), &[])
            .await
    }

    /// `POST /orders/{id}/qc`. `items` is merged over what is stored, so a
    /// partial map is safe. Requires a staff token held by `qc.perform`.
    pub async fn merge_qc(
        &self,
        order_id: &str,
        items: serde_json::Map<String, Value>,
        status: Option<&str>,
        notes: Option<&str>,
        staff_token: &str,
        actor_staff_id: &str,
    ) -> Result<QcRunDoc, XbmError> {
        let mut payload = serde_json::json!({
            "items": items,
            "actorStaffId": actor_staff_id,
        });
        if let Some(status) = status {
            payload["status"] = Value::String(status.to_string());
        }
        if let Some(notes) = notes {
            payload["notes"] = Value::String(notes.to_string());
        }
        self.request_as(
            reqwest::Method::POST,
            &format!("/orders/{}/qc", Self::order_path_id(order_id)),
            &[],
            Some(&payload),
            Some(staff_token),
        )
        .await
    }

    /// `POST /staff/authenticate`. Exchanges a floor credential for a staff
    /// token; `require_permission` turns an unauthorised tech away at login
    /// rather than at their first write.
    pub async fn authenticate_staff(
        &self,
        method: StaffAuthMethod<'_>,
        require_permission: Option<&str>,
    ) -> Result<StaffAuth, XbmError> {
        let mut payload = match method {
            StaffAuthMethod::Pin { staff_id, pin } => {
                serde_json::json!({ "method": "pin", "staffId": staff_id, "pin": pin })
            }
            StaffAuthMethod::Qr { token } => serde_json::json!({ "method": "qr", "token": token }),
            StaffAuthMethod::Nfc { nfc_id } => {
                serde_json::json!({ "method": "nfc", "nfcId": nfc_id })
            }
        };
        if let Some(permission) = require_permission {
            payload["requirePermission"] = Value::String(permission.to_string());
        }
        self.post("/staff/authenticate", payload).await
    }


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

    /// `GET /orders/{id}` narrowed to one build; an unknown build is 404 `config_not_found`.
    pub async fn order_detail_for_config(
        &self,
        order_id: &str,
        config: ConfigRef<'_>,
    ) -> Result<BuildDetail, XbmError> {
        self.get(
            &format!("/orders/{}", Self::order_path_id(order_id)),
            &[config.query_pair()],
        )
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

    /// `POST /orders/{id}/release-pool-unit` — unit and Odoo reservation back to the pool (scope: workflow).
    pub async fn release_pool_unit(
        &self,
        order_id: &str,
        request: &ReleasePoolUnitRequest,
    ) -> Result<PoolReleaseResult, XbmError> {
        self.post(
            &format!(
                "/orders/{}/release-pool-unit",
                Self::order_path_id(order_id)
            ),
            json_body(request)?,
        )
        .await
    }

    // ─── Service / repair intake ────────────────────────────────────────────

    /// `GET /orders/{id}/service`.
    pub async fn service_record(&self, order_id: &str) -> Result<ServiceRecord, XbmError> {
        self.get(
            &format!("/orders/{}/service", Self::order_path_id(order_id)),
            &[],
        )
        .await
    }

    /// `PATCH /orders/{id}/service` (scope: write); `qc_signoff` needs a `qc.perform` staff token.
    pub async fn update_service_record(
        &self,
        order_id: &str,
        patch: &ServicePatch,
        staff_token: Option<&str>,
    ) -> Result<ServiceRecord, XbmError> {
        self.request_as(
            reqwest::Method::PATCH,
            &format!("/orders/{}/service", Self::order_path_id(order_id)),
            &[],
            Some(&json_body(patch)?),
            staff_token,
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

    /// `GET /staff?permission=` — holders of `permission`; an unknown key is a 400.
    pub async fn staff_with_permission(
        &self,
        permission: &str,
        active: Option<bool>,
    ) -> Result<StaffPayload, XbmError> {
        let mut query = vec![("permission", permission.to_string())];
        if let Some(active) = active {
            query.push(("active", active.to_string()));
        }
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

    /// `POST /pool-reconcile` — unit statuses from PrestaShop + Odoo; `dry_run: false` writes (scope: write).
    pub async fn pool_reconcile(
        &self,
        dry_run: bool,
        company_id: Option<u32>,
    ) -> Result<Value, XbmError> {
        let mut body = serde_json::json!({ "dryRun": dry_run });
        if let Some(id) = company_id {
            body["companyId"] = Value::from(id);
        }
        self.post("/pool-reconcile", body).await
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

    /// `PATCH /purchase-orders/{id}` — draft header/lines (scope: write).
    pub async fn update_purchase_order(
        &self,
        po_id: &str,
        patch: Value,
    ) -> Result<Value, XbmError> {
        self.patch(&format!("/purchase-orders/{po_id}"), patch)
            .await
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

    // ─── OA3 / bench telemetry ──────────────────────────────────────────────

    /// `GET /oa3/attach` — the scanned machine's Windows line and any key already on it.
    pub async fn oa3_attach_target(
        &self,
        reference: &str,
        config_id: Option<&str>,
    ) -> Result<Oa3AttachTarget, XbmError> {
        let mut query = vec![("ref", reference.to_string())];
        if let Some(id) = config_id {
            query.push(("configId", id.to_string()));
        }
        self.get("/oa3/attach", &query).await
    }

    /// `POST /oa3/attach` — binds the key and reserves its Odoo lot, consuming a licence (scope: workflow).
    pub async fn oa3_attach(
        &self,
        request: &Oa3AttachRequest,
    ) -> Result<Oa3AttachResult, XbmError> {
        self.post("/oa3/attach", json_body(request)?).await
    }

    /// `GET /oa3/injection-log`, newest first.
    pub async fn oa3_injections(
        &self,
        filter: Oa3InjectionQuery<'_>,
    ) -> Result<Oa3InjectionsPayload, XbmError> {
        self.get("/oa3/injection-log", &filter.query()).await
    }

    /// `POST /oa3/injection-log` — records one attempt, no side effects (scope: write).
    pub async fn record_oa3_injection(
        &self,
        report: &Oa3InjectionReport,
    ) -> Result<Oa3InjectionReceipt, XbmError> {
        self.post("/oa3/injection-log", json_body(report)?).await
    }

    /// `POST /oa3/baseboards` (scope: write).
    pub async fn report_baseboard(
        &self,
        sighting: &BaseboardSighting,
    ) -> Result<BaseboardReceipt, XbmError> {
        self.post("/oa3/baseboards", json_body(sighting)?).await
    }

    /// `POST /client-errors` — deduplicated by `(app, fingerprint)` (scope: write).
    pub async fn report_client_error(
        &self,
        report: &ClientErrorReport,
    ) -> Result<ClientErrorReceipt, XbmError> {
        self.post("/client-errors", json_body(report)?).await
    }

    /// `GET /app-versions?name=` — latest published build.
    pub async fn app_version(&self, name: &str) -> Result<AppVersion, XbmError> {
        self.get("/app-versions", &[("name", name.to_string())])
            .await
    }

    // ─── SKUs / labels ──────────────────────────────────────────────────────

    /// `GET /skus/resolve` — exact match only; `None` on 404 `sku_not_found`.
    pub async fn resolve_sku(&self, sku: &str) -> Result<Option<SkuResolution>, XbmError> {
        match self.get("/skus/resolve", &[("sku", sku.to_string())]).await {
            Ok(found) => Ok(Some(found)),
            Err(e) if e.is_not_found() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `GET /builds/cec-label` — `reference` is the scanned build-sheet value, verbatim.
    pub async fn cec_label(
        &self,
        reference: &str,
        mode: CecLabelMode,
    ) -> Result<CecLabel, XbmError> {
        let mut query = vec![("ref", reference.to_string())];
        query.extend(mode.query_pair());
        self.get("/builds/cec-label", &query).await
    }

    // ─── Roles (Value-level v1) ─────────────────────────────────────────────

    /// `GET /roles`; `staff_id` narrows to the roles that staffer holds.
    pub async fn roles(&self, staff_id: Option<&str>) -> Result<Value, XbmError> {
        let query: Vec<(&str, String)> = staff_id
            .map(|id| vec![("staffId", id.to_string())])
            .unwrap_or_default();
        self.get("/roles", &query).await
    }

    /// `GET /roles/{id}` — the role and who holds it.
    pub async fn role(&self, role_id: &str) -> Result<Value, XbmError> {
        self.get(&format!("/roles/{role_id}"), &[]).await
    }

    /// `POST /roles` (scope: write).
    pub async fn create_role(&self, role: &NewRole) -> Result<Value, XbmError> {
        self.post("/roles", json_body(role)?).await
    }

    /// `PATCH /roles/{id}` — `permissions` replaces the whole list (scope: write).
    pub async fn update_role(&self, role_id: &str, patch: Value) -> Result<Value, XbmError> {
        self.patch(&format!("/roles/{role_id}"), patch).await
    }

    /// `DELETE /roles/{id}` — refused while anyone holds it (scope: write).
    pub async fn delete_role(&self, role_id: &str) -> Result<Value, XbmError> {
        self.delete(&format!("/roles/{role_id}")).await
    }

    /// `POST /roles/{id}/staff` — idempotent (scope: write).
    pub async fn assign_role(
        &self,
        role_id: &str,
        staff_id: &str,
        assigned_by: Option<&str>,
    ) -> Result<Value, XbmError> {
        let mut body = serde_json::json!({ "staffId": staff_id });
        if let Some(by) = assigned_by {
            body["assignedBy"] = Value::from(by);
        }
        self.post(&format!("/roles/{role_id}/staff"), body).await
    }

    /// `DELETE /roles/{id}/staff` (scope: write).
    pub async fn unassign_role(&self, role_id: &str, staff_id: &str) -> Result<Value, XbmError> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/roles/{role_id}/staff"),
            &[],
            Some(&serde_json::json!({ "staffId": staff_id })),
        )
        .await
    }

    // ─── Corporate deals / RMA / balance invoices (Value-level v1) ──────────

    /// `GET /corp-deals`; `status` is `draft | orders_created | released | cancelled`.
    pub async fn corp_deals(
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
        self.get("/corp-deals", &query).await
    }

    /// `POST /corp-deals` — writes nothing to Shopify (scope: write).
    pub async fn create_corp_deal(&self, deal: &NewCorpDeal) -> Result<Value, XbmError> {
        self.post("/corp-deals", json_body(deal)?).await
    }

    /// `GET /corp-deals/{id}` — unit lines, per-machine orders, buy list.
    pub async fn corp_deal(&self, deal_id: &str) -> Result<Value, XbmError> {
        self.get(&format!("/corp-deals/{deal_id}"), &[]).await
    }

    /// `POST /corp-deals/{id}`; re-call a chunked action until `result.remaining` is 0.
    pub async fn corp_deal_action(
        &self,
        deal_id: &str,
        action: CorpDealAction,
        limit: Option<u32>,
    ) -> Result<Value, XbmError> {
        let mut body = serde_json::json!({ "action": action });
        if let Some(limit) = limit {
            body["limit"] = Value::from(limit);
        }
        self.post(&format!("/corp-deals/{deal_id}"), body).await
    }

    /// `GET /rma/reports/{report}` — rows plus `shouldReadZero`.
    pub async fn rma_report(&self, report: RmaReport) -> Result<Value, XbmError> {
        self.get(&format!("/rma/reports/{}", report.as_str()), &[])
            .await
    }

    /// `GET /rma/import-odoo` — what an import would bring in; writes nothing.
    pub async fn rma_import_preview(
        &self,
        options: RmaImportOptions<'_>,
    ) -> Result<Value, XbmError> {
        self.get("/rma/import-odoo", &options.query()).await
    }

    /// `POST /rma/import-odoo` — idempotent on `odooRtvId` (scope: workflow).
    pub async fn rma_import(&self, options: RmaImportOptions<'_>) -> Result<Value, XbmError> {
        self.request(
            reqwest::Method::POST,
            "/rma/import-odoo",
            &options.query(),
            None,
        )
        .await
    }

    /// `GET /rma/dwell-sweep` — what would escalate, plus the 60-day receivables; stamps nothing.
    pub async fn rma_dwell_preview(&self) -> Result<Value, XbmError> {
        self.get("/rma/dwell-sweep", &[]).await
    }

    /// `POST /rma/dwell-sweep` — stamps 14/30/60-day escalations, returns who to notify (scope: workflow).
    pub async fn rma_dwell_sweep(&self) -> Result<Value, XbmError> {
        self.request(reqwest::Method::POST, "/rma/dwell-sweep", &[], None)
            .await
    }

    /// `GET /balance-invoices` — collectible balances with a skip reason per order; stamps nothing.
    pub async fn balance_invoices(&self, limit: Option<u32>) -> Result<Value, XbmError> {
        let query: Vec<(&str, String)> = limit
            .map(|l| vec![("limit", l.to_string())])
            .unwrap_or_default();
        self.get("/balance-invoices", &query).await
    }

    /// `GET /balance-invoices?orderId=` — one order's balance-link state; null when unknown.
    pub async fn balance_invoice(&self, order_id: &str) -> Result<Value, XbmError> {
        self.get("/balance-invoices", &[("orderId", order_id.to_string())])
            .await
    }

    /// `POST /balance-invoices` `op=send` — emails the payment link (scope: write); 409 `not_collectible`.
    pub async fn send_balance_invoice(
        &self,
        request: &BalanceSendRequest,
    ) -> Result<Value, XbmError> {
        let mut body = json_body(request)?;
        body["op"] = Value::from("send");
        self.post("/balance-invoices", body).await
    }

    /// `POST /balance-invoices` `op=sweep` — `dry_run: false` emails every eligible customer (scope: write).
    pub async fn sweep_balance_invoices(
        &self,
        request: &BalanceSweepRequest,
    ) -> Result<Value, XbmError> {
        let mut body = json_body(request)?;
        body["op"] = Value::from("sweep");
        self.post("/balance-invoices", body).await
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

#[cfg(test)]
mod wire_tests {
    //! Each wrapper against a one-shot local HTTP stub: method, path, query, headers, body.

    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Captured {
        /// `"GET /path?query"`.
        line: String,
        head: String,
        body: Value,
    }

    impl Captured {
        fn header(&self, name: &str) -> Option<&str> {
            self.head.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case(name).then(|| v.trim())
            })
        }
    }

    struct Stub {
        client: XbmClient,
        request: tokio::task::JoinHandle<Captured>,
    }

    impl Stub {
        async fn answer(status: u16, envelope: Value) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let request = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let captured = read_request(&mut socket).await;
                let body = envelope.to_string();
                let response = format!(
                    "HTTP/1.1 {status} STUB\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                captured
            });
            Self {
                client: XbmClient::new(base, "xbm_test"),
                request,
            }
        }

        async fn ok(data: Value) -> Self {
            Self::answer(200, json!({ "ok": true, "data": data })).await
        }

        async fn captured(self) -> Captured {
            self.request.await.unwrap()
        }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> Captured {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let head_end = loop {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0, "connection closed before the request head");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let head = String::from_utf8(buf[..head_end].to_vec()).unwrap();
        let length = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while buf.len() < head_end + length {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0, "connection closed mid-body");
            buf.extend_from_slice(&chunk[..n]);
        }
        let raw_body = &buf[head_end..head_end + length];
        Captured {
            line: head
                .lines()
                .next()
                .unwrap()
                .trim_end_matches(" HTTP/1.1")
                .to_string(),
            body: if raw_body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(raw_body).unwrap()
            },
            head,
        }
    }

    #[tokio::test]
    async fn api_errors_keep_details() {
        let stub = Stub::answer(
            404,
            json!({ "ok": false, "error": {
                "code": "legacy_order_not_migrated", "message": "PrestaShop only",
                "details": { "legacyOrderId": "2145629" }
            }}),
        )
        .await;
        let err = stub.client.resolve("SN1").await.unwrap_err();
        match err {
            XbmError::Api {
                status,
                code,
                details,
                ..
            } => {
                assert_eq!(status, 404);
                assert_eq!(code, "legacy_order_not_migrated");
                assert_eq!(details.unwrap()["legacyOrderId"], "2145629");
            }
            other => panic!("expected an API error, got {other:?}"),
        }
        stub.captured().await;
    }

    #[tokio::test]
    async fn order_detail_narrows_by_config_id_or_gid() {
        let stub =
            Stub::ok(json!({ "order": { "id": "gid://shopify/Order/1", "name": "#1" } })).await;
        stub.client
            .order_detail_for_config("gid://shopify/Order/1", ConfigRef::Id("53147"))
            .await
            .unwrap();
        assert_eq!(stub.captured().await.line, "GET /orders/1?configId=53147");

        let stub = Stub::ok(json!({})).await;
        stub.client
            .order_detail_for_config("1", ConfigRef::Gid("gid://shopify/Metaobject/5"))
            .await
            .unwrap();
        assert_eq!(
            stub.captured().await.line,
            "GET /orders/1?configGid=gid%3A%2F%2Fshopify%2FMetaobject%2F5"
        );
    }

    #[tokio::test]
    async fn comments_page_and_post_with_staff_token() {
        let stub = Stub::ok(json!({ "comments": [], "nextBefore": null })).await;
        stub.client
            .comments_with(
                "1",
                CommentsQuery {
                    limit: Some(200),
                    before: Some("2026-09-01T00:00:00Z"),
                    visibility: Some(CommentVisibility::Customer),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            stub.captured().await.line,
            "GET /orders/1/comments?limit=200&before=2026-09-01T00%3A00%3A00Z&visibility=customer"
        );

        let stub = Stub::answer(
            201,
            json!({ "ok": true, "data": { "comment": { "id": "c1", "type": "qc" } } }),
        )
        .await;
        let comment = NewComment {
            body: "Passed".into(),
            kind: Some(CommentKind::Qc),
            actor_staff_id: Some("cmq89".into()),
            ..Default::default()
        };
        let created = stub
            .client
            .post_comment_with("1", &comment, Some("st_1"))
            .await
            .unwrap();
        assert_eq!(created.id, "c1");
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /orders/1/comments");
        assert_eq!(req.header("x-staff-token"), Some("st_1"));
        assert_eq!(
            req.body,
            json!({ "body": "Passed", "type": "qc", "actorStaffId": "cmq89" })
        );

        let stub = Stub::answer(
            201,
            json!({ "ok": true, "data": { "comment": { "id": "c2" } } }),
        )
        .await;
        stub.client
            .post_comment("1", "note text", None, None)
            .await
            .unwrap();
        let req = stub.captured().await;
        assert_eq!(req.body, json!({ "body": "note text", "type": "note" }));
        assert_eq!(req.header("x-staff-token"), None);
    }

    #[tokio::test]
    async fn release_pool_unit_returns_refusals_as_data() {
        let stub = Stub::answer(
            422,
            json!({ "ok": true, "data": { "ok": false, "error": "held by another order", "warnings": [] } }),
        )
        .await;
        let result = stub
            .client
            .release_pool_unit(
                "gid://shopify/Order/9",
                &ReleasePoolUnitRequest {
                    disposition: PoolReleaseDisposition::Defective,
                    reason: Some("GPU artifacts".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(!result.ok);
        assert_eq!(result.error.as_deref(), Some("held by another order"));
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /orders/9/release-pool-unit");
        assert_eq!(
            req.body,
            json!({ "disposition": "defective", "reason": "GPU artifacts" })
        );
    }

    #[tokio::test]
    async fn pool_reconcile_sends_the_dry_run_flag() {
        let stub = Stub::ok(json!({ "dryRun": true, "units": 0, "rows": [] })).await;
        stub.client.pool_reconcile(true, Some(1)).await.unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /pool-reconcile");
        assert_eq!(req.body, json!({ "dryRun": true, "companyId": 1 }));
    }

    #[tokio::test]
    async fn service_record_read_and_patch() {
        let stub = Stub::ok(
            json!({ "orderGid": "gid://shopify/Order/5", "details": { "device_name": "Laptop" } }),
        )
        .await;
        let record = stub.client.service_record("5").await.unwrap();
        assert_eq!(record.details.device_name, "Laptop");
        assert_eq!(stub.captured().await.line, "GET /orders/5/service");

        let stub = Stub::ok(
            json!({ "orderGid": "gid://shopify/Order/5", "details": { "qc_signoff": "Jane" } }),
        )
        .await;
        let patch = ServicePatch {
            qc_signoff: Some("Jane".into()),
            device_password: Some(Secret::new("pw")),
            ..Default::default()
        };
        let record = stub
            .client
            .update_service_record("5", &patch, Some("st_2"))
            .await
            .unwrap();
        assert_eq!(record.details.qc_signoff, "Jane");
        let req = stub.captured().await;
        assert_eq!(req.line, "PATCH /orders/5/service");
        assert_eq!(req.header("x-staff-token"), Some("st_2"));
        assert_eq!(
            req.body,
            json!({ "device_password": "pw", "qc_signoff": "Jane" })
        );
    }

    #[tokio::test]
    async fn oa3_attach_lookup_and_bind() {
        let stub = Stub::ok(json!({ "isPoolUnit": false, "name": "#3840", "matchedBy": "order_number", "osLines": [] })).await;
        stub.client
            .oa3_attach_target("#3840", Some("53147"))
            .await
            .unwrap();
        assert_eq!(
            stub.captured().await.line,
            "GET /oa3/attach?ref=%233840&configId=53147"
        );

        let stub =
            Stub::ok(json!({ "ok": true, "name": "#3840", "key": "K", "warnings": [] })).await;
        let request = Oa3AttachRequest {
            reference: "3840-53147".into(),
            key: "K".into(),
            cbr_present: true,
            ..Default::default()
        };
        stub.client.oa3_attach(&request).await.unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /oa3/attach");
        assert_eq!(
            req.body,
            json!({ "ref": "3840-53147", "key": "K", "cbrPresent": true })
        );
    }

    #[tokio::test]
    async fn oa3_injection_ledger_and_report() {
        let stub = Stub::ok(json!({ "injections": [] })).await;
        stub.client
            .oa3_injections(Oa3InjectionQuery {
                tier_agrees: Some(false),
                limit: Some(5),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            stub.captured().await.line,
            "GET /oa3/injection-log?tierAgrees=false&limit=5"
        );

        let stub = Stub::answer(201, json!({ "ok": true, "data": { "id": "i1", "duplicate": false, "pkid": "1", "tierAgrees": true } })).await;
        let report = Oa3InjectionReport {
            client_event_id: "e1".into(),
            pkid: "1".into(),
            ..Default::default()
        };
        let receipt = stub.client.record_oa3_injection(&report).await.unwrap();
        assert!(!receipt.duplicate);
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /oa3/injection-log");
        assert_eq!(req.body, json!({ "clientEventId": "e1", "pkid": "1" }));
    }

    #[tokio::test]
    async fn bench_telemetry_posts() {
        let stub = Stub::ok(json!({ "skipped": true, "reason": "generic_name" })).await;
        let sighting = BaseboardSighting {
            product_wmi: "Standard".into(),
            ..Default::default()
        };
        assert!(
            stub.client
                .report_baseboard(&sighting)
                .await
                .unwrap()
                .skipped
        );
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /oa3/baseboards");
        assert_eq!(req.body, json!({ "productWmi": "Standard" }));

        let stub = Stub::answer(
            201,
            json!({ "ok": true, "data": { "id": "e1", "occurrences": 1, "fingerprint": "f" } }),
        )
        .await;
        let report = ClientErrorReport {
            app: "MasterTech".into(),
            message: Some("boom".into()),
            ..Default::default()
        };
        assert_eq!(
            stub.client
                .report_client_error(&report)
                .await
                .unwrap()
                .occurrences,
            1
        );
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /client-errors");
        assert_eq!(req.body, json!({ "app": "MasterTech", "message": "boom" }));

        let stub = Stub::ok(json!({ "name": "OA3InjectionWrapper", "version": "1.2.3", "sha256": "ab", "retired": false })).await;
        assert_eq!(
            stub.client
                .app_version("OA3InjectionWrapper")
                .await
                .unwrap()
                .version,
            "1.2.3"
        );
        assert_eq!(
            stub.captured().await.line,
            "GET /app-versions?name=OA3InjectionWrapper"
        );
    }

    #[tokio::test]
    async fn resolve_sku_maps_not_found_to_none() {
        let stub = Stub::answer(
            404,
            json!({ "ok": false, "error": { "code": "sku_not_found", "message": "No product variant carries SKU" } }),
        )
        .await;
        assert!(
            stub.client
                .resolve_sku("RAM/32/6000")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            stub.captured().await.line,
            "GET /skus/resolve?sku=RAM%2F32%2F6000"
        );

        let stub =
            Stub::ok(json!({ "sku": "LAP/BB/165070TI", "productTitle": "BIMBOX Slim 16" })).await;
        let found = stub
            .client
            .resolve_sku("LAP/BB/165070TI")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.product_title, "BIMBOX Slim 16");
        stub.captured().await;
    }

    #[tokio::test]
    async fn cec_label_modes_map_to_query_flags() {
        for (mode, line) in [
            (CecLabelMode::Replay, "GET /builds/cec-label?ref=3840-53147"),
            (
                CecLabelMode::Refresh,
                "GET /builds/cec-label?ref=3840-53147&refresh=1",
            ),
            (
                CecLabelMode::DryRun,
                "GET /builds/cec-label?ref=3840-53147&dry=1",
            ),
        ] {
            let stub = Stub::ok(json!({ "label_value": "3840-53147" })).await;
            let label = stub.client.cec_label("3840-53147", mode).await.unwrap();
            assert_eq!(label.label_value, "3840-53147");
            assert_eq!(stub.captured().await.line, line);
        }
    }

    #[tokio::test]
    async fn staff_by_permission_and_role_assignment() {
        let stub = Stub::ok(json!({ "staff": [] })).await;
        stub.client
            .staff_with_permission("qc.perform", Some(true))
            .await
            .unwrap();
        assert_eq!(
            stub.captured().await.line,
            "GET /staff?permission=qc.perform&active=true"
        );

        let stub = Stub::ok(json!({ "removed": true })).await;
        stub.client.unassign_role("r1", "s1").await.unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "DELETE /roles/r1/staff");
        assert_eq!(req.body, json!({ "staffId": "s1" }));

        let stub = Stub::ok(json!({ "assigned": true })).await;
        stub.client.assign_role("r1", "s1", None).await.unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /roles/r1/staff");
        assert_eq!(req.body, json!({ "staffId": "s1" }));
    }

    #[tokio::test]
    async fn back_office_actions() {
        let stub =
            Stub::ok(json!({ "action": "create-orders", "result": { "remaining": 30 } })).await;
        stub.client
            .corp_deal_action("d1", CorpDealAction::CreateOrders, Some(10))
            .await
            .unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /corp-deals/d1");
        assert_eq!(req.body, json!({ "action": "create-orders", "limit": 10 }));

        let stub = Stub::ok(json!({ "ok": true })).await;
        stub.client
            .rma_import(RmaImportOptions {
                include_settled: true,
                max_pages: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = stub.captured().await;
        assert_eq!(
            req.line,
            "POST /rma/import-odoo?includeSettled=1&maxPages=2"
        );
        assert_eq!(req.body, Value::Null);

        let stub = Stub::ok(json!({ "report": "credit-not-debited", "rows": [] })).await;
        stub.client
            .rma_report(RmaReport::CreditNotDebited)
            .await
            .unwrap();
        assert_eq!(
            stub.captured().await.line,
            "GET /rma/reports/credit-not-debited"
        );

        let stub = Stub::ok(json!({ "purchaseOrder": {} })).await;
        stub.client
            .update_purchase_order("po1", json!({ "noteToVendor": "Ship to RIV" }))
            .await
            .unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "PATCH /purchase-orders/po1");
        assert_eq!(req.body, json!({ "noteToVendor": "Ship to RIV" }));
    }

    #[tokio::test]
    async fn balance_invoice_ops() {
        let stub = Stub::ok(json!({ "dryRun": true, "orders": [] })).await;
        stub.client
            .sweep_balance_invoices(&BalanceSweepRequest::default())
            .await
            .unwrap();
        let req = stub.captured().await;
        assert_eq!(req.line, "POST /balance-invoices");
        assert_eq!(req.body, json!({ "op": "sweep", "dryRun": true }));

        let stub = Stub::ok(json!({ "sent": true })).await;
        let send = BalanceSendRequest {
            order_id: "123".into(),
            ..Default::default()
        };
        stub.client.send_balance_invoice(&send).await.unwrap();
        assert_eq!(
            stub.captured().await.body,
            json!({ "op": "send", "orderId": "123" })
        );

        let stub = Stub::ok(Value::Null).await;
        assert!(stub.client.balance_invoice("123").await.unwrap().is_null());
        assert_eq!(
            stub.captured().await.line,
            "GET /balance-invoices?orderId=123"
        );
    }
}
