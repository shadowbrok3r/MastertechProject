//! Per-store Odoo stock for a part search, and which store should send one.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::schema::Store;
use crate::{ODOO_API_KEY, ODOO_DB, ODOO_JSONRPC_URL, ODOO_UID};

/// Products a search returns at most.
const PRODUCT_LIMIT: u32 = 8;

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<Vec<Value>>,
    #[serde(default)]
    error: Option<Value>,
}

fn client() -> reqwest::Client {
    #[cfg(not(target_arch = "wasm32"))]
    {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    }
    #[cfg(target_arch = "wasm32")]
    {
        reqwest::Client::new()
    }
}

/// Odoo `execute_kw` search_read; `domain` is the full positional domain argument.
pub async fn search_read(model: &str, domain: Value, fields: Value, limit: Option<u32>) -> anyhow::Result<Vec<Value>> {
    let uid: u32 = ODOO_UID.parse().map_err(|_| anyhow::anyhow!("ODOO_UID must be a decimal u32"))?;
    let mut kwargs = json!({ "fields": fields });
    if let Some(l) = limit {
        kwargs["limit"] = json!(l);
    }
    let body = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 1,
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [ODOO_DB, uid, ODOO_API_KEY, model, "search_read", domain, kwargs]
        }
    });
    let env: Envelope = client().post(ODOO_JSONRPC_URL).json(&body).send().await?.json().await?;
    if let Some(err) = env.error {
        let msg = err.pointer("/data/message").or_else(|| err.get("message")).cloned().unwrap_or(err);
        return Err(anyhow::anyhow!("Odoo {model} search_read failed: {msg}"));
    }
    Ok(env.result.unwrap_or_default())
}

/// A product that matched a part search.
#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub code: Option<String>,
}

/// Available units of one product at one store.
#[derive(Clone, Debug, PartialEq)]
pub struct StoreStock {
    pub product_id: i64,
    pub store: Store,
    pub available: f64,
}

/// Odoo's `false` for an empty char field becomes `None`.
fn text(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The id of a many2one `[id, "name"]` pair.
fn m2o_id(v: Option<&Value>) -> Option<i64> {
    v.and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_i64)
}

/// Domain matching every word in the name, or the whole query as internal reference or barcode.
pub fn product_domain(query: &str) -> Value {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut domain: Vec<Value> = vec![json!("|"), json!("|")];
    domain.extend(std::iter::repeat_n(json!("&"), words.len().saturating_sub(1)));
    domain.extend(words.iter().map(|w| json!(["name", "ilike", w])));
    if words.is_empty() {
        domain.push(json!(["name", "ilike", query.trim()]));
    }
    domain.push(json!(["default_code", "ilike", query.trim()]));
    domain.push(json!(["barcode", "ilike", query.trim()]));
    json!([domain])
}

fn product_from_row(r: &Value) -> Option<Product> {
    Some(Product {
        id: r.get("id")?.as_i64()?,
        name: text(r.get("display_name")).unwrap_or_default(),
        code: text(r.get("default_code")),
    })
}

/// Products whose name has every word of `query`, or whose code or barcode contains it.
pub async fn find_products(query: &str) -> anyhow::Result<Vec<Product>> {
    let rows = search_read(
        "product.product",
        product_domain(query),
        json!(["id", "display_name", "default_code"]),
        Some(PRODUCT_LIMIT),
    )
    .await?;
    Ok(rows.iter().filter_map(product_from_row).collect())
}

/// One product by its `product.product` id.
pub async fn product_by_id(id: i64) -> anyhow::Result<Option<Product>> {
    let rows = search_read(
        "product.product",
        json!([[["id", "=", id]]]),
        json!(["id", "display_name", "default_code"]),
        Some(1),
    )
    .await?;
    Ok(rows.iter().find_map(product_from_row))
}

/// Odoo location ids of the five store stock locations.
fn store_locations() -> Vec<i32> {
    Store::VALUES.iter().map(|s| s.into_odoo_store_id()).collect()
}

/// Sums quant rows into available units per product and store.
pub fn tally_quants(rows: &[Value]) -> Vec<StoreStock> {
    let mut sums: BTreeMap<(i64, Store), f64> = BTreeMap::new();
    for r in rows {
        let (Some(product), Some(location)) = (m2o_id(r.get("product_id")), m2o_id(r.get("location_id"))) else {
            continue;
        };
        let Some(store) = Store::try_from_odoo_store_id(&location.to_string()) else { continue };
        let qty = r.get("quantity").and_then(Value::as_f64).unwrap_or(0.0);
        let reserved = r.get("reserved_quantity").and_then(Value::as_f64).unwrap_or(0.0);
        *sums.entry((product, store)).or_default() += qty - reserved;
    }
    sums.into_iter()
        .map(|((product_id, store), available)| StoreStock { product_id, store, available: available.max(0.0) })
        .collect()
}

