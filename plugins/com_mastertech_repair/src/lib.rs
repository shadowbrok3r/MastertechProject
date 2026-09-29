//! System Repair client plugin (SDK port).
//!
//! Windows component-store and file repair, TPM/BitLocker read-outs, and an
//! arbitrary PowerShell escape hatch. DISM/SFC run detached under
//! C:\ProgramData\MTechRepair because they outlast the host dispatch watchdog;
//! callers start them, then poll `repair_status`. Destructive tools
//! (`tpm_initialize_attempt`, `uninstall_superantispyware`) require confirm:true.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

const REPAIR_DIR: &str = r"C:\ProgramData\MTechRepair";

#[derive(Facet, Deserialize)]
struct RunCommandArgs {
    /// PowerShell command to execute.
    command: String,
    /// Max output bytes to capture (default 1000000).
    out_max: Option<u32>,
}

#[derive(Facet, Deserialize)]
struct RepairOpArgs {
    /// One of: sfc, dism_scan, dism_restore.
    op: String,
}

#[derive(Facet, Deserialize)]
struct RestorePointArgs {
    /// Checkpoint description (default "Mastertech repair").
    description: Option<String>,
}

#[derive(Facet, Deserialize)]
struct TpmInitArgs {
    /// Pass -AllowPhysicalPresence (default false).
    allow_phy_presence: Option<bool>,
    /// Must be true: clearing the TPM drops BitLocker's key protector (forcing a
    /// recovery-key prompt on next boot) and wipes Windows Hello / passkeys.
    confirm: Option<bool>,
}

#[derive(Facet, Deserialize)]
struct ConfirmArgs {
    /// Must be true to run this destructive action.
    confirm: Option<bool>,
}

/// Wraps a raw command result as valid JSON, parsing it when it is itself JSON.
fn envelope(tool: &str, result: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(result.trim())
        .unwrap_or(serde_json::Value::String(result));
    serde_json::json!({ "tool": tool, "output": output })
}

/// Maps a repair op name to its command and log basename.
fn op_command(op: &str) -> Option<(&'static str, &'static str)> {
    match op {
        "sfc" => Some(("sfc /scannow", "sfc")),
        "dism_scan" => Some(("DISM /Online /Cleanup-Image /ScanHealth", "dism_scan")),
        "dism_restore" => Some((
            "DISM /Online /Cleanup-Image /RestoreHealth",
            "dism_restore",
        )),
        _ => None,
    }
}

/// Launches a long repair op detached, writing its log and a status marker.
fn repair_start(a: RepairOpArgs) -> Result<serde_json::Value, SdkError> {
    let Some((cmd, base)) = op_command(&a.op) else {
        return Err(SdkError::invalid_args(
            "op must be one of: sfc, dism_scan, dism_restore",
        ));
    };
    host::log(&format!("[repair] repair_start {base}"));
    let script = format!(
        r#"$ErrorActionPreference='SilentlyContinue'
$dir='{REPAIR_DIR}'
New-Item -ItemType Directory -Force $dir | Out-Null
$log=Join-Path $dir '{base}.log'
$st=Join-Path $dir '{base}.status'
Set-Content -Encoding utf8 $st 'running'
$inner="`$ErrorActionPreference='SilentlyContinue'; {cmd} *>&1 | Out-File -Encoding utf8 -FilePath '$log'; Set-Content -Encoding utf8 '$st' 'done'"
Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-Command',$inner | Out-Null
[PSCustomObject]@{{ op='{base}'; status='running'; log=$log }} | ConvertTo-Json -Compress"#
    );
    Ok(envelope("repair_start", host::run_command(&script)))
}

