//! Live round-trip for the dashboard's outcome-override write path.
//!
//! Proves the whole loop against a real DB: the field ASSERT accepts the three
//! verdicts and rejects anything else, a set override outranks inference in the
//! rollup, `excluded` leaves the buckets but stays listed, and clearing restores
//! the inferred verdict. Every assertion runs after the session is restored, so
//! a failure never leaves an override behind.
//!
//! Writes to whatever DB it points at. Marked `#[ignore]`; run explicitly:
//!
//! ```text
//! DB_ROOT_USER=root DB_ROOT_PASS=... \
//!   cargo test -p database --test outcome_override_live -- --ignored --nocapture
//! ```
//!
//! Skips (does not fail) when creds or a usable session are absent.

use database::schema::{
    set_computer_internal, set_outcome_override, OutcomeOverride, OutcomeRollup, RecordId,
    SessionOutcomeRow,
};
use database::{db, DB, DB_URL_DEV, NS};

const WINDOW: u32 = 30;

async fn connect_or_skip() -> Option<()> {
    let (Ok(user), Ok(pass)) = (std::env::var("DB_ROOT_USER"), std::env::var("DB_ROOT_PASS"))
    else {
        eprintln!("override_live: DB_ROOT_USER/DB_ROOT_PASS unset - skipping");
        return None;
    };
    // Both rustls backends get enabled by feature unification, so pick one.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let url = std::env::var("OVERRIDE_LIVE_DB_URL").unwrap_or_else(|_| DB_URL_DEV.to_string());
    db().connect::<surrealdb::engine::remote::ws::Wss>(url.clone())
        .await
        .expect("connect failed");
    db().signin(surrealdb::opt::auth::Root { username: user, password: pass })
        .await
        .expect("root signin failed");
    db().use_ns(NS).use_db(DB).await.expect("use_ns/use_db failed");
    eprintln!("override_live: connected to {url} ns={NS} db={DB}");
    Some(())
}

/// A scored session carrying no override, so the test starts from inference.
async fn target_session() -> Option<RecordId> {
    if let Ok(id) = std::env::var("OVERRIDE_LIVE_SESSION") {
        return database::schema::record_id_from_string(
            database::schema::DIAGNOSTIC_SESSION_TABLE,
            &id,
        );
    }
    let mut res = db()
        .query(
            "SELECT VALUE id FROM diagnostic_session \
             WHERE status IN ['resolved', 'escalated'] AND outcome_override = NONE \
             AND computer_id != NONE AND ended_at != NONE LIMIT 1",
        )
        .await
        .ok()?;
    res.take::<Vec<RecordId>>(0).ok()?.into_iter().next()
}

fn row_for<'a>(rows: &'a [SessionOutcomeRow], id: &str) -> Option<&'a SessionOutcomeRow> {
    rows.iter().find(|r| r.session_id.contains(id))
}

fn verdict(row: &SessionOutcomeRow) -> &str {
    row.per_window
        .iter()
        .find(|(w, _)| *w == WINDOW)
        .map(|(_, v)| v.as_str())
        .unwrap_or("missing")
}

async fn rollup() -> OutcomeRollup {
    OutcomeRollup::compute(&[WINDOW], true).await.expect("rollup failed")
}

#[tokio::test]
#[ignore = "writes to a live DB; run with --ignored"]
async fn override_outranks_inference_and_clears_cleanly() {
    let Some(()) = connect_or_skip().await else { return };
    let Some(id) = target_session().await else {
        eprintln!("override_live: no unoverridden scored session - skipping");
        return;
    };
    let key = database::schema::RecordIdExt::key_string(&id);
    eprintln!("override_live: target {key}");

    // Findings are collected, not asserted, so the restore below always runs.
    let mut findings: Vec<(&str, String)> = Vec::new();
    let mut check = |name: &'static str, ok: bool, detail: String| {
        if !ok {
            findings.push((name, detail));
        }
    };

    let before = rollup().await;
    let baseline = row_for(before.sessions.as_deref().unwrap_or_default(), &key)
        .map(|r| verdict(r).to_string())
        .unwrap_or_else(|| "absent".into());
    eprintln!("override_live: inferred verdict {baseline}");

    // A verdict inference would not reach on its own.
    let forced =
        if baseline == "comeback" { OutcomeOverride::ConfirmedFixed } else { OutcomeOverride::Comeback };
    let set = set_outcome_override(&id, Some(forced), Some("live test of the override path"), "test@pclaptops.com").await;
    check("set", set.is_ok(), format!("{set:?}"));

    let after = rollup().await;
    match row_for(after.sessions.as_deref().unwrap_or_default(), &key) {
        Some(row) => {
            check(
                "forced verdict",
                verdict(row) == forced.as_str(),
                format!("{} != {}", verdict(row), forced.as_str()),
            );
            check("override echoed", row.outcome_override == Some(forced), format!("{:?}", row.outcome_override));
            check(
                "author recorded",
                row.override_by.as_deref() == Some("test@pclaptops.com"),
                format!("{:?}", row.override_by),
            );
        },
        None => check("row present", false, "session missing from rollup".into()),
    }
    let bucket = after.overall.iter().find(|b| b.window_days == WINDOW).cloned().unwrap_or_default();
    check("counted as human", bucket.overridden >= 1, format!("overridden={}", bucket.overridden));

    // Excluded rows leave every bucket but stay listed so they can be restored.
    let set = set_outcome_override(&id, Some(OutcomeOverride::Excluded), Some("live test"), "test@pclaptops.com").await;
    check("set excluded", set.is_ok(), format!("{set:?}"));
    let excl = rollup().await;
    check(
        "dropped from buckets",
        row_for(excl.sessions.as_deref().unwrap_or_default(), &key).is_none(),
        "still scored".into(),
    );
    check(
        "listed as excluded",
        row_for(excl.excluded_sessions.as_deref().unwrap_or_default(), &key).is_some(),
        "not in excluded_sessions".into(),
    );
    check("excluded counted", excl.excluded_override >= 1, format!("{}", excl.excluded_override));

    // The DB is the last line of defense against a bad verdict string.
    let mut bad = db()
        .query("UPDATE $id SET outcome_override = 'definitely_fixed' RETURN VALUE id")
        .bind(("id", id.clone()))
        .await
        .expect("query dispatch failed");
    let rejected = bad.take::<Vec<RecordId>>(0).is_err();
    check("assert rejects junk", rejected, "a bogus verdict was accepted".into());

    // Restore before asserting anything.
    let cleared = set_outcome_override(&id, None, None, "").await;
    let restored = rollup().await;
    let back = row_for(restored.sessions.as_deref().unwrap_or_default(), &key)
        .map(|r| (verdict(r).to_string(), r.outcome_override));

    check("clear", cleared.is_ok(), format!("{cleared:?}"));
    match back {
        Some((v, ov)) => {
            check("verdict restored", v == baseline, format!("{v} != {baseline}"));
            check("override gone", ov.is_none(), format!("{ov:?}"));
        },
        None => check("row restored", false, "session missing after clear".into()),
    }

    // A no-op UPDATE must not read as success.
    let missing = set_computer_internal(
        &RecordId::new(database::schema::COMPUTER_TABLE, "no-such-machine:000000000"),
        true,
    )
    .await;
    check("no-op surfaces", missing.is_err(), "a missing record reported success".into());

    assert!(findings.is_empty(), "override live test failures: {findings:?}");
    eprintln!("override_live: all checks passed; {key} restored to {baseline}");
}
