//! Live read-parity sweep: fetches real orders from both backends and reports
//! every field that disagrees.
//!
//! Network, a real `XBM_API_KEY`, and a reachable PrestaShop. Marked `#[ignore]`
//! so the default `cargo test` stays offline.
//!
//! ```text
//! # Sweep the Shopify build queue, comparing any order that carries a legacy
//! # PrestaShop id against its PrestaShop original:
//! cargo test -p database --test parity_live -- --ignored --nocapture
//!
//! # Compare one specific pair instead:
//! PARITY_PS_ID=2152446 PARITY_SHOPIFY=1003 \
//!   cargo test -p database --test parity_live one_pair -- --ignored --nocapture
//! ```
//!
//! Skips (does not fail) when a backend is unconfigured, so it is safe in CI.
//! It reports rather than asserts: the exit criterion for Phase 1 is that the
//! differences are *understood*, and a red test cannot tell you that.

use database::orders::{parity, OrderKey, ShopifyBackend};

fn shopify_or_skip() -> Option<ShopifyBackend> {
    let backend = ShopifyBackend::from_env();
    if backend.configured() {
        Some(backend)
    } else {
        eprintln!("parity_live: Shopify/XBM not configured — skipping");
        None
    }
}

fn prestashop_configured() -> bool {
    if database::prestashop_configured() {
        true
    } else {
        eprintln!("parity_live: PrestaShop not configured in this build — skipping");
        false
    }
}

/// Compare one explicitly named pair from the environment.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn one_pair() {
    let (Some(ps_id), Some(shopify_ref)) = (
        std::env::var("PARITY_PS_ID").ok(),
        std::env::var("PARITY_SHOPIFY").ok(),
    ) else {
        eprintln!("parity_live: set PARITY_PS_ID and PARITY_SHOPIFY to compare a specific pair");
        return;
    };
    if shopify_or_skip().is_none() || !prestashop_configured() {
        return;
    }

    let ps_key = OrderKey::Prestashop(ps_id);
    let shopify_key =
        OrderKey::parse(&shopify_ref).unwrap_or(OrderKey::ShopifyOrderNumber(shopify_ref));
    let report = parity::compare(&ps_key, &shopify_key).await;
    println!("{}", report.summary());
    for diff in report.mismatches() {
        println!(
            "  {:<22} {:?}\n      prestashop: {}\n      shopify   : {}",
            diff.field, diff.verdict, diff.prestashop, diff.shopify
        );
    }
}

/// Sweep the build queue and compare every order that has a PrestaShop
/// ancestor. Prints a per-field tally so the common differences surface first.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn sweep_migrated_orders() {
    let Some(backend) = shopify_or_skip() else { return };
    if !prestashop_configured() {
        return;
    }

    let limit: usize = std::env::var("PARITY_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);

    let queue = match backend.recent_orders(limit).await {
        Ok(q) => q,
        Err(e) => {
            eprintln!("parity_live: queue fetch failed: {e}");
            return;
        }
    };
    println!("comparing up to {} orders from the build queue\n", queue.len());

    let mut compared = 0usize;
    let mut clean = 0usize;
    let mut skipped_no_legacy = 0usize;
    let mut field_tally: std::collections::BTreeMap<String, usize> = Default::default();

    for summary in queue {
        let key = OrderKey::parse(&summary.reference)
            .unwrap_or_else(|| OrderKey::ShopifyOrderNumber(summary.reference.clone()));
        match parity::compare_against_legacy(&key).await {
            Ok(None) => {
                // Shopify-native order with no PrestaShop ancestor: nothing to
                // compare against, which is expected and not a finding.
                skipped_no_legacy += 1;
            }
            Ok(Some(report)) => {
                compared += 1;
                if report.is_clean() {
                    clean += 1;
                } else {
                    print!("{}", report.summary());
                    for diff in report.mismatches() {
                        // Strip the sku out of `item[x].y` so the tally groups.
                        let bucket = match (diff.field.find('['), diff.field.find(']')) {
                            (Some(a), Some(b)) if b > a => {
                                format!("{}[…]{}", &diff.field[..a], &diff.field[b + 1..])
                            }
                            _ => diff.field.clone(),
                        };
                        *field_tally.entry(bucket).or_default() += 1;
                    }
                }
            }
            Err(e) => eprintln!("{}: compare failed: {e}", summary.reference),
        }
    }

    println!("\n─── read parity ───");
    println!("compared           {compared}");
    println!("clean              {clean}");
    println!("with differences   {}", compared.saturating_sub(clean));
    println!("no PrestaShop id   {skipped_no_legacy} (expected for Shopify-native orders)");
    if !field_tally.is_empty() {
        println!("\nfields by how often they differed:");
        let mut rows: Vec<(&String, &usize)> = field_tally.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (field, count) in rows {
            println!("  {count:>4}  {field}");
        }
    }
}