/// Reads the status marker and log tail for a repair op.
fn repair_status(a: RepairOpArgs) -> Result<serde_json::Value, SdkError> {
    let Some((_, base)) = op_command(&a.op) else {
        return Err(SdkError::invalid_args(
            "op must be one of: sfc, dism_scan, dism_restore",
        ));
    };
    let script = format!(
        r#"$ErrorActionPreference='SilentlyContinue'
$dir='{REPAIR_DIR}'
$log=Join-Path $dir '{base}.log'
$st=Join-Path $dir '{base}.status'
$status=if(Test-Path $st){{(Get-Content $st -Raw).Trim()}}else{{'not_started'}}
$tail=if(Test-Path $log){{(Get-Content $log -Tail 40 | Out-String)}}else{{''}}
[PSCustomObject]@{{ op='{base}'; status=$status; log_tail=$tail }} | ConvertTo-Json -Compress"#
    );
    Ok(envelope("repair_status", host::run_command(&script)))
}

/// Creates a System Restore checkpoint (best-effort; enables protection first).
fn restore_point_create(a: RestorePointArgs) -> Result<serde_json::Value, SdkError> {
    let desc = a
        .description
        .as_deref()
        .unwrap_or("Mastertech repair")
        .replace('\'', "");
    host::log("[repair] restore_point_create");
    let script = format!(
        r#"$ErrorActionPreference='SilentlyContinue'
try {{ Enable-ComputerRestore -Drive 'C:\' -EA SilentlyContinue }} catch {{}}
New-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore' -Name SystemRestorePointCreationFrequency -Value 0 -PropertyType DWord -Force | Out-Null
try {{
  Checkpoint-Computer -Description '{desc}' -RestorePointType MODIFY_SETTINGS -EA Stop
  $ok=$true
}} catch {{ $ok=$false; $err=$_.Exception.Message }}
$rp=Get-ComputerRestorePoint -EA SilentlyContinue | Select-Object -Last 1 SequenceNumber,Description,CreationTime
[PSCustomObject]@{{ created=$ok; error=$err; latest=$rp }} | ConvertTo-Json -Compress -Depth 4"#
    );
    Ok(envelope("restore_point_create", host::run_command(&script)))
}

fn dism_restore_health() -> Result<serde_json::Value, SdkError> {
    repair_start(RepairOpArgs { op: "dism_restore".to_string() })
}

fn sfc_scannow() -> Result<serde_json::Value, SdkError> {
    repair_start(RepairOpArgs { op: "sfc".to_string() })
}

fn chkdsk_schedule() -> Result<serde_json::Value, SdkError> {
    host::log("[repair] chkdsk_schedule");
    let out = host::run_command(
        r#"$o = 'Y' | chkdsk C: /f 2>&1 | Out-String
$sched = $o -match 'checked the next time' -or $o -match 'schedule'
[PSCustomObject]@{ scheduled=$sched; output=$o.Trim() } | ConvertTo-Json -Compress"#,
    );
    Ok(envelope("chkdsk_schedule", out))
}

fn uninstall_superantispyware(a: ConfirmArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm != Some(true) {
        return Err(SdkError::invalid_args(
            "uninstall_superantispyware removes CPS-bundled software the shop sells; pass confirm:true to proceed",
        ));
    }
    host::log("[repair] uninstall_superantispyware");
    let find = host::run_command(
        r#"Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -EA SilentlyContinue | Where-Object { $_.DisplayName -like '*SuperAntiSpyware*' -or $_.DisplayName -like '*SUPERAnti*' } | Select-Object DisplayName,UninstallString,QuietUninstallString | ConvertTo-Json -Compress"#,
    );
    let uninst = host::run_command(
        r#"$t = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -EA SilentlyContinue | Where-Object { $_.DisplayName -like '*SuperAntiSpyware*' -or $_.DisplayName -like '*SUPERAnti*' }
$r=foreach($app in $t){ $us = if($app.QuietUninstallString){$app.QuietUninstallString}else{$app.UninstallString}; if($us){ if($us -match 'MsiExec'){$guid=([regex]'\{[^}]+\}').Match($us).Value; $p=Start-Process msiexec -ArgumentList "/X $guid /qn /norestart" -Wait -PassThru; "MSI $($app.DisplayName): exit $($p.ExitCode)"}else{$p=Start-Process cmd -ArgumentList "/c $us /S" -Wait -PassThru; "EXE $($app.DisplayName): exit $($p.ExitCode)"}}else{"no uninstall string for $($app.DisplayName)"}}
$still = [bool](Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -EA SilentlyContinue | Where-Object { $_.DisplayName -like '*SUPERAnti*' })
[PSCustomObject]@{ uninstall=@($r); still_installed=$still } | ConvertTo-Json -Compress"#,
    );
    let drv = host::run_command(
        r#"$r=foreach($d in @('SASDIFSV','SASKUTIL')){ $s=Get-Service $d -EA SilentlyContinue; if($s){Stop-Service $d -Force -EA SilentlyContinue; Set-Service $d -StartupType Disabled -EA SilentlyContinue; & sc.exe delete $d *>$null; $gone=-not (Get-Service $d -EA SilentlyContinue); "$d service delete requested (gone=$gone)"}else{$k="HKLM:\System\CurrentControlSet\Services\$d"; if(Test-Path $k){Remove-Item $k -Recurse -Force -EA SilentlyContinue; "$d registry key removed"}else{"$d not present"}}}
[PSCustomObject]@{ drivers=@($r) } | ConvertTo-Json -Compress"#,
    );
    Ok(serde_json::json!({
        "tool": "uninstall_superantispyware",
        "found": serde_json::from_str::<serde_json::Value>(find.trim()).unwrap_or(serde_json::Value::String(find)),
        "uninstall": serde_json::from_str::<serde_json::Value>(uninst.trim()).unwrap_or(serde_json::Value::String(uninst)),
        "drivers": serde_json::from_str::<serde_json::Value>(drv.trim()).unwrap_or(serde_json::Value::String(drv)),
    }))
}

fn run_command(a: RunCommandArgs) -> Result<serde_json::Value, SdkError> {
    if a.command.trim().is_empty() {
        return Err(SdkError::invalid_args("command is required"));
    }
    let cap = a.out_max.unwrap_or(1_000_000).clamp(1024, 16_000_000) as usize;
    host::log(&format!("[repair] run_command: {}", a.command));
    Ok(envelope("run_command", host::run_command_capped(&a.command, cap)))
}

fn tpm_win11_readiness() -> Result<serde_json::Value, SdkError> {
    host::log("[repair] tpm_win11_readiness");
    let out = host::run_command(
        r#"$o=[ordered]@{}
try{$o.secure_boot_confirm=[bool](Confirm-SecureBootUEFI)}catch{$o.secure_boot_confirm_err=$_.Exception.Message}
try{$s=Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State' -EA SilentlyContinue;$o.UEFISecureBootEnabled_reg=$s.UEFISecureBootEnabled;$o.SetupMode_reg=$s.SetupMode}catch{}
try{$o.firmware_type=(Get-ComputerInfo -Property BiosFirmwareType -EA Stop).BiosFirmwareType}catch{}
try{$d=Get-Disk -EA SilentlyContinue|Where-Object IsBoot|Select-Object -First 1;$o.boot_partition_style=[string]$d.PartitionStyle}catch{}
try{$t=Get-Tpm -EA SilentlyContinue;$o.get_tpm=@{TpmPresent=$t.TpmPresent;TpmReady=$t.TpmReady;TpmEnabled=$t.TpmEnabled;TpmActivated=$t.TpmActivated;ManufacturerId=$t.ManufacturerId}}catch{$o.get_tpm_err=$_.Exception.Message}
try{$w=Get-CimInstance Win32_Tpm -Namespace root\CIMV2\Security\MicrosoftTpm -EA SilentlyContinue;if($w){$o.tpm_spec_version=$w.SpecVersion}}catch{}
try{$o.tpm_devices=(Get-PnpDevice -EA SilentlyContinue|Where-Object{$_.FriendlyName -match 'TPM|Trusted Platform|Pluton'}|Select FriendlyName,Status,Problem|ConvertTo-Json -Compress)}catch{}
try{$o.os_build=[int]([System.Environment]::OSVersion.Version.Build)}catch{}
$o|ConvertTo-Json -Compress -Depth 6"#,
    );
    Ok(envelope("tpm_win11_readiness", out))
}

fn bitlocker_status() -> Result<serde_json::Value, SdkError> {
    host::log("[repair] bitlocker_status");
    let out = host::run_command(
        r#"try{$v=Get-BitLockerVolume -EA Stop;$r=$v|ForEach-Object{[PSCustomObject]@{MountPoint=$_.MountPoint;VolumeStatus=[string]$_.VolumeStatus;ProtectionStatus=[string]$_.ProtectionStatus;EncryptionPercentage=$_.EncryptionPercentage;LockStatus=[string]$_.LockStatus;KeyProtectors=@($_.KeyProtector|ForEach-Object{[string]$_.KeyProtectorType})}};[PSCustomObject]@{volumes=@($r)}|ConvertTo-Json -Compress -Depth 5}catch{[PSCustomObject]@{error=$_.Exception.Message}|ConvertTo-Json -Compress}"#,
    );
    Ok(envelope("bitlocker_status", out))
}

fn tpm_initialize_attempt(a: TpmInitArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm != Some(true) {
        return Err(SdkError::invalid_args(
            "tpm_initialize_attempt clears the TPM: this drops BitLocker's key protector (recovery key needed at next boot) and wipes Windows Hello / passkeys. Check bitlocker_status first, then pass confirm:true to proceed",
        ));
    }
    host::log("[repair] tpm_initialize_attempt (confirmed)");
    let ps = if a.allow_phy_presence == Some(true) {
        r#"try { Initialize-Tpm -AllowClear -AllowPhysicalPresence -EA Stop | ConvertTo-Json -Compress } catch { $_.Exception.Message }"#
    } else {
        r#"try { Initialize-Tpm -AllowClear -EA Stop | ConvertTo-Json -Compress } catch { $_.Exception.Message }"#
    };
    Ok(envelope("tpm_initialize_attempt", host::run_command(ps)))
}

mtech_plugin! {
    id: "com.mastertech.repair",
    name: "System Repair",
    version: "0.4.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Start DISM /Online /Cleanup-Image /RestoreHealth in the background (survives the dispatch watchdog). Poll repair_status with op "dism_restore".
        dism_restore_health() => dism_restore_health,
        /// Start SFC /scannow in the background. Poll repair_status with op "sfc".
        sfc_scannow() => sfc_scannow,
        /// Start a long repair op detached under C:\ProgramData\MTechRepair. op: sfc | dism_scan | dism_restore.
        repair_start(RepairOpArgs) => repair_start,
        /// Read the status marker and log tail for a repair op. op: sfc | dism_scan | dism_restore.
        repair_status(RepairOpArgs) => repair_status,
        /// Create a System Restore checkpoint (enables protection and clears the 24h throttle first). Run before any change.
        restore_point_create(RestorePointArgs) => restore_point_create,
        /// Find and uninstall SUPERAntiSpyware and remove its orphaned SASDIFSV/SASKUTIL drivers. Destructive: requires confirm:true (this is CPS-bundled software the shop sells).
        uninstall_superantispyware(ConfirmArgs) => uninstall_superantispyware,
        /// Schedule chkdsk C: /f for the next reboot.
        chkdsk_schedule() => chkdsk_schedule,
        /// Run an arbitrary PowerShell command and return its output.
        run_command(RunCommandArgs) => run_command,
        /// Read-only: Secure Boot, firmware type, boot partition style, TPM presence/readiness, and OS build for Win11 readiness.
        tpm_win11_readiness() => tpm_win11_readiness,
        /// BitLocker summary for all volumes, including each volume's key protectors.
        bitlocker_status() => bitlocker_status,
        /// Initialize-Tpm -AllowClear. Destructive: clears the TPM (BitLocker recovery-key prompt, Windows Hello/passkeys wiped). Requires confirm:true.
        tpm_initialize_attempt(TpmInitArgs) => tpm_initialize_attempt,
    }
}
