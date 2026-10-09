//! When a downloaded MasterTech update installs: this machine's update mode and the work that holds the restart back.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long Later holds back the next update prompt.
pub const SNOOZE: Duration = Duration::from_secs(60 * 60);

const AGENT_POLL_EVERY: Duration = Duration::from_secs(30);

/// A downloaded update waiting to install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUpdate {
    /// Size of the staged executable in bytes.
    pub size: u64,
    pub version: String,
    /// A technician clicked Update now, or Update in the menu.
    pub accepted: bool,
    /// No prompt before this, after a Later click.
    pub snoozed_until: Option<Instant>,
    /// The last reason logged for waiting.
    pub waiting_on: Option<String>,
}

impl PendingUpdate {
    pub fn new(size: u64, version: String, accepted: bool) -> Self {
        Self {
            size,
            version,
            accepted,
            snoozed_until: None,
            waiting_on: None,
        }
    }
}

/// How a downloaded update installs on this machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// A notification asks before restarting.
    #[default]
    Prompt,
    /// Restarts as soon as nothing is running.
    Auto,
}

#[derive(Default, Serialize, Deserialize)]
struct UpdateSettings {
    #[serde(default)]
    mode: UpdateMode,
}

/// Per-machine settings file; eframe storage is keyed by version, so it starts empty after every update.
fn settings_file() -> PathBuf {
    if let Ok(dir) = std::env::var("MASTERTECH_UPDATE_SETTINGS_DIR") {
        return PathBuf::from(dir).join("update_settings.json");
    }
    #[cfg(target_os = "windows")]
    let dir = PathBuf::from(r"C:\ProgramData\Mastertech");
    #[cfg(not(target_os = "windows"))]
    let dir = std::env::temp_dir().join("mastertech");
    dir.join("update_settings.json")
}

/// This machine's update mode; Prompt when the file is missing or unreadable.
pub fn load_mode() -> UpdateMode {
    std::fs::read_to_string(settings_file())
        .ok()
        .and_then(|raw| serde_json::from_str::<UpdateSettings>(&raw).ok())
        .map(|s| s.mode)
        .unwrap_or_default()
}

pub fn save_mode(mode: UpdateMode) -> anyhow::Result<()> {
    let path = settings_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&UpdateSettings { mode })?,
    )?;
    Ok(())
}

/// Remote catalog script batches running on this client.
static REMOTE_SCRIPT_RUNS: AtomicU32 = AtomicU32::new(0);

/// Set by the poller while an AI session is working on this computer.
static AGENT_WORKING: AtomicBool = AtomicBool::new(false);

/// Counts a remote script batch as running until dropped.
pub struct ScriptRunGuard(());

impl ScriptRunGuard {
    pub fn start() -> Self {
        REMOTE_SCRIPT_RUNS.fetch_add(1, Ordering::SeqCst);
        Self(())
    }
}

impl Drop for ScriptRunGuard {
    fn drop(&mut self) {
        REMOTE_SCRIPT_RUNS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn set_agent_working(working: bool) {
    AGENT_WORKING.store(working, Ordering::SeqCst);
}

static AGENT_POLL_STARTED: AtomicBool = AtomicBool::new(false);

/// Checks every 30 s whether an AI session is working on this computer; starts once per process.
pub fn spawn_agent_poll() {
    if AGENT_POLL_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async {
        let Ok(cs) =
            tokio::task::spawn_blocking(|| crate::filesystem::get_client_hash().connection_string)
                .await
        else {
            return;
        };
        loop {
            match database::schema::agent_thread::AgentThread::working_for_connection(&cs, None)
                .await
            {
                Ok(thread) => set_agent_working(thread.is_some()),
                Err(e) => log::debug!("update_policy -> agent session check failed: {e}"),
            }
            tokio::time::sleep(AGENT_POLL_EVERY).await;
        }
    });
}

/// Work on this client that an update restart would cut off.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub remote_jobs: u32,
    pub remote_scripts: u32,
    pub stress: bool,
    pub remote_control_armed: bool,
    pub agent_working: bool,
}

