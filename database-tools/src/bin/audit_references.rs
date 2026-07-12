//! Tranche 0 — referential-integrity audit for the production SurrealDB.
//!
//! **Read-only.** Walks every `record<X>` foreign-key field defined in the
//! production schema and reports orphans: rows whose FK value points at a
//! record that no longer exists in the target table. Never issues
//! `UPDATE` / `DELETE` / `CREATE`.
//!
//! ## What it checks
//!
//! For each (source_table, fk_field, target_table) triple:
//!
//!   - **Total rows** in the source table.
//!   - **NULL count** — how many rows have `fk_field == NONE`. Surfaced
//!     separately for FK fields the production schema declares
//!     `record<X>` (NOT NULL). A non-zero NULL count on a NOT NULL field
//!     means either the schema constraint isn't being enforced or there
//!     is data predating the constraint.
//!   - **Orphan count** — how many rows have `fk_field` set to a value
//!     that isn't present in the target table.
//!   - **Sample of orphans** — up to `--sample-size` example source IDs
//!     for spot-checking before any cleanup script touches them.
//!
//! Array-valued FKs (`array<record<X>>`, e.g. `customer.computers`,
//! `task.jobs`) are flattened: each row contributes one orphan entry
//! per dangling element in its array.
//!
//! ## Connection target
//!
//! Uses `database::init_database()`, which connects to `DB_URL_LOCAL` in
//! debug builds and `DB_URL_DEV` in release builds — the same selection
//! the main app uses. To audit production data:
//!
//! ```text
//! cargo run --release -p database-tools --bin audit-references -- --json > audit.json
//! ```
//!
//! ## Output
//!
//! Default human-readable summary table on stdout. With `--json`, emits
//! a structured array of [`FkReport`] objects suitable for feeding into
//! a follow-up cleanup script.

use std::collections::HashSet;

use anyhow::{Context, Result};
use clap::Parser;
use database::schema::RecordIdExt;
use serde::{Deserialize, Serialize};
use surrealdb::types::{RecordId, SurrealValue};

/// Helper — surrealdb 3.1's `RecordId` doesn't implement `Display`, so
/// every place we want a canonical `table:key` string goes through this.
fn rid_str(r: &RecordId) -> String {
    format!("{}:{}", r.table, r.key_string())
}

#[derive(Debug, clap::Parser)]
#[command(version, about = "Audit referential integrity in the Mastertech SurrealDB.")]
struct Args {
    /// Emit results as JSON to stdout. Default is a human-readable table.
    #[arg(long)]
    json: bool,

    /// How many sample orphan rows to record per FK. Capped at 100 to
    /// keep JSON output reviewable.
    #[arg(long, default_value_t = 10)]
    sample_size: usize,

    /// Skip FKs whose source table has zero rows. The default already
    /// reports them with `total: 0`; this just trims the human-readable
    /// output for readability.
    #[arg(long)]
    hide_empty: bool,
}

/// Cardinality / nullability classification for one FK constraint.
/// Determines how we read the field out of SurrealDB and how we
/// interpret the null count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum Flavor {
    /// `record<X>` — single ref, NOT NULL per the production schema.
    /// A non-zero null count here is itself a finding.
    Required,
    /// `none | record<X>` — single ref, nullable. NULLs are expected.
    Optional,
    /// `array<record<X>>` — repeated ref. We flatten the array and
    /// check each element. Empty arrays count as a single "NULL".
    Array,
}

/// One FK reference to audit. The full table is hand-maintained from
/// `production-2026-05-15.surql`'s `DEFINE FIELD ... TYPE record<...>`
/// statements — see the FK inventory in the database-refactor plan.
#[derive(Debug, Clone, Copy)]
struct Fk {
    source: &'static str,
    field: &'static str,
    target: &'static str,
    flavor: Flavor,
}

