//! Shopify (Xidax) implementation of [`OrderBackend`].
//!
//! Reads go direct to the Admin GraphQL API with a minimal-scope token.
//! Writes (status advance, QC submission, comments) are Worker territory —
//! the `/qc/*` routes from the master plan — so they return explicit errors
//! until those routes exist. Metafield layout follows the Data Model doc:
//! `xidax_workflow.current_status` → status metaobject with `legacy_id`,
//! `xidax_order.{configs,installed_serials,build_serial,build_photos}`.

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

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
}

impl std::fmt::Debug for ShopifyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShopifyBackend")
            .field("store_url", &self.store_url)
            .field("api_version", &self.api_version)
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
        Self {
            store_url: SHOPIFY_STORE_URL.trim_end_matches('/').to_string(),
            token: SHOPIFY_ADMIN_TOKEN.to_string(),
            api_version: SHOPIFY_API_VERSION.to_string(),
        }
    }

    pub fn configured(&self) -> bool {
        !self.store_url.is_empty() && !self.token.is_empty()
    }

    fn ensure_configured(&self) -> anyhow::Result<()> {
        if self.configured() {
            Ok(())
        } else {
            Err(anyhow!(
                "Shopify backend not configured — set SHOPIFY_STORE_URL and SHOPIFY_ADMIN_TOKEN in .env and rebuild."
            ))
        }
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
}

impl OrderBackend for ShopifyBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Shopify
    }

    async fn find_order(&self, key: &OrderKey) -> anyhow::Result<QcOrder> {
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
        let Some(configs) = order.shopify_configs.as_ref().and_then(|c| c.as_array()) else {
            return Ok(BuildSpec {
                model: order.items.first().map(|i| i.name.clone()).unwrap_or_default(),
                ..Default::default()
            });
        };

        let mut spec = BuildSpec::default();
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

            // Selection shapes vary: object keyed by slot, or array of picks.
            let picks: Vec<(String, String)> = match &selection {
                Value::Object(map) => map
                    .iter()
                    .map(|(slot, v)| {
                        let name = match v {
                            Value::String(s) => s.clone(),
                            Value::Object(o) => o
                                .get("title")
                                .or_else(|| o.get("name"))
                                .or_else(|| o.get("label"))
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
                if Self::slot_matches(&slot, &["processor", "cpu"]) && spec.cpu.is_empty() {
                    spec.cpu = name;
                } else if Self::slot_matches(&slot, &["gpu", "graphics", "video"]) && spec.gpu.is_empty() {
                    spec.gpu = name;
                } else if Self::slot_matches(&slot, &["memory", "ram"]) && spec.ram.is_empty() {
                    spec.ram = name;
                } else if Self::slot_matches(&slot, &["storage", "ssd", "hdd", "drive", "nvme", "m.2"]) {
                    let kind = if name.to_lowercase().contains("hdd") { "HDD" } else { "SSD" };
                    spec.drives.push(DriveSpec { name, kind: kind.into() });
                } else if Self::slot_matches(&slot, &["motherboard", "mainboard"]) && spec.motherboard.is_none() {
                    spec.motherboard = Some(name);
                } else if Self::slot_matches(&slot, &["os", "operating", "windows"]) && spec.os.is_none() {
                    spec.os = Some(name);
                } else {
                    spec.extra.push(SlotPick { slot, name });
                }
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

    async fn advance_status(&self, _order: &QcOrder, to_legacy_id: i64) -> anyhow::Result<()> {
        Err(anyhow!(
            "Shopify status advance ({to_legacy_id}) goes through the Worker POST /qc/advance route — not yet deployed (W7)."
        ))
    }

    async fn submit_qc(&self, _order: &QcOrder, _report: &QcReportPayload) -> anyhow::Result<()> {
        Err(anyhow!(
            "Shopify QC submission writes xidax_qc.bench through the Worker POST /qc/report route — not yet deployed (W7)."
        ))
    }

    async fn authenticate_tech(&self, _email: &str, _password: &str) -> anyhow::Result<TechIdentity> {
        Err(anyhow!(
            "Xidax bench identity is undecided (floor-staff PIN list vs PS employees) — open question #4 in the master plan."
        ))
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
            "Posting Xidax order comments goes through the Worker — not yet deployed (W7)."
        ))
    }

    async fn check_build_photos(&self, order: &QcOrder) -> anyhow::Result<PhotoCheck> {
        let Some(gid) = order.gid.as_ref() else {
            return Ok(PhotoCheck::default());
        };
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
