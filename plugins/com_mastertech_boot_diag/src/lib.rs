//! Boot & Secure Boot diagnostics plugin (SDK port).
//!
//! Merges the former boot-config-diag and secure-boot-diag plugins: firmware
//! mode, BCD entries/flags, boot volumes, Secure Boot / TPM state, Code
//! Integrity blocks, unsigned boot drivers, and driver-load failures. All
//! read-only.

use mtech_plugin_sdk::{host, mtech_plugin, SdkError};

/// Null-safe truncation helper prepended to event-reading scripts.
const T: &str =
    "function T($m,$n){if($null -eq $m){return ''};$s=[string]$m;if($s.Length -gt $n){$s.Substring(0,$n)}else{$s}};";

fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

fn firmware_boot_mode() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] firmware_boot_mode");
    let cmd = r#"$fw='unknown';
try{$fw=(Get-ComputerInfo -Property BiosFirmwareType -EA Stop).BiosFirmwareType}catch{};
$sb='unknown';
try{$sb=[string](Confirm-SecureBootUEFI)}catch{$sb='not_uefi_or_denied'};
$disk=Get-Disk -EA SilentlyContinue|Where-Object IsBoot|Select-Object -First 1 Number,PartitionStyle,@{N='model';E={$_.FriendlyName}},@{N='size_gb';E={[math]::Round($_.Size/1GB,1)}};
[PSCustomObject]@{firmware_type=$fw;secure_boot=$sb;boot_disk=$disk}|ConvertTo-Json -Compress -Depth 4"#;
    Ok(envelope("firmware_boot_mode", host::run_command(cmd)))
}

fn boot_entries() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] boot_entries");
    let cmd = r#"try{$t=(bcdedit /enum all 2>&1|Out-String);if(-not $t){$t='no output (run elevated)'};[PSCustomObject]@{bcdedit=$t}|ConvertTo-Json -Compress}catch{[PSCustomObject]@{bcdedit=('error: '+$_.Exception.Message)}|ConvertTo-Json -Compress}"#;
    Ok(envelope("boot_entries", host::run_command(cmd)))
}

fn boot_volumes() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] boot_volumes");
    let cmd = r#"try{
  $disks=Get-Disk -EA SilentlyContinue|Select-Object Number,@{N='model';E={$_.FriendlyName}},OperationalStatus,HealthStatus,PartitionStyle,@{N='size_gb';E={[math]::Round($_.Size/1GB,1)}},IsBoot,IsSystem,IsOffline;
  $vols=Get-Volume -EA SilentlyContinue|Where-Object {$_.DriveLetter}|Select-Object DriveLetter,FileSystemLabel,FileSystem,@{N='size_gb';E={[math]::Round($_.Size/1GB,1)}},HealthStatus,DriveType;
  $osvol=$null;
  foreach($v in $vols){ $p=($v.DriveLetter+':\Windows\System32\ntoskrnl.exe'); if(Test-Path $p){$osvol=[string]$v.DriveLetter;break} };
  $offline=@($disks|Where-Object {$_.IsOffline}).Count;
  [PSCustomObject]@{os_volume=$osvol;offline_disk_count=$offline;disks=$disks;volumes=$vols}|ConvertTo-Json -Compress -Depth 4
}catch{'{"error":"query failed"}'}"#;
    Ok(envelope("boot_volumes", host::run_command(cmd)))
}

fn boot_flags() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] boot_flags");
    let cmd = r#"try{
  $bcd=(bcdedit 2>&1|Out-String);
  $safeboot=if($bcd -match '(?im)^\s*safeboot'){'true'}else{'false'};
  $recovery=if($bcd -match '(?im)recoveryenabled\s+Yes'){'true'}else{'false'};
  $state='unknown';
  try{$state=(Get-CimInstance Win32_ComputerSystem -EA Stop).BootupState}catch{};
  [PSCustomObject]@{safeboot_flag_set=$safeboot;recovery_enabled=$recovery;current_bootup_state=$state}|ConvertTo-Json -Compress
}catch{'{"error":"query failed"}'}"#;
    Ok(envelope("boot_flags", host::run_command(cmd)))
}

fn secure_boot_state() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] secure_boot_state");
    let cmd = r#"$sb='unknown';
