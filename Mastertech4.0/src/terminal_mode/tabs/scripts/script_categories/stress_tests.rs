use std::sync::Arc;

use stress_kit::telemetry::TelemetryAgent;
use stress_runner::{build_stress_script_spec, drive_blocking, RunResult, RunUpdate};

use crate::terminal_mode::tabs::{
    checklist::Category,
    scripts::{Reporter, ScriptsTab},
};

impl<'a> ScriptsTab<'a> {
    pub fn handle_stress_tests(&mut self, item_text: &str, category: &Category) {
        self.current_reporter.replace(Reporter::StressTest);
        self.log_message(&format!("Starting Stress Tests script: {}", item_text));
        if stress_runner::is_benchmark_script(item_text) {
            self.run_benchmark_script(item_text, category);
            return;
        }
        self.run_stress_script(item_text, category);
    }

    /// Scored benchmarks ("Benchmark Suite" / "Benchmark: X"): each kind runs
    /// a fixed measurement window and persists a `benchmark_result` row plus
    /// its backing `stress_test_run`. No service number required — scores are
    /// machine-keyed, not order-keyed.
    fn run_benchmark_script(&mut self, item_text: &str, category: &Category) {
        let client = crate::filesystem::get_client_hash();
        let Some(computer) = client.computer.clone() else {
            self.log_message("get_client_hash returned no computer record; aborting benchmark");
            let _ = self
                .checklist_completion_tx
                .try_send((category.clone(), item_text.to_string(), false));
            return;
        };

        let telemetry = {
            let mut guard = self.stress_telemetry.borrow_mut();
            if guard.is_none() {
                *guard = Some(Arc::new(TelemetryAgent::start(1000)));
            }
            guard.as_ref().unwrap().clone()
        };

        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_clone = item_text.to_string();
        let name = item_text.to_string();

        std::thread::spawn(move || {
            let include_gpu = !telemetry.snapshot().gpus.is_empty();
            let secs = stress_runner::DEFAULT_BENCH_SECS;
            let _ = log_tx.try_send(format!(
                "{name}: {secs}s per benchmark, gpu kinds {}",
                if include_gpu { "included" } else { "skipped (no GPU)" }
            ));

            let Some(outcomes) =
                stress_runner::run_benchmark_script(&name, computer, telemetry, secs, include_gpu)
            else {
                let _ = log_tx.try_send(format!("Unknown benchmark script: {name}"));
                let _ = checklist_tx.try_send((category_clone, item_clone, false));
                return;
            };

            let mut success = true;
            for o in &outcomes {
                if o.errors > 0 || o.error.is_some() {
                    success = false;
                }
                let _ = log_tx.try_send(format!(
                    "{}: {:.1} {} (peak {:.1}) errors={}{}{}",
                    o.kind,
                    o.score,
                    o.unit,
                    o.peak.unwrap_or(o.score),
                    o.errors,
                    o.result_id
                        .as_deref()
                        .map(|id| format!(" [{id}]"))
                        .unwrap_or_default(),
                    o.error
                        .as_deref()
                        .map(|e| format!(" — {e}"))
                        .unwrap_or_default(),
                ));
            }
            let _ = log_tx.try_send(format!(
                "{name} complete: {} benchmark(s), {}",
                outcomes.len(),
                if success { "all clean" } else { "errors detected" }
            ));
            let _ = checklist_tx.try_send((category_clone, item_clone, success));
        });
    }

    fn run_stress_script(&mut self, item_text: &str, category: &Category) {
        if self.service_number.trim().is_empty() {
            self.log_message(&format!(
                "{item_text}: a service number is required so stress_test_run carries service_order / customer / computer info — aborting."
            ));
            let _ = self
                .checklist_completion_tx
                .try_send((category.clone(), item_text.to_string(), false));
            return;
        }

        let client = crate::filesystem::get_client_hash();
        let Some(computer) = client.computer.clone() else {
            self.log_message("get_client_hash returned no computer record; aborting stress run");
            let _ = self
                .checklist_completion_tx
                .try_send((category.clone(), item_text.to_string(), false));
            return;
        };
        let duration_secs = *self.stress_duration_secs.borrow();
        let Some(mut spec) = build_stress_script_spec(item_text, computer, duration_secs) else {
            self.log_message(&format!("Unknown Stress Tests script: {}", item_text));
            let _ = self
                .checklist_completion_tx
                .try_send((category.clone(), item_text.to_string(), false));
            return;
        };

        spec.tags.push("origin:scripts".into());
        spec.hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .ok();
        spec.machine_id = Some(client.client_hash.clone());
        spec.service_order = Some(database::schema::RecordId::new(
            database::schema::TICKET_TABLE,
            self.service_number.clone(),
        ));
        if let Some(session_id) = self
            .mcp_diagnostic_session_id
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            spec.session_ref = Some(database::schema::entity_link::parse_record_id(
                session_id,
                database::schema::DIAGNOSTIC_SESSION_TABLE,
            ));
        }

        let telemetry = {
            let mut guard = self.stress_telemetry.borrow_mut();
            if guard.is_none() {
                *guard = Some(Arc::new(TelemetryAgent::start(1000)));
            }
            guard.as_ref().unwrap().clone()
        };

        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_clone = item_text.to_string();
        let label = item_text.to_string();

        std::thread::spawn(move || {
            let mut success = false;
            drive_blocking(spec, telemetry, |update| match update {
                RunUpdate::Started { run_id } => {
                    use database::schema::RecordIdExt;
                    let _ = log_tx.try_send(format!(
                        "{label}: stress_test_run id: {}",
                        run_id.key_string()
                    ));
                }
                RunUpdate::StageStarted { index, label: stage_label, stage_count } => {
                    if stage_count > 1 {
                        let _ = log_tx.try_send(format!(
                            "{label} stage {}/{stage_count}: {stage_label}",
                            index + 1
                        ));
                    }
                }
                RunUpdate::Tick { metrics, stage_label, .. } => {
                    if let Some(err) = metrics.last_error.as_ref() {
                        let stage = stage_label.unwrap_or_else(|| "single".into());
                        let _ = log_tx.try_send(format!("{label} {stage}: {err}"));
                    }
                }
                RunUpdate::StageFinished { .. } => {}
                RunUpdate::Finished(v) => {
                    success = v.result == RunResult::Pass;
                    let result_str = match v.result {
                        RunResult::Pass => "PASSED",
                        RunResult::Fail => "FAILED",
                        RunResult::Aborted => "ABORTED",
                        RunResult::Inconclusive => "INCONCLUSIVE",
                        RunResult::InProgress => "IN_PROGRESS",
                    };
                    let _ = log_tx.try_send(format!(
                        "{label} {result_str} in {:.1}s (run persisted)",
                        v.duration_secs
                    ));
                }
                RunUpdate::Warning { message } => {
                    let _ = log_tx.try_send(format!("{label} warning: {message}"));
                }
                RunUpdate::Error { message } => {
                    let _ = log_tx.try_send(format!("{label} error: {message}"));
                }
            });
            let _ = checklist_tx.try_send((category_clone, item_clone, success));
        });
    }
}
