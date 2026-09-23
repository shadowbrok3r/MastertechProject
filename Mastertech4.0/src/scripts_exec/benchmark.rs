//! Runs the scored benchmarks, each persisting a `benchmark_result` row.

use std::sync::Arc;
use std::time::Instant;

use displays::scripts::catalog::{CATALOG, ScriptDef};
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;
use stress_kit::telemetry::TelemetryAgent;
use stress_runner::{BenchmarkOutcome, BenchmarkStatus};

pub struct BenchmarkExecutor;

impl ScriptExecutor for BenchmarkExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        CATALOG
            .get(id)
            .is_some_and(|def| stress_runner::is_benchmark_script(&def.name))
    }

    fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle {
        let (tx, done) = crossbeam::channel::bounded(1);
        let def = def.clone();
        let ctx = ctx.clone();
        let started = Instant::now();

        std::thread::spawn(move || {
            let result = run(&def, &ctx);
            let _ = tx.send(ScriptOutcome::plain(
                def.id.clone(),
                run_token,
                result,
                started.elapsed(),
            ));
        });

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    let category = def.category();
    let name = def.name.as_str();

    let client = crate::filesystem::get_client_hash();
    let Some(computer) = client.computer.clone() else {
        let msg = "get_client_hash returned no computer record";
        ctx.log_error(category, name, msg);
        return ScriptResult::Error(msg.into());
    };

    // GPU kinds run only when wgpu finds a hardware adapter.
    let include_gpu = stress_kit::gpu_stack::check_gpu_stack().has_hardware_gpu();
    let secs = stress_runner::DEFAULT_BENCH_SECS;
    ctx.log_info(
        category.clone(),
        name,
        format!(
            "{name}: {secs}s per benchmark, gpu kinds {}",
            if include_gpu { "included" } else { "skipped (no GPU)" }
        ),
    );

    let telemetry = Arc::new(TelemetryAgent::start(1000));
    let Some(outcomes) =
        stress_runner::run_benchmark_script(name, computer, telemetry, secs, include_gpu)
    else {
        let msg = format!("Unknown benchmark script '{name}'");
        ctx.log_warning(category, name, msg.clone());
        return ScriptResult::Skipped(msg);
    };

    let mut errors = false;
    let mut no_samples: Vec<&str> = Vec::new();
    for outcome in &outcomes {
        errors |= outcome.errors > 0 || outcome.error.is_some();
        if outcome.status == BenchmarkStatus::NoSamples {
            no_samples.push(&outcome.kind);
        }
        ctx.log_info(category.clone(), name, outcome_line(outcome));
    }

    let summary = if errors {
        "errors detected".to_string()
    } else if no_samples.is_empty() {
        "all clean".to_string()
    } else {
        format!("clean, no samples from: {}", no_samples.join(", "))
    };
    let msg = format!("{name} complete: {} benchmark(s), {summary}", outcomes.len());
    if errors {
        ctx.log_error(category, name, msg.clone());
        ScriptResult::Error(msg)
    } else {
        ctx.log_success(category, name, msg.clone());
        ScriptResult::Success(msg)
    }
}

fn outcome_line(o: &BenchmarkOutcome) -> String {
    format!(
        "{}: {:.1} {} (peak {:.1}) errors={}{}{}{}",
        o.kind,
        o.score,
        o.unit,
        o.peak.unwrap_or(o.score),
        o.errors,
        if o.status == BenchmarkStatus::NoSamples {
            " \u{2014} status: no_samples (not scored)"
        } else {
            ""
        },
        o.result_id
            .as_deref()
            .map(|id| format!(" [{id}]"))
            .unwrap_or_default(),
        o.error
            .as_deref()
            .map(|e| format!(" \u{2014} {e}"))
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod benchmark_executor_tests {
    use super::*;

    #[test]
    fn claims_every_benchmark_and_nothing_else() {
        let executor = BenchmarkExecutor;
        for def in CATALOG.iter() {
            assert_eq!(
                executor.handles(&def.id),
                stress_runner::is_benchmark_script(&def.name),
                "{} is claimed wrongly",
                def.id
            );
        }
    }

    #[test]
    fn every_benchmark_name_is_in_the_catalog() {
        for name in stress_runner::BENCHMARK_SCRIPT_NAMES {
            let id = CATALOG
                .id_for_legacy_name(name)
                .unwrap_or_else(|| panic!("{name} is not in the catalog"));
            assert!(BenchmarkExecutor.handles(id), "{name} has no executor");
        }
    }
}
