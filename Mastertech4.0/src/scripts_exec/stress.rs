//! Stress and cert runs, driven through `stress-runner`.
//!
//! One executor covers the whole family: it asks the catalog whether an id is a
//! stress script rather than registering an object per entry.
//!
//! Two things this gains over the path it replaces. Stop now reaches the run —
//! it goes to `drive_blocking_cancellable`, so the load actually drops and the
//! verdict and `stress_test_run` row still land, instead of the thread being
//! abandoned while it keeps hammering the machine. And the run id is taken from
//! the controller's own `Started` update rather than scraped back out of the log.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use displays::scripts::ScriptCategory;
use displays::scripts::catalog::{CATALOG, ScriptDef, Surface};
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;
use stress_kit::telemetry::TelemetryAgent;
use stress_runner::{RunResult, RunUpdate, build_stress_script_spec, drive_blocking_cancellable};

pub struct StressExecutor;

impl ScriptExecutor for StressExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        CATALOG.get(id).is_some_and(|def| {
            def.category() == ScriptCategory::StressTests
                && stress_runner::is_stress_script(&def.name)
        })
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
        let flag = cancel.as_flag();
        let started = Instant::now();

        std::thread::spawn(move || {
            let (result, run_id) = run(&def, &ctx, flag);
            let mut outcome =
                ScriptOutcome::plain(def.id.clone(), run_token, result, started.elapsed());
            outcome.run_id = run_id;
            let _ = tx.send(outcome);
        });

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

