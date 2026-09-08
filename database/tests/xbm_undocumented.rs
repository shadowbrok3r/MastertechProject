//! Probe the Build Management endpoints that exist but are absent from
//! `/api/v1/openapi.json`.
//!
//! Diffing `app/routes/api.v1.*` in the `build-management` repo against the
//! hand-maintained spec turned up 32 routes the description does not mention.
//! Every one of them authenticates the same way the documented routes do, so
//! this asks the live API which ones our existing key can actually use — a
//! route being undocumented says nothing about whether it answers.
//!
//! ```text
//! cargo test -p database --test xbm_undocumented -- --ignored --nocapture
//! ```

use database::xbm::XbmClient;
use serde_json::json;

/// GET, POST, or "listed but not called here".
enum Probe {
    Get(&'static [(&'static str, &'static str)]),
    Post(fn() -> serde_json::Value),
}

fn describe(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            keys.truncate(8);
            format!("object {{{}}}", keys.join(", "))
        }
        serde_json::Value::Array(items) => format!("array[{}]", items.len()),
        other => {
            let s = other.to_string();
            s.chars().take(60).collect()
        }
    }
}

#[tokio::test]
#[ignore = "hits the live API; run with --ignored"]
async fn undocumented_endpoints_answer_our_key() {
    let client = XbmClient::from_env();
    if !client.configured() {
        eprintln!("xbm_undocumented: XBM not configured — skipping");
        return;
    }

    // Only the read-only probes run. The write-side gaps
    // (/inventory/mirror, /order-backlog/import, /odoo/picking-validate,
    // /roles POST, /staff/link-prestashop) mutate live data and are listed in
    // the report rather than called.
    let probes: Vec<(&str, Probe)> = vec![
        ("/odoo/serial-history", Probe::Get(&[("serial", "1234"), ("limit", "5")])),
        ("/odoo/product-meta", Probe::Get(&[])),
        ("/odoo/count", Probe::Get(&[("model", "sale.order"), ("domain", "[]")])),
        (
            "/odoo/query",
            Probe::Get(&[("model", "res.partner"), ("fields", "[\"name\"]"), ("limit", "2")]),
        ),
        ("/odoo/uninvoiced-orders", Probe::Get(&[("limit", "2")])),
        ("/odoo/adjustment-sweep", Probe::Get(&[("limit", "2")])),
        ("/odoo/rma-vendor-sweep", Probe::Get(&[("limit", "2")])),
        ("/odoo/product-history", Probe::Get(&[("limit", "2")])),
        ("/odoo/order-detail", Probe::Get(&[("name", "SA002104886")])),
        ("/roles", Probe::Get(&[])),
        ("/inventory-counts", Probe::Get(&[])),
        ("/odoo/serial-receipts", Probe::Post(|| json!({ "serials": ["1234"] }))),
        (
            "/odoo/component-snapshot",
            Probe::Post(|| json!({ "odooProductIds": [] })),
        ),
    ];

    let mut reachable = 0usize;
    let mut refused = 0usize;

    println!("\n{:<28} {:<8} {}", "endpoint", "verdict", "shape / error");
    println!("{}", "-".repeat(96));
    for (path, probe) in probes {
        let result = match probe {
            Probe::Get(params) => {
                let owned: Vec<(&str, String)> =
                    params.iter().map(|(k, v)| (*k, v.to_string())).collect();
                client.get_json(path, &owned).await
            }
            Probe::Post(body) => client.post_json(path, body()).await,
        };
        match result {
            Ok(value) => {
                reachable += 1;
                println!("{path:<28} {:<8} {}", "REACHES", describe(&value));
            }
            Err(e) => {
                refused += 1;
                println!("{path:<28} {:<8} {e}", "REFUSED");
            }
        }
    }

    println!("\nreachable {reachable}  refused {refused}");
    println!(
        "not probed (they mutate): /inventory/mirror, /order-backlog/import, \
         /odoo/picking-validate, /roles POST+PATCH+DELETE, /staff/link-prestashop, \
         and the 14 /inventory-counts/{{countId}}/* subroutes"
    );
}

/// No documented endpoint returns a Shopify customer id, which is what
/// `createServiceOrder` requires. Proven here rather than asserted so the
/// finding stays current: if a projection later gains the id, this prints it.
#[tokio::test]
#[ignore = "hits the live API; run with --ignored"]
async fn no_endpoint_hands_back_a_shopify_customer_id() {
    let client = XbmClient::from_env();
    if !client.configured() {
        return;
    }

    let mut found_any = false;
    for (label, value) in [
        ("/serials/1234", client.get_json("/serials/1234", &[]).await),
        ("/orders", client.get_json("/orders", &[]).await),
    ] {
        let Ok(value) = value else { continue };
        let raw = value.to_string();
        let has_gid = raw.contains("gid://shopify/Customer/");
        found_any |= has_gid;
        println!("{label:<18} carries a Customer gid: {has_gid}");
    }

    let detail = client.get_json("/orders/resolve", &[("ref", "1234".to_string())]).await;
    if let Ok(resolved) = detail {
        if let Some(gid) = resolved.get("orderGid").and_then(|v| v.as_str()) {
            let order = client.get_json(&format!("/orders/{gid}"), &[]).await;
            if let Ok(order) = order {
                let raw = order.to_string();
                let has_gid = raw.contains("gid://shopify/Customer/");
                found_any |= has_gid;
                println!("{:<18} carries a Customer gid: {has_gid}", "/orders/{id}");
                if let Some(customer) = order.get("order").and_then(|o| o.get("customer")) {
                    println!("{:<18} customer projection: {customer}", "");
                }
            }
        }
    }

    println!(
        "\nany Shopify customer id reachable: {found_any} \
         (false blocks order creation and customer attach)"
    );
}
/// `/odoo/query` takes `fields` comma-separated while `domain` is JSON, and a
/// JSON `fields` comes back undecodable rather than as the 400 a bad field
/// name gets. Pinned because the mistake is easy and the failure is opaque.
#[tokio::test]
#[ignore = "hits the live API; run with --ignored"]
async fn odoo_query_fields_is_csv_not_json() {
    let client = XbmClient::from_env();
    if !client.configured() {
        return;
    }

    let variants: Vec<(&str, Vec<(&str, String)>)> = vec![
        ("no fields", vec![("model", "res.partner".into()), ("limit", "1".into())]),
        ("bad model", vec![("model", "not.a.model".into())]),
        (
            "fields json",
            vec![
                ("model", "res.partner".into()),
                ("fields", "[\"name\"]".into()),
                ("limit", "1".into()),
            ],
        ),
        (
            "fields csv",
            vec![
                ("model", "res.partner".into()),
                ("fields", "name,email".into()),
                ("limit", "1".into()),
            ],
        ),
        (
            "fields typo",
            vec![("model", "sale.order".into()), ("fields", "nmae".into()), ("limit", "1".into())],
        ),
    ];

    for (label, params) in variants {
        match client.get_json("/odoo/query", &params).await {
            Ok(v) => println!(
                "{label:<12} OK  {}",
                v.to_string().chars().take(140).collect::<String>()
            ),
            Err(e) => println!("{label:<12} ERR {e}"),
        }
    }
}
