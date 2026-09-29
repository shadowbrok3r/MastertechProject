//! Network diagnostics & Wi-Fi plugin (SDK port).
//!
//! Consolidates the Wi-Fi tools that were scattered across diagnostics and
//! wifi-quickfix. Read-only Wi-Fi state and WLAN event surveys, a reachability
//! probe, and a SAFE gated repair. Unlike the old wifi_fix, the repair is
//! wireless-only and never cycles the adapter or resets the IP stack (which
//! severed Wi-Fi-managed sessions and wiped static IP configuration).

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

const T: &str =
    "function T($m,$n){if($null -eq $m){return ''};$s=[string]$m;if($s.Length -gt $n){$s.Substring(0,$n)}else{$s}};";

#[derive(Facet, Deserialize)]
struct ConfirmArgs {
    /// Must be true to run the repair.
    confirm: Option<bool>,
}

fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

fn wifi_status() -> Result<serde_json::Value, SdkError> {
    host::log("[network] wifi_status");
    let adapters = host::run_command("Get-NetAdapter | Select-Object Name,InterfaceDescription,Status,MacAddress,LinkSpeed,DriverVersion,DriverProvider,PhysicalMediaType | ConvertTo-Json -Compress");
    let svc = host::run_command("Get-Service WlanSvc | Select-Object Name,Status,StartType | ConvertTo-Json -Compress");
    let iface = host::run_command("netsh wlan show interfaces");
    let drivers = host::run_command("netsh wlan show drivers");
    let rf = host::run_command("try { $v = (Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\RadioManagement\\SystemRadioState' -EA Stop).SystemRadioState; \"RadioState=$v\" } catch { 'RadioState=unknown' }");
    let devmgr = host::run_command("Get-PnpDevice -Class Net | Where-Object { $_.FriendlyName -match 'Wi.?Fi|Wireless|802\\.11|AX210|AX200|MT7921|MT7922|RTL88|Qualcomm' } | Select-Object FriendlyName,Status,Problem,InstanceId | ConvertTo-Json -Compress");
    let parse = |s: String| serde_json::from_str::<serde_json::Value>(s.trim()).unwrap_or(serde_json::Value::String(s));
    Ok(serde_json::json!({
        "tool": "wifi_status",
        "adapters": parse(adapters),
        "wlan_service": parse(svc),
        "netsh_interfaces": iface,
        "netsh_drivers": drivers,
        "radio_state": rf.trim(),
        "pnp_wifi_devices": parse(devmgr),
    }))
}

fn wifi_event_logs() -> Result<serde_json::Value, SdkError> {
    host::log("[network] wifi_event_logs");
    let wlan = host::run_command(&format!("{T}Get-WinEvent -LogName 'Microsoft-Windows-WLAN-AutoConfig/Operational' -MaxEvents 30 -ErrorAction SilentlyContinue | Select-Object TimeCreated,Id,LevelDisplayName,@{{N='Msg';E={{T ($_.Message -replace '\\s+',' ') 250}}}} | ConvertTo-Json -Compress"));
    let sys = host::run_command(&format!("{T}Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='Microsoft-Windows-WLAN-AutoConfig','bowser','srv','mrxsmb'}} -MaxEvents 15 -ErrorAction SilentlyContinue | Select-Object TimeCreated,Id,LevelDisplayName,ProviderName,@{{N='Msg';E={{T ($_.Message -replace '\\s+',' ') 200}}}} | ConvertTo-Json -Compress"));
    let ndis = host::run_command(&format!("{T}Get-WinEvent -FilterHashtable @{{LogName='System'; ProviderName='ndis'}} -MaxEvents 10 -ErrorAction SilentlyContinue | Select-Object TimeCreated,Id,LevelDisplayName,@{{N='Msg';E={{T ($_.Message -replace '\\s+',' ') 200}}}} | ConvertTo-Json -Compress"));
    let parse = |s: String| serde_json::from_str::<serde_json::Value>(s.trim()).unwrap_or(serde_json::Value::String(s));
    Ok(serde_json::json!({
        "tool": "wifi_event_logs",
        "wlan_autoconfig": parse(wlan),
        "system_wlan": parse(sys),
        "ndis": parse(ndis),
    }))
}

