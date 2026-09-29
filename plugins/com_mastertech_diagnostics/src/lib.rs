//! General diagnostics plugin (SDK port).
//!
//! Read-only system/process/disk/event surveys plus a generic event query and
//! an app-crash summary. WHEA reads the System log by provider. Retired vs the
//! legacy plugin: burn_* (use the host StressTests scripts), dump analysis (use
//! minidump_analyze), the MSI/GameInput/VSSrv one-offs, and the Wi-Fi tools
//! (moving to a dedicated network plugin).

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

/// Null-safe truncation helper prepended to event-reading scripts.
const T: &str =
    "function T($m,$n){if($null -eq $m){return ''};$s=[string]$m;if($s.Length -gt $n){$s.Substring(0,$n)}else{$s}};";

#[derive(Facet, Deserialize)]
struct EventQueryArgs {
    /// Event log: System, Application, Security, or Setup (default System).
    log: Option<String>,
    /// Level filter: 1=Critical, 2=Error, 3=Warning, 4=Info.
    levels: Option<Vec<u32>>,
    /// Event IDs to match.
    ids: Option<Vec<u32>>,
    /// Provider names to match.
    providers: Option<Vec<String>>,
    /// ISO timestamp lower bound (e.g. 2026-01-01T00:00:00).
    since: Option<String>,
    /// Max events (default 40, cap 200).
    max: Option<u32>,
}

#[derive(Facet, Deserialize)]
struct AppCrashArgs {
    /// Look back this many days (default 14, cap 90).
    days: Option<u32>,
}

fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

/// Allows only characters safe to interpolate into a single-quoted PS literal.
fn sanitize_token(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 80 {
        return None;
    }
    v.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
        .then(|| v.to_string())
}

/// Allows an ISO-8601-ish timestamp with no quotes.
fn sanitize_ts(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 40 {
        return None;
    }
    v.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | 'T' | ' ' | '.' | 'Z' | '+'))
        .then(|| v.to_string())
}

fn system_summary() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] system_summary");
    let ps = "$os = Get-CimInstance Win32_OperatingSystem; $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1; $uptime = (Get-Date) - $os.LastBootUpTime; [PSCustomObject]@{ Hostname = $env:COMPUTERNAME; OS = $os.Caption; OSBuild = $os.BuildNumber; Uptime = \"$([int]$uptime.TotalHours)h $($uptime.Minutes)m\"; TotalRAM_GB = [math]::Round($os.TotalVisibleMemorySize/1MB,2); FreeRAM_GB = [math]::Round($os.FreePhysicalMemory/1MB,2); CPU = $cpu.Name; Cores = $cpu.NumberOfCores; Threads = $cpu.NumberOfLogicalProcessors } | ConvertTo-Json -Compress";
    Ok(envelope("system_summary", host::run_command(ps)))
}

fn top_processes() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] top_processes");
    Ok(envelope("top_processes", host::run_command("Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First 15 Name,Id,@{N='RAM_MB';E={[math]::Round($_.WorkingSet64/1MB,1)}},@{N='CPU_sec';E={[math]::Round($_.CPU,1)}} | ConvertTo-Json -Compress")))
}

fn disk_info() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] disk_info");
    Ok(envelope("disk_info", host::run_command("Get-PSDrive -PSProvider FileSystem | Select-Object Name,@{N='Total_GB';E={if($_.Used+$_.Free -gt 0){[math]::Round(($_.Used+$_.Free)/1GB,1)}else{0}}},@{N='Free_GB';E={[math]::Round($_.Free/1GB,1)}},@{N='Used_GB';E={[math]::Round($_.Used/1GB,1)}},@{N='Pct_Free';E={if($_.Used+$_.Free -gt 0){[math]::Round($_.Free/($_.Used+$_.Free)*100,1)}else{0}}} | ConvertTo-Json -Compress")))
}

fn recent_system_errors() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] recent_system_errors");
    let cmd = format!("{T}Get-WinEvent -FilterHashtable @{{LogName='System';Level=1,2}} -MaxEvents 15 -ErrorAction SilentlyContinue | Select-Object TimeCreated,Id,LevelDisplayName,ProviderName,@{{N='Message';E={{T ($_.Message -replace '\\s+',' ') 300}}}} | ConvertTo-Json -Compress");
    Ok(envelope("recent_system_errors", host::run_command(&cmd)))
}

