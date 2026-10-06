//! Crash artifacts outside the kernel dump folders: WER reports, app dumps and Application Error events.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// App crash artifacts older than this are left out.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_DUMP_SUMMARIES: usize = 25;
const MAX_EVENTS: usize = 100;
const MAX_REPORTS: usize = 50;
/// Distinct apps with memory-corruption crashes that raise the spread flag.
const SPREAD_MIN_APPS: usize = 4;

const WER_ROOT: &str = r"C:\ProgramData\Microsoft\Windows\WER";
const WER_QUEUES: [&str; 2] = ["ReportQueue", "ReportArchive"];
const APPLICATION_LOG: &str = r"C:\Windows\System32\winevt\Logs\Application.evtx";
const SKIP_PROFILES: [&str; 4] = ["public", "default", "default user", "all users"];
const SERVICE_PROFILES: [(&str, &str); 3] = [
    ("SYSTEM", r"C:\Windows\System32\config\systemprofile"),
    ("LocalService", r"C:\Windows\ServiceProfiles\LocalService"),
    (
        "NetworkService",
        r"C:\Windows\ServiceProfiles\NetworkService",
    ),
];
const CORRUPTION_CODES: [&str; 5] = [
    "0xc0000005",
    "0xc0000374",
    "0xc0000409",
    "0xc000001d",
    "0xc0000096",
];
const OS_MODULES: [&str; 9] = [
    "ntdll.dll",
    "kernelbase.dll",
    "kernel32.dll",
    "ucrtbase.dll",
    "msvcrt.dll",
    "combase.dll",
    "user32.dll",
    "win32u.dll",
    "vcruntime140.dll",
];
/// Field order of event 1000 `EventData` on builds that omit the names.
const EVENT_1000_FIELDS: [&str; 12] = [
    "AppName",
    "AppVersion",
    "AppTimeStamp",
    "ModuleName",
    "ModuleVersion",
    "ModuleTimeStamp",
    "ExceptionCode",
    "FaultingOffset",
    "ProcessId",
    "ProcessCreationTime",
    "AppPath",
    "ModulePath",
];

/// One crash artifact on disk and its name inside the archive.
#[derive(Debug, Clone)]
pub struct ArtifactFile {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: SystemTime,
}

impl ArtifactFile {
    fn new(path: PathBuf, name: String) -> Option<Self> {
        let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
        let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());
        Some(Self {
            path,
            name,
            size: meta.len(),
            modified,
        })
    }

    fn is_recent(&self, max_age: Duration) -> bool {
        self.modified.elapsed().map_or(true, |age| age <= max_age)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn starts_with_ci(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Files directly inside `dir` accepted by `keep`, named `<prefix>/<file>`.
fn files_in(dir: &Path, prefix: &str, keep: impl Fn(&Path) -> bool) -> Vec<ArtifactFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| keep(p))
        .filter_map(|p| {
            let name = format!("{prefix}/{}", file_name(&p));
            ArtifactFile::new(p, name)
        })
        .collect()
}

/// Real user profiles under `C:\Users`; junctions and shared profiles are skipped.
fn user_profiles() -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(r"C:\Users") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .filter(|(name, _)| !SKIP_PROFILES.contains(&name.to_ascii_lowercase().as_str()))
        .collect()
}

/// The machine WER root plus each profile's per-user WER root.
fn wer_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(WER_ROOT)];
    roots.extend(
        user_profiles()
            .into_iter()
            .map(|(_, home)| home.join(r"AppData\Local\Microsoft\Windows\WER")),
    );
    roots
}

/// Report folders under every WER queue whose name passes `keep`, with their mtimes.
fn report_dirs(roots: &[PathBuf], keep: impl Fn(&str) -> bool) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for root in roots {
        for queue in WER_QUEUES {
            let Ok(entries) = std::fs::read_dir(root.join(queue)) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().to_string();
                if !keep(&name) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or_else(|_| SystemTime::now());
                out.push((entry.path(), modified));
            }
        }
    }
    out.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    out
}

fn is_app_report(name: &str) -> bool {
    starts_with_ci(name, "AppCrash_") || starts_with_ci(name, "AppHang_")
}

/// Kernel dumps Windows queued under WER instead of LiveKernelReports, named `WER/<folder>/<file>`.
pub fn wer_kernel_dumps() -> Vec<ArtifactFile> {
    report_dirs(&[PathBuf::from(WER_ROOT)], |n| starts_with_ci(n, "Kernel_"))
        .into_iter()
        .flat_map(|(dir, _)| {
            files_in(&dir, &format!("WER/{}", file_name(&dir)), |p| {
                has_ext(p, "dmp")
            })
        })
        .collect()
}

