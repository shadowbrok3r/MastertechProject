//! RemoteExec: long-running privileged jobs owned by the client.
//!
//! The admin submits a job and returns immediately; the client owns the
//! process, buffers its output, and keeps the exit record so a reconnecting
//! admin can still collect it. This exists because the WASM plugin path is
//! capped by the PluginManager watchdog, which makes anything longer than that
//! unreachable through a plugin.
//!
//! Every entry point is gated by [`gate`], which refuses work unless a
//! technician armed the client against a diagnostic session and the consent
//! banner is currently painting.

pub mod banner_egui;
pub mod banner_tui;
pub mod exec_shell;
pub mod gate;
pub mod job;
pub mod journal;
pub mod registry;
pub mod winjob;

use displays::remote_exec::{
    GateStatus, JobSignal, JobSnapshot, RemoteExecCapabilities, RemoteJobSpec, RiskTier, ShellKind,
    REMOTE_EXEC_PROTOCOL_VERSION,
};

use job::JobHandle;

/// Default output budget returned to the admin so it can pace polling.
const DEFAULT_TAIL_BYTES: u32 = 256 * 1024;

/// egui context used to wake the UI when the gate is armed.
///
/// Without this the interlock deadlocks: no job runs until the consent banner
/// paints, but the banner only keeps itself repainting once it is already
/// painting — so arming an idle egui client would never wake it.
static REPAINT: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();

/// Registered once by the egui app. Terminal mode does not need it: its render
/// loop runs continuously.
pub fn set_repaint_handle(ctx: eframe::egui::Context) {
    let _ = REPAINT.set(ctx);
}

fn wake_ui() {
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

/// Last gated screen capture or input injection, so the banner can say the
/// screen is being watched rather than only that a session is open.
static SCREEN_ACTIVITY: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);

/// Activity within this window counts as "live" on the banner.
///
/// Generous on purpose: a screen-control loop pauses between actions while the
/// operator decides what to do next, and an indicator that blinks out during
/// those gaps tells someone glancing at the machine that nobody is watching.
/// Over-reporting is the safe direction for a consent light.
const SCREEN_ACTIVE_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

pub fn note_screen_activity() {
    if let Ok(mut g) = SCREEN_ACTIVITY.lock() {
        *g = Some(std::time::Instant::now());
    }
    wake_ui();
}

pub fn screen_is_live() -> bool {
    SCREEN_ACTIVITY
        .lock()
        .ok()
        .and_then(|g| *g)
        .is_some_and(|t| t.elapsed() <= SCREEN_ACTIVE_WINDOW)
}

/// What this build supports.
pub fn capabilities() -> RemoteExecCapabilities {
    RemoteExecCapabilities {
        job_kinds: vec!["shell".to_string()],
        shells: vec![ShellKind::PowerShell, ShellKind::Pwsh, ShellKind::Cmd],
        protocol_version: REMOTE_EXEC_PROTOCOL_VERSION,
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        max_concurrent_jobs: registry::MAX_CONCURRENT_JOBS,
        ring_bytes: job::RING_BYTES,
        default_timeout_secs: exec_shell::DEFAULT_TIMEOUT_SECS,
    }
}

pub fn arm(
    session_id: String,
    tech: String,
    diagnostic_session_id: String,
    reason: String,
    ttl_secs: u64,
) -> GateStatus {
    journal::record_gate("armed", &tech, &diagnostic_session_id, Some(ttl_secs));
    let status = gate::arm(session_id, tech, diagnostic_session_id, reason, ttl_secs);
    wake_ui();
    status
}

pub fn disarm(kill_running: bool) -> GateStatus {
    let killed = if kill_running { registry::cancel_all() } else { 0 };
    journal::record_gate("disarmed", "", "", None);
    if killed > 0 {
        log::warn!("[remote_exec] disarm terminated {killed} running job(s)");
    }
    gate::disarm();
    wake_ui();
    gate::status(registry::running_count())
}

fn describe(spec: &RemoteJobSpec) -> String {
    match spec {
        RemoteJobSpec::Shell { shell, script, .. } => {
            let first = script
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("<empty script>");
            let head: String = first.chars().take(120).collect();
            format!("{shell:?}: {head}")
        }
    }
}

/// Submits a job. Returns as soon as it is accepted; the work runs detached.
pub fn start(
    job_id: String,
    tech: String,
    reason: String,
    risk: RiskTier,
    spec: RemoteJobSpec,
) -> Result<JobSnapshot, String> {
    if let Err(why) = gate::check_admits_job() {
        journal::record_denied(&job_id, &tech, &why);
        return Err(why);
    }

    // A destructive job on a customer machine must carry a stated reason.
    if matches!(risk, RiskTier::Destructive) && reason.trim().is_empty() {
        let why = "destructive jobs require a non-empty reason".to_string();
        journal::record_denied(&job_id, &tech, &why);
        return Err(why);
    }

    let summary = describe(&spec);
    let handle = JobHandle::new(job_id.clone(), summary.clone(), risk, reason.clone(), tech.clone());
    registry::insert(handle.clone())?;

    journal::record_submitted(
        &job_id,
        &tech,
        &reason,
        &format!("{risk:?}"),
        &summary,
        gate::banner_info()
            .as_ref()
            .map(|b| b.diagnostic_session_id.as_str()),
    );
    log::warn!("[remote_exec] job {job_id} submitted by {tech} ({risk:?}): {summary}");

    let driver = handle.clone();
    tokio::spawn(async move {
        exec_shell::run(driver, spec).await;
    });

    Ok(handle.snapshot(None, 0))
}

/// Cancel, kill or detach a running job. Idempotent on terminal jobs.
pub fn signal(job_id: &str, signal: JobSignal) -> Result<JobSnapshot, String> {
    let Some(handle) = registry::get(job_id) else {
        return Err(format!("unknown job {job_id}"));
    };
    journal::record_signal(job_id, &format!("{signal:?}"));

    match signal {
        JobSignal::Cancel | JobSignal::Kill => {
            handle
                .cancel
                .store(true, std::sync::atomic::Ordering::SeqCst);
            handle.tree.terminate();
        }
        // Detach leaves the process running; the client keeps owning it.
        JobSignal::Detach => {}
    }
    Ok(handle.snapshot(None, 0))
}

/// Read job state and buffered output.
pub fn query(
    job_id: Option<&str>,
    from_seq: Option<u64>,
    max_bytes: Option<u32>,
) -> Vec<JobSnapshot> {
    registry::snapshot(job_id, from_seq, max_bytes.unwrap_or(DEFAULT_TAIL_BYTES))
}

pub fn status() -> GateStatus {
    gate::status(registry::running_count())
}

/// Called once at client startup: a job the registry still calls running
/// belongs to a process that no longer exists.
pub fn recover_on_start() {
    let n = registry::orphan_stale();
    if n > 0 {
        log::warn!("[remote_exec] marked {n} stale job(s) Orphaned at startup; none resumed");
    }
}
