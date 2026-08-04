//! Hostname and machine id this host is identified by.
//!
//! Booted into WinPE (HBCD_PE, the Mastertech PXE image), `System::host_name()`
//! returns the PE image's own name, so every derived record key —
//! `connected_client:HBCD_PE:81ac48bac` and its `computer` twin — is a fresh row
//! that no longer matches the machine's normal Windows check-in and has to be
//! hand-linked. Under PE *only*, this module reads the offline install's
//! `ComputerName` out of its SYSTEM hive and adopts the machine id that install
//! persisted, so both halves of the key match the installed-OS identity.
//!
//! Every fallback lands back on the live PE values, so a locked, dirty or absent
//! offline Windows degrades to the old `HBCD_PE:…` behaviour instead of failing.

use std::path::Path;
use std::sync::OnceLock;

pub use database::schema::BootEnvironment;

/// Hostname baked into the Hiren's Boot CD PE image.
const HBCD_PE_HOSTNAME: &str = "HBCD_PE";

/// Mount point for the offline SYSTEM hive.
#[cfg(target_os = "windows")]
const OFFLINE_HIVE_MOUNT: &str = r"HKLM\MTOFFSYS";

/// `machine_id.txt` under a user profile. Mirrors the tail of
/// `ProjectDirs::data_local_dir` on Windows; pinned by a test below.
#[cfg(target_os = "windows")]
const MACHINE_ID_TAIL: &str = r"AppData\Local\Mastertech\MastertechQC\data\machine_id.txt";

/// A Windows installation on a volume other than the running PE image.
#[derive(Debug, Clone)]
pub struct OfflineWindows {
    /// Volume root it was found on, e.g. `C:\`.
    pub volume: String,
    /// `ComputerName` read from the offline hive.
    pub hostname: String,
    /// Control set the hostname came from, e.g. `ControlSet001`.
    pub control_set: String,
    /// The install's persisted `machine_id.txt`, when a profile has one.
    pub machine_id: Option<String>,
    /// How many installs were found; >1 means the pick was ambiguous.
    pub candidates: usize,
}

/// Hostname reported by the running OS.
pub fn live_hostname() -> String {
    sysinfo::System::host_name().unwrap_or_default()
}

/// `true` when this process is running under WinPE rather than an installed OS.
pub fn is_winpe() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(detect_winpe)
}

/// Boot environment recorded on this client's `connected_client` row.
pub fn boot_environment() -> BootEnvironment {
    if is_winpe() {
        BootEnvironment::WinPe
    } else {
        BootEnvironment::Installed
    }
}

/// The offline Windows install this PE session stands in for. `None` off PE, or
/// when no install with a readable SYSTEM hive was found.
pub fn offline_windows() -> Option<&'static OfflineWindows> {
    static CACHED: OnceLock<Option<OfflineWindows>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            if !is_winpe() {
                return None;
            }
            resolve_offline_windows()
        })
        .as_ref()
}

/// Machine id the offline install persisted, when running under PE.
pub fn offline_machine_id() -> Option<String> {
    offline_windows().and_then(|w| w.machine_id.clone())
}

/// Hostname this machine's `connected_client` / `computer` key is derived from.
/// The live hostname everywhere except a PE session that resolved a different
/// offline hostname.
pub fn identity_hostname() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let live = live_hostname();
            if !is_winpe() {
                return live;
            }
            let resolved = offline_windows()
                .map(|w| w.hostname.clone())
                .filter(|h| !h.is_empty());
            match resolved {
                Some(h) if h == live => live,
                Some(h) => {
                    let from = offline_windows()
                        .map(|w| format!("{}{}", w.volume, w.control_set))
                        .unwrap_or_default();
                    log::info!(
                        "host_identity: WinPE session identifying as offline hostname \
                         {h:?} (from {from}) instead of live {live:?}"
                    );
                    h
                }
                None => {
                    log::warn!(
                        "host_identity: WinPE session found no offline hostname; \
                         keeping the live PE hostname {live:?}"
                    );
                    live
                }
            }
        })
        .clone()
}

