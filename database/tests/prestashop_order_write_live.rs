//! Live round-trip tests for the serialized order writer against a dedicated test order.
//!
//! These WRITE to live Prestashop, so they are `#[ignore]`d. Run explicitly:
//!
//! ```text
//! cargo test -p database --test prestashop_order_write_live -- --ignored --nocapture
//! ```
//!
//! Order 2111019 is a disposable test order. Every phase restores what it changed, and the
//! original field values are put back at the end.

use database::schema::prestashop::{order_write, Order, OrderState, Prestashop};

const TEST_ORDER: &str = "2111019";

/// Check-in Shelf pseudo-employee.
const CHECKIN_SHELF: &str = "1347";
const REP_A: &str = "1382";
const REP_B: &str = "183";
const SPLIT_REP: &str = "35";

async fn read_order() -> Order {
    Prestashop::default()
        .request_subresources_by_id_wasm::<Order>("orders", "order", TEST_ORDER)
        .await
        .expect("failed to read test order")
}

/// Applies fields, records any error instead of panicking, and returns the re-read order.
async fn write_fields(label: &str, fields: &[(&str, &str)], errors: &mut Vec<String>) -> Order {
    if let Err(e) = order_write::set_order_fields(TEST_ORDER, fields).await {
        errors.push(format!("{label}: {e}"));
    }
    let now = read_order().await;
    summarize(label, &now);
    now
}

fn summarize(label: &str, order: &Order) {
    eprintln!(
        "  {label:<22} state={:<4} sales_rep={:<6} split_rep={:<6} date_upd={}",
        order.current_state, order.id_employee_sales_rep, order.id_employee_split_rep, order.date_upd
    );
}

/// Walks the fields the audit table edits, then reproduces the concurrent status +
/// sales-rep burst that silently reverted the rep before writes were serialized.
#[tokio::test]
#[ignore = "writes to live Prestashop order 2111019; run with --ignored"]
async fn order_field_writes_round_trip_and_survive_concurrency() {
    let original = read_order().await;
    eprintln!("\n[baseline] order {TEST_ORDER}");
    summarize("original", &original);

    // Nothing below panics: every write's outcome is captured so the restore at the end
    // always runs, and the assertions happen once the order is back as we found it.
    let mut errors: Vec<String> = Vec::new();

    eprintln!("\n[phase 1] sales rep");
    let after_shelf = write_fields("-> Check-in Shelf", &[("id_employee_sales_rep", CHECKIN_SHELF)], &mut errors).await;
    let after_rep_a = write_fields("-> rep A", &[("id_employee_sales_rep", REP_A)], &mut errors).await;

    eprintln!("\n[phase 2] split rep");
    let after_split_set = write_fields("-> split rep set", &[("id_employee_split_rep", SPLIT_REP)], &mut errors).await;
    let after_split_zero = write_fields("-> split rep \"0\"", &[("id_employee_split_rep", "0")], &mut errors).await;

    eprintln!("\n[phase 3] status");
    let in_repair = OrderState::InRepair.to_id_str();
    let checkin = OrderState::CheckinShelf.to_id_str();
    let after_in_repair = write_fields("-> In Repair", &[("current_state", in_repair)], &mut errors).await;
    let after_checkin = write_fields("-> Check-in Shelf", &[("current_state", checkin)], &mut errors).await;

    // --- Phase 4: the regression -------------------------------------------------------
    eprintln!("\n[phase 4] concurrent status + sales rep");
    write_fields(
        "seeded",
        &[("current_state", checkin), ("id_employee_sales_rep", CHECKIN_SHELF)],
        &mut errors,
    )
    .await;

    // Both writes launched together, as the two combobox clicks were.
    let (status_result, rep_result) = tokio::join!(
        order_write::set_order_field(TEST_ORDER, "current_state", OrderState::InRepair.to_id_str()),
        order_write::set_order_field(TEST_ORDER, "id_employee_sales_rep", REP_B),
    );
    if let Err(e) = status_result {
        errors.push(format!("concurrent status write: {e}"));
    }
    if let Err(e) = rep_result {
        errors.push(format!("concurrent sales_rep write: {e}"));
    }
    let after_burst = read_order().await;
    summarize("after burst", &after_burst);

    // --- Restore -----------------------------------------------------------------------
    let restore = order_write::set_order_fields(
        TEST_ORDER,
        &[
            ("current_state", &original.current_state),
            ("id_employee_sales_rep", &original.id_employee_sales_rep),
            ("id_employee_split_rep", &original.id_employee_split_rep),
        ],
    )
    .await;
    let restored = read_order().await;
    eprintln!("\n[restore]");
    summarize("restored", &restored);
    if let Err(e) = restore {
        eprintln!("  restore reported: {e}");
    }
    eprintln!();

    assert!(errors.is_empty(), "writes reported errors:\n  {}", errors.join("\n  "));

    assert_eq!(after_shelf.id_employee_sales_rep, CHECKIN_SHELF, "sales rep did not persist");
    assert_eq!(after_rep_a.id_employee_sales_rep, REP_A, "sales rep did not persist");
    assert_eq!(after_split_set.id_employee_split_rep, SPLIT_REP, "split rep did not persist");
    assert_eq!(after_split_zero.id_employee_split_rep, "0", "split rep was not cleared");
    assert_eq!(after_in_repair.current_state, OrderState::InRepair.to_id_str(), "status did not persist");
    assert_eq!(after_checkin.current_state, OrderState::CheckinShelf.to_id_str(), "status did not persist");

    // The two fields the incident lost. Before writes were serialized, whichever PUT landed
    // last reverted the other field to its pre-burst value.
    assert_eq!(
        after_burst.current_state,
        OrderState::InRepair.to_id_str(),
        "concurrent burst lost the status write"
    );
    assert_eq!(
        after_burst.id_employee_sales_rep, REP_B,
        "concurrent burst lost the sales-rep write (the reported bug)"
    );

    assert_eq!(restored.current_state, original.current_state, "status not restored");
    assert_eq!(
        restored.id_employee_sales_rep, original.id_employee_sales_rep,
        "sales rep not restored"
    );
}

