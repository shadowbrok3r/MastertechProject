//! TechDB driver share + BiosLove firmware share plugin (SDK port).
//!
//! Mounts the PCL TechDB driver share and the BiosLove firmware share with the
//! RIV tech credentials and lists/stages OEM driver and BIOS/EC packages. The
//! audio/sleep/restart one-offs from v0.7 are dropped (they overlap
//! remote_reboot_client / scripts_run / minidump_analyze). Typed args replace
//! the old bare-string hack. NOTE: credentials are RIV-only and hardcoded; each
//! store has its own OPK, so this works against opk-riv only for now.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

const SHARE_ROOT: &str = r"\\opk-riv\winbits\Drivers\7\TechDB";
const BIOSLOVE_ROOT: &str = r"\\opk-riv\winbits\Drivers\Thumb\multiboot\BiosLove";
const SHARE_USER: &str = "Images";
const SHARE_PASS: &str = "Password123";
const SHARE_SERVER: &str = r"\\opk-riv\winbits";
const LIST_CAP: usize = 4 * 1024 * 1024;

#[derive(Facet, Deserialize)]
struct ModelArgs {
    /// TechDB model folder name, e.g. GX5HRXG.
    model: String,
}

#[derive(Facet, Deserialize)]
struct FetchModelArgs {
    /// TechDB model folder name, e.g. GX5HRXG.
    model: String,
    /// Relative path under the model to copy; omit to stage the whole model folder.
    relpath: Option<String>,
}

#[derive(Facet, Deserialize)]
struct BiosModelArgs {
    /// BiosLove side: laptop or desktop.
    side: String,
    /// BiosLove model folder name, e.g. PD50SNEG.
    folder: String,
}

#[derive(Facet, Deserialize)]
struct FetchFwArgs {
    /// BiosLove side: laptop or desktop.
    side: String,
    /// Relative path under the side (folder or folder\\sub) to stage.
    relpath: String,
}

fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

/// TechDB/BiosLove folder-name charset, blocking quotes and traversal.
fn sanitize_model(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '&' | ' ' | '(' | ')'))
        .collect();
    if cleaned.contains("..") {
        String::new()
    } else {
        cleaned.trim().to_string()
    }
}

/// Wider relpath charset (adds backslash for nesting), blocking traversal.
fn sanitize_relpath(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| {
            c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '&' | ' ' | '\\' | '(' | ')')
        })
        .collect();
    if cleaned.contains("..") {
        String::new()
    } else {
        cleaned.trim_matches(['\\', ' ']).to_string()
    }
}

/// laptop|desktop only.
fn sanitize_side(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "laptop" => Some("laptop"),
        "desktop" => Some("desktop"),
        _ => None,
    }
}

/// Drops any prior mapping (avoids error 1219 when already mapped as another
/// user), then remounts with the tech credentials.
fn mount_share() {
    let _ = host::run_command(&format!(
        "net use {SHARE_SERVER} /delete /y 2>&1 | Out-Null; net use {SHARE_SERVER} {SHARE_PASS} /user:{SHARE_USER} 2>&1 | Out-String"
    ));
}

fn list_techdb_models() -> Result<serde_json::Value, SdkError> {
    host::log("[driver-fetch] list_techdb_models");
    mount_share();
    let cmd = format!(
        r#"try {{ Get-ChildItem -LiteralPath '{SHARE_ROOT}' -Directory -EA Stop | Select-Object -ExpandProperty Name | ConvertTo-Json -Compress }} catch {{ "ERROR: $($_.Exception.Message)" }}"#
    );
    Ok(envelope("list_techdb_models", host::run_command(&cmd)))
}

fn list_model_drivers(a: ModelArgs) -> Result<serde_json::Value, SdkError> {
    let model = sanitize_model(&a.model);
    if model.is_empty() {
        return Err(SdkError::invalid_args("model is required (empty or contained '..')"));
    }
    host::log(&format!("[driver-fetch] list_model_drivers model={model}"));
    mount_share();
    let cmd = format!(
        r#"$path = '{SHARE_ROOT}\{model}'
try {{
    Get-ChildItem -LiteralPath $path -Recurse -EA Stop |
        Select-Object @{{n='RelPath';e={{$_.FullName.Substring($path.Length+1)}}}},Length,LastWriteTime,PSIsContainer |
        ConvertTo-Json -Compress -Depth 4
}} catch {{ "ERROR: $($_.Exception.Message)" }}"#
    );
    Ok(envelope("list_model_drivers", host::run_command_capped(&cmd, LIST_CAP)))
}

