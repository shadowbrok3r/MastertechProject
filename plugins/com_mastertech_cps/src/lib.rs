//! CPS security-software plugin (Webroot + SUPERAntiSpyware).
//!
//! Read-only license/state checks, a corrected Windows Security Center product
//! survey, a deep Webroot remnant survey, and a confirm-gated full Webroot
//! uninstall. Split out of hw-diag so hardware diagnostics and the shop's
//! security bundle are separate concerns. WSC productState is decoded on the
//! middle byte (real-time state), not the low byte (signature state).

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

#[derive(Facet, Deserialize)]
struct CleanupArgs {
    /// Must be true. Acknowledges this is a FULL Webroot uninstall, not remnant pruning.
    confirm_full_uninstall: Option<bool>,
}

/// Parses the PS JSON output into the tool envelope, stderr-safe.
fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

const WEBROOT_LICENSE_PS: &str = r#"$o=[ordered]@{product='webroot';installed=$false;active=$null;days_remaining=$null;source=$null;executable=$null;wmi=$null;registry_hits=@();note=$null};
$wrExe=@('C:\Program Files\Webroot\WRSA.exe','C:\Program Files (x86)\Webroot\WRSA.exe')|Where-Object{Test-Path $_}|Select-Object -First 1;
if($wrExe){$o.installed=$true;$o.executable=$wrExe};
function Add-Hit($path,$name,$val){
  if($val -is [byte[]]){return};
  $s=[string]$val;
  if($s.Length -gt 120){$s=$s.Substring(0,120)+'…'};
  $script:o.registry_hits+=[ordered]@{path=$path;name=$name;value=$s}
};
function Try-DaysFromNameValue($path,$name,$val){
  if($val -is [byte[]]){return};
  if($name -match '(?i)day'){
    if($val -is [int] -or $val -is [long] -or $val -is [uint32]){
      $iv=[int]$val;
      if($iv -gt 0 -and $iv -lt 8000){$script:o.days_remaining=$iv;$script:o.source=('registry:'+$path+'\'+$name)}
    }
  }
  if($name -match '(?i)expir|enddate|licenseend|subscri|renew|trialend'){
    try{
      $dt=[datetime]::Parse([string]$val);
      $days=[int]([timespan]($dt-(Get-Date))).TotalDays;
      if($days -ge -120 -and $days -lt 8000){$script:o.days_remaining=$days;$script:o.source=('registry:'+$path+'\'+$name)}
    }catch{}
  }
};
foreach($r in @('HKLM:\SOFTWARE\WOW6432Node\WRData','HKLM:\SOFTWARE\WRData')){
  if(-not (Test-Path $r)){continue};
  $o.installed=$true;
  try{
    $p=Get-ItemProperty $r -ErrorAction Stop;
    foreach($prop in $p.PSObject.Properties){
      $n=$prop.Name;
      if($n -match '^PS'){continue};
      if($n -match '(?i)day|expir|license|key|subscri|trial|renew|valid|end|hpl|gsm'){Add-Hit $r $n $prop.Value};
      Try-DaysFromNameValue $r $n $prop.Value
    }
    $nSub=0;
    Get-ChildItem $r -Recurse -Depth 2 -ErrorAction SilentlyContinue|ForEach-Object{
      if($nSub++ -gt 120){return};
      try{
        $g=Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue;
        foreach($prop in $g.PSObject.Properties){
          $n=$prop.Name;
          if($n -match '^PS'){continue};
          if($n -match '(?i)day|expir|license|key|subscri|trial|renew|valid|end'){
            Add-Hit $_.PSPath $n $prop.Value;
            Try-DaysFromNameValue $_.PSPath $n $prop.Value
          }
        }
      }catch{}
    }
  }catch{}
};
try{
  $av=Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction Stop|Where-Object{$_.displayName -like '*Webroot*'}|Select-Object -First 1;
  if($av){
    $o.installed=$true;
    $st=[int]$av.productState;
    $prod=($st -shr 8) -band 0xFF;
    $sig=$st -band 0xFF;
    $o.wmi=[ordered]@{displayName=$av.displayName;instanceGuid=$av.instanceGuid;product_state_hex=('0x{0:x6}' -f $st);product_state=('0x{0:x2}' -f $prod);signature_state=('0x{0:x2}' -f $sig)};
    $o.active=(($prod -band 0x10) -ne 0);
    $o.definitions_up_to_date=($sig -eq 0);
    if(-not $o.active -and -not $o.note){$o.note='WSC: real-time protection off, expired, or snoozed.'}
  }
}catch{};
if(-not $o.installed){$o.note='Webroot not detected (no WRSA.exe / WRData / WSC listing).'}
elseif($null -eq $o.days_remaining -and (-not $o.note)){$o.note='Days remaining not parsed from registry; confirm in WRSA (gear > My Account / subscription).'};
$o|ConvertTo-Json -Compress -Depth 6"#;

const SAS_LICENSE_PS: &str = r#"$o=[ordered]@{product='superantispyware';installed=$false;active=$null;days_remaining=$null;source=$null;executable=$null;wmi=$null;registry_hits=@();note=$null};
$sasExe='C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe';
if(Test-Path $sasExe){$o.installed=$true;$o.executable=$sasExe};
function Add-Hit($path,$name,$val){
  if($val -is [byte[]]){return};
  $s=[string]$val;
  if($s.Length -gt 120){$s=$s.Substring(0,120)+'…'};
  $script:o.registry_hits+=[ordered]@{path=$path;name=$name;value=$s}
};
function Try-DaysFromNameValue($path,$name,$val){
  if($val -is [byte[]]){return};
  if($name -match '(?i)day|trial|valid'){
    if($val -is [int] -or $val -is [long] -or $val -is [uint32]){
      $iv=[int]$val;
      if($iv -gt 0 -and $iv -lt 8000){$script:o.days_remaining=$iv;$script:o.source=('registry:'+$path+'\'+$name)}
    }
  }
  if($name -match '(?i)expir|enddate|license|registr|renew|subscri'){
    try{
      $dt=[datetime]::Parse([string]$val);
      $days=[int]([timespan]($dt-(Get-Date))).TotalDays;
      if($days -ge -120 -and $days -lt 8000){$script:o.days_remaining=$days;$script:o.source=('registry:'+$path+'\'+$name)}
    }catch{}
  }
};
foreach($r in @('HKLM:\SOFTWARE\SUPERAntiSpyware','HKLM:\SOFTWARE\WOW6432Node\SUPERAntiSpyware')){
  if(-not (Test-Path $r)){continue};
  $o.installed=$true;
  try{
    $p=Get-ItemProperty $r -ErrorAction Stop;
    foreach($prop in $p.PSObject.Properties){
      $n=$prop.Name;
      if($n -match '^PS'){continue};
      if($n -match '(?i)day|expir|license|registr|trial|renew|valid|end|key|subscri'){Add-Hit $r $n $prop.Value};
      Try-DaysFromNameValue $r $n $prop.Value
    }
    $nSub=0;
    Get-ChildItem $r -Recurse -Depth 3 -ErrorAction SilentlyContinue|ForEach-Object{
      if($nSub++ -gt 120){return};
      try{
        $g=Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue;
        if(-not $g){return};
        foreach($prop in $g.PSObject.Properties){
          $n=$prop.Name;
          if($n -match '^PS'){continue};
          if($n -match '(?i)day|expir|license|registr|trial|renew|valid|end'){
            Add-Hit $_.PSPath $n $prop.Value;
            Try-DaysFromNameValue $_.PSPath $n $prop.Value
          }
        }
      }catch{}
    }
  }catch{}
};
try{
  $av=Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction Stop|
    Where-Object{$_.displayName -like '*SUPERAntiSpyware*' -or $_.displayName -like '*SuperAntiSpyware*'}|
    Select-Object -First 1;
  if($av){
    $o.installed=$true;
    $st=[int]$av.productState;
    $prod=($st -shr 8) -band 0xFF;
    $sig=$st -band 0xFF;
    $o.wmi=[ordered]@{displayName=$av.displayName;instanceGuid=$av.instanceGuid;product_state_hex=('0x{0:x6}' -f $st);product_state=('0x{0:x2}' -f $prod);signature_state=('0x{0:x2}' -f $sig)};
    $o.active=(($prod -band 0x10) -ne 0);
    $o.definitions_up_to_date=($sig -eq 0);
    if(-not $o.active -and -not $o.note){$o.note='WSC: real-time protection off, expired, or snoozed.'}
  }
}catch{};
if(-not $o.installed){$o.note='SUPERAntiSpyware not detected.'}
elseif($null -eq $o.days_remaining -and (-not $o.note)){$o.note='Days remaining not parsed from registry; check Help > About / registration in SAS.'};
$o|ConvertTo-Json -Compress -Depth 6"#;

const WSC_PRODUCTS_PS: &str = r#"$rows=@();
foreach($cls in @('AntiVirusProduct','AntiSpywareProduct','FirewallProduct')){
  try{
    Get-CimInstance -Namespace root/SecurityCenter2 -ClassName $cls -EA Stop | ForEach-Object {
      $st=[int]$_.productState;
      $prod=($st -shr 8) -band 0xFF;
      $sig=$st -band 0xFF;
      $exe=[string]$_.pathToSignedProductExe;
      $exeReal=if($exe){$exe -replace '^windowsdefender://',''}else{''};
      $missing=$false;
      if($exeReal -and ($exeReal -notmatch '://') -and -not (Test-Path $exeReal)){$missing=$true};
      $rows+=[ordered]@{
        class=$cls;
        displayName=$_.displayName;
        instanceGuid=$_.instanceGuid;
        product_state_hex=('0x{0:x6}' -f $st);
        real_time_on=(($prod -band 0x10) -ne 0);
        definitions_up_to_date=($sig -eq 0);
        exe=$exe;
        exe_missing=$missing
      }
    }
  }catch{}
};
[PSCustomObject]@{ products=@($rows); count=@($rows).Count } | ConvertTo-Json -Compress -Depth 5"#;

const WEBROOT_STATUS_DEEP_PS: &str = r#"$o=[ordered]@{services=@();driver=$null;process=$null;install_dirs=@();reg_keys=@();sc_registrations=@();sc_ghost_count=0;scheduled_tasks=@()};
$o.services=Get-Service -EA SilentlyContinue|Where-Object{$_.Name -match '(?i)wr|webroot'}|Select-Object Name,DisplayName,Status,StartType;
$drv=Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\WRkrn' -EA SilentlyContinue;
if($drv){$o.driver=[ordered]@{name='WRkrn';image=$drv.ImagePath;start=$drv.Start}};
$o.process=Get-Process WRSA -EA SilentlyContinue|Select-Object Id,@{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}};
foreach($d in @('C:\Program Files\Webroot','C:\Program Files (x86)\Webroot','C:\ProgramData\WRData','C:\ProgramData\WRCore')){if(Test-Path $d){$o.install_dirs+=$d}};
foreach($k in @('HKLM:\SOFTWARE\WRData','HKLM:\SOFTWARE\WRCore','HKLM:\SOFTWARE\WRMIDData','HKLM:\SOFTWARE\Webroot','HKLM:\SOFTWARE\WOW6432Node\WRData','HKLM:\SOFTWARE\WOW6432Node\WRCore','HKLM:\SOFTWARE\WOW6432Node\Webroot')){if(Test-Path $k){$o.reg_keys+=$k}};
try{$sc=Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -EA Stop|Where-Object{$_.displayName -like '*Webroot*'}|Select-Object displayName,@{N='state';E={'0x{0:x8}' -f [int]$_.productState}},instanceGuid,pathToSignedProductExe;$o.sc_registrations=@($sc);$o.sc_ghost_count=@($sc).Count}catch{};
$o.scheduled_tasks=Get-ScheduledTask -EA SilentlyContinue|Where-Object{$_.TaskName -match '(?i)webroot|wrsa'}|Select-Object TaskName,State;
$o|ConvertTo-Json -Compress -Depth 5"#;

const WEBROOT_CLEANUP_PS: &str = r#"$uns=@('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*');
function UninstallEntries { @(Get-ItemProperty $uns -EA SilentlyContinue | Where-Object {$_.DisplayName -like '*Webroot*'}) };
function StillInstalled { (Test-Path 'C:\Program Files\Webroot') -or (Test-Path 'C:\Program Files (x86)\Webroot') -or (@(Get-Service WRSVC,WRCoreService,WRSkyClient,WRBoot -EA SilentlyContinue).Count -gt 0) -or (@(Get-Process WRSA -EA SilentlyContinue).Count -gt 0) };
function ScCount { try{ @(Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -EA Stop | Where-Object {$_.displayName -like '*Webroot*'}).Count }catch{ -1 } };
$before=[ordered]@{installed=(StillInstalled);uninstall_entries=(UninstallEntries).Count;sc_registrations=(ScCount)};
$r=@();
function Step($n,$b,$v){
  $script:detail=$null; $err=$null;
  try{ & $b } catch { $err=$_.Exception.Message };
  $still=$true; try{ $still=[bool](& $v) } catch { $still=$true };
  $rec=[ordered]@{step=$n;ok=(-not $still);removed=(-not $still)};
  if($still){$rec.blocked=$true};
  if($script:detail){$rec.detail=(('{0}' -f $script:detail).Trim())};
  if($err){$rec.err=$err};
  $script:r+=$rec
};
Step 'kill_wrsa' { Get-Process WRSA,WRSACt64,WRSACt32 -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue } { @(Get-Process WRSA,WRSACt64,WRSACt32 -EA SilentlyContinue).Count -gt 0 };
foreach($svc in @('WRSVC','WRCoreService','WRSkyClient','WRBoot')){
  if(Get-Service $svc -EA SilentlyContinue){
    $sv=$svc;
    Step ('stop_'+$sv) { Stop-Service $sv -Force -EA SilentlyContinue } { (Get-Service $sv -EA SilentlyContinue).Status -eq 'Running' };
    Step ('delete_'+$sv) { $script:detail = (& sc.exe delete $sv 2>&1 | Out-String) } { @(Get-Service $sv -EA SilentlyContinue).Count -gt 0 }
  }
};
Step 'delete_WRkrn_driver' { if(Test-Path 'HKLM:\SYSTEM\CurrentControlSet\Services\WRkrn'){$script:detail = (& sc.exe delete WRkrn 2>&1 | Out-String)} } { Test-Path 'HKLM:\SYSTEM\CurrentControlSet\Services\WRkrn' };
foreach($d in @('C:\Program Files\Webroot','C:\Program Files (x86)\Webroot','C:\ProgramData\WRData','C:\ProgramData\WRCore','C:\ProgramData\WRMIDData')){
  if(Test-Path $d){ $dd=$d; Step ('rmdir_'+$dd) { Remove-Item $dd -Recurse -Force -EA SilentlyContinue } { Test-Path $dd } }
};
foreach($k in @('HKLM:\SOFTWARE\WRData','HKLM:\SOFTWARE\WRCore','HKLM:\SOFTWARE\WRMIDData','HKLM:\SOFTWARE\Webroot','HKLM:\SOFTWARE\WOW6432Node\WRData','HKLM:\SOFTWARE\WOW6432Node\WRCore','HKLM:\SOFTWARE\WOW6432Node\WRMIDData','HKLM:\SOFTWARE\WOW6432Node\Webroot')){
  if(Test-Path $k){ $kk=$k; Step ('rmreg_'+$kk) { Remove-Item $kk -Recurse -Force -EA SilentlyContinue } { Test-Path $kk } }
};
Step 'remove_uninstall_entries' { if(-not (StillInstalled)){ (UninstallEntries) | ForEach-Object { Remove-Item $_.PSPath -Recurse -Force -EA SilentlyContinue } } else { $script:detail='skipped: Webroot still installed — leaving the Uninstall entry to avoid stranding the product' } } { if(StillInstalled){ $false } else { (UninstallEntries).Count -gt 0 } };
foreach($run in @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run')){
  $rk=$run;
  Step ('clean_run_'+$rk) {
    $p=Get-ItemProperty $rk -EA SilentlyContinue;
    if($p){ $p.PSObject.Properties | Where-Object {$_.Name -match '(?i)webroot|wrsa'} | ForEach-Object { Remove-ItemProperty -Path $rk -Name $_.Name -EA SilentlyContinue } }
  } {
    $p=Get-ItemProperty $rk -EA SilentlyContinue;
    if(-not $p){ $false } else { @($p.PSObject.Properties | Where-Object {$_.Name -match '(?i)webroot|wrsa'}).Count -gt 0 }
  }
};
Step 'remove_scheduled_tasks' { Get-ScheduledTask -EA SilentlyContinue | Where-Object {$_.TaskName -match '(?i)webroot|wrsa'} | Unregister-ScheduledTask -Confirm:$false -EA SilentlyContinue } { @(Get-ScheduledTask -EA SilentlyContinue | Where-Object {$_.TaskName -match '(?i)webroot|wrsa'}).Count -gt 0 };
$after=[ordered]@{installed=(StillInstalled);uninstall_entries=(UninstallEntries).Count;sc_registrations=(ScCount)};
$blocked=@($r | Where-Object {-not $_.ok}).Count;
$warn=@();
if($blocked -gt 0){$warn+=('Webroot self-protection blocked {0} of {1} steps; ok:false means the target is still present.' -f $blocked,@($r).Count)};
if($after.installed){$warn+='Webroot is still installed. Shop policy is that Webroot stays - if this run was not the first half of a planned reinstall, stop here.'};
if($after.sc_registrations -gt 0){$warn+=('{0} Webroot Security Center registration(s) remain. This tool never touches root/SecurityCenter2 and cannot clear ghost WSC entries.' -f $after.sc_registrations)};
[ordered]@{steps=@($r);before=$before;after=$after;blocked_steps=$blocked;warnings=$warn}|ConvertTo-Json -Compress -Depth 6"#;

fn webroot_license() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] webroot_license");
    Ok(envelope("webroot_license", host::run_command(WEBROOT_LICENSE_PS)))
}

fn sas_license() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] sas_license");
    Ok(envelope("sas_license", host::run_command(SAS_LICENSE_PS)))
}

fn wsc_products() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] wsc_products");
    Ok(envelope("wsc_products", host::run_command(WSC_PRODUCTS_PS)))
}

fn webroot_status_deep() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] webroot_status_deep");
    Ok(envelope(
        "webroot_status_deep",
        host::run_command(WEBROOT_STATUS_DEEP_PS),
    ))
}

