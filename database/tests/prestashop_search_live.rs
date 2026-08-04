//! Live read-only tests for the order/customer search paths backing `search_prestashop_orders`.
//!
//! These hit live Prestashop, so they are `#[ignore]`d. Run explicitly:
//!
//! ```text
//! cargo test -p database --test prestashop_search_live -- --ignored --nocapture
//! ```
//!
//! Order 2111019 is the same disposable test order used by `prestashop_order_write_live`.
//! Nothing here writes; the customer identity is read from that order rather than hardcoded.

use database::schema::prestashop::{Customer, Order, Prestashop};

const TEST_ORDER: &str = "2111019";

async fn read_test_order() -> Order {
    Prestashop::default()
        .request_subresources_by_id_wasm::<Order>("orders", "order", TEST_ORDER)
        .await
        .expect("failed to read test order")
}

#[tokio::test]
#[ignore = "reads live Prestashop; run with --ignored"]
async fn orders_resolve_by_customer_and_reference() {
    let api = Prestashop::default();
    let order = read_test_order().await;
    assert!(
        !order.id_customer.is_empty(),
        "test order {TEST_ORDER} has no id_customer"
    );

    let by_customer = api
        .find_orders_by_customer(&order.id_customer, 100)
        .await
        .expect("orders-by-customer lookup failed");
    assert!(
        by_customer.iter().any(|o| o.id == order.id),
        "orders for id_customer={} did not include {TEST_ORDER}",
        order.id_customer
    );

    // The reference filter is a `%[value]%` contains-match; its `%` and brackets are what
    // used to reach the server unencoded.
    let stem = &order.reference[..order.reference.len().min(5)];
    let by_reference = api
        .find_orders_by_reference(stem, 100)
        .await
        .expect("orders-by-reference lookup failed");
    assert!(
        by_reference.iter().all(|o| o.reference.contains(stem)),
        "reference search for {stem:?} returned a non-matching reference"
    );
}

#[tokio::test]
#[ignore = "reads live Prestashop; run with --ignored"]
async fn customer_resolves_by_email_and_name() {
    let api = Prestashop::default();
    let order = read_test_order().await;

    let customer = api
        .request_subresources_by_id_wasm::<Customer>("customers", "customer", &order.id_customer)
        .await
        .expect("failed to read the test order's customer");
    assert!(!customer.email.is_empty(), "test customer has no email");

    let by_email = api
        .find_customers_by_email(&customer.email)
        .await
        .expect("email lookup failed");
    assert!(
        by_email.iter().any(|c| c.id == customer.id),
        "email lookup did not return id_customer={}",
        customer.id
    );

    let full_name = format!("{} {}", customer.firstname, customer.lastname);
    let by_name = api
        .find_customers_by_name(&full_name)
        .await
        .expect("name lookup failed");
    assert!(
        by_name.iter().any(|c| c.id == customer.id),
        "name lookup for {full_name:?} did not return id_customer={}",
        customer.id
    );
}

/// A miss must be an empty vec, not one default-valued element.
#[tokio::test]
#[ignore = "reads live Prestashop; run with --ignored"]
async fn no_match_yields_no_rows() {
    let api = Prestashop::default();

    let customers = api
        .find_customers_by_email("no.such.customer.zzz@example.invalid")
        .await
        .expect("negative email lookup failed");
    assert!(
        customers.is_empty(),
        "expected no customers, got {customers:#?}"
    );

    let orders = api
        .find_orders_by_reference("ZZZNOSUCHREF", 10)
        .await
        .expect("negative reference lookup failed");
    assert!(orders.is_empty(), "expected no orders, got {}", orders.len());
}
