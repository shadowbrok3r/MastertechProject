//! Capture panics to disk so the next launch can offer to file a bug report.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrashInfo {
    pub timestamp: String,
    pub version: String,
    pub message: String,
    pub location: String,
    pub backtrace: String,
}

/// Directory holding captured crash reports.
pub fn crashes_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("crashes"))
        .unwrap_or_else(|| std::env::temp_dir().join("mastertech_qc_crashes"))
}

/// Chain a panic hook that writes a `CrashInfo` json before the previous hook runs.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = payload_string(info.payload());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        let crash = CrashInfo {
            timestamp: chrono::Utc::now().to_rfc3339(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            message,
            location,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        let _ = write_crash(&crash);
        previous(info);
    }));
}

/// Downcast a panic payload to its string form.
fn payload_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Serialize a crash report to a millis-stamped file in `crashes_dir`.
fn write_crash(crash: &CrashInfo) -> std::io::Result<()> {
    let dir = crashes_dir();
    std::fs::create_dir_all(&dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("crash-{millis}.json"));
    let json = serde_json::to_string_pretty(crash)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Read all stored crash reports paired with their file paths.
pub fn scan_pending() -> Vec<(PathBuf, CrashInfo)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(crashes_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e.eq_ignore_ascii_case("json")) != Some(true) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(info) = serde_json::from_slice::<CrashInfo>(&bytes) {
                out.push((path, info));
            }
        }
    }
    out
}

/// Delete a stored crash report, treating a missing file as success.
pub fn delete(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow::anyhow!("delete {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_downcasts_str_and_string() {
        let s: &str = "boom";
        assert_eq!(payload_string(&s), "boom");
        let owned: String = "kaboom".to_string();
        assert_eq!(payload_string(&owned), "kaboom");
        let other: i32 = 7;
        assert_eq!(payload_string(&other), "non-string panic payload");
    }

    #[test]
    fn round_trips_crash_info() {
        let crash = CrashInfo {
            timestamp: chrono::Utc::now().to_rfc3339(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            message: "boom".to_string(),
            location: "src/x.rs:1:1".to_string(),
            backtrace: "frame".to_string(),
        };
        let json = serde_json::to_string(&crash).unwrap();
        let back: CrashInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message, "boom");
        assert_eq!(back.location, "src/x.rs:1:1");
    }

    #[test]
    fn delete_missing_is_ok() {
        let path = crashes_dir().join("crash-does-not-exist-99999.json");
        assert!(delete(&path).is_ok());
    }
}