/// Every file of the WER kernel report folders.
pub fn wer_kernel_report_files() -> Vec<ArtifactFile> {
    report_dirs(&[PathBuf::from(WER_ROOT)], |n| starts_with_ci(n, "Kernel_"))
        .into_iter()
        .flat_map(|(dir, _)| files_in(&dir, &format!("WER/{}", file_name(&dir)), |_| true))
        .collect()
}

/// Every file of the app crash and hang report folders newer than `max_age`.
pub fn wer_app_report_files(max_age: Duration) -> Vec<ArtifactFile> {
    report_dirs(&wer_roots(), is_app_report)
        .into_iter()
        .filter(|(_, modified)| modified.elapsed().map_or(true, |age| age <= max_age))
        .flat_map(|(dir, _)| files_in(&dir, &format!("WER/{}", file_name(&dir)), |_| true))
        .collect()
}

/// Folders WER LocalDumps writes app crash dumps to, labelled by owner.
fn app_dump_dirs() -> Vec<(String, PathBuf)> {
    let mut dirs: Vec<(String, PathBuf)> = user_profiles()
        .into_iter()
        .map(|(user, home)| (user, home.join(r"AppData\Local\CrashDumps")))
        .collect();
    dirs.extend(SERVICE_PROFILES.iter().map(|(label, home)| {
        (
            label.to_string(),
            Path::new(home).join(r"AppData\Local\CrashDumps"),
        )
    }));
    dirs.extend(local_dumps_folders());
    let mut seen = HashSet::new();
    dirs.retain(|(_, dir)| dir.is_dir() && seen.insert(dir.to_string_lossy().to_lowercase()));
    dirs
}

/// `DumpFolder` values from the LocalDumps policy key and its per-app subkeys.
#[cfg(target_os = "windows")]
fn local_dumps_folders() -> Vec<(String, PathBuf)> {
    const LOCAL_DUMPS_KEY: &str = r"SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps";
    let Ok(root) = windows_registry::LOCAL_MACHINE.open(LOCAL_DUMPS_KEY) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(dir) = root.get_string("DumpFolder") {
        out.push(("LocalDumps".to_string(), PathBuf::from(expand_env(&dir))));
    }
    let subkeys: Vec<String> = root.keys().map(|k| k.collect()).unwrap_or_default();
    for app in subkeys {
        if let Ok(dir) = root.open(&app).and_then(|k| k.get_string("DumpFolder")) {
            out.push((format!("LocalDumps-{app}"), PathBuf::from(expand_env(&dir))));
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn local_dumps_folders() -> Vec<(String, PathBuf)> {
    Vec::new()
}

/// Expands `%VAR%` references from the process environment; unknown ones stay as written.
fn expand_env(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// App crash dumps newer than `max_age`, newest first, named `AppCrashDumps/<owner>/<file>`.
pub fn app_dumps(max_age: Duration) -> Vec<ArtifactFile> {
    let mut out: Vec<ArtifactFile> = app_dump_dirs()
        .into_iter()
        .flat_map(|(owner, dir)| {
            files_in(&dir, &format!("AppCrashDumps/{owner}"), |p| {
                has_ext(p, "dmp")
            })
        })
        .filter(|f| f.is_recent(max_age))
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.modified));
    out
}

/// One WER app crash or hang report.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WerReport {
    pub folder: String,
    pub event_type: String,
    pub time: Option<String>,
    pub app: Option<String>,
    pub app_version: Option<String>,
    pub module: Option<String>,
    pub module_version: Option<String>,
    pub exception_code: Option<String>,
    pub offset: Option<String>,
    pub app_path: Option<String>,
}

impl WerReport {
    pub fn is_hang(&self) -> bool {
        starts_with_ci(&self.event_type, "AppHang")
    }
}

/// `Report.wer` text: UTF-16LE when it carries the byte-order mark, UTF-8 otherwise.
pub fn decode_wer(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes)
            .trim_start_matches('\u{feff}')
            .to_string(),
    }
}

