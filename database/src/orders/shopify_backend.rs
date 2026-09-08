//! Shopify (Xidax) implementation of [`OrderBackend`].
//!
//! The Build Management API (`crate::xbm`) is the primary surface: bench
//! machines hold one `xbm_` key, and side-effectful actions (status advance)
//! run their Odoo/email legs server-side. When no XBM key is configured,
//! reads fall back to the Admin GraphQL API with a minimal-scope token.
//! Metafield layout follows the Data Model doc:
//! `xidax_workflow.current_status` → status metaobject with `legacy_id`,
//! `xidax_order.{configs,installed_serials,build_serial,build_photos}`.

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

use crate::xbm::{AdvanceRequest, BuildDetail, QUEUE_BUCKETS, ResolveResult, StaffAuthMethod, XbmClient};
use crate::{SHOPIFY_ADMIN_TOKEN, SHOPIFY_API_VERSION, SHOPIFY_STORE_URL};

use super::gate::{self, GateDecision};
use super::{
    BackendKind, BuildSpec, ChecklistState, DriveSpec, OrderBackend, OrderComment, OrderKey, OrderKind, PhotoCheck, QcOrder, QcOrderItem, QcReportPayload, SlotPick, StatusInfo, TechIdentity,
};

#[derive(Clone, Default)]
pub struct ShopifyBackend {
    store_url: String,
    token: String,
    api_version: String,
    xbm: Option<XbmClient>,
}

impl std::fmt::Debug for ShopifyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShopifyBackend")
            .field("store_url", &self.store_url)
            .field("api_version", &self.api_version)
            .field("xbm_configured", &self.xbm.is_some())
            .finish_non_exhaustive()
    }
}

const ORDER_QUERY: &str = r#"
query QcOrderLookup($q: String!) {
  orders(first: 1, query: $q) {
    nodes {
      id
      name
      legacyResourceId
      note
      customer { displayName }
      currentTotalPriceSet { shopMoney { amount currencyCode } }
      lineItems(first: 100) {
        pageInfo { hasNextPage }
        nodes {
          id
          name
          sku
          quantity
          originalUnitPriceSet { shopMoney { amount } }
        }
      }
      currentStatus: metafield(namespace: "xidax_workflow", key: "current_status") {
        reference { ... on Metaobject { fields { key value } } }
      }
      orderType: metafield(namespace: "xidax_workflow", key: "order_type") {
        reference { ... on Metaobject { fields { key value } } }
      }
      buildSerial: metafield(namespace: "xidax_order", key: "build_serial") { value }
      buildPhotos: metafield(namespace: "xidax_order", key: "build_photos") { value }
      legacyPs: metafield(namespace: "xidax_legacy", key: "id_order_prestashop") { value }
      configs: metafield(namespace: "xidax_order", key: "configs") {
        references(first: 10) { pageInfo { hasNextPage } nodes { ... on Metaobject { fields { key value } } } }
      }
      installedSerials: metafield(namespace: "xidax_order", key: "installed_serials") {
        references(first: 100) { pageInfo { hasNextPage } nodes { ... on Metaobject { fields { key value } } } }
      }
    }
  }
}
"#;

/// Per-request ceiling; without one a hung connection blocks a bench QC run.
const GRAPHQL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const GRAPHQL_MAX_RETRIES: u32 = 4;
/// Comment page size; the API caps this at 200.
const COMMENT_PAGE_SIZE: u32 = 100;

/// First 300 characters of a response body, for error messages.
fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= 300 {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(300).collect();
    format!("{cut}…")
}

/// `2^attempt` seconds, capped at 16.
fn backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(1u64 << attempt.min(4))
}

/// Delay before retrying a failed HTTP status, or `None` if it is not worth
/// retrying. 429 and 5xx are transient; 4xx otherwise is not.
fn retryable_status_delay(
    status: reqwest::StatusCode,
    retry_after: Option<std::time::Duration>,
    attempt: u32,
) -> Option<std::time::Duration> {
    if attempt >= GRAPHQL_MAX_RETRIES {
        return None;
    }
    let transient = status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
    transient.then(|| retry_after.unwrap_or_else(|| backoff(attempt)))
}

/// True when any GraphQL error carries `extensions.code == "THROTTLED"`.
fn is_throttled(errors: &[Value]) -> bool {
    errors.iter().any(|e| {
        e.pointer("/extensions/code")
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.eq_ignore_ascii_case("THROTTLED"))
    })
}

/// Seconds to wait for the query's cost to be restored, from
/// `extensions.cost.throttleStatus`. Falls back to plain backoff.
fn throttle_delay(response: &Value, attempt: u32) -> std::time::Duration {
    let cost = response.pointer("/extensions/cost");
    let requested = cost
        .and_then(|c| c.get("requestedQueryCost"))
        .and_then(|v| v.as_f64());
    let available = cost
        .and_then(|c| c.pointer("/throttleStatus/currentlyAvailable"))
        .and_then(|v| v.as_f64());
    let restore_rate = cost
        .and_then(|c| c.pointer("/throttleStatus/restoreRate"))
        .and_then(|v| v.as_f64())
        .filter(|r| *r > 0.0);

    match (requested, available, restore_rate) {
        (Some(requested), Some(available), Some(rate)) if requested > available => {
            let seconds = ((requested - available) / rate).clamp(0.5, 16.0);
            std::time::Duration::from_secs_f64(seconds)
        }
        _ => backoff(attempt),
    }
}

/// Flatten the section checklist into the `{itemKey: bool}` map the QC route
/// merges. Unset and N/A items are omitted so a partial run does not record a
/// pass for something nobody checked.
fn checklist_to_items(state: &ChecklistState) -> serde_json::Map<String, Value> {
    let mut items = serde_json::Map::new();
    for section in &state.sections {
        if !section.applicable {
            continue;
        }
        for item in &section.items {
            match item.status.as_str() {
                "Pass" => {
                    items.insert(item.key.clone(), Value::Bool(true));
                }
                "Fail" => {
                    items.insert(item.key.clone(), Value::Bool(false));
                }
                _ => {}
            }
        }
    }
    items
}