fn fetch_model_path(a: FetchModelArgs) -> Result<serde_json::Value, SdkError> {
    let model = sanitize_model(&a.model);
    if model.is_empty() {
        return Err(SdkError::invalid_args("model is required (empty or contained '..')"));
    }
    let relpath = match a.relpath.as_deref() {
        Some(r) if !r.trim().is_empty() => {
            let s = sanitize_relpath(r);
            if s.is_empty() {
                return Err(SdkError::invalid_args("relpath rejected (contained '..')"));
            }
            s
        }
        _ => String::new(),
    };
    host::log(&format!("[driver-fetch] fetch_model_path model={model} relpath={relpath}"));
    mount_share();
    let (src_suffix, dst_suffix) = if relpath.is_empty() {
        (String::new(), String::new())
    } else {
        (format!(r"\{relpath}"), relpath.clone())
    };
    let cmd = format!(
        r#"$src = '{SHARE_ROOT}\{model}{src_suffix}'
$model = '{model}'
$relpath = '{dst_suffix}'
try {{
    if (!(Test-Path -LiteralPath $src)) {{ throw "Source not found: $src" }}
    $item = Get-Item -LiteralPath $src -Force
    if ($item.PSIsContainer) {{
        $dst = if ($relpath) {{ "C:\ProgramData\MTechDrivers\$model\$relpath" }} else {{ "C:\ProgramData\MTechDrivers\$model" }}
        New-Item -ItemType Directory -Force -Path $dst | Out-Null
        robocopy $src $dst /MIR /R:1 /W:1 | Out-Null
        $files = Get-ChildItem -LiteralPath $dst -Recurse -File | Select-Object Name,Length
        [ordered]@{{ staged_path = $dst; is_file = $false; file_count = @($files).Count; files = $files }} | ConvertTo-Json -Compress -Depth 4
    }} else {{
        $parent = Split-Path $relpath -Parent
        $dstDir = if ($parent) {{ "C:\ProgramData\MTechDrivers\$model\$parent" }} else {{ "C:\ProgramData\MTechDrivers\$model" }}
        New-Item -ItemType Directory -Force -Path $dstDir | Out-Null
        Copy-Item -LiteralPath $src -Destination $dstDir -Force
        $dstFile = Join-Path $dstDir $item.Name
        [ordered]@{{ staged_path = $dstFile; is_file = $true; file_count = 1; files = @(@{{ Name = $item.Name; Length = $item.Length }}) }} | ConvertTo-Json -Compress -Depth 4
    }}
}} catch {{ "ERROR: $($_.Exception.Message)" }}"#
    );
    Ok(envelope("fetch_model_path", host::run_command_capped(&cmd, LIST_CAP)))
}

