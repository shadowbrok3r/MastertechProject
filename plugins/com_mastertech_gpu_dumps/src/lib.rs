//! GPU / Unreal Engine crash-artifact plugin.
//!
//! Discovers `*.nv-gpudmp` and `CrashContext.runtime-xml` under
//! `C:\Users\*\AppData\Local\*\Saved\Crashes\**` for every user profile and
//! returns the embedded `FGenericCrashContext` XML as text. Every read shells
//! PowerShell through `host_run_command`; the guest has no preopened WASI
//! directories. `dump-triage` turns the XML into a crash signature; this plugin
//! does not parse it.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

const LIST_CAP: usize = 2 * 1024 * 1024;
const READ_CAP: usize = 1024 * 1024;
const XML_DEFAULT_CAP: u32 = 65_536;
const XML_HARD_CAP: u32 = 262_144;
const READABLE_SUFFIXES: [&str; 5] = [".nv-gpudmp", ".runtime-xml", ".xml", ".txt", ".dmp"];

#[derive(Facet, Deserialize)]
struct ListArgs {
    /// Max crash folders to return, newest first. Default 25, clamped to 1..200.
    limit: Option<u32>,
}

#[derive(Facet, Deserialize)]
struct ReadContextArgs {
    /// Absolute path to a .nv-gpudmp, CrashContext.runtime-xml, or any file holding an FGenericCrashContext block.
    path: String,
    /// Byte cap on the returned XML. Default 65536, clamped to 1024..262144.
    max_bytes: Option<u32>,
}

/// Escapes a value for a PowerShell single-quoted literal.
fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Requires an absolute Windows crash-artifact path with no traversal.
fn check_artifact_path(raw: &str) -> Result<String, SdkError> {
    let p = raw.trim();
    if p.is_empty() || p.len() > 4096 {
        return Err(SdkError::invalid_args("path must be 1..4096 bytes"));
    }
    if p.chars().any(char::is_control) || p.contains("..") {
        return Err(SdkError::invalid_args(
            "path must not contain control characters or '..'",
        ));
    }
    let b = p.as_bytes();
    let drive = b.len() > 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && matches!(b[2], b'\\' | b'/');
    if !drive {
        return Err(SdkError::invalid_args(
            "path must be absolute, e.g. C:\\Users\\...",
        ));
    }
    let lower = p.to_ascii_lowercase();
    if !READABLE_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        return Err(SdkError::invalid_args(
            "path must end in .nv-gpudmp, .runtime-xml, .xml, .txt or .dmp",
        ));
    }
    Ok(p.to_string())
}

/// First `n` bytes of `s` on a char boundary.
fn head(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let cut = (0..=n).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
    format!("{}...", &s[..cut])
}

/// Outermost `{...}` span, skipping the host's `[stderr]` suffix and any noise.
fn parse_ps_json(tool: &str, out: &str) -> Result<serde_json::Value, SdkError> {
    let start = out.find('{');
    let end = out.rfind('}');
    let slice = match (start, end) {
        (Some(a), Some(b)) if b > a => &out[a..=b],
        _ => {
            return Err(SdkError::host_failed(format!(
                "{tool}: no JSON in host output: {}",
                head(out, 400)
            )))
        }
    };
    let v: serde_json::Value = serde_json::from_str(slice).map_err(|e| {
        SdkError::host_failed(format!("{tool}: bad JSON from host ({e}): {}", head(slice, 400)))
    })?;
    match v.get("error").and_then(|e| e.as_str()) {
        Some(msg) => Err(SdkError::host_failed(msg.to_string())),
        None => Ok(v),
    }
}

