//! Runtime backend selection.

use std::sync::Mutex;

use super::{BackendId, LowLevelAccess, LowLevelBackend, RejectedBackend, WeakAccess};

/// Forces one backend for bench A/B comparison. An override that fails does not
/// fall through, so a comparison never silently measures a different backend.
const OVERRIDE_ENV: &str = "MTECH_LOWLEVEL_BACKEND";

/// Priority order. Fixed, not configurable: a signed provider always beats an
/// unsigned one on a customer machine.
const ORDER: &[BackendId] = &[BackendId::Mtdrv, BackendId::WinRing0];

/// An opened backend and the sentence describing why it is live.
type Opened = (Box<dyn LowLevelBackend>, String);

fn parse_override(raw: &str) -> Option<BackendId> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "none" | "off" => Some(BackendId::None),
        "mtdrv" | "mastertech" => Some(BackendId::Mtdrv),
        "winring0" | "win_ring0" => Some(BackendId::WinRing0),
        _ => None,
    }
}

/// The one backend this process has open, while any caller still holds it.
static SHARED: Mutex<Option<WeakAccess>> = Mutex::new(None);

/// The process-wide backend, opening one on first use.
///
/// Every caller shares a single provider. Opening a second one is not merely
/// wasteful: `WinRing0Backend::open` stops and deletes the running driver
/// service before restarting it, and its `Drop` unloads the driver outright, so
/// a second opener silently invalidates the device handle the first is still
/// polling — the sampler behind a running stress test goes blind the moment
/// anything else asks for telemetry.
pub fn open() -> LowLevelAccess {
    let mut cached = match SHARED.lock() {
        Ok(guard) => guard,
        // A panic mid-open left the slot mid-write; the value itself is a Weak.
        Err(poisoned) => poisoned.into_inner(),
    };
    // A handle whose provider died is never handed to a new caller: the loss
    // latch is one-way, so reusing it would blind every sampler started after
    // it for the life of the process.
    if let Some(live) = cached
        .as_ref()
        .and_then(WeakAccess::upgrade)
        .filter(|a| !a.is_lost())
    {
        return live;
    }
    let access = open_uncached();
    *cached = Some(access.downgrade());
    access
}

/// Opens the first backend that is compiled in and answers, recording why each
/// earlier one was skipped.
fn open_uncached() -> LowLevelAccess {
    match std::env::var(OVERRIDE_ENV) {
        Ok(raw) => open_overridden(&raw),
        Err(_) => open_in_priority_order(),
    }
}

fn open_overridden(raw: &str) -> LowLevelAccess {
    let Some(forced) = parse_override(raw) else {
        return LowLevelAccess::unavailable(
            format!("{OVERRIDE_ENV}={raw} is not a known backend; no backend opened"),
            Vec::new(),
        );
    };
    if forced == BackendId::None {
        return LowLevelAccess::unavailable(
            format!("{OVERRIDE_ENV}={raw} — no backend opened by request"),
            Vec::new(),
        );
    }
    match try_open(forced) {
        Ok((backend, detail)) => LowLevelAccess::new(backend, detail, Vec::new()),
        Err(reason) => LowLevelAccess::unavailable(
            format!("{OVERRIDE_ENV}={raw} could not open: {reason}"),
            vec![RejectedBackend { backend: forced, reason }],
        ),
    }
}

fn open_in_priority_order() -> LowLevelAccess {
    let mut rejected = Vec::new();
    for &candidate in ORDER {
        match try_open(candidate) {
            Ok((backend, detail)) => {
                log::info!("stress-kit/lowlevel: {} live", candidate.label());
                return LowLevelAccess::new(backend, detail, rejected);
            }
            Err(reason) => {
                log::debug!("stress-kit/lowlevel: {} unavailable — {reason}", candidate.label());
                rejected.push(RejectedBackend { backend: candidate, reason });
            }
        }
    }
    log::info!(
        "stress-kit/lowlevel: no backend opened; CPU die temperature and board voltages \
         unavailable"
    );
    LowLevelAccess::unavailable(
        "No low-level sensor backend is available, so CPU die temperature and board voltage \
         rails cannot be read. Disk temperatures and ACPI zones are unaffected.",
        rejected,
    )
}

/// Attempts one backend. `Err` carries the operator-actionable reason.
fn try_open(candidate: BackendId) -> Result<Opened, String> {
    match candidate {
        BackendId::Mtdrv => Err("not compiled in".into()),

        #[cfg(all(target_os = "windows", feature = "backend-winring0"))]
        BackendId::WinRing0 => super::winring0::WinRing0Backend::open().map(|b| {
            let detail = "WinRing0 (legacy) is live. It needs Memory Integrity and the \
                          Vulnerable Driver Blocklist off, and current Defender definitions \
                          quarantine the driver file."
                .to_string();
            (Box::new(b) as Box<dyn LowLevelBackend>, detail)
        }),
        #[cfg(not(all(target_os = "windows", feature = "backend-winring0")))]
        BackendId::WinRing0 => Err("not compiled in".into()),

        BackendId::None | BackendId::Mock => Err("not selectable".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_spellings_parse() {
        assert_eq!(parse_override("winring0"), Some(BackendId::WinRing0));
        assert_eq!(parse_override("  WinRing0 "), Some(BackendId::WinRing0));
        assert_eq!(parse_override("none"), Some(BackendId::None));
        assert_eq!(parse_override("mtdrv"), Some(BackendId::Mtdrv));
        assert_eq!(parse_override("pawnio"), None);
    }

    /// Priority must place the signed driver ahead of the legacy one.
    #[test]
    fn signed_backend_outranks_winring0() {
        let mtdrv = ORDER.iter().position(|b| *b == BackendId::Mtdrv);
        let legacy = ORDER.iter().position(|b| *b == BackendId::WinRing0);
        assert!(mtdrv < legacy, "WinRing0 must never be preferred");
    }

    /// An unparseable override opens nothing rather than falling through to a
    /// backend the operator did not ask for.
    #[test]
    fn unknown_override_opens_nothing() {
        let access = open_overridden("pawnio");
        assert_eq!(access.id(), BackendId::None);
        assert!(access.status().detail.contains("not a known backend"));
    }

    #[test]
    fn explicit_none_override_opens_nothing() {
        let access = open_overridden("none");
        assert_eq!(access.id(), BackendId::None);
        assert!(access.status().rejected.is_empty());
    }

    /// Concurrent callers must share one provider. A second open would stop the
    /// driver service the first is still reading through, which is how a
    /// running stress test lost its CPU die temperature the moment anything
    /// else asked for telemetry.
    #[test]
    fn overlapping_callers_share_one_provider() {
        let first = open();
        let second = open();
        assert!(
            first.same_provider(&second),
            "a second caller opened its own provider"
        );

        drop(first);
        drop(second);
        // Every share released, so the next caller opens fresh rather than
        // holding the provider loaded for the life of the process.
        assert!(
            SHARED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .and_then(WeakAccess::upgrade)
                .is_none(),
            "the cache kept the provider alive past its last holder"
        );
    }
}