/// Complete list of record-typed FKs in the production schema as of
/// 2026-05-15. Update when `DEFINE FIELD ... TYPE record<...>` statements
/// are added/removed.
const FKS: &[Fk] = &[
    Fk { source: "chat_thread",        field: "thread_users",  target: "user",                flavor: Flavor::Array    },
    Fk { source: "chat_thread",        field: "user_created",  target: "user",                flavor: Flavor::Required },
    Fk { source: "computer",           field: "customer",      target: "customer",            flavor: Flavor::Required },
    Fk { source: "connected_client",   field: "assigned_user", target: "user",                flavor: Flavor::Required },
    Fk { source: "connected_client",   field: "computer",      target: "computer",            flavor: Flavor::Optional },
    Fk { source: "connected_client",   field: "customer",      target: "customer",            flavor: Flavor::Optional },
    Fk { source: "customer",           field: "computers",     target: "computer",            flavor: Flavor::Array    },
    Fk { source: "customer",           field: "services",      target: "service_order",       flavor: Flavor::Array    },
    Fk { source: "diagnostic_entry",   field: "session_ref",   target: "diagnostic_session",  flavor: Flavor::Required },
    Fk { source: "diagnostic_session", field: "computer_id",   target: "computer",            flavor: Flavor::Optional },
    Fk { source: "diagnostic_session", field: "customer_id",   target: "customer",            flavor: Flavor::Optional },
    Fk { source: "diagnostic_session", field: "service_order", target: "service_order",       flavor: Flavor::Optional },
    Fk { source: "diagnostic_session", field: "task_ref",      target: "task",                flavor: Flavor::Optional },
    Fk { source: "job",                field: "computer",      target: "computer",            flavor: Flavor::Required },
    Fk { source: "notification",       field: "user",          target: "user",                flavor: Flavor::Required },
    Fk { source: "qc",                 field: "task",          target: "task",                flavor: Flavor::Required },
    Fk { source: "service_order",      field: "customer",      target: "customer",            flavor: Flavor::Required },
    Fk { source: "service_order",      field: "computer",      target: "computer",            flavor: Flavor::Optional },
    Fk { source: "task",               field: "assignee",      target: "user",                flavor: Flavor::Required },
    Fk { source: "task",               field: "jobs",          target: "job",                 flavor: Flavor::Array    },
    Fk { source: "task",               field: "service_ticket",target: "service_order",       flavor: Flavor::Optional },
    Fk { source: "task_history",       field: "task_id",       target: "task",                flavor: Flavor::Required },
    Fk { source: "task_history",       field: "user",          target: "user",                flavor: Flavor::Required },
    Fk { source: "task_note",          field: "task_id",       target: "task",                flavor: Flavor::Required },
    Fk { source: "task_note",          field: "user",          target: "user",                flavor: Flavor::Required },
    Fk { source: "user_message",       field: "thread_id",     target: "chat_thread",         flavor: Flavor::Required },
    Fk { source: "user_message",       field: "user",          target: "user",                flavor: Flavor::Required },
];

/// One audited FK reference. Serialized to JSON when `--json` is set.
#[derive(Debug, Serialize)]
struct FkReport {
    source_table: &'static str,
    fk_field: &'static str,
    target_table: &'static str,
    flavor: Flavor,
    total_source_rows: usize,
    /// For `Required` and `Optional`: rows where `fk_field == NONE`.
    /// For `Array`: rows where `fk_field == NONE` *or* `array::len == 0`.
    null_count: usize,
    /// Rows with at least one orphan reference (for Array flavor) or
    /// whose single FK value points at a missing target (for One).
    orphan_count: usize,
    /// Sample of orphan source IDs (up to `--sample-size`). For Array
    /// flavor each element is `source_id::fk_index::orphan_value` so we
    /// can see which slot in the array points at what.
    sample_orphans: Vec<String>,
    /// Schema-violation count: a `Required` FK with `null_count > 0` is
    /// data that contradicts the `DEFINE FIELD` constraint. For
    /// `Optional` and `Array` this is always 0.
    schema_violations: usize,
}

/// One source row with id + raw FK value, deserialized from
/// `SELECT id, <fk> AS fk FROM <table>`. `SurrealValue` is what
/// `Response::take` requires in surrealdb 3.x.
#[derive(Debug, Serialize, Deserialize, SurrealValue)]
struct SourceRowSingle {
    id: RecordId,
    fk: Option<RecordId>,
}

#[derive(Debug, Serialize, Deserialize, SurrealValue)]
struct SourceRowArray {
    id: RecordId,
    /// `None` if the field is NONE; `Some(vec)` (possibly empty) if it's an array.
    fk: Option<Vec<RecordId>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .try_init()
        .ok();

