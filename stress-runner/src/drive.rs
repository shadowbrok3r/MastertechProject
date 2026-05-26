//! Blocking driver for hosts that run stress tests off the UI thread.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use stress_kit::telemetry::TelemetryAgent;

use crate::{RunController, RunSpec, RunUpdate, RunVerdict};

/// Drive a run to completion on the calling thread. Persists DB rows via the
/// controller worker; returns the final verdict when `Finished` fires.
pub fn drive_blocking(
    spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    mut on_update: impl FnMut(RunUpdate),
) -> Option<RunVerdict> {
    let controller = RunController::start(spec, telemetry);
    let mut verdict = None;
    loop {
        for update in controller.poll() {
            if let RunUpdate::Finished(v) = &update {
                verdict = Some(v.clone());
            }
            on_update(update);
        }
        if !controller.is_running() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    verdict
}