/// Repair intake block from `serviceDetails`. Keys are the ported PrestaShop
/// column names (snake_case), not the camelCase the rest of the API uses.
fn service_info_from(details: Option<&Value>) -> Option<super::ServiceInfo> {
    let obj = details?.as_object()?;
    let field = |key: &str| {
        obj.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let info = super::ServiceInfo {
        device_name: field("device_name"),
        device_mfg: field("device_mfg"),
        device_model: field("device_model"),
        device_serial: field("device_serial"),
        physical_damage: field("physical_damage"),
        check_in_notes: field("check_in_notes"),
        intake_notes: field("intake_notes"),
    };
    // An all-blank block is the API's empty shell, not an intake record.
    let empty = [
        &info.device_name,
        &info.device_mfg,
        &info.device_model,
        &info.device_serial,
        &info.physical_damage,
        &info.check_in_notes,
        &info.intake_notes,
    ]
    .iter()
    .all(|v| v.is_empty());
    (!empty).then_some(info)
}

impl ShopifyBackend {
    pub fn from_env() -> Self {
        let xbm = XbmClient::from_env();
        Self {
            store_url: SHOPIFY_STORE_URL.trim_end_matches('/').to_string(),
            token: SHOPIFY_ADMIN_TOKEN.to_string(),
            api_version: SHOPIFY_API_VERSION.to_string(),
            xbm: xbm.configured().then_some(xbm),
        }
    }

    /// Target a specific Shopify store, e.g. `pclaptops` or `37rkv3-nc`. The
    /// Build Management API is multi-store and defaults to Xidax.
    pub fn for_shop(mut self, shop: &str) -> Self {
        self.xbm = self.xbm.map(|c| c.for_shop(shop));
        self
    }

    pub fn configured(&self) -> bool {
        self.xbm.is_some() || self.graphql_configured()
    }

    fn graphql_configured(&self) -> bool {
        !self.store_url.is_empty() && !self.token.is_empty()
    }

    fn ensure_configured(&self) -> anyhow::Result<()> {
        if self.graphql_configured() {
            Ok(())
        } else {
            Err(anyhow!(
                "Shopify backend not configured — set XBM_API_KEY (preferred) or SHOPIFY_STORE_URL + SHOPIFY_ADMIN_TOKEN in .env and rebuild."
            ))
        }
    }

    fn xbm(&self) -> anyhow::Result<&XbmClient> {
        self.xbm.as_ref().ok_or_else(|| {
            anyhow!("Build Management API not configured — set XBM_API_KEY in .env and rebuild.")
        })
    }

    /// Ask the Build Management API what a scanned string refers to. The
    /// server tries every reading (order number, build-sheet pair, reference,
    /// legacy PrestaShop id, component serial), so the input is passed
    /// verbatim and never pre-parsed. `Ok(None)` means no Shopify order
    /// matched, which is the caller's cue to try PrestaShop.
    pub async fn resolve_ref(&self, reference: &str) -> anyhow::Result<Option<ResolveResult>> {
        let xbm = self.xbm()?;
        match xbm.resolve(reference).await {
            Ok(result) => Ok(Some(result)),
            Err(crate::xbm::XbmError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(anyhow!(e)).context("Build Management resolve failed"),
        }
    }

    async fn graphql(&self, query: &str, variables: Value) -> anyhow::Result<Value> {
        self.ensure_configured()?;
        let url = format!(
            "{}/admin/api/{}/graphql.json",
            self.store_url, self.api_version
        );
        let body = json!({ "query": query, "variables": variables });

        let mut attempt = 0u32;
        loop {
            let response = crate::xbm::shared_http()
                .post(&url)
                .header("X-Shopify-Access-Token", &self.token)
                .timeout(GRAPHQL_TIMEOUT)
                .json(&body)
                .send()
                .await
                .context("Shopify GraphQL request failed")?;

            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<f64>().ok())
                .map(std::time::Duration::from_secs_f64);
            let text = response
                .text()
                .await
                .context("Shopify GraphQL response body unreadable")?;

            if !status.is_success() {
                if let Some(delay) = retryable_status_delay(status, retry_after, attempt) {
                    attempt += 1;
                    log::warn!(
                        "Shopify GraphQL {status}, retry {attempt}/{GRAPHQL_MAX_RETRIES} in {:.1}s",
                        delay.as_secs_f64()
                    );
                    crate::sleep_compat(delay).await;
                    continue;
                }
                // Status first: a 401/429 used to surface as "returned non-JSON".
                return Err(anyhow!(
                    "Shopify GraphQL HTTP {status}: {}",
                    snippet(&text)
                ));
            }

            let response: Value = serde_json::from_str(&text).with_context(|| {
                format!("Shopify GraphQL returned non-JSON: {}", snippet(&text))
            })?;

            if let Some(errors) = response.get("errors").and_then(|e| e.as_array()) {
                if !errors.is_empty() {
                    if is_throttled(errors) && attempt < GRAPHQL_MAX_RETRIES {
                        let delay = throttle_delay(&response, attempt);
                        attempt += 1;
                        log::warn!(
                            "Shopify GraphQL THROTTLED, retry {attempt}/{GRAPHQL_MAX_RETRIES} in {:.1}s",
                            delay.as_secs_f64()
                        );
                        crate::sleep_compat(delay).await;
                        continue;
                    }
                    return Err(anyhow!("Shopify GraphQL errors: {errors:?}"));
                }
            }
            return Ok(response);
        }
    }

    fn metaobject_fields(node: &Value) -> std::collections::HashMap<String, String> {
        node.get("fields")
            .and_then(|f| f.as_array())
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|f| {
                        let key = f.get("key")?.as_str()?.to_string();
                        let value = match f.get("value") {
                            Some(Value::String(s)) => s.clone(),
                            Some(Value::Null) | None => String::new(),
                            Some(other) => other.to_string(),
                        };
                        Some((key, value))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn gid_tail(gid: &str) -> &str {
        gid.rsplit('/').next().unwrap_or(gid)
    }

    fn order_number_from_key(key: &OrderKey) -> anyhow::Result<String> {
        match key {
            OrderKey::ShopifyOrderNumber(n) => Ok(n.clone()),
            // `build_serial` is minted as `XBS-<orderNumber>` at orders/create.
            OrderKey::BuildSerial(s) => Ok(s.trim_start_matches("XBS-").to_string()),
            other => Err(anyhow!(
                "ShopifyBackend cannot resolve key {:?}; route it to the PrestaShop backend.",
                other
            )),
        }
    }

    fn parse_order_node(&self, node: &Value, key: &OrderKey) -> QcOrder {
        let status_fields = node
            .get("currentStatus")
            .and_then(|m| m.get("reference"))
            .map(Self::metaobject_fields)
            .unwrap_or_default();
        let legacy_id: i64 = status_fields
            .get("legacy_id")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let status_name = status_fields.get("name").cloned().unwrap_or_default();

        let type_fields = node
            .get("orderType")
            .and_then(|m| m.get("reference"))
            .map(Self::metaobject_fields)
            .unwrap_or_default();
        let kind = match type_fields.get("legacy_id").and_then(|s| s.parse::<i64>().ok()) {
            Some(1) | Some(3) | Some(14) => OrderKind::Sales,
            Some(2) => OrderKind::Service,
            Some(4) | Some(5) | Some(6) | Some(12) => OrderKind::Repair,
            Some(_) => OrderKind::Other,
            None => OrderKind::Sales,
        };

        // Serial metaobjects group by numeric line id; detached ones drop out.
        let mut serials_by_line: std::collections::HashMap<String, Vec<String>> = Default::default();
        if let Some(nodes) = node
            .pointer("/installedSerials/references/nodes")
            .and_then(|n| n.as_array())
        {
            for serial_node in nodes {
                let fields = Self::metaobject_fields(serial_node);
                let disposition = fields.get("disposition").cloned().unwrap_or_default();
                if matches!(disposition.as_str(), "qc_reject" | "rma_bin" | "manual_remove" | "warehouse_stock") {
                    continue;
                }
                let line_id = fields.get("order_line_item_id").cloned().unwrap_or_default();
                if let Some(serial) = fields.get("serial_number") {
                    if !serial.trim().is_empty() {
                        serials_by_line.entry(line_id).or_default().push(serial.clone());
                    }
                }
            }
        }

        let items = node
            .pointer("/lineItems/nodes")
            .and_then(|n| n.as_array())
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|li| {
                        let gid = li.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                        let line_id = Self::gid_tail(gid).to_string();
                        QcOrderItem {
                            row_id: line_id.clone(),
                            product_id: String::new(),
                            name: li.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            reference: li.get("sku").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            quantity: li.get("quantity").and_then(|v| v.as_f64()).unwrap_or(1.0),
                            unit_price: li
                                .pointer("/originalUnitPriceSet/shopMoney/amount")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            serials: serials_by_line.remove(&line_id).unwrap_or_default(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let truncated = [
            ("/lineItems/pageInfo/hasNextPage", "line items"),
            ("/configs/references/pageInfo/hasNextPage", "build configs"),
            ("/installedSerials/references/pageInfo/hasNextPage", "installed serials"),
        ]
        .into_iter()
        .filter(|(ptr, _)| node.pointer(ptr).and_then(|v| v.as_bool()).unwrap_or(false))
        .map(|(_, label)| label.to_string())
        .collect::<Vec<_>>();
        if !truncated.is_empty() {
            log::warn!(
                "Shopify order {} truncated: {}",
                key.display(),
                truncated.join(", ")
            );
        }

        QcOrder {
            backend: Some(BackendKind::Shopify),
            key: Some(key.clone()),
            truncated,
            id: node
                .get("legacyResourceId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            gid: node.get("id").and_then(|v| v.as_str()).map(str::to_string),
            reference: node.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
            customer_name: node
                .pointer("/customer/displayName")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            kind,
            status: StatusInfo {
                legacy_id,
                name: gate::status_display(legacy_id, &status_name),
            },
            items,
            total_paid: node
                .pointer("/currentTotalPriceSet/shopMoney/amount")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            everest_doc: None,
            parent_order_id: node
                .pointer("/legacyPs/value")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            id_customer: None,
            build_serial: node
                .pointer("/buildSerial/value")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            config: None,
            service_info: None,
            note: node
                .get("note")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty()),
            ..Default::default()
        }
    }

    fn slot_matches(slot: &str, needles: &[&str]) -> bool {
        let lower = slot.to_lowercase();
        needles.iter().any(|n| lower.contains(n))
    }

    /// Reverse-lookup the Shopify order a serial is installed on, via the XBM
    /// federated serial endpoint. `None` when unconfigured or not found.
    pub async fn resolve_by_serial(&self, serial: &str) -> anyhow::Result<Option<super::OrderSummary>> {
        let Some(xbm) = self.xbm.as_ref() else { return Ok(None) };
        let history = match xbm.serial_history(serial).await {
            Ok(h) => h,
            Err(e) if e.is_not_found() => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let Some(order) = history.shopify.and_then(|s| s.order) else {
            return Ok(None);
        };
        let name = order.name.unwrap_or_default();
        if name.is_empty() {
            return Ok(None);
        }
        Ok(Some(super::OrderSummary {
            backend: Some(BackendKind::Shopify),
            id: order.gid.rsplit('/').next().unwrap_or("").to_string(),
            gid: Some(order.gid),
            reference: name,
            customer_name: order.customer.unwrap_or_default(),
            build_serial: Some(serial.to_string()),
            ..Default::default()
        }))
    }

    /// `GET /serials/{serial}` — federated history flattened for the bench.
    pub async fn serial_history(&self, serial: &str) -> anyhow::Result<super::SerialHistorySummary> {
        let xbm = self.xbm()?;
        let history = xbm
            .serial_history(serial)
            .await
            .context("Build Management serial lookup failed")?;
        Ok(history.into())
    }

    /// Newest orders sitting in the build-intake statuses (Order Placed /
    /// Ready to Build), capped at `limit`. Reads the XBM build queue; only
    /// available when an `xbm_` key is configured (the Admin token exposes no
    /// queue surface).
    pub async fn recent_orders(&self, limit: usize) -> anyhow::Result<Vec<super::OrderSummary>> {
        let xbm = self.xbm()?;
        let queue = xbm
            .orders(QUEUE_BUCKETS, None, None)
            .await
            .context("Build Management queue fetch failed")?;
        Ok(Self::summaries_from_queue(queue, limit))
    }

    /// Filter the queue to build-intake statuses, newest first, capped.
    fn summaries_from_queue(queue: crate::xbm::QueuePayload, limit: usize) -> Vec<super::OrderSummary> {
        let mut rows: Vec<super::OrderSummary> = queue
            .orders
            .into_iter()
            .filter(|o| {
                o.status
                    .as_ref()
                    .is_some_and(|s| gate::is_build_intake_status(&s.name))
            })
            .map(Self::summary_from_queue_order)
            .collect();
        // Fixed-width ISO-8601 UTC timestamps sort lexicographically.
        rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        rows.truncate(limit);
        rows
    }

    fn summary_from_queue_order(o: crate::xbm::QueueOrder) -> super::OrderSummary {
        let id = Self::gid_tail(&o.id).to_string();
        let status_name = o.status.map(|s| s.name).unwrap_or_default();
        super::OrderSummary {
            backend: Some(BackendKind::Shopify),
            id,
            gid: Some(o.id),
            reference: o.name,
            customer_name: o.customer.name.unwrap_or_default(),
            status: StatusInfo { legacy_id: 0, name: status_name },
            model: o.build_name,
            build_serial: o.build_serial.filter(|s| !s.trim().is_empty()),
            created_at: o.created_at,
            order_type: o.order_type,
            expected_serials: o.expected_serials,
            attached_serials: o.attached_serials,
        }
    }

    /// Queue match by order name (`#N`) or build serial, then full detail.
    async fn find_order_xbm(&self, key: &OrderKey) -> anyhow::Result<QcOrder> {
        let xbm = self.xbm()?;

        // Resolve first: the queue holds only orders in the active build
        // buckets, so a repair on the shelf or anything shipped is not in it.
        // `resolve` reads every form of the reference server-side.
        let gid = match xbm.resolve(key.display()).await {
            Ok(result) => Some(result.order_gid),
            Err(crate::xbm::XbmError::Api { status: 404, .. }) => None,
            Err(e) => return Err(anyhow!(e)).context("Build Management resolve failed"),
        };

        // Queue scan stays as the fallback, and still supplies the build serial
        // the detail payload usually leaves empty.
        let wanted_name = format!("#{}", Self::order_number_from_key(key).unwrap_or_default());
        let wanted_serial = match key {
            OrderKey::BuildSerial(s) => Some(s.to_uppercase()),
            _ => None,
        };
        let queue = xbm.orders(QUEUE_BUCKETS, None, None).await.ok();
        let hit = queue.as_ref().and_then(|q| {
            q.orders.iter().find(|o| {
                o.name.eq_ignore_ascii_case(&wanted_name)
                    || wanted_serial.as_deref().is_some_and(|s| {
                        o.build_serial.as_deref().is_some_and(|b| b.eq_ignore_ascii_case(s))
                    })
            })
        });

        let Some(gid) = gid.or_else(|| hit.map(|h| h.id.clone())) else {
            return Err(anyhow!(
                "No Shopify order matches {} (tried resolve and the build queue).",
                key.display()
            ));
        };

        let detail = xbm
            .order_detail(&gid)
            .await
            .context("Build Management order detail fetch failed")?;
        let mut order = Self::order_from_detail(&detail, key, &gid);
        if order.build_serial.as_deref().unwrap_or("").is_empty() {
            order.build_serial = hit
                .and_then(|h| h.build_serial.clone())
                .filter(|s| !s.trim().is_empty());
        }
        Ok(order)
    }

    /// Map a Build Management detail payload onto the backend-neutral order.
    fn order_from_detail(detail: &BuildDetail, key: &OrderKey, gid: &str) -> QcOrder {
        let order = detail.order.as_ref();
        let status = detail.current_status.as_ref();
        let legacy_id = status.map(|s| s.legacy_id).unwrap_or(0);
        let status_name = status.map(|s| s.name.clone()).unwrap_or_default();

        let kind = match detail.order_type.as_ref().map(|t| t.legacy_id) {
            Some(1) | Some(3) | Some(14) => OrderKind::Sales,
            Some(2) => OrderKind::Service,
            Some(4) | Some(5) | Some(6) | Some(12) => OrderKind::Repair,
            Some(_) => OrderKind::Other,
            None => OrderKind::Sales,
        };

        let items = detail
            .line_items
            .iter()
            .map(|li| QcOrderItem {
                row_id: Self::gid_tail(&li.id).to_string(),
                product_id: li.product_handle.clone().unwrap_or_default(),
                name: li.title.clone(),
                reference: li.sku.clone().unwrap_or_default(),
                quantity: li.qty as f64,
                unit_price: String::new(),
                serials: li
                    .serials
                    .iter()
                    .filter(|s| s.reservation_status != "detached")
                    .map(|s| s.serial.clone())
                    .collect(),
            })
            .collect();

        QcOrder {
            // XBM detail returns the full build in one response.
            truncated: Vec::new(),
            backend: Some(BackendKind::Shopify),
            key: Some(key.clone()),
            id: Self::gid_tail(gid).to_string(),
            gid: Some(gid.to_string()),
            reference: order.map(|o| o.name.clone()).unwrap_or_default(),
            customer_name: order
                .and_then(|o| o.customer.as_ref())
                .and_then(|c| c.name.clone())
                .unwrap_or_default(),
            kind,
            status: StatusInfo {
                legacy_id,
                name: gate::status_display(legacy_id, &status_name),
            },
            items,
            total_paid: String::new(),
            everest_doc: None,
            parent_order_id: None,
            id_customer: None,
            build_serial: detail
                .config
                .as_ref()
                .and_then(|c| c.build_serial.clone())
                .filter(|s| !s.trim().is_empty()),
            config: None,
            service_info: service_info_from(detail.service_details.as_ref()),
            note: order
                .and_then(|o| o.note.clone())
                .filter(|s| !s.trim().is_empty()),
            raw_prestashop: None,
            // XBM config block (object) vs GraphQL metaobject nodes (array);
            // `build_spec` branches on the JSON shape.
            shopify_configs: detail
                .config
                .as_ref()
                .and_then(|c| serde_json::to_value(SpecConfig::from(c)).ok()),
        }
    }
}

/// Route slot picks into the build spec. Selection shapes vary: object keyed
/// by slot, or array of pick objects.
fn apply_selection(spec: &mut BuildSpec, selection: &Value) {
    let picks: Vec<(String, String)> = match selection {
        Value::Object(map) => map
            .iter()
            .map(|(slot, v)| {
                let name = match v {
                    Value::String(s) => s.clone(),
                    Value::Object(o) => o
                        .get("title")
                        .or_else(|| o.get("name"))
                        .or_else(|| o.get("label"))
                        .or_else(|| o.get("product_name"))
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    other => other.to_string(),
                };
                (slot.clone(), name)
            })
            .collect(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| {
                let o = v.as_object()?;
                let slot = o
                    .get("slot")
                    .or_else(|| o.get("category"))
                    .or_else(|| o.get("type"))
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = o
                    .get("title")
                    .or_else(|| o.get("name"))
                    .or_else(|| o.get("label"))
                    .or_else(|| o.get("product_name"))
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string();
                (!name.is_empty()).then_some((slot, name))
            })
            .collect(),
        _ => vec![],
    };

    for (slot, name) in picks {
        if name.trim().is_empty() {
            continue;
        }
        let slot_l = slot.to_lowercase();
        // "cpu-cooling" contains "cpu"; coolers/fans/cases/PSUs are accessories.
        let is_accessory = slot_l.contains("cool") || slot_l.contains("fan");
        let is_cpu = !is_accessory && (slot_l.contains("processor") || slot_l.contains("cpu"));
        if is_cpu && spec.cpu.is_empty() {
            spec.cpu = name;
        } else if ShopifyBackend::slot_matches(&slot, &["gpu", "graphics", "video"]) && spec.gpu.is_empty() {
            spec.gpu = name;
        } else if ShopifyBackend::slot_matches(&slot, &["memory", "ram"]) && spec.ram.is_empty() {
            spec.ram = name;
        } else if !is_accessory
            && ShopifyBackend::slot_matches(&slot, &["storage", "ssd", "hdd", "drive", "nvme", "m.2"])
        {
            let kind = if name.to_lowercase().contains("hdd") { "HDD" } else { "SSD" };
            spec.drives.push(DriveSpec { name, kind: kind.into() });
        } else if ShopifyBackend::slot_matches(&slot, &["motherboard", "mainboard"]) && spec.motherboard.is_none() {
            spec.motherboard = Some(name);
        } else if ShopifyBackend::slot_matches(&slot, &["os", "operating", "windows"]) && spec.os.is_none() {
            spec.os = Some(name);
        } else {
            spec.extra.push(SlotPick { slot, name });
        }
    }
}

/// Subset of the XBM config block that `build_spec` consumes.
#[derive(serde::Serialize, serde::Deserialize)]
struct SpecConfig {
    build_name: String,
    build_template: String,
    selection: Value,
}

impl From<&crate::xbm::DetailConfig> for SpecConfig {
    fn from(c: &crate::xbm::DetailConfig) -> Self {
        Self {
            build_name: c.build_name.clone().unwrap_or_default(),
            build_template: c.build_template.clone().unwrap_or_default(),
            selection: c.selection.clone().unwrap_or(Value::Null),
        }
    }
}

impl OrderBackend for ShopifyBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Shopify
    }

    async fn find_order(&self, key: &OrderKey) -> anyhow::Result<QcOrder> {
        if self.xbm.is_some() {
            return self.find_order_xbm(key).await;
        }

        let number = Self::order_number_from_key(key)?;

        for query_string in [format!("name:#{number}"), format!("name:{number}")] {
            let response = self
                .graphql(ORDER_QUERY, json!({ "q": query_string }))
                .await?;
            if let Some(node) = response
                .pointer("/data/orders/nodes/0")
                .filter(|n| !n.is_null())
            {
                let mut order = self.parse_order_node(node, key);
                // Stash the raw configs JSON for build_spec without refetching.
                order.shopify_configs = node.pointer("/configs/references/nodes").cloned();
                return Ok(order);
            }
        }
        Err(anyhow!("No Shopify order found for #{number}."))
    }

    async fn build_spec(&self, order: &QcOrder) -> anyhow::Result<BuildSpec> {
        if let Some(reason) = order.truncation_reason() {
            return Err(anyhow!(reason));
        }
        let mut spec = BuildSpec::default();
        match order.shopify_configs.as_ref() {
            // XBM detail config block: `{build_name, build_template, selection}`.
            Some(Value::Object(obj)) => {
                spec.model = obj
                    .get("build_name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .or_else(|| obj.get("build_template").and_then(|v| v.as_str()))
                    .unwrap_or_default()
                    .to_string();
                if let Some(selection) = obj.get("selection") {
                    apply_selection(&mut spec, selection);
                }
            }
            // Admin GraphQL config metaobject nodes.
            Some(Value::Array(configs)) => {
                for config_node in configs {
                    let fields = Self::metaobject_fields(config_node);
                    if spec.model.is_empty() {
                        spec.model = fields
                            .get("build_name")
                            .or_else(|| fields.get("build_template"))
                            .cloned()
                            .unwrap_or_default();
                    }
                    let Some(selection_raw) = fields.get("selection") else { continue };
                    let Ok(selection) = serde_json::from_str::<Value>(selection_raw) else { continue };
                    apply_selection(&mut spec, &selection);
                }
            }
            _ => {
                spec.model = order.items.first().map(|i| i.name.clone()).unwrap_or_default();
            }
        }

        if let Some(serial) = order.build_serial.as_ref() {
            spec.device_serial = serial.clone();
        }
        Ok(spec)
    }

    fn status_gate(&self, order: &QcOrder) -> GateDecision {
        gate::evaluate_shopify(order.status.legacy_id, &order.status.name)
    }

    async fn advance_status(&self, order: &QcOrder, to_legacy_id: i64) -> anyhow::Result<()> {
        let xbm = self.xbm()?;
        let order_id = order
            .gid
            .as_deref()
            .or((!order.id.is_empty()).then_some(order.id.as_str()))
            .ok_or_else(|| anyhow!("Order has no Shopify id to advance."))?;

        let statuses = xbm.statuses().await.context("status list fetch failed")?;
        let target = statuses
            .statuses
            .iter()
            .find(|s| s.legacy_id == to_legacy_id)
            .ok_or_else(|| {
                anyhow!("No workflow status carries legacy_id {to_legacy_id} — check /statuses seeding.")
            })?;

        let result = xbm
            .advance_order(
                order_id,
                &AdvanceRequest {
                    to_status_gid: Some(target.gid.clone()),
                    to_status_name: None,
                    note: Some("Bench QC status advance".to_string()),
                    force: None,
                },
            )
            .await
            .context("status advance request failed")?;

        if result.ok {
            Ok(())
        } else if result.precheck_failed == Some(true) {
            Err(anyhow!(
                "Advance to '{}' blocked by prechecks: {}",
                target.name,
                result.error.unwrap_or_else(|| "unmet precondition".into())
            ))
        } else {
            Err(anyhow!(
                "Advance to '{}' rejected: {}",
                target.name,
                result.error.unwrap_or_else(|| "unknown error".into())
            ))
        }
    }

    async fn submit_qc(&self, order: &QcOrder, report: &QcReportPayload) -> anyhow::Result<()> {
        let xbm = self.xbm()?;
        let key = order.gid.as_deref().unwrap_or(order.id.as_str());

        // A QC result is a sign-off, so the route refuses an API key alone.
        let staff_token = report.staff_token.as_deref().ok_or_else(|| {
            anyhow!("QC submission needs a floor credential — sign in with a PIN before submitting.")
        })?;
        let actor = report
            .tech_employee_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("QC submission needs the signing tech's staff id."))?;

        let items = checklist_to_items(&report.checklist);
        let status = match report.verdict.as_str() {
            "passed" => "passed",
            "failed" => "failed",
            _ => "in_progress",
        };
        xbm.merge_qc(
            key,
            items,
            Some(status),
            Some(&report.summary_text()),
            staff_token,
            actor,
        )
        .await
        .context("Build Management QC submission failed")?;
        Ok(())
    }

    async fn authenticate_tech(&self, name_or_email: &str, pin: &str) -> anyhow::Result<TechIdentity> {
        let xbm = self.xbm()?;
        let wanted = name_or_email.trim();
        let roster = xbm
            .staff(Some(true))
            .await
            .context("floor staff roster fetch failed")?;
        let staff = roster
            .staff
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(wanted) || s.id == wanted)
            .ok_or_else(|| {
                anyhow!("No active floor staff named '{wanted}' — enter the name exactly as on the roster.")
            })?;

        // A blank PIN is a roster match only: it names the tech but carries no
        // token, so it cannot sign off QC or author a note.
        if pin.trim().is_empty() {
            return Ok(TechIdentity {
                id_employee: staff.id.clone(),
                name: staff.name.clone(),
                email: String::new(),
                id_profile: None,
                staff_token: None,
                permissions: Vec::new(),
            });
        }

        let auth = xbm
            .authenticate_staff(
                StaffAuthMethod::Pin {
                    staff_id: &staff.id,
                    pin: pin.trim(),
                },
                None,
            )
            .await
            .context("floor credential rejected")?;
        Ok(TechIdentity {
            id_employee: auth.staff_id,
            name: auth.name,
            email: String::new(),
            id_profile: None,
            staff_token: Some(auth.staff_token),
            permissions: auth.permissions,
        })
    }

    async fn fetch_comments(&self, order: &QcOrder) -> anyhow::Result<Vec<OrderComment>> {
        // The order note is not part of the comment stream, so it is kept as a
        // synthetic first entry.
        let mut comments = Vec::new();
        if let Some(note) = order.note.as_ref() {
            comments.push(OrderComment {
                id: format!("note-{}", order.id),
                author: "Order note".into(),
                author_employee_id: None,
                body: note.clone(),
                created_at: String::new(),
                private: false,
            });
        }
        if let Some(xbm) = self.xbm.as_ref() {
            let key = order.gid.as_deref().unwrap_or(order.id.as_str());
            let payload = xbm
                .comments(key, None, Some(COMMENT_PAGE_SIZE))
                .await
                .context("Build Management comment fetch failed")?;
            comments.extend(payload.comments.into_iter().map(|c| OrderComment {
                id: c.id,
                author: c.author,
                author_employee_id: c.author_staff_id,
                body: c.body,
                created_at: c.created_at.unwrap_or_default(),
                private: c.visibility != "customer",
            }));
        }
        Ok(comments)
    }

    async fn post_comment(
        &self,
        order: &QcOrder,
        tech: &TechIdentity,
        body: &str,
    ) -> anyhow::Result<OrderComment> {
        let xbm = self.xbm()?;
        let key = order.gid.as_deref().unwrap_or(order.id.as_str());
        // The API rejects actorStaffId without a matching staff token, so both
        // travel together or neither does.
        let (token, actor) = match tech.staff_token.as_deref() {
            Some(token) => (Some(token), Some(tech.id_employee.as_str())),
            None => (None, None),
        };
        let posted = xbm
            .post_comment(key, body, token, actor)
            .await
            .context("Build Management comment post failed")?;
        Ok(OrderComment {
            id: posted.id,
            author: posted.author,
            author_employee_id: posted.author_staff_id,
            body: posted.body,
            created_at: posted.created_at.unwrap_or_default(),
            private: posted.visibility != "customer",
        })
    }

    async fn check_build_photos(&self, order: &QcOrder) -> anyhow::Result<PhotoCheck> {
        let Some(gid) = order.gid.as_ref() else {
            return Ok(PhotoCheck::default());
        };

        if let Some(xbm) = self.xbm.as_ref() {
            let detail = xbm
                .order_detail(gid)
                .await
                .context("Build Management order detail fetch failed")?;
            let count = detail.build_photos.len();
            return Ok(PhotoCheck { present: count > 0, count });
        }

        let query = r#"
            query QcBuildPhotos($id: ID!) {
              order(id: $id) {
                photos: metafield(namespace: "xidax_order", key: "build_photos") { value }
              }
            }
        "#;
        let response = self.graphql(query, json!({ "id": gid })).await?;
        let count = response
            .pointer("/data/order/photos/value")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<Vec<Value>>(s).ok())
            .map(|v| v.len())
            .unwrap_or(0);
        Ok(PhotoCheck { present: count > 0, count })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xbm::BuildDetail;

    fn sample_detail() -> BuildDetail {
        serde_json::from_value(serde_json::json!({
            "order": {
                "id": "gid://shopify/Order/123",
                "name": "#1020",
                "customer": { "name": "Jane Doe", "email": "j@x.com" },
                "note": "fragile"
            },
            "config": {
                "buildSerial": "XBS-1020",
                "buildName": "Apex X-10",
                "selection": [
                    { "slot": "cpu-cooling", "product_name": "TRYX TURRIS 620 COOLER" },
                    { "slot": "processors", "product_name": "Ryzen 9 9950X" },
                    { "slot": "graphics-cards", "product_name": "RTX 5080" },
                    { "slot": "memory", "product_name": "64GB DDR5-6000" },
                    { "slot": "ssd-m2-nvme", "product_name": "2TB NVMe SSD" },
                    { "slot": "motherboards", "product_name": "MSI MEG X670E" },
                    { "slot": "operating-systems", "product_name": "Windows 11" },
                    { "slot": "power-supplies", "product_name": "1650W TITANIUM PSU" },
                    { "slot": "case", "product_name": "GAMMA DARK" }
                ]
            },
            "lineItems": [{
                "id": "gid://shopify/LineItem/456",
                "title": "Ryzen 9 9950X",
                "qty": 1,
                "slot": "processor",
                "sku": "CPU-9950X",
                "expectedSerials": 1,
                "serials": [
                    {
                        "metaobjectGid": "gid://shopify/Metaobject/77",
                        "serial": "SN-LIVE",
                        "reservationStatus": "reserved"
                    },
                    {
                        "metaobjectGid": "gid://shopify/Metaobject/78",
                        "serial": "SN-GONE",
                        "reservationStatus": "detached"
                    }
                ]
            }],
            "currentStatus": { "gid": "gid://shopify/Metaobject/9", "name": "In QC", "color": "#888", "legacyId": 109 },
            "orderType": { "gid": "gid://shopify/Metaobject/2", "handle": "custom", "legacyId": 1, "name": "Custom" },
            "buildPhotos": [],
            "installedSerials": ["gid://shopify/Metaobject/77"]
        }))
        .unwrap()
    }

    #[test]
    fn auth_and_rate_failures_report_the_status_not_non_json() {
        // 401 is terminal; 429 and 5xx are worth retrying.
        assert!(retryable_status_delay(reqwest::StatusCode::UNAUTHORIZED, None, 0).is_none());
        assert!(retryable_status_delay(reqwest::StatusCode::NOT_FOUND, None, 0).is_none());
        assert!(retryable_status_delay(reqwest::StatusCode::TOO_MANY_REQUESTS, None, 0).is_some());
        assert!(
            retryable_status_delay(reqwest::StatusCode::INTERNAL_SERVER_ERROR, None, 0).is_some()
        );
    }

    #[test]
    fn retry_after_header_wins_over_backoff() {
        let delay = retryable_status_delay(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            Some(std::time::Duration::from_secs(7)),
            0,
        );
        assert_eq!(delay, Some(std::time::Duration::from_secs(7)));
    }

    #[test]
    fn retries_stop_at_the_ceiling() {
        assert!(
            retryable_status_delay(
                reqwest::StatusCode::TOO_MANY_REQUESTS,
                None,
                GRAPHQL_MAX_RETRIES
            )
            .is_none()
        );
        // Backoff is capped so a long retry chain cannot stall a bench run.
        assert_eq!(backoff(99), std::time::Duration::from_secs(16));
    }

    #[test]
    fn throttled_is_detected_from_the_extensions_code() {
        let errors = vec![serde_json::json!({
            "message": "Throttled",
            "extensions": { "code": "THROTTLED" }
        })];
        assert!(is_throttled(&errors));

        let other = vec![serde_json::json!({
            "message": "Field does not exist",
            "extensions": { "code": "undefinedField" }
        })];
        assert!(!is_throttled(&other));
        assert!(!is_throttled(&[]));
    }

    #[test]
    fn throttle_delay_comes_from_the_cost_envelope() {
        // Needs 100, has 20, restores 50/s -> 1.6s.
        let response = serde_json::json!({
            "extensions": { "cost": {
                "requestedQueryCost": 100,
                "throttleStatus": { "maximumAvailable": 1000, "currentlyAvailable": 20, "restoreRate": 50 }
            }}
        });
        let delay = throttle_delay(&response, 0);
        assert!(
            (delay.as_secs_f64() - 1.6).abs() < 0.01,
            "got {delay:?}"
        );

        // No cost block -> plain backoff.
        assert_eq!(throttle_delay(&serde_json::json!({}), 2), backoff(2));
        // restoreRate 0 must not divide by zero.
        let zero_rate = serde_json::json!({
            "extensions": { "cost": {
                "requestedQueryCost": 100,
                "throttleStatus": { "currentlyAvailable": 0, "restoreRate": 0 }
            }}
        });
        assert_eq!(throttle_delay(&zero_rate, 1), backoff(1));
    }

    #[test]
    fn snippet_truncates_long_bodies() {
        assert_eq!(snippet("  short  "), "short");
        let long = "x".repeat(500);
        let cut = snippet(&long);
        assert_eq!(cut.chars().count(), 301);
        assert!(cut.ends_with('…'));
    }

    /// Order lookup must not depend on build-queue membership. The queue is
    /// `status:open` in the active buckets, so a repair on the shelf, a
    /// cancelled order or anything shipped is absent from it — order 3879
    /// became unfindable the moment it was cancelled.
    #[test]
    fn find_order_resolves_before_scanning_the_queue() {
        let src = include_str!("shopify_backend.rs");
        let body = src
            .split("async fn find_order_xbm")
            .nth(1)
            .expect("find_order_xbm present");
        let resolve_at = body.find("xbm.resolve(").expect("resolve is called");
        let queue_at = body.find("xbm.orders(").expect("queue scan is present as a fallback");
        assert!(
            resolve_at < queue_at,
            "resolve must run before the queue scan, or orders outside the active buckets are unfindable"
        );
    }

    #[test]
    fn service_details_parse_from_the_ported_column_names() {
        // Shape captured from live repair order #3879.
        let details = serde_json::json!({
            "device_name": "Laptop",
            "device_mfg": "PC Laptops PCL",
            "device_model": "SM3",
            "device_serial": " 1234 ",
            "device_password": "1234",
            "physical_damage": "",
            "check_in_notes": "test checkin",
            "intake_notes": "",
            "data_transfer_status": false
        });
        let info = service_info_from(Some(&details)).expect("intake block parsed");
        assert_eq!(info.device_name, "Laptop");
        assert_eq!(info.device_mfg, "PC Laptops PCL");
        assert_eq!(info.device_model, "SM3");
        assert_eq!(info.device_serial, "1234", "value should be trimmed");
        assert_eq!(info.check_in_notes, "test checkin");
        assert!(info.intake_notes.is_empty());
    }

    #[test]
    fn the_empty_service_shell_is_not_an_intake_record() {
        // /service answers for every order, service or not; an all-blank block
        // must not read as "this machine was checked in".
        let blank = serde_json::json!({
            "device_name": "", "device_mfg": "", "device_model": "",
            "device_serial": "", "physical_damage": "", "check_in_notes": "",
            "intake_notes": "", "data_transfer_status": false
        });
        assert!(service_info_from(Some(&blank)).is_none());
        assert!(service_info_from(Some(&serde_json::json!({}))).is_none());
        assert!(service_info_from(None).is_none());
        // A non-object is not a record either.
        assert!(service_info_from(Some(&serde_json::json!("nope"))).is_none());
    }

    #[test]
    fn checklist_flattens_to_pass_fail_only() {
        let state = ChecklistState {
            kind: "BuildQC".into(),
            sections: vec![
                super::super::checklist::SectionState {
                    number: 1,
                    title: "Applicable".into(),
                    applicable: true,
                    items: vec![
                        item("passed_item", "Pass"),
                        item("failed_item", "Fail"),
                        item("untouched_item", "Unset"),
                        item("na_item", "NA"),
                    ],
                    ..Default::default()
                },
                super::super::checklist::SectionState {
                    number: 2,
                    title: "Skipped".into(),
                    applicable: false,
                    items: vec![item("in_skipped_section", "Pass")],
                    ..Default::default()
                },
            ],
        };

        let items = checklist_to_items(&state);
        assert_eq!(items.get("passed_item"), Some(&serde_json::Value::Bool(true)));
        assert_eq!(items.get("failed_item"), Some(&serde_json::Value::Bool(false)));
        // An unchecked box must never be reported as a pass.
        assert!(!items.contains_key("untouched_item"));
        assert!(!items.contains_key("na_item"));
        // A non-applicable section contributes nothing.
        assert!(!items.contains_key("in_skipped_section"));
        assert_eq!(items.len(), 2);
    }

    fn item(key: &str, status: &str) -> super::super::checklist::ItemState {
        super::super::checklist::ItemState {
            key: key.into(),
            status: status.into(),
            ..Default::default()
        }
    }

    #[test]
    fn truncated_connections_block_the_spec() {
        let key = OrderKey::ShopifyOrderNumber("1020".into());
        let node = serde_json::json!({
            "id": "gid://shopify/Order/123",
            "legacyResourceId": "123",
            "name": "#1020",
            "lineItems": { "pageInfo": { "hasNextPage": true }, "nodes": [] },
            "installedSerials": {
                "references": { "pageInfo": { "hasNextPage": true }, "nodes": [] }
            }
        });
        let order = ShopifyBackend::from_env().parse_order_node(&node, &key);
        assert_eq!(order.truncated, vec!["line items", "installed serials"]);
        let reason = order.truncation_reason().expect("truncation reported");
        assert!(reason.contains("do not QC"), "{reason}");
    }

    #[test]
    fn complete_connections_leave_the_order_untruncated() {
        let key = OrderKey::ShopifyOrderNumber("1020".into());
        let node = serde_json::json!({
            "id": "gid://shopify/Order/123",
            "legacyResourceId": "123",
            "name": "#1020",
            "lineItems": { "pageInfo": { "hasNextPage": false }, "nodes": [] }
        });
        let order = ShopifyBackend::from_env().parse_order_node(&node, &key);
        assert!(order.truncated.is_empty());
        assert!(order.truncation_reason().is_none());
    }

    #[test]
    fn order_from_detail_maps_core_fields() {
        let key = OrderKey::ShopifyOrderNumber("1020".into());
        let order =
            ShopifyBackend::order_from_detail(&sample_detail(), &key, "gid://shopify/Order/123");
        assert_eq!(order.id, "123");
        assert_eq!(order.gid.as_deref(), Some("gid://shopify/Order/123"));
        assert_eq!(order.reference, "#1020");
        assert_eq!(order.customer_name, "Jane Doe");
        assert_eq!(order.status.legacy_id, 109);
        assert_eq!(order.status.name, "In QC");
        assert_eq!(order.kind, OrderKind::Sales);
        assert_eq!(order.build_serial.as_deref(), Some("XBS-1020"));
        assert_eq!(order.note.as_deref(), Some("fragile"));
        assert_eq!(order.items.len(), 1);
        assert_eq!(order.items[0].row_id, "456");
        // Detached serials drop out of the QC view.
        assert_eq!(order.items[0].serials, vec!["SN-LIVE".to_string()]);
    }

    #[test]
    fn recent_orders_filters_to_build_intake_newest_first() {
        let env: crate::xbm::Envelope =
            serde_json::from_str(include_str!("../xbm/fixtures/orders-qc.json")).unwrap();
        let queue: crate::xbm::QueuePayload = serde_json::from_value(env.data.unwrap()).unwrap();
        let rows = ShopifyBackend::summaries_from_queue(queue, 10);
        assert_eq!(rows.len(), 10);
        // Only Order Placed / Ready to Build survive the filter.
        assert!(rows
            .iter()
            .all(|r| r.status.name == "Order Placed" || r.status.name.starts_with("Ready to Build")));
        // Shipped + Pre-Pulled rows are dropped.
        assert!(rows.iter().all(|r| r.status.name != "Shipped" && r.status.name != "Pre-Pulled"));
        // Newest first.
        for w in rows.windows(2) {
            assert!(w[0].created_at >= w[1].created_at);
        }
        assert_eq!(rows[0].reference, "#1032");
        // Both target statuses are represented (a Ready to Build row makes the cut).
        assert!(rows.iter().any(|r| r.status.name.starts_with("Ready to Build")));
        // Round-trips back to a Shopify lookup key.
        assert_eq!(rows[0].lookup_input(), "#1032");
    }

    #[tokio::test]
    async fn build_spec_parses_xbm_selection() {
        let key = OrderKey::ShopifyOrderNumber("1020".into());
        let order =
            ShopifyBackend::order_from_detail(&sample_detail(), &key, "gid://shopify/Order/123");
        let backend = ShopifyBackend::default();
        let spec = backend.build_spec(&order).await.unwrap();
        assert_eq!(spec.model, "Apex X-10");
        // cpu-cooling precedes processors yet must NOT capture the CPU field.
        assert_eq!(spec.cpu, "Ryzen 9 9950X");
        assert_eq!(spec.gpu, "RTX 5080");
        assert_eq!(spec.ram, "64GB DDR5-6000");
        assert_eq!(spec.drives.len(), 1);
        assert_eq!(spec.drives[0].kind, "SSD");
        assert_eq!(spec.motherboard.as_deref(), Some("MSI MEG X670E"));
        assert_eq!(spec.os.as_deref(), Some("Windows 11"));
        // cpu-cooling, power-supplies, case land in extras.
        let extra_slots: Vec<&str> = spec.extra.iter().map(|e| e.slot.as_str()).collect();
        assert!(extra_slots.contains(&"cpu-cooling"), "got {extra_slots:?}");
        assert!(extra_slots.contains(&"power-supplies"));
        assert!(extra_slots.contains(&"case"));
        assert_eq!(spec.device_serial, "XBS-1020");
    }
}