/// Contents of `machine_id.txt` at `path`, if it holds a usable hash.
pub(crate) fn read_machine_id(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    (trimmed.len() >= 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| trimmed.to_string())
}

// ============================================================
// Windows
// ============================================================

#[cfg(target_os = "windows")]
fn detect_winpe() -> bool {
    let system_root = std::env::var("SystemRoot").unwrap_or_default();
    let winpe =
        // PE's own %SystemRoot% is the X: RAM disk.
        system_root.to_ascii_uppercase().starts_with(r"X:\")
        // Control\MiniNT exists only in a PE registry.
        || reg(&["query", r"HKLM\SYSTEM\CurrentControlSet\Control\MiniNT"]).is_some()
        // Redundant next to those two, but it is the signature techs recognise.
        || live_hostname().eq_ignore_ascii_case(HBCD_PE_HOSTNAME);
    if winpe {
        log::info!(
            "host_identity: WinPE detected (SystemRoot={system_root:?}, hostname={:?})",
            live_hostname()
        );
    }
    winpe
}

/// Picks the install to identify as: valid `Select\Current` first, then largest
/// volume.
#[cfg(target_os = "windows")]
fn resolve_offline_windows() -> Option<OfflineWindows> {
    struct Candidate {
        volume: String,
        hostname: String,
        control_set: String,
        machine_id: Option<String>,
        size: u64,
        select_valid: bool,
    }

    let pe_volume = std::env::var("SystemRoot")
        .ok()
        .and_then(|root| root.get(..3).map(str::to_ascii_uppercase));

    let mut candidates: Vec<Candidate> = Vec::new();
    for (volume, size) in fixed_volumes() {
        if pe_volume.as_deref() == Some(volume.as_str()) {
            continue;
        }
        let hive = Path::new(&volume).join(r"Windows\System32\config\SYSTEM");
        if !hive.is_file() {
            continue;
        }
        match read_offline_computer_name(&hive) {
            Some((hostname, control_set, select_valid)) => candidates.push(Candidate {
                machine_id: offline_machine_id_on(&volume),
                volume,
                hostname,
                control_set,
                size,
                select_valid,
            }),
            None => log::warn!(
                "host_identity: {volume} has a Windows install but its SYSTEM hive is \
                 unreadable (BitLocker-locked, dirty, or reg load denied)"
            ),
        }
    }

    let found = candidates.len();
    if found > 1 {
        let listed: Vec<String> = candidates
            .iter()
            .map(|c| {
                let control_set = if c.select_valid {
                    c.control_set.clone()
                } else {
                    format!("{} assumed", c.control_set)
                };
                format!(
                    "{}={} ({control_set}, {} GiB)",
                    c.volume,
                    c.hostname,
                    c.size / (1024 * 1024 * 1024)
                )
            })
            .collect();
        log::warn!(
            "host_identity: {found} offline Windows installs found [{}]; picking by valid \
             control set then largest volume",
            listed.join(", ")
        );
    }

    candidates.sort_by(|a, b| b.select_valid.cmp(&a.select_valid).then(b.size.cmp(&a.size)));
    let best = candidates.into_iter().next()?;
    Some(OfflineWindows {
        volume: best.volume,
        hostname: best.hostname,
        control_set: best.control_set,
        machine_id: best.machine_id,
        candidates: found,
    })
}

/// Non-removable volume roots (`C:\`) and their sizes.
#[cfg(target_os = "windows")]
fn fixed_volumes() -> Vec<(String, u64)> {
    sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .filter(|disk| !disk.is_removable())
        .filter_map(|disk| {
            let mount = disk.mount_point().to_str()?.to_ascii_uppercase();
            if !mount.starts_with(|c: char| c.is_ascii_alphabetic()) {
                return None;
            }
            // A root without the trailing separator would make every join below
            // drive-relative rather than absolute.
            let root = if mount.ends_with('\\') {
                mount
            } else {
                format!("{mount}\\")
            };
            Some((root, disk.total_space()))
        })
        .collect()
}

/// `(ComputerName, control set, whether Select\Current resolved)` from an
/// offline SYSTEM hive.
#[cfg(target_os = "windows")]
fn read_offline_computer_name(hive: &Path) -> Option<(String, String, bool)> {
    let _loaded = LoadedHive::load(hive)?;

    let (control_set, select_valid) =
        match reg_dword(&format!(r"{OFFLINE_HIVE_MOUNT}\Select"), "Current") {
            Some(n) if n > 0 => (format!("ControlSet{n:03}"), true),
            _ => ("ControlSet001".to_string(), false),
        };

    let control = format!(r"{OFFLINE_HIVE_MOUNT}\{control_set}\Control\ComputerName");
    let hostname = reg_value(&format!(r"{control}\ComputerName"), "ComputerName")
        .or_else(|| reg_value(&format!(r"{control}\ActiveComputerName"), "ComputerName"))?;
    let hostname = hostname.trim().to_string();
    if hostname.is_empty() {
        return None;
    }
    Some((hostname, control_set, select_valid))
}

/// `machine_id.txt` from the newest user profile on `volume` that has one.
#[cfg(target_os = "windows")]
fn offline_machine_id_on(volume: &str) -> Option<String> {
    let mut newest: Option<(std::time::SystemTime, String)> = None;
    for profile in std::fs::read_dir(Path::new(volume).join("Users")).ok()?.flatten() {
        let path = profile.path().join(MACHINE_ID_TAIL);
        let Some(id) = read_machine_id(&path) else {
            continue;
        };
        let modified = path
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(seen, _)| modified > *seen) {
            newest = Some((modified, id));
        }
    }
    if let Some((_, id)) = &newest {
        log::info!("host_identity: adopting offline machine id {id} from {volume}");
    }
    newest.map(|(_, id)| id)
}

