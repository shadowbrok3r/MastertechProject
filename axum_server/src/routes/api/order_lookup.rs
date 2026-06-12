//! Serial → order lookup for the pre-OS UEFI QC agent.
//!
//! `GET /api/v1/qc/order-by-serial/{serial}` resolves a scanned/typed value to
//! an order through every backend the warehouse uses, in this priority:
//!
//! 1. `XBS-*` build serials → Shopify (Xidax).
//! 2. PrestaShop `order_serial` rows (component serial attached at build).
//! 3. XBM serial history (Shopify install scans, with Odoo lot context).
//! 4. Order-number shapes (`#1042`, `2xxxxx`, `5xxxxxxx`) routed by key shape.
//!
//! The response is a slim, stable JSON contract shaped for the UEFI renderer:
//! firmware parses it with a fixed struct, so additive changes only.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use database::orders::{BackendKind, OrderKey, QcBackend, QcOrder};
use database::schema::prestashop::{OrderSerialEntry, Prestashop};
use database::xbm::XbmClient;
use serde_json::{json, Value};

/// Per-leg budget so one slow backend can't starve the chain; the UEFI client
/// reads with a 45 s budget overall.
const LEG_TIMEOUT: Duration = Duration::from_secs(12);

pub async fn order_by_serial(Path(serial): Path<String>) -> impl IntoResponse {
    let needle = serial.trim().to_string();
    if needle.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "found": false, "error": "empty serial" })),
        );
    }

    let mut tried: Vec<String> = Vec::new();
    match resolve(&needle, &mut tried).await {
        Some(hit) => {
            let body = render_hit(&needle, hit).await;
            (StatusCode::OK, Json(body))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "found": false,
                "serial": needle,
                "tried": tried,
                "error": "no order found for serial",
            })),
        ),
    }
}

struct Hit {
    backend: QcBackend,
    order: QcOrder,
    matched_by: &'static str,
    odoo: Option<Value>,
}

async fn resolve(needle: &str, tried: &mut Vec<String>) -> Option<Hit> {
    // 1. Xidax build serial minted at orders/create.
    if needle.to_uppercase().starts_with("XBS-") {
        tried.push("shopify:build_serial".into());
        let key = OrderKey::BuildSerial(needle.to_uppercase());
        if let Some(order) = find_with(&key).await {
            return Some(Hit {
                backend: QcBackend::for_key(&key),
                order,
                matched_by: "build_serial",
                odoo: None,
            });
        }
    }

    // 2. PrestaShop component-serial rows (PCL builds).
    tried.push("prestashop:order_serial".into());
    if let Some(id_order) = prestashop_serial_to_order(needle).await {
        let key = OrderKey::Prestashop(id_order);
        if let Some(order) = find_with(&key).await {
            return Some(Hit {
                backend: QcBackend::for_key(&key),
                order,
                matched_by: "order_serial",
                odoo: None,
            });
        }
    }

    // 3. XBM serial history: Shopify install scans + Odoo lot for the serial.
    let xbm = XbmClient::from_env();
    if xbm.configured() {
        tried.push("xbm:serial_history".into());
        if let Ok(Ok(hist)) =
            tokio::time::timeout(LEG_TIMEOUT, xbm.serial_history(needle)).await
        {
            let odoo = hist.odoo.as_ref().map(|o| {
                json!({
                    "lot_id": o.lot_id,
                    "name": o.name,
                    "product_name": o.product_name,
                    "ref": o.r#ref,
                })
            });
            // Prefer the Shopify install record's order; fall back to a PS row
            // the history may carry.
            let shopify_order_no = hist
                .shopify
                .as_ref()
                .and_then(|s| s.order.as_ref())
                .and_then(|o| o.name.as_deref())
                .map(|n| n.trim_start_matches('#').to_string());
            if let Some(no) = shopify_order_no {
                let key = OrderKey::ShopifyOrderNumber(no);
                if let Some(order) = find_with(&key).await {
                    return Some(Hit {
                        backend: QcBackend::for_key(&key),
                        order,
                        matched_by: "xbm_serial_history",
                        odoo,
                    });
                }
            }
            if let Some(id_order) = hist
                .prestashop
                .iter()
                .find_map(|p| p.id_order.clone())
            {
                let key = OrderKey::Prestashop(id_order);
                if let Some(order) = find_with(&key).await {
                    return Some(Hit {
                        backend: QcBackend::for_key(&key),
                        order,
                        matched_by: "xbm_serial_history",
                        odoo,
                    });
                }
            }
        }
    }

    // 4. Order-number shapes typed directly (PS id / Everest doc / Shopify #).
    if let Some(key) = OrderKey::parse(needle) {
        tried.push(format!("order_key:{}", key.backend().as_str()));
        if let Some(order) = find_with(&key).await {
            return Some(Hit {
                backend: QcBackend::for_key(&key),
                order,
                matched_by: "order_number",
                odoo: None,
            });
        }
    }

    None
}