/// Parses `Report.wer`; signature fields are matched by their `Sig[n].Name`.
pub fn parse_report_wer(text: &str, folder: &str) -> WerReport {
    let fields: HashMap<&str, &str> = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let sigs: HashMap<String, &str> = (0..32)
        .filter_map(|i| {
            let name = fields.get(format!("Sig[{i}].Name").as_str())?;
            let value = fields.get(format!("Sig[{i}].Value").as_str())?;
            Some((name.to_ascii_lowercase(), *value))
        })
        .collect();
    let sig = |name: &str| {
        sigs.get(name)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
    };
    let field = |name: &str| {
        fields
            .get(name)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
    };
    WerReport {
        folder: folder.to_string(),
        event_type: field("EventType").unwrap_or_default(),
        time: field("EventTime")
            .and_then(|t| t.parse().ok())
            .and_then(filetime_to_rfc3339),
        app: sig("application name").or_else(|| field("NsAppName")),
        app_version: sig("application version"),
        module: sig("fault module name"),
        module_version: sig("fault module version"),
        exception_code: sig("exception code").map(|c| hex_code(&c)),
        offset: sig("exception offset")
            .or_else(|| sig("fault offset"))
            .map(|o| hex_offset(&o)),
        app_path: field("AppPath").or_else(|| field("UI[2]")),
    }
}

/// Windows FILETIME (100 ns ticks since 1601) as RFC 3339 UTC.
fn filetime_to_rfc3339(ticks: u64) -> Option<String> {
    const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let secs = i64::try_from(ticks / 10_000_000).ok()? - EPOCH_DIFF_SECS;
    let nanos = u32::try_from((ticks % 10_000_000) * 100).ok()?;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Exception code as lowercase `0x`-prefixed hex.
fn hex_code(raw: &str) -> String {
    let digits = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    format!("0x{}", digits.to_ascii_lowercase())
}

/// Offset as lowercase `0x`-prefixed hex without leading zeros.
fn hex_offset(raw: &str) -> String {
    let digits = raw
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches('0');
    format!(
        "0x{}",
        if digits.is_empty() {
            "0".to_string()
        } else {
            digits.to_ascii_lowercase()
        }
    )
}

/// Parsed `Report.wer` of every app crash and hang report newer than `max_age`, newest first.
pub fn wer_app_reports(max_age: Duration) -> Vec<WerReport> {
    report_dirs(&wer_roots(), is_app_report)
        .into_iter()
        .filter(|(_, modified)| modified.elapsed().map_or(true, |age| age <= max_age))
        .filter_map(|(dir, _)| {
            let bytes = std::fs::read(dir.join("Report.wer")).ok()?;
            Some(parse_report_wer(&decode_wer(&bytes), &file_name(&dir)))
        })
        .collect()
}

/// One Application Error (event 1000) record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppErrorEvent {
    pub time: String,
    pub app: String,
    pub app_version: Option<String>,
    pub module: Option<String>,
    pub module_version: Option<String>,
    pub exception_code: Option<String>,
    pub offset: Option<String>,
    pub app_path: Option<String>,
}

fn json_event_id(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        Value::Object(o) => o
            .get("#text")
            .and_then(|t| t.as_u64().or_else(|| t.as_str()?.parse().ok())),
        _ => None,
    }
}

fn json_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(o) => o.get("#text").and_then(json_text),
        _ => None,
    }
}

/// `EventData` as name to text; unnamed `Data` values map positionally onto event 1000's fields.
fn event_data(data: &Value) -> HashMap<String, String> {
    let Some(obj) = data.as_object() else {
        return HashMap::new();
    };
    let Some(list) = obj.get("Data") else {
        return obj
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), json_text(v)?)))
            .collect();
    };
    let values: Vec<String> = match list {
        Value::Array(items) => items.iter().filter_map(json_text).collect(),
        Value::Object(o) => match o.get("#text") {
            Some(Value::Array(items)) => items.iter().filter_map(json_text).collect(),
            Some(v) => json_text(v).into_iter().collect(),
            None => Vec::new(),
        },
        other => json_text(other).into_iter().collect(),
    };
    EVENT_1000_FIELDS
        .iter()
        .map(|k| k.to_string())
        .zip(values)
        .collect()
}

/// The Application Error record in an evtx JSON event, or `None` for any other event.
pub fn app_error_event(event: &Value) -> Option<AppErrorEvent> {
    let system = event.pointer("/Event/System")?;
    if system.get("EventID").and_then(json_event_id)? != 1000 {
        return None;
    }
    let provider = system
        .pointer("/Provider/#attributes/Name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !provider.eq_ignore_ascii_case("Application Error") {
        return None;
    }
    let time = system
        .pointer("/TimeCreated/#attributes/SystemTime")
        .and_then(Value::as_str)?
        .to_string();
    let data = event_data(event.pointer("/Event/EventData")?);
    let get = |key: &str| data.get(key).filter(|v| !v.is_empty()).cloned();
    Some(AppErrorEvent {
        time,
        app: get("AppName")?,
        app_version: get("AppVersion"),
        module: get("ModuleName"),
        module_version: get("ModuleVersion"),
        exception_code: get("ExceptionCode").map(|c| hex_code(&c)),
        offset: get("FaultingOffset").map(|o| hex_offset(&o)),
        app_path: get("AppPath"),
    })
}

