//! Read-only checks about this machine.
//!
//! Every check here runs to a value and returns it. The old shape spawned a
//! thread that only logged, so the queue had to guess from the log when the work
//! had finished; these report completion instead.

use std::time::Instant;

use displays::scripts::catalog::ScriptDef;
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

#[cfg(target_os = "windows")]
use crate::terminal_mode::tabs::scripts::script_categories::check_windows_activation;
#[cfg(target_os = "windows")]
use crate::utilities::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask};

/// Ids this executor claims. Everything else stays on the legacy path until it
/// is ported, which is what lets the two coexist.
const HANDLED: &[&str] = &[
    "windows-version",
    "is-windows-activated",
    "is-supereasybackup-installed",
    "is-webroot-installed",
    "is-superantispyware-installed",
    "is-hibernation-sleep-enabled",
    "are-there-scheduled-tasks-for-it",
    "any-recent-blue-screens",
    "when-was-the-last-service-date",
    "network-status",
    "onedrive-health",
    "acl-deny-scan",
];

pub struct InformationalExecutor;

impl ScriptExecutor for InformationalExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        HANDLED.contains(&id.as_str())
    }

    fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle {
        let (tx, done) = crossbeam::channel::bounded(1);
        let def = def.clone();
        let ctx = ctx.clone();
        let started = Instant::now();

        std::thread::spawn(move || {
            let result = run(&def, &ctx);
            let _ = tx.send(ScriptOutcome::plain(
                def.id.clone(),
                run_token,
                result,
                started.elapsed(),
            ));
        });

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    let category = def.category();
    let name = def.name.as_str();

    match def.id.as_str() {
        "windows-version" => {
            let version = sysinfo::System::long_os_version().unwrap_or_default();
            ctx.log_info(category, name, format!("Windows Version: {version}"));
            ScriptResult::Success(format!("Windows Version: {version}"))
        }
        "is-windows-activated" => check_activation(ctx, def),
        "is-supereasybackup-installed" => check_installed(ctx, def, "supereasybackup"),
        "is-webroot-installed" => check_installed(ctx, def, "webroot"),
        "is-superantispyware-installed" => check_installed(ctx, def, "superantispyware"),
        "is-hibernation-sleep-enabled" => check_power(ctx, def),
        "are-there-scheduled-tasks-for-it" => check_sas_scheduled_tasks(ctx, def),
        "any-recent-blue-screens" => bsod_scan(ctx, def),
        "network-status" => network_status(ctx, def),
        "onedrive-health" => onedrive_health(ctx, def),
        "acl-deny-scan" => acl_deny_scan(ctx, def),
        "when-was-the-last-service-date" => {
            super::not_implemented(ctx, def, "Service date check not yet implemented")
        }
        other => ScriptResult::Error(format!("informational executor does not run '{other}'")),
    }
}

