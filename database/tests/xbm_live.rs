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

use database::xbm::{
    CecLabelMode, CommentVisibility, CommentsQuery, ConfigRef, Oa3InjectionQuery, RmaReport,
    XbmClient, XbmError,
};

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

/// Decode failures, transport errors and 400s; other refusals are printed only.
#[derive(Default)]
struct Probes {
    failures: Vec<String>,
}

impl Probes {
    fn check<T: std::fmt::Display>(&mut self, label: &str, result: Result<T, XbmError>) {
        match result {
            Ok(summary) => eprintln!("  ok       {label:<44} {summary}"),
            Err(e @ XbmError::Api { status, .. }) if status != 400 => {
                eprintln!("  refused  {label:<44} {e}")
            }
            Err(e) => {
                eprintln!("  FAIL     {label:<44} {e}");
                self.failures.push(format!("{label}: {e}"));
            }
        }
    }
}

fn shape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            format!("{{{}}}", keys.join(", "))
        }
        other => other.to_string().chars().take(60).collect(),
    }
}

/// Read-only calls; the CEC label runs in `DryRun`, which writes no snapshot.
#[tokio::test]
#[ignore = "hits the live build-mgmt API; run with --ignored"]
async fn new_read_endpoints_decode() {
    let Some(client) = client_or_skip() else {
        return;
    };
    let mut probes = Probes::default();

    probes.check(
        "GET /statuses",
        client.statuses().await.map(|p| {
            let retired = p.statuses.iter().filter(|s| s.retired).count();
            let ordered = p.statuses.iter().filter(|s| s.display_order > 0).count();
            format!(
                "{} statuses, {retired} retired, {ordered} ordered",
                p.statuses.len()
            )
        }),
    );
    probes.check(
        "GET /staff?permission=qc.perform",
        client
            .staff_with_permission("qc.perform", Some(true))
            .await
            .map(|p| format!("{} holders", p.staff.len())),
    );
    probes.check(
        "GET /app-versions?name=OA3InjectionWrapper",
        client
            .app_version("OA3InjectionWrapper")
            .await
            .map(|v| format!("{} retired={}", v.version, v.retired)),
    );
    probes.check(
        "GET /skus/resolve?sku=LAP/BB/165070TI",
        client
            .resolve_sku("LAP/BB/165070TI")
            .await
            .map(|r| match r {
                Some(r) => format!("{:?} build={}", r.product_title, r.build.is_some()),
                None => "sku_not_found".to_string(),
            }),
    );
    probes.check(
        "GET /oa3/injection-log?limit=3",
        client
            .oa3_injections(Oa3InjectionQuery {
                limit: Some(3),
                ..Default::default()
            })
            .await
            .map(|p| format!("{} rows", p.injections.len())),
    );
    probes.check(
        "GET /corp-deals",
        client.corp_deals(None, None).await.map(|v| shape(&v)),
    );
    probes.check("GET /roles", client.roles(None).await.map(|v| shape(&v)));
    probes.check(
        "GET /rma/reports/credit-not-debited",
        client
            .rma_report(RmaReport::CreditNotDebited)
            .await
            .map(|v| shape(&v)),
    );
    probes.check(
        "GET /rma/dwell-sweep",
        client.rma_dwell_preview().await.map(|v| shape(&v)),
    );
    probes.check(
        "GET /balance-invoices?limit=3",
        client.balance_invoices(Some(3)).await.map(|v| shape(&v)),
    );

    let queue = client
        .orders(&[], None, None)
        .await
        .expect("GET /orders failed");
    let named = || queue.orders.iter().filter(|o| o.name.starts_with('#'));
    let Some(first) = named()
        .find(|o| o.build_serial.as_deref().is_some_and(|s| !s.is_empty()))
        .or_else(|| named().next())
    else {
        eprintln!("xbm_live: no #-named order for the order-scoped probes");
        assert!(probes.failures.is_empty(), "{:#?}", probes.failures);
        return;
    };
    let number = first.name.trim_start_matches('#');
    let resolved = client
        .resolve(number)
        .await
        .expect("GET /orders/resolve failed");
    eprintln!("xbm_live: order-scoped probes on {}", first.name);

    probes.check(
        "GET /oa3/attach",
        client
            .oa3_attach_target(number, resolved.config_id.as_deref())
            .await
            .map(|t| {
                format!(
                    "{} os lines, injected={}",
                    t.os_lines.len(),
                    t.already_injected
                )
            }),
    );
    probes.check(
        "GET /orders/{id}/service",
        client
            .service_record(&resolved.order_gid)
            .await
            .map(|r| format!("device={:?}", r.details.device_name)),
    );
    probes.check(
        "GET /orders/{id}/comments?visibility=internal",
        client
            .comments_with(
                &resolved.order_gid,
                CommentsQuery {
                    limit: Some(1),
                    visibility: Some(CommentVisibility::Internal),
                    ..Default::default()
                },
            )
            .await
            .map(|p| format!("{} comments", p.comments.len())),
    );

    // Build-scoped probes on the Xidax store, using the guide's documented build-sheet pair.
    let xidax = XbmClient::from_env().for_shop("");
    match xidax.resolve("3840-53147").await {
        Ok(pair) => {
            if let Some(config_id) = pair.config_id.as_deref() {
                probes.check(
                    "GET /orders/{id}?configId= (xidax)",
                    xidax
                        .order_detail_for_config(&pair.order_gid, ConfigRef::Id(config_id))
                        .await
                        .map(|d| {
                            let active = d.configs.iter().filter(|c| c.active).count();
                            format!(
                                "{} lines, {} configs ({active} active)",
                                d.line_items.len(),
                                d.configs.len()
                            )
                        }),
                );
            }
            probes.check(
                "GET /builds/cec-label?dry=1 (xidax)",
                xidax
                    .cec_label("3840-53147", CecLabelMode::DryRun)
                    .await
                    .map(|l| {
                        format!(
                            "{:?} complete={} missing={}",
                            l.cec_model,
                            l.cec_complete,
                            l.cec_missing.len()
                        )
                    }),
            );
        }
        Err(e) => eprintln!("  skipped  xidax build-scoped probes: {e}"),
    }

    assert!(probes.failures.is_empty(), "{:#?}", probes.failures);
}
