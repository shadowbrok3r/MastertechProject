//! Live check for `link_connected_client_record` against a connected client
//! whose `computer` row is missing.
//!
//! Regression target: `cpu`/`gpu`/`ram`/`drives` had no schema DEFAULT, so the
//! upsert could not satisfy them and the tool could never mint the row.
//! Rollout `20260804120000__computer_spec_field_defaults` added the defaults;
//! `ComputerData` gained `#[surreal(default)]` so the spec-less row it creates
//! still deserializes.
//!
//! Writes to whatever DB it points at. Marked `#[ignore]`; run explicitly:
//!
//! ```text
//! DB_ROOT_USER=root DB_ROOT_PASS=... LINK_LIVE_CS=HOST:hash9 LINK_LIVE_CUSTOMER=customer:2 \
//!   cargo test -p database --test link_connected_client_live -- --ignored --nocapture
//! ```
//!
//! Skips (does not fail) when creds or the target client are not configured.

use database::schema::entity_link::{
    link_connected_client_record, validate_link_bundle, LinkBundle,
};
use database::{db, DB, DB_URL_DEV, NS};

async fn connect_or_skip() -> Option<()> {
    let (Ok(user), Ok(pass)) = (std::env::var("DB_ROOT_USER"), std::env::var("DB_ROOT_PASS"))
    else {
        eprintln!("link_live: DB_ROOT_USER/DB_ROOT_PASS unset — skipping");
        return None;
    };
    let url = std::env::var("LINK_LIVE_DB_URL").unwrap_or_else(|_| DB_URL_DEV.to_string());
    db().connect::<surrealdb::engine::remote::ws::Wss>(url.clone())
        .await
        .expect("connect failed");
    db().signin(surrealdb::opt::auth::Root {
        username: user,
        password: pass,
    })
    .await
    .expect("root signin failed");
    db().use_ns(NS)
        .use_db(DB)
        .await
        .expect("use_ns/use_db failed");
    eprintln!("link_live: connected to {url} ns={NS} db={DB}");
    Some(())
}

#[tokio::test]
#[ignore = "writes to a live DB; run with --ignored"]
async fn links_client_and_mints_spec_less_computer() {
    let Some(()) = connect_or_skip().await else {
        return;
    };
    let Ok(cs) = std::env::var("LINK_LIVE_CS") else {
        eprintln!("link_live: LINK_LIVE_CS unset — skipping");
        return;
    };
    let customer = std::env::var("LINK_LIVE_CUSTOMER").unwrap_or_else(|_| "customer:2".into());

    let report = link_connected_client_record(&cs, &customer, None)
        .await
        .expect("link_connected_client_record must succeed");
    eprintln!("link_live: report = {report}");
    assert_eq!(report["linked"], serde_json::json!(true));

    // The tool's own claim: the computer resolves and carries the customer,
    // with specs left blank for the client's check-in to fill.
    let mut res = db()
        .query(
            "SELECT customer, computer.id AS computer_deref, \
             computer.customer AS computer_customer, computer.hostname AS hostname, \
             computer.cpu AS cpu, computer.gpu AS gpu, computer.ram AS ram, \
             computer.drives AS drives \
             FROM connected_client WHERE connection_string == $cs",
        )
        .bind(("cs", cs.clone()))
        .await
        .expect("verify query failed");
    let rows: Vec<serde_json::Value> = res.take(0).expect("verify take failed");
    let row = rows.first().expect("connected_client row absent");
    eprintln!("link_live: {row}");

    assert!(
        !row["customer"].is_null(),
        "connected_client.customer still null"
    );
    assert!(
        !row["computer_deref"].is_null(),
        "connected_client.computer still dangles"
    );
    assert!(
        !row["computer_customer"].is_null(),
        "computer.customer not set"
    );
    for spec in ["cpu", "gpu", "ram"] {
        assert!(
            row[spec].is_string(),
            "{spec} should be a string, got {}",
            row[spec]
        );
    }
    assert!(row["drives"].is_array(), "drives should be an array");

    // The spec-less row must read back through the typed validator, or
    // create_diagnostic_session still rejects the client as MissingComputer.
    let validation = validate_link_bundle(&LinkBundle {
        connection_string: Some(cs.clone()),
        customer_id: None,
        computer_id: None,
    })
    .await;
    eprintln!("link_live: validation = {validation:?}");
    assert!(
        validation.ok,
        "links must validate, got issues {:?}",
        validation.issues
    );
}
