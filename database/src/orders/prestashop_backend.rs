//! PrestaShop implementation of [`OrderBackend`] — wraps the existing
//! `schema::prestashop` client. Ported from QCWizard's `OrderBuilder` /
//! `Employee.Authenticate` / `OrderImage` / customer-thread handling.

use std::collections::HashMap;

use anyhow::{anyhow, Context};
use serde_json::Value;

use crate::schema::everest::request_everest_header_by_docnum;
use crate::schema::prestashop::xml::{modify_xml, remove_xml_tag};
use crate::schema::prestashop::{
    CustomerMessage, CustomerThread, Employee, Order, OrderSerial, Prestashop,
};
use crate::{PRESTASHOP_API_URL_WASM, PRESTASHOP_AUTH_URL};

use super::gate::{self, GateDecision};
use super::{
    BackendKind, BuildSpec, DriveSpec, OrderBackend, OrderComment, OrderConfigInfo, OrderKey,
    OrderKind, OrderSummary, PhotoCheck, QcOrder, QcOrderItem, QcReportPayload, ServiceInfo,
    SlotPick, StatusInfo, TechIdentity,
};

/// Percent-encodes a value for an `application/x-www-form-urlencoded` body.
fn form_urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct PrestashopBackend;

impl PrestashopBackend {
    pub fn new() -> Self {
        Self
    }

    /// Reverse-lookup the PS order a serial is installed on
    /// (`order_serial?filter[serial_number]=…` → `id_order`).
    pub async fn resolve_by_serial(&self, serial: &str) -> anyhow::Result<Option<OrderSummary>> {
        if PRESTASHOP_API_URL_WASM.is_empty() {
            return Ok(None);
        }
        let api = Prestashop::default();
        let filter = format!("[{serial}]");
        let mut params = HashMap::new();
        params.insert("filter[serial_number]", filter.as_str());
        params.insert("output_format", "JSON");
        let serials: Vec<OrderSerial> = api
            .request_resources_wasm("order_serials", params)
            .await
            .unwrap_or_default();
        let id_order = serials
            .iter()
            .map(|s| s.id_order.clone())
            .find(|id| !id.is_empty() && id != "0");
        let Some(id_order) = id_order else { return Ok(None) };
        Ok(Some(OrderSummary {
            backend: Some(BackendKind::Prestashop),
            id: id_order.clone(),
            reference: id_order,
            build_serial: Some(serial.to_string()),
            ..Default::default()
        }))
    }

    async fn fetch_order(&self, id: &str) -> anyhow::Result<Order> {
        let api = Prestashop::default();
        api.request_subresources_by_id_wasm::<Order>("orders", "order", id)
            .await
            .with_context(|| format!("PrestaShop order {id} fetch failed"))
    }

    async fn fetch_customer_name(&self, id_customer: &str) -> Option<String> {
        if id_customer.is_empty() || id_customer == "0" {
            return None;
        }
        let api = Prestashop::default();
        let customer: crate::schema::prestashop::Customer = api
            .request_subresources_by_id_wasm("customers", "customer", id_customer)
            .await
            .ok()?;
        let name = format!("{} {}", customer.firstname, customer.lastname);
        let name = name.trim().to_string();
        (!name.is_empty()).then_some(name)
    }

    /// Raw resource fetch probing both singular and plural collection keys,
    /// for custom PS resources whose JSON key isn't guaranteed.
    async fn fetch_resource_rows(&self, resource: &str, id_order: &str) -> anyhow::Result<Vec<Value>> {
        let url = format!(
            "{PRESTASHOP_API_URL_WASM}/{resource}?output_format=JSON&display=full&filter[id_order]={id_order}"
        );
        let response: Value = crate::xbm::shared_http()
            .get(&url)
            .send()
            .await?
            .json()
            .await
            .with_context(|| format!("{resource} fetch for order {id_order} returned non-JSON"))?;

        for key in [resource.to_string(), format!("{resource}s")] {
            if let Some(arr) = response.get(&key).and_then(|v| v.as_array()) {
                return Ok(arr.clone());
            }
        }
        // An empty result set serializes as `[]` rather than a keyed object.
        if response.as_array().is_some_and(|a| a.is_empty()) {
            return Ok(vec![]);
        }
        Ok(vec![])
    }