#[cfg(target_os = "windows")]
fn check_activation(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    match check_windows_activation() {
        Ok(status) if status.license_status == 1 => {
            ctx.log_success(category, name, "Windows is activated");
            ScriptResult::Success("Windows is activated".into())
        }
        Ok(_) => {
            ctx.log_warning(category, name, "Windows is NOT activated");
            ScriptResult::Warning("Windows is NOT activated".into())
        }
        Err(e) => {
            let msg = format!("Check failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn check_installed(ctx: &ScriptContext, def: &ScriptDef, search_term: &str) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    let search = search_term.to_lowercase();
    ctx.log_info(category.clone(), name, format!("Searching for {search}..."));

    if let Ok(programs) = InstalledProgram::get_installed_programs() {
        for program in &programs {
            let display_name = program
                .display_name
                .clone()
                .unwrap_or_default()
                .to_lowercase();
            let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
            if display_name.contains(&search) || publisher.contains(&search) {
                let found = program.display_name.clone().unwrap_or_default();
                let version = program.display_version.clone().unwrap_or_default();
                ctx.log_success(category.clone(), name, format!("{search} Found!"));
                ctx.log_info(category.clone(), name, format!("  Display Name: {found}"));
                ctx.log_info(category, name, format!("  Version: {version}"));
                return ScriptResult::Success(format!("{found} {version}"));
            }
        }
    }

    // An AV product can be present without an uninstall entry.
    if let Ok(av_products) = AntiVirusProduct::query_installed() {
        for product in &av_products {
            if product.display_name.to_lowercase().contains(&search) {
                let msg = format!("{search} Found (AV): {}", product.display_name);
                ctx.log_success(category, name, msg.clone());
                return ScriptResult::Success(msg);
            }
        }
    }

    let msg = format!("{search} not installed");
    ctx.log_warning(category, name, msg.clone());
    ScriptResult::Warning(msg)
}

#[cfg(target_os = "windows")]
/// Reports the active plan and each sleep, hibernate and display timeout on AC and DC.
fn check_power(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    match super::powershell::run(POWER_REPORT) {
        Ok(output) => {
            let lines: Vec<String> = output
                .lines
                .into_iter()
                .filter(|line| !line.trim().is_empty())
                .collect();
            let enabled = lines
                .iter()
                .any(|line| line.contains("ENABLED on at least one setting"));
            let summary = lines.last().cloned().unwrap_or_default();
            for line in lines {
                ctx.log_info(category.clone(), name, line);
            }
            if enabled {
                ScriptResult::Warning(summary)
            } else {
                ScriptResult::Success(summary)
            }
        }
        Err(failure) => {
            if let Some(code) = failure.exit_code {
                ctx.log_info(category.clone(), name, format!("Exit code: {code:?}"));
            }
            let msg = format!("Error querying power options: {}", failure.message);
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn check_sas_scheduled_tasks(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Checking scheduled tasks...");

    match ScheduledTask::list_tasks() {
        Ok(tasks) => {
            let sas_tasks: Vec<String> = tasks
                .iter()
                .filter_map(|t| t.task_name.clone())
                .filter(|n| n.contains("SUPERAntiSpyware"))
                .collect();

            if sas_tasks.is_empty() {
                let msg = "No SAS scheduled tasks found";
                ctx.log_warning(category, name, msg);
                return ScriptResult::Warning(msg.into());
            }

            let msg = format!("Found {} SAS scheduled task(s)", sas_tasks.len());
            ctx.log_success(category.clone(), name, msg.clone());
            for task in &sas_tasks {
                ctx.log_info(category.clone(), name, format!("  • {task}"));
            }
            ScriptResult::Success(msg)
        }
        Err(e) => {
            let msg = format!("Failed to get tasks: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn bsod_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::bsod_scan::{self, BsodVerdict};

    let (category, name) = (def.category(), def.name.as_str());
    match bsod_scan::scan_blocking(bsod_scan::DEFAULT_DAYS) {
        Ok(scan) => {
            let verdict = scan.verdict();
            let mut lines = scan.report_lines();
            // The last line is the verdict, and it carries the entry severity.
            let summary = lines.pop().unwrap_or_default();
            for line in lines {
                ctx.log_info(category.clone(), name, line);
            }
            match verdict {
                BsodVerdict::Error => {
                    ctx.log_error(category, name, summary.clone());
                    ScriptResult::Error(summary)
                }
                BsodVerdict::Warning => {
                    ctx.log_warning(category, name, summary.clone());
                    ScriptResult::Warning(summary)
                }
                BsodVerdict::Clean => {
                    ctx.log_success(category, name, summary.clone());
                    ScriptResult::Success(summary)
                }
            }
        }
        Err(e) => {
            let msg = format!("BSOD check failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

/// Wi-Fi networks in range, WLAN connection state and the adapter list.
#[cfg(target_os = "windows")]
fn network_status(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::windows::net_adapter::{
        check_network_adapters, get_wlan_status, scan_wifi_networks,
    };

    let (category, name) = (def.category(), def.name.as_str());
    let mut failures = Vec::new();

    match scan_wifi_networks() {
        Ok(networks) => ctx.log_info(
            category.clone(),
            name,
            format!("wifi networks visible: {}", networks.len()),
        ),
        Err(e) => {
            let msg = format!("wifi scan error: {e}");
            ctx.log_error(category.clone(), name, msg.clone());
            failures.push(msg);
        }
    }
    // A WLAN status error is logged, not counted as a failure.
    match get_wlan_status() {
        Ok(()) => ctx.log_info(category.clone(), name, "wlan status: OK"),
        Err(e) => ctx.log_info(category.clone(), name, format!("wlan status: {e:?}")),
    }
    match check_network_adapters() {
        Ok(adapters) => ctx.log_info(
            category.clone(),
            name,
            format!("network adapters: {adapters:?}"),
        ),
        Err(e) => {
            let msg = format!("adapter check error: {e}");
            ctx.log_error(category.clone(), name, msg.clone());
            failures.push(msg);
        }
    }

    if failures.is_empty() {
        ctx.log_success(category, name, "Network checks complete");
        ScriptResult::Success("Network checks complete".into())
    } else {
        ScriptResult::Error(failures.join("; "))
    }
}

#[cfg(not(target_os = "windows"))]
fn network_status(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(target_os = "windows")]
fn onedrive_health(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    run_verdict_report(ctx, def, ONEDRIVE_HEALTH)
}

#[cfg(target_os = "windows")]
fn acl_deny_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    run_verdict_report(ctx, def, ACL_DENY_SCAN)
}

/// Runs a read-only PowerShell report and maps its final `VERDICT: <tag> <msg>`
/// line to the outcome; every other non-empty line is logged as info.
#[cfg(target_os = "windows")]
fn run_verdict_report(ctx: &ScriptContext, def: &ScriptDef, script: &str) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    match super::powershell::run(script) {
        Ok(output) => {
            let mut summary = String::new();
            let mut tag = String::from("OK");
            let mut body: Vec<String> = Vec::new();
            for line in output.lines {
                if let Some(rest) = line.trim_start().strip_prefix("VERDICT:") {
                    let rest = rest.trim();
                    let (t, msg) = rest.split_once(' ').unwrap_or((rest, ""));
                    tag = t.to_ascii_uppercase();
                    summary = msg.trim().to_string();
                } else if !line.trim().is_empty() {
                    body.push(line);
                }
            }
            for line in body {
                ctx.log_info(category.clone(), name, line);
            }
            if summary.is_empty() {
                summary = "no verdict reported".to_string();
            }
            match tag.as_str() {
                "FAIL" => {
                    ctx.log_error(category, name, summary.clone());
                    ScriptResult::Error(summary)
                }
                "WARN" => {
                    ctx.log_warning(category, name, summary.clone());
                    ScriptResult::Warning(summary)
                }
                _ => {
                    ctx.log_success(category, name, summary.clone());
                    ScriptResult::Success(summary)
                }
            }
        }
        Err(failure) => {
            let msg = format!("Failed: {}", failure.message);
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn onedrive_health(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn acl_deny_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn check_activation(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn check_installed(ctx: &ScriptContext, def: &ScriptDef, _search_term: &str) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn check_power(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn check_sas_scheduled_tasks(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn bsod_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn unsupported(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

#[cfg(target_os = "windows")]
const POWER_REPORT: &str = r#"
Write-Output ((powercfg /getactivescheme) -join '')
$settings = @(
    @{ Name='Sleep after';      Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='29f6c1db-86da-48c5-9fdb-f2b67b1f44da'; Units='Seconds' },
    @{ Name='Hibernate after';  Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='9d7815a6-7ee4-497e-8888-515a05f02364'; Units='Seconds' },
    @{ Name='Hybrid sleep';     Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='94ac6d29-73ce-41a6-809f-6363ba21b47e'; Units='OnOff'   },
    @{ Name='Turn off display'; Sub='7516b95f-f776-4464-8c53-06167f40cc99'; Guid='3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e'; Units='Seconds' }
)
$anyEnabled = $false
foreach ($s in $settings) {
    $out = powercfg /query SCHEME_CURRENT $s.Sub $s.Guid 2>$null
    $acMatch = $out | Select-String 'Current AC Power Setting Index:\s*(0x[0-9a-fA-F]+)'
    $dcMatch = $out | Select-String 'Current DC Power Setting Index:\s*(0x[0-9a-fA-F]+)'
    $ac = if ($acMatch) { $acMatch.Matches[0].Groups[1].Value } else { '0x00000000' }
    $dc = if ($dcMatch) { $dcMatch.Matches[0].Groups[1].Value } else { '0x00000000' }
    if ($s.Units -eq 'Seconds') {
        $acVal = [uint32]$ac
        $dcVal = [uint32]$dc
        Write-Output ("{0}: AC={1}s  DC={2}s" -f $s.Name, $acVal, $dcVal)
        if ($acVal -gt 0 -or $dcVal -gt 0) { $anyEnabled = $true }
    } else {
        $acOn = if ($ac -ne '0x00000000') { 'On' } else { 'Off' }
        $dcOn = if ($dc -ne '0x00000000') { 'On' } else { 'Off' }
        Write-Output ("{0}: AC={1}  DC={2}" -f $s.Name, $acOn, $dcOn)
        if ($acOn -eq 'On' -or $dcOn -eq 'On') { $anyEnabled = $true }
    }
}
$states = (powercfg /availablesleepstates 2>&1) -join "`n"
if ($states -match 'Hibernate') { Write-Output 'Hibernation available: YES' } else { Write-Output 'Hibernation available: NO' }
if ($anyEnabled) { Write-Output 'Sleep/Hibernation: ENABLED on at least one setting' } else { Write-Output 'Sleep/Hibernation: all timeouts at 0 (disabled)' }
"#;

#[cfg(target_os = "windows")]
const ONEDRIVE_HEALTH: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$anyElevated = $false
$running = $false
$profiles = 0

try {
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MtechElev {
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a, bool i, int pid);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr h, uint a, out IntPtr t);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, out uint info, uint len, out uint ret);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr h);
  public static string Check(int pid) {
    IntPtr h = OpenProcess(0x1000, false, pid);
    if (h == IntPtr.Zero) { return "unknown"; }
    IntPtr tok = IntPtr.Zero;
    if (!OpenProcessToken(h, 0x0008, out tok)) { CloseHandle(h); return "unknown"; }
    uint info = 0; uint ret = 0; string r = "unknown";
    if (GetTokenInformation(tok, 20, out info, 4, out ret)) { if (info != 0) { r = "elevated"; } else { r = "not-elevated"; } }
    CloseHandle(tok); CloseHandle(h);
    return r;
  }
}
'@
} catch { }

Write-Output '== OneDrive processes =='
$procs = Get-CimInstance Win32_Process -Filter "Name = 'OneDrive.exe'"
if (-not $procs) {
    Write-Output 'OneDrive.exe: not running'
} else {
    foreach ($p in $procs) {
        $running = $true
        $owner = Invoke-CimMethod -InputObject $p -MethodName GetOwner
        if ($owner.Domain) { $acct = $owner.Domain + '\' + $owner.User } else { $acct = [string]$owner.User }
        $elev = 'unknown'
        try { $elev = [MtechElev]::Check([int]$p.ProcessId) } catch { }
        if ($elev -eq 'elevated') { $anyElevated = $true }
        Write-Output ('PID ' + $p.ProcessId + '  user ' + $acct + '  session ' + $p.SessionId + '  ' + $elev)
    }
}

Write-Output '== OneDrive per-user state =='
$hku = Get-ChildItem 'Registry::HKEY_USERS' | Where-Object { $_.PSChildName -match '^S-1-5-21' -and $_.PSChildName -notmatch '_Classes$' }
foreach ($h in $hku) {
    $sid = $h.PSChildName
    $accountsKey = 'Registry::HKEY_USERS\' + $sid + '\Software\Microsoft\OneDrive\Accounts'
    if (-not (Test-Path $accountsKey)) { continue }
    $profiles = $profiles + 1
    $who = $sid
    try { $who = ([System.Security.Principal.SecurityIdentifier]$sid).Translate([System.Security.Principal.NTAccount]).Value } catch { }
    Write-Output ('user ' + $who + ' (' + $sid + ')')
    foreach ($acc in (Get-ChildItem $accountsKey)) {
        $props = Get-ItemProperty -Path $acc.PSPath
        Write-Output ('  account ' + $acc.PSChildName + '  email ' + [string]$props.UserEmail + '  folder ' + [string]$props.UserFolder)
        if ($props.LastSignInTime) { Write-Output ('    LastSignInTime ' + [string]$props.LastSignInTime) }
        if ($props.LastModifiedTime) { Write-Output ('    LastModifiedTime ' + [string]$props.LastModifiedTime) }
    }
    $sf = 'Registry::HKEY_USERS\' + $sid + '\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders'
    if (Test-Path $sf) {
        $sfp = Get-ItemProperty -Path $sf
        Write-Output ('  Desktop  -> ' + [string]$sfp.Desktop)
        Write-Output ('  Personal -> ' + [string]$sfp.Personal)
        Write-Output ('  Pictures -> ' + [string]$sfp.'My Pictures')
    }
}

$logRoot = Join-Path $env:LOCALAPPDATA 'Microsoft\OneDrive\logs'
if (Test-Path $logRoot) {
    $log = Get-ChildItem $logRoot -Recurse -File | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($log) {
        Write-Output ('== newest sync log: ' + $log.FullName + ' ==')
        Get-Content -LiteralPath $log.FullName -Tail 15 | ForEach-Object { Write-Output ('  ' + $_) }
    }
}

if ($anyElevated) {
    Write-Output 'VERDICT: FAIL OneDrive.exe is running elevated; it must run as the signed-in user.'
} elseif ((-not $running) -or ($profiles -eq 0)) {
    Write-Output 'VERDICT: WARN OneDrive is not running or no OneDrive profile was found.'
} else {
    Write-Output 'VERDICT: OK OneDrive is running non-elevated for the signed-in user.'
}
"##;

#[cfg(target_os = "windows")]
const ACL_DENY_SCAN: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$targets = New-Object System.Collections.Generic.List[string]
$targets.Add('C:\')
$skip = @('Public','Default','Default User','All Users','Public Desktop')
foreach ($u in (Get-ChildItem 'C:\Users' -Directory | Where-Object { $skip -notcontains $_.Name })) {
    $root = $u.FullName
    $targets.Add($root)
    $od = Join-Path $root 'OneDrive'
    if (Test-Path $od) { $targets.Add($od) }
    foreach ($d1 in (Get-ChildItem $root -Directory -Force)) {
        $targets.Add($d1.FullName)
        foreach ($d2 in (Get-ChildItem $d1.FullName -Directory -Force)) {
            $targets.Add($d2.FullName)
        }
    }
}

$seen = @{}
$hits = New-Object System.Collections.Generic.List[string]
foreach ($t in $targets) {
    if ($seen.ContainsKey($t)) { continue }
    $seen[$t] = $true
    $acl = Get-Acl -LiteralPath $t
    if (-not $acl) { continue }
    foreach ($ace in $acl.Access) {
        if ($ace.AccessControlType -ne 'Deny') { continue }
        if (([int]$ace.FileSystemRights) -eq 1) { continue }
        Write-Output ('DENY ' + $t)
        Write-Output ('  identity  ' + [string]$ace.IdentityReference)
        Write-Output ('  rights    ' + [string]$ace.FileSystemRights)
        Write-Output ('  inherited ' + [string]$ace.IsInherited)
        $hits.Add($t)
    }
}

if ($hits.Count -gt 0) {
    Write-Output '== SDDL of affected paths =='
    foreach ($p in ($hits | Select-Object -Unique)) {
        Write-Output ($p + '  ' + (Get-Acl -LiteralPath $p).Sddl)
    }
    Write-Output ('VERDICT: FAIL ' + $hits.Count + ' non-standard Deny ACE(s) found.')
} else {
    Write-Output 'VERDICT: OK No non-standard Deny ACEs beyond the profile-junction guards.'
}
"##;

#[cfg(test)]
mod informational_executor_tests {
    use super::*;
    use displays::scripts::catalog::{CATALOG, Surface};

    /// Catalog-known but implemented nowhere, so it is claimed only to turn a
    /// legacy name into a declarative skip.
    const STUBS: &[&str] = &["when-was-the-last-service-date"];

    /// Run only as a step of a composite.
    const STEPS: &[&str] = &["network-status"];

    #[test]
    fn every_claimed_id_is_a_real_informational_script() {
        for id in HANDLED {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(
                def.category(),
                displays::scripts::ScriptCategory::Informational,
                "{id} is not an informational script"
            );
            assert!(
                def.offered_on(Surface::Egui) || STUBS.contains(id) || STEPS.contains(id),
                "{id} is claimed but never offered in the tab"
            );
        }
    }

    #[test]
    fn the_stubs_are_offered_nowhere() {
        for id in STUBS {
            let def = CATALOG.get(&ScriptId::new(*id)).expect("catalog entry");
            assert!(!def.offered_on(Surface::Egui), "{id} is offered in the tab");
        }
    }

    /// The catalog probes must be read-only, so the RemoteExec 'read' guard passes them.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_probe_scripts_are_read_only() {
        use displays::plugins::mcp_bridge::state_changing_commands;
        for (name, body) in [("OneDrive Health", ONEDRIVE_HEALTH), ("ACL Deny Scan", ACL_DENY_SCAN)] {
            let found = state_changing_commands(body);
            assert!(found.is_empty(), "{name} is not read-only; guard flagged {found:?}");
        }
    }

    #[test]
    fn the_executor_claims_only_what_it_runs() {
        let executor = InformationalExecutor;
        for id in HANDLED {
            assert!(executor.handles(&ScriptId::new(*id)));
        }
        assert!(!executor.handles(&ScriptId::new("activate-cps")));
        assert!(!executor.handles(&ScriptId::new("stress-cpu")));
        // Still on the legacy path until ported.
        assert!(!executor.handles(&ScriptId::new("run-prechecks")));
    }
}
