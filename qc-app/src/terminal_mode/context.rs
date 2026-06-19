use std::sync::Arc;

use database::schema::RecordId;
use stress_kit::telemetry::{TelemetryAgent, TelemetrySnapshot};
use stress_runner::RunVerdict;

/// Shared blackboard for the terminal-mode QC app, cloned via `Arc<Mutex<>>`
/// into the tabs and the headless tick loop.
#[derive(Default)]
pub struct QcContext {
    /// Latest telemetry tick from the background `HwSampler`.
    pub snapshot: Option<TelemetrySnapshot>,
    /// Shared telemetry agent handle, set once the sampler starts. Stress runs
    /// hand this to `RunController::start`.
    pub telemetry: Option<Arc<TelemetryAgent>>,
    /// Stable `computer:<machine_id>` record new stress runs link to.
    pub computer: Option<RecordId>,
    /// `(service_order, tech-name)` stamped onto stress runs while an order is open.
    pub order_context: Option<(RecordId, String)>,
    /// Verdict of the most recent stress run, read into the QC report.
    pub last_verdict: Option<RunVerdict>,
    /// Preset label of the most recent stress run.
    pub last_preset: Option<String>,
}
