//! Driver Time Machine client plugin.
//!
//! Snapshots the Windows DriverStore (pnputil), exports driver packages as
//! rollback points, and stages/commits rollbacks. The admin console parses
//! `snapshot` text with `database::schema::driver_intel::parse_pnputil_enum`
//! and persists it as a `driver_snapshot` row.

unsafe extern "C" {
    fn host_log(ptr: i32, len: i32);
    fn host_run_command(cmd_ptr: i32, cmd_len: i32, out_ptr: i32, out_max: i32) -> i32;
}

const OUT_CAP: i32 = 1024 * 1024;

fn log_msg(msg: &str) {
    unsafe { host_log(msg.as_ptr() as i32, msg.len() as i32) };
}

fn run_ps(cmd: &str) -> String {
    let mut buf = vec![0u8; OUT_CAP as usize];
    let n = unsafe {
        host_run_command(
            cmd.as_ptr() as i32,
            cmd.len() as i32,
            buf.as_mut_ptr() as i32,
            buf.len() as i32,
        )
    };
    if n <= 0 {
        return String::new();
    }
    buf.truncate(n as usize);
    String::from_utf8_lossy(&buf).into_owned()
}

fn leak_bytes(b: &[u8]) -> u64 {
    let boxed: Box<[u8]> = b.to_vec().into_boxed_slice();
    let len = boxed.len() as u64;
    let ptr = Box::into_raw(boxed) as *mut u8 as u64;
    (ptr << 32) | (len & 0xffff_ffff)
}