/// Load the status table from the live backends and report what the compiled
/// tables were missing.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn status_catalog_covers_more_than_the_compiled_tables() {
    use database::orders::{gate, status_catalog};

    let cached = status_catalog::refresh().await;
    if cached == 0 {
        eprintln!("parity_live: no backend served a status table — skipping");
        return;
    }
    println!("status catalog: {cached} statuses loaded");

    let mut gained = Vec::new();
    let mut renamed = Vec::new();
    for id in 1..=260i64 {
        let Some(live) = status_catalog::name(id) else { continue };
        let compiled = gate::status_name(id);
        if compiled.is_empty() {
            gained.push((id, live));
        } else if compiled != live {
            renamed.push((id, compiled.to_string(), live));
        }
    }

    println!("\nstatuses the compiled table could not name: {}", gained.len());
    for (id, name) in gained.iter().take(20) {
        println!("  {id:>4}  {name}");
    }
    if gained.len() > 20 {
        println!("  … and {} more", gained.len() - 20);
    }

    println!("\nstatuses whose compiled name disagrees with the live one: {}", renamed.len());
    for (id, compiled, live) in &renamed {
        println!("  {id:>4}  compiled={compiled:?}  live={live:?}");
    }
}

/// The Build Management API is multi-store and defaults to Xidax. This proves
/// our client can reach the PC Laptops store, which it could not before.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn client_can_target_the_pcl_store() {
    use database::xbm::XbmClient;

    let base = XbmClient::from_env();
    if !base.configured() {
        eprintln!("parity_live: XBM not configured — skipping");
        return;
    }

    for shop in ["", "pclaptops"] {
        let client = XbmClient::from_env().for_shop(shop);
        match client.orders(&[], None, None).await {
            Ok(q) => {
                let entities: std::collections::BTreeSet<&str> = q
                    .orders
                    .iter()
                    .filter_map(|o| o.entity.as_deref())
                    .collect();
                println!(
                    "shop={:<12} {:>3} orders  entities={:?}",
                    if shop.is_empty() { "<default>" } else { shop },
                    q.orders.len(),
                    entities
                );
            }
            Err(e) => println!("shop={shop:<12} error: {e}"),
        }
    }
}

/// Read a real Xidax repair order end-to-end through our own backend.
/// `PARITY_ORDER` / `PARITY_SHOP` override the defaults.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn service_order_reads_through_the_backend() {
    use database::orders::{OrderBackend, OrderKey};
    use database::xbm::XbmClient;

    let reference = std::env::var("PARITY_ORDER").unwrap_or_else(|_| "3879".into());
    let shop = std::env::var("PARITY_SHOP").unwrap_or_else(|_| "37rkv3-nc".into());

    if !XbmClient::from_env().configured() {
        eprintln!("parity_live: XBM not configured — skipping");
        return;
    }
    println!("compiled XBM_SHOP = {:?}; this test targets {shop:?}", database::XBM_SHOP);

    let backend = ShopifyBackend::from_env().for_shop(&shop);
    let key = OrderKey::parse(&reference).expect("reference parses");

    match backend.find_order(&key).await {
        Err(e) => println!("find_order failed: {e}"),
        Ok(order) => {
            println!("\n─── order as our backend sees it ───");
            println!("  reference     {}", order.reference);
            println!("  kind          {:?}", order.kind);
            println!("  status        {} ({})", order.status.name, order.status.legacy_id);
            println!("  customer      {}", order.customer_name);
            println!("  items         {}", order.items.len());
            println!("  build_serial  {:?}", order.build_serial);
            println!("  truncated     {:?}", order.truncated);
            match order.service_info.as_ref() {
                Some(s) => println!(
                    "  service       device={:?} mfg={:?} model={:?} serial={:?}\n                checkin={:?}",
                    s.device_name, s.device_mfg, s.device_model, s.device_serial, s.check_in_notes
                ),
                None => println!("  service       <<NOT PARSED>>"),
            }
        }
    }
}

