//! Live check that the TUR sheet's pull reaches an order on either backend and
//! either store.
//!
//! ```text
//! cargo test -p database --test tur_pull_live -- --ignored --nocapture
//! TUR_REFS=3881,2152446 cargo test -p database --test tur_pull_live -- --ignored --nocapture
//! ```

use database::orders::tur_pull;
use database::schema::task_creation::OrderLookup;

#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn pull_reaches_orders_on_both_stores() {
    let refs: Vec<String> = std::env::var("TUR_REFS")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_else(|_| vec!["3881".into(), "3879".into()]);

    println!("compiled XBM_SHOP = {:?}", database::XBM_SHOP);

    for reference in refs {
        println!("\n=== {reference} ===");
        match tur_pull::pull_order(OrderLookup::ServiceNumber(reference.clone())).await {
            Err(e) => println!("  FAILED {e:#}"),
            Ok(pulled) => {
                let p = &pulled.payload;
                println!("  origin        {}", pulled.origin());
                println!("  order.id      {}", p.order.id);
                println!("  order_type    {:?}", p.order.order_type);
                println!("  current_state {:?}", p.order.current_state);
                println!("  reference     {:?}", p.order.reference);
                println!("  payment       {:?}", p.order.payment);
                println!(
                    "  customer      {:?} <{}> {}",
                    p.customer.name, p.customer.email, p.customer.phone_number
                );
                println!("  order_rows    {}", p.order.associations.order_rows.len());
                for row in p.order.associations.order_rows.iter().take(3) {
                    println!("      {}x {} [{}]", row.product_quantity, row.product_name, row.product_reference);
                }
                println!("  service       {}", p.order.associations.order_service.len());
                if let Some(svc) = p.order.associations.order_service.first() {
                    println!(
                        "      {} / {} / {} serial={:?}",
                        svc.device_mfg, svc.device_model, svc.device_name, svc.device_serial
                    );
                    println!("      checkin: {:?}", svc.check_in_notes);
                }
                println!(
                    "  sales_rep     {:?}",
                    p.sales_rep.as_ref().map(|e| format!("{} {} <{}>", e.firstname, e.lastname, e.email))
                );
                println!("  task_notes    {}", p.task_notes.len());
            }
        }
    }
}

/// What the API actually returns for these orders, so a blank field in the
/// sheet can be told apart from a field we failed to map.
#[tokio::test]
#[ignore = "hits live backends; run with --ignored"]
async fn raw_detail_for_a_service_order() {
    let client = database::xbm::XbmClient::from_env().for_shop("");
    if !client.configured() {
        return;
    }
    let resolved = client.resolve("3881").await.expect("3881 resolves");
    let detail = client.order_detail(&resolved.order_gid).await.expect("detail");
    println!("line_items       {}", detail.line_items.len());
    for li in detail.line_items.iter() {
        println!("   {:?} qty={} sku={:?}", li.title, li.qty, li.sku);
    }
    println!("customer         {:?}", detail.order.as_ref().and_then(|o| o.customer.clone()));
    println!("order_details    {:?}", detail.order_details);
    println!("service_details  {:?}", detail.service_details);

    let comments = client.comments(&resolved.order_gid, None, Some(50)).await;
    match comments {
        Ok(p) => {
            println!("comments         {}", p.comments.len());
            for c in p.comments.iter() {
                println!("   [{}] author={:?} staff={:?} vis={:?}", c.kind, c.author, c.author_staff_id, c.visibility);
            }
        }
        Err(e) => println!("comments ERR {e}"),
    }
}