fn set_output(s: String) -> u64 {
    leak_bytes(s.as_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(n: i32) -> i32 {
    let mut v: Vec<u8> = Vec::with_capacity(n as usize);
    let p = v.as_mut_ptr() as i32;
    std::mem::forget(v);
    p
}

#[unsafe(no_mangle)]
pub extern "C" fn dealloc(ptr: i32, size: i32) {
    unsafe {
        let _ = Vec::from_raw_parts(ptr as *mut u8, size as usize, size as usize);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_id() -> u64 {
    leak_bytes(b"com.mastertech.driverstore")
}
#[unsafe(no_mangle)]
pub extern "C" fn plugin_name() -> u64 {
    leak_bytes(b"Driver Time Machine")
}
#[unsafe(no_mangle)]
pub extern "C" fn plugin_version() -> u64 {
    leak_bytes(b"0.1.0")
}
#[unsafe(no_mangle)]
pub extern "C" fn on_load() {
    log_msg("Driver Time Machine v0.1.0 loaded");
}
#[unsafe(no_mangle)]
pub extern "C" fn on_unload() {}
#[unsafe(no_mangle)]
pub extern "C" fn logic() {}
#[unsafe(no_mangle)]
pub extern "C" fn ui_commands() -> u64 {
    set_output("[]".into())
}

#[unsafe(no_mangle)]
pub extern "C" fn mcp_tools() -> u64 {
    let t = concat!(
        r#"[{"name":"snapshot","description":"Full DriverStore inventory via 'pnputil /enum-drivers'. Returns raw pnputil text for the admin console to parse and persist as a driver_snapshot row.","parameters_schema":{"type":"object","properties":{}}},"#,
        r#"{"name":"list_exports","description":"List exported driver-package rollback points under C:\\ProgramData\\MTechDriverStore.","parameters_schema":{"type":"object","properties":{}}},"#,
        r#"{"name":"export_driver","description":"Export one driver package (rollback point) to C:\\ProgramData\\MTechDriverStore\\<inf>_<timestamp> via 'pnputil /export-driver'.","parameters_schema":{"type":"object","properties":{"published_name":{"type":"string","description":"Published INF name, e.g. oem12.inf"}},"required":["published_name"]}},"#,
        r#"{"name":"rollback_driver","description":"DESTRUCTIVE - confirm with a tech first. Optionally 'pnputil /delete-driver <published_name> /uninstall /force' the current package, then 'pnputil /add-driver <restore_path>\\*.inf /subdirs /install' a previously exported one. Reboot may be required.","parameters_schema":{"type":"object","properties":{"restore_path":{"type":"string","description":"Folder of a previously exported driver package under C:\\ProgramData\\MTechDriverStore"},"delete_published_name":{"type":"string","description":"Optional current published INF (oemXX.inf) to uninstall before restoring"}},"required":["restore_path"]}}]"#,
    );
    set_output(t.to_string())
}

/// Allow only pnputil-safe tokens (oemNN.inf, export folder names, drive paths).
fn sanitize_arg(v: &str, allow_path: bool) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 200 {
        return None;
    }
    let ok = v.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '.' | '_' | '-')
            || (allow_path && matches!(c, '\\' | '/' | ':' | ' '))
    });
    if !ok || v.contains("..") {
        return None;
    }
    Some(v.to_string())
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

fn snapshot() -> String {
    log_msg("[driverstore] snapshot");
    let out = run_ps("pnputil /enum-drivers");
    if out.trim().is_empty() {
        return err_json("pnputil produced no output");
    }
    serde_json::json!({
        "tool": "snapshot",
        "source": "pnputil",
        "driver_text": out,
    })
    .to_string()
}

fn list_exports() -> String {
    log_msg("[driverstore] list_exports");
    let cmd = r##"$ErrorActionPreference='SilentlyContinue'
$root='C:\ProgramData\MTechDriverStore'
if(-not (Test-Path $root)){ '{"exports":[]}'; exit }
$rows = Get-ChildItem $root -Directory | ForEach-Object {
  $size = (Get-ChildItem $_.FullName -Recurse -File | Measure-Object Length -Sum).Sum
  [PSCustomObject]@{ name=$_.Name; path=$_.FullName; created=$_.CreationTime.ToString('s'); mb=[math]::Round(($size ?? 0)/1MB,1) }
}
[PSCustomObject]@{ exports=@($rows) } | ConvertTo-Json -Depth 4 -Compress"##;
    let out = run_ps(cmd);
    let t = out.trim();
    if t.is_empty() {
        return err_json("no output");
    }
    format!("{{\"tool\":\"list_exports\",\"data\":{t}}}")
}

fn export_driver(args: &serde_json::Value) -> String {
    let Some(inf) = args
        .get("published_name")
        .and_then(|v| v.as_str())
        .and_then(|v| sanitize_arg(v, false))
    else {
        return err_json("published_name (oemXX.inf) is required");
    };
    log_msg(&format!("[driverstore] export_driver {inf}"));
    let stem = inf.trim_end_matches(".inf");
    let cmd = format!(
        r##"$ErrorActionPreference='SilentlyContinue'
$root='C:\ProgramData\MTechDriverStore'
New-Item -ItemType Directory -Force $root | Out-Null
$dest = Join-Path $root ('{stem}_' + (Get-Date -Format 'yyyyMMdd_HHmmss'))
New-Item -ItemType Directory -Force $dest | Out-Null
$out = & pnputil /export-driver {inf} $dest 2>&1 | Out-String
$files = @(Get-ChildItem $dest -Recurse -File).Count
[PSCustomObject]@{{ export_path=$dest; files=$files; pnputil=($out.Trim()) }} | ConvertTo-Json -Compress"##
    );
    let out = run_ps(&cmd);
    let t = out.trim();
    if t.is_empty() {
        return err_json("no output");
    }
    format!("{{\"tool\":\"export_driver\",\"data\":{t}}}")
}

fn rollback_driver(args: &serde_json::Value) -> String {
    let Some(restore_path) = args
        .get("restore_path")
        .and_then(|v| v.as_str())
        .and_then(|v| sanitize_arg(v, true))
    else {
        return err_json("restore_path is required");
    };
    if !restore_path
        .to_ascii_lowercase()
        .starts_with(r"c:\programdata\mtechdriverstore")
    {
        return err_json("restore_path must be under C:\\ProgramData\\MTechDriverStore");
    }
    let delete_inf = args
        .get("delete_published_name")
        .and_then(|v| v.as_str())
        .and_then(|v| sanitize_arg(v, false));
    log_msg(&format!(
        "[driverstore] rollback_driver restore={restore_path} delete={delete_inf:?}"
    ));
    let delete_block = match &delete_inf {
        Some(inf) => format!("$del = & pnputil /delete-driver {inf} /uninstall /force 2>&1 | Out-String"),
        None => "$del = 'skipped'".to_string(),
    };
    let cmd = format!(
        r##"$ErrorActionPreference='SilentlyContinue'
if(-not (Test-Path '{restore_path}')){{ '{{"error":"restore_path not found"}}'; exit }}
{delete_block}
$add = & pnputil /add-driver '{restore_path}\*.inf' /subdirs /install 2>&1 | Out-String
$reboot = ($add -match 'reboot') -or ($del -match 'reboot')
[PSCustomObject]@{{ delete_output=([string]$del).Trim(); add_output=$add.Trim(); reboot_required=$reboot }} | ConvertTo-Json -Compress"##
    );
    let out = run_ps(&cmd);
    let t = out.trim();
    if t.is_empty() {
        return err_json("no output");
    }
    format!("{{\"tool\":\"rollback_driver\",\"data\":{t}}}")
}

#[unsafe(no_mangle)]
pub extern "C" fn handle_mcp_call(
    tool_ptr: i32,
    tool_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> u64 {
    let tool = unsafe {
        let s = std::slice::from_raw_parts(tool_ptr as *const u8, tool_len as usize);
        std::str::from_utf8(s).unwrap_or("").to_string()
    };
    let args: serde_json::Value = if args_len > 0 {
        let raw = unsafe {
            let s = std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize);
            std::str::from_utf8(s).unwrap_or("{}").to_string()
        };
        serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    let result = match tool.as_str() {
        "snapshot" => snapshot(),
        "list_exports" => list_exports(),
        "export_driver" => export_driver(&args),
        "rollback_driver" => rollback_driver(&args),
        other => err_json(&format!("unknown tool: {other}")),
    };
    set_output(result)
}
