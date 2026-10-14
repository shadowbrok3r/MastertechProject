//! Crash-safe self-update swap that never leaves the running exe's path empty.

#[cfg(target_os = "windows")]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Staged download written by the GitHub self-updater.
#[cfg(target_os = "windows")]
pub const STAGED_NAME: &str = "git-MasterTech.exe";
/// Staged download written by the GitHub self-updater.
#[cfg(unix)]
pub const STAGED_NAME: &str = ".mastertech-update.staged";
/// Staged binary written by the admin-console remote update.
#[cfg(target_os = "windows")]
pub const REMOTE_STAGED_NAME: &str = "MasterTech_update_pending.exe";
/// Staged binary written by the admin-console remote update.
#[cfg(unix)]
pub const REMOTE_STAGED_NAME: &str = ".mastertech-remote-update.staged";

/// Longest a staged binary may take to answer `--version`.
#[cfg(unix)]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Exe path handed from the self-update handler to the terminal-mode exit path.
static RESTART: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Path of the running executable.
#[cfg(target_os = "windows")]
pub fn running_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// Path of the running executable, even after it was replaced on disk.
#[cfg(unix)]
pub fn running_exe() -> std::io::Result<PathBuf> {
    mtech_update::unix::running_exe()
}

/// Path the GitHub self-updater stages its download at, beside the running exe.
pub fn staged_update_path() -> anyhow::Result<PathBuf> {
    let exe = running_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("running exe has no parent directory"))?;
    Ok(dir.join(STAGED_NAME))
}

/// Backup name the running exe is parked under during a swap.
#[cfg(target_os = "windows")]
pub fn backup_path_for(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// Checks `staged` against the running exe, makes it executable and probes its version.
#[cfg(unix)]
pub fn prepare_staged(staged: &Path, expected_version: Option<&str>) -> anyhow::Result<()> {
    let exe = running_exe()?;
    mtech_update::unix::prepare(staged, &exe, expected_version, PROBE_TIMEOUT)
}

/// [`prepare_staged`] on the blocking pool.
#[cfg(unix)]
pub async fn prepare_staged_blocking(staged: PathBuf, expected_version: String) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || prepare_staged(&staged, Some(expected_version.as_str())))
        .await?
}

/// Validates `staged` and swaps it into the running exe's path. Returns the
/// exe path to relaunch. On failure the running exe keeps its original name.
pub fn apply_staged_update(staged: &Path, expected_len: u64) -> anyhow::Result<PathBuf> {
    let exe = running_exe()?;
    install(staged, &exe, expected_len)?;
    Ok(exe)
}

#[cfg(target_os = "windows")]
fn install(staged: &Path, exe: &Path, expected_len: u64) -> anyhow::Result<()> {
    let meta = std::fs::metadata(staged)
        .map_err(|e| anyhow::anyhow!("staged update missing at {staged:?}: {e}"))?;
    if expected_len > 0 && meta.len() != expected_len {
        anyhow::bail!(
            "staged update is {} bytes, expected {expected_len}",
            meta.len()
        );
    }
    let mut magic = [0u8; 2];
    std::fs::File::open(staged)?.read_exact(&mut magic)?;
    if &magic != b"MZ" {
        anyhow::bail!("staged update is not a Windows executable");
    }

    let backup = backup_path_for(exe);
    let _ = std::fs::remove_file(&backup);
    // Renaming a running image is allowed; deleting it is not.
    std::fs::rename(exe, &backup)
        .map_err(|e| anyhow::anyhow!("could not park running exe: {e}"))?;
    match std::fs::rename(staged, exe) {
        Ok(()) => Ok(()),
        Err(e) => {
            if std::fs::rename(&backup, exe).is_err() {
                let _ = std::fs::copy(&backup, exe);
            }
            Err(anyhow::anyhow!("could not install staged update: {e}"))
        }
    }
}

#[cfg(unix)]
fn install(staged: &Path, exe: &Path, expected_len: u64) -> anyhow::Result<()> {
    mtech_update::unix::install(staged, exe, expected_len)
}

/// Relaunches `exe` detached, forwarding this process's CLI args plus `envs`.
#[cfg(target_os = "windows")]
pub fn relaunch(exe: &Path, envs: &[(&str, &str)]) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args().skip(1));
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.creation_flags(DETACHED_PROCESS);
    cmd.spawn()?;
    Ok(())
}

/// Relaunches `exe` in its own process group, forwarding this process's CLI args plus `envs`.
#[cfg(unix)]
pub fn relaunch(exe: &Path, envs: &[(&str, &str)]) -> anyhow::Result<()> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    mtech_update::unix::spawn_detached(exe, &args, envs)?;
    Ok(())
}

/// Asks the terminal-mode loop to exit so `exe` can replace this process.
pub fn request_restart(exe: PathBuf) {
    *RESTART.lock().unwrap_or_else(PoisonError::into_inner) = Some(exe);
}

