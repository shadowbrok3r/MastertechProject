//! Makes the PawnIO driver present.
//!
//! PawnIO creates its device object from a PnP AddDevice routine, and its
//! Microsoft attestation signature lives in the package catalog rather than in
//! the `.sys`. So it needs a real driver-package install and a `Root\PawnIO`
//! devnode: a staged image started through the service manager would fail
//! signing policy, and even past that would create nothing openable.

#[cfg(feature = "pawnio-triplet")]
mod triplet;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::lowlevel::busmutex::NamedLease;

use super::device::Absent;
use super::{device, devnode};

/// Selects the install route. `none` leaves an absent driver absent, which is
/// how a machine opts out of ever having a driver installed on it.
const OVERRIDE_ENV: &str = "MTECH_PAWNIO_INSTALL";

/// Serializes provisioning across MasterTech instances. Several run at once on
/// a bench box, and a second instance installing over the first is one way to
/// end up holding a device that is not there yet.
const INSTALL_LEASE: &str = r"Global\Mastertech_PawnIO_Install";
const LEASE_WAIT_MS: u32 = 60_000;

/// How long the device may take to appear after the install call returns. The
/// SetupAPI call returns once PnP has been told to start the node, which is not
/// the same instant its device object becomes openable.
const DEVICE_WAIT: Duration = Duration::from_secs(15);
const DEVICE_POLL: Duration = Duration::from_millis(250);

/// Reads before concluding the driver is absent. Opening the device while a
/// peer instance is closing its own handle can fail transiently, and the
/// response to "absent" is a driver install — far too large a step to take on
/// one failed read.
const PROBE_TRIES: u32 = 3;
const PROBE_GAP: Duration = Duration::from_millis(250);

/// One install attempt per process, so a failing install is not retried on
/// every telemetry sample.
static ATTEMPTED: OnceLock<Result<(), String>> = OnceLock::new();

/// How the driver gets onto the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Embedded x64 driver package, installed through SetupAPI.
    Triplet,
    /// Leave an absent driver absent.
    None,
}

fn parse_route(raw: &str) -> Option<Route> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "triplet" | "inf" => Some(Route::Triplet),
        "none" | "off" => Some(Route::None),
        _ => None,
    }
}

/// The running driver version, installing the driver first when it is absent.
pub fn ensure_available() -> Result<(u16, u8, u8), String> {
    if let Ok(version) = probe_device() {
        return Ok(version);
    }
    let _lease = NamedLease::acquire(INSTALL_LEASE, LEASE_WAIT_MS);
    // A peer instance may have finished installing while we waited.
    let why = match probe_device() {
        Ok(version) => return Ok(version),
        Err(why) => why,
    };
    log::info!("stress-kit/pawnio: {why}; considering an install");
    install(why)?;
    wait_for_device().map_err(|last| {
        format!(
            "the PawnIO install reported success but its device did not appear within {}s: {last}",
            DEVICE_WAIT.as_secs()
        )
    })
}

/// Reads the driver version, retrying a failed read before reporting nothing.
/// The last failure is carried out so callers can say what actually went wrong.
fn probe_device() -> Result<(u16, u8, u8), Absent> {
    let mut last = device::version();
    for _ in 1..PROBE_TRIES {
        if last.is_ok() {
            break;
        }
        std::thread::sleep(PROBE_GAP);
        last = device::version();
    }
    last
}

/// Polls rather than testing once: PnP finishes starting the node after the
/// install call has already returned, so an immediate open loses the race and
/// reports a working driver as absent.
fn wait_for_device() -> Result<(u16, u8, u8), Absent> {
    let deadline = Instant::now() + DEVICE_WAIT;
    loop {
        let last = match device::version() {
            Ok(version) => return Ok(version),
            Err(why) => why,
        };
        if Instant::now() >= deadline {
            return Err(last);
        }
        std::thread::sleep(DEVICE_POLL);
    }
}

/// `why` is the read that failed, carried through so a refusal to install says
/// what the device actually did rather than only that it said nothing.
fn install(why: Absent) -> Result<(), String> {
    ATTEMPTED.get_or_init(|| install_once(why)).clone()
}

fn install_once(why: Absent) -> Result<(), String> {
    // Installing a driver package is a persistent change to the host, which a
    // unit test must never make.
    if cfg!(test) {
        return Err(format!("{why}, and tests do not install PawnIO"));
    }
    // Only one node can own the \Device\PawnIO symlink, so an existing node blocks installing.
    let existing = devnode::count();
    if existing > 0 {
        return Err(format!(
            "{why}, yet {existing} Root\\PawnIO device node(s) already exist, so no install was \
             attempted — installing beside them would only add another. Remove the node(s) and \
             let it install fresh, or reboot if a driver change is pending."
        ));
    }
    match std::env::var(OVERRIDE_ENV) {
        Ok(raw) => install_overridden(&raw),
        Err(_) => run(Route::Triplet),
    }
}

fn install_overridden(raw: &str) -> Result<(), String> {
    match parse_route(raw) {
        Some(route) => run(route),
        None => Err(format!("{OVERRIDE_ENV}={raw} is not a known install route")),
    }
}

fn run(route: Route) -> Result<(), String> {
    match route {
        Route::None => Err(format!(
            "{OVERRIDE_ENV}=none, so an absent PawnIO was left absent"
        )),

        #[cfg(feature = "pawnio-triplet")]
        Route::Triplet => triplet::install(),
        #[cfg(not(feature = "pawnio-triplet"))]
        Route::Triplet => {
            Err("no PawnIO install route is compiled in; enable pawnio-triplet".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_spellings_parse() {
        assert_eq!(parse_route("triplet"), Some(Route::Triplet));
        assert_eq!(parse_route("  INF "), Some(Route::Triplet));
        assert_eq!(parse_route("none"), Some(Route::None));
        assert_eq!(parse_route("msi"), None);
    }

    /// Upstream's setup.exe was dropped once the triplet proved out. Its
    /// spellings must not silently parse as the surviving route: a bench asking
    /// for the installer needs to be told it is gone, not handed the other one.
    #[test]
    fn the_dropped_installer_spellings_do_not_parse() {
        assert_eq!(parse_route("installer"), None);
        assert_eq!(parse_route("setup"), None);
        assert_eq!(parse_route("exe"), None);
    }

    /// Nothing may install a kernel driver on the machine running the tests.
    #[test]
    fn tests_never_install() {
        assert!(install(Absent::Open(2)).is_err_and(|e| e.contains("tests do not install")));
    }
}
