//! Hardware diagnostics plugin (SDK port).
//!
//! Read-only hardware/crash/display surveys plus storage and battery health,
//! and a single gated `uninstall_program` in place of the old vendor-specific
//! uninstallers. WHEA reads the System log by provider (the Operational channel
//! does not exist). CPS/Webroot moved to com.mastertech.cps; minidump analysis
//! is host-native (minidump_analyze); DDU / Night Light / vendor one-offs dropped.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

/// Null-safe truncation helper prepended to event-reading scripts.
const T: &str =
    "function T($m,$n){if($null -eq $m){return ''};$s=[string]$m;if($s.Length -gt $n){$s.Substring(0,$n)}else{$s}};";

#[derive(Facet, Deserialize)]
struct ListSoftwareArgs {
    /// Case-insensitive substring matched against DisplayName or Publisher.
    filter: Option<String>,
}

#[derive(Facet, Deserialize)]
struct UninstallArgs {
    /// Exact-ish DisplayName substring of the program to remove.
    name: String,
    /// Must be true: this uninstalls software from the customer's machine.
    confirm: Option<bool>,
}

/// Parses PS JSON output into the tool envelope, stderr-safe.
fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

/// Allows a DisplayName filter with no PowerShell metacharacters.
fn sanitize_name(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 120 {
        return None;
    }
    let ok = v.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '(' | ')' | '+' | '&')
    });
    ok.then(|| v.to_string())
}

fn system_info() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] system_info");
    let cmd = "$os=Get-CimInstance Win32_OperatingSystem;$cs=Get-CimInstance Win32_ComputerSystem;$bios=Get-CimInstance Win32_BIOS;$up=(Get-Date)-$os.LastBootUpTime;[PSCustomObject]@{hostname=$cs.Name;os=$os.Caption;build=$os.BuildNumber;last_boot=$os.LastBootUpTime.ToString('o');uptime_hours=[math]::Round($up.TotalHours,1);ram_gb=[math]::Round($cs.TotalPhysicalMemory/1GB,2);bios_ver=$bios.SMBIOSBIOSVersion;bios_date=$bios.ReleaseDate.ToString('o')}|ConvertTo-Json -Compress";
    Ok(envelope("system_info", host::run_command(cmd)))
}

fn bsod_events() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] bsod_events");
    let cmd = format!("{T}try{{$e=Get-WinEvent -LogName System -EA Stop|Where-Object {{$_.Id -eq 41 -or $_.Id -eq 6008 -or $_.Id -eq 1001}}|Select-Object -First 20 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='type';E={{if($_.Id -eq 41){{'KernelPower'}}elseif($_.Id -eq 6008){{'UnexpectedShutdown'}}else{{'BSOD'}}}}}},@{{N='msg';E={{T $_.Message 250}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("bsod_events", host::run_command(&cmd)))
}

fn critical_events() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] critical_events");
    let cmd = format!("{T}try{{$e=Get-WinEvent -LogName System -MaxEvents 200 -EA Stop|Where-Object {{$_.Level -le 2}}|Select-Object -First 25 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='level';E={{if($_.Level -eq 1){{'Critical'}}else{{'Error'}}}}}},@{{N='id';E={{$_.Id}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='msg';E={{T $_.Message 200}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("critical_events", host::run_command(&cmd)))
}

fn whea_errors() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] whea_errors");
    let cmd = format!("{T}try{{$e=Get-WinEvent -FilterHashtable @{{LogName='System';ProviderName='Microsoft-Windows-WHEA-Logger'}} -MaxEvents 30 -EA Stop|Select-Object @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='level';E={{$_.LevelDisplayName}}}},@{{N='msg';E={{T $_.Message 300}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{ if($_.Exception.Message -match 'No events'){{'[]'}}else{{[PSCustomObject]@{{whea_query_error=$_.Exception.Message}}|ConvertTo-Json -Compress}} }}");
    Ok(envelope("whea_errors", host::run_command(&cmd)))
}

fn disk_health() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] disk_health");
    let cmd = "try{$d=Get-PhysicalDisk|Select-Object FriendlyName,MediaType,BusType,OperationalStatus,HealthStatus,@{N='SizeGB';E={[math]::Round($_.Size/1GB,1)}};$s=Get-WmiObject -Namespace root\\wmi -Class MSStorageDriver_FailurePredictStatus -EA SilentlyContinue|Select-Object InstanceName,PredictFailure,Reason;[PSCustomObject]@{disks=@($d);smart=@($s)}|ConvertTo-Json -Compress -Depth 3}catch{'{\"error\":\"query failed\"}'}";
    Ok(envelope("disk_health", host::run_command(cmd)))
}

