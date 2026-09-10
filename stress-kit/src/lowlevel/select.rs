//! Runtime backend selection.

use std::sync::Mutex;

use super::{BackendId, LowLevelAccess, LowLevelBackend, RejectedBackend, WeakAccess};

/// Chooses the backend. A forced backend that fails does not fall through, so a
/// comparison never silently measures a different one.
const OVERRIDE_ENV: &str = "MTECH_LOWLEVEL_BACKEND";

/// Priority order, used by [`Selection::Auto`]. Fixed, not configurable: a
/// signed provider always beats an unsigned one on a customer machine, and a
/// driver that also reads board rails beats the WMI path that only reads a
/// temperature.
const ORDER: &[BackendId] = &[
    BackendId::PawnIo,
    BackendId::Mtdrv,
    BackendId::WinRing0,
    BackendId::EsifWmi,
];

/// What runs when [`OVERRIDE_ENV`] is unset.
///
/// PawnIO alone while the migration is being proven. Falling back to WinRing0
/// hides why PawnIO declined — the fallback answers, the snapshot looks healthy,
/// and the reason PawnIO is not live only survives in `backend_rejected`, where
/// it is easy to miss. Set `MTECH_LOWLEVEL_BACKEND=auto` for the full chain.
const DEFAULT_SELECTION: Selection = Selection::Only(BackendId::PawnIo);

/// An opened backend and the sentence describing why it is live.
type Opened = (Box<dyn LowLevelBackend>, String);

/// Which backends a run may open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    /// Every compiled-in backend in [`ORDER`], first one that answers.
    Auto,
    /// This backend or nothing.
    Only(BackendId),
}

fn parse_selection(raw: &str) -> Option<Selection> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" | "order" | "chain" => Some(Selection::Auto),
        "none" | "off" => Some(Selection::Only(BackendId::None)),
        "pawnio" | "pawn_io" => Some(Selection::Only(BackendId::PawnIo)),
        "mtdrv" | "mastertech" => Some(Selection::Only(BackendId::Mtdrv)),
        "winring0" | "win_ring0" => Some(Selection::Only(BackendId::WinRing0)),
        "esif" | "esif_wmi" | "dptf" => Some(Selection::Only(BackendId::EsifWmi)),
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
    // A handle that can no longer read is never handed to a new caller. The
    // loss latch is one-way, and a handle that opened nothing stays empty, so
    // reusing either would blind every sampler started after it for the life of
    // the process — a machine whose driver was merely busy at first-open would
    // never read a sensor again.
    if let Some(live) = cached
        .as_ref()
        .and_then(WeakAccess::upgrade)
        .filter(|a| !a.is_dead())
    {
        return live;
    }
    let access = open_uncached();
    *cached = Some(access.downgrade());
    access
}

/// Resolves the selection, then opens under it.
fn open_uncached() -> LowLevelAccess {
    let selection = match std::env::var(OVERRIDE_ENV) {
        Ok(raw) => match parse_selection(&raw) {
            Some(selection) => selection,
            None => {
                return LowLevelAccess::unavailable(
                    format!("{OVERRIDE_ENV}={raw} is not a known backend; no backend opened"),
                    Vec::new(),
                );
            }
        },
        Err(_) => DEFAULT_SELECTION,
    };
    match selection {
        Selection::Auto => open_in_priority_order(),
        Selection::Only(forced) => open_only(forced),
    }
}