try{$sb=[string](Confirm-SecureBootUEFI)}catch{$sb='not_uefi_or_denied'};
$fw='unknown';
try{$fw=(Get-ComputerInfo -Property BiosFirmwareType -EA Stop).BiosFirmwareType}catch{};
$tpm='unavailable';
try{$t=Get-Tpm -EA Stop;$tpm=[ordered]@{present=$t.TpmPresent;ready=$t.TpmReady;enabled=$t.TpmEnabled}}catch{};
$disk=Get-Disk -EA SilentlyContinue|Where-Object IsBoot|Select-Object -First 1 Number,PartitionStyle,@{N='model';E={$_.FriendlyName}};
[PSCustomObject]@{secure_boot_enabled=$sb;firmware_type=$fw;tpm=$tpm;boot_disk=$disk}|ConvertTo-Json -Compress -Depth 4"#;
    Ok(envelope("secure_boot_state", host::run_command(cmd)))
}

fn code_integrity_events() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] code_integrity_events");
    let cmd = format!("{T}try{{$e=Get-WinEvent -LogName 'Microsoft-Windows-CodeIntegrity/Operational' -MaxEvents 300 -EA Stop|Where-Object {{$_.Id -eq 3001 -or $_.Id -eq 3002 -or $_.Id -eq 3004 -or $_.Id -eq 3010 -or $_.Id -eq 3023 -or $_.Id -eq 3033 -or $_.Id -eq 3034}}|Select-Object -First 30 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='msg';E={{T $_.Message 300}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("code_integrity_events", host::run_command(&cmd)))
}

fn unsigned_boot_drivers() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] unsigned_boot_drivers");
    let cmd = r#"try{
  $drv='C:\Windows\System32\drivers';
  $bad=Get-ChildItem $drv -Filter *.sys -EA SilentlyContinue|ForEach-Object{
    $s=Get-AuthenticodeSignature $_.FullName -EA SilentlyContinue;
    if($s -and $s.Status -ne 'Valid'){[PSCustomObject]@{name=$_.Name;status=[string]$s.Status;signer=if($s.SignerCertificate){$s.SignerCertificate.Subject}else{'none'}}}
  };
  if($bad){$bad|Select-Object -First 40|ConvertTo-Json -Compress}else{'[]'}
}catch{'[]'}"#;
    Ok(envelope("unsigned_boot_drivers", host::run_command(cmd)))
}

fn driver_load_failures() -> Result<serde_json::Value, SdkError> {
    host::log("[boot-diag] driver_load_failures");
    let cmd = format!("{T}try{{$e=Get-WinEvent -LogName System -MaxEvents 5000 -EA Stop|Where-Object {{$_.Id -eq 7026 -or $_.Id -eq 7000 -or $_.Id -eq 7001 -or $_.Id -eq 7011 -or $_.Id -eq 219}}|Select-Object -First 30 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='msg';E={{T $_.Message 300}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("driver_load_failures", host::run_command(&cmd)))
}

mtech_plugin! {
    id: "com.mastertech.boot-diag",
    name: "Boot & Secure Boot Diagnostics",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Firmware type (UEFI/Legacy), Secure Boot state, and the boot disk's partition style (GPT/MBR). A GPT disk booting Legacy is a common no-boot cause.
        firmware_boot_mode() => firmware_boot_mode,
        /// Raw 'bcdedit /enum all' - Boot Manager and loader entries. Read-only.
        boot_entries() => boot_entries,
        /// Disks, partitions and volumes; identifies which volume holds \Windows and flags offline disks or a missing OS volume.
        boot_volumes() => boot_volumes,
        /// BCD safeboot flag, recovery-enabled state, and current bootup mode (normal vs safe).
        boot_flags() => boot_flags,
        /// Secure Boot enabled/disabled, UEFI vs legacy, boot partition style, and TPM presence/readiness.
        secure_boot_state() => secure_boot_state,
        /// CodeIntegrity/Operational events (3001-3034) where a driver/image was blocked or flagged for an invalid/unsigned signature.
        code_integrity_events() => code_integrity_events,
        /// Boot drivers under System32\drivers whose Authenticode signature is not Valid (invalid, unsigned, or untrusted).
        unsigned_boot_drivers() => unsigned_boot_drivers,
        /// System-log driver/service load failures (7026, 7000, 7001, 7011, 219), including 0xC0000428 invalid-signature failures.
        driver_load_failures() => driver_load_failures,
    }
}
