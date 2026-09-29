//! XWorm V7.4 + ITarian/Comodo RMM remnant scanner & cleaner (SDK port).
//!
//! Read-only sweeps for XWorm loader/persistence/C2 and ITarian RMM artifacts,
//! plus a gated cleanup of the inert leftovers. Built for the Jared Stone IR
//! case; reusable for XWorm / fake-RMM incidents. Cleanup now refuses to remove
//! Comodo/ITarian registry keys when a live product of that name is installed.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

#[derive(Facet, Deserialize)]
struct CleanupArgs {
    /// Must be true to remove the inert ITarian/Comodo leftovers.
    confirm: Option<bool>,
}

fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

const SCAN_PS: &str = r#"$o=[ordered]@{}
$wg=Join-Path $env:LOCALAPPDATA 'WordGenius Technologies'
$o.WordGenius = if(Test-Path $wg){ Get-ChildItem -Force $wg -ea SilentlyContinue | Select-Object Name,Length,LastWriteTime } else { 'NOT PRESENT' }
$su=[Environment]::GetFolderPath('Startup')
$o.StartupFiles = Get-ChildItem -Force $su -ea SilentlyContinue | Select-Object Name,LastWriteTime
$o.StartupUrls = Get-ChildItem -Force (Join-Path $su '*.url') -ea SilentlyContinue | ForEach-Object { @{ name=$_.Name; content=(Get-Content $_.FullName -Raw -ea SilentlyContinue) } }
$o.Suspect = Get-CimInstance Win32_Process -ea SilentlyContinue | Where-Object { $_.Name -match 'jsc\.exe|AppLaunch|SwiftWrite|AutoIt3|ITSM|Rmm|cmdagent|Comodo' } | Select-Object Name,ProcessId,ParentProcessId,ExecutablePath,CommandLine
$o.C2 = Get-NetTCPConnection -ea SilentlyContinue | Where-Object { $_.RemoteAddress -eq '193.142.146.154' -or $_.RemotePort -eq 7004 } | Select-Object LocalPort,RemoteAddress,RemotePort,State,OwningProcess
$rh=@(); foreach($k in (Get-ChildItem 'HKCU:\Software' -ea SilentlyContinue)){ $v=Get-ItemProperty $k.PSPath -ea SilentlyContinue; if($null -ne $v.IsUpdate -or $null -ne $v.Uninstaller){ $rh += @{ key=$k.PSChildName; IsUpdate=[string]$v.IsUpdate; Uninstaller=[string]$v.Uninstaller } } }
$o.RegXworm = if($rh.Count){ $rh } else { 'none' }
$lt=Join-Path $env:TEMP 'Log.tmp'
$o.Keylog = if(Test-Path $lt){ @{ present=$true; size=(Get-Item $lt).Length; modified=[string](Get-Item $lt).LastWriteTime } } else { 'NOT PRESENT' }
$o.HostsActive = Get-Content (Join-Path $env:WINDIR 'System32\drivers\etc\hosts') -ea SilentlyContinue | Where-Object { $_ -and $_ -notmatch '^\s*#' }
$o.ItarianSvc = Get-Service -ea SilentlyContinue | Where-Object { $_.Name -match 'ITSM|Rmm|Comodo|cmdagent|CmdVirth' -or $_.DisplayName -match 'ITarian|Comodo|Endpoint Manager' } | Select-Object Name,DisplayName,Status,StartType
$o.ItarianDirs = @("$env:ProgramFiles\COMODO","${env:ProgramFiles(x86)}\COMODO","${env:ProgramFiles(x86)}\ITarian","$env:ProgramData\Comodo") | Where-Object { Test-Path $_ }
$uk='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
$o.ItarianUninstall = Get-ChildItem $uk -ea SilentlyContinue | ForEach-Object { Get-ItemProperty $_.PSPath -ea SilentlyContinue } | Where-Object { $_.DisplayName -match 'ITarian|Comodo|Endpoint Manager|Communication Client' } | Select-Object DisplayName,DisplayVersion,Publisher,InstallDate
$dl=Join-Path $env:USERPROFILE 'Downloads'
$o.DownloadsDroppers = Get-ChildItem -Force $dl -Recurse -ea SilentlyContinue | Where-Object { $_.Name -match 'Tax_Preparation|ORGANIZERCOPY|em_z3r5XEiH|installer_Win7-Win11' } | Select-Object FullName,Length,LastWriteTime
$o.SchedTasks = Get-ScheduledTask -ea SilentlyContinue | Where-Object { ($_.Actions.Execute -match 'WordGenius|SwiftWrite|AutoIt3|\.pif|AppLaunch') -or ($_.Actions.Arguments -match 'WordGenius|SwiftWrite') } | Select-Object TaskName,TaskPath
$o | ConvertTo-Json -Depth 5"#;