/// ver.txt parse + payload inventory + stale-script check, shared by the firmware tools.
const PS_INSPECT_FOLDER: &str = r##"
function Inspect-FirmwareFolder($dir) {
    $r = [ordered]@{}
    $r.folder_path = $dir
    $verPath = Join-Path $dir 'ver.txt'
    if (Test-Path -LiteralPath $verPath) {
        $ver = [string](Get-Content -LiteralPath $verPath -Raw -EA SilentlyContinue)
        $r.ver_txt_present = $true
        if ($ver.Length -gt 1200) { $r.ver_txt = $ver.Substring(0,1200) + '...[truncated]' } else { $r.ver_txt = $ver }
        $r.bios_versions = @([regex]::Matches($ver,'(?im)^\s*B:\s*(.+?)\s*$') | ForEach-Object { $_.Groups[1].Value })
        $r.ec_versions   = @([regex]::Matches($ver,'(?im)^\s*E:\s*(.+?)\s*$') | ForEach-Object { $_.Groups[1].Value })
        $r.me_versions   = @([regex]::Matches($ver,'(?im)^\s*ME:\s*(.+?)\s*$') | ForEach-Object { $_.Groups[1].Value })
        $r.ver_txt_date  = (Get-Item -LiteralPath $verPath).LastWriteTime.ToString('yyyy-MM-dd')
    } else {
        $r.ver_txt_present = $false
        $r.note_no_ver = 'No ver.txt in this folder (only ~36% of laptop folders have one, and no desktop folders do). Infer the version from the payload filename in bios_payloads.'
    }
    $files = @(Get-ChildItem -LiteralPath $dir -File -EA SilentlyContinue)
    $r.bios_payloads = @($files | Where-Object {
            $_.Length -gt 100KB -and (
                $_.Extension -match '(?i)^\.(efi|exe|rom|bin|cap)$' -or
                ($_.Length -gt 1MB -and $_.Extension -notmatch '(?i)^\.(pdf|doc|docx|txt|xls|xlsx|nsh|bat|zip|7z|rar|log|ini|cat|inf|cer|pfx|pvk|md)$')
            )
        } |
        Select-Object Name,@{n='MB';e={[math]::Round($_.Length/1MB,2)}},@{n='Date';e={$_.LastWriteTime.ToString('yyyy-MM-dd')}})
    $r.flash_scripts = @($files | Where-Object { $_.Extension -match '(?i)^\.(nsh|bat)$' } |
        Select-Object Name,@{n='Date';e={$_.LastWriteTime.ToString('yyyy-MM-dd')}})
    $present = @($files | Select-Object -ExpandProperty Name)
    $presentLower = @($present | ForEach-Object { $_.ToLower() })
    $stale = @()
    $sysBins = @('cacls.exe','cmd.exe','reg.exe','robocopy.exe','xcopy.exe','shutdown.exe','powershell.exe','wscript.exe','cscript.exe','choice.exe','find.exe','findstr.exe','timeout.exe','del.exe')
    foreach ($s in ($files | Where-Object { $_.Extension -match '(?i)^\.(nsh|bat)$' })) {
        $lines = @(Get-Content -LiteralPath $s.FullName -EA SilentlyContinue)
        if (-not $lines) { continue }
        $live = @($lines | Where-Object { $_.Trim() -notmatch '^\s*(#|::|rem\s|@?rem\s)' })
        if (-not $live) { continue }
        $txt = ($live -join "`n")
        $refs = @()
        foreach ($m in [regex]::Matches($txt,'(?i)\b[A-Za-z0-9_\-]+\.(?:efi|exe|rom|bin|cap)\b')) { $refs += $m.Value }
        foreach ($m in [regex]::Matches($txt,'(?im)^\s*@?set\s+(?:BIOSROM|EC1|EC_ROM|FLASH_TOOL|KBCK_TOOL)\s+([A-Za-z0-9_\-\.]+)')) { $refs += $m.Groups[1].Value }
        foreach ($ref in ($refs | Sort-Object -Unique)) {
            if ($sysBins -contains $ref.ToLower()) { continue }
            if ($presentLower -notcontains $ref.ToLower()) {
                $stale += [pscustomobject]@{ script = $s.Name; references = $ref; exists = $false }
            }
        }
    }
    $r.stale_script_refs = @($stale)
    if ($stale.Count -gt 0) {
        $r.stale_warning = 'These scripts name payload files that are NOT in the folder - they are leftovers from an earlier drop and will fail. Do not follow help.txt blindly; drive the payloads listed in bios_payloads (flash.nsh / FlashWinX64.bat for BIOS, EcFlash.nsh for EC).'
    }
    $r.ec_payloads = @($files | Where-Object { $_.Name -match '(?i)^[A-Za-z0-9_\-]+\.\d{2}$' -and $_.Length -lt 1MB } |
        Select-Object Name,@{n='KB';e={[math]::Round($_.Length/1KB,0)}},@{n='Date';e={$_.LastWriteTime.ToString('yyyy-MM-dd')}})
    $r.doc_files = @($files | Where-Object { $_.Extension -match '(?i)^\.(pdf|doc|docx|txt)$' } | Select-Object -ExpandProperty Name)
    return $r
}
"##;