/// Decodes standard base64, ignoring whitespace and stopping at padding.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 3);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Enumerates UE crash folders per user profile as compact JSON.
const LIST_TPL: &str = r##"$ErrorActionPreference='SilentlyContinue'
$ProgressPreference='SilentlyContinue'
try{[Console]::OutputEncoding=[System.Text.Encoding]::UTF8}catch{}
$limit={LIMIT}
$GPU='.nv-gpudmp'
function Kind($f){
  if($f.Extension -eq '.nv-gpudmp'){'aftermath'}
  elseif($f.Name -eq 'CrashContext.runtime-xml'){'crash_context'}
  elseif($f.Name -like 'Breadcrumbs_*'){'breadcrumbs'}
  elseif($f.Extension -eq '.dmp'){'minidump'}
  elseif($f.Extension -eq '.log'){'game_log'}
  else{'other'}
}
$rows=New-Object System.Collections.ArrayList
$users=@(Get-ChildItem -LiteralPath ($env:SystemDrive+'\Users') -Directory)
foreach($u in $users){
  $la=Join-Path $u.FullName 'AppData\Local'
  if(-not (Test-Path -LiteralPath $la)){ continue }
  foreach($proj in @(Get-ChildItem -LiteralPath $la -Directory)){
    $cr=Join-Path $proj.FullName 'Saved\Crashes'
    if(-not (Test-Path -LiteralPath $cr)){ continue }
    foreach($cf in @(Get-ChildItem -LiteralPath $cr -Directory)){
      $files=@(Get-ChildItem -LiteralPath $cf.FullName -File -Recurse -Depth 2)
      if($files.Count -eq 0){ continue }
      $gpu=@($files|Where-Object { $_.Extension -eq $GPU })
      $ctx=@($files|Where-Object { $_.Name -eq 'CrashContext.runtime-xml' })
      if($gpu.Count -eq 0 -and $ctx.Count -eq 0){ continue }
      $man=@($files|ForEach-Object {
        [PSCustomObject]@{ name=$_.Name; path=$_.FullName; size=$_.Length; kind=(Kind $_) }
      })
      [void]$rows.Add([PSCustomObject]@{
        user=$u.Name
        game=$proj.Name
        crash_guid=$cf.Name
        dir=$cf.FullName
        mtime=$cf.LastWriteTimeUtc.ToString('s')+'Z'
        total_bytes=(($files|Measure-Object Length -Sum).Sum)
        gpu_dump_count=$gpu.Count
        has_crash_context=($ctx.Count -gt 0)
        context_path=$(if($ctx.Count -gt 0){$ctx[0].FullName}elseif($gpu.Count -gt 0){$gpu[0].FullName}else{$null})
        files=$man
      })
    }
  }
}
$sorted=@($rows|Sort-Object -Property mtime -Descending|Select-Object -First $limit)
[PSCustomObject]@{
  scanned_users=$users.Count
  crash_folder_count=$rows.Count
  returned=$sorted.Count
  folders=$sorted
}|ConvertTo-Json -Depth 6 -Compress"##;

/// Extracts the embedded FGenericCrashContext byte range as base64.
const READ_TPL: &str = r##"$ErrorActionPreference='SilentlyContinue'
try{[Console]::OutputEncoding=[System.Text.Encoding]::UTF8}catch{}
$p='{PATH}'
$cap={CAP}
if(-not (Test-Path -LiteralPath $p)){ '{"error":"path not found"}'; exit }
$fi=Get-Item -LiteralPath $p
if($fi.PSIsContainer){ '{"error":"path is a directory"}'; exit }
if($fi.Length -gt 64MB){ '{"error":"file too large to scan for an embedded crash context"}'; exit }
$b=[System.IO.File]::ReadAllBytes($p)
$s=[System.Text.Encoding]::GetEncoding(28591).GetString($b)
$i=$s.IndexOf('<FGenericCrashContext')
if($i -lt 0){ '{"error":"no FGenericCrashContext block found in this file"}'; exit }
$e=$s.IndexOf('</FGenericCrashContext>',$i)
$len=$(if($e -ge 0){ $e-$i+23 } else { $s.Length-$i })
$truncated=$false
if($len -gt $cap){ $len=$cap; $truncated=$true }
$raw=[byte[]]::new($len)
[System.Array]::Copy($b,$i,$raw,0,$len)
[PSCustomObject]@{
  path=$fi.FullName
  file_bytes=$fi.Length
  file_mtime=$fi.LastWriteTimeUtc.ToString('s')+'Z'
  xml_offset=$i
  xml_bytes=$len
  truncated=$truncated
  well_formed=($e -ge 0)
  xml_base64=[Convert]::ToBase64String($raw)
}|ConvertTo-Json -Depth 3 -Compress"##;