fn webroot_cleanup(a: CleanupArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm_full_uninstall != Some(true) {
        return Err(SdkError::invalid_args(
            "webroot_cleanup is a FULL uninstall of Webroot (software the shop sells), not remnant pruning and not a ghost-WSC fix. Pass confirm_full_uninstall:true only as the first half of a deliberate repair-and-reinstall with a re-key in hand.",
        ));
    }
    host::log("[cps] webroot_cleanup (confirmed)");
    Ok(envelope("webroot_cleanup", host::run_command(WEBROOT_CLEANUP_PS)))
}

mtech_plugin! {
    id: "com.mastertech.cps",
    name: "CPS Security Software",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// CPS / Webroot: installed state, Windows Security Center real-time protection, and days-remaining from registry heuristics.
        webroot_license() => webroot_license,
        /// CPS / SUPERAntiSpyware: installed state, WSC real-time protection, and days-remaining from registry heuristics.
        sas_license() => sas_license,
        /// All Windows Security Center products (AV/AntiSpyware/Firewall) with correctly decoded real-time and signature state, flagging any whose product exe is missing.
        wsc_products() => wsc_products,
        /// Deep Webroot remnant survey: services, WRkrn driver, install dirs, registry keys, ALL Security Center registrations (ghost count), WRSA process, scheduled tasks. Read-only.
        webroot_status_deep() => webroot_status_deep,
        /// DESTRUCTIVE full uninstall of Webroot SecureAnywhere. Skips deleting the Uninstall entry while the product is still installed (avoids stranding). Does NOT clear ghost WSC registrations. Requires confirm_full_uninstall:true.
        webroot_cleanup(CleanupArgs) => webroot_cleanup,
    }
}
