//! Crash-safe self-update swap. The staged binary is validated, the running
//! exe is parked under a backup name, and the staged file is renamed into
//! place; any failure rolls back so the original exe path is never left empty.

use std::io::Read;
use std::path::{Path, PathBuf};

/// Staged download written by the GitHub self-updater.
pub const STAGED_NAME: &str = "git-MasterTech.exe";
/// Staged binary written by the admin-console remote update.
pub const REMOTE_STAGED_NAME: &str = "MasterTech_update_pending.exe";

/// Path the GitHub self-updater stages its download at, beside the running exe.
pub fn staged_update_path() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("running exe has no parent directory"))?;
    Ok(dir.join(STAGED_NAME))
}

/// Backup name the running exe is parked under during a swap.
pub fn backup_path_for(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// Validates `staged` and swaps it into the running exe's path. Returns the
/// exe path to relaunch. On failure the running exe keeps its original name.
pub fn apply_staged_update(staged: &Path, expected_len: u64) -> anyhow::Result<PathBuf> {
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

    let exe = std::env::current_exe()?;
    let backup = backup_path_for(&exe);
    let _ = std::fs::remove_file(&backup);
    // Renaming a running image is allowed; deleting it is not.
    std::fs::rename(&exe, &backup)
        .map_err(|e| anyhow::anyhow!("could not park running exe: {e}"))?;
    match std::fs::rename(staged, &exe) {
        Ok(()) => Ok(exe),
        Err(e) => {
            if std::fs::rename(&backup, &exe).is_err() {
                let _ = std::fs::copy(&backup, &exe);
            }
            Err(anyhow::anyhow!("could not install staged update: {e}"))
        }
    }
}

/// Relaunches `exe` detached, forwarding this process's CLI args plus `envs`.
pub fn relaunch(exe: &Path, envs: &[(&str, &str)]) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args().skip(1));
    for (key, value) in envs {
        cmd.env(key, value);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    cmd.spawn()?;
    Ok(())
}

/// Removes swap leftovers at startup: the parked `.old.exe` (retried while the
/// previous process exits), stray staged downloads, and restores
/// `MasterTech.exe` when running under a staged name after a broken update.
pub fn cleanup_update_leftovers() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    let self_name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let backup = backup_path_for(&exe);
    if !self_name.ends_with(".old.exe") && backup.exists() {
        for _ in 0..5 {
            if std::fs::remove_file(&backup).is_ok() {
                log::info!("removed old exe backup {backup:?}");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
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
    let running_as_staged = [STAGED_NAME, REMOTE_STAGED_NAME]
        .iter()
        .any(|n| self_name == n.to_ascii_lowercase());
    let real = dir.join("MasterTech.exe");
    if running_as_staged && !real.exists() {
        match std::fs::copy(&exe, &real) {
            Ok(_) => log::info!("restored MasterTech.exe from {self_name}"),
            Err(e) => log::error!("could not restore MasterTech.exe: {e}"),
        }
    }
}