fn list_gpu_dumps(a: ListArgs) -> Result<serde_json::Value, SdkError> {
    let limit = a.limit.unwrap_or(25).clamp(1, 200);
    host::log(&format!("[gpu-dumps] list_gpu_dumps limit={limit}"));
    let script = LIST_TPL.replace("{LIMIT}", &limit.to_string());
    let out = host::run_command_capped(&script, LIST_CAP);
    let data = parse_ps_json("list_gpu_dumps", &out)?;
    Ok(serde_json::json!({ "tool": "list_gpu_dumps", "data": data }))
}

fn read_gpu_dump_context(a: ReadContextArgs) -> Result<serde_json::Value, SdkError> {
    let path = check_artifact_path(&a.path)?;
    let cap = a.max_bytes.unwrap_or(XML_DEFAULT_CAP).clamp(1024, XML_HARD_CAP);
    host::log(&format!("[gpu-dumps] read_gpu_dump_context {path} cap={cap}"));
    let script = READ_TPL
        .replace("{PATH}", &ps_quote(&path))
        .replace("{CAP}", &cap.to_string());
    let out = host::run_command_capped(&script, READ_CAP);
    let mut data = parse_ps_json("read_gpu_dump_context", &out)?;
    let b64 = data
        .as_object_mut()
        .and_then(|m| m.remove("xml_base64"))
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let xml = b64_decode(&b64)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    if xml.is_empty() {
        return Err(SdkError::host_failed(
            "extracted crash context was empty or not valid base64",
        ));
    }
    if let Some(m) = data.as_object_mut() {
        m.insert("xml".to_string(), serde_json::Value::String(xml));
    }
    Ok(serde_json::json!({ "tool": "read_gpu_dump_context", "data": data }))
}

mtech_plugin! {
    id: "com.mastertech.gpu-dumps",
    name: "GPU Crash Dumps",
    version: "0.1.0",
    heap: 8 * 1024 * 1024,
    tools: {
        /// Enumerate GPU and Unreal Engine crash artifacts for EVERY user profile on this machine.
        /// Walks C:\Users\*\AppData\Local\<AnyUEProject>\Saved\Crashes\<UECC-Windows-GUID_NNNN>\ -
        /// FortniteGame is NOT special-cased, every UE title uses this layout - and returns one row per
        /// crash folder: user, game (the LOCALAPPDATA project folder), crash_guid, dir, mtime, total_bytes,
        /// gpu_dump_count, has_crash_context, context_path (feed this straight to read_gpu_dump_context),
        /// and a files[] manifest tagging each artifact aftermath (*.nv-gpudmp), crash_context
        /// (CrashContext.runtime-xml), breadcrumbs (Breadcrumbs_*), minidump (*.dmp), game_log (*.log) or
        /// other. Folders with neither an aftermath dump nor a crash context are skipped. Newest first;
        /// limit defaults to 25 and is clamped to 200.
        list_gpu_dumps(ListArgs) => list_gpu_dumps,
        /// Return the FGenericCrashContext XML embedded in a GPU crash artifact as PLAIN TEXT - no binary
        /// transfer is needed for the useful part. Works on *.nv-gpudmp (an NVIDIA Aftermath dump carries
        /// the entire Unreal crash context verbatim), on CrashContext.runtime-xml, and on any file holding
        /// an <FGenericCrashContext> block. The file is scanned as ISO-8859-1 so character indices are
        /// exact byte offsets, and the original byte range is returned undecoded then handed back as UTF-8,
        /// which keeps non-ASCII map and user names intact. Expect roughly 9 KB. max_bytes caps the returned
        /// XML (default 65536, hard cap 262144); truncated:true means the cap was hit and well_formed:false
        /// means the closing tag was missing. The admin console auto-ingests this result as a crash_sighting
        /// with dump_kind 'gpu_aftermath': GPUCrash.D3DDeviceRemovedReason is a SIGNED int32, so it is
        /// masked with 0xFFFFFFFF before naming it (-2005270521 -> 0x887A0007 DXGI_ERROR_DEVICE_RESET), and
        /// in <Breadcrumbs> the trailing marker letter is the node state, so the DEEPEST A-marked node is
        /// where the GPU stopped executing. path must be absolute and end in .nv-gpudmp, .runtime-xml,
        /// .xml, .txt or .dmp.
        read_gpu_dump_context(ReadContextArgs) => read_gpu_dump_context,
    }
}