fn storage_health() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] storage_health");
    let cmd = "try{$d=Get-PhysicalDisk|ForEach-Object{$rc=$_|Get-StorageReliabilityCounter -EA SilentlyContinue;[ordered]@{name=$_.FriendlyName;media=$_.MediaType;bus=$_.BusType;health=$_.HealthStatus;size_gb=[math]::Round($_.Size/1GB,1);wear=$rc.Wear;temperature_c=$rc.Temperature;temperature_max_c=$rc.TemperatureMax;power_on_hours=$rc.PowerOnHours;read_errors_total=$rc.ReadErrorsTotal;read_errors_uncorrected=$rc.ReadErrorsUncorrected;write_errors_total=$rc.WriteErrorsTotal;write_errors_uncorrected=$rc.WriteErrorsUncorrected;start_stop_cycles=$rc.StartStopCycleCount}};[PSCustomObject]@{disks=@($d)}|ConvertTo-Json -Compress -Depth 4}catch{'{\"error\":\"query failed\"}'}";
    Ok(envelope("storage_health", host::run_command(cmd)))
}

fn battery_health() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] battery_health");
    let cmd = r#"$tmp=Join-Path $env:TEMP 'mtech_batt.xml';
try{ powercfg /batteryreport /xml /output $tmp | Out-Null }catch{};
if(-not (Test-Path $tmp)){ '{"battery":"none_or_failed"}' }
else {
  try{
    [xml]$x=Get-Content $tmp -Raw;
    $b=$x.BatteryReport.Batteries.Battery;
    if(-not $b){ '{"battery":"none"}' }
    else {
      $rows=@($b|ForEach-Object{
        $design=[int64]$_.DesignCapacity; $full=[int64]$_.FullChargeCapacity;
        $health=if($design -gt 0){[math]::Round(100.0*$full/$design,1)}else{$null};
        [ordered]@{id=$_.id;design_capacity_mwh=$design;full_charge_capacity_mwh=$full;cycle_count=[int]$_.CycleCount;health_percent=$health}
      });
      [PSCustomObject]@{batteries=$rows}|ConvertTo-Json -Compress -Depth 4
    }
  }catch{ '{"error":"battery parse failed"}' }
}"#;
    Ok(envelope("battery_health", host::run_command(cmd)))
}

fn reliability_records() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] reliability_records");
    let cmd = format!("{T}try{{$r=Get-WmiObject -Class Win32_ReliabilityRecords -EA Stop|Where-Object {{$_.SourceName -eq 'Hardware Error' -or $_.ProductName -like '*hardware*' -or $_.SourceName -like '*Kernel*' -or $_.SourceName -like '*WHEA*' -or $_.EventIdentifier -lt 0}}|Sort-Object TimeGenerated -Descending|Select-Object -First 30 @{{N='time';E={{[Management.ManagementDateTimeConverter]::ToDateTime($_.TimeGenerated).ToString('o')}}}},@{{N='source';E={{$_.SourceName}}}},@{{N='product';E={{$_.ProductName}}}},@{{N='event_id';E={{$_.EventIdentifier}}}},@{{N='msg';E={{T $_.Message 300}}}};if($r){{$r|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("reliability_records", host::run_command(&cmd)))
}

fn tdr_gpu_events() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] tdr_gpu_events");
    let cmd = format!("{T}try{{$sys=Get-WinEvent -LogName System -MaxEvents 5000 -EA Stop|Where-Object {{$_.Id -eq 4101 -or $_.Id -eq 4109 -or ($_.Id -eq 14 -and ($_.ProviderName -like '*display*' -or $_.ProviderName -like '*nv*' -or $_.ProviderName -like '*amd*')) -or ($_.ProviderName -eq 'Microsoft-Windows-Kernel-Power' -and $_.Id -eq 137)}}|Select-Object -First 20 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='msg';E={{T $_.Message 300}}}};[PSCustomObject]@{{tdr_display=@($sys)}}|ConvertTo-Json -Compress -Depth 4}}catch{{'{{}}'}}");
    Ok(envelope("tdr_gpu_events", host::run_command(&cmd)))
}

