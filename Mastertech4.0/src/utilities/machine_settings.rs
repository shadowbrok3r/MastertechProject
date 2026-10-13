//! Per-machine JSON settings files, kept outside eframe storage.

use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

/// Directory holding this machine's settings files.
pub fn dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let dir = PathBuf::from(r"C:\ProgramData\Mastertech");
    #[cfg(not(target_os = "windows"))]
    let dir = std::env::temp_dir().join("mastertech");
    dir
}

/// The settings stored at `path`; the default when the file is missing or unreadable.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Writes `settings` to `path` as JSON, creating its directory.
pub fn save<T: Serialize>(path: &Path, settings: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}