const ITARIAN_PS: &str = r#"$o=[ordered]@{}
$base="${env:ProgramFiles(x86)}\ITarian"
$o.Dir = Get-ChildItem -Force $base -Recurse -ea SilentlyContinue | Select-Object FullName,Length,LastWriteTime
$o.SvcByPath = Get-CimInstance Win32_Service -ea SilentlyContinue | Where-Object { $_.PathName -match 'ITarian|Comodo|ITSM|cmdagent|RmmService|EmCommunication|rmm|cis\.exe' } | Select-Object Name,DisplayName,State,StartMode,PathName
$o.ProcByPath = Get-CimInstance Win32_Process -ea SilentlyContinue | Where-Object { $_.ExecutablePath -match 'ITarian|Comodo|ITSM' } | Select-Object Name,ProcessId,ExecutablePath,CommandLine
$o.SchedTasks = Get-ScheduledTask -ea SilentlyContinue | Where-Object { ($_.Actions.Execute -match 'ITarian|Comodo|ITSM|cmdagent|rmm') -or ($_.TaskPath -match 'ITarian|Comodo|Endpoint') } | Select-Object TaskName,TaskPath,@{n='Exec';e={[string]$_.Actions.Execute}}
$o.RegKeys = @('HKLM:\SOFTWARE\ITarian','HKLM:\SOFTWARE\COMODO','HKLM:\SOFTWARE\WOW6432Node\ITarian','HKLM:\SOFTWARE\WOW6432Node\COMODO') | Where-Object { Test-Path $_ }
$uk='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
$o.Uninstall = Get-ChildItem $uk -ea SilentlyContinue | ForEach-Object { Get-ItemProperty $_.PSPath -ea SilentlyContinue } | Where-Object { $_.DisplayName -match 'ITarian|Comodo|Endpoint|Communication Client|Remote Control' -or $_.Publisher -match 'ITarian|Comodo' } | Select-Object DisplayName,DisplayVersion,Publisher,InstallDate,UninstallString
$o | ConvertTo-Json -Depth 5"#;

const REGDUMP_PS: &str = r#"function Dump($p){ if(Test-Path $p){ $r=[ordered]@{}; $r.values=(Get-ItemProperty $p -ea SilentlyContinue | Select-Object * -ExcludeProperty PSPath,PSParentPath,PSChildName,PSDrive,PSProvider); $r.subkeys=@(Get-ChildItem $p -Recurse -ea SilentlyContinue | ForEach-Object { @{ path=$_.Name; values=(Get-ItemProperty $_.PSPath -ea SilentlyContinue | Select-Object * -ExcludeProperty PSPath,PSParentPath,PSChildName,PSDrive,PSProvider) } }); $r } else { 'NOT PRESENT' } }
$o=[ordered]@{}
$o.COMODO = Dump 'HKLM:\SOFTWARE\COMODO'
$o.ITarian = Dump 'HKLM:\SOFTWARE\WOW6432Node\ITarian'
$o | ConvertTo-Json -Depth 10"#;

const CLEANUP_PS: &str = r#"$o=[ordered]@{}
$live = @(Get-Service -ea SilentlyContinue | Where-Object { $_.Name -match '(?i)cmdagent|ITSM|Rmm|Comodo|CmdVirth' }).Count -gt 0
$live = $live -or (Test-Path "${env:ProgramFiles(x86)}\COMODO") -or (Test-Path "$env:ProgramFiles\COMODO") -or (Test-Path "$env:ProgramData\Comodo")
$o.live_product_detected = $live
$d="${env:ProgramFiles(x86)}\ITarian"
if(Test-Path $d){ $cnt=(Get-ChildItem -Force $d -Recurse -ea SilentlyContinue | Measure-Object).Count; if($cnt -eq 0){ Remove-Item $d -Force -Recurse -ea SilentlyContinue; $o.ITarianDir = if(Test-Path $d){'STILL PRESENT'}else{'REMOVED (was empty)'} } else { $o.ITarianDir = "SKIPPED - not empty ($cnt items)" } } else { $o.ITarianDir = 'already absent' }
$res=[ordered]@{}
foreach($k in @('HKLM:\SOFTWARE\COMODO','HKLM:\SOFTWARE\WOW6432Node\ITarian')){ if($live){ $res[$k]='SKIPPED - live Comodo/ITarian product detected' } elseif(Test-Path $k){ Remove-Item $k -Recurse -Force -ea SilentlyContinue; $res[$k] = if(Test-Path $k){'STILL PRESENT'}else{'REMOVED'} } else { $res[$k]='already absent' } }
$o.RegKeys=$res
$o | ConvertTo-Json -Depth 4"#;

fn scan() -> Result<serde_json::Value, SdkError> {
    host::log("[xworm] scan");
    Ok(envelope("scan", host::run_command(SCAN_PS)))
}

fn itarian() -> Result<serde_json::Value, SdkError> {
    host::log("[xworm] itarian");
    Ok(envelope("itarian", host::run_command(ITARIAN_PS)))
}

fn regdump() -> Result<serde_json::Value, SdkError> {
    host::log("[xworm] regdump");
    Ok(envelope("regdump", host::run_command(REGDUMP_PS)))
}

fn cleanup(a: CleanupArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm != Some(true) {
        return Err(SdkError::invalid_args(
            "cleanup removes the inert ITarian/Comodo leftovers; pass confirm:true to proceed (it refuses to touch keys while a live Comodo/ITarian product is installed)",
        ));
    }
    host::log("[xworm] cleanup (confirmed)");
    Ok(envelope("cleanup", host::run_command(CLEANUP_PS)))
}

mtech_plugin! {
    id: "com.mastertech.xworm-remnant-scan",
    name: "XWorm/ITarian Remnant Scanner",
    version: "0.4.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Read-only sweep for XWorm V7.4 + ITarian RMM remnants (files, startup, processes, C2, registry, keylog, hosts, services, downloads, tasks).
        scan() => scan,
        /// Read-only deep-dive on the ITarian/Comodo RMM agent (dir, services, processes, tasks, registry, uninstall).
        itarian() => itarian,
        /// Read-only dump of HKLM SOFTWARE COMODO and HKLM SOFTWARE WOW6432Node ITarian (values + subkeys) for RMM tenant/enrollment intel.
        regdump() => regdump,
        /// Remove the inert ITarian/Comodo leftovers (empty ITarian folder, HKLM COMODO, WOW6432Node ITarian). Skips the registry keys when a live Comodo/ITarian product is present. Requires confirm:true.
        cleanup(CleanupArgs) => cleanup,
    }
}
