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
    /// Final update.  After this, `is_running` returns false.
    Finished(RunVerdict),
    /// Non-fatal warning — surface in the UI but the run continues.
    Warning { message: String },
    /// Fatal error — followed by a `Finished` update with `RunResult::Aborted`.
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct RunVerdict {
    pub run_id: RecordId,
    pub result: RunResult,
    pub finish_reason: DbFinishReason,
    pub failure_mode: FailureMode,
    pub summary: RunSummary,
    pub duration_secs: f64,
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

        let join = thread::Builder::new()
            .name("stress-runner-controller".into())
            .spawn(move || {
                worker(
                    spec,
                    telemetry,
                    cancel_worker,
                    update_tx,
                    run_id_worker,
                    running_worker,
                );
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

    // ---- 0. Hardware-component middleware ----
    // Upsert `hardware_component` rows for the CPU + every GPU this machine
    // reports, then patch the spec so the run row links them via
    // `target_component` / `touched_components`. Best-effort: failures are
    // logged but don't abort the run — the row would just have a `NONE`
    // target_component which is permitted by the schema.
    let snapshot = telemetry.snapshot();
    let (cpu_component, all_components) =
        crate::hardware::ensure_components_from_snapshot(&snapshot);
    if spec.target_component.is_none() {
        spec.target_component = cpu_component;
    }
    if spec.touched_components.is_empty() {
        spec.touched_components = all_components;
    }

    // ---- 1. Build + persist the StressTestRun row ----
    let run = build_run(&spec);
    let run_id_clone = run.id.clone();

    // Clone into the async block so the future is `'static` (required by
    // the host-runtime path, which spawns + awaits via a oneshot channel).
    let run_for_create = run.clone();
    match runtime::block_on(async move { StressTestRun::create(&run_for_create).await }) {
        Ok(_) => {
            if let Ok(mut guard) = run_id.lock() {
                *guard = Some(run_id_clone.clone());
            }
            send(&update_tx, RunUpdate::Started {
                run_id: run_id_clone.clone(),
            });
        }
        Err(err) => {
            send(
                &update_tx,
                RunUpdate::Error {
                    message: format!("failed to create stress_test_run row: {err}"),
                },
            );
            running.store(false, Ordering::SeqCst);
            return;
        }
    }

    // ---- 2. Track state for the final summary ----
    let mut acc = SummaryAccumulator::default();
    let mut stages: Vec<ScenarioStageSummary> = Vec::new();

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
                throughput_unit,
                &telemetry,
                &cancel,
                &update_tx,
                &run_id_clone,
                &mut acc,
                duration_secs,
                started_at,
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
                &mut stages,
            );
        }
    }

    // ---- 4. Finalize ----
    let duration_secs = started_at.elapsed().as_secs_f64();
    let verdict = acc.into_verdict(&run_id_clone, &cancel, duration_secs, &spec.tool);

    // Best-effort scenario_stages write would go here if we needed it on the
    // run row.  The finalize call below only updates summary + verdict; if a
    // future caller wants per-stage rows on the run record, add an UPDATE
    // here that sets `scenario_stages = $stages`.
    let _ = stages;

    // Same `'static` requirement as the create call — clone everything into
    // an owned async block.
    let finalize_id = run_id_clone.clone();
    let finalize_result = verdict.result;
    let finalize_reason = verdict.finish_reason;
    let finalize_failure = verdict.failure_mode.clone();
    let finalize_summary = verdict.summary.clone();
    let res = runtime::block_on(async move {
        StressTestRun::finalize(
            &finalize_id,
            finalize_result,
            finalize_reason,
            finalize_failure,
            finalize_summary,
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
    run
}

/// Drive a single-stressor run: tick at 1 Hz, sample telemetry, batch metric
/// writes, forward updates to the UI.
fn drive_single(
    session: &StressSession,
    stressor: Stressor,
    throughput_unit: &'static str,
    telemetry: &Arc<TelemetryAgent>,
    cancel: &Arc<AtomicBool>,
    update_tx: &Sender<RunUpdate>,
    run_id: &RecordId,
    acc: &mut SummaryAccumulator,
    duration_secs: Option<u64>,
    started_at: Instant,
) {
    let mut last_tick = Instant::now();
    let mut latest_metrics = Metrics {
        elapsed_secs: 0.0,
        throughput: 0.0,
        last_error: None,
    };
    let mut metric_batch: Vec<StressTestMetric> = Vec::with_capacity(METRIC_BATCH_SIZE);

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
            latest_metrics = m;
        }

        if last_tick.elapsed() >= TICK_INTERVAL {
            last_tick = Instant::now();
            let snapshot = telemetry.snapshot();
            acc.absorb(&latest_metrics, &snapshot, throughput_unit);

            let metric = metric_from_snapshot(
                run_id.clone(),
                &snapshot,
                Some(latest_metrics.throughput),
                Some(throughput_unit),
                latest_metrics.last_error.as_deref(),
                None,
                None,
            );
            metric_batch.push(metric);
            if metric_batch.len() >= METRIC_BATCH_SIZE {
                flush_metrics(&mut metric_batch);
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
        let _ = stressor; // captured for future use (e.g. failure rubric)
    }

    flush_metrics(&mut metric_batch);
}

/// Drive a scenario run.  ScenarioRunner already does most of the work — we
/// just translate its events into UI updates + DB events, and overlay our
/// own 1 Hz telemetry sampling for the per-tick metric rows.
fn drive_scenario(
    runner: &ScenarioRunner,
    stage_specs: &[RunStage],
    telemetry: &Arc<TelemetryAgent>,
    cancel: &Arc<AtomicBool>,
    update_tx: &Sender<RunUpdate>,
    run_id: &RecordId,
    acc: &mut SummaryAccumulator,
    stages: &mut Vec<ScenarioStageSummary>,
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
    let mut latest_metrics = Metrics {
        elapsed_secs: 0.0,
        throughput: 0.0,
        last_error: None,
    };
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
                    let stressor_label = stage_specs
                        .get(index)
                        .map(|s| s.stressor.label())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    stages.push(ScenarioStageSummary {
                        index: index as u32,
                        label: current_stage_label.clone().unwrap_or_default(),
                        stressor: stressor_label,
                        threads: stage_specs.get(index).map(|s| s.threads as u32).unwrap_or(0),
                        duration_planned_secs: planned,
                        duration_actual_secs: elapsed,
                        peak_throughput: current_stage_peak_throughput,
                        avg_throughput: avg,
                        throughput_unit: current_unit.to_string(),
                        had_error: current_stage_had_error,
                        last_error: current_stage_last_error.clone(),
                    });

                    persist_event(
                        run_id,
                        DbEventKind::StageFinished,
                        "stress-kit",
                        current_stage_label.clone(),
                        None,
                    );

                    send(update_tx, RunUpdate::StageFinished { index });
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
            acc.absorb(&latest_metrics, &snapshot, current_unit);

            let metric = metric_from_snapshot(
                run_id.clone(),
                &snapshot,
                Some(latest_metrics.throughput),
                Some(current_unit),
                latest_metrics.last_error.as_deref(),
                current_stage_index.map(|i| i as u32),
                current_stage_label.clone(),
            );
            metric_batch.push(metric);
            if metric_batch.len() >= METRIC_BATCH_SIZE {
                flush_metrics(&mut metric_batch);
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

    flush_metrics(&mut metric_batch);
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

fn flush_metrics(batch: &mut Vec<StressTestMetric>) {
    if batch.is_empty() {
        return;
    }
    let take = std::mem::take(batch);
    runtime::spawn(async move {
        for m in &take {
            if let Err(err) = StressTestMetric::create(m).await {
                log::warn!("stress-runner: failed to persist metric: {err}");
            }
        }
    });
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
    disk_io_errors: u32,
    last_error: Option<String>,
    scenario_finish: Option<DbFinishReason>,
}

impl SummaryAccumulator {
    fn absorb(&mut self, metrics: &Metrics, snapshot: &TelemetrySnapshot, unit: &'static str) {
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
        if let Some(err) = &metrics.last_error {
            self.disk_io_errors = self.disk_io_errors.saturating_add(1);
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
            max_power_w: None,
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
            tdr_count: 0,
            bsod_detected: false,
            bsod_code: None,
            disk_io_errors: self.disk_io_errors,
            memory_errors: 0,
        }
    }

    fn into_verdict(
        self,
        run_id: &RecordId,
        cancel: &Arc<AtomicBool>,
        duration_secs: f64,
        _tool: &TestTool,
    ) -> RunVerdict {
        let summary = self.into_summary();
        let cancelled = cancel.load(Ordering::Relaxed);
        let had_failure = summary.whea_delta_count > 0
            || summary.disk_io_errors > 0
            || summary.bsod_detected
            || summary.thermal_throttle_detected;

        let (result, finish_reason, failure_mode) = if had_failure {
            let mode = if summary.whea_delta_count > 0 {
                FailureMode::WheaError {
                    count: summary.whea_delta_count,
                }
            } else if summary.disk_io_errors > 0 {
                FailureMode::DiskIoError {
                    message: self.last_error.clone().unwrap_or_default(),
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
        }
    }
}
