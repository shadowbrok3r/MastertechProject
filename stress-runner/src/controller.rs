//! Sync `RunController` that drives stress-kit + telemetry + DB writes.
//!
//! Lifecycle:
//! 1. `start` builds a `StressTestRun` in the InProgress state, persists it
//!    via the shared tokio runtime, and emits `RunUpdate::Started`.
//! 2. A worker thread wraps a `StressSession` (single-stressor) or a
//!    `ScenarioRunner` (multi-stage), samples telemetry at the run cadence,
//!    builds `StressTestMetric` rows, and forwards UI updates.
//! 3. Scenario events translate to `StressTestEvent` rows.
//! 4. On finish/cancel/error the worker rolls up `RunSummary`, calls
//!    `StressTestRun::finalize`, and emits `RunUpdate::Finished`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam::channel::{bounded, Receiver, Sender, TrySendError};

use database::schema::{
    random_record_id, BiosSettings, DriverVersions, FailureMode,
    FinishReason as DbFinishReason, RecordId, RunResult, RunSummary, ScenarioStageSummary,
    StressTestEvent as DbStressTestEvent, StressTestMetric, StressTestRun, TargetKind, TestTool,
    EventKind as DbEventKind, STRESS_TEST_EVENT_TABLE,
};
use stress_kit::{
    scenario::{
        FinishReason as SkFinishReason, ScenarioDefinition, ScenarioEvent, ScenarioRunner,
        ScenarioStage,
    },
    telemetry::{TelemetryAgent, TelemetrySnapshot},
    Metrics, StressConfig, StressSession, Stressor,
};

use crate::mapping::{default_target_kind, metric_from_snapshot};
use crate::rules::{evaluate_stage, RuleViolation, StageStats, StageVerdict, VerdictRules};
use crate::runtime;

/// Tick interval for telemetry sampling and `StressTestMetric` row creation.
/// Matches the cadence the schema is sized for (~1 Hz, so a 1-hour run writes
/// ~3,600 rows).
const TICK_INTERVAL: Duration = Duration::from_millis(1_000);

/// Flush at most this many metric rows in a single DB batch.  Keeps WS frame
/// sizes reasonable and limits the worst-case spawn-burst when the queue
/// drains after a UI pause.
const METRIC_BATCH_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Public spec types
// ---------------------------------------------------------------------------

/// One stage of a scenario run.
#[derive(Debug, Clone)]
pub struct RunStage {
    pub label: String,
    pub stressor: Stressor,
    pub threads: usize,
    pub duration_secs: u64,
    pub memory_cap_mb: u64,
    pub disk_file_mb: u64,
}

/// What to actually execute.  `Single` uses stress-kit's `StressSession`;
/// `Scenario` uses `ScenarioRunner`.
#[derive(Debug, Clone)]
pub enum RunPlan {
    Single {
        stressor: Stressor,
        threads: usize,
        /// `None` = no timeout (run until the operator stops it).
        duration_secs: Option<u64>,
        memory_cap_mb: u64,
        disk_file_mb: u64,
    },
    Scenario {
        stages: Vec<RunStage>,
        total_wall_secs: Option<u64>,
        repeat_until_total: bool,
    },
    /// Run every lane at the same time, each as its own `StressSession`.
    /// `threads == 0` lanes are budgeted across the core pool at launch.
    Concurrent {
        lanes: Vec<RunStage>,
        /// `None` = run until the operator stops it.
        duration_secs: Option<u64>,
    },
}

/// Declarative description of one stress run.  Identifying fields
/// (`computer`, `tool`, `target_kind`) are required; everything else is
/// optional and defaults to None / empty.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub computer: RecordId,
    pub tool: TestTool,
    pub target_kind: TargetKind,
    pub target_component: Option<RecordId>,
    pub touched_components: Vec<RecordId>,
    pub service_order: Option<RecordId>,
    pub session_ref: Option<RecordId>,
    pub task_ref: Option<RecordId>,
    pub tech: Option<String>,
    pub hostname: Option<String>,
    pub machine_id: Option<String>,
    pub bios_settings: BiosSettings,
    pub driver_versions: DriverVersions,
    pub notes: Option<String>,
    pub preset_label: Option<String>,
    pub tags: Vec<String>,
    pub plan: RunPlan,
    /// Per-stage pass/fail policy; `None` keeps the legacy whole-run verdict.
    pub rules: Option<VerdictRules>,
}

impl RunSpec {
    /// Smallest-shape constructor: just enough to start a run for an ad-hoc
    /// stress-kit stressor against a known computer.  Caller can patch other
    /// fields after.
    pub fn single_stresskit(
        computer: RecordId,
        stressor: Stressor,
        duration_secs: Option<u64>,
    ) -> Self {
        let target_kind = default_target_kind(stressor);
        let tool = TestTool::StressKit {
            stressor: crate::mapping::stressor_to_db(stressor),
        };
        Self {
            computer,
            tool,
            target_kind,
            target_component: None,
            touched_components: Vec::new(),
            service_order: None,
            session_ref: None,
            task_ref: None,
            tech: None,
            hostname: None,
            machine_id: None,
            bios_settings: BiosSettings::default(),
            driver_versions: DriverVersions::default(),
            notes: None,
            preset_label: None,
            tags: Vec::new(),
            plan: RunPlan::Single {
                stressor,
                threads: 0,
                duration_secs,
                memory_cap_mb: 256,
                disk_file_mb: 16,
            },
            rules: None,
        }
    }
}

// ---------------------------------------------------------------------------
// UI updates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum RunUpdate {
    /// Emitted immediately after the `StressTestRun` row is persisted.
    /// Forwarded once at the start of every run.
    Started { run_id: RecordId },
    /// Scenario-only.  Single-stressor runs emit one of these with
    /// `index = 0, stage_count = 1` so UIs can render uniformly.
    StageStarted {
        index: usize,
        label: String,
        stage_count: usize,
    },
    /// Per-tick update.  `telemetry` is fresh; `metrics` is the latest
    /// stress-kit reading (may be a repeat of a prior tick's value when the
    /// stressor is between bursts).
    Tick {
        stage_index: Option<u32>,
        stage_label: Option<String>,
        metrics: Metrics,
        telemetry: TelemetrySnapshot,
        throughput_unit: &'static str,
    },
    StageFinished {
        index: usize,
    },
    /// Rules-evaluated stage outcome; emitted right after `StageFinished`
    /// when the run carries `VerdictRules`.
    StageVerdict {
        index: usize,
        label: String,
        pass: bool,
        violations: Vec<String>,
        peak_throughput: Option<f64>,
    },
    /// Final update.  After this, `is_running` returns false.
    Finished(RunVerdict),
    /// Non-fatal warning — surface in the UI but the run continues.
    Warning { message: String },
    /// Fatal error — followed by a `Finished` update with `RunResult::Aborted`.
    Error { message: String },
}

/// One finished stage: persisted summary plus its rules verdict (if any).
#[derive(Debug, Clone)]
pub struct StageOutcome {
    pub summary: ScenarioStageSummary,
    pub verdict: Option<StageVerdict>,
}

#[derive(Debug, Clone)]
pub struct RunVerdict {
    pub run_id: RecordId,
    pub result: RunResult,
    pub finish_reason: DbFinishReason,
    pub failure_mode: FailureMode,
    pub summary: RunSummary,
    pub duration_secs: f64,
    pub stage_outcomes: Vec<StageOutcome>,
}

// ---------------------------------------------------------------------------
// Controller
// ---------------------------------------------------------------------------

pub struct RunController {
    cancel: Arc<AtomicBool>,
    update_rx: Receiver<RunUpdate>,
    run_id: Arc<Mutex<Option<RecordId>>>,
    running: Arc<AtomicBool>,
    /// Held only so the worker thread isn't reaped before it finishes its
    /// finalize call.  Joined on drop.
    join: Option<JoinHandle<()>>,
}

impl RunController {
    /// Start a run.  Spawns a worker thread that drives stress-kit, samples
    /// the supplied telemetry agent, and persists rows.  Returns immediately;
    /// drive [`RunController::poll`] from the host UI loop.
    pub fn start(spec: RunSpec, telemetry: Arc<TelemetryAgent>) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let run_id = Arc::new(Mutex::new(None));
        // Bounded channel — if the UI stops polling we'd rather drop ticks
        // than balloon memory.  The worker logs dropped updates.
        let (update_tx, update_rx) = bounded::<RunUpdate>(256);

        let cancel_worker = cancel.clone();
        let running_worker = running.clone();
        let run_id_worker = run_id.clone();
        // Channel + running flag also referenced from the panic handler so a
        // worker crash still produces a `RunUpdate::Error` instead of a dead
        // thread with `running=true` forever (the failure mode that left
        // sessions with zero stress_test_run / hardware_component records).
        let update_tx_panic = update_tx.clone();
        let running_panic = running_worker.clone();
        let run_id_panic = run_id.clone();

