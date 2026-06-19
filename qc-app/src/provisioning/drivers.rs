//! G3 driver install: catalog lookup → stage from share → silent install.
//! Ported from QCWizard `Chipset` / `Display` / `InstallerBase`. Windows-only
//! execution; the lookup/staging logic is shared.

use std::path::Path;

use super::catalog_query::DriverRow;
use super::download;

/// Stage and silently install one catalog driver. Returns a status line.
pub fn install_driver(row: &DriverRow) -> anyhow::Result<String> {
    let relative = row
        .url_download
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| row.file_name.clone());
    let staged = download::stage_from_share(&relative)?;
    let args = row.argument_string.clone().unwrap_or_default();
    run_installer(&staged, &args)?;
    Ok(format!("Installed {}", row.file_name))
}

/// Run a staged installer: `.msi` via msiexec, otherwise the exe with its args.
#[cfg(windows)]
fn run_installer(path: &Path, args: &str) -> anyhow::Result<()> {
    use std::process::Command;
    let is_msi = path.extension().map(|e| e.eq_ignore_ascii_case("msi")).unwrap_or(false);
    let arg_list: Vec<&str> = args.split_whitespace().collect();
    let status = if is_msi {
        Command::new("msiexec").arg("/i").arg(path).args(&arg_list).status()
    } else {
        Command::new(path).args(&arg_list).status()
    }
    .map_err(|e| anyhow::anyhow!("spawn installer {}: {e}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("installer {} exited with {status}", path.display()))
    }
}

#[cfg(not(windows))]
fn run_installer(_path: &Path, _args: &str) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("driver install is Windows-only"))
}