    let args = Args::parse();
    let sample_size = args.sample_size.min(100);

    database::init_database()
        .await
        .context("could not connect to database — check .env and that surreal is running")?;

    log::info!(
        "audit-references: connected to ns={} db={} — auditing {} FKs",
        database::NS,
        database::DB,
        FKS.len()
    );

    let mut reports = Vec::with_capacity(FKS.len());
    for fk in FKS {
        let started = std::time::Instant::now();
        let report = match fk.flavor {
            Flavor::Required | Flavor::Optional => audit_single(*fk, sample_size).await,
            Flavor::Array => audit_array(*fk, sample_size).await,
        };

        match report {
            Ok(r) => {
                log::info!(
                    "  {}.{} → {} : {} rows, {} null, {} orphan ({:?})",
                    fk.source,
                    fk.field,
                    fk.target,
                    r.total_source_rows,
                    r.null_count,
                    r.orphan_count,
                    started.elapsed(),
                );
                reports.push(r);
            }
            Err(e) => {
                // Don't bail — one missing/permission-denied table
                // shouldn't kill the whole audit. Surface as an empty
                // report with the error in stderr so the run still
                // completes.
                log::error!(
                    "  {}.{}: SKIPPED — {:#}",
                    fk.source, fk.field, e
                );
            }
        }
    }

    if args.json {
        serde_json::to_writer_pretty(std::io::stdout(), &reports)
            .context("write json report")?;
        println!();
    } else {
        print_human(&reports, args.hide_empty);
    }

    Ok(())
}

/// Single-ref audit (`Required` / `Optional` flavors).
///
/// Strategy: read every `id, fk` pair from the source table and the set
/// of all valid target IDs, then do the set-diff in Rust. Simple, O(n+m),
/// and trivially debuggable. SurrealDB joins/subqueries would be more
/// efficient at the wire level but harder to reason about — and we're
/// nowhere near the row counts where the wire-level cost matters.
async fn audit_single(fk: Fk, sample_size: usize) -> Result<FkReport> {
    let db = database::db();

    let select_sources = format!("SELECT id, {field} AS fk FROM {src}", field = fk.field, src = fk.source);
    let select_targets = format!("SELECT VALUE id FROM {tgt}", tgt = fk.target);

    let mut response = db
        .query(&select_sources)
        .query(&select_targets)
        .await
        .with_context(|| format!("query failed for {}.{}", fk.source, fk.field))?;

    let sources: Vec<SourceRowSingle> = response
        .take(0)
        .with_context(|| format!("could not parse `{select_sources}` response"))?;
    let targets: Vec<RecordId> = response
        .take(1)
        .with_context(|| format!("could not parse `{select_targets}` response"))?;

    // RecordId in surrealdb 3.x doesn't impl Hash/Eq cleanly across
    // every key variant, so we key by the canonical `table:key` string
    // form. This is what SurrealDB itself round-trips, so it can't
    // be wrong.
    let valid: HashSet<String> = targets.iter().map(rid_str).collect();

    let total = sources.len();
    let mut null_count = 0usize;
    let mut orphan_count = 0usize;
    let mut sample_orphans = Vec::new();

    for row in &sources {
        match &row.fk {
            None => null_count += 1,
            Some(fk_val) => {
                if !valid.contains(&rid_str(fk_val)) {
                    orphan_count += 1;
                    if sample_orphans.len() < sample_size {
                        sample_orphans.push(format!("{} → {}", rid_str(&row.id), rid_str(fk_val)));
                    }
                }
            }
        }
    }

    let schema_violations = if matches!(fk.flavor, Flavor::Required) {
        null_count
    } else {
        0
    };

    Ok(FkReport {
        source_table: fk.source,
        fk_field: fk.field,
        target_table: fk.target,
        flavor: fk.flavor,
        total_source_rows: total,
        null_count,
        orphan_count,
        sample_orphans,
        schema_violations,
    })
}

