//! Live smoke test against the Xidax Build Management API.
//!
//! Network + a real `XBM_API_KEY` baked at compile time. Marked `#[ignore]`
//! so the default `cargo test` stays offline; run explicitly:
//!
//! ```text
//! cargo test -p database --test xbm_live -- --ignored --nocapture
//! ```
//!
//! Skips (does not fail) when no key is configured, so it's safe in CI.

use database::xbm::XbmClient;

fn client_or_skip() -> Option<XbmClient> {
    let c = XbmClient::from_env();
    if c.configured() {
        Some(c)
    } else {
        eprintln!("xbm_live: XBM_API_KEY not configured — skipping live test");
        None
    }
}

#[tokio::test]
#[ignore = "hits the live build-mgmt API; run with --ignored"]
async fn statuses_round_trip() {
    let Some(client) = client_or_skip() else { return };
    let statuses = client.statuses().await.expect("GET /statuses failed");
    assert!(!statuses.statuses.is_empty(), "no statuses returned");

    // 117-status machine migrated from PrestaShop: 4 = Shipped is stable.
    let shipped = statuses.statuses.iter().find(|s| s.legacy_id == 4);
    assert!(shipped.is_some(), "legacy_id 4 (Shipped) absent");
    eprintln!(
        "xbm_live: /statuses ok — {} statuses, e.g. {}",
        statuses.statuses.len(),
        shipped.map(|s| s.name.as_str()).unwrap_or("?")
    );

    // Surface the gate-relevant ids so drift from gate.rs is visible in logs.
    for lid in [73i64, 71, 67, 76, 109, 43, 224, 225] {
        match statuses.statuses.iter().find(|s| s.legacy_id == lid) {
            Some(s) => eprintln!("  legacy {lid:>4} = {:?}", s.name),
            None => eprintln!("  legacy {lid:>4} = <ABSENT>"),
        }
    }
}

#[tokio::test]
#[ignore = "hits the live build-mgmt API; run with --ignored"]
async fn queue_then_detail_round_trip() {
    let Some(client) = client_or_skip() else { return };

    let queue = client
        .orders(&[], None, None)
        .await
        .expect("GET /orders failed");
    eprintln!("xbm_live: /orders ok — {} orders", queue.orders.len());
    let Some(first) = queue.orders.first() else {
        eprintln!("xbm_live: empty queue — nothing to detail");
        return;
    };
    eprintln!(
        "  first: {} status={:?} serial={:?}",
        first.name,
        first.status.as_ref().map(|s| s.name.as_str()),
        first.build_serial
    );

    let detail = client
        .order_detail(&first.id)
        .await
        .expect("GET /orders/{id} failed");
    let order = detail.order.as_ref().expect("detail.order missing");
    assert!(!order.name.is_empty(), "order name empty");
    assert!(
        detail.current_status.is_some(),
        "detail.current_status missing — gate evaluation needs legacy_id"
    );
    eprintln!(
        "xbm_live: /orders/{} ok — {} line items, {} build photos, status legacy_id={}",
        XbmClient::order_path_id(&first.id),
        detail.line_items.len(),
        detail.build_photos.len(),
        detail.current_status.as_ref().map(|s| s.legacy_id).unwrap_or(-1),
    );

    // A line item that carries serials proves the nested decode path.
    if let Some(line) = detail.line_items.iter().find(|l| !l.serials.is_empty()) {
        let s = &line.serials[0];
        eprintln!(
            "  serial sample: slot={:?} serial={:?} reservation={:?}",
            line.slot, s.serial, s.reservation_status
        );
    }
}

#[tokio::test]
#[ignore = "hits the live build-mgmt API; run with --ignored"]
async fn order_backend_lookup_round_trip() {
    use database::orders::{OrderBackend, OrderKey, ShopifyBackend};

    let Some(_) = client_or_skip() else { return };
    let backend = ShopifyBackend::from_env();

    // Look up a seeded order by number, exercising the full QcOrder mapping.
    let queue = XbmClient::from_env()
        .orders(&[], None, None)
        .await
        .expect("queue fetch failed");
    let Some(sample) = queue.orders.iter().find(|o| o.name.starts_with('#')) else {
        eprintln!("xbm_live: no #-named order to look up");
        return;
    };
    let number = sample.name.trim_start_matches('#').to_string();
    let key = OrderKey::ShopifyOrderNumber(number.clone());

    let order = backend
        .find_order(&key)
        .await
        .unwrap_or_else(|e| panic!("find_order(#{number}) failed: {e:#}"));
    assert_eq!(order.reference, sample.name);
    let gate = backend.status_gate(&order);
    let spec = backend.build_spec(&order).await.expect("build_spec failed");
    eprintln!(
        "xbm_live: QcOrder #{number} — {} items, gate={:?} ({}), cpu={:?} gpu={:?} ram={:?}",
        order.items.len(),
        gate.outcome,
        gate.status_name,
        spec.cpu,
        spec.gpu,
        spec.ram,
    );

    // Federated serial lookup on the first attached serial, if any.
    if let Some(serial) = order.items.iter().flat_map(|i| &i.serials).next() {
        let hist = backend
            .serial_history(serial)
            .await
            .unwrap_or_else(|e| panic!("serial_history({serial}) failed: {e:#}"));
        eprintln!(
            "xbm_live: serial {serial} — found={} current={:?} odoo={:?} ps_allocs={} flags={:?}",
            hist.found, hist.current_order, hist.odoo_lot, hist.prestashop_allocations, hist.flags
        );

        // Reverse-resolve that serial back to this order (Phase 2 auto-resolve).
        let resolved = database::orders::resolve_any(std::slice::from_ref(serial)).await;
        let summary = resolved.unwrap_or_else(|| panic!("resolve_any({serial}) found nothing"));
        assert_eq!(summary.reference, sample.name, "serial should resolve to its order");
        assert_eq!(summary.lookup_input(), sample.name, "lookup_input round-trips to #N");
        eprintln!(
            "xbm_live: resolve_any({serial}) → {} ({}) lookup_input={}",
            summary.reference, summary.customer_name, summary.lookup_input()
        );
    }
}
