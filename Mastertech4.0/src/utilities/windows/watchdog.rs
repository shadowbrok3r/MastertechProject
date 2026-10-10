//! Scheduled task that relaunches MasterTech while RemoteExec is armed.

use std::os::windows::process::CommandExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::reboot::ps_quote;

pub const WATCHDOG_TASK_NAME: &str = "MastertechWatchdog";

/// Marks a scheduled launch, which exits when MasterTech is already running.
pub const WATCHDOG_FLAG: &str = "--watchdog";

/// Minutes between relaunch checks.
const CHECK_MINUTES: u32 = 5;

/// How long the task outlives the RemoteExec lease that armed it.
const GRACE: Duration = Duration::from_secs(3600);

/// Opened by every GUI and terminal instance for the life of the process.
const INSTANCE_MUTEX: windows::core::PCWSTR = windows::core::w!("Local\\MasterTech.Instance");

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static TERMINAL_MODE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum Desired {
    Until(Instant),
    Removed,
}

static DESIRED: Mutex<Desired> = Mutex::new(Desired::Removed);

/// Serialises task updates so the last request wins.
static APPLY: Mutex<()> = Mutex::new(());

/// Opens the instance mutex and keeps it open; false when another instance already has it.
pub fn claim_instance() -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    match unsafe { CreateMutexW(None, false, INSTANCE_MUTEX) } {
        Ok(_) => {
            let last = unsafe { GetLastError() };
            last != ERROR_ALREADY_EXISTS
        }
        Err(e) => {
            log::warn!("instance mutex unavailable: {e}");
            true
        }
    }
}

/// Records that this process runs terminal mode, so a relaunch passes `-t`.
pub fn set_terminal_mode() {
    TERMINAL_MODE.store(true, Ordering::Relaxed);
}

/// Logs a launch made by the watchdog or the reboot relaunch task.
pub fn log_launch() {
    if std::env::args().any(|a| a == WATCHDOG_FLAG) {
        log::warn!("started by a scheduled relaunch ({WATCHDOG_FLAG})");
    }
}

/// Relaunches MasterTech until `lease` plus [`GRACE`] from now.
pub fn arm(lease: Duration) {
    if std::env::var_os("MTECH_NO_WATCHDOG").is_some() {
        log::info!("{WATCHDOG_TASK_NAME}: MTECH_NO_WATCHDOG is set; not registering");
        return;
    }
    request(Desired::Until(Instant::now() + lease + GRACE));
}

pub fn disarm() {
    request(Desired::Removed);
}

/// Deletes the task when the user closes MasterTech; an ending Windows session keeps it.
pub fn remove_on_exit() {
    if session_ending() {
        log::info!("{WATCHDOG_TASK_NAME}: Windows session is ending; keeping the task");
        return;
    }
    *DESIRED.lock().unwrap_or_else(|e| e.into_inner()) = Desired::Removed;
    apply();
}

fn request(desired: Desired) {
    *DESIRED.lock().unwrap_or_else(|e| e.into_inner()) = desired;
    std::thread::spawn(apply);
}

fn apply() {
    let _serial = APPLY.lock().unwrap_or_else(|e| e.into_inner());
    let desired = *DESIRED.lock().unwrap_or_else(|e| e.into_inner());
    let result = match desired {
        Desired::Until(at) => register(at.saturating_duration_since(Instant::now())),
        Desired::Removed => remove_task(),
    };
    if let Err(e) = result {
        log::warn!("{WATCHDOG_TASK_NAME}: {e}");
    }
}

fn register(window: Duration) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("exe path has no parent directory"))?;
    let script = register_script(
        &exe.to_string_lossy(),
        &dir.to_string_lossy(),
        window.as_secs(),
        TERMINAL_MODE.load(Ordering::Relaxed),
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "Register-ScheduledTask failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    log::info!(
        "{WATCHDOG_TASK_NAME}: relaunching {} for the next {} min",
        exe.display(),
        window.as_secs() / 60
    );
    Ok(())
}