fn network_diag() -> Result<serde_json::Value, SdkError> {
    host::log("[network] network_diag");
    let cmd = r#"$gw=(Get-NetIPConfiguration -EA SilentlyContinue|Where-Object{$_.IPv4DefaultGateway}|Select-Object -First 1).IPv4DefaultGateway.NextHop;
$gwPing=if($gw){Test-Connection $gw -Count 2 -Quiet -EA SilentlyContinue}else{$null};
$dns=try{(Resolve-DnsName microsoft.com -EA Stop|Select-Object -First 1).IPAddress}catch{$null};
$https=try{(Invoke-WebRequest -Uri 'https://www.microsoft.com' -UseBasicParsing -TimeoutSec 8 -Method Head).StatusCode}catch{$null};
[PSCustomObject]@{gateway=$gw;gateway_reachable=$gwPing;dns_resolves=($null -ne $dns);sample_dns_result=$dns;https_status=$https}|ConvertTo-Json -Compress"#;
    Ok(envelope("network_diag", host::run_command(cmd)))
}

fn wlan_repair(a: ConfirmArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm != Some(true) {
        return Err(SdkError::invalid_args(
            "wlan_repair enables a disabled wireless adapter, sets WlanSvc to Automatic and starts it, and flushes DNS; pass confirm:true to proceed",
        ));
    }
    host::log("[network] wlan_repair (confirmed)");
    let cmd = r#"$steps=@();
$wifi=Get-NetAdapter -EA SilentlyContinue|Where-Object{$_.PhysicalMediaType -like '*802.11*' -or $_.PhysicalMediaType -like '*Wireless*'};
foreach($a in $wifi){ if($a.Status -eq 'Disabled'){ try{Enable-NetAdapter -Name $a.Name -Confirm:$false -EA Stop; $steps+="enabled $($a.Name)"}catch{$steps+="enable failed $($a.Name): $($_.Exception.Message)"} } };
try{Set-Service WlanSvc -StartupType Automatic -EA SilentlyContinue; if((Get-Service WlanSvc).Status -ne 'Running'){Start-Service WlanSvc -EA Stop; $steps+='WlanSvc started'}else{$steps+='WlanSvc already running'}}catch{$steps+="WlanSvc start failed: $($_.Exception.Message)"};
$steps+=('flushdns: '+(((ipconfig /flushdns) | Out-String).Trim()));
$final=$wifi|Select-Object Name,Status,PhysicalMediaType;
[PSCustomObject]@{steps=@($steps);wireless_adapters=@($final);note='Wireless-only; does not cycle adapters or reset the IP stack (that would sever a Wi-Fi-managed session or wipe static IP).'}|ConvertTo-Json -Compress -Depth 4"#;
    Ok(envelope("wlan_repair", host::run_command(cmd)))
}

mtech_plugin! {
    id: "com.mastertech.network",
    name: "Network & Wi-Fi",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Full Wi-Fi state: adapters, WLAN service, netsh interfaces/drivers, radio state, and PnP device problem codes. Read-only.
        wifi_status() => wifi_status,
        /// WLAN-AutoConfig operational events plus System-log WLAN and NDIS entries. Read-only.
        wifi_event_logs() => wifi_event_logs,
        /// Reachability: default gateway ping, DNS resolution, and an HTTPS HEAD probe. Read-only; run before any repair.
        network_diag() => network_diag,
        /// SAFE repair: enable a disabled wireless adapter, set WlanSvc to Automatic and start it, flush DNS. Wireless-only; never cycles the adapter or resets the IP stack. Requires confirm:true.
        wlan_repair(ConfirmArgs) => wlan_repair,
    }
}