        let join = thread::Builder::new()
            .name("stress-runner-controller".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(
                        spec,
                        telemetry,
                        cancel_worker,
                        update_tx,
                        run_id_worker,
                        running_worker,
                    );
                }));
                if let Err(payload) = result {
                    let msg = panic_payload_str(&payload);
                    log::error!("[stress-runner/worker] worker thread panicked: {msg}");
                    // Finalize a panicked run as aborted so it isn't left in_progress.
                    if let Some(run_id) = run_id_panic.lock().ok().and_then(|g| g.clone()) {
                        let fin_msg = format!("worker thread panicked: {msg}");
                        let _ = runtime::block_on(async move {
                            StressTestRun::finalize(
                                &run_id,
                                RunResult::Aborted,
                                DbFinishReason::Crashed,
                                FailureMode::AppError { exit_code: None, message: fin_msg },
                                RunSummary::default(),
                                Vec::new(),
                                None,
                            )
                            .await
                        });
                    }
                    let _ = update_tx_panic.try_send(RunUpdate::Error {
                        message: format!("worker thread panicked: {msg}"),
                    });
                    running_panic.store(false, Ordering::SeqCst);
                }
            })
            .expect("stress-runner: failed to spawn controller thread");

        Self {
            cancel,
            update_rx,
            run_id,
            running,
            join: Some(join),
        }
    }

    /// Drain all pending UI updates.  Call once per frame.
    pub fn poll(&self) -> Vec<RunUpdate> {
        let mut out = Vec::new();
        while let Ok(u) = self.update_rx.try_recv() {
            out.push(u);
        }
        out
    }

    /// Signal cancel.  The worker will roll up an `Aborted` verdict and emit
    /// a final `Finished` update before exiting.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Available once the `Started` update has fired (i.e. the row has been
    /// successfully created in the database).
    pub fn run_id(&self) -> Option<RecordId> {
        self.run_id.lock().ok().and_then(|g| g.clone())
    }
}

impl Drop for RunController {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

fn worker(
    mut spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    cancel: Arc<AtomicBool>,
    update_tx: Sender<RunUpdate>,
    run_id: Arc<Mutex<Option<RecordId>>>,
    running: Arc<AtomicBool>,
) {
    let started_at = Instant::now();
    // Drop guard bumps the process-wide STRESS_ACTIVE counter. Reset by
    // the guard's Drop, so it works even if the worker panics — the
    // `catch_unwind` in `RunController::start` still unwinds locals.
    let _stress_active_guard = crate::StressActiveGuard::new();
    log::info!(
        "[stress-runner/worker] start: computer={:?} target_kind={:?} plan={:?}",
        spec.computer, spec.target_kind, std::mem::discriminant(&spec.plan)
    );

    // ---- 0. Hardware-component middleware ----
    // Upsert `hardware_component` rows for the CPU + every GPU this machine
    // reports, then patch the spec so the run row links them via
    // `target_component` / `touched_components`.
    //
    // **Required**, not best-effort: every stress_test_run must reference
    // at least one hardware_component row, otherwise downstream consumers
    // (compare_to_baseline, hardware_test_baseline view, the QC UI's
    // "history for this hardware" panel) have no way to interpret the
    // results. If the middleware can't link anything, refuse to start.
    let (cpu_component, all_components, hw_notices) =
        crate::hardware::ensure_components_for_run(&telemetry);
    if spec.target_component.is_none() {
        spec.target_component = cpu_component;
    }
    if spec.touched_components.is_empty() {
        spec.touched_components = all_components;
    }
    for notice in hw_notices {
        send(
            &update_tx,
            RunUpdate::Warning {
                message: format!("hardware_component: {notice}"),
            },
        );
    }

    // Driver-stack integrity: a discrete controller WMI-active but invisible
    // to wgpu/NVML means GPU work lands on the iGPU; warn on every run.
    for fault in stress_kit::gpu_stack::check_gpu_stack().broken {
        send(&update_tx, RunUpdate::Warning { message: fault });
    }

    if spec.target_component.is_none() && spec.touched_components.is_empty() {
        let msg = "refusing to start run: no hardware_component records could be \
                   created or linked (see warnings above). Every stress_test_run \
                   must reference at least one hardware_component — otherwise the \
                   metrics and events have no hardware context.".to_string();
        log::error!("[stress-runner/worker] {msg}");
        send(&update_tx, RunUpdate::Error { message: msg });
        running.store(false, Ordering::SeqCst);
        return;
    }
    log::info!(
        "[stress-runner/worker] hardware linked: target={:?}, touched={} component(s)",
        spec.target_component,
        spec.touched_components.len()
    );

    // ---- 1. Build + persist the StressTestRun row ----
    // `StressTestRun::create` already does a read-back via `Self::exists`,
    // so an Ok here proves the row landed in SurrealDB.
    let run = build_run(&spec);
    let run_id_clone = run.id.clone();
    let run_for_create = run.clone();
    match runtime::block_on(async move { StressTestRun::create(&run_for_create).await }) {
        Ok(persisted_id) => {
            log::info!(
                "[stress-runner/worker] stress_test_run created and verified: {persisted_id:?}"
            );
            if let Ok(mut guard) = run_id.lock() {
                *guard = Some(run_id_clone.clone());
            }
            send(&update_tx, RunUpdate::Started {
                run_id: run_id_clone.clone(),
            });
        }
        Err(err) => {
            let msg = format!("failed to create stress_test_run row: {err}");
            log::error!("[stress-runner/worker] {msg}");
            send(
                &update_tx,
                RunUpdate::Error { message: msg },
            );
            running.store(false, Ordering::SeqCst);
            return;
        }
    }

    // ---- 2. Track state for the final summary ----
    let mut acc = SummaryAccumulator::default();
    let mut outcomes: Vec<StageOutcome> = Vec::new();
    let rules = spec.rules.clone();

    // ---- 3. Branch on plan ----
    match spec.plan.clone() {
        RunPlan::Single {
            stressor,
            threads,
            duration_secs,
            memory_cap_mb,
            disk_file_mb,
        } => {
            let label = stressor.label().to_string();
            send(
                &update_tx,
                RunUpdate::StageStarted {
                    index: 0,
                    label: label.clone(),
                    stage_count: 1,
                },
            );
            persist_event(
                &run_id_clone,
                DbEventKind::StageStarted,
                "stress-kit",
                Some(label.clone()),
                None,
            );

            let config = StressConfig {
                stressor,
                threads,
                timeout: duration_secs.map(Duration::from_secs),
                memory_cap_mb,
                disk_file_mb,
            };
            let session = StressSession::start(config);
            let throughput_unit = stressor.throughput_unit();

            drive_single(
                &session,
                stressor,
                threads,
                throughput_unit,
                &telemetry,
                &cancel,
                &update_tx,
                &run_id_clone,
                &mut acc,
                duration_secs,
                started_at,
                &rules,
                &mut outcomes,
            );

            persist_event(
                &run_id_clone,
                DbEventKind::StageFinished,
                "stress-kit",
                Some(label),
                None,
            );
            send(&update_tx, RunUpdate::StageFinished { index: 0 });
            emit_stage_verdict(&update_tx, outcomes.last());
        }

        RunPlan::Scenario {
            stages: stage_specs,
            total_wall_secs,
            repeat_until_total,
        } => {
            let def = ScenarioDefinition {
                stages: stage_specs
                    .iter()
                    .map(|s| ScenarioStage {
                        label: s.label.clone(),
                        config: StressConfig {
                            stressor: s.stressor,
                            threads: s.threads,
                            timeout: None,
                            memory_cap_mb: s.memory_cap_mb,
                            disk_file_mb: s.disk_file_mb,
                        },
                        duration_secs: s.duration_secs,
                    })
                    .collect(),
                total_wall_secs,
                repeat_until_total,
            };
            let runner = ScenarioRunner::start(def);

            drive_scenario(
                &runner,
                &stage_specs,
                &telemetry,
                &cancel,
                &update_tx,
                &run_id_clone,
                &mut acc,
                &rules,
                &mut outcomes,
            );
        }

        RunPlan::Concurrent { lanes: mut lane_specs, duration_secs } => {
            if let Some(extra) = budget_concurrent_threads(&mut lane_specs) {
                send(&update_tx, RunUpdate::Warning { message: extra });
            }
            let label = format!("Concurrent ({} lanes)", lane_specs.len());
            send(
                &update_tx,
                RunUpdate::StageStarted { index: 0, label: label.clone(), stage_count: 1 },
            );
            persist_event(
                &run_id_clone,
                DbEventKind::StageStarted,
                "stress-kit",
                Some(label.clone()),
                None,
            );

            let sessions: Vec<StressSession> = lane_specs
                .iter()
                .map(|l| {
                    StressSession::start(StressConfig {
                        stressor: l.stressor,
                        threads: l.threads,
                        timeout: duration_secs.map(Duration::from_secs),
                        memory_cap_mb: l.memory_cap_mb,
                        disk_file_mb: l.disk_file_mb,
                    })
                })
                .collect();

            drive_concurrent(
                &lane_specs,
                &sessions,
                &telemetry,
                &cancel,
                &update_tx,
                &run_id_clone,
                &mut acc,
                duration_secs,
                started_at,
                &rules,
                &mut outcomes,
            );

            persist_event(
                &run_id_clone,
                DbEventKind::StageFinished,
                "stress-kit",
                Some(label),
                None,
            );
            send(&update_tx, RunUpdate::StageFinished { index: 0 });
        }
    }

    // ---- 4. Finalize ----
    let duration_secs = started_at.elapsed().as_secs_f64();
    let stages: Vec<ScenarioStageSummary> =
        outcomes.iter().map(|o| o.summary.clone()).collect();
    let verdict = acc.into_verdict(&run_id_clone, &cancel, duration_secs, &spec.tool, outcomes);

    // Same `'static` requirement as the create call — clone everything into
    // an owned async block.
    let finalize_id = run_id_clone.clone();
    let finalize_result = verdict.result;
    let finalize_reason = verdict.finish_reason;
    let finalize_failure = verdict.failure_mode.clone();
    let finalize_summary = verdict.summary.clone();
    let finalize_stages = stages.clone();
    let res = runtime::block_on(async move {
        StressTestRun::finalize(
            &finalize_id,
            finalize_result,
            finalize_reason,
            finalize_failure,
            finalize_summary,
            finalize_stages,
            None,
        )
        .await
    });
    if let Err(err) = res {
        send(
            &update_tx,
            RunUpdate::Warning {
                message: format!("finalize failed (run still recorded as in_progress): {err}"),
            },
        );
    }

    send(&update_tx, RunUpdate::Finished(verdict));
    running.store(false, Ordering::SeqCst);
}

/// Translate the `RunSpec` into the persisted `StressTestRun` row.
fn build_run(spec: &RunSpec) -> StressTestRun {
    let mut run = StressTestRun::new_for(spec.computer.clone(), spec.tool.clone(), spec.target_kind);
    run.target_component = spec.target_component.clone();
    run.touched_components = spec.touched_components.clone();
    run.service_order = spec.service_order.clone();
    run.session_ref = spec.session_ref.clone();
    run.task_ref = spec.task_ref.clone();
    run.tech = spec.tech.clone();
    run.hostname = spec.hostname.clone();
    run.machine_id = spec.machine_id.clone();
    run.bios_settings = spec.bios_settings.clone();
    run.driver_versions = spec.driver_versions.clone();
    run.notes = spec.notes.clone();
    run.preset_label = spec.preset_label.clone();
    run.tags = spec.tags.clone();

    if let RunPlan::Single { duration_secs, .. } = &spec.plan {
        run.duration_planned_secs = *duration_secs;
    }
    if let RunPlan::Scenario { stages, total_wall_secs, .. } = &spec.plan {
        let total: u64 = total_wall_secs
            .unwrap_or_else(|| stages.iter().map(|s| s.duration_secs).sum());
        run.duration_planned_secs = Some(total);
    }
    if let RunPlan::Concurrent { duration_secs, .. } = &spec.plan {
        run.duration_planned_secs = *duration_secs;
    }
    run
}

/// Text used when a fatal sample carries no `last_error`.
const FATAL_FALLBACK: &str = "run aborted (fatal stressor error)";

/// Operator-facing text for a fatal stressor sample.
fn fatal_message(metrics: &Metrics) -> String {
    metrics
        .last_error
        .clone()
        .unwrap_or_else(|| FATAL_FALLBACK.to_string())
}

/// Stage verdict when the run carries rules, plus unconditionally when the
/// stressor aborted — a fatal abort must never leave a stage unjudged.
fn stage_verdict_for(
    stats: &StageStats,
    rules: &Option<VerdictRules>,
    effective: &VerdictRules,
) -> Option<StageVerdict> {
    match rules {
        Some(r) => Some(evaluate_stage(stats, r)),
        None if stats.fatal_abort => Some(evaluate_stage(stats, effective)),
        None => None,
    }
}

/// Drive a single-stressor run: tick at 1 Hz, sample telemetry, batch metric
/// writes, forward updates to the UI.
fn drive_single(
    session: &StressSession,
    stressor: Stressor,
    threads: usize,
    throughput_unit: &'static str,
    telemetry: &Arc<TelemetryAgent>,
    cancel: &Arc<AtomicBool>,
    update_tx: &Sender<RunUpdate>,
    run_id: &RecordId,
    acc: &mut SummaryAccumulator,
    duration_secs: Option<u64>,
    started_at: Instant,
    rules: &Option<VerdictRules>,
    outcomes: &mut Vec<StageOutcome>,
) {
    let mut last_tick = Instant::now();
    let mut latest_metrics = Metrics::default();
    let mut metric_batch: Vec<StressTestMetric> = Vec::with_capacity(METRIC_BATCH_SIZE);
    let label = stressor.label().to_string();
    let effective_rules = rules.clone().unwrap_or_default();
    let mut stage_stats = StageStats::begin(0, &label, stressor, &telemetry.snapshot());
    let mut stage_last_error: Option<String> = None;
    let mut stage_had_error = false;
    let mut stage_peak: Option<f64> = None;
    let mut stage_tp_sum = 0.0_f64;
    let mut stage_tp_count = 0_u32;
    // Stressors latch `fatal` on every later sample; report it once.
    let mut fatal_reported = false;

    let deadline = duration_secs.map(|d| started_at + Duration::from_secs(d));

    loop {
        if cancel.load(Ordering::Relaxed) {
            session.stop();
            break;
        }
        if session.is_stopping() {
            break;
        }
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                session.stop();
                break;
            }
        }

