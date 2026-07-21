//! Driver Time Machine client plugin.
//!
//! Snapshots the Windows DriverStore (pnputil), exports driver packages as
//! rollback points, and stages/commits rollbacks. The admin console parses
//! `snapshot` text with `database::schema::driver_intel::parse_pnputil_enum`
//! and persists it as a `driver_snapshot` row.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

#[derive(Facet, Deserialize)]
struct ExportArgs {
    /// Published INF name, e.g. oem12.inf
    published_name: String,
}

#[derive(Facet, Deserialize)]
struct RollbackArgs {
    /// Folder of a previously exported driver package under C:\ProgramData\MTechDriverStore
    restore_path: String,
    /// Optional current published INF (oemXX.inf) to uninstall before restoring
    delete_published_name: Option<String>,
}

/// Enumerates exported rollback points and their sizes as compact JSON.
const LIST_EXPORTS_PS: &str = r##"$ErrorActionPreference='SilentlyContinue'
$root='C:\ProgramData\MTechDriverStore'
if(-not (Test-Path $root)){ '{"exports":[]}'; exit }
$rows = Get-ChildItem $root -Directory | ForEach-Object {
  $size = (Get-ChildItem $_.FullName -Recurse -File | Measure-Object Length -Sum).Sum
  [PSCustomObject]@{ name=$_.Name; path=$_.FullName; created=$_.CreationTime.ToString('s'); mb=[math]::Round(($size ?? 0)/1MB,1) }
}
[PSCustomObject]@{ exports=@($rows) } | ConvertTo-Json -Depth 4 -Compress"##;

/// Allow only pnputil-safe tokens (oemNN.inf, export folder names, drive paths).
fn sanitize_arg(v: &str, allow_path: bool) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 200 {
        return None;
    }
    let ok = v.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '.' | '_' | '-')
            || (allow_path && matches!(c, '\\' | '/' | ':' | ' '))
    });
    if !ok || v.contains("..") {
        return None;
    }
    Some(v.to_string())
}

/// Builds the pnputil export script for one published INF.
fn export_ps(inf: &str) -> String {
    let stem = inf.trim_end_matches(".inf");
    format!(
        r##"$ErrorActionPreference='SilentlyContinue'
$root='C:\ProgramData\MTechDriverStore'
New-Item -ItemType Directory -Force $root | Out-Null
$dest = Join-Path $root ('{stem}_' + (Get-Date -Format 'yyyyMMdd_HHmmss'))
New-Item -ItemType Directory -Force $dest | Out-Null
$out = & pnputil /export-driver {inf} $dest 2>&1 | Out-String
$files = @(Get-ChildItem $dest -Recurse -File).Count
[PSCustomObject]@{{ export_path=$dest; files=$files; pnputil=($out.Trim()) }} | ConvertTo-Json -Compress"##
    )
}

/// Builds the pnputil rollback script, optionally deleting the current package first.
fn rollback_ps(restore_path: &str, delete_inf: Option<&str>) -> String {
    let delete_block = match delete_inf {
        Some(inf) => {
            format!("$del = & pnputil /delete-driver {inf} /uninstall /force 2>&1 | Out-String")
        }
        None => "$del = 'skipped'".to_string(),
    };
    format!(
        r##"$ErrorActionPreference='SilentlyContinue'
if(-not (Test-Path '{restore_path}')){{ '{{"error":"restore_path not found"}}'; exit }}
{delete_block}
$add = & pnputil /add-driver '{restore_path}\*.inf' /subdirs /install 2>&1 | Out-String
$reboot = ($add -match 'reboot') -or ($del -match 'reboot')
[PSCustomObject]@{{ delete_output=([string]$del).Trim(); add_output=$add.Trim(); reboot_required=$reboot }} | ConvertTo-Json -Compress"##
    )
}

/// Full DriverStore inventory as raw pnputil text.
fn snapshot() -> Result<serde_json::Value, SdkError> {
    host::log("[driverstore] snapshot");
    let out = host::run_command("pnputil /enum-drivers");
    if out.trim().is_empty() {
        return Err(SdkError::host_failed("pnputil produced no output"));
    }
    Ok(serde_json::json!({
        "tool": "snapshot",
        "source": "pnputil",
        "driver_text": out,
    }))
}

/// Exported rollback points under the driver store root.
fn list_exports() -> Result<serde_json::Value, SdkError> {
    host::log("[driverstore] list_exports");
    let out = host::run_command(LIST_EXPORTS_PS);
    let t = out.trim();
    if t.is_empty() {
        return Err(SdkError::host_failed("no output"));
    }
    let data: serde_json::Value = serde_json::from_str(t)?;
    Ok(serde_json::json!({ "tool": "list_exports", "data": data }))
}

/// Exports one driver package to a timestamped rollback folder.
fn export_driver(a: ExportArgs) -> Result<serde_json::Value, SdkError> {
    let Some(inf) = sanitize_arg(&a.published_name, false) else {
        return Err(SdkError::invalid_args("published_name (oemXX.inf) is required"));
    };
    host::log(&format!("[driverstore] export_driver {inf}"));
    let out = host::run_command(&export_ps(&inf));
    let t = out.trim();
    if t.is_empty() {
        return Err(SdkError::host_failed("no output"));
    }
    let data: serde_json::Value = serde_json::from_str(t)?;
    Ok(serde_json::json!({ "tool": "export_driver", "data": data }))
}

/// Uninstalls the current package (optional) and reinstalls a prior export.
fn rollback_driver(a: RollbackArgs) -> Result<serde_json::Value, SdkError> {
    let Some(restore_path) = sanitize_arg(&a.restore_path, true) else {
        return Err(SdkError::invalid_args("restore_path is required"));
    };
    if !restore_path
        .to_ascii_lowercase()
        .starts_with(r"c:\programdata\mtechdriverstore")
    {
        return Err(SdkError::invalid_args(
            "restore_path must be under C:\\ProgramData\\MTechDriverStore",
        ));
    }
    let delete_inf = a
        .delete_published_name
        .as_deref()
        .and_then(|v| sanitize_arg(v, false));
    host::log(&format!(
        "[driverstore] rollback_driver restore={restore_path} delete={delete_inf:?}"
    ));
    let out = host::run_command(&rollback_ps(&restore_path, delete_inf.as_deref()));
    let t = out.trim();
    if t.is_empty() {
        return Err(SdkError::host_failed("no output"));
    }
    let data: serde_json::Value = serde_json::from_str(t)?;
    Ok(serde_json::json!({ "tool": "rollback_driver", "data": data }))
}

mtech_plugin! {
    id: "com.mastertech.driverstore",
    name: "Driver Time Machine",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Full DriverStore inventory via 'pnputil /enum-drivers'. Returns raw pnputil text for the admin console to parse and persist as a driver_snapshot row.
        snapshot() => snapshot,
        /// List exported driver-package rollback points under C:\ProgramData\MTechDriverStore.
        list_exports() => list_exports,
        /// Export one driver package (rollback point) to C:\ProgramData\MTechDriverStore\<inf>_<timestamp> via 'pnputil /export-driver'.
        export_driver(ExportArgs) => export_driver,
        /// DESTRUCTIVE - confirm with a tech first. Optionally 'pnputil /delete-driver <published_name> /uninstall /force' the current package, then 'pnputil /add-driver <restore_path>\*.inf /subdirs /install' a previously exported one. Reboot may be required.
        rollback_driver(RollbackArgs) => rollback_driver,
    }
}