/// True once [`request_restart`] has been called.
pub fn restart_requested() -> bool {
    RESTART
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

/// Execs the binary passed to [`request_restart`], if any; exits when exec fails.
#[cfg(unix)]
pub fn exec_requested_restart() {
    let Some(exe) = RESTART.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return;
    };
    let args = mtech_update::relaunch_args(
        std::env::args_os().skip(1),
        super::run_mode::is_terminal(),
    );
    log::info!("self-update -> exec {exe:?} {args:?}");
    let err = mtech_update::unix::exec_in_place(
        &exe,
        &args,
        &[("MASTERTECH_SELF_UPDATE_CHILD", "1")],
    );
    log::error!("self-update -> exec of {exe:?} failed: {err}");
    std::process::exit(1);
}

/// Removes swap leftovers at startup: the parked `.old.exe` (retried while the
/// previous process exits), stray staged downloads, and restores
/// `MasterTech.exe` when running under a staged name after a broken update.
pub fn cleanup_update_leftovers() {
    if let Ok(exe) = std::env::current_exe() {
        cleanup_in(&exe);
    }
}

/// [`cleanup_update_leftovers`] for the exe at `exe`.
fn cleanup_in(exe: &Path) {
    let Some(dir) = exe.parent() else {
        return;
    };
    let self_name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    #[cfg(target_os = "windows")]
    {
        let backup = backup_path_for(exe);
        if !self_name.ends_with(".old.exe") && backup.exists() {
            for _ in 0..5 {
                if std::fs::remove_file(&backup).is_ok() {
                    log::info!("removed old exe backup {backup:?}");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }

    for staged_name in [STAGED_NAME, REMOTE_STAGED_NAME] {
        if self_name != staged_name.to_ascii_lowercase() {
            let staged = dir.join(staged_name);
            if staged.exists() && std::fs::remove_file(&staged).is_ok() {
                log::info!("removed stray staged update {staged:?}");
            }
        }
    }

    // Running under a staged name with no MasterTech.exe present: restore it.
    #[cfg(target_os = "windows")]
    {
        let running_as_staged = [STAGED_NAME, REMOTE_STAGED_NAME]
            .iter()
            .any(|n| self_name == n.to_ascii_lowercase());
        let real = dir.join("MasterTech.exe");
        if running_as_staged && !real.exists() {
            match std::fs::copy(exe, &real) {
                Ok(_) => log::info!("restored MasterTech.exe from {self_name}"),
                Err(e) => log::error!("could not restore MasterTech.exe: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_install_parks_the_exe_and_rolls_back_on_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("MasterTech.exe");
        let staged = dir.path().join(STAGED_NAME);

        std::fs::write(&exe, b"MZ old").unwrap();
        std::fs::write(&staged, b"not a PE").unwrap();
        assert!(install(&staged, &exe, 0).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ old");

        std::fs::write(&staged, b"MZ new").unwrap();
        assert!(install(&staged, &exe, 99).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ old");

        install(&staged, &exe, 6).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ new");
        assert_eq!(std::fs::read(backup_path_for(&exe)).unwrap(), b"MZ old");
        assert!(!staged.exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_cleanup_removes_leftovers_and_restores_the_exe() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("MasterTech.exe");
        std::fs::write(&exe, b"MZ current").unwrap();
        std::fs::write(backup_path_for(&exe), b"MZ old").unwrap();
        std::fs::write(dir.path().join(STAGED_NAME), b"MZ staged").unwrap();
        std::fs::write(dir.path().join(REMOTE_STAGED_NAME), b"MZ remote").unwrap();

        cleanup_in(&exe);
        assert!(!backup_path_for(&exe).exists());
        assert!(!dir.path().join(STAGED_NAME).exists());
        assert!(!dir.path().join(REMOTE_STAGED_NAME).exists());
        assert!(exe.exists());

        std::fs::remove_file(&exe).unwrap();
        let running_as_staged = dir.path().join(REMOTE_STAGED_NAME);
        std::fs::write(&running_as_staged, b"MZ remote").unwrap();
        cleanup_in(&running_as_staged);
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ remote");
        assert!(running_as_staged.exists());
    }

    #[cfg(unix)]
    #[test]
    fn unix_cleanup_removes_staged_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Mastertech-linux");
        let other = dir.path().join("notes.txt");
        std::fs::write(&exe, b"elf").unwrap();
        std::fs::write(&other, b"keep").unwrap();
        std::fs::write(dir.path().join(STAGED_NAME), b"staged").unwrap();
        std::fs::write(dir.path().join(REMOTE_STAGED_NAME), b"remote").unwrap();

        cleanup_in(&exe);
        assert!(!dir.path().join(STAGED_NAME).exists());
        assert!(!dir.path().join(REMOTE_STAGED_NAME).exists());
        assert!(!dir.path().join("MasterTech.exe").exists());
        assert!(exe.exists() && other.exists());
    }
}