        if let Some(m) = session.try_recv() {
            if m.fatal {
                if !fatal_reported {
                    fatal_reported = true;
                    send(update_tx, RunUpdate::Error { message: fatal_message(&m) });
                }
                // Fold immediately: the loop breaks on `is_stopping` before the
                // next periodic absorb.
                stage_stats.absorb_final(&m);
                session.stop();
            }
            if m.throughput > 0.0 {
                stage_peak = Some(stage_peak.map_or(m.throughput, |p: f64| p.max(m.throughput)));
                stage_tp_sum += m.throughput;
                stage_tp_count += 1;
            }
            if let Some(err) = &m.last_error {
                stage_had_error = true;
                stage_last_error = Some(err.clone());
            }
            latest_metrics = m;
        }

        if last_tick.elapsed() >= TICK_INTERVAL {
            last_tick = Instant::now();
            let snapshot = telemetry.snapshot();
            if snapshot.is_populated() {
                let whea_before = acc.whea_delta_count;
                let tdr_before = acc.tdr_delta_count;
                let new_errors = acc.absorb(&latest_metrics, &snapshot, throughput_unit);
                stage_stats.absorb_tick(&latest_metrics, &snapshot, &effective_rules);
                persist_counter_events(run_id, &acc, whea_before, tdr_before);
                if new_errors > 0 {
                    persist_error_event(run_id, stressor, new_errors, latest_metrics.last_error.clone());
                }

                match metric_from_snapshot(
                    run_id.clone(),
                    &snapshot,
                    Some(latest_metrics.throughput),
                    Some(throughput_unit),
                    latest_metrics.last_error.as_deref(),
                    None,
                    None,
                ) {
                    Ok(metric) => {
                        metric_batch.push(metric);
                        if metric_batch.len() >= METRIC_BATCH_SIZE {
                            if let Err(err) = flush_metrics(&mut metric_batch) {
                                send(
                                    update_tx,
                                    RunUpdate::Error {
                                        message: format!("stress_test_metric persist failed: {err}"),
                                    },
                                );
                                session.stop();
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        send(
                            update_tx,
                            RunUpdate::Error {
                                message: format!("invalid telemetry for stress_test_metric: {err}"),
                            },
                        );
                        session.stop();
                        break;
                    }
                }
            }

            send(
                update_tx,
                RunUpdate::Tick {
                    stage_index: None,
                    stage_label: None,
                    metrics: latest_metrics.clone(),
                    telemetry: snapshot,
                    throughput_unit,
                },
            );
        }

        thread::sleep(Duration::from_millis(50));
    }

    if let Err(err) = flush_metrics(&mut metric_batch) {
        send(
            update_tx,
            RunUpdate::Error {
                message: format!("stress_test_metric persist failed: {err}"),
            },
        );
    }

    // Final drain: any sample emitted after the last periodic absorb — the
    // fatal tick included — is still folded into the stage.
    if let Some(m) = session.try_recv() {
        if m.throughput > 0.0 {
            stage_peak = Some(stage_peak.map_or(m.throughput, |p: f64| p.max(m.throughput)));
            stage_tp_sum += m.throughput;
            stage_tp_count += 1;
        }
        if let Some(err) = &m.last_error {
            stage_had_error = true;
            stage_last_error = Some(err.clone());
        }
        latest_metrics = m;
    }
    stage_stats.absorb_final(&latest_metrics);
    if stage_stats.fatal_abort && !fatal_reported {
        send(
            update_tx,
            RunUpdate::Error {
                message: stage_stats
                    .fatal_reason
                    .clone()
                    .unwrap_or_else(|| FATAL_FALLBACK.to_string()),
            },
        );
    }
    stage_stats.finish(&telemetry.snapshot());
    let verdict = stage_verdict_for(&stage_stats, rules, &effective_rules);
    let avg = if stage_tp_count > 0 {
        Some(stage_tp_sum / stage_tp_count as f64)
    } else {
        None
    };
    let summary = stage_summary_from_stats(
        &stage_stats,
        stressor,
        threads as u32,
        duration_secs.unwrap_or(0),
        started_at.elapsed().as_secs_f64(),
        stage_peak,
        avg,
        throughput_unit,
        stage_had_error,
        stage_last_error,
        verdict.as_ref(),
    );
    if let Some(v) = &verdict {
        persist_stage_verdict_events(run_id, v);
    }
    outcomes.push(StageOutcome { summary, verdict });
}

/// Assign `threads` to `threads == 0` CPU lanes by dividing the core pool,
/// reserving one core when any lane drives the GPU. Returns a warning when
/// more than one lane contends for the single physical GPU.
fn budget_concurrent_threads(lanes: &mut [RunStage]) -> Option<String> {
    let total = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let uses_gpu = |s: Stressor| {
        s.is_gpu() || matches!(s, Stressor::Combined | Stressor::Psu | Stressor::PsuTransient)
    };
    let gpu_lane_count = lanes.iter().filter(|l| uses_gpu(l.stressor)).count();
    let reserve = usize::from(gpu_lane_count > 0);
    let pool = total.saturating_sub(reserve).max(1);

    let auto: Vec<usize> = lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| l.threads == 0 && !l.stressor.is_gpu())
        .map(|(i, _)| i)
        .collect();
    if !auto.is_empty() {
        let per = (pool / auto.len()).max(1);
        for i in auto {
            lanes[i].threads = per;
        }
    }