/// Application Error events newer than `max_age` from the Application log, newest first.
#[cfg(target_os = "windows")]
pub fn application_errors(max_age: Duration) -> Result<Vec<AppErrorEvent>, String> {
    const MAX_LOG_BYTES: u64 = 256 * 1024 * 1024;
    let size = std::fs::metadata(APPLICATION_LOG)
        .map_err(|e| e.to_string())?
        .len();
    if size > MAX_LOG_BYTES {
        return Err(format!(
            "Application log is {} MiB; not scanned",
            size / (1024 * 1024)
        ));
    }
    let cutoff = chrono::Utc::now() - chrono::Duration::from_std(max_age).unwrap_or_default();
    let mut parser = evtx::EvtxParser::from_path(APPLICATION_LOG).map_err(|e| e.to_string())?;
    let mut events: Vec<AppErrorEvent> = parser
        .records_json_value()
        .filter_map(Result::ok)
        .filter_map(|record| app_error_event(&record.data))
        .filter(|e| chrono::DateTime::parse_from_rfc3339(&e.time).is_ok_and(|t| t >= cutoff))
        .collect();
    events.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(events)
}

#[cfg(not(target_os = "windows"))]
pub fn application_errors(_max_age: Duration) -> Result<Vec<AppErrorEvent>, String> {
    Err(format!("{APPLICATION_LOG} is Windows-only"))
}

/// Exception and faulting module of one app crash dump.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AppDumpSummary {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: Option<String>,
    pub process: Option<String>,
    pub exception_code: Option<String>,
    pub exception_address: Option<String>,
    pub module: Option<String>,
    pub module_offset: Option<String>,
    pub module_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn base_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

#[cfg(target_os = "windows")]
pub fn summarize_app_dump(file: &ArtifactFile) -> AppDumpSummary {
    use minidump::{Minidump, MinidumpException, MinidumpModuleList, Module};

    let mut summary = AppDumpSummary {
        name: file.name.clone(),
        path: file.path.to_string_lossy().to_string(),
        size: file.size,
        modified: Some(
            chrono::DateTime::<chrono::Utc>::from(file.modified)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ),
        ..Default::default()
    };
    let dump = match Minidump::read_path(&file.path) {
        Ok(dump) => dump,
        Err(e) => {
            summary.error = Some(e.to_string());
            return summary;
        }
    };
    let modules = dump.get_stream::<MinidumpModuleList>().ok();
    summary.process = modules
        .as_ref()
        .and_then(|m| m.main_module())
        .map(|m| base_name(&m.code_file()));
    match dump.get_stream::<MinidumpException>() {
        Ok(exception) => {
            let record = &exception.raw.exception_record;
            let address = record.exception_address;
            summary.exception_code = Some(format!("0x{:08x}", record.exception_code));
            summary.exception_address = Some(format!("0x{address:x}"));
            if let Some(module) = modules.as_ref().and_then(|m| m.module_at_address(address)) {
                summary.module = Some(base_name(&module.code_file()));
                summary.module_offset = Some(format!("0x{:x}", address - module.base_address()));
                summary.module_version = module.version().map(|v| v.to_string());
            }
        }
        Err(e) => summary.error = Some(format!("no exception stream: {e}")),
    }
    summary
}

#[cfg(not(target_os = "windows"))]
pub fn summarize_app_dump(file: &ArtifactFile) -> AppDumpSummary {
    AppDumpSummary {
        name: file.name.clone(),
        path: file.path.to_string_lossy().to_string(),
        size: file.size,
        error: Some("app dump parsing is Windows-only".to_string()),
        ..Default::default()
    }
}

/// One app crash reduced to what the summary counts.
#[derive(Debug, Clone, PartialEq)]
pub struct CrashRecord {
    pub time: Option<String>,
    pub app: String,
    pub module: Option<String>,
    pub exception_code: Option<String>,
}

impl From<&AppErrorEvent> for CrashRecord {
    fn from(e: &AppErrorEvent) -> Self {
        Self {
            time: Some(e.time.clone()),
            app: e.app.clone(),
            module: e.module.clone(),
            exception_code: e.exception_code.clone(),
        }
    }
}