fn recent_app_crashes() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] recent_app_crashes");
    let cmd = format!("{T}Get-WinEvent -FilterHashtable @{{LogName='Application';Level=1,2}} -MaxEvents 15 -ErrorAction SilentlyContinue | Select-Object TimeCreated,Id,LevelDisplayName,ProviderName,@{{N='Message';E={{T ($_.Message -replace '\\s+',' ') 300}}}} | ConvertTo-Json -Compress");
    Ok(envelope("recent_app_crashes", host::run_command(&cmd)))
}

fn stopped_auto_services() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] stopped_auto_services");
    Ok(envelope("stopped_auto_services", host::run_command("Get-Service | Where-Object {$_.StartType -eq 'Automatic' -and $_.Status -ne 'Running'} | Select-Object Name,DisplayName,Status,StartType | ConvertTo-Json -Compress")))
}

fn network_info() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] network_info");
    Ok(envelope("network_info", host::run_command("Get-NetIPConfiguration | Select-Object InterfaceAlias,@{N='IPv4';E={$_.IPv4Address.IPAddress -join ','}},@{N='Gateway';E={$_.IPv4DefaultGateway.NextHop}},@{N='DNS';E={$_.DNSServer.ServerAddresses -join ','}} | ConvertTo-Json -Compress")))
}

fn startup_programs() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] startup_programs");
    Ok(envelope("startup_programs", host::run_command("Get-CimInstance Win32_StartupCommand | Select-Object Name,Command,Location,User | ConvertTo-Json -Compress")))
}

fn stability_report() -> Result<serde_json::Value, SdkError> {
    host::log("[diagnostics] stability_report");
    let cmd = format!(
        r#"{T}$since=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime;
$errs=Get-WinEvent -FilterHashtable @{{LogName='System','Application';Level=1,2;StartTime=$since}} -MaxEvents 50 -EA SilentlyContinue|Select-Object @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='level';E={{$_.LevelDisplayName}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='id';E={{$_.Id}}}},@{{N='msg';E={{T $_.Message 200}}}};
$whea=@();try{{$whea=Get-WinEvent -FilterHashtable @{{LogName='System';ProviderName='Microsoft-Windows-WHEA-Logger';StartTime=$since}} -MaxEvents 20 -EA Stop|Select-Object @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='level';E={{$_.LevelDisplayName}}}},@{{N='msg';E={{T $_.Message 200}}}}}}catch{{}};
$bsod=Get-WinEvent -FilterHashtable @{{LogName='System';ProviderName='Microsoft-Windows-WER-SystemErrorReporting';StartTime=$since}} -MaxEvents 10 -EA SilentlyContinue|Select-Object @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='bugcheck';E={{[string]$_.Properties[0].Value}}}};
$kp41=@(Get-WinEvent -FilterHashtable @{{LogName='System';ProviderName='Microsoft-Windows-Kernel-Power';Id=41;StartTime=$since}} -MaxEvents 10 -EA SilentlyContinue).Count;
[PSCustomObject]@{{since=$since.ToString('o');errors=@($errs);whea=@($whea);bsods=@($bsod);kernel_power_41_count=$kp41}}|ConvertTo-Json -Compress -Depth 4"#
    );
    Ok(envelope("stability_report", host::run_command(&cmd)))
}