fn driver_errors() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] driver_errors");
    let cmd = format!("{T}try{{$e=Get-WinEvent -LogName System -MaxEvents 5000 -EA Stop|Where-Object {{($_.Id -eq 7026 -or $_.Id -eq 7001 -or $_.Id -eq 7000 -or $_.Id -eq 219 -or $_.Id -eq 411) -and $_.Level -le 3}}|Select-Object -First 30 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='msg';E={{T $_.Message 250}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("driver_errors", host::run_command(&cmd)))
}

fn disk_errors() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] disk_errors");
    let cmd = format!("{T}try{{$e=Get-WinEvent -LogName System -MaxEvents 5000 -EA Stop|Where-Object {{($_.ProviderName -eq 'disk' -or $_.ProviderName -eq 'Ntfs' -or $_.ProviderName -like '*storport*' -or $_.ProviderName -like '*stornvme*' -or $_.ProviderName -eq 'volmgr') -and ($_.Id -eq 11 -or $_.Id -eq 51 -or $_.Id -eq 153 -or $_.Id -eq 9 -or $_.Id -eq 55 -or $_.Id -eq 57 -or $_.Level -le 3)}}|Select-Object -First 30 @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='msg';E={{T $_.Message 250}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{'[]'}}");
    Ok(envelope("disk_errors", host::run_command(&cmd)))
}

fn wer_hardware() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] wer_hardware");
    let cmd = "try{$paths=@(\"$env:ProgramData\\Microsoft\\Windows\\WER\\ReportQueue\",\"$env:ProgramData\\Microsoft\\Windows\\WER\\ReportArchive\");$reports=foreach($p in $paths){if(Test-Path $p){Get-ChildItem $p -Directory -EA SilentlyContinue|Where-Object {$_.Name -like 'LiveKernelEvent*' -or $_.Name -like 'Kernel*' -or $_.Name -like 'AppHang*'}|Sort-Object LastWriteTime -Descending|Select-Object -First 15 @{N='time';E={$_.LastWriteTime.ToString('o')}},@{N='report';E={$_.Name}},@{N='location';E={if($p -like '*Queue*'){'pending'}else{'archived'}}}}};if($reports){$reports|ConvertTo-Json -Compress}else{'[]'}}catch{'[]'}";
    Ok(envelope("wer_hardware", host::run_command(cmd)))
}

fn display_connections() -> Result<serde_json::Value, SdkError> {
    host::log("[hw-diag] display_connections");
    let cmd = "$vc=Get-CimInstance Win32_VideoController|Select-Object Name,DriverVersion,VideoProcessor,@{N='CurrentRes';E={\"$($_.CurrentHorizontalResolution)x$($_.CurrentVerticalResolution)\"}},@{N='Status';E={$_.Status}},@{N='DriverDate';E={$_.DriverDate}};$monitors=Get-CimInstance Win32_DesktopMonitor -EA SilentlyContinue|Select-Object Name,MonitorType,ScreenWidth,ScreenHeight,Status;$portKeys=Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Enum\\DISPLAY\\*\\*\\Device Parameters' -EA SilentlyContinue|Select-Object @{N='device';E={Split-Path (Split-Path $_.PSPath -Parent) -Leaf}},@{N='edid_exists';E={if($_.EDID){'true'}else{'false'}}};$gpu=Get-PnpDevice -Class Display -EA SilentlyContinue|Select-Object FriendlyName,Status,Problem,InstanceId;[PSCustomObject]@{video_controllers=@($vc);monitors=@($monitors);display_devices=@($portKeys);display_adapters=@($gpu)}|ConvertTo-Json -Compress -Depth 4";
    Ok(envelope("display_connections", host::run_command(cmd)))
}

fn list_software(a: ListSoftwareArgs) -> Result<serde_json::Value, SdkError> {
    let raw = a.filter.as_deref().unwrap_or("").trim().to_string();
    let f = if raw.is_empty() {
        String::new()
    } else {
        match sanitize_name(&raw) {
            Some(s) => s,
            None => {
                return Err(SdkError::invalid_args("filter contains unsupported characters"))
            }
        }
    };
    host::log(&format!("[hw-diag] list_software filter={f:?}"));
    let where_clause = if f.is_empty() {
        "$_.DisplayName".to_string()
    } else {
        format!("($_.DisplayName -like '*{f}*' -or $_.Publisher -like '*{f}*')")
    };
    let cmd = format!(
        "$reg=@('HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*');$sw=Get-ItemProperty $reg -EA SilentlyContinue|Where-Object {{ {where_clause} }}|Select-Object DisplayName,DisplayVersion,Publisher,@{{N='quiet';E={{if($_.QuietUninstallString){{$_.QuietUninstallString}}else{{$_.UninstallString}}}}}};if($sw){{$sw|ConvertTo-Json -Compress}}else{{'[]'}}"
    );
    Ok(envelope("list_software", host::run_command(&cmd)))
}

