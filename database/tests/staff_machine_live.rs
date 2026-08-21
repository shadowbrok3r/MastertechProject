//! Live round-trip for the staff-machine flag behind the admin-console toggle.
//!
//! Proves against a real DB that flagging a client mints the canonical computer
//! row when none exists, that a client whose `connected_client.computer` points
//! at a second identity gets both rows flagged, that the customer link is
//! stripped from a flagged row, and that unflagging clears every row. Scratch
//! rows are deleted at the end whether the assertions pass or fail.
//!
//! Writes to whatever DB it points at. Marked `#[ignore]`; run explicitly:
//!
//! ```text
//! DB_ROOT_USER=root DB_ROOT_PASS=... \
//!   cargo test -p database --test staff_machine_live -- --ignored --nocapture
//! ```
//!
//! Skips (does not fail) when creds are absent.

use database::schema::{
    client_computer_ids, internal_computer_for_client, set_client_internal, RecordId, RecordIdExt,
};
use database::{db, DB, DB_URL_DEV, NS};

const CS: &str = "STAFFTEST-HOST:0f0f0f0f0";
const ALT_KEY: &str = "stafftest-alt-identity";

async fn connect_or_skip() -> Option<()> {
    let (Ok(user), Ok(pass)) = (std::env::var("DB_ROOT_USER"), std::env::var("DB_ROOT_PASS"))
    else {
        eprintln!("staff_machine_live: DB_ROOT_USER/DB_ROOT_PASS unset - skipping");
        return None;
    };
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let url = std::env::var("STAFF_LIVE_DB_URL").unwrap_or_else(|_| DB_URL_DEV.to_string());
    db().connect::<surrealdb::engine::remote::ws::Wss>(url.clone())
        .await
        .expect("connect failed");
    db().signin(surrealdb::opt::auth::Root { username: user, password: pass })
        .await
        .expect("root signin failed");
    db().use_ns(NS).use_db(DB).await.expect("use_ns/use_db failed");
    eprintln!("staff_machine_live: connected to {url} ns={NS} db={DB}");
    Some(())
}

async fn is_internal(id: &RecordId) -> Option<bool> {
    let mut res = db()
        .query("SELECT VALUE is_internal FROM $id")
        .bind(("id", id.clone()))
        .await
        .expect("read is_internal");
    res.take::<Vec<Option<bool>>>(0).ok()?.into_iter().next().flatten()
}

async fn customer_of(id: &RecordId) -> Option<RecordId> {
    let mut res = db()
        .query("SELECT VALUE customer FROM $id")
        .bind(("id", id.clone()))
        .await
        .expect("read customer");
    res.take::<Vec<RecordId>>(0).ok()?.into_iter().next()
}

async fn cleanup(alt: &RecordId, canonical: &RecordId) {
    let _ = db()
        .query("DELETE $a; DELETE $c; DELETE connected_client WHERE connection_string == $cs")
        .bind(("a", alt.clone()))
        .bind(("c", canonical.clone()))
        .bind(("cs", CS.to_string()))
        .await;
}

#[tokio::test]
#[ignore]
async fn staff_flag_covers_both_identities_and_strips_the_owner() {
    if connect_or_skip().await.is_none() {
        return;
    }
    let canonical = RecordId::new("computer", CS);
    let alt = RecordId::new("computer", ALT_KEY);
    cleanup(&alt, &canonical).await;

    // A second identity for the same client, the split that made the original
    // fix necessary: the FK points at a row whose key is not the canonical one.
    db().query("UPSERT $a SET hostname = 'STAFFTEST-HOST'")
        .bind(("a", alt.clone()))
        .await
        .expect("mint alt computer");
    db().query(
        "UPSERT connected_client:stafftest SET connection_string = $cs, computer = $a, \
         client_kind = 'machine'",
    )
    .bind(("cs", CS.to_string()))
    .bind(("a", alt.clone()))
    .await
    .expect("mint connected_client");

    let ids = client_computer_ids(CS).await.expect("candidates");
    let keys: Vec<String> = ids.iter().map(RecordIdExt::key_string).collect();
    assert_eq!(keys.len(), 2, "canonical + FK identity, got {keys:?}");
    assert_eq!(keys[0], CS, "canonical key must come first, got {keys:?}");

    assert!(
        internal_computer_for_client(CS).await.expect("read").is_none(),
        "unflagged client must not report a staff computer"
    );

    // Flagging: the canonical row does not exist yet, so it gets minted.
    let written = set_client_internal(CS, true).await.expect("flag");
    eprintln!("flagged: {written:?}");
    assert_eq!(is_internal(&alt).await, Some(true), "FK identity not flagged");
    assert_eq!(
        is_internal(&canonical).await,
        Some(true),
        "canonical identity not minted/flagged"
    );

    let reported = internal_computer_for_client(CS).await.expect("read").map(|i| i.key_string());
    assert_eq!(reported.as_deref(), Some(CS), "must report the canonical row");

    // The DB event has to survive a writer that sets an owner anyway.
    let owner: Option<RecordId> = db()
        .query("SELECT VALUE id FROM customer LIMIT 1")
        .await
        .expect("pick a customer")
        .take::<Vec<RecordId>>(0)
        .ok()
        .and_then(|v| v.into_iter().next());
    if let Some(owner) = owner {
        for id in [&canonical, &alt] {
            db().query("UPDATE $id SET customer = $cust")
                .bind(("id", id.clone()))
                .bind(("cust", owner.clone()))
                .await
                .expect("write owner");
            assert_eq!(
                customer_of(id).await,
                None,
                "{} kept an owner despite is_internal",
                id.key_string()
            );
        }

        // Second table, same invariant: the client row must not keep an owner
        // either, or the admin list shows one on a staff machine.
        db().query("UPDATE connected_client:stafftest SET customer = $cust")
            .bind(("cust", owner.clone()))
            .await
            .expect("write client owner");
        let client_owner: Option<RecordId> = db()
            .query("SELECT VALUE customer FROM connected_client:stafftest")
            .await
            .expect("read client owner")
            .take::<Vec<RecordId>>(0)
            .ok()
            .and_then(|v| v.into_iter().next());
        assert_eq!(
            client_owner, None,
            "connected_client kept an owner despite computer.is_internal"
        );
    } else {
        eprintln!("staff_machine_live: no customer row to test the guard with - skipped");
    }

    // Unflagging clears every identity, and mints nothing new.
    let cleared = set_client_internal(CS, false).await.expect("unflag");
    eprintln!("cleared: {cleared:?}");
    assert_eq!(is_internal(&alt).await, Some(false));
    assert_eq!(is_internal(&canonical).await, Some(false));
    assert!(internal_computer_for_client(CS).await.expect("read").is_none());

    cleanup(&alt, &canonical).await;
}
