//! Tune-up sweeps that each return one compact JSON summary.

use mtech_plugin_sdk::{host, mtech_plugin, SdkError};

const HEALTH_CHECK_PS: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$flags = New-Object System.Collections.Generic.List[string]
$os = Get-CimInstance Win32_OperatingSystem
$boot = $os.LastBootUpTime
$since30 = (Get-Date).AddDays(-30)
function Count-Events($filter) { try { @(Get-WinEvent -FilterHashtable $filter -ErrorAction Stop).Count } catch { 0 } }
function Power-Index([string]$sub, [string]$setting) {
    $vals = foreach ($line in @(powercfg /qh SCHEME_CURRENT $sub $setting 2>$null)) {
        if ($line -match 'Current (AC|DC) Power Setting Index:\s*0x([0-9a-fA-F]+)') { [Convert]::ToInt64($matches[2], 16) }
    }
    @($vals)
}

$predict = 0
foreach ($p in @(Get-CimInstance -Namespace root\wmi -ClassName MSStorageDriver_FailurePredictStatus)) { if ($p.PredictFailure) { $predict++ } }
if ($predict -gt 0) { $flags.Add("SMART predicts failure on $predict disk(s)") }
$disks = @(foreach ($d in @(Get-PhysicalDisk)) {
    $rc = $d | Get-StorageReliabilityCounter
    $bad = [int64]$rc.ReadErrorsUncorrected + [int64]$rc.WriteErrorsUncorrected
    $row = [ordered]@{
        name = [string]$d.FriendlyName; media = [string]$d.MediaType; bus = [string]$d.BusType
        health = [string]$d.HealthStatus; size_gb = [math]::Round($d.Size / 1GB)
        wear_pct = $rc.Wear; temp_c = $rc.Temperature; power_on_hours = $rc.PowerOnHours; uncorrected_errors = $bad
    }
    if ($row.health -and $row.health -ne 'Healthy') { $flags.Add("disk $($row.name): $($row.health)") }
    if ($rc.Wear -ge 80) { $flags.Add("disk $($row.name): wear $($rc.Wear)%") }
    if ($bad -gt 0) { $flags.Add("disk $($row.name): $bad uncorrected errors") }
    $row
})
$fixed = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object {
    $free = [math]::Round($_.FreeSpace / 1GB, 1); $size = [math]::Round($_.Size / 1GB, 1)
    if ($_.DeviceID -eq $env:SystemDrive -and $size -gt 0 -and ($free -lt 20 -or $free / $size -lt 0.1)) { $flags.Add("system drive low: $free GB free of $size GB") }
    [ordered]@{ drive = $_.DeviceID; free_gb = $free; size_gb = $size }
})

$events = [ordered]@{
    whea_since_boot = Count-Events @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-WHEA-Logger'; StartTime = $boot }
    whea_30d = Count-Events @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-WHEA-Logger'; StartTime = $since30 }
    tdr_30d = Count-Events @{ LogName = 'System'; ProviderName = 'Display'; Id = 4101; StartTime = $since30 }
    cpu_throttle_since_boot = Count-Events @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Kernel-Processor-Power'; Id = 37; StartTime = $boot }
    unexpected_shutdowns_30d = Count-Events @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Kernel-Power'; Id = 41; StartTime = $since30 }
    bugchecks_30d = Count-Events @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-WER-SystemErrorReporting'; Id = 1001; StartTime = $since30 }
    disk_errors_30d = Count-Events @{ LogName = 'System'; ProviderName = 'disk', 'stornvme', 'storahci', 'iaStorA', 'iaStorAC', 'Ntfs'; Id = 7, 11, 51, 55, 129, 153; StartTime = $since30 }
}
if ($events.whea_since_boot -gt 0) { $flags.Add("WHEA errors since boot: $($events.whea_since_boot)") }
elseif ($events.whea_30d -gt 0) { $flags.Add("WHEA errors in 30 days: $($events.whea_30d)") }
if ($events.tdr_30d -gt 0) { $flags.Add("GPU driver resets (TDR 4101) in 30 days: $($events.tdr_30d)") }
if ($events.cpu_throttle_since_boot -gt 0) { $flags.Add("firmware CPU throttling (Kernel-Processor-Power 37) since boot: $($events.cpu_throttle_since_boot)") }
if ($events.unexpected_shutdowns_30d -gt 0) { $flags.Add("unexpected shutdowns (Kernel-Power 41) in 30 days: $($events.unexpected_shutdowns_30d)") }
if ($events.bugchecks_30d -gt 0) { $flags.Add("bugchecks in 30 days: $($events.bugchecks_30d)") }
if ($events.disk_errors_30d -gt 0) { $flags.Add("disk/storage errors in 30 days: $($events.disk_errors_30d)") }

