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
        }
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
    match gate.banner_heartbeat {
        Some(stamp) if stamp.elapsed() <= BANNER_STALE_AFTER => Ok(()),
        Some(stamp) => Err(format!(
            "consent banner not rendering (last painted {:.1}s ago); refusing to run \
             unattended on a customer machine",
            stamp.elapsed().as_secs_f32()
        )),
        None => Err(
            "consent banner has not painted since the gate was armed; refusing to run \
             unattended on a customer machine"
                .into(),
        ),
    }
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
                denied_reason: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        disarm();
    }

    #[test]
    fn unarmed_gate_refuses() {
        reset();
        assert!(check_admits_job().is_err());
    }

    #[test]
    fn armed_without_banner_fails_closed() {
        reset();
        arm("s".into(), "tech".into(), "diag".into(), "why".into(), 60);
        let err = check_admits_job().unwrap_err();
        assert!(
            err.contains("banner"),
            "an armed gate with no banner must refuse: {err}"
        );
        reset();
    }

    #[test]
    fn armed_with_fresh_banner_admits() {
        reset();
        arm("s".into(), "tech".into(), "diag".into(), "why".into(), 60);
        stamp_banner();
        assert!(check_admits_job().is_ok());
        reset();
    }

    #[test]
    fn expired_lease_refuses_and_clears() {
        reset();
        arm("s".into(), "tech".into(), "diag".into(), "why".into(), 0);
        stamp_banner();
        assert!(check_admits_job().is_err());
        assert!(!status(0).armed, "an expired gate must clear itself");
        reset();
    }

    #[test]
    fn ttl_is_clamped() {
        reset();
        let st = arm("s".into(), "t".into(), "d".into(), "r".into(), u64::MAX / 2);
        assert!(st.expires_in_secs.unwrap() <= MAX_TTL.as_secs());
        reset();
    }
}