/// Available units of `product_ids` at each store.
pub async fn store_stock(product_ids: &[i64]) -> anyhow::Result<Vec<StoreStock>> {
    if product_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = search_read(
        "stock.quant",
        json!([[["product_id", "in", product_ids], ["location_id", "in", store_locations()]]]),
        json!(["product_id", "location_id", "quantity", "reserved_quantity"]),
        Some(5000),
    )
    .await?;
    Ok(tally_quants(&rows))
}

/// Approximate miles along I-15 from Riverdale.
fn position(store: Store) -> i32 {
    match store {
        Store::RIV => 0,
        Store::LTN => 12,
        Store::MUR => 35,
        Store::SAN => 43,
        Store::ORE => 65,
    }
}

/// Where a part for `dest` should come from.
#[derive(Clone, Debug, PartialEq)]
pub enum RoutePlan {
    InStock { available: f64 },
    SendFrom { store: Store, available: f64 },
    NoStock,
}

/// Uses `dest`'s own stock when enough, else the nearest store holding `need` units.
pub fn plan_route(dest: Store, need: f64, stock: &[(Store, f64)]) -> RoutePlan {
    let here = stock.iter().filter(|(s, _)| *s == dest).map(|(_, n)| *n).sum::<f64>();
    if here >= need {
        return RoutePlan::InStock { available: here };
    }
    stock
        .iter()
        .filter(|(s, n)| *s != dest && *n >= need)
        .min_by(|(a, na), (b, nb)| {
            (position(*a) - position(dest)).abs().cmp(&(position(*b) - position(dest)).abs()).then(nb.total_cmp(na))
        })
        .map(|(store, available)| RoutePlan::SendFrom { store: *store, available: *available })
        .unwrap_or(RoutePlan::NoStock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_ands_name_words_and_ors_codes() {
        let d = product_domain("1TB NVMe");
        assert_eq!(
            d,
            json!([[
                "|",
                "|",
                "&",
                ["name", "ilike", "1TB"],
                ["name", "ilike", "NVMe"],
                ["default_code", "ilike", "1TB NVMe"],
                ["barcode", "ilike", "1TB NVMe"]
            ]])
        );
        let single = product_domain("SSD");
        assert_eq!(single[0].as_array().unwrap().len(), 5);
    }

    #[test]
    fn quants_sum_per_store_minus_reserved() {
        let rows = vec![
            json!({"product_id": [7, "SSD"], "location_id": [76, "RIV/Stock"], "quantity": 2.0, "reserved_quantity": 1.0}),
            json!({"product_id": [7, "SSD"], "location_id": [76, "RIV/Stock"], "quantity": 1.0, "reserved_quantity": 0.0}),
            json!({"product_id": [7, "SSD"], "location_id": [77, "SAN/Stock"], "quantity": 3.0, "reserved_quantity": 0.0}),
            json!({"product_id": [7, "SSD"], "location_id": [999, "Elsewhere"], "quantity": 9.0}),
            json!({"product_id": false, "location_id": [77, "SAN/Stock"], "quantity": 9.0}),
        ];
        let stock = tally_quants(&rows);
        assert_eq!(
            stock,
            vec![
                StoreStock { product_id: 7, store: Store::RIV, available: 2.0 },
                StoreStock { product_id: 7, store: Store::SAN, available: 3.0 },
            ]
        );
    }

    #[test]
    fn route_prefers_own_stock_then_nearest() {
        assert_eq!(plan_route(Store::RIV, 1.0, &[(Store::RIV, 2.0)]), RoutePlan::InStock { available: 2.0 });
        let stock = [(Store::ORE, 5.0), (Store::LTN, 1.0), (Store::SAN, 4.0)];
        assert_eq!(plan_route(Store::RIV, 1.0, &stock), RoutePlan::SendFrom { store: Store::LTN, available: 1.0 });
        // LTN holds too few for two, so the next nearest store sends.
        assert_eq!(plan_route(Store::RIV, 2.0, &stock), RoutePlan::SendFrom { store: Store::SAN, available: 4.0 });
        assert_eq!(plan_route(Store::MUR, 9.0, &stock), RoutePlan::NoStock);
    }
}