fn uninstall_program(a: UninstallArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm != Some(true) {
        return Err(SdkError::invalid_args(
            "uninstall_program removes software from the customer's machine; pass confirm:true to proceed",
        ));
    }
    let Some(name) = sanitize_name(&a.name) else {
        return Err(SdkError::invalid_args(
            "name is required and must contain no PowerShell metacharacters",
        ));
    };
    host::log(&format!("[hw-diag] uninstall_program {name}"));
    let cmd = format!(
        r#"$reg=@('HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*');
$t=Get-ItemProperty $reg -EA SilentlyContinue|Where-Object {{$_.DisplayName -like '*{name}*'}};
if(-not $t){{ [PSCustomObject]@{{matched=0;note='no installed program matched'}}|ConvertTo-Json -Compress }}
else {{
  $r=foreach($app in @($t)){{
    $us=if($app.QuietUninstallString){{$app.QuietUninstallString}}else{{$app.UninstallString}};
    if(-not $us){{ "no uninstall string for $($app.DisplayName)" }}
    elseif($us -match 'MsiExec'){{ $guid=([regex]'\{{[^}}]+\}}').Match($us).Value; $p=Start-Process msiexec -ArgumentList "/X $guid /qn /norestart" -Wait -PassThru; "MSI $($app.DisplayName): exit $($p.ExitCode)" }}
    else {{ $p=Start-Process cmd -ArgumentList "/c $us /S" -Wait -PassThru; "EXE $($app.DisplayName): exit $($p.ExitCode)" }}
  }};
  $still=@(Get-ItemProperty $reg -EA SilentlyContinue|Where-Object {{$_.DisplayName -like '*{name}*'}}).Count;
  [PSCustomObject]@{{matched=@($t).Count;results=@($r);still_present=$still}}|ConvertTo-Json -Compress -Depth 4
}}"#
    );
    Ok(envelope("uninstall_program", host::run_command(&cmd)))
}

mtech_plugin! {
    id: "com.mastertech.hw-diag",
    name: "HW Diagnostics",
    version: "0.7.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// OS, BIOS, last boot, uptime, RAM.
        system_info() => system_info,
        /// Unexpected shutdowns and BSODs (Event IDs 41, 6008, 1001).
        bsod_events() => bsod_events,
        /// Recent Critical/Error events from the Windows System log.
        critical_events() => critical_events,
        /// WHEA CPU/memory/PCIe faults (System log, WHEA-Logger provider). A query error is reported, not hidden as "no events".
        whea_errors() => whea_errors,
        /// Physical disk health and SMART failure prediction (SATA).
        disk_health() => disk_health,
        /// NVMe/SSD reliability counters: wear, temperature, error totals, power-on hours (covers drives SMART misses).
        storage_health() => storage_health,
        /// Battery design vs full-charge capacity, cycle count and health percent (laptops).
        battery_health() => battery_health,
        /// Win32_ReliabilityRecords hardware errors - the Reliability Monitor source.
        reliability_records() => reliability_records,
        /// GPU/display TDR events - timeout detection and recovery (freeze cause).
        tdr_gpu_events() => tdr_gpu_events,
        /// Driver load failures and service errors from the System log.
        driver_errors() => driver_errors,
        /// Disk I/O errors, paging errors, and Storport/NVMe events.
        disk_errors() => disk_errors,
        /// Windows Error Reporting hardware fault reports and live kernel event dumps.
        wer_hardware() => wer_hardware,
        /// Active display adapters, connected monitors, EDID presence, and adapter problem codes.
        display_connections() => display_connections,
        /// List installed software with uninstall strings, optionally filtered by a DisplayName/Publisher substring.
        list_software(ListSoftwareArgs) => list_software,
        /// Uninstall an installed program by DisplayName substring (MSI or EXE silent), then verify it is gone. Requires confirm:true.
        uninstall_program(UninstallArgs) => uninstall_program,
    }
}
