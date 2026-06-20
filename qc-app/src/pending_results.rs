//! Disk-backed queue for stress-test results that could not be uploaded.
//! The stress panel dumps the upload JSON here when the orchestrator/DB is
//! offline; an uploader drains the directory on reconnect/startup.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Directory holding queued result payloads.
pub fn pending_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("pending_results"))
        .unwrap_or_else(|| std::env::temp_dir().join("mastertech_qc_pending_results"))
}

/// Writes a payload to a uniquely named file and returns its path.
pub fn save(payload: &serde_json::Value) -> anyhow::Result<PathBuf> {
    let dir = pending_dir();
    std::fs::create_dir_all(&dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{millis}-{}-{n}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_vec(payload)?)?;
    Ok(path)
}

/// Reads every `*.json` in `pending_dir()`, skipping unreadable or non-JSON files.
pub fn load_all() -> Vec<(PathBuf, serde_json::Value)> {
    let dir = pending_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("pending_results: read_dir {} failed: {e}", dir.display());
            }
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read(&path) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("pending_results: read {} failed: {e}", path.display());
                continue;
            }
        };
        match serde_json::from_slice::<serde_json::Value>(&raw) {
            Ok(v) => out.push((path, v)),
            Err(e) => log::warn!("pending_results: parse {} failed: {e}", path.display()),
        }
    }
    out
}

/// Removes a queued file, treating a missing file as success.
pub fn delete(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Counts queued `*.json` files.
pub fn pending_count() -> usize {
    let dir = pending_dir();
    match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .count(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_delete_round_trip() {
        let tag = format!("pending_test_{}_{}", std::process::id(), {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        });
        let payload = serde_json::json!({ "tag": tag, "score": 1234 });
        let path = save(&payload).expect("save should succeed");

        let found = load_all()
            .into_iter()
            .find(|(_, v)| v.get("tag").and_then(|t| t.as_str()) == Some(tag.as_str()));
        let (found_path, found_val) = found.expect("saved payload should be in load_all");
        assert_eq!(found_path, path);
        assert_eq!(found_val, payload);

        delete(&path).expect("delete should succeed");
        assert!(!path.exists());
        delete(&path).expect("delete of missing file is ok");
    }
}