impl Activity {
    /// What is running on this client right now.
    pub fn current() -> Self {
        Self {
            remote_jobs: crate::remote_exec::registry::running_count(),
            remote_scripts: REMOTE_SCRIPT_RUNS.load(Ordering::SeqCst),
            stress: stress_runner::is_stress_active(),
            remote_control_armed: crate::remote_exec::gate::banner_info()
                .is_some_and(|b| b.expires_in_secs > 0),
            agent_working: AGENT_WORKING.load(Ordering::SeqCst),
        }
    }

    /// Why the restart has to wait, or `None` when nothing is running.
    pub fn reason(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.agent_working {
            parts.push("an AI session is working on this computer".to_string());
        }
        if self.remote_jobs > 0 {
            parts.push(plural(self.remote_jobs, "remote job"));
        }
        if self.remote_scripts > 0 {
            parts.push(plural(self.remote_scripts, "remote script run"));
        }
        if self.stress {
            parts.push("a stress test is running".to_string());
        }
        if self.remote_control_armed {
            parts.push("remote control is armed".to_string());
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

fn plural(n: u32, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun} running")
    } else {
        format!("{n} {noun}s running")
    }
}

/// What a downloaded update does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStep {
    /// Something is running; check again later.
    Wait,
    /// Ask the technician with the update notification.
    Prompt,
    /// Install and restart now.
    Install,
}

/// The next step for a downloaded update. `accepted` is a technician's Update now or Update click.
pub fn next_step(mode: UpdateMode, busy: bool, accepted: bool) -> UpdateStep {
    if busy {
        UpdateStep::Wait
    } else if accepted || mode == UpdateMode::Auto {
        UpdateStep::Install
    } else {
        UpdateStep::Prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn a_busy_machine_always_waits() {
        for mode in [UpdateMode::Prompt, UpdateMode::Auto] {
            for accepted in [false, true] {
                assert_eq!(
                    next_step(mode, true, accepted),
                    UpdateStep::Wait,
                    "{mode:?} accepted={accepted}"
                );
            }
        }
    }

    #[test]
    fn an_idle_machine_prompts_unless_auto_or_accepted() {
        assert_eq!(
            next_step(UpdateMode::Prompt, false, false),
            UpdateStep::Prompt
        );
        assert_eq!(
            next_step(UpdateMode::Prompt, false, true),
            UpdateStep::Install
        );
        assert_eq!(
            next_step(UpdateMode::Auto, false, false),
            UpdateStep::Install
        );
    }

    #[test]
    fn reason_names_every_running_thing() {
        assert_eq!(Activity::default().reason(), None);
        let busy = Activity {
            remote_jobs: 2,
            remote_scripts: 1,
            stress: true,
            remote_control_armed: true,
            agent_working: true,
        };
        assert_eq!(
            busy.reason().as_deref(),
            Some(
                "an AI session is working on this computer, 2 remote jobs running, 1 remote script run running, a stress test is running, remote control is armed"
            )
        );
    }

    #[test]
    fn script_guards_count_until_dropped() {
        let before = REMOTE_SCRIPT_RUNS.load(Ordering::SeqCst);
        let first = ScriptRunGuard::start();
        let second = ScriptRunGuard::start();
        assert_eq!(REMOTE_SCRIPT_RUNS.load(Ordering::SeqCst), before + 2);
        drop(first);
        drop(second);
        assert_eq!(REMOTE_SCRIPT_RUNS.load(Ordering::SeqCst), before);
    }

    #[test]
    fn the_mode_survives_a_round_trip_and_defaults_to_prompt() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir =
            std::env::temp_dir().join(format!("mtech-update-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: ENV_LOCK serializes every test that touches this variable.
        unsafe { std::env::set_var("MASTERTECH_UPDATE_SETTINGS_DIR", &dir) };
        assert_eq!(
            load_mode(),
            UpdateMode::Prompt,
            "a missing file means Prompt"
        );
        save_mode(UpdateMode::Auto).expect("save");
        assert_eq!(load_mode(), UpdateMode::Auto);
        std::fs::write(settings_file(), "not json").expect("write");
        assert_eq!(
            load_mode(),
            UpdateMode::Prompt,
            "an unreadable file means Prompt"
        );
        unsafe { std::env::remove_var("MASTERTECH_UPDATE_SETTINGS_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
    }
}
