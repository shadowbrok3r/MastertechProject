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

use crate::xbm::{AdvanceRequest, BuildDetail, XbmClient, QUEUE_BUCKETS};
use crate::{SHOPIFY_ADMIN_TOKEN, SHOPIFY_API_VERSION, SHOPIFY_STORE_URL};

use super::gate::{self, GateDecision};
use super::{
    BackendKind, BuildSpec, DriveSpec, OrderBackend, OrderComment, OrderKey, OrderKind,
    PhotoCheck, QcOrder, QcOrderItem, QcReportPayload, SlotPick, StatusInfo, TechIdentity,
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
        references(first: 10) { nodes { ... on Metaobject { fields { key value } } } }
      }
      installedSerials: metafield(namespace: "xidax_order", key: "installed_serials") {
        references(first: 100) { nodes { ... on Metaobject { fields { key value } } } }
      }
    }
  }
}
"#;

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

    async fn graphql(&self, query: &str, variables: Value) -> anyhow::Result<Value> {
        self.ensure_configured()?;
        let url = format!(
            "{}/admin/api/{}/graphql.json",
            self.store_url, self.api_version
        );
        let response: Value = reqwest::Client::new()
            .post(&url)
            .header("X-Shopify-Access-Token", &self.token)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .context("Shopify GraphQL request failed")?
            .json()
            .await
            .context("Shopify GraphQL returned non-JSON")?;

        if let Some(errors) = response.get("errors").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                return Err(anyhow!("Shopify GraphQL errors: {errors:?}"));
            }
        }
        Ok(response)
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

        QcOrder {
            backend: Some(BackendKind::Shopify),
            key: Some(key.clone()),
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

    /// `GET /serials/{serial}` — federated history flattened for the bench.
    pub async fn serial_history(&self, serial: &str) -> anyhow::Result<super::SerialHistorySummary> {
        let xbm = self.xbm()?;
        let history = xbm
            .serial_history(serial)
            .await
            .context("Build Management serial lookup failed")?;
        Ok(history.into())
    }

    /// Queue match by order name (`#N`) or build serial, then full detail.
    async fn find_order_xbm(&self, key: &OrderKey) -> anyhow::Result<QcOrder> {
        let xbm = self.xbm()?;
        let wanted_name = format!("#{}", Self::order_number_from_key(key)?);
        let wanted_serial = match key {
            OrderKey::BuildSerial(s) => Some(s.to_uppercase()),
            _ => None,
        };

        let queue = xbm
            .orders(QUEUE_BUCKETS, None, None)
            .await
            .context("Build Management queue fetch failed")?;
        let hit = queue.orders.iter().find(|o| {
            o.name.eq_ignore_ascii_case(&wanted_name)
                || wanted_serial.as_deref().is_some_and(|s| {
                    o.build_serial.as_deref().is_some_and(|b| b.eq_ignore_ascii_case(s))
                })
        });
        let Some(hit) = hit else {
            return Err(anyhow!(
                "No build-queue order matches {} (searched all workflow buckets).",
                key.display()
            ));
        };

        let detail = xbm
            .order_detail(&hit.id)
            .await
            .context("Build Management order detail fetch failed")?;
        let mut order = Self::order_from_detail(&detail, key, &hit.id);
        // Build detail's config.buildSerial is often empty; the queue payload
        // carries it.
        if order.build_serial.as_deref().unwrap_or("").is_empty() {
            order.build_serial = hit.build_serial.clone().filter(|s| !s.trim().is_empty());
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
            service_info: None,
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

    async fn submit_qc(&self, _order: &QcOrder, _report: &QcReportPayload) -> anyhow::Result<()> {
        Err(anyhow!(
            "The Build Management API has no QC report endpoint yet — xidax_qc.bench submission needs a /qc route on build-mgmt (the report stays in SurrealDB meanwhile)."
        ))
    }

    async fn authenticate_tech(&self, name_or_email: &str, _pin: &str) -> anyhow::Result<TechIdentity> {
        // Roster match only; the API exposes no PIN verification endpoint yet.
        let xbm = self.xbm()?;
        let roster = xbm
            .staff(Some(true))
            .await
            .context("floor staff roster fetch failed")?;
        let wanted = name_or_email.trim();
        let staff = roster
            .staff
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(wanted) || s.id == wanted)
            .ok_or_else(|| {
                anyhow!("No active floor staff named '{wanted}' — enter the name exactly as on the roster.")
            })?;
        Ok(TechIdentity {
            id_employee: staff.id.clone(),
            name: staff.name.clone(),
            email: String::new(),
        })
    }

    async fn fetch_comments(&self, order: &QcOrder) -> anyhow::Result<Vec<OrderComment>> {
        // Comments mapping is an open question; the order note is the only
        // read surfaced for now. xidax_status_history rendering lands with W7.
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
        Ok(comments)
    }

    async fn post_comment(&self, _order: &QcOrder, _tech: &TechIdentity, _body: &str) -> anyhow::Result<OrderComment> {
        Err(anyhow!(
            "The Build Management API has no order-comment endpoint yet — bench notes ride along on status advances for now."
        ))
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
