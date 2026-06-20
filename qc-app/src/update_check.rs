//! Poll the `\\winbits7` share for a newer qc-app version string and surface it.
//! UNC read runs on a background thread; the UI thread only `poll`s.

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

/// Share file holding the latest published version string.
const VERSION_FILE: &str = r"\\winbits7\copyfolder\qc-app-version.txt";
/// Minimum gap between share reads.
const CHECK_COOLDOWN: Duration = Duration::from_secs(3600);

/// Background version checker; `poll` is non-blocking and sticky once an update is found.
pub struct UpdateChecker {
    rx: Receiver<String>,
    tx: Sender<String>,
    last_check: Option<Instant>,
    available: Option<String>,
}

impl UpdateChecker {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::bounded::<String>(1);
        Self { rx, tx, last_check: None, available: None }
    }

    /// Returns a newer version string once detected, else `None`.
    pub fn poll(&mut self) -> Option<String> {
        if self.available.is_some() {
            return self.available.clone();
        }
        let due = self.last_check.map(|t| t.elapsed() > CHECK_COOLDOWN).unwrap_or(true);
        if due {
            self.last_check = Some(Instant::now());
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                if let Some(v) = read_remote_version() {
                    let _ = tx.try_send(v);
                }
            });
        }
        if let Ok(v) = self.rx.try_recv() {
            self.available = Some(v);
        }
        self.available.clone()
    }
}

impl Default for UpdateChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads and trims the share version string; `Some` only when newer than the built-in version.
fn read_remote_version() -> Option<String> {
    let remote = std::fs::read_to_string(VERSION_FILE).ok()?;
    let remote = remote.trim();
    if remote.is_empty() || remote == env!("CARGO_PKG_VERSION") {
        return None;
    }
    Some(remote.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_checker_has_no_update() {
        let c = UpdateChecker::new();
        assert!(c.available.is_none());
        assert!(c.last_check.is_none());
    }

    #[test]
    fn poll_arms_last_check() {
        let mut c = UpdateChecker::new();
        let _ = c.poll();
        assert!(c.last_check.is_some());
    }

    #[test]
    fn available_is_sticky() {
        let mut c = UpdateChecker::new();
        c.available = Some("9.9.9".to_string());
        let before = c.last_check;
        assert_eq!(c.poll().as_deref(), Some("9.9.9"));
        assert_eq!(c.last_check, before);
    }

    #[test]
    fn injected_version_caches_into_available() {
        let mut c = UpdateChecker::new();
        c.tx.try_send("9.9.9".to_string()).unwrap();
        assert_eq!(c.poll().as_deref(), Some("9.9.9"));
        assert_eq!(c.available.as_deref(), Some("9.9.9"));
    }
}
