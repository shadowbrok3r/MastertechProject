//! End-to-end check for one scored benchmark kind against a local SurrealDB,
//! through the full production path: hardware middleware, `stress_test_run`,
//! tick condensation, and the persisted `benchmark_result` row.
//!
//! ```text
//! surreal start --user root --pass root --bind 127.0.0.1:8000 memory
//! cargo run -p stress-runner --example bench_linpack_local [kind] [duration_secs]
//! ```
//!
//! Env overrides: `BENCH_DB_URL` (default `127.0.0.1:8000`), `DB_ROOT_USER` /
//! `DB_ROOT_PASS` (default `root` / `root`).

use std::sync::Arc;

use database::DATABASE;
use stress_runner::{
    local_computer_record, parse_benchmark_kind, run_benchmark, BenchmarkKind, TelemetryAgent,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("BENCH_DB_URL").unwrap_or_else(|_| "127.0.0.1:8000".into());
    let user = std::env::var("DB_ROOT_USER").unwrap_or_else(|_| "root".into());
    let pass = std::env::var("DB_ROOT_PASS").unwrap_or_else(|_| "root".into());

    DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(url.clone()).await?;
    DATABASE
        .signin(surrealdb::opt::auth::Root { username: user, password: pass })
        .await?;
    DATABASE.use_ns(database::NS).use_db(database::DB).await?;
    println!("connected to {url} ns={} db={}", database::NS, database::DB);

    // Blank throwaway DBs lack fn::embed_text + table DDL; real DBs keep theirs.
    let probe_ok = DATABASE
        .query("RETURN fn::embed_text('probe')")
        .await
        .and_then(|r| r.check())
        .is_ok();
    if !probe_ok {
        DATABASE
            .query(include_str!("../../database/tests/fixtures/stress_schema.surql"))
            .await?
            .check()?;
        println!("applied stress_schema.surql fixture (blank database)");
    }

    stress_runner::set_runtime_handle(tokio::runtime::Handle::current());

    let kind = std::env::args()
        .nth(1)
        .and_then(|a| parse_benchmark_kind(&a))
        .unwrap_or(BenchmarkKind::Linpack);
    let duration_secs = std::env::args()
        .nth(2)
        .and_then(|a| a.parse().ok())
        .unwrap_or(stress_runner::DEFAULT_BENCH_SECS);

    let computer = local_computer_record();
    let telemetry = Arc::new(TelemetryAgent::start(1000));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    println!("running benchmark kind={} for {duration_secs}s ...", kind.as_str());
    let outcome =
        tokio::task::spawn_blocking(move || run_benchmark(kind, computer, telemetry, duration_secs))
            .await?;
    println!("outcome: {}", serde_json::to_string_pretty(&outcome)?);

    let rows: Vec<serde_json::Value> = DATABASE
        .query(
            "SELECT kind_label, score, unit, samples, threads, duration_secs, errors, notes, \
             hostname, captured_at FROM benchmark_result ORDER BY captured_at DESC",
        )
        .await?
        .take(0)?;
    println!("benchmark_result rows: {}", serde_json::to_string_pretty(&rows)?);

    anyhow::ensure!(outcome.error.is_none(), "benchmark error: {:?}", outcome.error);
    anyhow::ensure!(outcome.samples > 0, "no throughput samples collected");
    anyhow::ensure!(outcome.score > 0.0, "zero score");
    anyhow::ensure!(outcome.result_id.is_some(), "benchmark_result row not persisted");
    println!(
        "PASS: {} scored {:.2} {} from {} samples ({} threads, {:.1}s)",
        outcome.kind, outcome.score, outcome.unit, outcome.samples, outcome.threads,
        outcome.duration_secs
    );
    Ok(())
}