fn event_query(a: EventQueryArgs) -> Result<serde_json::Value, SdkError> {
    let log = match a.log.as_deref().unwrap_or("System") {
        l @ ("System" | "Application" | "Security" | "Setup") => l.to_string(),
        _ => return Err(SdkError::invalid_args("log must be System, Application, Security, or Setup")),
    };
    let mut parts = vec![format!("LogName='{log}'")];
    if let Some(levels) = a.levels.as_ref().filter(|v| !v.is_empty()) {
        let nums: Vec<String> = levels.iter().filter(|n| **n <= 5).map(|n| n.to_string()).collect();
        if !nums.is_empty() {
            parts.push(format!("Level={}", nums.join(",")));
        }
    }
    if let Some(ids) = a.ids.as_ref().filter(|v| !v.is_empty()) {
        let nums: Vec<String> = ids.iter().map(|n| n.to_string()).collect();
        parts.push(format!("Id={}", nums.join(",")));
    }
    if let Some(providers) = a.providers.as_ref().filter(|v| !v.is_empty()) {
        let mut quoted = Vec::new();
        for p in providers {
            match sanitize_token(p) {
                Some(s) => quoted.push(format!("'{s}'")),
                None => return Err(SdkError::invalid_args("a provider name has unsupported characters")),
            }
        }
        parts.push(format!("ProviderName={}", quoted.join(",")));
    }
    if let Some(since) = a.since.as_deref() {
        match sanitize_ts(since) {
            Some(s) => parts.push(format!("StartTime=[datetime]'{s}'")),
            None => return Err(SdkError::invalid_args("since must be an ISO-8601 timestamp")),
        }
    }
    let max = a.max.unwrap_or(40).clamp(1, 200);
    let hashtable = parts.join(";");
    host::log(&format!("[diagnostics] event_query @{{{hashtable}}}"));
    let cmd = format!("{T}try{{$e=Get-WinEvent -FilterHashtable @{{{hashtable}}} -MaxEvents {max} -EA Stop|Select-Object @{{N='time';E={{$_.TimeCreated.ToString('o')}}}},@{{N='id';E={{$_.Id}}}},@{{N='level';E={{$_.LevelDisplayName}}}},@{{N='provider';E={{$_.ProviderName}}}},@{{N='msg';E={{T $_.Message 300}}}};if($e){{$e|ConvertTo-Json -Compress}}else{{'[]'}}}}catch{{ if($_.Exception.Message -match 'No events'){{'[]'}}else{{[PSCustomObject]@{{query_error=$_.Exception.Message}}|ConvertTo-Json -Compress}} }}");
    Ok(envelope("event_query", host::run_command(&cmd)))
}

fn app_crash_summary(a: AppCrashArgs) -> Result<serde_json::Value, SdkError> {
    let days = a.days.unwrap_or(14).clamp(1, 90);
    host::log(&format!("[diagnostics] app_crash_summary days={days}"));
    let cmd = format!(
        r#"$since=(Get-Date).AddDays(-{days});
$e=Get-WinEvent -FilterHashtable @{{LogName='Application';ProviderName='Application Error','Windows Error Reporting','Application Hang';StartTime=$since}} -EA SilentlyContinue;
$rows=$e|ForEach-Object{{ $p=$_.Properties; [ordered]@{{app=if($p.Count -gt 0){{[string]$p[0].Value}}else{{'unknown'}};module=if($p.Count -gt 3){{[string]$p[3].Value}}else{{''}}}} }};
$grp=$rows|Group-Object app|Sort-Object Count -Descending|Select-Object -First 20 @{{N='app';E={{$_.Name}}}},@{{N='count';E={{$_.Count}}}},@{{N='modules';E={{(($_.Group.module|Where-Object{{$_}}|Select-Object -Unique) -join ', ')}}}};
[PSCustomObject]@{{since=$since.ToString('o');total=@($rows).Count;apps=@($grp)}}|ConvertTo-Json -Compress -Depth 4"#
    );
    Ok(envelope("app_crash_summary", host::run_command(&cmd)))
}

mtech_plugin! {
    id: "com.mastertech.diagnostics",
    name: "Diagnostics",
    version: "0.2.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// OS, build, uptime, RAM, CPU model and core/thread count.
        system_summary() => system_summary,
        /// Top 15 processes by RAM with CPU seconds.
        top_processes() => top_processes,
        /// File system drives with total, used, free GB and percent free.
        disk_info() => disk_info,
        /// Last 15 Critical/Error events from the System log.
        recent_system_errors() => recent_system_errors,
        /// Last 15 Critical/Error events from the Application log.
        recent_app_crashes() => recent_app_crashes,
        /// Services set to Automatic start that are currently stopped.
        stopped_auto_services() => stopped_auto_services,
        /// Network adapters with IPv4, gateway and DNS.
        network_info() => network_info,
        /// Programs and scripts configured to run at startup.
        startup_programs() => startup_programs,
        /// Since last boot: system/app errors, WHEA events, BSOD bugchecks, and Kernel-Power 41 count.
        stability_report() => stability_report,
        /// Generic Windows event query by log, level, id, provider and time. Replaces the hand-written event readers.
        event_query(EventQueryArgs) => event_query,
        /// Application crashes/hangs over N days grouped by program and faulting module.
        app_crash_summary(AppCrashArgs) => app_crash_summary,
    }
}
