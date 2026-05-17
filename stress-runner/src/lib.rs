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
mod mapping;
mod runtime;

pub use controller::{RunController, RunPlan, RunSpec, RunStage, RunUpdate, RunVerdict};
pub use mapping::{
    compute_machine_id, default_target_kind, metric_from_snapshot, stressor_from_db,
    stressor_to_db,
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