/// Array-valued FK audit. Each row's array is flattened and each element
/// checked against the target set. A row counts as orphaned if **any**
/// of its array elements is dangling. The sample captures
/// `source_id [idx] orphan_value` triples so the cleanup script knows
/// exactly which slot to null out.
async fn audit_array(fk: Fk, sample_size: usize) -> Result<FkReport> {
    let db = database::db();

    let select_sources = format!("SELECT id, {field} AS fk FROM {src}", field = fk.field, src = fk.source);
    let select_targets = format!("SELECT VALUE id FROM {tgt}", tgt = fk.target);

    let mut response = db
        .query(&select_sources)
        .query(&select_targets)
        .await
        .with_context(|| format!("query failed for {}.{}", fk.source, fk.field))?;

    let sources: Vec<SourceRowArray> = response
        .take(0)
        .with_context(|| format!("could not parse `{select_sources}` response"))?;
    let targets: Vec<RecordId> = response
        .take(1)
        .with_context(|| format!("could not parse `{select_targets}` response"))?;
    let valid: HashSet<String> = targets.iter().map(rid_str).collect();

    let total = sources.len();
    let mut null_count = 0usize;
    let mut orphan_count = 0usize;
    let mut sample_orphans = Vec::new();

    for row in &sources {
        let arr = match &row.fk {
            None => {
                null_count += 1;
                continue;
            }
            Some(v) if v.is_empty() => {
                null_count += 1;
                continue;
            }
            Some(v) => v,
        };

        let mut row_has_orphan = false;
        for (idx, elem) in arr.iter().enumerate() {
            if !valid.contains(&rid_str(elem)) {
                row_has_orphan = true;
                if sample_orphans.len() < sample_size {
                    sample_orphans.push(format!("{} [{}] → {}", rid_str(&row.id), idx, rid_str(elem)));
                }
            }
        }
        if row_has_orphan {
            orphan_count += 1;
        }
    }

    Ok(FkReport {
        source_table: fk.source,
        fk_field: fk.field,
        target_table: fk.target,
        flavor: fk.flavor,
        total_source_rows: total,
        null_count,
        orphan_count,
        sample_orphans,
        // Array flavor is always nullable in the production schema (the
        // outer field is `none | array<record<X>>`), so empty/missing
        // arrays aren't a schema violation.
        schema_violations: 0,
    })
}

/// Render the report as a human-readable table on stdout. The summary
/// at the bottom counts schema violations across all FKs so a one-line
/// "n NOT-NULL constraints with bad data" indictment is easy to spot.
fn print_human(reports: &[FkReport], hide_empty: bool) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "\n=== Referential integrity audit ===\n");
    let _ = writeln!(
        out,
        "{:<22} {:<16} → {:<22} {:<9} {:>7} {:>7} {:>7} {:>8}",
        "SOURCE", "FK FIELD", "TARGET", "FLAVOR", "TOTAL", "NULL", "ORPHAN", "VIOLATE"
    );
    let _ = writeln!(out, "{}", "─".repeat(110));

    let mut total_orphans = 0usize;
    let mut total_violations = 0usize;
    for r in reports {
        if hide_empty && r.total_source_rows == 0 {
            continue;
        }
        let flavor = match r.flavor {
            Flavor::Required => "Required",
            Flavor::Optional => "Optional",
            Flavor::Array    => "Array",
        };
        let violation_marker = if r.schema_violations > 0 { "⚠" } else { " " };
        let orphan_marker    = if r.orphan_count > 0      { "✗" } else { " " };

        let _ = writeln!(
            out,
            "{:<22} {:<16} → {:<22} {:<9} {:>7} {:>7} {:>6}{} {:>7}{}",
            r.source_table,
            r.fk_field,
            r.target_table,
            flavor,
            r.total_source_rows,
            r.null_count,
            r.orphan_count, orphan_marker,
            r.schema_violations, violation_marker,
        );

        if !r.sample_orphans.is_empty() {
            for s in &r.sample_orphans {
                let _ = writeln!(out, "    {}", s);
            }
        }

        total_orphans += r.orphan_count;
        total_violations += r.schema_violations;
    }

    let _ = writeln!(out, "{}", "─".repeat(110));
    let _ = writeln!(
        out,
        "Totals: {} FKs audited, {} orphan rows, {} schema violations (NOT NULL field == NONE)",
        reports.len(),
        total_orphans,
        total_violations
    );

    if total_violations > 0 {
        let _ = writeln!(
            out,
            "\n⚠  Schema violations indicate either drift between the production .surql and\n   the running DB, or writes that bypass the schema-defined constraint. Investigate\n   before relaxing or tightening those fields in any migration."
        );
    }
}