/// `reg load` of an offline hive, unloaded on drop.
#[cfg(target_os = "windows")]
struct LoadedHive;

#[cfg(target_os = "windows")]
impl LoadedHive {
    fn load(hive: &Path) -> Option<Self> {
        // A run that died before unloading leaves the mount point taken.
        let _ = reg(&["unload", OFFLINE_HIVE_MOUNT]);
        reg(&["load", OFFLINE_HIVE_MOUNT, &hive.to_string_lossy()]).map(|_| Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for LoadedHive {
    fn drop(&mut self) {
        // Each reg call is its own process, so nothing this code opened is still
        // holding the hive when the unload runs.
        if reg(&["unload", OFFLINE_HIVE_MOUNT]).is_none() {
            log::warn!(
                "host_identity: reg unload {OFFLINE_HIVE_MOUNT} failed; the hive stays \
                 mounted for the rest of this session"
            );
        }
    }
}

/// Runs `reg.exe`, returning stdout on a zero exit.
#[cfg(target_os = "windows")]
fn reg(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::var("SystemRoot")
        .map(|root| format!(r"{root}\System32\reg.exe"))
        .unwrap_or_else(|_| "reg.exe".to_string());
    let output = std::process::Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Data of the `name` value under `key`.
#[cfg(target_os = "windows")]
fn reg_value(key: &str, name: &str) -> Option<String> {
    parse_reg_query(&reg(&["query", key, "/v", name])?, name)
}

/// `reg_value` parsed as a REG_DWORD (`0x…`).
#[cfg(target_os = "windows")]
fn reg_dword(key: &str, name: &str) -> Option<u32> {
    let raw = reg_value(key, name)?;
    let hex = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(hex, 16).ok()
}

/// Data of the `name` value in `reg query` output, whose value lines read
/// `<name>    REG_SZ    <data>` under an echoed key path.
fn parse_reg_query(output: &str, name: &str) -> Option<String> {
    let wanted = name.to_ascii_lowercase();
    for line in output.lines() {
        let line = line.trim();
        if !line.to_ascii_lowercase().starts_with(&wanted) {
            continue;
        }
        let Some(typed) = line.find("REG_").map(|at| &line[at..]) else {
            continue;
        };
        let data = typed
            .split_once(char::is_whitespace)
            .map(|(_, data)| data.trim())
            .unwrap_or_default();
        if !data.is_empty() {
            return Some(data.to_string());
        }
    }
    None
}

// ============================================================
// Everything else
// ============================================================

#[cfg(not(target_os = "windows"))]
fn detect_winpe() -> bool {
    let _ = HBCD_PE_HOSTNAME;
    false
}

#[cfg(not(target_os = "windows"))]
fn resolve_offline_windows() -> Option<OfflineWindows> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim `reg query … /v ComputerName` output.
    const COMPUTER_NAME_QUERY: &str = "\r\n\
        HKEY_LOCAL_MACHINE\\SYSTEM\\ControlSet001\\Control\\ComputerName\\ComputerName\r\n    \
        ComputerName    REG_SZ    DESKTOP-EI5PV29\r\n\r\n";

    /// Verbatim `reg query … /v Current` output.
    const SELECT_CURRENT_QUERY: &str = "\r\n\
        HKEY_LOCAL_MACHINE\\SYSTEM\\Select\r\n    \
        Current    REG_DWORD    0x2\r\n\r\n";

    #[test]
    fn parses_reg_sz_data() {
        assert_eq!(
            parse_reg_query(COMPUTER_NAME_QUERY, "ComputerName").as_deref(),
            Some("DESKTOP-EI5PV29"),
            "the echoed key path also ends in ComputerName and must not be read as the value"
        );
    }

    #[test]
    fn parses_reg_dword_data() {
        assert_eq!(
            parse_reg_query(SELECT_CURRENT_QUERY, "Current").as_deref(),
            Some("0x2")
        );
    }

    #[test]
    fn parse_reg_query_rejects_a_missing_value() {
        let missing = "ERROR: The system was unable to find the specified registry key or value.";
        assert_eq!(parse_reg_query(missing, "ComputerName"), None);
        assert_eq!(parse_reg_query(COMPUTER_NAME_QUERY, "Nope"), None);
    }

    #[test]
    fn read_machine_id_rejects_junk() {
        let dir = std::env::temp_dir().join("mtech_host_identity_test");
        std::fs::create_dir_all(&dir).expect("temp dir");

        let short = dir.join("short.txt");
        std::fs::write(&short, "deadbeef").expect("write");
        assert_eq!(read_machine_id(&short), None, "under 32 chars is not a hash");

        let nonhex = dir.join("nonhex.txt");
        std::fs::write(&nonhex, "z".repeat(64)).expect("write");
        assert_eq!(read_machine_id(&nonhex), None, "non-hex is not a hash");

        let good = dir.join("good.txt");
        let hash = "a".repeat(64);
        std::fs::write(&good, format!("\n{hash}\n")).expect("write");
        assert_eq!(read_machine_id(&good), Some(hash), "surrounding whitespace is trimmed");

        assert_eq!(read_machine_id(&dir.join("absent.txt")), None);
    }

    /// The offline lookup rebuilds the path `directories` produces locally, so
    /// the two must not drift.
    #[cfg(target_os = "windows")]
    #[test]
    fn machine_id_tail_matches_project_dirs() {
        let local = crate::mapping::machine_id_path().expect("ProjectDirs resolves");
        let local = local.to_string_lossy().replace('/', "\\");
        assert!(
            local.ends_with(MACHINE_ID_TAIL),
            "machine_id_path() is {local}, which does not end with {MACHINE_ID_TAIL}"
        );
    }

    /// Off PE nothing is scanned and the live hostname is used verbatim, so
    /// existing `connected_client` keys cannot move.
    #[test]
    fn installed_boot_keeps_live_hostname() {
        if is_winpe() {
            return;
        }
        assert_eq!(boot_environment(), BootEnvironment::Installed);
        assert!(offline_windows().is_none());
        assert_eq!(offline_machine_id(), None);
        assert_eq!(identity_hostname(), live_hostname());
    }
}
