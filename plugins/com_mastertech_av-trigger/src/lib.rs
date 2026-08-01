//! Drives on-access antivirus scanning with the EICAR test file, as a
//! repeatable trigger for filesystem-minifilter faults, plus a live read of
//! the registered minifilter stack.

const BUF: usize = 4 * 1024 * 1024;
static mut HEAP: [u8; BUF] = [0; BUF];
static mut HEAP_POS: usize = 0;

const OUT_CAP: i32 = 512 * 1024;

/// Rewind the bump allocator. Safe at the top of a call: the host has already
/// copied out the previous call's returned buffer.
fn heap_reset() {
    unsafe { HEAP_POS = 0 };
}

/// Where the test files are written; `cleanup` removes exactly this directory.
const DROP_DIR: &str = "mtech-avtrigger";

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn host_run_command(cmd_ptr: i32, cmd_len: i32, out_ptr: i32, out_max: i32) -> i32;
}

fn align_up(pos: usize, align: usize) -> usize {
    (pos + align - 1) & !(align - 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe {
        let size = size as usize;
        if size == 0 {
            return 1;
        }
        let p = align_up(HEAP_POS, 16);
        if p + size > BUF {
            return 0;
        }
        HEAP_POS = p + size;
        (&raw mut HEAP).cast::<u8>().add(p) as i32
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dealloc(_ptr: i32, _size: i32) {}

fn leak_bytes(slice: &[u8]) -> u64 {
    unsafe {
        let len = slice.len() as i32;
        let ptr = alloc(len);
        if ptr == 0 {
            return 0;
        }
        std::ptr::copy_nonoverlapping(slice.as_ptr(), ptr as *mut u8, slice.len());
        ((ptr as u64) << 32) | (len as u64 & 0xffff_ffff)
    }
}

fn run(cmd: &str) -> String {
    let out_ptr = alloc(OUT_CAP);
    if out_ptr == 0 {
        return String::from("[error] out buffer alloc failed");
    }
    let n = unsafe { host_run_command(cmd.as_ptr() as i32, cmd.len() as i32, out_ptr, OUT_CAP) };
    let n = n.max(0) as usize;
    unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(out_ptr as *const u8, n))
            .unwrap_or("[error] non-utf8 output")
            .trim()
            .to_string()
    }
}

/// The EICAR anti-malware test string, an inert printable-ASCII sequence every
/// engine detects by signature. Assembled at runtime so the literal never
/// appears contiguously in this artifact and the plugin is not itself
/// quarantined on the way to the client.
fn eicar() -> String {
    let a = "X5O!P%@AP[4\\PZX54(P^)7CC)7}";
    let b = "$EICAR-STANDARD-ANTIVIRUS-";
    let c = "TEST-FILE!$H+H*";
    let mut s = String::with_capacity(68);
    s.push_str(a);
    s.push_str(b);
    s.push_str(c);
    s
}

/// Number parsed out of a flat `{"count":N}` argument object, clamped.
fn arg_count(args: &str, default: i64, max: i64) -> i64 {
    arg_count_named(args, "count", default, max)
}

/// Number parsed out of a flat `{"<key>":N}` argument object, clamped.
fn arg_count_named(args: &str, key: &str, default: i64, max: i64) -> i64 {
    let needle = format!("\"{key}\"");
    let Some(i) = args.find(&needle) else {
        return default;
    };
    let tail = &args[i + needle.len()..];
    let digits: String = tail
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    match digits.parse::<i64>() {
        Ok(n) if n >= 1 => n.min(max),
        _ => default,
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn ok_json(field: &str, body: &str) -> u64 {
    leak_bytes(format!("{{\"{field}\":\"{}\"}}", json_escape(body)).as_bytes())
}

/// Writes `count` copies of the test file, then reports how many the on-access
/// scanner removed. A removal count above zero proves the detection and
/// remediation path ran through the minifilter stack.
fn burst(count: i64) -> String {
    let cmd = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $s='{payload}'; \
         $d=Join-Path $env:TEMP '{dir}'; \
         New-Item -ItemType Directory -Force -Path $d | Out-Null; \
         $w=0; 1..{count} | ForEach-Object {{ \
           Set-Content -LiteralPath (Join-Path $d \"eicar_$_.com\") -Value $s -Encoding Ascii -NoNewline; \
           if ($?) {{ $w++ }} }}; \
         $left={count}; $waited=0; \
         while ($waited -lt 30000 -and $left -gt 0) {{ \
           Start-Sleep -Milliseconds 1000; $waited += 1000; \
           $left=@(Get-ChildItem -LiteralPath $d -Filter 'eicar_*.com').Count \
         }}; \
         \"dir=$d written=$w remaining=$left removed_by_av=$({count}-$left) settle_ms=$waited\"",
        payload = eicar(),
        dir = DROP_DIR,
        count = count
    );
    run(&cmd)
}

/// Opens files back to back and reads their metadata, driving the
/// create-then-query path that runs each minifilter's post-operation callback.
/// This is the shape of TiWorker servicing traffic, not of a file write.
fn metadata_storm(max_files: i64, root: &str) -> String {
    let cmd = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $root='{root}'; $max={max_files}; $n=0; $err=0; \
         $sw=[Diagnostics.Stopwatch]::StartNew(); \
         foreach ($f in [IO.Directory]::EnumerateFiles($root,'*',[IO.SearchOption]::AllDirectories)) {{ \
           if ($n -ge $max) {{ break }} \
           try {{ \
             $h=[IO.File]::Open($f,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite); \
             $null=$h.Length; $h.Close(); \
             $null=[IO.File]::GetAttributes($f); \
             $null=[IO.File]::GetLastWriteTimeUtc($f); \
             $n++ \
           }} catch {{ $err++ }} \
         }}; \
         $sw.Stop(); \
         \"root=$root opened=$n errors=$err elapsed_ms=$($sw.ElapsedMilliseconds)\"",
        root = root,
        max_files = max_files
    );
    run(&cmd)
}

/// Security-service inventory, and optionally restart the ESET engine so its
/// on-access scanning is live again. `action` is status | start | restart.
fn av_service(action: &str) -> String {
    let act = match action {
        "start" => "'== start =='; sc.exe start ekrn 2>&1 | Out-String;",
        "restart" => {
            "'== restart =='; sc.exe stop ekrn 2>&1 | Out-String;              Start-Sleep -Seconds 5; sc.exe start ekrn 2>&1 | Out-String;"
        }
        _ => "",
    };
    // sc.exe rather than Get-Service: enumerating all services blocks for
    // minutes when ESET self-defense is wedged, and named queries do not.
    let cmd = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         '== services =='; \
         foreach ($n in 'ekrn','ehdrv','epfwwfp','epfw','WinDefend','WdNisSvc') {{ \
           $q = (sc.exe query $n 2>&1 | Select-String 'STATE') -join ' '; \
           if (-not $q) {{ $q = 'not present' }}; \
           \"$n : $q\" \
         }}; \
         '== ESET processes =='; \
         (Get-Process ekrn,egui,eguiProxy -ErrorAction SilentlyContinue | \
           Select-Object Name,Id | Format-Table -AutoSize | Out-String); \
         {act}",
        act = act
    );
    run(&cmd)
}

/// Log the detached scanner writes to; `eset_status` tails it.
const SCAN_LOG: &str = "mtech-eset-scan.log";

/// PowerShell that binds `$b` to an ESET CLI binary under the product
/// directory. Restricted to the scanner and command interfaces.
fn eset_bin_expr(bin: &str) -> String {
    let name = match bin {
        "ecmd" => "ecmd.exe",
        "eshell" => "eShell.exe",
        _ => "ecls.exe",
    };
    format!(
        "$b=@(Get-ChildItem -Path 'C:\\Program Files\\ESET','C:\\Program Files (x86)\\ESET' \
         -Recurse -Filter '{name}' -File -ErrorAction SilentlyContinue)[0]; \
         if (-not $b) {{ \"[error] {name} not found\"; exit }}; "
    )
}

/// Inventory of the installed ESET CLI surface plus each binary's own usage
/// text, so argument syntax comes from the product rather than from memory.
fn eset_discover() -> String {
    let cmd = "$ErrorActionPreference='SilentlyContinue'; \
         '== ESET binaries =='; \
         $exes=@(Get-ChildItem -Path 'C:\\Program Files\\ESET','C:\\Program Files (x86)\\ESET' \
           -Recurse -Filter '*.exe' -File | Sort-Object Name); \
         ($exes | Select-Object Name,@{n='ver';e={$_.VersionInfo.FileVersion}},FullName | \
           Format-Table -AutoSize | Out-String); \
         '== ecls usage =='; \
         $c=@($exes | Where-Object Name -eq 'ecls.exe')[0]; \
         if ($c) { (& $c.FullName --help 2>&1 | Out-String) } else { 'ecls.exe not found' }; \
         '== ecmd usage =='; \
         $m=@($exes | Where-Object Name -eq 'ecmd.exe')[0]; \
         if ($m) { (& $m.FullName /? 2>&1 | Out-String) } else { 'ecmd.exe not found' }; \
         '== modules =='; \
         (Get-ChildItem 'C:\\Program Files\\ESET\\ESET Security\\Modules' -Filter '*.dat' | \
           Sort-Object LastWriteTime -Descending | Select-Object -First 8 Name,LastWriteTime,Length | \
           Format-Table -AutoSize | Out-String)";
    run(cmd)
}

/// Invokes an ESET CLI binary. `detach` starts it hidden and returns the pid so
/// a scan that takes the machine down does not also take the call down with it.
fn eset_run(bin: &str, args: &str, detach: bool) -> String {
    let find = eset_bin_expr(bin);
    let body = if detach {
        format!(
            "$log=Join-Path $env:TEMP '{SCAN_LOG}'; \
             Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue; \
             $p=Start-Process -FilePath $b.FullName -ArgumentList '{args}' -WindowStyle Hidden \
               -RedirectStandardOutput $log -PassThru; \
             \"started pid=$($p.Id) bin=$($b.FullName) log=$log args={args}\""
        )
    } else {
        format!(
            "$o=(& $b.FullName {args} 2>&1 | Out-String); \
             \"bin=$($b.FullName) exit=$LASTEXITCODE\"; '== output =='; \
             if ($o.Length -gt 12000) {{ $o.Substring(0,12000) }} else {{ $o }}"
        )
    };
    run(&format!(
        "$ErrorActionPreference='SilentlyContinue'; {find}{body}"
    ))
}

/// Liveness and log tail for a detached scan, plus uptime so a reboot that
/// happened while the scan ran is visible without a separate call.
fn eset_status() -> String {
    let cmd = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $log=Join-Path $env:TEMP '{SCAN_LOG}'; \
         $p=@(Get-Process ecls -ErrorAction SilentlyContinue); \
         $up=[int]((Get-Date)-(Get-CimInstance Win32_OperatingSystem).LastBootUpTime).TotalSeconds; \
         $sz=(Get-Item -LiteralPath $log -ErrorAction SilentlyContinue).Length; \
         \"running=$($p.Count) pids=$(($p|ForEach-Object{{$_.Id}}) -join ',') \
log_bytes=$sz uptime_s=$up\"; \
         '== tail =='; \
         (Get-Content -LiteralPath $log -Tail 40 -ErrorAction SilentlyContinue | Out-String)"
    );
    run(&cmd)
}

/// Defender running mode, exclusions and threat history, plus the exact Win32
/// error raised when opening a dropped test file. Error 225 on that open names
/// an antivirus as the blocker rather than leaving it inferred.
fn av_state() -> String {
    let cmd = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         '== Defender status =='; \
         (Get-MpComputerStatus | Select-Object AMRunningMode,RealTimeProtectionEnabled, \
           AntivirusEnabled,OnAccessProtectionEnabled,IoavProtectionEnabled, \
           BehaviorMonitorEnabled,IsTamperProtected,AntivirusSignatureVersion, \
           AntivirusSignatureLastUpdated | Format-List | Out-String); \
         '== Defender exclusions =='; \
         $p=Get-MpPreference; \
         \"paths   : $($p.ExclusionPath -join ' | ')\"; \
         \"ext     : $($p.ExclusionExtension -join ' | ')\"; \
         \"process : $($p.ExclusionProcess -join ' | ')\"; \
         \"disableRealtime=$($p.DisableRealtimeMonitoring) disableIoav=$($p.DisableIOAVProtection)\"; \
         '== recent detections =='; \
         (Get-MpThreatDetection | Sort-Object InitialDetectionTime -Descending | \
           Select-Object -First 8 ThreatID,InitialDetectionTime,Resources | \
           Format-List | Out-String); \
         '== open probe =='; \
         $d=Join-Path $env:TEMP '{DROP_DIR}'; \
         $f=@(Get-ChildItem -LiteralPath $d -Filter 'eicar_*.com')[0]; \
         if ($f) {{ \
           try {{ \
             $h=[IO.File]::Open($f.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read); \
             $h.Close(); \"open OK len=$($f.Length)\" \
           }} catch {{ \
             $e=$_.Exception; \
             $hr=[Runtime.InteropServices.Marshal]::GetHRForException($e); \
             \"open FAILED win32=$($hr -band 0xffff) hr=0x$('{{0:X8}}' -f $hr) msg=$($e.Message)\" \
           }} \
         }} else {{ 'no test file present' }}",
    );
    run(&cmd)
}

/// Schedules a restart far enough out that this call returns first. A boot
/// cycle is the only way left to re-arm an ESET engine that self-defense will
/// not let us restart, and it is what ran TiWorker servicing before each crash.
fn reboot(delay_secs: i64) -> String {
    let cmd = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         shutdown /r /t {delay_secs} /c 'Mastertech diagnostic reboot' 2>&1 | Out-String; \
         \"scheduled=in {delay_secs}s uptime_before=$([int]((Get-Date) - \
           (Get-CimInstance Win32_OperatingSystem).LastBootUpTime).TotalSeconds)s\""
    );
    run(&cmd)
}

/// Value of a flat `{"root":"..."}` argument, else the servicing-heavy default.
fn arg_root(args: &str) -> String {
    arg_str(args, "root", "C:\\Windows\\System32")
}

/// Value of a flat `{"<key>":"..."}` argument. JSON escapes are decoded, so a
/// `\\` in the wire text reaches the command as one backslash; single quotes
/// are then doubled so the value can sit inside a PowerShell literal.
fn arg_str(args: &str, key: &str, default: &str) -> String {
    let needle = format!("\"{key}\"");
    let Some(i) = args.find(&needle) else {
        return default.to_string();
    };
    let tail = &args[i + needle.len()..];
    let Some(open) = tail.find('"') else {
        return default.to_string();
    };
    let mut out = String::new();
    let mut it = tail[open + 1..].chars();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                return if out.is_empty() {
                    default.to_string()
                } else {
                    out.replace('\'', "''")
                };
            }
            '\\' => match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(esc) => out.push(esc),
                None => break,
            },
            c => out.push(c),
        }
    }
    default.to_string()
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_id() -> u64 {
    leak_bytes(b"com.mastertech.av-trigger")
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_name() -> u64 {
    leak_bytes(b"AV Trigger")
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_version() -> u64 {
    leak_bytes(b"0.1.0")
}

#[unsafe(no_mangle)]
pub extern "C" fn on_load() {}

#[unsafe(no_mangle)]
pub extern "C" fn on_unload() {}

#[unsafe(no_mangle)]
pub extern "C" fn logic() {}

#[unsafe(no_mangle)]
pub extern "C" fn ui_commands() -> u64 {
    leak_bytes(b"[]")
}

#[unsafe(no_mangle)]
pub extern "C" fn mcp_tools() -> u64 {
    leak_bytes(
        br#"[{"name":"filters","description":"List the live filesystem minifilter stack (fltmc filters + fltmc instances) and registered antivirus products. Read-only. Use to identify third-party minifilters before and after a change.","parameters_schema":{"type":"object","properties":{}}},{"name":"drop_eicar","description":"Write ONE EICAR anti-malware test file to a temp directory so on-access scanners engage. EICAR is the standard inert AMTSO test string, not malware. Reports whether the scanner removed it.","parameters_schema":{"type":"object","properties":{}}},{"name":"eicar_burst","description":"Write many EICAR test files back to back to drive sustained detection and quarantine traffic through the minifilter stack. Repeatable BSOD reproduction step: run before a fix to confirm the crash, and again after to confirm it is gone. Args: {count} default 25, max 500.","parameters_schema":{"type":"object","properties":{"count":{"type":"integer","description":"How many test files to write (default 25, max 500)"}}}},{"name":"metadata_storm","description":"Open files back to back and read their metadata, driving the create-then-query path that runs every minifilter post-operation callback. This is the TiWorker servicing I/O shape and the reproduction step for AV_eamonm / FLTMGR post-callback faults - eicar_burst does NOT reproduce those because writes are a different path. Args: {max_files} default 20000 max 200000, {root} default C:\Windows\System32.","parameters_schema":{"type":"object","properties":{"max_files":{"type":"integer","description":"How many files to open (default 20000, max 200000)"},"root":{"type":"string","description":"Directory to walk recursively (default C:\Windows\System32)"}}}},{"name":"av_service","description":"Inspect ESET and Defender service state, and optionally start or restart the ESET engine so on-access scanning is live. An attached-but-inert minifilter neither detects nor faults, so re-arming the engine is the precondition for reproducing a post-callback crash. Args: {action} status | start | restart, default status.","parameters_schema":{"type":"object","properties":{"action":{"type":"string","description":"status | start | restart (default status)"}}}},{"name":"reboot","description":"Schedule a restart of the client. Used to re-arm an antivirus engine that self-defense will not let us restart in place, and to run Windows startup servicing (TiWorker), which is the workload present in the AV_eamonm crash dump. Args: {delay_secs} default 15, max 300.","parameters_schema":{"type":"object","properties":{"delay_secs":{"type":"integer","description":"Seconds before the restart (default 15, max 300)"}}}},{"name":"cleanup","description":"Delete the temp directory used by drop_eicar/eicar_burst and report what remained.","parameters_schema":{"type":"object","properties":{}}},{"name":"eset_discover","description":"Inventory the installed ESET command-line surface (ecls.exe on-demand scanner, ecmd.exe feature control, eShell.exe) with versions, each binary's own usage text, and the newest signature module files. Read-only. Run this before eset_cli so argument syntax comes from the product.","parameters_schema":{"type":"object","properties":{}}},{"name":"eset_cli","description":"Invoke an ESET command-line binary. Drives the scanning engine directly while the on-access minifilter is attached, which is the workload present in the AV_eamonm crash dump and reaches paths that file writes alone do not. Args: {bin} ecls | ecmd | eshell (default ecls), {args} argument string passed through, {mode} detach (default, returns a pid immediately so a crash does not lose the call) or wait.","parameters_schema":{"type":"object","properties":{"bin":{"type":"string","description":"ecls | ecmd | eshell (default ecls)"},"args":{"type":"string","description":"Argument string passed to the binary"},"mode":{"type":"string","description":"detach (default) or wait"}}}},{"name":"eset_status","description":"Liveness, log tail and host uptime for a detached ESET scan started by eset_cli. Poll this instead of blocking a scan call.","parameters_schema":{"type":"object","properties":{}}},{"name":"av_state","description":"Authoritative on-access protection state: Defender running mode (Normal vs Passive vs SxS Passive), real-time and IOAV toggles, exclusion lists, recent threat detections, and the exact Win32 error returned when opening a dropped test file. Win32 225 on that open proves an antivirus is blocking rather than leaving it inferred from a removal count.","parameters_schema":{"type":"object","properties":{}}}]"#,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn handle_mcp_call(
    tool_ptr: i32,
    tool_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> u64 {
    if tool_len <= 0 || tool_ptr <= 0 {
        return leak_bytes(br#"{"error":"bad tool"}"#);
    }
    let tool = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            tool_ptr as *const u8,
            tool_len as usize,
        ))
        .unwrap_or("")
    };
    let args = if args_len > 0 && args_ptr > 0 {
        unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(
                args_ptr as *const u8,
                args_len as usize,
            ))
            .unwrap_or("")
        }
    } else {
        ""
    };
    // Owned before the rewind: both slices point into the heap the host wrote.
    let tool = tool.to_string();
    let args = args.to_string();
    heap_reset();

    match tool.as_str() {
        "filters" => {
            let out = run(
                "$ErrorActionPreference='SilentlyContinue'; '== fltmc filters =='; fltmc filters; ''; '== fltmc instances =='; fltmc instances; ''; '== AV products =='; Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct | Select-Object displayName,productState,pathToSignedProductExe | Format-List | Out-String",
            );
            ok_json("report", &out)
        }
        "drop_eicar" => ok_json("report", &burst(1)),
        "eicar_burst" => ok_json("report", &burst(arg_count(&args, 25, 500))),
        "metadata_storm" => ok_json(
            "report",
            &metadata_storm(arg_count_named(&args, "max_files", 20_000, 200_000), &arg_root(&args)),
        ),
        "av_service" => ok_json("report", &av_service(&arg_str(&args, "action", "status"))),
        "reboot" => ok_json("report", &reboot(arg_count_named(&args, "delay_secs", 15, 300))),
        "eset_discover" => ok_json("report", &eset_discover()),
        "eset_cli" => {
            let bin = arg_str(&args, "bin", "ecls");
            let cli = arg_str(&args, "args", "--help");
            let detach = arg_str(&args, "mode", "detach") != "wait";
            ok_json("report", &eset_run(&bin, &cli, detach))
        }
        "eset_status" => ok_json("report", &eset_status()),
        "av_state" => ok_json("report", &av_state()),
        "cleanup" => {
            let cmd = format!(
                "$ErrorActionPreference='SilentlyContinue'; $d=Join-Path $env:TEMP '{DROP_DIR}'; $left=@(Get-ChildItem -LiteralPath $d -Filter 'eicar_*.com').Count; Remove-Item -LiteralPath $d -Recurse -Force; \"removed_dir=$d files_present_before_delete=$left exists_now=$(Test-Path $d)\""
            );
            ok_json("report", &run(&cmd))
        }
        _ => leak_bytes(br#"{"error":"unknown tool"}"#),
    }
}