    async fn fetch_order_config(&self, id_order: &str) -> Option<OrderConfigInfo> {
        let rows = self.fetch_resource_rows("order_config", id_order).await.ok()?;
        let row = rows.first()?;
        let field = |k: &str| -> String {
            match row.get(k) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            }
        };
        let id = if !field("id").is_empty() { field("id") } else { field("id_order_config") };
        if id.is_empty() {
            return None;
        }
        Some(OrderConfigInfo {
            id,
            name: field("name"),
            id_config: field("id_config"),
            builder_employee: Some(field("id_employee_builder")).filter(|s| !s.is_empty() && s != "0"),
            qc_employee: Some(field("id_employee_builder_qc")).filter(|s| !s.is_empty() && s != "0"),
            state_legacy_id: field("id_order_state").parse::<i64>().ok(),
        })
    }

    async fn employee_directory(&self) -> HashMap<String, Employee> {
        let api = Prestashop::default();
        let mut params = HashMap::new();
        params.insert("output_format", "JSON");
        let employees: Vec<Employee> = api
            .request_resources_wasm("employees", params)
            .await
            .unwrap_or_default();
        employees
            .into_iter()
            .filter(|e| !e.id.is_empty())
            .map(|e| (e.id.clone(), e))
            .collect()
    }

    fn items_from_order(order: &Order) -> Vec<QcOrderItem> {
        order
            .associations
            .order_rows
            .iter()
            .filter(|row| !row.id.is_empty())
            .map(|row| {
                let serials: Vec<String> = order
                    .associations
                    .order_serial
                    .iter()
                    .filter(|s| {
                        (!s.id_order_detail.is_empty() && s.id_order_detail == row.id)
                            || (!s.product_reference.is_empty()
                                && s.product_reference.eq_ignore_ascii_case(&row.product_reference))
                    })
                    .map(|s| s.serial_number.clone())
                    .filter(|s| !s.trim().is_empty())
                    .collect();
                QcOrderItem {
                    row_id: row.id.clone(),
                    product_id: row.product_id.clone(),
                    name: row.product_name.clone(),
                    reference: row.product_reference.clone(),
                    quantity: row.product_quantity.parse().unwrap_or(1.0),
                    unit_price: row.product_price.clone(),
                    serials,
                }
            })
            .collect()
    }
}