fn run(
    def: &ScriptDef,
    ctx: &ScriptContext,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> (ScriptResult, Option<String>) {
    let category = def.category();
    let name = def.name.clone();
    ctx.log_info(
        category.clone(),
        &name,
        format!("{name}: running via stress-runner (persisted)"),
    );

    let client = crate::filesystem::get_client_hash();
    let Some(computer) = client.computer.clone() else {
        let msg = "get_client_hash returned no computer record";
        ctx.log_error(category, &name, msg);
        return (ScriptResult::Error(msg.into()), None);
    };

    const DURATION_SECS: u64 = 60;
    let Some(mut spec) = build_stress_script_spec(&name, computer, DURATION_SECS) else {
        let msg = format!("Unknown stress script '{name}'");
        ctx.log_warning(category, &name, msg.clone());
        return (ScriptResult::Skipped(msg), None);
    };

    let origin = if ctx.surface == Some(Surface::Remote) {
        "origin:remote_scripts"
    } else {
        "origin:scripts"
    };
    spec.tags.push(origin.into());
    spec.hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok();
    spec.machine_id = Some(client.client_hash.clone());
    if let Some(service_number) = ctx.service_number.as_ref().filter(|s| !s.is_empty()) {
        spec.service_order = Some(database::schema::RecordId::new(
            database::schema::TICKET_TABLE,
            service_number.clone(),
        ));
    }
    if let Some(session) = ctx.diagnostic_session_id.as_ref().filter(|s| !s.is_empty()) {
        spec.session_ref = Some(database::schema::entity_link::parse_record_id(
            session,
            database::schema::DIAGNOSTIC_SESSION_TABLE,
        ));
    }

    let telemetry = Arc::new(TelemetryAgent::start(1000));
    let captured_run_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let run_id_sink = captured_run_id.clone();

    let verdict = drive_blocking_cancellable(spec, telemetry, cancel, |update| match update {
        RunUpdate::Started { run_id } => {
            use database::schema::RecordIdExt;
            let key = run_id.key_string();
            if let Ok(mut slot) = run_id_sink.lock() {
                *slot = Some(key.clone());
            }
            ctx.log_info(
                category.clone(),
                &name,
                format!("stress_test_run id: {key}"),
            );
        }
        RunUpdate::StageStarted {
            index,
            label,
            stage_count,
        } => {
            if stage_count > 1 {
                ctx.log_info(
                    category.clone(),
                    &name,
                    format!("Stage {}/{stage_count}: {label}", index + 1),
                );
            }
        }
        RunUpdate::Tick {
            metrics,
            stage_label,
            ..
        } => {
            if let Some(err) = metrics.last_error.as_ref() {
                let stage = stage_label.unwrap_or_else(|| "single".into());
                ctx.log_warning(category.clone(), &name, format!("{stage}: {err}"));
            }
        }
        RunUpdate::StageFinished { .. } => {}
        RunUpdate::StageVerdict {
            index,
            label,
            pass,
            violations,
            unevaluated,
            ..
        } => {
            let line = format!(
                "{name} stage {} '{label}': {}",
                index + 1,
                stress_runner::stage_verdict_token(pass, &unevaluated)
            );
            if pass {
                ctx.log_info(category.clone(), &name, line);
            } else {
                ctx.log_warning(category.clone(), &name, line);
            }
            for violation in violations {
                ctx.log_warning(
                    category.clone(),
                    &name,
                    format!("{name} stage {} violation: {violation}", index + 1),
                );
            }
            for gap in unevaluated {
                ctx.log_warning(
                    category.clone(),
                    &name,
                    format!("{name} stage {} ungraded: {gap}", index + 1),
                );
            }
        }
        RunUpdate::Finished(_) => {}
        RunUpdate::Warning { message } => {
            ctx.log_warning(
                category.clone(),
                &name,
                format!("{name} warning: {message}"),
            );
        }
        RunUpdate::Error { message } => {
            ctx.log_error(category.clone(), &name, format!("{name} error: {message}"));
        }
    });

    let run_id = captured_run_id.lock().ok().and_then(|slot| slot.clone());

    let Some(v) = verdict else {
        let msg = format!("{name}: stress-runner exited without a verdict");
        ctx.log_error(category, &name, msg.clone());
        return (ScriptResult::Error(msg), run_id);
    };

    let token = match v.result {
        RunResult::Pass => "PASSED",
        RunResult::Fail => "FAILED",
        RunResult::Aborted => "ABORTED",
        RunResult::Inconclusive => "INCONCLUSIVE",
        RunResult::InProgress => "IN_PROGRESS",
    };
    let outcome = format!("{name} {token} in {:.1}s (run persisted)", v.duration_secs);

    let result = match v.result {
        RunResult::Pass => {
            ctx.log_success(category, &name, outcome.clone());
            ScriptResult::Success(outcome)
        }
        // A cancelled run aborts, which is the operator's doing rather than a fault.
        RunResult::Aborted => {
            ctx.log_warning(category, &name, outcome.clone());
            ScriptResult::Warning(outcome)
        }
        _ => {
            ctx.log_error(category, &name, outcome.clone());
            ScriptResult::Error(outcome)
        }
    };
    (result, run_id)
}

#[cfg(test)]
mod stress_executor_tests {
    use super::*;
    use displays::scripts::catalog::Surface;

    /// Every stress script the tab offers must have an executor, or queueing it
    /// silently does nothing - which is exactly the bug the benchmarks had.
    #[test]
    fn every_runnable_stress_script_is_claimed() {
        let executor = StressExecutor;
        for name in stress_runner::STRESS_SCRIPT_NAMES {
            let id = CATALOG
                .id_for_legacy_name(name)
                .unwrap_or_else(|| panic!("{name} is not in the catalog"));
            assert!(executor.handles(id), "{name} has no executor");
        }
    }

    /// The scored benchmarks cannot be built by build_stress_script_spec, so the
    /// executor must not claim them and the tab must not offer them.
    #[test]
    fn benchmarks_are_neither_claimed_nor_offered() {
        let executor = StressExecutor;
        for name in stress_runner::BENCHMARK_SCRIPT_NAMES {
            let id = CATALOG
                .id_for_legacy_name(name)
                .unwrap_or_else(|| panic!("{name} is not in the catalog"));
            assert!(!executor.handles(id), "{name} would queue and do nothing");
            let def = CATALOG.get(id).expect("def");
            assert!(
                !def.offered_on(Surface::Egui),
                "{name} is offered in the tab"
            );
        }
    }

    #[test]
    fn nothing_outside_the_family_is_claimed() {
        let executor = StressExecutor;
        assert!(!executor.handles(&ScriptId::new("activate-cps")));
        assert!(!executor.handles(&ScriptId::new("windows-version")));
    }
}
