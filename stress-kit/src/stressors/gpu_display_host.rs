//! Parent side of the out-of-process `gpu_display` run.
//!
//! `gpu_display` is the only stressor that presents, and presenting is what
//! resets the display miniport. A reset invalidates every graphics context in
//! the process, so an in-process run takes the host app's own renderer down
//! with it — the app paints white and faults when its context is torn down.
//! Running the load in a child keeps that blast radius off the app, which also
//! means the app is still alive to report the fault it just caused.
//!
//! The child streams [`Metrics`] to stdout as one JSON object per line. A child
//! that dies mid-run IS the fault and is reported as one; a child that never
//! got going is a tooling problem and is reported as inconclusive.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::{DisplayOptions, Metrics};

/// Helper executable, shipped beside the host app.
const HELPER_STEM: &str = "stresskit-display";
/// Gap between child liveness checks.
const SUPERVISE_POLL: Duration = Duration::from_millis(200);
/// A child that dies sooner than this never ran the load.
const STARTUP_GRACE: Duration = Duration::from_secs(3);
/// Slack added to the stage timeout before the child self-terminates.
const CHILD_BELT_SLACK: Duration = Duration::from_secs(30);

/// Runs the display load, isolated in a child process when one is available.
pub(super) fn run(
    options: DisplayOptions,
    timeout: Option<Duration>,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    if !isolation_requested() {
        log::info!(
            "[stress-kit/gpu_display] STRESSKIT_DISPLAY_ISOLATE=off, running in-process; a miniport \
             reset will take this process's graphics context with it"
        );
        return super::gpu_display::run(options, cancel, tx, started_at);
    }
    let Some(helper) = helper_path() else {
        log::warn!(
            "[stress-kit/gpu_display] no {HELPER_STEM} beside the current exe, running in-process; \
             a miniport reset will take this process's graphics context with it"
        );
        return super::gpu_display::run(options, cancel, tx, started_at);
    };
    if let Err(e) = run_isolated(&helper, options, timeout, cancel, tx, started_at) {
        log::warn!("[stress-kit/gpu_display] isolated run failed to start ({e}), running in-process");
        super::gpu_display::run(options, cancel, tx, started_at);
    }
}

/// `false` only when `STRESSKIT_DISPLAY_ISOLATE` names an off value.
fn isolation_requested() -> bool {
    isolation_from_env(std::env::var("STRESSKIT_DISPLAY_ISOLATE").ok().as_deref())
}

/// Isolation is the default; only an explicit off value disables it.
fn isolation_from_env(raw: Option<&str>) -> bool {
    !matches!(
        raw.unwrap_or_default().trim().to_ascii_lowercase().as_str(),
        "off" | "0" | "none" | "false"
    )
}

/// The helper beside the running executable, if it is there.
fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        format!("{HELPER_STEM}.exe")
    } else {
        HELPER_STEM.to_string()
    };
    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn run_isolated(
    helper: &PathBuf,
    options: DisplayOptions,
    timeout: Option<Duration>,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) -> std::io::Result<()> {
    let mut cmd = Command::new(helper);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    if let Some(policy) = options.modeset {
        cmd.args(["--modeset", policy.as_str()]);
    }
    if let Some(cap) = options.max_outputs {
        cmd.args(["--max-outputs", &cap.to_string()]);
    }
    // Belt so an orphaned child cannot keep presenting after the parent goes.
    let belt = timeout.unwrap_or(Duration::from_secs(3600)) + CHILD_BELT_SLACK;
    cmd.args(["--max-secs", &belt.as_secs().to_string()]);

    let mut child = cmd.spawn()?;
    log::info!(
        "[stress-kit/gpu_display] isolated child {} started (pid {})",
        helper.display(),
        child.id()
    );

    let stdout = child.stdout.take().expect("stdout was piped");
    let forward_tx = tx.clone();
    let pump = std::thread::Builder::new()
        .name("stress-kit-display-pump".into())
        .spawn(move || {
            let mut ticks = 0u64;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Metrics>(line) {
                    Ok(m) => {
                        ticks += 1;
                        if forward_tx.send(m).is_err() {
                            break;
                        }
                    }
                    Err(e) => log::debug!("[stress-kit/gpu_display] child line ignored ({e}): {line}"),
                }
            }
            ticks
        })?;

    let stderr = child.stderr.take().expect("stderr was piped");
    std::thread::Builder::new()
        .name("stress-kit-display-err".into())
        .spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::warn!("[stress-kit/gpu_display] child stderr: {line}");
            }
        })?;

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(SUPERVISE_POLL);
    };

    let ticks = pump.join().unwrap_or(0);
    let ran = started_at.elapsed();
    let cancelled = cancel.load(Ordering::Relaxed);

    if cancelled || status.success() {
        log::info!(
            "[stress-kit/gpu_display] isolated child finished after {:.1}s, {ticks} tick(s)",
            ran.as_secs_f32()
        );
        return Ok(());
    }

    // Died before the load could have started: tooling, not the hardware.
    if ran < STARTUP_GRACE || ticks == 0 {
        send_fatal(
            tx,
            started_at,
            format!(
                "gpu_display: inconclusive - the isolated display host exited {status} after \
                 {:.1}s having produced {ticks} tick(s); the load never ran",
                ran.as_secs_f32()
            ),
            0,
        );
        return Ok(());
    }

    send_fatal(
        tx,
        started_at,
        format!(
            "gpu_display: the isolated display host was killed mid-run ({status}) after {:.1}s; a \
             display miniport reset takes the presenting process with it, so this counts as a \
             display-path fault. The app survived because the load was out of process.",
            ran.as_secs_f32()
        ),
        1,
    );
    Ok(())
}

fn send_fatal(tx: &mpsc::Sender<Metrics>, started_at: Instant, reason: String, errors: u64) {
    log::error!("[stress-kit/gpu_display] {reason}");
    let _ = tx.send(Metrics {
        elapsed_secs: started_at.elapsed().as_secs_f64(),
        throughput: 0.0,
        last_error: Some(reason),
        fatal: true,
        errors,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_is_the_default() {
        assert!(isolation_from_env(None));
        assert!(isolation_from_env(Some("")));
        assert!(isolation_from_env(Some("on")));
        assert!(isolation_from_env(Some("1")));
    }

    #[test]
    fn isolation_off_is_explicit_only() {
        for raw in ["off", "OFF", " 0 ", "none", "false"] {
            assert!(!isolation_from_env(Some(raw)), "{raw}");
        }
    }

    /// A dead child must be reported as a display fault, not as a data error
    /// from the load, and a stillborn one must read inconclusive so the runner
    /// scores it unproven rather than as evidence against the hardware.
    #[test]
    fn fatal_ticks_carry_the_right_shape() {
        let (tx, rx) = mpsc::channel();
        send_fatal(&tx, Instant::now(), "gpu_display: inconclusive - never ran".into(), 0);
        send_fatal(&tx, Instant::now(), "gpu_display: killed mid-run".into(), 1);

        let stillborn = rx.recv().expect("first tick");
        assert!(stillborn.fatal);
        assert_eq!(stillborn.errors, 0);
        assert!(stillborn.last_error.unwrap().contains("inconclusive - "));

        let killed = rx.recv().expect("second tick");
        assert!(killed.fatal);
        assert_eq!(killed.errors, 1, "a mid-run death is the fault, so it counts");
    }
}
