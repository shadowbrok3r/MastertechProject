//! Keeps this machine awake while MasterTech runs: a process-lifetime sleep
//! block plus the persistent sleep / hibernation / Fast Startup settings.

#[cfg(target_os = "windows")]
use std::{
    os::windows::process::CommandExt,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(target_os = "windows")]
const POWER_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Power";

/// Shortest gap between two persistent-settings passes.
#[cfg(target_os = "windows")]
const REAPPLY_AFTER: Duration = Duration::from_secs(300);

/// Set once the keep-awake thread is up.
#[cfg(target_os = "windows")]
static HOLDING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
static LAST_APPLY: Mutex<Option<Instant>> = Mutex::new(None);

/// Blocks system sleep for this process, then applies the persistent settings
/// off-thread. Calls within [`REAPPLY_AFTER`] skip the settings pass.
/// `MTECH_NO_POWER_POLICY` disables both.
#[cfg(target_os = "windows")]
pub fn ensure_awake(reason: &str) {
    if std::env::var_os("MTECH_NO_POWER_POLICY").is_some() {
        return;
    }

    hold_awake();

    if !claim_apply_slot() {
        return;
    }

    let reason = reason.to_owned();
    let spawned = std::thread::Builder::new()
        .name("mtech-power-policy".into())
        .spawn(move || {
            let failures = disable_sleep_states();
            if failures.is_empty() {
                log::info!(
                    "power policy ({reason}): sleep, hibernation and Fast Startup disabled"
                );
            } else {
                log::warn!("power policy ({reason}): {}", failures.join("; "));
            }
        });
    if let Err(e) = spawned {
        log::warn!("power policy: apply thread failed to start: {e}");
    }
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_awake(_reason: &str) {}

/// Sets the sleep and hibernate idle timeouts to never, turns hibernation off,
/// and clears Fast Startup. Returns one entry per setting that did not apply.
#[cfg(target_os = "windows")]
pub fn disable_sleep_states() -> Vec<String> {
    let passes: [&[&str]; 5] = [
        &["/change", "standby-timeout-ac", "0"],
        &["/change", "standby-timeout-dc", "0"],
        &["/change", "hibernate-timeout-ac", "0"],
        &["/change", "hibernate-timeout-dc", "0"],
        &["/hibernate", "off"],
    ];
    let mut failures: Vec<String> =
        passes.iter().filter_map(|args| powercfg(args).err()).collect();
    if let Err(e) = disable_fast_startup() {
        failures.push(e);
    }
    failures
}

#[cfg(not(target_os = "windows"))]
pub fn disable_sleep_states() -> Vec<String> {
    Vec::new()
}

/// Sets the display idle timeout to never on AC and battery.
#[cfg(target_os = "windows")]
pub fn disable_display_timeout() -> Vec<String> {
    ["monitor-timeout-ac", "monitor-timeout-dc"]
        .into_iter()
        .filter_map(|setting| powercfg(&["/change", setting, "0"]).err())
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub fn disable_display_timeout() -> Vec<String> {
    Vec::new()
}

/// Writes `HiberbootEnabled = 0`, the Fast Startup switch.
#[cfg(target_os = "windows")]
fn disable_fast_startup() -> Result<(), String> {
    windows_registry::LOCAL_MACHINE
        .options()
        .read()
        .write()
        .create()
        .open(POWER_KEY)
        .and_then(|key| key.set_u32("HiberbootEnabled", 0))
        .map_err(|e| format!("HiberbootEnabled=0 -> {e}"))
}

/// Runs one `powercfg` invocation; it prints nothing on success, so the exit
/// code is the only verdict. Output is captured, never inherited.
#[cfg(target_os = "windows")]
fn powercfg(args: &[&str]) -> Result<(), String> {
    let label = args.join(" ");
    let output = Command::new("powercfg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powercfg {label}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = match stderr.trim() {
        "" => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        text => text.to_owned(),
    };
    Err(format!("powercfg {label} -> {} ({detail})", output.status))
}

/// Parks the sleep block on a thread that outlives every caller; Windows
/// releases the request when the thread that made it exits.
#[cfg(target_os = "windows")]
fn hold_awake() {
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    if HOLDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("mtech-keep-awake".into())
        .spawn(|| {
            let previous =
                unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            if previous.0 == 0 {
                log::warn!("power policy: SetThreadExecutionState failed");
                HOLDING.store(false, Ordering::SeqCst);
                return;
            }
            log::info!("power policy: system sleep blocked for the life of this process");
            loop {
                std::thread::park();
            }
        });
    if let Err(e) = spawned {
        HOLDING.store(false, Ordering::SeqCst);
        log::warn!("power policy: keep-awake thread failed to start: {e}");
    }
}

/// True when the last settings pass was longer ago than [`REAPPLY_AFTER`].
#[cfg(target_os = "windows")]
fn claim_apply_slot() -> bool {
    let mut last = LAST_APPLY.lock().unwrap_or_else(|e| e.into_inner());
    match *last {
        Some(at) if at.elapsed() < REAPPLY_AFTER => false,
        _ => {
            *last = Some(Instant::now());
            true
        }
    }
}
