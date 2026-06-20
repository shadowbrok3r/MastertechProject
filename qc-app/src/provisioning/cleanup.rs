//! Post-build SysPrep and post-repair Service cleanup (QCWizard cleanup port).
//! Pure filesystem work plus a backend status advance; closing the app is the
//! caller's job. Functions return a multi-line status summary.

use std::path::{Path, PathBuf};

use database::orders::{QcBackend, QcOrder};

/// Legacy status id for "Ready to Ship".
pub const READY_TO_SHIP_LEGACY_ID: i64 = 233;

/// Desktop shortcut file names removed during cleanup.
const SHORTCUT_NAMES: &[&str] = &["At-Home Support.lnk"];

/// Run file cleanup, then advance the order to "Ready to Ship".
pub async fn sysprep_cleanup(order: &QcOrder, backend: &QcBackend) -> anyhow::Result<String> {
    let mut summary = match sysprep_files_only() {
        Ok(s) => s,
        Err(e) => format!("File cleanup failed: {e}"),
    };
    backend.advance_status(order, READY_TO_SHIP_LEGACY_ID).await?;
    summary.push_str("\nAdvanced to Ready to Ship.");
    Ok(summary)
}

/// Remove staging dirs and desktop shortcuts. Errors are collected, never fatal.
pub fn sysprep_files_only() -> anyhow::Result<String> {
    let mut report = Report::new();
    for dir in staging_dirs() {
        remove_dir(&dir, &mut report);
    }
    for shortcut in desktop_shortcuts() {
        remove_file(&shortcut, &mut report);
    }
    Ok(report.finish("SysPrep cleanup"))
}

/// Remove desktop shortcuts only (post-repair Service cleanup).
pub fn service_cleanup() -> anyhow::Result<String> {
    let mut report = Report::new();
    for shortcut in desktop_shortcuts() {
        remove_file(&shortcut, &mut report);
    }
    Ok(report.finish("Service cleanup"))
}

/// Staging directories removed during SysPrep cleanup.
fn staging_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(r"C:\Install Before Generalize")];
    let windir = std::env::var("windir").unwrap_or_default();
    if !windir.is_empty() {
        dirs.push(Path::new(&windir).join("Options"));
    }
    dirs
}

/// Desktop shortcut paths under %PUBLIC%\Desktop and %USERPROFILE%\Desktop.
fn desktop_shortcuts() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(public) = std::env::var("PUBLIC") {
        roots.push(Path::new(&public).join("Desktop"));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(Path::new(&profile).join("Desktop"));
    }
    roots
        .iter()
        .flat_map(|root| SHORTCUT_NAMES.iter().map(move |name| root.join(name)))
        .collect()
}

/// Collects skip and error lines into a labeled summary.
struct Report {
    removed: Vec<String>,
    errors: Vec<String>,
}

impl Report {
    fn new() -> Self {
        Self { removed: Vec::new(), errors: Vec::new() }
    }

    fn note_removed(&mut self, path: &Path) {
        self.removed.push(path.display().to_string());
    }

    fn note_error(&mut self, path: &Path, err: std::io::Error) {
        self.errors.push(format!("{}: {err}", path.display()));
    }

    fn finish(self, label: &str) -> String {
        let mut out = format!("{label}: removed {} item(s)", self.removed.len());
        for path in &self.removed {
            out.push_str(&format!("\n  removed {path}"));
        }
        for err in &self.errors {
            out.push_str(&format!("\n  error {err}"));
        }
        out
    }
}

/// Remove a directory tree; missing is skipped, other errors are collected.
fn remove_dir(path: &Path, report: &mut Report) {
    match std::fs::remove_dir_all(path) {
        Ok(()) => report.note_removed(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::debug!("cleanup skip (absent): {}", path.display());
        }
        Err(e) => report.note_error(path, e),
    }
}

/// Remove a file; missing is skipped, other errors are collected.
fn remove_file(path: &Path, report: &mut Report) {
    match std::fs::remove_file(path) {
        Ok(()) => report.note_removed(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::debug!("cleanup skip (absent): {}", path.display());
        }
        Err(e) => report.note_error(path, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_targets_are_not_errors() {
        let mut report = Report::new();
        remove_dir(Path::new(r"Z:\definitely\absent\dir"), &mut report);
        remove_file(Path::new(r"Z:\definitely\absent\file.lnk"), &mut report);
        assert!(report.removed.is_empty());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn removes_existing_dir_and_file() {
        let base = std::env::temp_dir().join(format!(
            "mtech_cleanup_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = base.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file = base.join("shortcut.lnk");
        std::fs::write(&file, b"x").unwrap();

        let mut report = Report::new();
        remove_file(&file, &mut report);
        remove_dir(&base, &mut report);
        assert_eq!(report.removed.len(), 2);
        assert!(report.errors.is_empty());
        assert!(!base.exists());
    }

    #[test]
    fn shortcut_paths_cover_both_desktops() {
        unsafe {
            std::env::set_var("PUBLIC", r"C:\Users\Public");
            std::env::set_var("USERPROFILE", r"C:\Users\Tech");
        }
        let paths = desktop_shortcuts();
        assert!(paths.iter().any(|p| p.ends_with(r"Public\Desktop\At-Home Support.lnk")));
        assert!(paths.iter().any(|p| p.ends_with(r"Tech\Desktop\At-Home Support.lnk")));
    }

    #[test]
    fn finish_reports_counts() {
        let mut report = Report::new();
        report.note_removed(Path::new(r"C:\a"));
        let out = report.finish("SysPrep cleanup");
        assert!(out.starts_with("SysPrep cleanup: removed 1 item(s)"));
    }
}