$pnp = @(Get-CimInstance Win32_PnPEntity -Filter 'ConfigManagerErrorCode <> 0' | Select-Object -First 15 | ForEach-Object {
    [ordered]@{ name = [string]$_.Name; code = [int]$_.ConfigManagerErrorCode; class = [string]$_.PNPClass }
})
$pnpReal = @($pnp | Where-Object { $_.code -ne 22 })
if ($pnpReal.Count -gt 0) { $flags.Add("PnP problem devices: " + (($pnpReal | ForEach-Object { "$($_.name) (code $($_.code))" }) -join '; ')) }

$cs = Get-CimInstance Win32_ComputerSystem
$ram = [ordered]@{
    total_gb = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
    modules = @(Get-CimInstance Win32_PhysicalMemory | Where-Object { $_.Capacity -ge 1GB } | ForEach-Object {
        [ordered]@{ slot = [string]$_.DeviceLocator; gb = [math]::Round($_.Capacity / 1GB); rated_mhz = $_.Speed; running_mhz = $_.ConfiguredClockSpeed; maker = ([string]$_.Manufacturer).Trim(); part = ([string]$_.PartNumber).Trim() }
    })
}

$battery = $null
$design = @(Get-CimInstance -Namespace root\wmi -ClassName BatteryStaticData)[0].DesignedCapacity
$full = @(Get-CimInstance -Namespace root\wmi -ClassName BatteryFullChargedCapacity)[0].FullChargedCapacity
if ($design -gt 0 -and $full -gt 0) {
    $pct = [math]::Round(100.0 * $full / $design, 1)
    $battery = [ordered]@{ design_mwh = $design; full_mwh = $full; health_pct = $pct; cycles = @(Get-CimInstance -Namespace root\wmi -ClassName BatteryCycleCount)[0].CycleCount; on_ac = [bool]@(Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus)[0].PowerOnline }
    if ($pct -lt 60) { $flags.Add("battery health $pct% of design") }
}

$sleep = Power-Index SUB_SLEEP STANDBYIDLE
$lid = Power-Index SUB_BUTTONS LIDACTION
$lidNames = @('do nothing', 'sleep', 'hibernate', 'shut down')
$power = [ordered]@{
    plan = ([string](powercfg /getactivescheme)) -replace '^.*\((.*)\)\s*$', '$1'
    sleep_after_ac_min = if ($sleep.Count -gt 0) { [math]::Round($sleep[0] / 60) } else { $null }
    sleep_after_dc_min = if ($sleep.Count -gt 1) { [math]::Round($sleep[1] / 60) } else { $null }
    lid_ac = if ($lid.Count -gt 0) { $lidNames[[int]$lid[0]] } else { $null }
    lid_dc = if ($lid.Count -gt 1) { $lidNames[[int]$lid[1]] } else { $null }
    hibernate_file = [bool](Test-Path "$env:SystemDrive\hiberfil.sys")
    fast_startup = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power').HiberbootEnabled -eq 1
}
if ($battery -and $power.lid_ac -and $power.lid_ac -ne 'do nothing') { $flags.Add("closing the lid on AC will $($power.lid_ac) the machine") }

[ordered]@{
    boot = $boot.ToString('s'); uptime_h = [math]::Round(((Get-Date) - $boot).TotalHours, 1)
    flags = @($flags); disks = $disks; volumes = $fixed; events = $events; pnp_problems = $pnp; ram = $ram; battery = $battery; power = $power
} | ConvertTo-Json -Compress -Depth 4
"##;

