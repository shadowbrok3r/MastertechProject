//! Blocking driver for hosts that run stress tests off the UI thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use stress_kit::telemetry::TelemetryAgent;

use crate::{RunController, RunSpec, RunUpdate, RunVerdict};

/// Drive a run to completion on the calling thread. Persists DB rows via the
/// controller worker; returns the final verdict when `Finished` fires.
pub fn drive_blocking(
    spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    on_update: impl FnMut(RunUpdate),
) -> Option<RunVerdict> {
    drive_blocking_cancellable(spec, telemetry, Arc::new(AtomicBool::new(false)), on_update)
}

/// As [`drive_blocking`], but a host-owned flag can stop the run in flight.
///
/// `drive_blocking` builds the controller internally, so its cancel was
/// unreachable and a caller asking to stop could only abandon the thread while
/// the run kept loading the machine. The flag is polled on the same cadence as
/// the update drain, and the run is left to finish its own teardown so the
/// verdict and `stress_test_run` row still land.
pub fn drive_blocking_cancellable(
    spec: RunSpec,
    telemetry: Arc<TelemetryAgent>,
    cancel: Arc<AtomicBool>,
    mut on_update: impl FnMut(RunUpdate),
) -> Option<RunVerdict> {
    let controller = RunController::start(spec, telemetry);
    let mut verdict = None;
    let mut asked_to_stop = false;
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
        if !asked_to_stop && cancel.load(Ordering::Relaxed) {
            controller.stop();
            asked_to_stop = true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    verdict
}