/// Opens one backend or nothing. The absence of a fallback is the point: the
/// reason this backend declined is the whole answer, and another provider
/// answering in its place buries it.
fn open_only(forced: BackendId) -> LowLevelAccess {
    if forced == BackendId::None {
        return LowLevelAccess::unavailable(
            format!("{OVERRIDE_ENV} selected no backend, so none was opened"),
            Vec::new(),
        );
    }
    match try_open(forced) {
        Ok((backend, detail)) => {
            log::info!("stress-kit/lowlevel: {} live", forced.label());
            LowLevelAccess::new(backend, detail, Vec::new())
        }
        Err(reason) => LowLevelAccess::unavailable(
            format!(
                "{} is the only selected backend and it could not open: {reason}. Set \
                 {OVERRIDE_ENV}=auto to fall back to the others.",
                forced.label()
            ),
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
        #[cfg(all(target_os = "windows", feature = "backend-pawnio"))]
        BackendId::PawnIo => super::pawnio::PawnIoBackend::open().map(|b| {
            let detail = b.detail();
            (Box::new(b) as Box<dyn LowLevelBackend>, detail)
        }),
        #[cfg(not(all(target_os = "windows", feature = "backend-pawnio")))]
        BackendId::PawnIo => Err("not compiled in".into()),

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

        #[cfg(all(target_os = "windows", feature = "backend-esif-wmi"))]
        BackendId::EsifWmi => open_esif_wmi(),
        #[cfg(not(all(target_os = "windows", feature = "backend-esif-wmi")))]
        BackendId::EsifWmi => Err("not compiled in".into()),

        BackendId::None | BackendId::Mock => Err("not selectable".into()),
    }
}

/// DPTF ships only on Intel platforms, so the vendor is checked before the WMI
/// round trip rather than after it fails.
#[cfg(all(target_os = "windows", feature = "backend-esif-wmi"))]
fn open_esif_wmi() -> Result<Opened, String> {
    use crate::telemetry::cpu_ceiling::{self, CpuVendor};

    if cpu_ceiling::vendor() != CpuVendor::Intel {
        return Err("not an Intel platform; DPTF/ESIF publishes no participants here".into());
    }
    super::esif_wmi::EsifWmiBackend::open().map(|b| {
        let detail = "Intel DPTF (WMI) is live. It reads the processor participant's own \
                      temperature out of the root/wmi EsifDeviceInformation class with no driver \
                      and no elevation, and carries no board voltage rails."
            .to_string();
        (Box::new(b) as Box<dyn LowLevelBackend>, detail)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_spellings_parse() {
        use Selection::Only;
        assert_eq!(parse_selection("winring0"), Some(Only(BackendId::WinRing0)));
        assert_eq!(parse_selection("  WinRing0 "), Some(Only(BackendId::WinRing0)));
        assert_eq!(parse_selection("none"), Some(Only(BackendId::None)));
        assert_eq!(parse_selection("mtdrv"), Some(Only(BackendId::Mtdrv)));
        assert_eq!(parse_selection("DPTF"), Some(Only(BackendId::EsifWmi)));
        assert_eq!(parse_selection("esif_wmi"), Some(Only(BackendId::EsifWmi)));
        assert_eq!(parse_selection("pawnio"), Some(Only(BackendId::PawnIo)));
        assert_eq!(parse_selection("auto"), Some(Selection::Auto));
        assert_eq!(parse_selection("winio"), None);
    }

    /// The default opens PawnIO alone. A fallback would answer in its place and
    /// bury the reason PawnIO declined, which is the one thing being measured
    /// while the migration is unproven.
    #[test]
    fn the_default_is_pawnio_alone() {
        assert_eq!(DEFAULT_SELECTION, Selection::Only(BackendId::PawnIo));
    }

    /// The full chain has to stay reachable without a rebuild, or a machine
    /// PawnIO cannot serve has no way back to a working backend.
    #[test]
    fn auto_restores_the_chain() {
        assert_eq!(parse_selection("auto"), Some(Selection::Auto));
        assert!(ORDER.len() > 1);
    }

    /// Priority must place the signed driver ahead of the legacy one, and both
    /// driver backends ahead of the WMI path, which reads no board rails.
    #[test]
    fn signed_backend_outranks_winring0() {
        let mtdrv = ORDER.iter().position(|b| *b == BackendId::Mtdrv);
        let legacy = ORDER.iter().position(|b| *b == BackendId::WinRing0);
        let wmi = ORDER.iter().position(|b| *b == BackendId::EsifWmi);
        let pawnio = ORDER.iter().position(|b| *b == BackendId::PawnIo);
        assert!(pawnio < legacy, "WinRing0 must never be preferred");
        assert!(mtdrv < legacy, "WinRing0 must never be preferred");
        assert!(legacy < wmi, "the temperature-only WMI path must be the last resort");
    }

    /// An unparseable override opens nothing rather than falling through to a
    /// backend the operator did not ask for.
    #[test]
    fn unknown_override_opens_nothing() {
        assert_eq!(parse_selection("winio"), None);
        let access = LowLevelAccess::unavailable(
            format!("{OVERRIDE_ENV}=winio is not a known backend; no backend opened"),
            Vec::new(),
        );
        assert_eq!(access.id(), BackendId::None);
        assert!(access.status().detail.contains("not a known backend"));
    }

    #[test]
    fn explicit_none_override_opens_nothing() {
        let access = open_only(BackendId::None);
        assert_eq!(access.id(), BackendId::None);
        assert!(access.status().rejected.is_empty());
    }

    /// Concurrent callers must share one open provider. A second open would
    /// stop the driver service the first is still reading through, which is how
    /// a running stress test lost its CPU die temperature the moment anything
    /// else asked for telemetry. A handle that opened nothing is exempt: there
    /// is no provider to invalidate, and pinning callers to it would blind
    /// every sampler started after a driver that was merely busy.
    #[test]
    fn overlapping_callers_share_one_provider() {
        let first = open();
        let second = open();
        if first.is_absent() {
            assert!(second.is_absent(), "an empty handle opened a provider on retry");
        } else {
            assert!(
                first.same_provider(&second),
                "a second caller opened its own provider"
            );
        }

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

    /// A cached handle that opened nothing must not be handed to a later
    /// caller: the driver may only have been busy, and reusing the empty handle
    /// blinds every sampler started after it for the life of the process.
    #[test]
    fn an_empty_handle_is_never_reused_from_the_cache() {
        let empty = LowLevelAccess::unavailable("nothing opened", Vec::new());
        assert!(empty.is_dead(), "an empty handle must read as dead");

        let mut cached = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        let restore = cached.take();
        *cached = Some(empty.downgrade());
        let reused = cached
            .as_ref()
            .and_then(WeakAccess::upgrade)
            .filter(|a| !a.is_dead());
        *cached = restore;
        drop(cached);

        assert!(reused.is_none(), "the cache handed back an empty handle");
    }
}
