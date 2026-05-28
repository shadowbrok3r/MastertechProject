//! Run-lifecycle glue between [`stress_kit`] and the [`database`] crate.
//!
//! `stress-kit` knows how to run a stress test and emit telemetry.  `database`
//! knows how to persist a `stress_test_run` row.  Neither knows about the
//! other — this crate is the only place that knows about both.
//!
//! # What lives here
//!
//! - [`RunSpec`]: declarative description of "run this tool against this
//!   computer for this long".
//! - [`RunController`]: synchronous handle the host app drives every frame.
//!   `start` spawns a worker thread that wraps either a stress-kit
//!   `StressSession` (single stressor) or `ScenarioRunner` (multi-stage),
//!   samples the shared `TelemetryAgent` at ~1 Hz, and forwards
//!   [`RunUpdate`] events to the UI.  A background tokio task persists
//!   `StressTestRun` / `StressTestMetric` / `StressTestEvent` rows.
//! - [`RunUpdate`]: events to render — stage transitions, ticks (with both
//!   stress-kit metrics and a `TelemetrySnapshot`), and the final verdict.
//!
//! # Host app integration
//!
//! - **qc-app** drives this from its existing egui stress panel; the panel
//!   stops calling `StressSession::start` / `ScenarioRunner::start` directly
//!   and goes through [`RunController::start`] instead.  DB persistence is
//!   then free.
//! - **Mastertech4.0** uses the same `RunController` from the terminal-mode
//!   scripts tab, with a ratatui UI on top.

mod controller;
mod drive;
mod gpu_probe;
mod hardware;
mod mapping;
mod runtime;
mod script_catalog;

use std::sync::atomic::{AtomicUsize, Ordering};

/// Process-wide count of active stress runs. Incremented at the top of
/// `RunController::worker` and decremented when the worker thread
/// returns (via the RAII `StressActiveGuard`, so panics still
/// decrement). Read by `is_stress_active()` from any thread.
///
/// Consumers (e.g. Mastertech4.0's LiveData loop) use this to throttle
/// chatty TCP traffic while a stress test is hogging the connection —
/// telemetry samples drop from 400 ms to a coarser cadence so WASM
/// plugin RPC and other Cmd traffic stay responsive.
pub(crate) static STRESS_ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// `true` while at least one stress run is in flight in this process.
pub fn is_stress_active() -> bool {
    STRESS_ACTIVE.load(Ordering::Relaxed) > 0
}

/// How many stress runs are currently in flight. Useful for diagnostics
/// when scenarios spawn multiple workers.
pub fn stress_active_count() -> usize {
    STRESS_ACTIVE.load(Ordering::Relaxed)
}

/// RAII guard: decrements `STRESS_ACTIVE` on drop. The worker thread
/// holds one for its lifetime so the counter goes back down regardless
/// of whether the run finished normally, errored out, or panicked
/// (the `catch_unwind` wrapper in `RunController::start` still lets the
/// guard's Drop fire).
pub(crate) struct StressActiveGuard;

impl StressActiveGuard {
    pub(crate) fn new() -> Self {
        STRESS_ACTIVE.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for StressActiveGuard {
    fn drop(&mut self) {
        STRESS_ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

pub use hardware::{ensure_components_for_run, ensure_components_from_snapshot};

pub use controller::{RunController, RunPlan, RunSpec, RunStage, RunUpdate, RunVerdict};
pub use drive::drive_blocking;
pub use gpu_probe::{gpu_probe_spec, gpu_probe_stages, GPU_PROBE_PRESET};
pub use script_catalog::{build_stress_script_spec, is_stress_script, STRESS_SCRIPT_NAMES};
pub use mapping::{
    compute_machine_id, computer_record_key, default_target_kind, generate_client_hash,
    local_computer_record, metric_from_snapshot, stressor_from_db, stressor_to_db,
};
pub use runtime::set_runtime_handle;

// Re-export the most-used database + stress-kit types so callers only need to
// depend on `stress-runner` for the common case.
pub use database::schema::{
    BiosSettings, DriverVersions, FailureMode, FinishReason as DbFinishReason, HardwareComponent,
    HardwareKind, RecordId, RunResult, RunSummary, ScenarioStageSummary, StressKitStressor,
    StressTestEvent, StressTestMetric, StressTestRun, TargetKind, TestTool,
};

pub use stress_kit::{
    scenario::{FinishReason as StressKitFinishReason, ScenarioEvent},
    telemetry::{TelemetryAgent, TelemetrySnapshot},
    Metrics, StressConfig, Stressor,
};