fn find_firmware_for_this_machine() -> Result<serde_json::Value, SdkError> {
    host::log("[driver-fetch] find_firmware_for_this_machine");
    mount_share();
    let body = r##"
$ErrorActionPreference = 'Continue'
$root = '@@BIOSLOVE@@'
$o = [ordered]@{}
$cs  = Get-CimInstance Win32_ComputerSystem -EA SilentlyContinue
$bb  = Get-CimInstance Win32_BaseBoard -EA SilentlyContinue
$bi  = Get-CimInstance Win32_BIOS -EA SilentlyContinue
$enc = Get-CimInstance Win32_SystemEnclosure -EA SilentlyContinue
$chassis = if ($bb -and $bb.Product) { $bb.Product } else { $cs.Model }
$o.system_manufacturer = $cs.Manufacturer
$o.system_model        = $cs.Model
$o.baseboard_product   = if ($bb) { $bb.Product } else { $null }
$o.chassis_string      = $chassis
$o.installed_bios      = if ($bi) { $bi.SMBIOSBIOSVersion } else { $null }
$o.installed_bios_date = if ($bi -and $bi.ReleaseDate) { $bi.ReleaseDate.ToString('yyyy-MM-dd') } else { $null }
$lapTypes = @(8,9,10,11,12,14,18,21,30,31,32)
$isLaptop = $false
if ($enc) { foreach ($t in @($enc.ChassisTypes)) { if ($lapTypes -contains [int]$t) { $isLaptop = $true } } }
$o.form_factor = if ($isLaptop) { 'laptop' } else { 'desktop' }
function Norm($s) { if ($null -eq $s) { return '' } ; return (($s -replace '[^A-Za-z0-9]','').ToUpper()) }
function Get-MatchTokens($s) {
    $sep = ([string]$s) -replace '[0-9]','_'
    $raw = @([regex]::Matches($sep,'[A-Za-z]{3,}') | ForEach-Object { $_.Value.ToUpper() })
    $out = @()
    foreach ($t in $raw) {
        $out += $t
        if ($t.Length -ge 4 -and $t.StartsWith('X')) { $out += $t.Substring(1) }
    }
    return @($out | Sort-Object -Unique)
}
$normChassis = Norm $chassis
$prefix = ([regex]::Match([string]$chassis,'^[A-Za-z]+')).Value.ToUpper()
$tokens = Get-MatchTokens $chassis
$o.match_prefix = $prefix
$o.match_tokens = $tokens
$sides = if ($isLaptop) { @('laptop','desktop') } else { @('desktop','laptop') }
$cands = @()
$counts = [ordered]@{}
foreach ($side in $sides) {
    $sp = Join-Path $root $side
    if (-not (Test-Path -LiteralPath $sp)) { $counts[$side] = 'NOT ACCESSIBLE'; continue }
    $dirs = @(Get-ChildItem -LiteralPath $sp -Directory -EA SilentlyContinue)
    $counts[$side] = $dirs.Count
    foreach ($d in $dirs) {
        $score = 0
        $why = @()
        $nd = Norm $d.Name
        if ($nd -eq $normChassis) { $score += 100; $why += 'exact-folder-name' }
        $verPath = Join-Path $d.FullName 'ver.txt'
        $hasVer = Test-Path -LiteralPath $verPath
        if ($hasVer) {
            $nv = Norm ([string](Get-Content -LiteralPath $verPath -Raw -EA SilentlyContinue))
            if ($normChassis -and $nv.Contains($normChassis)) { $score += 60; $why += 'chassis-in-ver.txt' }
            foreach ($t in $tokens) { if ($nv.Contains($t)) { $score += 8; $why += ('tokver:' + $t) } }
        }
        foreach ($t in $tokens) { if ($nd.Contains($t)) { $score += 12; $why += ('tokdir:' + $t) } }
        if ($prefix -and $nd.StartsWith($prefix)) { $score += 25; $why += 'prefix' }
        if ($side -eq $o.form_factor) { $score += 4; $why += 'form-factor' }
        if ($score -ge 20) {
            $cands += [pscustomobject]@{ side = $side; folder = $d.Name; score = $score; matched_on = ($why -join ','); has_ver_txt = $hasVer; path = $d.FullName }
        }
    }
}
$o.folders_scanned = $counts
$ranked = @($cands | Sort-Object score -Descending)
$o.candidates = @($ranked | Select-Object -First 6 side,folder,score,matched_on,has_ver_txt)
if ($ranked.Count -eq 0) {
    $o.verdict = "NO MATCH. No BiosLove folder scored above threshold for chassis '$chassis'. Fall back to list_bioslove_model with a folder name you pick by hand, or the firmware is not on the share."
} else {
    $best = $ranked[0]
    $o.best_match = [ordered]@{ side = $best.side; folder = $best.folder; score = $best.score; matched_on = $best.matched_on }
    $o.best_match_detail = Inspect-FirmwareFolder $best.path
    $avail = @($o.best_match_detail.bios_versions)
    $inst  = [string]$o.installed_bios
    if ($avail.Count -eq 0) {
        $o.verdict = "Matched $($best.side)\$($best.folder) but it has no ver.txt B: line. Installed BIOS is '$inst'. Compare against the payload filename in best_match_detail.bios_payloads and confirm the model list before flashing."
    } elseif ($avail -contains $inst) {
        $o.verdict = "CURRENT. Installed BIOS '$inst' matches an available version in $($best.side)\$($best.folder) ver.txt ($($avail -join ', ')). No BIOS flash needed. EC available: $(@($o.best_match_detail.ec_versions) -join ', ') - EC is versioned separately and a BIOS flash does not update it, so verify EC in BIOS setup."
    } else {
        $o.verdict = "UPDATE CANDIDATE. Installed BIOS '$inst' is not among the versions available in $($best.side)\$($best.folder) ver.txt ($($avail -join ', ')). EC available: $(@($o.best_match_detail.ec_versions) -join ', '). Confirm the ver.txt model list actually covers this chassis before flashing - matching is fuzzy. A BIOS flash does NOT update EC firmware; EcFlash.nsh is a separate pass."
    }
}
$o | ConvertTo-Json -Compress -Depth 5
"##;
    let script = format!("{PS_INSPECT_FOLDER}{body}").replace("@@BIOSLOVE@@", BIOSLOVE_ROOT);
    Ok(envelope(
        "find_firmware_for_this_machine",
        host::run_command_capped(&script, LIST_CAP),
    ))
}