impl CrashRecord {
    fn from_report(r: &WerReport) -> Option<Self> {
        Some(Self {
            time: r.time.clone(),
            app: r.app.clone()?,
            module: r.module.clone(),
            exception_code: r.exception_code.clone(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Count {
    pub name: String,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
}

/// Memory-corruption crashes across unrelated apps.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CorruptionSpread {
    pub flag: bool,
    pub apps: Vec<String>,
    pub shared_module: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AppCrashSummary {
    pub window_days: u64,
    pub source: &'static str,
    pub crashes: usize,
    pub hangs: usize,
    pub distinct_apps: usize,
    pub by_app: Vec<Count>,
    pub by_exception: Vec<Count>,
    pub by_module: Vec<Count>,
    pub corruption_spread: CorruptionSpread,
}

fn exception_name(code: &str) -> &'static str {
    match code {
        "0xc0000005" => "access violation",
        "0xc0000374" => "heap corruption",
        "0xc0000409" => "fail-fast",
        "0xc000001d" => "illegal instruction",
        "0xc0000096" => "privileged instruction",
        "0xc00000fd" => "stack overflow",
        "0xe0434352" => ".NET exception",
        "0x80000003" => "breakpoint",
        _ => "",
    }
}

/// Counts by `key`, case-insensitive, highest first, at most `limit` rows.
fn count_by(
    crashes: &[CrashRecord],
    key: impl Fn(&CrashRecord) -> Option<String>,
    limit: usize,
) -> Vec<Count> {
    let mut rows: Vec<Count> = Vec::new();
    for crash in crashes {
        let Some(name) = key(crash) else { continue };
        match rows.iter_mut().find(|r| r.name.eq_ignore_ascii_case(&name)) {
            Some(row) => {
                row.count += 1;
                if crash.time > row.last {
                    row.last = crash.time.clone();
                }
            }
            None => rows.push(Count {
                name,
                count: 1,
                last: crash.time.clone(),
            }),
        }
    }
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| b.last.cmp(&a.last)));
    rows.truncate(limit);
    rows
}

/// A non-OS fault module behind at least half of `crashes` across two or more apps.
fn shared_module(crashes: &[CrashRecord]) -> Option<String> {
    let mut apps_by_module: HashMap<String, (HashSet<String>, usize)> = HashMap::new();
    for crash in crashes {
        let Some(module) = crash.module.as_deref().map(str::to_ascii_lowercase) else {
            continue;
        };
        if OS_MODULES.contains(&module.as_str()) || module == crash.app.to_ascii_lowercase() {
            continue;
        }
        let entry = apps_by_module.entry(module).or_default();
        entry.0.insert(crash.app.to_ascii_lowercase());
        entry.1 += 1;
    }
    apps_by_module
        .into_iter()
        .filter(|(_, (apps, hits))| apps.len() >= 2 && hits * 2 >= crashes.len())
        .max_by_key(|(_, (_, hits))| *hits)
        .map(|(module, _)| module)
}

fn corruption_spread(crashes: &[CrashRecord]) -> CorruptionSpread {
    let corrupt: Vec<CrashRecord> = crashes
        .iter()
        .filter(|c| {
            c.exception_code
                .as_deref()
                .is_some_and(|code| CORRUPTION_CODES.contains(&code))
        })
        .cloned()
        .collect();
    let mut apps: Vec<String> = Vec::new();
    for crash in &corrupt {
        if !apps.iter().any(|a| a.eq_ignore_ascii_case(&crash.app)) {
            apps.push(crash.app.clone());
        }
    }
    let shared = shared_module(&corrupt);
    let flag = apps.len() >= SPREAD_MIN_APPS && shared.is_none();
    let note = match (&shared, flag) {
        (Some(module), _) => format!(
            "Memory-corruption crashes in {} apps share the fault module {module}; suspect that module before hardware.",
            apps.len()
        ),
        (None, true) => format!(
            "{} unrelated apps crashed with memory-corruption codes and no shared fault module: consistent with CPU or RAM instability.",
            apps.len()
        ),
        (None, false) => String::new(),
    };
    CorruptionSpread {
        flag,
        apps,
        shared_module: shared,
        note,
    }
}

pub fn summarize(crashes: &[CrashRecord], hangs: usize, source: &'static str) -> AppCrashSummary {
    let mut distinct: HashSet<String> = HashSet::new();
    for crash in crashes {
        distinct.insert(crash.app.to_ascii_lowercase());
    }
    AppCrashSummary {
        window_days: MAX_AGE.as_secs() / 86_400,
        source,
        crashes: crashes.len(),
        hangs,
        distinct_apps: distinct.len(),
        by_app: count_by(crashes, |c| Some(c.app.clone()), 15),
        by_exception: count_by(
            crashes,
            |c| {
                c.exception_code
                    .as_deref()
                    .map(|code| match exception_name(code) {
                        "" => code.to_string(),
                        name => format!("{code} {name}"),
                    })
            },
            10,
        ),
        by_module: count_by(crashes, |c| c.module.clone(), 10),
        corruption_spread: corruption_spread(crashes),
    }
}

/// App crash evidence: summary, recent events, WER reports and dump summaries.
pub fn app_crash_report() -> Value {
    let (events, event_log_error) = match application_errors(MAX_AGE) {
        Ok(events) => (events, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let reports = wer_app_reports(MAX_AGE);
    let hangs = reports.iter().filter(|r| r.is_hang()).count();
    let (records, source): (Vec<CrashRecord>, &'static str) = if events.is_empty() {
        (
            reports
                .iter()
                .filter(|r| !r.is_hang())
                .filter_map(CrashRecord::from_report)
                .collect(),
            "wer_reports",
        )
    } else {
        (events.iter().map(CrashRecord::from).collect(), "event_log")
    };
    let dumps = app_dumps(MAX_AGE);
    let dump_summaries: Vec<AppDumpSummary> = dumps
        .iter()
        .take(MAX_DUMP_SUMMARIES)
        .map(summarize_app_dump)
        .collect();
    json!({
        "summary": summarize(&records, hangs, source),
        "events": events.iter().take(MAX_EVENTS).collect::<Vec<_>>(),
        "event_log_error": event_log_error,
        "wer_reports": reports.iter().take(MAX_REPORTS).collect::<Vec<_>>(),
        "dumps": dump_summaries,
        "dump_count": dumps.len(),
    })
}

/// Per-file and total byte limits for one group of archive entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub file: u64,
    pub total: u64,
}

/// Per-group archive limits for a streamed or relayed transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLimits {
    pub wer_kernel: Limit,
    pub app_dumps: Limit,
    pub wer_app: Limit,
}

impl ZipLimits {
    const MIB: u64 = 1024 * 1024;

    pub fn for_transport(streamed: bool) -> Self {
        let m = Self::MIB;
        if streamed {
            Self {
                wer_kernel: Limit {
                    file: 1024 * m,
                    total: 1024 * m,
                },
                app_dumps: Limit {
                    file: 768 * m,
                    total: 1536 * m,
                },
                wer_app: Limit {
                    file: 32 * m,
                    total: 256 * m,
                },
            }
        } else {
            Self {
                wer_kernel: Limit {
                    file: 128 * m,
                    total: 128 * m,
                },
                app_dumps: Limit {
                    file: 64 * m,
                    total: 128 * m,
                },
                wer_app: Limit {
                    file: 8 * m,
                    total: 32 * m,
                },
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ManifestEntry {
    pub name: String,
    pub source: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedEntry {
    pub source: String,
    pub size: u64,
    pub reason: &'static str,
}

/// What an archive holds and what it left out.
#[derive(Debug, Default, Serialize)]
pub struct Manifest {
    pub included: Vec<ManifestEntry>,
    pub skipped: Vec<SkippedEntry>,
}

/// Files that fit `limit`, in order, with the rest marked as skipped.
pub fn select(
    files: Vec<ArtifactFile>,
    limit: Limit,
    manifest: &mut Manifest,
) -> Vec<ArtifactFile> {
    let mut used = 0u64;
    let mut keep = Vec::new();
    for file in files {
        let reason = if file.size > limit.file {
            Some("larger than the per-file limit")
        } else if used + file.size > limit.total {
            Some("over the group's total limit")
        } else {
            None
        };
        match reason {
            Some(reason) => manifest.skipped.push(SkippedEntry {
                source: file.path.to_string_lossy().to_string(),
                size: file.size,
                reason,
            }),
            None => {
                used += file.size;
                keep.push(file);
            }
        }
    }
    keep
}

/// Streams `files` into `zip` and records each one in `manifest`.
pub fn add_to_zip<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
    files: &[ArtifactFile],
    manifest: &mut Manifest,
    added: &mut u32,
) {
    for file in files {
        let Ok(mut source) = std::fs::File::open(&file.path) else {
            manifest.skipped.push(SkippedEntry {
                source: file.path.to_string_lossy().to_string(),
                size: file.size,
                reason: "unreadable",
            });
            continue;
        };
        if zip.start_file(file.name.as_str(), options).is_ok()
            && std::io::copy(&mut source, zip).is_ok()
        {
            *added += 1;
            manifest.included.push(ManifestEntry {
                name: file.name.clone(),
                source: file.path.to_string_lossy().to_string(),
                size: file.size,
            });
        }
    }
}

/// Writes `value` as a pretty JSON entry named `name`.
pub fn add_json<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
    name: &str,
    value: &impl Serialize,
) -> bool {
    zip.start_file(name, options).is_ok()
        && serde_json::to_vec_pretty(value).is_ok_and(|bytes| zip.write_all(&bytes).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_with_bom(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    const REPORT: &str = "Version=1\r\nEventType=APPCRASH\r\nEventTime=134355457052798380\r\n\
        Sig[0].Name=Application Name\r\nSig[0].Value=MasterTech.exe\r\n\
        Sig[1].Name=Application Version\r\nSig[1].Value=4.8.9.0\r\n\
        Sig[3].Name=Fault Module Name\r\nSig[3].Value=MasterTech.exe\r\n\
        Sig[4].Name=Fault Module Version\r\nSig[4].Value=4.8.9.0\r\n\
        Sig[6].Name=Exception Code\r\nSig[6].Value=C0000005\r\n\
        Sig[7].Name=Exception Offset\r\nSig[7].Value=0000000004691012\r\n\
        UI[2]=C:\\Tools\\MasterTech.exe\r\n";

    #[test]
    fn report_wer_decodes_utf16_and_maps_signature_names() {
        let report = parse_report_wer(
            &decode_wer(&utf16_with_bom(REPORT)),
            "AppCrash_MasterTech.exe_1",
        );
        assert_eq!(report.event_type, "APPCRASH");
        assert_eq!(report.app.as_deref(), Some("MasterTech.exe"));
        assert_eq!(report.app_version.as_deref(), Some("4.8.9.0"));
        assert_eq!(report.module.as_deref(), Some("MasterTech.exe"));
        assert_eq!(report.exception_code.as_deref(), Some("0xc0000005"));
        assert_eq!(report.offset.as_deref(), Some("0x4691012"));
        assert_eq!(
            report.app_path.as_deref(),
            Some("C:\\Tools\\MasterTech.exe")
        );
        assert_eq!(report.time.as_deref(), Some("2026-10-04T00:01:45Z"));
        assert!(!report.is_hang());
    }

    #[test]
    fn report_wer_reads_bex64_order_and_hangs() {
        let bex = "EventType=BEX64\nSig[0].Name=Application Name\nSig[0].Value=game.exe\n\
                   Sig[6].Name=Exception Offset\nSig[6].Value=00000000000a1b2c\n\
                   Sig[7].Name=Exception Code\nSig[7].Value=c0000409\n";
        let report = parse_report_wer(bex, "AppCrash_game.exe_2");
        assert_eq!(report.exception_code.as_deref(), Some("0xc0000409"));
        assert_eq!(report.offset.as_deref(), Some("0xa1b2c"));

        let hang = parse_report_wer(
            "EventType=AppHangB1\nSig[0].Name=Application Name\nSig[0].Value=x.exe\n",
            "AppHang_x",
        );
        assert!(hang.is_hang());
        assert_eq!(hang.exception_code, None);
    }

    #[test]
    fn utf8_report_is_read_as_is() {
        let report = parse_report_wer(&decode_wer(b"\xEF\xBB\xBFEventType=APPCRASH\n"), "f");
        assert_eq!(report.event_type, "APPCRASH");
    }

    fn event(event_data: Value) -> Value {
        json!({ "Event": {
            "System": {
                "Provider": { "#attributes": { "Name": "Application Error" } },
                "EventID": 1000,
                "TimeCreated": { "#attributes": { "SystemTime": "2026-10-05T20:58:02.123Z" } }
            },
            "EventData": event_data
        }})
    }

    #[test]
    fn named_event_data_is_read() {
        let e = app_error_event(&event(json!({
            "AppName": "MasterTech.exe", "AppVersion": "4.8.9.0", "ModuleName": "MasterTech.exe",
            "ExceptionCode": "c0000005", "FaultingOffset": "0000000004691012", "AppPath": "C:\\Tools\\MasterTech.exe"
        })))
        .expect("event 1000");
        assert_eq!(e.app, "MasterTech.exe");
        assert_eq!(e.exception_code.as_deref(), Some("0xc0000005"));
        assert_eq!(e.offset.as_deref(), Some("0x4691012"));
        assert_eq!(e.time, "2026-10-05T20:58:02.123Z");
    }

    #[test]
    fn unnamed_event_data_maps_positionally() {
        let e = app_error_event(&event(json!({ "Data": [
            "game.exe", "1.0", "aa", "oo2core_9_win64.dll", "2.9", "bb", "c0000005", "00000000000123ab"
        ]})))
        .expect("event 1000");
        assert_eq!(e.app, "game.exe");
        assert_eq!(e.module.as_deref(), Some("oo2core_9_win64.dll"));
        assert_eq!(e.offset.as_deref(), Some("0x123ab"));
    }

    #[test]
    fn other_events_are_ignored() {
        let mut other = event(json!({ "AppName": "x.exe" }));
        other["Event"]["System"]["EventID"] = json!({ "#text": 1001 });
        assert!(app_error_event(&other).is_none());
        let mut hang = event(json!({ "AppName": "x.exe" }));
        hang["Event"]["System"]["Provider"]["#attributes"]["Name"] = json!("Application Hang");
        assert!(app_error_event(&hang).is_none());
    }

    fn crash(app: &str, module: &str, code: &str) -> CrashRecord {
        CrashRecord {
            time: Some("2026-10-04T01:33:00Z".to_string()),
            app: app.to_string(),
            module: Some(module.to_string()),
            exception_code: Some(code.to_string()),
        }
    }

    #[test]
    fn unrelated_apps_with_corruption_raise_the_flag() {
        let crashes = vec![
            crash("TiWorker.exe", "cbscore.dll", "0xc0000005"),
            crash("MsMpEng.exe", "mpengine.dll", "0xc0000005"),
            crash("mbamservice.exe", "ntdll.dll", "0xc0000374"),
            crash("OCCT.exe", "OCCT.exe", "0xc0000005"),
            crash("chrome.exe", "chrome.dll", "0x80000003"),
        ];
        let summary = summarize(&crashes, 0, "event_log");
        assert_eq!(summary.distinct_apps, 5);
        assert!(summary.corruption_spread.flag);
        assert_eq!(summary.corruption_spread.apps.len(), 4);
        assert_eq!(summary.corruption_spread.shared_module, None);
        assert_eq!(summary.by_exception[0].name, "0xc0000005 access violation");
        assert_eq!(summary.by_exception[0].count, 3);
    }

    #[test]
    fn a_shared_injected_module_points_away_from_hardware() {
        let crashes = vec![
            crash("a.exe", "overlay64.dll", "0xc0000005"),
            crash("b.exe", "overlay64.dll", "0xc0000005"),
            crash("c.exe", "overlay64.dll", "0xc0000005"),
            crash("d.exe", "ntdll.dll", "0xc0000374"),
        ];
        let spread = summarize(&crashes, 0, "event_log").corruption_spread;
        assert!(!spread.flag);
        assert_eq!(spread.shared_module.as_deref(), Some("overlay64.dll"));
        assert!(spread.note.contains("overlay64.dll"));
    }

    #[test]
    fn few_apps_do_not_raise_the_flag() {
        let crashes = vec![
            crash("a.exe", "a.exe", "0xc0000005"),
            crash("a.exe", "a.exe", "0xc0000005"),
            crash("b.exe", "b.exe", "0xc0000005"),
        ];
        let spread = summarize(&crashes, 0, "event_log").corruption_spread;
        assert!(!spread.flag);
        assert!(spread.note.is_empty());
    }

    fn file(name: &str, size: u64) -> ArtifactFile {
        ArtifactFile {
            path: PathBuf::from(name),
            name: name.to_string(),
            size,
            modified: SystemTime::now(),
        }
    }

    #[test]
    fn select_respects_file_and_total_limits() {
        let mut manifest = Manifest::default();
        let kept = select(
            vec![file("a", 40), file("b", 200), file("c", 50), file("d", 20)],
            Limit {
                file: 100,
                total: 100,
            },
            &mut manifest,
        );
        let names: Vec<&str> = kept.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
        assert_eq!(manifest.skipped.len(), 2);
        assert_eq!(manifest.skipped[0].reason, "larger than the per-file limit");
        assert_eq!(manifest.skipped[1].reason, "over the group's total limit");
    }

    #[test]
    fn expand_env_keeps_unknown_names() {
        let path = std::env::var("PATH").expect("PATH is set");
        assert_eq!(
            expand_env("%PATH%\\CrashDumps"),
            format!("{path}\\CrashDumps")
        );
        assert_eq!(
            expand_env("%MTECH_NO_SUCH_VAR%\\x"),
            "%MTECH_NO_SUCH_VAR%\\x"
        );
        assert_eq!(expand_env("100%"), "100%");
    }

    #[test]
    fn hex_helpers_normalize() {
        assert_eq!(hex_code("C0000005"), "0xc0000005");
        assert_eq!(hex_code("0xE0434352"), "0xe0434352");
        assert_eq!(hex_offset("0000000000000000"), "0x0");
        assert_eq!(hex_offset("00000000001ebefe"), "0x1ebefe");
    }
}