const PUP_SWEEP_PS: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$pupRx = 'wave ?browser|swbrowser|onelaunch|onestart|shift ?browser|webnavigator|web ?navigator|pc ?app ?store|search ?baron|bytefence|segurazo|santivirus|restoro|reimage|fortect|driver ?booster|driver ?easy|driver ?support|driverpack|advanced ?systemcare|pc ?accelerate|mypc ?utilities|winzip ?(driver|system)|pc ?helpsoft|systweak|outbyte|auslogics|pc ?cleaner|reg ?clean|cleanmymac|clean ?master|sweet ?browser|tutorial ?toolbar|ask ?toolbar|conduit|mindspark|myway|hola ?vpn|astromenda|delta ?search|qone8|opencandy|installcore|adaware ?web|webadvisor|safe ?search|glance|dcprotect|csdi|easy ?pdf|pdf ?(converter|maker|suite|pro ?free)|maps ?(now|frenzy)|weather ?(now|bar)|free ?forms|translator ?now|coupon|shopping ?assistant|torch ?browser|kinza|lightning ?browser|norton ?secure ?browser|avast ?secure ?browser|avg ?secure ?browser|ccleaner ?browser|opera ?(installer|setup)|1click|smart ?pc ?(fixer|care)|total ?pc ?cleaner|speedup|tweakbit|reviversoft'
$remoteRx = 'ultraviewer|anydesk|teamviewer|rustdesk|screenconnect|connectwise|splashtop|supremo|remotepc|aeroadmin|ammyy|logmein|gotoassist|zoho ?assist|islonline|showmypc|remote ?utilities|dwagent|dwservice|getscreen|quickassist'
$oddPathRx = '\\appdata\\local\\temp\\|\\downloads\\|\\appdata\\roaming\\[^\\]+\.exe|\\users\\public\\'
$report = New-Object System.Collections.Generic.List[string]
$hits = [ordered]@{}
$odd = New-Object System.Collections.Generic.List[object]
function Clip([string]$s, [int]$n = 140) { if ($null -eq $s) { return '' }; if ($s.Length -gt $n) { $s.Substring(0, $n) } else { $s } }
function Check([string]$where, [string]$item, [string]$detail) {
    $text = "$item $detail"
    foreach ($kind in @('remote_access', 'pup')) {
        $rx = if ($kind -eq 'pup') { $pupRx } else { $remoteRx }
        $m = [regex]::Match($text, $rx, 'IgnoreCase')
        if (-not $m.Success) { continue }
        $name = $m.Value.ToLower() -replace ' ', ''
        $key = "$kind|$name"
        if (-not $hits.Contains($key)) {
            $hits[$key] = [ordered]@{ kind = $kind; name = $name; seen_in = New-Object System.Collections.Generic.List[string]; example = Clip $(if ($detail) { $detail } else { $item }) }
        }
        if (-not $hits[$key].seen_in.Contains($where)) { $hits[$key].seen_in.Add($where) }
        return
    }
}

$profiles = @(Get-ChildItem "$env:SystemDrive\Users" -Directory -Force | Where-Object { $_.Name -notin @('Public', 'Default', 'Default User', 'All Users') -and (Test-Path (Join-Path $_.FullName 'AppData')) })
$installers = New-Object System.Collections.Generic.List[object]
foreach ($p in $profiles) {
    $dirs = @("$($p.FullName)\Downloads", "$($p.FullName)\Desktop") + @(Get-ChildItem $p.FullName -Directory -Force -Filter 'OneDrive*' | ForEach-Object { "$($_.FullName)\Desktop"; "$($_.FullName)\Downloads" })
    foreach ($d in $dirs) {
        foreach ($f in @(Get-ChildItem $d -File -Force -Include *.exe, *.msi, *.msix, *.appx -Recurse -Depth 1)) {
            $installers.Add($f); $report.Add("installer`t$($f.FullName)`t$([math]::Round($f.Length / 1MB, 1)) MB`t$($f.LastWriteTime.ToString('yyyy-MM-dd'))")
            Check 'installer' $f.Name $f.FullName
        }
    }
    foreach ($root in @("$($p.FullName)\AppData\Local", "$($p.FullName)\AppData\Roaming", "$($p.FullName)\AppData\Local\Programs")) {
        foreach ($dir in @(Get-ChildItem $root -Directory -Force)) { $report.Add("appdata`t$($dir.FullName)"); Check 'appdata' $dir.Name $dir.FullName }
    }
}