fn list_bioslove_model(a: BiosModelArgs) -> Result<serde_json::Value, SdkError> {
    let Some(side) = sanitize_side(&a.side) else {
        return Err(SdkError::invalid_args("side must be laptop or desktop"));
    };
    let folder = sanitize_model(&a.folder);
    if folder.is_empty() {
        return Err(SdkError::invalid_args("folder is required (empty or contained '..')"));
    }
    host::log(&format!("[driver-fetch] list_bioslove_model side={side} folder={folder}"));
    mount_share();
    let body = r##"
$ErrorActionPreference = 'Continue'
$dir = '@@BIOSLOVE@@\@@SIDE@@\@@FOLDER@@'
if (-not (Test-Path -LiteralPath $dir)) {
    [ordered]@{ error = "Not found: $dir" } | ConvertTo-Json -Compress
} else {
    $o = Inspect-FirmwareFolder $dir
    $o.side = '@@SIDE@@'
    $o.folder = '@@FOLDER@@'
    $o.all_entries = @(Get-ChildItem -LiteralPath $dir -Recurse -EA SilentlyContinue |
        Select-Object @{n='RelPath';e={$_.FullName.Substring($dir.Length+1)}},
                      @{n='KB';e={if($_.PSIsContainer){$null}else{[math]::Round($_.Length/1KB,0)}}},
                      @{n='Date';e={$_.LastWriteTime.ToString('yyyy-MM-dd')}},
                      PSIsContainer)
    $o | ConvertTo-Json -Compress -Depth 6
}
"##;
    let script = format!("{PS_INSPECT_FOLDER}{body}")
        .replace("@@BIOSLOVE@@", BIOSLOVE_ROOT)
        .replace("@@SIDE@@", side)
        .replace("@@FOLDER@@", &folder);
    Ok(envelope("list_bioslove_model", host::run_command_capped(&script, LIST_CAP)))
}