impl OrderBackend for PrestashopBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Prestashop
    }

    async fn find_order(&self, key: &OrderKey) -> anyhow::Result<QcOrder> {
        match key {
            OrderKey::Prestashop(id) => {
                let order = self.fetch_order(id).await?;
                if order.id.is_empty() {
                    return Err(anyhow!("Could not find order {id} in PrestaShop."));
                }

                let legacy_id: i64 = order.current_state.parse().unwrap_or(0);
                let kind = if order.id_order_type == "4" { OrderKind::Repair } else { OrderKind::Sales };
                let customer_name = self
                    .fetch_customer_name(&order.id_customer)
                    .await
                    .unwrap_or_default();
                let config = self.fetch_order_config(id).await;

                let service_info = order.associations.order_service.first().map(|s| ServiceInfo {
                    device_name: s.device_name.clone(),
                    device_mfg: s.device_mfg.clone(),
                    device_model: s.device_model.clone(),
                    device_serial: s.device_serial.clone(),
                    physical_damage: s.physical_damage.clone(),
                    check_in_notes: s.check_in_notes.clone(),
                    intake_notes: s.intake_notes.clone(),
                });

                Ok(QcOrder {
                    backend: Some(BackendKind::Prestashop),
                    key: Some(key.clone()),
                    id: order.id.clone(),
                    gid: None,
                    reference: order.reference.clone(),
                    customer_name,
                    kind,
                    status: StatusInfo {
                        legacy_id,
                        name: gate::status_display(legacy_id, ""),
                    },
                    items: Self::items_from_order(&order),
                    total_paid: order.total_paid.clone(),
                    everest_doc: Some(order.id_order_everest.clone()).filter(|s| !s.is_empty() && s != "0"),
                    parent_order_id: Some(order.id_order_parent.clone()).filter(|s| !s.is_empty() && s != "0"),
                    id_customer: Some(order.id_customer.clone()).filter(|s| !s.is_empty()),
                    build_serial: None,
                    config,
                    service_info,
                    note: None,
                    raw_prestashop: Some(order),
                    shopify_configs: None,
                })
            }
            OrderKey::Everest(docnum) => {
                let (customer_name, doc) = request_everest_header_by_docnum(docnum).await?;
                Ok(QcOrder {
                    backend: Some(BackendKind::Prestashop),
                    key: Some(key.clone()),
                    id: doc.clone(),
                    reference: doc.clone(),
                    customer_name,
                    kind: OrderKind::Sales,
                    status: StatusInfo { legacy_id: 0, name: "Everest document (no PS status)".into() },
                    everest_doc: Some(doc),
                    note: Some("Everest line items not federated yet — verify parts in Everest.".into()),
                    ..Default::default()
                })
            }
            other => Err(anyhow!(
                "PrestashopBackend cannot resolve key {:?}; route it to the Shopify backend.",
                other
            )),
        }
    }

    async fn build_spec(&self, order: &QcOrder) -> anyhow::Result<BuildSpec> {
        let Some(ps_order) = order.raw_prestashop.as_ref() else {
            return Ok(BuildSpec::default());
        };
        let extracted = ps_order.extract_specs().await;
        let drives = ps_order
            .extract_drives()
            .into_iter()
            .map(|(name, kind)| DriveSpec { name, kind })
            .collect();

        let mut extra = Vec::new();
        if let Some(config) = order.config.as_ref() {
            if !config.name.is_empty() {
                extra.push(SlotPick { slot: "Config".into(), name: config.name.clone() });
            }
        }

        Ok(BuildSpec {
            model: ps_order.extract_model(),
            cpu: extracted.cpu,
            gpu: extracted.gpu,
            ram: extracted.ram,
            motherboard: ps_order.extract_motherboard(),
            os: ps_order.extract_os(),
            drives,
            extra,
            device_serial: extracted.device_serial,
            device_mfg: extracted.device_mfg,
        })
    }

    fn status_gate(&self, order: &QcOrder) -> GateDecision {
        gate::evaluate_prestashop(order.kind, order.status.legacy_id, &order.status.name)
    }

    async fn advance_status(&self, order: &QcOrder, to_legacy_id: i64) -> anyhow::Result<()> {
        gate::update_allowed(order.status.legacy_id, to_legacy_id).map_err(|e| anyhow!(e))?;

        let api = Prestashop::default();
        let xml = api
            .request_raw_resource_by_id("orders", &order.id)
            .await
            .with_context(|| format!("fetching order {} XML for status update", order.id))?;
        let updated = modify_xml(&xml, "current_state", &to_legacy_id.to_string())?;
        let final_xml = remove_xml_tag(&updated, "tax_exempt")?;
        let response = api.modify_prestashop_order(&final_xml).await?;
        if response.contains("<errors>") {
            return Err(anyhow!("PrestaShop rejected the status update: {response}"));
        }
        log::info!(
            "orders/prestashop -> order {} status {} -> {}",
            order.id,
            order.status.legacy_id,
            to_legacy_id
        );
        Ok(())
    }

    async fn submit_qc(&self, order: &QcOrder, report: &QcReportPayload) -> anyhow::Result<()> {
        // PS-visible artifact is a private order comment; SurrealDB holds the
        // structured record (MySQL qc_wizard stays decommissioned).
        let id_employee = report
            .tech_employee_id
            .clone()
            .ok_or_else(|| anyhow!("QC report has no authenticated tech — sign in before submitting."))?;
        let tech = TechIdentity {
            id_employee,
            name: report.tech.clone().unwrap_or_default(),
            email: String::new(),
            id_profile: None,
        };
        self.post_comment(order, &tech, &report.summary_text()).await?;
        Ok(())
    }

    async fn authenticate_tech(&self, email: &str, password: &str) -> anyhow::Result<TechIdentity> {
        if PRESTASHOP_AUTH_URL.is_empty() {
            return Err(anyhow!(
                "PRESTASHOP_AUTH_URL is not configured — add it to .env and rebuild."
            ));
        }

        let body = format!(
            "email={}&password={}",
            form_urlencode(email),
            form_urlencode(password)
        );
        let response = crate::xbm::shared_http()
            .post(PRESTASHOP_AUTH_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .context("employee authentication request failed")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("employee authentication HTTP {status}"));
        }
        // QCWizard parity: an error page contains "error" or inline "style".
        if body.contains("error") || body.contains("style") {
            return Err(anyhow!("Invalid credentials."));
        }

        let api = Prestashop::default();
        let mut params = HashMap::new();
        let filter = format!("[{email}]");
        params.insert("filter[email]", filter.as_str());
        params.insert("output_format", "JSON");
        let employees: Vec<Employee> = api
            .request_resources_wasm("employees", params)
            .await
            .unwrap_or_default();
        let employee = employees
            .into_iter()
            .find(|e| !e.id.is_empty() && e.email.eq_ignore_ascii_case(email))
            .ok_or_else(|| anyhow!("Authenticated, but no PrestaShop employee record matches {email}."))?;

        Ok(TechIdentity {
            id_employee: employee.id,
            name: format!("{} {}", employee.firstname, employee.lastname).trim().to_string(),
            email: employee.email,
            id_profile: Some(employee.id_profile).filter(|p| !p.is_empty() && p != "0"),
        })
    }

    async fn fetch_comments(&self, order: &QcOrder) -> anyhow::Result<Vec<OrderComment>> {
        let api = Prestashop::default();
        let mut params = HashMap::new();
        params.insert("filter[id_order]", order.id.as_str());
        params.insert("output_format", "JSON");
        let threads: Vec<CustomerThread> = api
            .request_resources_wasm("customer_threads", params)
            .await
            .unwrap_or_default();

        let employees = self.employee_directory().await;
        let mut comments = Vec::new();
        for thread in threads.iter().filter(|t| !t.id.is_empty()) {
            for msg_ref in &thread.associations.customer_messages {
                if msg_ref.id.is_empty() {
                    continue;
                }
                let msg: CustomerMessage = match api
                    .request_subresources_by_id_wasm("customer_messages", "customer_message", &msg_ref.id)
                    .await
                {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("orders/prestashop -> customer_message {} fetch failed: {e:?}", msg_ref.id);
                        continue;
                    }
                };
                let author = employees
                    .get(&msg.id_employee)
                    .map(|e| format!("{} {}", e.firstname, e.lastname).trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        if msg.id_employee.is_empty() || msg.id_employee == "0" {
                            "Customer".to_string()
                        } else {
                            format!("Employee {}", msg.id_employee)
                        }
                    });
                comments.push(OrderComment {
                    id: msg.id.clone(),
                    author,
                    author_employee_id: Some(msg.id_employee.clone()).filter(|s| !s.is_empty() && s != "0"),
                    body: msg.message.clone(),
                    created_at: msg.date_add.clone(),
                    private: msg.private == "1",
                });
            }
        }
        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(comments)
    }

    async fn post_comment(&self, order: &QcOrder, tech: &TechIdentity, body: &str) -> anyhow::Result<OrderComment> {
        let api = Prestashop::default();
        let mut params = HashMap::new();
        params.insert("filter[id_order]", order.id.as_str());
        params.insert("output_format", "JSON");
        let threads: Vec<CustomerThread> = api
            .request_resources_wasm("customer_threads", params)
            .await
            .unwrap_or_default();

        let thread_id = match threads.iter().find(|t| !t.id.is_empty()) {
            Some(t) => t.id.clone(),
            None => {
                let id_customer = order
                    .id_customer
                    .as_deref()
                    .ok_or_else(|| anyhow!("order has no customer id; cannot open a thread"))?;
                api.create_customer_thread(&order.id, id_customer).await?.id
            }
        };

        let escaped = body
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let created = api
            .create_customer_message(&tech.id_employee, &thread_id, &escaped)
            .await?;

        Ok(OrderComment {
            id: created.id,
            author: tech.name.clone(),
            author_employee_id: Some(tech.id_employee.clone()),
            body: body.to_string(),
            created_at: created.date_add,
            private: true,
        })
    }

    async fn check_build_photos(&self, order: &QcOrder) -> anyhow::Result<PhotoCheck> {
        let rows = self.fetch_resource_rows("order_image", &order.id).await?;
        let count = rows
            .iter()
            .filter(|r| {
                r.get("id")
                    .map(|v| match v {
                        Value::String(s) => !s.is_empty(),
                        Value::Number(_) => true,
                        _ => false,
                    })
                    .unwrap_or(false)
            })
            .count();
        Ok(PhotoCheck { present: count > 0, count })
    }
}