/// Puts the test order back to its resting values, for use after an interrupted run.
/// State 6 is Canceled, which is where this order normally sits.
#[tokio::test]
#[ignore = "writes to live Prestashop order 2111019; run with --ignored"]
async fn reset_test_order() {
    summarize("before", &read_order().await);
    let result = order_write::set_order_fields(
        TEST_ORDER,
        &[
            ("current_state", "6"),
            ("id_employee_sales_rep", CHECKIN_SHELF),
            ("id_employee_split_rep", "0"),
        ],
    )
    .await;
    if let Err(e) = result {
        eprintln!("  reset reported: {e}");
    }
    summarize("after", &read_order().await);
}

/// Reports which `current_state` values this order will accept, and restores the sales rep.
/// Diagnostic only — never fails, so it can be run to inspect the order's shape.
#[tokio::test]
#[ignore = "probes live Prestashop order 2111019; run with --ignored"]
async fn probe_state_transitions() {
    let before = read_order().await;
    eprintln!("\n[probe] order {TEST_ORDER}");
    eprintln!("  id_order_type={} id_store={} reference={}", before.id_order_type, before.id_store, before.reference);
    eprintln!("  invoice_number={} total_paid={}", before.invoice_number, before.total_paid);
    summarize("current", &before);

    for state in [
        OrderState::CheckinShelf,
        OrderState::InRepair,
        OrderState::DoneShelf,
    ] {
        let id = state.to_id_str();
        match order_write::set_order_field(TEST_ORDER, "current_state", id).await {
            Ok(_) => {
                let now = read_order().await;
                eprintln!("  {:<18} ({id:>3}) -> OK, state now {}", state.as_str(), now.current_state);
            }
            Err(e) => eprintln!("  {:<18} ({id:>3}) -> REJECTED: {e}", state.as_str()),
        }
    }

    // Put back whatever we found, best effort.
    let _ = order_write::set_order_fields(
        TEST_ORDER,
        &[
            ("current_state", &before.current_state),
            ("id_employee_sales_rep", &before.id_employee_sales_rep),
            ("id_employee_split_rep", &before.id_employee_split_rep),
        ],
    )
    .await;
    summarize("restored", &read_order().await);
    eprintln!();
}

/// A misspelled or absent field must fail loudly rather than PUT an unchanged order and
/// report success. Reads only — it errors before the PUT.
#[tokio::test]
#[ignore = "reads live Prestashop order 2111019; run with --ignored"]
async fn absent_field_is_rejected() {
    let before = read_order().await;
    let err = order_write::set_order_field(TEST_ORDER, "id_employee_sales_repp", REP_A)
        .await
        .expect_err("a typo'd field name must not report success");
    eprintln!("\n[absent field] rejected with: {err}");

    let after = read_order().await;
    assert_eq!(
        before.date_upd, after.date_upd,
        "a rejected write must not touch the order"
    );
}
