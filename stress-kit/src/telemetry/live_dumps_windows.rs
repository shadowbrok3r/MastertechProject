//! Watches `C:\Windows\LiveKernelReports` for watchdog live dumps written
//! during a run. Display-path hangs (AMD Crash Defender `AMD_WATCHDOG` /
//! `AMD_REPORT_UM`, dxgkrnl `WATCHDOG`) land here and never raise a TDR event,
//! so [`super::TdrCounters`] does not see them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const ROOT: &str = r"C:\Windows\LiveKernelReports";

/// Subdirectory nesting walked under [`ROOT`].
const MAX_DEPTH: usize = 3;

/// A live dump that appeared after the baseline snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDump {
    /// Subfolder the dump landed in — `AMD_WATCHDOG`, `WATCHDOG`, `NVIDIA_*`.
    pub bucket: String,
    pub file: String,
}

impl LiveDump {
    pub fn label(&self) -> String {
        if self.bucket.is_empty() {
            self.file.clone()
        } else {
            format!("{}\\{}", self.bucket, self.file)
        }
    }
}

pub struct LiveDumpWatcher {
    root: PathBuf,
    seen: HashSet<PathBuf>,
    /// `false` when the directory could not be read, so an empty result means
    /// "not checked" rather than "no watchdog dumps".
    available: bool,
}

impl LiveDumpWatcher {
    /// Snapshots the dumps already on disk; only later arrivals are reported.
    pub fn new() -> Self {
        Self::rooted(Path::new(ROOT))
    }

    fn rooted(root: &Path) -> Self {
        let mut watcher = Self {
            root: root.to_path_buf(),
            seen: HashSet::new(),
            available: false,
        };
        match collect_dumps(&watcher.root) {
            Ok(found) => {
                watcher.available = true;
                log::debug!(
                    "stress-kit/live-dumps: baselined {} dump(s) under {}",
                    found.len(),
                    watcher.root.display()
                );
                watcher.seen = found;
            }
            Err(e) => {
                log::warn!(
                    "stress-kit/live-dumps: cannot read {}: {e} — watchdog dump detection is off",
                    watcher.root.display()
                );
            }
        }
        watcher
    }

    /// `true` when the reports directory was readable at baseline.
    pub fn available(&self) -> bool {
        self.available
    }

    /// Dumps written since the last poll. Each is reported once.
    pub fn poll(&mut self) -> Vec<LiveDump> {
        if !self.available {
            return Vec::new();
        }
        let Ok(found) = collect_dumps(&self.root) else {
            return Vec::new();
        };
        let mut fresh: Vec<LiveDump> = found
            .difference(&self.seen)
            .map(|path| LiveDump {
                bucket: path
                    .parent()
                    .filter(|p| *p != self.root)
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                file: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        fresh.sort_by_key(|d| d.label());
        self.seen = found;
        fresh
    }
}

impl Default for LiveDumpWatcher {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_dumps(root: &Path) -> std::io::Result<HashSet<PathBuf>> {
    let mut out = HashSet::new();
    walk(root, 0, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, depth: usize, out: &mut HashSet<PathBuf>) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            // A subdirectory this process cannot open is not a scan failure.
            let _ = walk(&path, depth + 1, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dmp"))
        {
            out.insert(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"dump").unwrap();
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stress-kit-live-dumps-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn baselined_dumps_are_not_reported() {
        let root = temp_root("baseline");
        touch(&root.join("AMD_WATCHDOG").join("old.dmp"));

        let mut watcher = LiveDumpWatcher::rooted(&root);
        assert!(watcher.available());
        assert!(watcher.poll().is_empty(), "pre-existing dump reported as new");
    }

    #[test]
    fn new_dump_reports_its_bucket_once() {
        let root = temp_root("fresh");
        touch(&root.join("AMD_WATCHDOG").join("old.dmp"));
        let mut watcher = LiveDumpWatcher::rooted(&root);

        touch(&root.join("AMD_REPORT_UM").join("new.dmp"));
        let found = watcher.poll();
        assert_eq!(found.len(), 1, "expected one new dump, got {found:?}");
        assert_eq!(found[0].label(), "AMD_REPORT_UM\\new.dmp");

        assert!(watcher.poll().is_empty(), "same dump reported twice");
    }

    #[test]
    fn missing_directory_reads_as_unavailable() {
        let root = std::env::temp_dir().join("stress-kit-live-dumps-absent");
        let _ = std::fs::remove_dir_all(&root);
        let mut watcher = LiveDumpWatcher::rooted(&root);
        assert!(!watcher.available());
        assert!(watcher.poll().is_empty());
    }
}