$uninstallKeys = @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')
$runKeys = @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce')
foreach ($sid in @(Get-ChildItem Registry::HKEY_USERS | Where-Object { $_.PSChildName -match '^S-1-5-21-[\d-]+$' })) {
    $uninstallKeys += "Registry::HKEY_USERS\$($sid.PSChildName)\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
    $runKeys += "Registry::HKEY_USERS\$($sid.PSChildName)\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
    $runKeys += "Registry::HKEY_USERS\$($sid.PSChildName)\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce"
}
foreach ($app in @(Get-ItemProperty $uninstallKeys | Where-Object { $_.DisplayName })) {
    $report.Add("program`t$($app.DisplayName)`t$($app.Publisher)`t$($app.InstallDate)")
    Check 'program' ([string]$app.DisplayName) ([string]$app.Publisher)
}
$startup = New-Object System.Collections.Generic.List[string]
foreach ($k in $runKeys) {
    $props = Get-ItemProperty $k
    if (-not $props) { continue }
    foreach ($v in $props.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' }) {
        $cmd = [string]$v.Value
        $startup.Add("$($v.Name)")
        $report.Add("run`t$k`t$($v.Name)`t$cmd")
        Check 'startup' $v.Name $cmd
        if ($cmd -match $oddPathRx) { $odd.Add([ordered]@{ where = 'startup'; item = Clip $v.Name; detail = Clip $cmd }) }
    }
}

$tasks = @(Get-ScheduledTask | Where-Object { $_.TaskPath -notlike '\Microsoft\*' })
foreach ($t in $tasks) {
    $exec = (@($t.Actions) | ForEach-Object { "$($_.Execute) $($_.Arguments)" }) -join ' | '
    $report.Add("task`t$($t.TaskPath)$($t.TaskName)`t$($t.State)`t$exec")
    Check 'task' $t.TaskName $exec
    if ($exec -match $oddPathRx) { $odd.Add([ordered]@{ where = 'task'; item = Clip $t.TaskName; detail = Clip $exec }) }
}

$extensions = [ordered]@{}
function Ext-Name($manifestPath) {
    $m = [string](Get-Content $manifestPath -Raw) | ConvertFrom-Json
    $name = [string]$m.name
    if ($name -like '__MSG_*') {
        $key = $name.Substring(6).TrimEnd('_')
        $dir = Split-Path $manifestPath
        foreach ($loc in @('en', 'en_US', [string]$m.default_locale)) {
            $msgs = Join-Path $dir "_locales\$loc\messages.json"
            if ($loc -and (Test-Path $msgs)) {
                $j = [string](Get-Content $msgs -Raw) | ConvertFrom-Json
                $hit = $j.PSObject.Properties | Where-Object { $_.Name -ieq $key } | Select-Object -First 1
                if ($hit) { return [string]$hit.Value.message }
            }
        }
    }
    $name
}
foreach ($p in $profiles) {
    $browsers = @{ chrome = "$($p.FullName)\AppData\Local\Google\Chrome\User Data"; edge = "$($p.FullName)\AppData\Local\Microsoft\Edge\User Data"; brave = "$($p.FullName)\AppData\Local\BraveSoftware\Brave-Browser\User Data" }
    foreach ($b in $browsers.Keys) {
        foreach ($m in @(Get-ChildItem "$($browsers[$b])\*\Extensions\*\*\manifest.json" -Force)) {
            $n = Ext-Name $m.FullName
            if (-not $n -or $n -match '^(Chrome|Edge) (Web Store )?Payments$|^Google (Docs Offline|Drive|Slides|Sheets|Docs)$') { continue }
            if (-not $extensions.Contains($b)) { $extensions[$b] = New-Object System.Collections.Generic.List[string] }
            if (-not $extensions[$b].Contains($n)) { $extensions[$b].Add($n); $report.Add("extension`t$b`t$n`t$($m.FullName)"); Check "extension_$b" $n '' }
        }
    }
    foreach ($ff in @(Get-ChildItem "$($p.FullName)\AppData\Roaming\Mozilla\Firefox\Profiles\*\extensions.json" -Force)) {
        $j = [string](Get-Content $ff.FullName -Raw) | ConvertFrom-Json
        foreach ($a in @($j.addons | Where-Object { $_.type -eq 'extension' -and $_.location -eq 'app-profile' })) {
            $n = [string]$a.defaultLocale.name
            if (-not $extensions.Contains('firefox')) { $extensions['firefox'] = New-Object System.Collections.Generic.List[string] }
            if ($n -and -not $extensions['firefox'].Contains($n)) { $extensions['firefox'].Add($n); $report.Add("extension`tfirefox`t$n"); Check 'extension_firefox' $n '' }
        }
    }
}