/// Exercise the write paths on a real order through our own backend.
/// Opt in explicitly — this mutates a live order.
#[tokio::test]
#[ignore = "mutates a live order; run with --ignored and PARITY_WRITE=1"]
async fn write_paths_through_the_backend() {
    use database::orders::{OrderBackend, OrderKey, TechIdentity};

    if std::env::var("PARITY_WRITE").ok().as_deref() != Some("1") {
        eprintln!("parity_live: set PARITY_WRITE=1 to run the write test");
        return;
    }
    let reference = std::env::var("PARITY_ORDER").unwrap_or_else(|_| "3879".into());
    let shop = std::env::var("PARITY_SHOP").unwrap_or_else(|_| "37rkv3-nc".into());

    let backend = ShopifyBackend::from_env().for_shop(&shop);
    if !backend.configured() {
        eprintln!("parity_live: XBM not configured — skipping");
        return;
    }
    let key = OrderKey::parse(&reference).expect("reference parses");
    let order = match backend.find_order(&key).await {
        Ok(o) => o,
        Err(e) => { println!("find_order failed: {e}"); return }
    };
    println!("order {} status={} ({})", order.reference, order.status.name, order.status.legacy_id);

    // 1. Post a note as the API key (no floor credential exchanged).
    let tech = TechIdentity {
        id_employee: "48".into(),
        name: "Logan Lees".into(),
        ..Default::default()
    };
    match backend.post_comment(&order, &tech, "Mastertech backend write test — post_comment.").await {
        Ok(c) => println!("post_comment   OK  id={} author={}", c.id, c.author),
        Err(e) => println!("post_comment   ERR {e}"),
    }

    // 2. Read the stream back, which is what a tech would see.
    match backend.fetch_comments(&order).await {
        Ok(cs) => {
            println!("fetch_comments OK  {} comments", cs.len());
            for c in cs.iter().take(3) {
                println!("   [{}] {}", c.author, c.body.chars().take(60).collect::<String>());
            }
        }
        Err(e) => println!("fetch_comments ERR {e}"),
    }

    // 3. Status advance — expected to fail on scope, which is the finding.
    match backend.advance_status(&order, 30).await {
        Ok(()) => println!("advance_status OK  moved to 30"),
        Err(e) => println!("advance_status ERR {e:#}"),
    }
}

#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn serial_history_is_stable() {
    use database::xbm::XbmClient;
    let c = XbmClient::from_env().for_shop("37rkv3-nc");
    if !c.configured() { return }
    for i in 1..=4 {
        match c.serial_history("1234").await {
            Ok(h) => println!("  {i}: ok found={} events={}", h.found, h.history.len()),
            Err(e) => println!("  {i}: ERR {e}"),
        }
    }
}

/// Resolve a serial to its customer through both legs. `PARITY_SERIAL`
/// overrides; the default is a serial known to be attached on the PCL store.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn serial_resolves_to_a_customer() {
    use database::orders::serial_lookup;

    let serials: Vec<String> = std::env::var("PARITY_SERIAL")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(|_| vec!["1234".into()]);

    for serial in serials {
        match serial_lookup::lookup(&serial).await {
            Ok(None) => println!("{serial:<24} no backend knows it"),
            Ok(Some(hit)) => {
                println!(
                    "{serial:<24} source={} name={:?} email={:?} phone={:?}",
                    hit.source, hit.name, hit.email, hit.phone
                );
                println!(
                    "  ids: shopify={:?} prestashop={:?} attachable={}",
                    hit.customer_gid,
                    hit.id_customer,
                    hit.has_customer_id()
                );
                for order in &hit.orders {
                    println!(
                        "  order {:<14} legacy={:<10} status={:<24} matched_by={}",
                        order.reference, order.legacy_order_id, order.status_name, order.matched_by
                    );
                }
            }
            Err(e) => println!("{serial:<24} ERR {e:#}"),
        }
    }
}

/// Pull real serials out of PrestaShop and run the lookup on them, so the
/// PrestaShop leg is exercised against data that exists.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn prestashop_serials_resolve_to_their_customers() {
    use database::orders::serial_lookup;
    use database::schema::prestashop::{Customer, Prestashop};

    if !prestashop_configured() {
        return;
    }
    let api = Prestashop::default();
    let mut query = std::collections::HashMap::new();
    query.insert("output_format", "JSON");
    query.insert("display", "full");
    query.insert("limit", "8");
    query.insert("sort", "id_DESC");

    #[derive(serde::Deserialize, Debug)]
    struct Row {
        serial_number: Option<String>,
    }
    let rows: Vec<Row> = match api
        .request_resources_checked_as("order_serial", "order_serials", query)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            println!("order_serial listing failed: {e}");
            return;
        }
    };
    println!("sampled {} order_serial rows", rows.len());

    for row in rows.iter().take(5) {
        let Some(serial) = row.serial_number.as_deref().map(str::trim).filter(|s| !s.is_empty())
        else {
            continue;
        };

        let direct = Customer::find_customer_by_serial(serial).await;
        match &direct {
            Ok(pairs) => println!(
                "{serial:<26} prestashop leg: {} customer(s){}",
                pairs.len(),
                pairs
                    .first()
                    .map(|(c, a)| format!(" -> {} {} <{}> {}", c.firstname, c.lastname, c.email, a.phone))
                    .unwrap_or_default()
            ),
            Err(e) => println!("{serial:<26} prestashop leg ERR {e}"),
        }

        match serial_lookup::lookup(serial).await {
            Ok(Some(hit)) => println!(
                "{:<26} routed  : source={} name={:?} attachable={}",
                "", hit.source, hit.name, hit.has_customer_id()
            ),
            Ok(None) => println!("{:<26} routed  : no match", ""),
            Err(e) => println!("{:<26} routed  : ERR {e:#}", ""),
        }
    }
}