    (gpu_lane_count > 1).then(|| {
        format!("{gpu_lane_count} GPU lanes selected; they share one physical GPU and will contend")
    })
}

/// Drive a concurrent run: every lane runs at once as its own `StressSession`.
/// System telemetry rolls up once per tick; each lane writes its own metric
/// rows tagged by `stage_index` and gets its own stage verdict.
#[allow(clippy::too_many_arguments)]
fn drive_concurrent(
    lanes: &[RunStage],
    sessions: &[StressSession],
    telemetry: &Arc<TelemetryAgent>,
    cancel: &Arc<AtomicBool>,
    update_tx: &Sender<RunUpdate>,
    run_id: &RecordId,
    acc: &mut SummaryAccumulator,
    duration_secs: Option<u64>,
    started_at: Instant,
    rules: &Option<VerdictRules>,
    outcomes: &mut Vec<StageOutcome>,
) {
    let n = lanes.len();
    let effective_rules = rules.clone().unwrap_or_default();
    let snapshot0 = telemetry.snapshot();

    let mut latest: Vec<Metrics> = vec![Metrics::default(); n];
    let mut stats: Vec<StageStats> = lanes
        .iter()
        .enumerate()
        .map(|(i, l)| StageStats::begin(i as u32, &l.label, l.stressor, &snapshot0))
        .collect();
    let mut peak: Vec<Option<f64>> = vec![None; n];
    let mut tp_sum = vec![0.0_f64; n];
    let mut tp_count = vec![0_u32; n];
    let mut last_error: Vec<Option<String>> = vec![None; n];
    let mut had_error = vec![false; n];
    let mut seen_errors = vec![0_u64; n];
    let mut fatal_seen = vec![false; n];

    let mut last_tick = Instant::now();
    let mut metric_batch: Vec<StressTestMetric> = Vec::with_capacity(METRIC_BATCH_SIZE);
    let deadline = duration_secs.map(|d| started_at + Duration::from_secs(d));
    let stop_all = |sessions: &[StressSession]| sessions.iter().for_each(|s| s.stop());
    let mut aborted = false;

    loop {
        if cancel.load(Ordering::Relaxed) {
            stop_all(sessions);
            break;
        }
        if sessions.iter().all(|s| s.is_stopping()) {
            break;
        }
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                stop_all(sessions);
                break;
            }
        }

        for (i, s) in sessions.iter().enumerate() {
            if let Some(m) = s.try_recv() {
                if m.fatal {
                    if !fatal_seen[i] {
                        fatal_seen[i] = true;
                        let msg = m.last_error.clone().unwrap_or_else(|| {
                            format!("{} aborted (fatal stressor error)", lanes[i].label)
                        });
                        send(update_tx, RunUpdate::Error { message: msg });
                    }
                    stats[i].absorb_final(&m);
                    s.stop();
                }
                if m.throughput > 0.0 {
                    peak[i] = Some(peak[i].map_or(m.throughput, |p: f64| p.max(m.throughput)));
                    tp_sum[i] += m.throughput;
                    tp_count[i] += 1;
                }
                if let Some(err) = &m.last_error {
                    had_error[i] = true;
                    last_error[i] = Some(err.clone());
                }
                latest[i] = m;
            }
        }

        if last_tick.elapsed() >= TICK_INTERVAL {
            last_tick = Instant::now();
            let snapshot = telemetry.snapshot();
            if snapshot.is_populated() {
                let whea_before = acc.whea_delta_count;
                let tdr_before = acc.tdr_delta_count;
                acc.absorb(&Metrics::default(), &snapshot, "mixed");
                persist_counter_events(run_id, acc, whea_before, tdr_before);

                for (i, lane) in lanes.iter().enumerate() {
                    let unit = lane.stressor.throughput_unit();
                    stats[i].absorb_tick(&latest[i], &snapshot, &effective_rules);
                    if latest[i].errors > seen_errors[i] {
                        let new_errors = latest[i].errors - seen_errors[i];
                        seen_errors[i] = latest[i].errors;
                        persist_error_event(
                            run_id,
                            lane.stressor,
                            new_errors,
                            latest[i].last_error.clone(),
                        );
                    }
                    match metric_from_snapshot(
                        run_id.clone(),
                        &snapshot,
                        Some(latest[i].throughput),
                        Some(unit),
                        latest[i].last_error.as_deref(),
                        Some(i as u32),
                        Some(lane.label.clone()),
                    ) {
                        Ok(metric) => {
                            metric_batch.push(metric);
                            if metric_batch.len() >= METRIC_BATCH_SIZE {
                                if let Err(err) = flush_metrics(&mut metric_batch) {
                                    send(update_tx, RunUpdate::Error {
                                        message: format!("stress_test_metric persist failed: {err}"),
                                    });
                                    stop_all(sessions);
                                    aborted = true;
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            send(update_tx, RunUpdate::Error {
                                message: format!("invalid telemetry for stress_test_metric: {err}"),
                            });
                            stop_all(sessions);
                            aborted = true;
                            break;
                        }
                    }
                }
            }

            for (i, lane) in lanes.iter().enumerate() {
                send(update_tx, RunUpdate::Tick {
                    stage_index: Some(i as u32),
                    stage_label: Some(lane.label.clone()),
                    metrics: latest[i].clone(),
                    telemetry: snapshot.clone(),
                    throughput_unit: lane.stressor.throughput_unit(),
                });
            }

            if aborted {
                break;
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    if let Err(err) = flush_metrics(&mut metric_batch) {
        send(update_tx, RunUpdate::Error {
            message: format!("stress_test_metric persist failed: {err}"),
        });
    }

    let final_snapshot = telemetry.snapshot();
    let mut total_test_errors = 0_u64;
    for (i, lane) in lanes.iter().enumerate() {
        // Final drain per lane, then fold: the last sample can land after the
        // last periodic absorb.
        if let Some(m) = sessions.get(i).and_then(|s| s.try_recv()) {
            if m.throughput > 0.0 {
                peak[i] = Some(peak[i].map_or(m.throughput, |p: f64| p.max(m.throughput)));
                tp_sum[i] += m.throughput;
                tp_count[i] += 1;
            }
            if let Some(err) = &m.last_error {
                had_error[i] = true;
                last_error[i] = Some(err.clone());
            }
            latest[i] = m;
        }
        stats[i].absorb_final(&latest[i]);
        fatal_seen[i] |= stats[i].fatal_abort;
        stats[i].finish(&final_snapshot);
        total_test_errors = total_test_errors.saturating_add(stats[i].errors);
        // A lane that aborted mid-run is a failure even with no error counter;
        // classify it the way `absorb` would for the single-stressor path.
        if fatal_seen[i] {
            let detail = last_error[i].clone().unwrap_or_default();
            if is_gpu_error_message(&detail) {
                acc.gpu_device_errors = acc.gpu_device_errors.saturating_add(1);
                acc.last_gpu_error = Some(detail);
            } else {
                acc.disk_io_errors = acc.disk_io_errors.saturating_add(1);
                acc.last_disk_error = Some(detail);
            }
        }
        let unit = lane.stressor.throughput_unit();
        let verdict = stage_verdict_for(&stats[i], rules, &effective_rules);
        let avg = if tp_count[i] > 0 {
            Some(tp_sum[i] / tp_count[i] as f64)
        } else {
            None
        };
        let summary = stage_summary_from_stats(
            &stats[i],
            lane.stressor,
            lane.threads as u32,
            duration_secs.unwrap_or(0),
            started_at.elapsed().as_secs_f64(),
            peak[i],
            avg,
            unit,
            had_error[i],
            last_error[i].take(),
            verdict.as_ref(),
        );
        if let Some(v) = &verdict {
            persist_stage_verdict_events(run_id, v);
        }
        outcomes.push(StageOutcome { summary, verdict });
        emit_stage_verdict(update_tx, outcomes.last());
    }
    acc.completed_stage_errors = acc.completed_stage_errors.saturating_add(total_test_errors);
}

/// Drive a scenario run.
fn drive_scenario(
    runner: &ScenarioRunner,
    stage_specs: &[RunStage],
    telemetry: &Arc<TelemetryAgent>,
    cancel: &Arc<AtomicBool>,
    update_tx: &Sender<RunUpdate>,
    run_id: &RecordId,
    acc: &mut SummaryAccumulator,
    rules: &Option<VerdictRules>,
    outcomes: &mut Vec<StageOutcome>,
) {
    let mut last_tick = Instant::now();
    let mut current_stage_index: Option<usize> = None;
    let mut current_stage_label: Option<String> = None;
    let mut current_unit: &'static str = "ops/s";
    let mut current_stage_started_at: Option<Instant> = None;
    let mut current_stage_peak_throughput: Option<f64> = None;
    let mut current_stage_throughput_sum: f64 = 0.0;
    let mut current_stage_throughput_count: u32 = 0;
    let mut current_stage_last_error: Option<String> = None;
    let mut current_stage_had_error = false;
    let mut current_stage_stats: Option<StageStats> = None;
    let effective_rules = rules.clone().unwrap_or_default();
    let mut latest_metrics = Metrics::default();
    let mut metric_batch: Vec<StressTestMetric> = Vec::with_capacity(METRIC_BATCH_SIZE);
    let mut finished = false;

    while !finished {
        if cancel.load(Ordering::Relaxed) {
            runner.stop();
        }

        for event in runner.try_recv_all() {
            match event {
                ScenarioEvent::StageStarted { index, label, stage_count } => {
                    let stressor = stage_specs
                        .get(index)
                        .map(|s| s.stressor)
                        .unwrap_or(Stressor::Cpu);
                    current_unit = stressor.throughput_unit();
                    current_stage_index = Some(index);
                    current_stage_label = Some(label.clone());
                    current_stage_started_at = Some(Instant::now());
                    current_stage_peak_throughput = None;
                    current_stage_throughput_sum = 0.0;
                    current_stage_throughput_count = 0;
                    current_stage_last_error = None;
                    current_stage_had_error = false;
                    // Stressors latch `fatal`/`last_error`; a stale carry-over
                    // would be charged to this stage.
                    latest_metrics = Metrics::default();
                    current_stage_stats = Some(StageStats::begin(
                        index as u32,
                        &label,
                        stressor,
                        &telemetry.snapshot(),
                    ));

                    // Persist a stress_test_event row for the stage transition.
                    persist_event(
                        run_id,
                        DbEventKind::StageStarted,
                        "stress-kit",
                        Some(label.clone()),
                        None,
                    );

                    send(
                        update_tx,
                        RunUpdate::StageStarted { index, label, stage_count },
                    );
                }
                ScenarioEvent::Tick { stage_index, metrics } => {
                    latest_metrics = metrics.clone();
                    current_stage_peak_throughput = Some(
                        current_stage_peak_throughput
                            .map(|p| p.max(metrics.throughput))
                            .unwrap_or(metrics.throughput),
                    );
                    current_stage_throughput_sum += metrics.throughput;
                    current_stage_throughput_count += 1;
                    if let Some(err) = &metrics.last_error {
                        current_stage_had_error = true;
                        current_stage_last_error = Some(err.clone());
                    }
                    // Every scenario tick is folded here, so a fatal can't be
                    // lost between the 1 Hz absorbs.
                    let first_fatal = metrics.fatal
                        && current_stage_stats.as_ref().is_some_and(|s| !s.fatal_abort);
                    if let Some(stats) = current_stage_stats.as_mut() {
                        stats.absorb_final(&metrics);
                    }
                    if first_fatal {
                        send(
                            update_tx,
                            RunUpdate::Error { message: fatal_message(&metrics) },
                        );
                    }
                    let _ = stage_index; // already tracked in current_stage_index
                }
                ScenarioEvent::StageFinished { index } => {
                    let elapsed = current_stage_started_at
                        .map(|s| s.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    let avg = if current_stage_throughput_count > 0 {
                        Some(current_stage_throughput_sum / current_stage_throughput_count as f64)
                    } else {
                        None
                    };
                    let planned = stage_specs.get(index).map(|s| s.duration_secs).unwrap_or(0);
                    let stressor = stage_specs
                        .get(index)
                        .map(|s| s.stressor)
                        .unwrap_or(Stressor::Cpu);
                    let mut stats = current_stage_stats.take().unwrap_or_else(|| {
                        StageStats::begin(
                            index as u32,
                            current_stage_label.as_deref().unwrap_or(""),
                            stressor,
                            &telemetry.snapshot(),
                        )
                    });
                    stats.absorb_final(&latest_metrics);
                    stats.finish(&telemetry.snapshot());
                    let verdict = stage_verdict_for(&stats, rules, &effective_rules);
                    let summary = stage_summary_from_stats(
                        &stats,
                        stressor,
                        stage_specs.get(index).map(|s| s.threads as u32).unwrap_or(0),
                        planned,
                        elapsed,
                        current_stage_peak_throughput,
                        avg,
                        current_unit,
                        current_stage_had_error,
                        current_stage_last_error.clone(),
                        verdict.as_ref(),
                    );
                    if let Some(v) = &verdict {
                        persist_stage_verdict_events(run_id, v);
                    }
                    outcomes.push(StageOutcome { summary, verdict });

                    persist_event(
                        run_id,
                        DbEventKind::StageFinished,
                        "stress-kit",
                        current_stage_label.clone(),
                        None,
                    );

                    send(update_tx, RunUpdate::StageFinished { index });
                    emit_stage_verdict(update_tx, outcomes.last());
                }
                ScenarioEvent::Finished { reason, total_elapsed_secs } => {
                    acc.scenario_finish = Some(map_finish_reason(reason));
                    let _ = total_elapsed_secs;
                    finished = true;
                }
            }
        }

        if last_tick.elapsed() >= TICK_INTERVAL && current_stage_index.is_some() {
            last_tick = Instant::now();
            let snapshot = telemetry.snapshot();
            if snapshot.is_populated() {
                let whea_before = acc.whea_delta_count;
                let tdr_before = acc.tdr_delta_count;
                let new_errors = acc.absorb(&latest_metrics, &snapshot, current_unit);
                if let Some(stats) = current_stage_stats.as_mut() {
                    stats.absorb_tick(&latest_metrics, &snapshot, &effective_rules);
                }
                persist_counter_events(run_id, &acc, whea_before, tdr_before);
                if new_errors > 0 {
                    let stressor = current_stage_index
                        .and_then(|i| stage_specs.get(i))
                        .map(|s| s.stressor)
                        .unwrap_or(Stressor::Cpu);
                    persist_error_event(run_id, stressor, new_errors, latest_metrics.last_error.clone());
                }

                match metric_from_snapshot(
                    run_id.clone(),
                    &snapshot,
                    Some(latest_metrics.throughput),
                    Some(current_unit),
                    latest_metrics.last_error.as_deref(),
                    current_stage_index.map(|i| i as u32),
                    current_stage_label.clone(),
                ) {
                    Ok(metric) => {
                        metric_batch.push(metric);
                        if metric_batch.len() >= METRIC_BATCH_SIZE {
                            if let Err(err) = flush_metrics(&mut metric_batch) {
                                send(
                                    update_tx,
                                    RunUpdate::Error {
                                        message: format!(
                                            "stress_test_metric persist failed: {err}"
                                        ),
                                    },
                                );
                                runner.stop();
                                finished = true;
                            }
                        }
                    }
                    Err(err) => {
                        send(
                            update_tx,
                            RunUpdate::Error {
                                message: format!(
                                    "invalid telemetry for stress_test_metric: {err}"
                                ),
                            },
                        );
                        runner.stop();
                        finished = true;
                    }
                }
            }

            send(
                update_tx,
                RunUpdate::Tick {
                    stage_index: current_stage_index.map(|i| i as u32),
                    stage_label: current_stage_label.clone(),
                    metrics: latest_metrics.clone(),
                    telemetry: snapshot,
                    throughput_unit: current_unit,
                },
            );
        }

        thread::sleep(Duration::from_millis(50));
    }

    if let Err(err) = flush_metrics(&mut metric_batch) {
        send(
            update_tx,
            RunUpdate::Error {
                message: format!("stress_test_metric persist failed: {err}"),
            },
        );
    }
}

/// Render a `catch_unwind` payload as a human-readable string. Handles the
/// two `panic!()`-default payload shapes (`&'static str` and `String`)
/// and falls back to a generic label for everything else.
fn panic_payload_str(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(unknown panic payload)".to_string()
    }
}

fn map_finish_reason(reason: SkFinishReason) -> DbFinishReason {
    match reason {
        SkFinishReason::Completed => DbFinishReason::Completed,
        SkFinishReason::Cancelled => DbFinishReason::Cancelled,
        SkFinishReason::TotalTime => DbFinishReason::TotalTime,
    }
}

fn persist_event(
    run_ref: &RecordId,
    kind: DbEventKind,
    source: &str,
    detail: Option<String>,
    data: Option<serde_json::Value>,
) {
    let mut event = DbStressTestEvent::new(run_ref.clone(), kind, source);
    if let Some(d) = detail {
        event.detail = d;
    }
    event.data = data;
    let _ = event.id; // suppress unused if we ever read it
    let _ = STRESS_TEST_EVENT_TABLE; // imported for callsite clarity
    let _ = random_record_id; // keeping import in scope for future callers
    runtime::spawn(async move {
        if let Err(err) = DbStressTestEvent::create(&event).await {
            log::warn!("stress-runner: failed to persist event: {err}");
        }
    });
}

/// Persist a `stress_test_event` for newly observed test errors. Memory
/// stressors map to `memory_error`; everything else lands as `custom`
/// with a `data_mismatch` code.
fn persist_error_event(
    run_ref: &RecordId,
    stressor: Stressor,
    new_errors: u64,
    detail: Option<String>,
) {
    let kind = match stressor {
        Stressor::MemTest | Stressor::Memory | Stressor::GpuVram => DbEventKind::MemoryError,
        _ => DbEventKind::Custom,
    };
    let mut event = DbStressTestEvent::new(run_ref.clone(), kind, "stress-kit");
    event.code = Some("data_mismatch".to_string());
    event.detail = detail.unwrap_or_else(|| {
        format!("{} reported {new_errors} new error(s)", stressor.label())
    });
    event.data = Some(serde_json::json!({
        "stressor": stressor.label(),
        "new_errors": new_errors,
    }));
    runtime::spawn(async move {
        if let Err(err) = DbStressTestEvent::create(&event).await {
            log::warn!("stress-runner: failed to persist error event: {err}");
        }
    });
}

/// Extended `ScenarioStageSummary` from per-stage stats + legacy tick fields.
#[allow(clippy::too_many_arguments)]
fn stage_summary_from_stats(
    stats: &StageStats,
    stressor: Stressor,
    threads: u32,
    planned_secs: u64,
    elapsed_secs: f64,
    peak_throughput: Option<f64>,
    avg_throughput: Option<f64>,
    unit: &str,
    had_error: bool,
    last_error: Option<String>,
    verdict: Option<&StageVerdict>,
) -> ScenarioStageSummary {
    ScenarioStageSummary {
        index: stats.index,
        label: stats.label.clone(),
        stressor: stressor.as_str().to_string(),
        threads,
        duration_planned_secs: planned_secs,
        duration_actual_secs: elapsed_secs,
        peak_throughput,
        avg_throughput,
        throughput_unit: unit.to_string(),
        had_error,
        last_error,
        result: verdict.map(|v| if v.pass { "pass" } else { "fail" }.to_string()),
        violations: verdict.map(|v| v.violation_lines()).unwrap_or_default(),
        max_temp_c: stats.max_cpu_temp_c,
        avg_temp_c: stats.avg_cpu_temp_c(),
        max_gpu_temp_c: stats.max_gpu_temp_c,
        min_v12_v: stats.min_v12_v,
        max_clock_mhz: stats.max_avg_clock_mhz,
        errors: stats.errors,
        whea_delta: stats.whea_delta,
        tdr_delta: stats.tdr_delta,
        throughput_cv: stats.throughput_cv(),
        clock_collapse_ticks: stats.worst_collapse_run,
    }
}

/// Forward the latest stage's rules verdict to the UI, if it carries one.
fn emit_stage_verdict(tx: &Sender<RunUpdate>, outcome: Option<&StageOutcome>) {
    let Some(outcome) = outcome else { return };
    let Some(v) = &outcome.verdict else { return };
    send(
        tx,
        RunUpdate::StageVerdict {
            index: v.index as usize,
            label: v.label.clone(),
            pass: v.pass,
            violations: v.violation_lines(),
            peak_throughput: outcome.summary.peak_throughput,
        },
    );
}

/// Persist WHEA/TDR event rows when the run counters moved this tick.
fn persist_counter_events(
    run_id: &RecordId,
    acc: &SummaryAccumulator,
    whea_before: u32,
    tdr_before: u32,
) {
    if acc.whea_delta_count > whea_before {
        let mut event = DbStressTestEvent::new(run_id.clone(), DbEventKind::WheaHit, "telemetry");
        event.detail = format!(
            "WHEA counter moved {whea_before} -> {}",
            acc.whea_delta_count
        );
        runtime::spawn(async move {
            if let Err(err) = DbStressTestEvent::create(&event).await {
                log::warn!("stress-runner: failed to persist whea event: {err}");
            }
        });
    }
    if acc.tdr_delta_count > tdr_before {
        let mut event = DbStressTestEvent::new(run_id.clone(), DbEventKind::Tdr, "telemetry");
        event.detail = format!("TDR counter moved {tdr_before} -> {}", acc.tdr_delta_count);
        runtime::spawn(async move {
            if let Err(err) = DbStressTestEvent::create(&event).await {
                log::warn!("stress-runner: failed to persist tdr event: {err}");
            }
        });
    }
}

/// Persist verdict-derived events: the failure event for a failed stage and a
/// warning event per verdict advisory (e.g. WHEA source unavailable).
fn persist_stage_verdict_events(run_id: &RecordId, verdict: &StageVerdict) {
    if !verdict.pass {
        persist_stage_verdict_event(run_id, verdict);
    }
    for warning in &verdict.warnings {
        log::warn!("stress-runner: stage '{}' — {warning}", verdict.label);
        let mut event = DbStressTestEvent::new(run_id.clone(), DbEventKind::Custom, "verdict-rules");
        event.code = Some("warning".to_string());
        event.detail = format!("stage '{}': {warning}", verdict.label);
        runtime::spawn(async move {
            if let Err(err) = DbStressTestEvent::create(&event).await {
                log::warn!("stress-runner: failed to persist stage warning event: {err}");
            }
        });
    }
}

/// Persist a `custom`/`stage_verdict` event for a failed stage.
fn persist_stage_verdict_event(run_id: &RecordId, verdict: &StageVerdict) {
    let mut event = DbStressTestEvent::new(run_id.clone(), DbEventKind::Custom, "verdict-rules");
    event.code = Some("stage_verdict".to_string());
    event.detail = format!(
        "stage '{}' failed: {}",
        verdict.label,
        verdict.violation_lines().join("; ")
    );
    event.data = serde_json::to_value(verdict).ok();
    runtime::spawn(async move {
        if let Err(err) = DbStressTestEvent::create(&event).await {
            log::warn!("stress-runner: failed to persist stage verdict event: {err}");
        }
    });
}

fn flush_metrics(batch: &mut Vec<StressTestMetric>) -> anyhow::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let take = std::mem::take(batch);
    runtime::block_on(async move {
        for m in &take {
            StressTestMetric::create(m).await?;
        }
        Ok::<(), anyhow::Error>(())
    })
}

fn send(tx: &Sender<RunUpdate>, update: RunUpdate) {
    match tx.try_send(update) {
        Ok(_) => {}
        Err(TrySendError::Full(_)) => {
            log::debug!("stress-runner: update channel full, dropping update");
        }
        Err(TrySendError::Disconnected(_)) => {
            // UI dropped the controller — fine, worker will notice cancel on next loop.
        }
    }
}

// ---------------------------------------------------------------------------
// Summary accumulator
// ---------------------------------------------------------------------------

/// `Metrics.last_error` strings originating from stress-kit's GPU stressors:
/// stage prefixes (`gpu:`, `gpu_matmul:`, `gpu_vram:`, `gpu_pcie:`), wgpu
/// device-loss/removal text, and the psu stressor's GPU-leg warnings.
fn is_gpu_error_message(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.starts_with("gpu")
        || m.contains("gpu device failed")
        || m.contains("device is lost")
        || m.contains("device lost")
        || m.contains("device removed")
        || m.contains("dxgi_error")
        || m.contains("gpu unavailable")
        || m.contains("gpu leg stopped")
        || m.contains("readback map failed")
        || m.contains("without 'gpu' feature")
}

#[derive(Default)]
struct SummaryAccumulator {
    max_temp_c: Option<f32>,
    sum_temp_c: f32,
    temp_samples: u32,
    max_clock_mhz: Option<u32>,
    sum_clock_mhz: u64,
    clock_samples: u32,
    max_usage_pct: f32,
    sum_usage_pct: f32,
    usage_samples: u32,
    peak_throughput: Option<f64>,
    sum_throughput: f64,
    throughput_samples: u32,
    last_throughput_unit: Option<String>,
    whea_delta_count: u32,
    tdr_delta_count: u32,
    max_gpu_temp_c: Option<f32>,
    max_cpu_temp_c: Option<f32>,
    /// Lowest +12V rail sample of the run; droop is the PSU diagnostic, not peak.
    min_v12_v: Option<f32>,
    disk_io_errors: u32,
    /// GPU-classified `last_error` transitions (device lost, acquire failed).
    gpu_device_errors: u32,
    max_power_w: Option<u32>,
    /// `Metrics.errors` from stages that already reset their counter.
    completed_stage_errors: u64,
    /// Latest cumulative `Metrics.errors` of the in-flight stage.
    current_stage_errors: u64,
    last_error: Option<String>,
    /// Message of the latest counted GPU-classified error.
    last_gpu_error: Option<String>,
    /// Message of the latest counted disk-classified error.
    last_disk_error: Option<String>,
    scenario_finish: Option<DbFinishReason>,
}

impl SummaryAccumulator {
    /// Fold one tick into the rollup. Returns how many new test errors this
    /// tick revealed so the caller can persist a `stress_test_event`.
    fn absorb(
        &mut self,
        metrics: &Metrics,
        snapshot: &TelemetrySnapshot,
        unit: &'static str,
    ) -> u64 {
        // throughput
        if metrics.throughput > 0.0 {
            self.peak_throughput = Some(
                self.peak_throughput
                    .map(|p| p.max(metrics.throughput))
                    .unwrap_or(metrics.throughput),
            );
            self.sum_throughput += metrics.throughput;
            self.throughput_samples = self.throughput_samples.saturating_add(1);
            self.last_throughput_unit = Some(unit.to_string());
        }

        // `Metrics.errors` is cumulative per stressor; a drop means a new
        // stage started with a fresh counter.
        let mut new_errors = 0u64;
        if metrics.errors < self.current_stage_errors {
            self.completed_stage_errors =
                self.completed_stage_errors.saturating_add(self.current_stage_errors);
            self.current_stage_errors = 0;
        }
        if metrics.errors > self.current_stage_errors {
            new_errors = metrics.errors - self.current_stage_errors;
            self.current_stage_errors = metrics.errors;
        }

        if let Some(err) = &metrics.last_error {
            // Count message transitions only; `latest_metrics` repeats the
            // same string every tick until the stressor replaces it.
            if metrics.errors == 0 && self.last_error.as_deref() != Some(err.as_str()) {
                if is_gpu_error_message(err) {
                    self.gpu_device_errors = self.gpu_device_errors.saturating_add(1);
                    self.last_gpu_error = Some(err.clone());
                } else {
                    self.disk_io_errors = self.disk_io_errors.saturating_add(1);
                    self.last_disk_error = Some(err.clone());
                }
            }
            self.last_error = Some(err.clone());
        }

        // temp / clock / usage from cores
        for c in &snapshot.cores {
            if let Some(t) = c.temp_c {
                self.sum_temp_c += t;
                self.temp_samples = self.temp_samples.saturating_add(1);
                self.max_temp_c = Some(self.max_temp_c.map(|m| m.max(t)).unwrap_or(t));
            }
            let mhz = c.freq_mhz as u32;
            if mhz > 0 {
                self.sum_clock_mhz = self.sum_clock_mhz.saturating_add(mhz as u64);
                self.clock_samples = self.clock_samples.saturating_add(1);
                self.max_clock_mhz = Some(self.max_clock_mhz.map(|m| m.max(mhz)).unwrap_or(mhz));
            }
            self.sum_usage_pct += c.usage_pct;
            self.usage_samples = self.usage_samples.saturating_add(1);
            self.max_usage_pct = self.max_usage_pct.max(c.usage_pct);
        }

        if let Some(whea) = &snapshot.whea {
            self.whea_delta_count = whea.delta_since_program_start as u32;
        }
        if let Some(tdr) = &snapshot.tdr {
            self.tdr_delta_count = tdr.delta_since_program_start as u32;
        }

        for g in &snapshot.gpus {
            if let Some(t) = g.temp_c {
                self.max_gpu_temp_c = Some(self.max_gpu_temp_c.map_or(t, |m| m.max(t)));
            }
        }

        if let Some(t) = snapshot.cpu_package_temp_c() {
            self.max_cpu_temp_c = Some(self.max_cpu_temp_c.map_or(t, |m| m.max(t)));
        }

        if let Some(v) = snapshot.rail_12v() {
            self.min_v12_v = Some(self.min_v12_v.map_or(v, |m| m.min(v)));
        }

        // GPU board power summed across cards (NVML); CPU package power has
        // no portable source, so this is the PSU-load proxy we have.
        let gpu_w: f64 = snapshot.gpus.iter().filter_map(|g| g.power_w).map(f64::from).sum();
        if gpu_w > 0.0 {
            let w = gpu_w.round() as u32;
            self.max_power_w = Some(self.max_power_w.map_or(w, |m| m.max(w)));
        }

        new_errors
    }

    fn total_test_errors(&self) -> u64 {
        self.completed_stage_errors
            .saturating_add(self.current_stage_errors)
    }

    fn into_summary(&self) -> RunSummary {
        RunSummary {
            max_temp_c: self.max_temp_c,
            avg_temp_c: if self.temp_samples > 0 {
                Some(self.sum_temp_c / self.temp_samples as f32)
            } else {
                None
            },
            max_clock_mhz: self.max_clock_mhz,
            avg_clock_mhz: if self.clock_samples > 0 {
                Some((self.sum_clock_mhz / self.clock_samples as u64) as u32)
            } else {
                None
            },
            max_cpu_usage_pct: if self.usage_samples > 0 {
                Some(self.max_usage_pct)
            } else {
                None
            },
            avg_cpu_usage_pct: if self.usage_samples > 0 {
                Some(self.sum_usage_pct / self.usage_samples as f32)
            } else {
                None
            },
            max_power_w: self.max_power_w,
            max_fan_rpm: None,
            peak_throughput: self.peak_throughput,
            avg_throughput: if self.throughput_samples > 0 {
                Some(self.sum_throughput / self.throughput_samples as f64)
            } else {
                None
            },
            throughput_unit: self.last_throughput_unit.clone(),
            thermal_throttle_detected: false,
            vrm_throttle_detected: false,
            whea_delta_count: self.whea_delta_count,
            tdr_count: self.tdr_delta_count,
            bsod_detected: false,
            bsod_code: None,
            disk_io_errors: self.disk_io_errors,
            memory_errors: 0,
            test_errors: self.total_test_errors().min(u32::MAX as u64) as u32,
            max_gpu_temp_c: self.max_gpu_temp_c,
            max_cpu_temp_c: self.max_cpu_temp_c,
            min_v12_v: self.min_v12_v,
        }
    }

    fn into_verdict(
        self,
        run_id: &RecordId,
        cancel: &Arc<AtomicBool>,
        duration_secs: f64,
        tool: &TestTool,
        stage_outcomes: Vec<StageOutcome>,
    ) -> RunVerdict {
        let mut summary = self.into_summary();
        // Memtest mismatches also count as memory errors for the
        // HCI/TM5-shaped rubric fields.
        if matches!(
            tool,
            TestTool::StressKit { stressor } if stressor == "memtest"
        ) {
            summary.memory_errors = summary.test_errors;
        }

        // Not gated on `rules`: stages without a policy still carry a verdict
        // when the stressor aborted.
        let rules_failure = rules_failure_mode(&stage_outcomes, &mut summary);

        let cancelled = cancel.load(Ordering::Relaxed);
        let had_failure = rules_failure.is_some()
            || summary.whea_delta_count > 0
            || summary.test_errors > 0
            || summary.disk_io_errors > 0
            || self.gpu_device_errors > 0
            || summary.bsod_detected
            || summary.thermal_throttle_detected;

        let (result, finish_reason, failure_mode) = if had_failure {
            let mode = if summary.whea_delta_count > 0 {
                FailureMode::WheaError {
                    count: summary.whea_delta_count,
                }
            } else if let Some(mode) = rules_failure {
                mode
            } else if self.gpu_device_errors > 0 {
                FailureMode::GpuDeviceLost {
                    message: self.last_gpu_error.clone().unwrap_or_default(),
                }
            } else if summary.test_errors > 0 {
                FailureMode::DataMismatch { addresses: None }
            } else if summary.disk_io_errors > 0 {
                FailureMode::DiskIoError {
                    message: self
                        .last_disk_error
                        .clone()
                        .or_else(|| self.last_error.clone())
                        .unwrap_or_default(),
                }
            } else {
                FailureMode::None
            };
            (RunResult::Fail, DbFinishReason::Completed, mode)
        } else if cancelled {
            (
                RunResult::Aborted,
                DbFinishReason::Cancelled,
                FailureMode::None,
            )
        } else {
            (
                RunResult::Pass,
                self.scenario_finish.unwrap_or(DbFinishReason::Completed),
                FailureMode::None,
            )
        };

        RunVerdict {
            run_id: run_id.clone(),
            result,
            finish_reason,
            failure_mode,
            summary,
            duration_secs,
            stage_outcomes,
        }
    }
}

/// Dominant `FailureMode` across failed stage verdicts, by severity:
/// WHEA > TDR > temp > data mismatch > fatal abort > clock collapse >
/// unstable throughput. A fatal abort outranks the throughput-derived rules
/// because those read a load that stopped running, but yields to the
/// independent hardware evidence above it. Also folds throttle observations
/// into the run summary flags.
fn rules_failure_mode(
    outcomes: &[StageOutcome],
    summary: &mut RunSummary,
) -> Option<FailureMode> {
    let mut tdr: Option<FailureMode> = None;
    let mut temp: Option<FailureMode> = None;
    let mut mismatch: Option<FailureMode> = None;
    let mut fatal_abort: Option<FailureMode> = None;
    let mut collapse: Option<FailureMode> = None;
    let mut unstable: Option<FailureMode> = None;
    let mut droop: Option<FailureMode> = None;

    for outcome in outcomes {
        let Some(verdict) = &outcome.verdict else { continue };
        let stage_had_temp_violation = verdict
            .violations
            .iter()
            .any(|v| matches!(v, RuleViolation::CpuTemp { .. } | RuleViolation::GpuTemp { .. }));
        for violation in &verdict.violations {
            match violation {
                RuleViolation::Whea { corrected, fatal } => {
                    return Some(FailureMode::WheaError { count: corrected + fatal });
                }
                RuleViolation::Tdr { delta } => {
                    tdr.get_or_insert(FailureMode::Tdr { count: *delta });
                }
                RuleViolation::CpuTemp { peak_c, .. } | RuleViolation::GpuTemp { peak_c, .. } => {
                    summary.thermal_throttle_detected = true;
                    temp.get_or_insert(FailureMode::ThermalThrottle { peak_temp_c: *peak_c });
                }
                RuleViolation::StressorErrors { .. } => {
                    mismatch.get_or_insert(FailureMode::DataMismatch { addresses: None });
                }
                RuleViolation::ClockCollapse { below_pct, .. } => {
                    if !stage_had_temp_violation {
                        summary.vrm_throttle_detected = true;
                    }
                    collapse.get_or_insert(FailureMode::ClockCollapse {
                        stage_label: verdict.label.clone(),
                        below_pct: *below_pct,
                    });
                }
                RuleViolation::ThroughputUnstable { cv, .. } => {
                    unstable.get_or_insert(FailureMode::ThroughputUnstable {
                        stage_label: verdict.label.clone(),
                        cv: *cv,
                    });
                }
                RuleViolation::RailDroop { rail, min_v, .. } => {
                    summary.vrm_throttle_detected = true;
                    droop.get_or_insert(FailureMode::RailDroop {
                        rail: rail.clone(),
                        min_v: *min_v,
                    });
                }
                // The stressor never ran its load: a tooling/environment error,
                // not evidence against the hardware.
                RuleViolation::FatalAbort { reason } => {
                    fatal_abort.get_or_insert(FailureMode::AppError {
                        exit_code: None,
                        message: reason.clone().unwrap_or_else(|| {
                            format!(
                                "stage '{}' aborted before its load completed",
                                verdict.label
                            )
                        }),
                    });
                }
            }
        }
    }

    tdr.or(temp)
        .or(mismatch)
        .or(fatal_abort)
        .or(collapse)
        .or(unstable)
        .or(droop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(last_error: Option<&str>, errors: u64) -> Metrics {
        Metrics {
            elapsed_secs: 1.0,
            throughput: 0.0,
            last_error: last_error.map(|s| s.to_string()),
            fatal: false,
            errors,
        }
    }

    fn verdict_for(acc: SummaryAccumulator) -> RunVerdict {
        let run_id = RecordId::new("stress_test_run", "verdict-test");
        let cancel = Arc::new(AtomicBool::new(false));
        let tool = TestTool::StressKit {
            stressor: "gpu_matmul".to_string(),
        };
        acc.into_verdict(&run_id, &cancel, 60.0, &tool, Vec::new())
    }

    /// One finished stage carrying only the supplied verdict.
    fn outcome_with(verdict: StageVerdict) -> StageOutcome {
        StageOutcome {
            summary: ScenarioStageSummary::default(),
            verdict: Some(verdict),
        }
    }

    #[test]
    fn gpu_error_messages_classify_as_gpu() {
        for msg in [
            "gpu: GPU device failed (1 error(s)): Unknown: Device is lost",
            "gpu_matmul: GPU device failed (3 error(s)): Validation Error",
            "gpu_vram: 3 consecutive readback failures; aborting stage",
            "gpu_pcie acquire failed: no GPU adapters found",
            "readback map failed (1/3)",
            "psu: inconclusive - GPU unavailable, GPU leg never ran (no GPU adapters found)",
            "psu: inconclusive - GPU leg stopped (GPU device failed (1 error(s)): Unknown: Device is lost)",
            "psu: inconclusive - GPU leg stopped (queue stalled, 3 consecutive wait timeouts)",
            "psu_transient: GPU unavailable (no GPU adapters found); the pulsed +12V load never ran, so this is not a valid PSU transient test",
            "psu_transient: GPU leg stopped responding, device wait timed out (Timeout)",
            "psu_transient: GPU unavailable for pulsed load; one submit takes 140ms against a 100ms ON window (measured duty cycle 72%), so the +12V transient is not being generated",
            "psu_transient: GPU leg stopped (device lost); the pulsed +12V load ended, so this is not a valid PSU transient test",
            "stress-kit built without 'gpu' feature",
        ] {
            assert!(is_gpu_error_message(msg), "should classify as GPU: {msg}");
        }
    }

    #[test]
    fn disk_error_messages_do_not_classify_as_gpu() {
        for msg in [
            "disk thread 0: Access is denied. (os error 5)",
            "disk thread 3: The device is not ready. (os error 21)",
        ] {
            assert!(!is_gpu_error_message(msg), "should not classify as GPU: {msg}");
        }
    }

    #[test]
    fn gpu_device_lost_maps_to_gpu_failure_kind() {
        let mut acc = SummaryAccumulator::default();
        let snapshot = TelemetrySnapshot::default();
        let msg = "gpu: GPU device failed (1 error(s)): Unknown: Device is lost";
        for _ in 0..3 {
            acc.absorb(&tick(Some(msg), 0), &snapshot, "GFLOPS");
        }
        assert_eq!(acc.gpu_device_errors, 1, "repeated message counts once");
        assert_eq!(acc.disk_io_errors, 0);

        let verdict = verdict_for(acc);
        assert_eq!(verdict.result, RunResult::Fail);
        assert_eq!(verdict.failure_mode.kind(), "gpu_device_lost");
        assert_eq!(
            verdict.failure_mode,
            FailureMode::GpuDeviceLost { message: msg.to_string() }
        );
    }

    #[test]
    fn disk_errors_still_map_to_disk_io_error() {
        let mut acc = SummaryAccumulator::default();
        let snapshot = TelemetrySnapshot::default();
        let msg = "disk thread 0: Access is denied. (os error 5)";
        acc.absorb(&tick(Some(msg), 0), &snapshot, "MiB/s");
        assert_eq!(acc.disk_io_errors, 1);
        assert_eq!(acc.gpu_device_errors, 0);

        let verdict = verdict_for(acc);
        assert_eq!(verdict.result, RunResult::Fail);
        assert_eq!(
            verdict.failure_mode,
            FailureMode::DiskIoError { message: msg.to_string() }
        );
    }

    #[test]
    fn gpu_failure_outranks_disk_error_and_keeps_gpu_message() {
        let mut acc = SummaryAccumulator::default();
        let snapshot = TelemetrySnapshot::default();
        let disk_msg = "disk thread 1: write failed";
        let gpu_msg = "gpu_matmul: GPU device failed (1 error(s)): Unknown: Device is lost";
        acc.absorb(&tick(Some(disk_msg), 0), &snapshot, "MiB/s");
        acc.absorb(&tick(Some(gpu_msg), 0), &snapshot, "GFLOPS");
        assert_eq!(acc.disk_io_errors, 1);
        assert_eq!(acc.gpu_device_errors, 1);

        let verdict = verdict_for(acc);
        assert_eq!(verdict.failure_mode.kind(), "gpu_device_lost");
        assert_eq!(
            verdict.failure_mode,
            FailureMode::GpuDeviceLost { message: gpu_msg.to_string() }
        );
    }

    /// The reported symptom: a stage whose stressor aborted must not persist
    /// `result: "pass"` next to `had_error: true`.
    #[test]
    fn fatal_stage_summary_reports_fail_not_pass() {
        let reason = "psu: inconclusive - GPU unavailable, GPU leg never ran";
        let rules = VerdictRules::certification();
        let snapshot = TelemetrySnapshot::default();
        let mut stats = StageStats::begin(0, "psu", Stressor::Psu, &snapshot);
        for _ in 0..40 {
            stats.absorb_tick(&tick(None, 0), &snapshot, &rules);
        }
        stats.absorb_final(&Metrics {
            elapsed_secs: 40.0,
            throughput: 120.0,
            last_error: Some(reason.to_string()),
            fatal: true,
            errors: 0,
        });

        let verdict = stage_verdict_for(&stats, &Some(rules.clone()), &rules)
            .expect("rules attached, verdict expected");
        assert!(!verdict.pass);

        let summary = stage_summary_from_stats(
            &stats,
            Stressor::Psu,
            8,
            60,
            40.0,
            Some(120.0),
            Some(120.0),
            "GFLOPS",
            true,
            Some(reason.to_string()),
            Some(&verdict),
        );
        assert_eq!(summary.result.as_deref(), Some("fail"));
        assert!(summary.had_error);
        assert!(
            summary.violations.iter().any(|v| v.contains(reason)),
            "violations: {:?}",
            summary.violations
        );
    }

    /// A legacy run with no policy still gets a verdict when the load aborted,
    /// so `StageVerdict`/`StageOutcome` consumers can't read it as clean.
    #[test]
    fn fatal_stage_gets_a_verdict_without_rules() {
        let snapshot = TelemetrySnapshot::default();
        let effective = VerdictRules::default();
        let mut clean = StageStats::begin(0, "cpu", Stressor::Cpu, &snapshot);
        clean.absorb_tick(&tick(None, 0), &snapshot, &effective);
        assert!(stage_verdict_for(&clean, &None, &effective).is_none());

        let mut aborted = StageStats::begin(0, "psu", Stressor::Psu, &snapshot);
        aborted.absorb_final(&Metrics {
            elapsed_secs: 1.0,
            throughput: 0.0,
            last_error: Some("psu: GPU leg never ran".to_string()),
            fatal: true,
            errors: 0,
        });
        let verdict = stage_verdict_for(&aborted, &None, &effective)
            .expect("fatal abort must produce a verdict");
        assert!(!verdict.pass);
    }

    /// A load that never ran is a tooling error, not proof of bad hardware.
    #[test]
    fn fatal_abort_maps_to_app_error_not_a_hardware_fault() {
        let reason = "psu: inconclusive - GPU unavailable, GPU leg never ran";
        let mut summary = RunSummary::default();
        let outcomes = vec![outcome_with(StageVerdict {
            index: 0,
            label: "psu".to_string(),
            pass: false,
            violations: vec![RuleViolation::FatalAbort {
                reason: Some(reason.to_string()),
            }],
            warnings: Vec::new(),
        })];

        let mode = rules_failure_mode(&outcomes, &mut summary).expect("failure mode expected");
        assert_eq!(mode.kind(), "app_error");
        assert_eq!(
            mode,
            FailureMode::AppError {
                exit_code: None,
                message: reason.to_string(),
            }
        );
        assert!(!summary.vrm_throttle_detected);
        assert!(!summary.thermal_throttle_detected);
    }

    /// Independent hardware evidence in the same stage still wins the headline.
    #[test]
    fn real_hardware_evidence_outranks_a_fatal_abort() {
        let mut summary = RunSummary::default();
        let outcomes = vec![outcome_with(StageVerdict {
            index: 0,
            label: "psu".to_string(),
            pass: false,
            violations: vec![
                RuleViolation::FatalAbort {
                    reason: Some("psu: GPU leg never ran".to_string()),
                },
                RuleViolation::Tdr { delta: 2 },
            ],
            warnings: Vec::new(),
        })];
        let mode = rules_failure_mode(&outcomes, &mut summary).expect("failure mode expected");
        assert_eq!(mode, FailureMode::Tdr { count: 2 });
    }

    #[test]
    fn clean_run_still_passes() {
        let mut acc = SummaryAccumulator::default();
        let snapshot = TelemetrySnapshot::default();
        acc.absorb(&tick(None, 0), &snapshot, "GFLOPS");

        let verdict = verdict_for(acc);
        assert_eq!(verdict.result, RunResult::Pass);
        assert_eq!(verdict.failure_mode, FailureMode::None);
    }
}