/// `order_serial` filter: component serial → owning PS order id.
async fn prestashop_serial_to_order(serial: &str) -> Option<String> {
    let api = Prestashop::default();
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[serial_number]", serial);
    query.insert("output_format", "JSON");
    let rows: Vec<OrderSerialEntry> = tokio::time::timeout(
        LEG_TIMEOUT,
        api.request_resources_wasm("order_serial", query),
    )
    .await
    .ok()?
    .ok()?;
    rows.into_iter().next().map(|r| r.id_order)
}

async fn find_with(key: &OrderKey) -> Option<QcOrder> {
    let backend = QcBackend::for_key(key);
    match tokio::time::timeout(LEG_TIMEOUT, backend.find_order(key)).await {
        Ok(Ok(order)) => Some(order),
        Ok(Err(e)) => {
            log::warn!("order-by-serial: {key:?} via {:?}: {e:#}", backend.backend_kind());
            None
        }
        Err(_) => {
            log::warn!("order-by-serial: {key:?} timed out");
            None
        }
    }
}

/// Slim JSON contract for the firmware renderer.
async fn render_hit(needle: &str, hit: Hit) -> Value {
    let Hit { backend, order, matched_by, odoo } = hit;

    // Spec + gate are best-effort decorations; the order itself is the result.
    let spec = match tokio::time::timeout(LEG_TIMEOUT, backend.build_spec(&order)).await {
        Ok(Ok(s)) if !s.is_empty() => serde_json::to_value(&s).ok(),
        _ => None,
    };
    let gate = serde_json::to_value(backend.status_gate(&order)).ok();

    let items: Vec<Value> = order
        .items
        .iter()
        .map(|i| {
            json!({
                "name": i.name,
                "reference": i.reference,
                "qty": i.quantity,
                "unit_price": i.unit_price,
                "serials": i.serials,
            })
        })
        .collect();

    let service = order.service_info.as_ref().map(|s| {
        json!({
            "device": s.device_name,
            "mfg": s.device_mfg,
            "model": s.device_model,
            "serial": s.device_serial,
            "notes": s.check_in_notes,
        })
    });

    json!({
        "found": true,
        "serial": needle,
        "matched_by": matched_by,
        "backend": match order.backend.unwrap_or(backend.backend_kind()) {
            BackendKind::Prestashop => "prestashop",
            BackendKind::Shopify => "shopify",
        },
        "order": {
            "id": order.id,
            "reference": order.reference,
            "customer": order.customer_name,
            "kind": order.kind.as_str(),
            "status": { "legacy_id": order.status.legacy_id, "name": order.status.name },
            "total": order.total_paid,
            "build_serial": order.build_serial,
            "everest_doc": order.everest_doc,
            "note": order.note,
            "items": items,
            "service": service,
        },
        "spec": spec,
        "gate": gate,
        "odoo": odoo,
    })
}
