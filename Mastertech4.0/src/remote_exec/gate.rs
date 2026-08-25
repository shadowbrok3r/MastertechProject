//! Consent gate for RemoteExec.
//!
//! No job runs unless a technician has armed the gate against a diagnostic
//! session, and the client is currently rendering the consent banner. The
//! banner is an interlock, not an indicator: the UI stamps a heartbeat every
//! frame and a stale stamp refuses new work, so a panicked UI, a minimised
//! window or a code path that skipped the banner all fail closed.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use displays::remote_exec::GateStatus;

/// The banner must have painted within this window for the gate to admit work.
const BANNER_STALE_AFTER: Duration = Duration::from_secs(2);

/// Upper bound on an arm, regardless of what the admin asks for.
const MAX_TTL: Duration = Duration::from_secs(8 * 3600);

struct ArmedGate {
    session_id: String,
    tech: String,
    diagnostic_session_id: String,
    reason: String,
    expires_at: Instant,
    banner_heartbeat: Option<Instant>,
    banner_blocked: Option<(Instant, String)>,
}

static GATE: Mutex<Option<ArmedGate>> = Mutex::new(None);

/// Opens the gate. `ttl_secs` is clamped to [`MAX_TTL`].
pub fn arm(
    session_id: String,
    tech: String,
    diagnostic_session_id: String,
    reason: String,
    ttl_secs: u64,
) -> GateStatus {
    let ttl = Duration::from_secs(ttl_secs).min(MAX_TTL);
    let mut g = match GATE.lock() {
        Ok(g) => g,
        Err(_) => return denied("gate lock poisoned"),
    };
    *g = Some(ArmedGate {
        session_id,
        tech: tech.clone(),
        diagnostic_session_id: diagnostic_session_id.clone(),
        reason,
        expires_at: Instant::now() + ttl,
        banner_blocked: None,
        banner_heartbeat: None,
    });
    log::warn!(
        "[remote_exec] gate ARMED by {tech} for session {diagnostic_session_id}, ttl {}s",
        ttl.as_secs()
    );
    GateStatus {
        armed: true,
        tech: Some(tech),
        diagnostic_session_id: Some(diagnostic_session_id),
        expires_in_secs: Some(ttl.as_secs()),
        running_jobs: 0,
        denied_reason: None,
    }
}

/// Closes the gate. Always succeeds so a tech can never be locked out of
/// revoking their own access.
pub fn disarm() {
    if let Ok(mut g) = GATE.lock() {
        if let Some(prev) = g.take() {
            log::warn!("[remote_exec] gate DISARMED (was {} )", prev.tech);
        }
    }
}

/// Called by the UI every frame while the consent banner is painted.
pub fn stamp_banner() {
    if let Ok(mut g) = GATE.lock() {
        if let Some(gate) = g.as_mut() {
            gate.banner_heartbeat = Some(Instant::now());
            gate.banner_blocked = None;
        }
    }
}

/// Called by a UI that is armed but cannot paint the banner, so the refusal
/// names the cause instead of the generic never-painted message.
pub fn note_banner_blocked(reason: impl Into<String>) {
    if let Ok(mut g) = GATE.lock() {
        if let Some(gate) = g.as_mut() {
            gate.banner_blocked = Some((Instant::now(), reason.into()));
        }
    }
}

/// A block report filed within the staleness window, so a resized terminal
/// stops explaining a problem it no longer has.
fn blocked_reason(gate: &ArmedGate) -> Option<String> {
    gate.banner_blocked
        .as_ref()
        .filter(|(at, _)| at.elapsed() <= BANNER_STALE_AFTER)
        .map(|(_, why)| why.clone())
}

/// Banner half of the admission test, without the lease check. Takes no lock,
/// so a caller already holding one can reuse it.
fn banner_admission(gate: &ArmedGate) -> Result<(), String> {
    match gate.banner_heartbeat {
        Some(stamp) if stamp.elapsed() <= BANNER_STALE_AFTER => Ok(()),
        Some(stamp) => Err(blocked_reason(gate)
            .unwrap_or_else(|| format!("consent banner not rendering (last painted {:.1}s ago); refusing to run unattended on a customer machine", stamp.elapsed().as_secs_f32()))),
        None => Err(blocked_reason(gate).unwrap_or_else(|| "consent banner has not painted since the gate was armed; refusing to run unattended on a customer machine".to_string())),
    }
}

/// `Ok(())` when a job may start. The error names the failed precondition.
pub fn check_admits_job() -> Result<(), String> {
    let mut g = match GATE.lock() {
        Ok(g) => g,
        Err(_) => return Err("gate lock poisoned".into()),
    };
    let Some(gate) = g.as_mut() else {
        return Err("remote control is not armed; call remote_exec_arm first".into());
    };
    if Instant::now() >= gate.expires_at {
        let tech = gate.tech.clone();
        *g = None;
        log::warn!("[remote_exec] gate EXPIRED (was {tech}); refusing job");
        return Err("remote control lease expired; re-arm to continue".into());
    }
    banner_admission(gate)
}