$procs = @(Get-Process | Where-Object { $_.Path -and $_.Path -match $oddPathRx } | Select-Object -First 20 | ForEach-Object { "$($_.ProcessName): $(Clip $_.Path 120)" })
foreach ($pr in @(Get-Process | Where-Object { $_.Path })) { Check 'process' $pr.ProcessName $pr.Path }

$reportPath = "$env:ProgramData\MTech\pupsweep.txt"
New-Item -ItemType Directory -Path (Split-Path $reportPath) -Force | Out-Null
$report | Set-Content -Path $reportPath -Encoding UTF8

$extOut = [ordered]@{}
foreach ($b in $extensions.Keys) { $extOut[$b] = @($extensions[$b] | Select-Object -First 25) }
function Hits([string]$kind) { @($hits.Values | Where-Object { $_.kind -eq $kind } | Select-Object -First 30 | ForEach-Object { [ordered]@{ name = $_.name; seen_in = @($_.seen_in); example = $_.example } }) }

[ordered]@{
    profiles = @($profiles | ForEach-Object { $_.Name })
    pup_candidates = @(Hits 'pup')
    remote_access = @(Hits 'remote_access')
    odd_path_autostarts = @($odd | Select-Object -First 15)
    odd_path_processes = $procs
    extensions = $extOut
    startup_entries = @($startup | Select-Object -Unique)
    non_microsoft_tasks = $tasks.Count
    installers = [ordered]@{ total = $installers.Count; newest = @($installers | Sort-Object LastWriteTime -Descending | Select-Object -First 8 | ForEach-Object { "$($_.Name) ($($_.LastWriteTime.ToString('yyyy-MM-dd')))" }) }
    report = $reportPath
} | ConvertTo-Json -Compress -Depth 4
"##;

/// Parses PS JSON output into the tool envelope, stderr-safe.
fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

fn health_check() -> Result<serde_json::Value, SdkError> {
    host::log("[tuneup] health_check");
    Ok(envelope("health_check", host::run_command(HEALTH_CHECK_PS)))
}

fn pup_sweep() -> Result<serde_json::Value, SdkError> {
    host::log("[tuneup] pup_sweep");
    Ok(envelope("pup_sweep", host::run_command(PUP_SWEEP_PS)))
}

mtech_plugin! {
    id: "com.mastertech.tuneup",
    name: "Tune-up Sweeps",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Read-only hardware health in one pass: disks (health, wear, temperature, uncorrected errors, SMART prediction), volume space, WHEA / TDR / Kernel-Processor-Power 37 / Kernel-Power 41 / bugcheck / disk-error counts, PnP problem devices, RAM modules, battery wear, power plan and lid action. `flags` lists only what needs attention.
        health_check() => health_check,
        /// PUP and remote-access sweep over every profile: installers in Downloads/Desktop (OneDrive too), AppData program folders, installed programs, Run keys, non-Microsoft scheduled tasks, Chrome/Edge/Brave/Firefox extensions and running processes. Returns grouped candidates and writes the full list to C:\ProgramData\MTech\pupsweep.txt; changes nothing else.
        pup_sweep() => pup_sweep,
    }
}