fn remove_task() -> anyhow::Result<()> {
    let out = std::process::Command::new("schtasks")
        .args(["/delete", "/tn", WATCHDOG_TASK_NAME, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if out.status.success() {
        log::info!("{WATCHDOG_TASK_NAME}: removed");
    }
    Ok(())
}

fn session_ending() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

/// PowerShell that registers the logon and repeating triggers, both expiring `secs` from now.
fn register_script(exe: &str, dir: &str, secs: u64, terminal: bool) -> String {
    let exe = ps_quote(exe);
    let dir = ps_quote(dir);
    let args = if terminal {
        format!("-t {WATCHDOG_FLAG}")
    } else {
        WATCHDOG_FLAG.to_string()
    };
    let secs = secs.max(u64::from(CHECK_MINUTES) * 60);
    format!(
        "$end=[DateTimeOffset]::Now.AddSeconds({secs}).ToString('yyyy-MM-ddTHH:mm:sszzz',[Globalization.CultureInfo]::InvariantCulture);\
         $a=New-ScheduledTaskAction -Execute '{exe}' -Argument '{args}' -WorkingDirectory '{dir}';\
         $l=New-ScheduledTaskTrigger -AtLogOn;$l.EndBoundary=$end;\
         $r=New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes({CHECK_MINUTES}) \
         -RepetitionInterval (New-TimeSpan -Minutes {CHECK_MINUTES}) -RepetitionDuration (New-TimeSpan -Seconds {secs});\
         $r.EndBoundary=$end;\
         $s=New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -AllowStartIfOnBatteries \
         -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) \
         -DeleteExpiredTaskAfter ([TimeSpan]::Zero);\
         Register-ScheduledTask -TaskName '{WATCHDOG_TASK_NAME}' -Action $a -Trigger $l,$r -Settings $s \
         -RunLevel Highest -Force | Out-Null"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_relaunches_with_the_watchdog_flag() {
        let s = register_script(r"C:\Tools\MasterTech.exe", r"C:\Tools", 7200, false);
        assert!(s.contains(r"-Execute 'C:\Tools\MasterTech.exe' -Argument '--watchdog' -WorkingDirectory 'C:\Tools'"));
        assert!(s.contains("-TaskName 'MastertechWatchdog'"));
    }

    #[test]
    fn terminal_mode_relaunches_in_terminal_mode() {
        let s = register_script(r"C:\MasterTech.exe", r"C:\", 7200, true);
        assert!(s.contains("-Argument '-t --watchdog'"));
    }

    #[test]
    fn both_triggers_expire_and_the_expired_task_deletes_itself() {
        let s = register_script(r"C:\MasterTech.exe", r"C:\", 9000, false);
        assert!(s.contains("AddSeconds(9000)"));
        assert!(s.contains("$l.EndBoundary=$end"));
        assert!(s.contains("$r.EndBoundary=$end"));
        assert!(s.contains("-DeleteExpiredTaskAfter ([TimeSpan]::Zero)"));
    }

    #[test]
    fn a_relaunched_instance_is_never_timed_out_or_doubled() {
        let s = register_script(r"C:\MasterTech.exe", r"C:\", 7200, false);
        assert!(s.contains("-ExecutionTimeLimit ([TimeSpan]::Zero)"));
        assert!(s.contains("-MultipleInstances IgnoreNew"));
        assert!(s.contains("-RunLevel Highest"));
    }

    #[test]
    fn quotes_in_paths_are_escaped() {
        let s = register_script(r"C:\Bob's\MasterTech.exe", r"C:\Bob's", 7200, false);
        assert!(s.contains(r"-Execute 'C:\Bob''s\MasterTech.exe'"));
        assert!(s.contains(r"-WorkingDirectory 'C:\Bob''s'"));
    }

    #[test]
    fn the_window_covers_at_least_one_check() {
        let s = register_script(r"C:\MasterTech.exe", r"C:\", 10, false);
        assert!(s.contains("AddSeconds(300)"));
    }

    #[test]
    fn the_command_line_accepts_the_watchdog_flag() {
        let m = crate::cli()
            .try_get_matches_from(["MasterTech", WATCHDOG_FLAG])
            .expect("--watchdog must parse");
        assert!(m.get_flag("watchdog"));
    }
}