fn fetch_firmware_path(a: FetchFwArgs) -> Result<serde_json::Value, SdkError> {
    let Some(side) = sanitize_side(&a.side) else {
        return Err(SdkError::invalid_args("side must be laptop or desktop"));
    };
    let relpath = sanitize_relpath(&a.relpath);
    if relpath.is_empty() {
        return Err(SdkError::invalid_args("relpath is required (empty or contained '..')"));
    }
    host::log(&format!("[driver-fetch] fetch_firmware_path side={side} relpath={relpath}"));
    mount_share();
    let cmd = format!(
        r#"$src = '{BIOSLOVE_ROOT}\{side}\{relpath}'
$side = '{side}'
$relpath = '{relpath}'
try {{
    if (!(Test-Path -LiteralPath $src)) {{ throw "Source not found: $src" }}
    $item = Get-Item -LiteralPath $src -Force
    if ($item.PSIsContainer) {{
        $dst = "C:\ProgramData\MTechFirmware\$side\$relpath"
        New-Item -ItemType Directory -Force -Path $dst | Out-Null
        robocopy $src $dst /MIR /R:1 /W:1 | Out-Null
        $files = Get-ChildItem -LiteralPath $dst -Recurse -File | Select-Object Name,Length
        [ordered]@{{ staged_path = $dst; is_file = $false; file_count = @($files).Count; files = $files; note = "Staged only - nothing was flashed. BIOS: FlashWinX64.bat (Windows) or flash.nsh (UEFI shell). EC: EcFlash.nsh, a separate pass that a BIOS flash does not cover." }} | ConvertTo-Json -Compress -Depth 4
    }} else {{
        $parent = Split-Path $relpath -Parent
        $dstDir = if ($parent) {{ "C:\ProgramData\MTechFirmware\$side\$parent" }} else {{ "C:\ProgramData\MTechFirmware\$side" }}
        New-Item -ItemType Directory -Force -Path $dstDir | Out-Null
        Copy-Item -LiteralPath $src -Destination $dstDir -Force
        $dstFile = Join-Path $dstDir $item.Name
        [ordered]@{{ staged_path = $dstFile; is_file = $true; file_count = 1; files = @(@{{ Name = $item.Name; Length = $item.Length }}); note = "Staged only - nothing was flashed." }} | ConvertTo-Json -Compress -Depth 4
    }}
}} catch {{ "ERROR: $($_.Exception.Message)" }}"#
    );
    Ok(envelope("fetch_firmware_path", host::run_command_capped(&cmd, LIST_CAP)))
}

mtech_plugin! {
    id: "com.mastertech.driver-fetch",
    name: "TechDB Driver Fetch",
    version: "0.8.0",
    heap: 8 * 1024 * 1024,
    tools: {
        /// List model folders under the PCL TechDB driver share. TechDB is DRIVERS ONLY; BIOS/EC firmware is on BiosLove (use find_firmware_for_this_machine).
        list_techdb_models() => list_techdb_models,
        /// Recursively list every file/folder for one TechDB model. Requires model (e.g. GX5HRXG).
        list_model_drivers(ModelArgs) => list_model_drivers,
        /// Copy a TechDB driver folder or file to C:\ProgramData\MTechDrivers. Requires model; omit relpath to stage the whole model.
        fetch_model_path(FetchModelArgs) => fetch_model_path,
        /// Read this machine's SMBIOS chassis, fuzzy-match it against BiosLove, and report available BIOS/EC/ME vs installed, with a stale_script_refs warning. No args.
        find_firmware_for_this_machine() => find_firmware_for_this_machine,
        /// Recursively inspect one BiosLove firmware folder. Requires side (laptop|desktop) and folder.
        list_bioslove_model(BiosModelArgs) => list_bioslove_model,
        /// Stage a BiosLove firmware folder/file to C:\ProgramData\MTechFirmware (never flashes). Requires side (laptop|desktop) and relpath.
        fetch_firmware_path(FetchFwArgs) => fetch_firmware_path,
    }
}
