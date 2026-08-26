//! Log file location, rotation, and the process banner every launch writes first.
//!
//! The file sink is installed on every launch path, never conditionally, so a machine that
//! renders nothing still leaves a log naming the reason. Resolution order is
//! `MTECH_LOG_DIR`, `%LOCALAPPDATA%\Mastertech\logs`, the executable's directory, then the
//! system temp directory; the first writable one wins. Nothing here panics — a launch with no
//! writable directory anywhere loses file logging, not the app.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Rotated generations kept beside the live `output.log`.
const ROTATED_KEEP: usize = 5;

static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The log file this process actually opened, once [`file_logger`] has run.
pub fn active_log_path() -> Option<&'static Path> {
    ACTIVE_PATH.get().map(PathBuf::as_path)
}

/// Directory candidates in preference order.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("MTECH_LOG_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.data_local_dir().join("Mastertech").join("logs"));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir);
    }
    dirs.push(std::env::temp_dir().join("Mastertech"));
    dirs
}

/// Shift `output.log` down to `output.1.log` .. `output.{ROTATED_KEEP}.log`, dropping the oldest.
fn rotate(dir: &Path) {
    let current = dir.join("output.log");
    if !current.exists() {
        return;
    }
    let _ = std::fs::remove_file(dir.join(format!("output.{ROTATED_KEEP}.log")));
    for n in (1..ROTATED_KEEP).rev() {
        let from = dir.join(format!("output.{n}.log"));
        if from.exists() {
            let _ = std::fs::rename(&from, dir.join(format!("output.{}.log", n + 1)));
        }
    }
    let _ = std::fs::rename(&current, dir.join("output.1.log"));
}

/// Open the live log in `dir`, falling back to a pid-suffixed name when another instance holds it.
fn open_in(dir: &Path) -> Option<(File, PathBuf)> {
    std::fs::create_dir_all(dir).ok()?;
    rotate(dir);
    let primary = dir.join("output.log");
    if let Ok(file) = File::create(&primary) {
        return Some((file, primary));
    }
    let per_process = dir.join(format!("output-{}.log", std::process::id()));
    File::create(&per_process)
        .ok()
        .map(|file| (file, per_process))
}

/// A `Trace`-level file sink, or `None` when no candidate directory is writable.
pub fn file_logger() -> Option<Box<dyn log::Log + 'static>> {
    let (file, path) = candidate_dirs().iter().find_map(|dir| open_in(dir))?;
    let logger = simplelog::WriteLogger::new(
        log::LevelFilter::Trace,
        simplelog::Config::default(),
        file,
    );
    let _ = ACTIVE_PATH.set(path);
    Some(logger)
}

/// Write the launch facts a blank-window report needs, as the first records of the run.
pub fn log_process_banner(mode: &str) {
    log::info!(
        "MasterTech {} starting: mode={mode} pid={} exe={:?}",
        database::version_with_build!(),
        std::process::id(),
        std::env::current_exe().ok()
    );
    log::info!(
        "launch args: {:?}",
        std::env::args().skip(1).collect::<Vec<_>>()
    );
    match active_log_path() {
        Some(path) => log::info!("log file: {}", path.display()),
        None => log::warn!("no writable log directory found; this run has no log file"),
    }
    log::info!(
        "renderer knobs: MTECH_NO_FROST={:?} MTECH_LOG_DIR={:?} skia-render={}",
        std::env::var("MTECH_NO_FROST").ok(),
        std::env::var("MTECH_LOG_DIR").ok(),
        cfg!(feature = "skia-render")
    );
}