/// Current gate state for reporting.
pub fn status(running_jobs: u32) -> GateStatus {
    match GATE.lock() {
        Ok(g) => match g.as_ref() {
            Some(gate) => GateStatus {
                armed: true,
                tech: Some(gate.tech.clone()),
                diagnostic_session_id: Some(gate.diagnostic_session_id.clone()),
                expires_in_secs: Some(
                    gate.expires_at
                        .saturating_duration_since(Instant::now())
                        .as_secs(),
                ),
                running_jobs,
                denied_reason: banner_admission(gate).err(),
            },
            None => GateStatus {
                armed: false,
                tech: None,
                diagnostic_session_id: None,
                expires_in_secs: None,
                running_jobs,
                denied_reason: None,
            },
        },
        Err(_) => denied("gate lock poisoned"),
    }
}

/// What the consent banner should display, or `None` when nothing is armed.
pub struct BannerInfo {
    pub tech: String,
    pub diagnostic_session_id: String,
    pub reason: String,
    pub expires_in_secs: u64,
}

pub fn banner_info() -> Option<BannerInfo> {
    let g = GATE.lock().ok()?;
    let gate = g.as_ref()?;
    Some(BannerInfo {
        tech: gate.tech.clone(),
        diagnostic_session_id: gate.diagnostic_session_id.clone(),
        reason: gate.reason.clone(),
        expires_in_secs: gate
            .expires_at
            .saturating_duration_since(Instant::now())
            .as_secs(),
    })
}

/// Session that armed the gate, for provenance on job records.
pub fn armed_session_id() -> Option<String> {
    let g = GATE.lock().ok()?;
    g.as_ref().map(|gate| gate.session_id.clone())
}

fn denied(reason: &str) -> GateStatus {
    GateStatus {
        armed: false,
        tech: None,
        diagnostic_session_id: None,
        expires_in_secs: None,
        running_jobs: 0,
        denied_reason: Some(reason.to_string()),
    }
}

/// Serialises tests that share the process-global gate.
#[cfg(test)]
pub(super) static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Backdates the banner heartbeat so staleness is testable without sleeping.
#[cfg(test)]
pub(super) fn backdate_banner(by: Duration) {
    if let Ok(mut g) = GATE.lock() {
        if let Some(gate) = g.as_mut() {
            if let Some(t) = gate.banner_heartbeat.and_then(|t| t.checked_sub(by)) {
                gate.banner_heartbeat = Some(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises against the shared gate and starts from a disarmed state.
    /// Poisoning is ignored so one failing test does not cascade.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        let held = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        disarm();
        held
    }

    fn arm_for_test() {
        arm("s".into(), "tech".into(), "diag".into(), "why".into(), 600);
    }

    #[test]
    fn unarmed_gate_refuses() {
        let _g = guard();
        assert!(check_admits_job().is_err());
    }

    #[test]
    fn armed_without_banner_fails_closed() {
        let _g = guard();
        arm_for_test();
        let err = check_admits_job().unwrap_err();
        assert!(
            err.contains("has not painted"),
            "an armed gate with no banner must refuse: {err}"
        );
    }

    #[test]
    fn armed_with_fresh_banner_admits() {
        let _g = guard();
        arm_for_test();
        stamp_banner();
        assert!(check_admits_job().is_ok());
    }

    #[test]
    fn a_stamp_older_than_the_stale_window_refuses() {
        let _g = guard();
        arm_for_test();
        stamp_banner();
        backdate_banner(BANNER_STALE_AFTER + Duration::from_millis(500));
        let err = check_admits_job().unwrap_err();
        assert!(err.contains("not rendering"), "{err}");
    }

    #[test]
    fn a_block_report_replaces_the_generic_refusal() {
        let _g = guard();
        arm_for_test();
        note_banner_blocked("the client's terminal is 8x1, too small");
        let err = check_admits_job().unwrap_err();
        assert!(err.contains("too small"), "{err}");
    }

    #[test]
    fn a_block_report_older_than_the_stale_window_is_ignored() {
        let _g = guard();
        arm_for_test();
        note_banner_blocked("stale complaint");
        if let Ok(mut g) = GATE.lock() {
            if let Some(gate) = g.as_mut() {
                if let Some((at, why)) = gate.banner_blocked.take() {
                    let old = at.checked_sub(BANNER_STALE_AFTER + Duration::from_secs(1));
                    gate.banner_blocked = old.map(|t| (t, why));
                }
            }
        }
        let err = check_admits_job().unwrap_err();
        assert!(err.contains("has not painted"), "{err}");
    }

    #[test]
    fn a_stamp_clears_an_earlier_block_report() {
        let _g = guard();
        arm_for_test();
        note_banner_blocked("too small");
        stamp_banner();
        assert!(check_admits_job().is_ok());
    }

    #[test]
    fn status_reports_why_work_would_be_refused() {
        let _g = guard();
        arm_for_test();
        assert!(
            status(0).denied_reason.is_some(),
            "an armed but unpainted gate must say why it would refuse"
        );
        stamp_banner();
        assert!(status(0).denied_reason.is_none());
    }

    #[test]
    fn expired_lease_refuses_and_clears() {
        let _g = guard();
        arm("s".into(), "tech".into(), "diag".into(), "why".into(), 0);
        stamp_banner();
        assert!(check_admits_job().is_err());
        assert!(!status(0).armed, "an expired gate must clear itself");
    }

    #[test]
    fn ttl_is_clamped() {
        let _g = guard();
        let st = arm("s".into(), "t".into(), "d".into(), "r".into(), u64::MAX / 2);
        assert!(st.expires_in_secs.unwrap() <= MAX_TTL.as_secs());
    }
}
