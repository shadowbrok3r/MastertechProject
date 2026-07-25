//! Provider-scoped BSOD / instability event scan for the "Any Recent Blue Screens?" script.
//!
//! Event ID 1001 is shared by `Windows Error Reporting` (user-mode app crashes, very high
//! volume) and `Microsoft-Windows-WER-SystemErrorReporting` (the real "rebooted from a
//! bugcheck" record), so every query here is scoped to an exact provider. Bugcheck codes come
//! from EventData properties, not message text.

pub const DEFAULT_DAYS: u32 = 30;

/// Cap on the WER app-crash count; the report marks a capped count.
const APP_CRASH_CAP: u32 = 1000;

/// What a collected event actually proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    /// `WER-SystemErrorReporting` / `BugCheck` 1001 — the machine bugchecked.
    Bugcheck,
    /// Kernel-Power 41 carrying a non-zero BugcheckCode — crashed, then reset.
    CrashThenReset,
    /// Kernel-Power 41 with BugcheckCode 0 — power pulled or held, no bugcheck.
    PowerLoss,
    /// EventLog 6008 — previous shutdown was unexpected.
    UnexpectedShutdown,
    /// WHEA machine-check (IDs 1/18/20).
    WheaFatal,
    /// WHEA corrected error (IDs 17/19/46/47).
    WheaCorrected,
    /// Any other WHEA-Logger ID.
    WheaOther,
    /// Display 4101 — display driver reset. Not a bugcheck.
    Tdr,
}

impl EventClass {
    pub fn label(self) -> &'static str {
        match self {
            EventClass::Bugcheck => "bugcheck",
            EventClass::CrashThenReset => "crash-then-reset",
            EventClass::PowerLoss => "power loss / forced off",
            EventClass::UnexpectedShutdown => "unexpected shutdown",
            EventClass::WheaFatal => "WHEA fatal",
            EventClass::WheaCorrected => "WHEA corrected",
            EventClass::WheaOther => "WHEA other",
            EventClass::Tdr => "TDR (display driver reset)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BsodEvent {
    pub class: EventClass,
    /// `TimeCreated` in ISO 8601, trimmed to seconds.
    pub time: String,
    pub id: u32,
    pub provider: String,
    /// Decoded bugcheck code where the event carries one.
    pub bugcheck: Option<u32>,
    pub message: String,
}

impl BsodEvent {
    /// `0x0000009f DRIVER_POWER_STATE_FAILURE` for events that carry a code.
    pub fn bugcheck_text(&self) -> Option<String> {
        self.bugcheck
            .map(|c| format!("{:#010x} {}", c, dump_triage::bugcheck::bugcheck_name(c)))
    }
}

#[derive(Debug, Clone)]
pub struct DumpFile {
    pub path: String,
    pub bytes: u64,
    pub time: String,
    pub age_days: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BsodVerdict {
    /// No crash or instability evidence in the window.
    Clean,
    /// Instability short of a bugcheck (power loss, unexpected shutdown, TDR, corrected WHEA).
    Warning,
    /// Bugcheck evidence or a fatal hardware error.
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct BsodScan {
    pub days: u32,
    pub events: Vec<BsodEvent>,
    /// `Windows Error Reporting` 1001 count — user-mode app crashes, reported but never a BSOD.
    pub app_crashes: u32,
    pub app_crashes_capped: bool,
    pub minidump_dir: String,
    pub minidumps: Vec<DumpFile>,
    pub memory_dmp: Option<DumpFile>,
    /// Query failures; a scan that could not read a provider never reads as clean.
    pub query_errors: Vec<String>,
}

impl BsodScan {
    pub fn count(&self, class: EventClass) -> usize {
        self.events.iter().filter(|e| e.class == class).count()
    }

    /// Real bugcheck evidence: bugcheck records plus Kernel-Power 41 with a bugcheck code.
    pub fn bugchecks(&self) -> usize {
        self.count(EventClass::Bugcheck) + self.count(EventClass::CrashThenReset)
    }

    pub fn whea_total(&self) -> usize {
        self.count(EventClass::WheaFatal)
            + self.count(EventClass::WheaCorrected)
            + self.count(EventClass::WheaOther)
    }

    /// Dumps written inside the scan window — authoritative bugcheck evidence.
    pub fn dumps_in_window(&self) -> Vec<&DumpFile> {
        self.minidumps
            .iter()
            .chain(self.memory_dmp.iter())
            .filter(|d| d.age_days <= self.days as f64)
            .collect()
    }

    /// Distinct bugcheck codes seen, most recent first.
    pub fn bugcheck_codes(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for e in &self.events {
            if let Some(c) = e.bugcheck {
                if c != 0 && !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        out
    }

    pub fn verdict(&self) -> BsodVerdict {
        if self.bugchecks() > 0
            || self.count(EventClass::WheaFatal) > 0
            || !self.dumps_in_window().is_empty()
        {
            return BsodVerdict::Error;
        }
        if self.count(EventClass::PowerLoss) > 0
            || self.count(EventClass::UnexpectedShutdown) > 0
            || self.count(EventClass::Tdr) > 0
            || self.whea_total() > 0
            || !self.query_errors.is_empty()
        {
            return BsodVerdict::Warning;
        }
        BsodVerdict::Clean
    }

    /// One-line verdict summary.
    pub fn summary(&self) -> String {
        let bugchecks = self.bugchecks();
        match self.verdict() {
            BsodVerdict::Error => {
                let codes = self.bugcheck_codes();
                let codes_text = if codes.is_empty() {
                    String::new()
                } else {
                    format!(
                        " — {}",
                        codes
                            .iter()
                            .map(|c| format!("{:#010x} {}", c, dump_triage::bugcheck::bugcheck_name(*c)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let dumps = self.dumps_in_window().len();
                format!(
                    "ERROR: {bugchecks} bugcheck event(s), {} fatal WHEA, {dumps} crash dump(s) in the last {} days{codes_text}",
                    self.count(EventClass::WheaFatal),
                    self.days
                )
            }
            BsodVerdict::Warning => format!(
                "WARNING: no bugcheck evidence, but {} unexpected reset(s) with no bugcheck code, {} unexpected shutdown(s), {} TDR(s), {} WHEA event(s) in the last {} days",
                self.count(EventClass::PowerLoss),
                self.count(EventClass::UnexpectedShutdown),
                self.count(EventClass::Tdr),
                self.whea_total(),
                self.days
            ),
            BsodVerdict::Clean => format!(
                "CLEAN: no bugchecks, no unexpected resets, no WHEA, no TDR, no crash dumps in the last {} days",
                self.days
            ),
        }
    }

    /// Human-readable report, one entry per log line.
    pub fn report_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "BSOD scan: last {} days, queried per provider (a bare Event ID 1001 is not a bugcheck)",
            self.days
        ));

        for err in &self.query_errors {
            out.push(format!(
                "WARNING: event query failed — results are INCOMPLETE: {err}"
            ));
        }

        out.push(format!(
            "Real bugchecks: {} (WER-SystemErrorReporting/BugCheck 1001: {}, Kernel-Power 41 with bugcheck code: {})",
            self.bugchecks(),
            self.count(EventClass::Bugcheck),
            self.count(EventClass::CrashThenReset)
        ));
        out.push(format!(
            "Power loss / hard reset (Kernel-Power 41, BugcheckCode 0 — NOT a BSOD): {}",
            self.count(EventClass::PowerLoss)
        ));
        out.push(format!(
            "Unexpected shutdown (EventLog 6008): {}",
            self.count(EventClass::UnexpectedShutdown)
        ));
        out.push(format!(
            "WHEA hardware errors: {} fatal, {} corrected, {} other",
            self.count(EventClass::WheaFatal),
            self.count(EventClass::WheaCorrected),
            self.count(EventClass::WheaOther)
        ));
        out.push(format!(
            "TDR / display driver resets (Display 4101 — a reset, not a bugcheck): {}",
            self.count(EventClass::Tdr)
        ));

        if self.events.is_empty() {
            out.push("No bugcheck / Kernel-Power 41 / 6008 / WHEA / TDR events in the window.".into());
        } else {
            out.push(format!("Events (most recent {} shown):", self.events.len().min(25)));
            for e in self.events.iter().take(25) {
                let code = e
                    .bugcheck_text()
                    .map(|t| format!(" bugcheck={t}"))
                    .unwrap_or_default();
                out.push(format!(
                    "  [{}] {} id={} provider={}{code} :: {}",
                    e.time,
                    e.class.label(),
                    e.id,
                    e.provider,
                    e.message
                ));
            }
        }

        if self.minidumps.is_empty() {
            out.push(format!("No minidumps in {}", self.minidump_dir));
        } else {
            out.push(format!("Recent minidumps in {}:", self.minidump_dir));
            for d in &self.minidumps {
                out.push(format!(
                    "  {}  {} bytes  {:.1} days old  {}",
                    d.time, d.bytes, d.age_days, d.path
                ));
            }
        }
        if let Some(m) = &self.memory_dmp {
            out.push(format!(
                "MEMORY.DMP present: {} bytes, last written {} ({:.1} days old)",
                m.bytes, m.time, m.age_days
            ));
        }

        out.push(format!(
            "User-mode app crash reports (Windows Error Reporting 1001 — NOT BSODs, informational): {}{}",
            self.app_crashes,
            if self.app_crashes_capped { "+ (count capped)" } else { "" }
        ));
        out.push(format!("BSOD verdict: {}", self.summary()));
        out
    }
}

/// PowerShell collector. One JSON object per line so a partial read stays parseable.
/// `__DAYS__` / `__APPCAP__` are substituted by [`ps_script`].
const PS_TEMPLATE: &str = r#"
$ErrorActionPreference = 'Stop'
# UTF-8 pipe so localized event text survives the redirect.
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}
$days = __DAYS__
$start = (Get-Date).AddDays(-$days)
$now = Get-Date
function Emit($o) { Write-Output (ConvertTo-Json -InputObject $o -Compress -Depth 4) }
function Benign($e) {
    return ($e.FullyQualifiedErrorId -like 'NoMatchingEventsFound*' -or $e.FullyQualifiedErrorId -like 'NoMatchingProvidersFound*')
}
$buckets = @(
    @{ Log = 'System'; Provider = 'Microsoft-Windows-WER-SystemErrorReporting'; Ids = @(1001); Max = 50 },
    @{ Log = 'System'; Provider = 'BugCheck';                                   Ids = @(1001); Max = 50 },
    @{ Log = 'System'; Provider = 'Microsoft-Windows-Kernel-Power';             Ids = @(41);   Max = 50 },
    @{ Log = 'System'; Provider = 'EventLog';                                   Ids = @(6008); Max = 50 },
    @{ Log = 'System'; Provider = 'Microsoft-Windows-WHEA-Logger';              Ids = $null;   Max = 50 },
    @{ Log = 'System'; Provider = 'Display';                                    Ids = @(4101); Max = 50 }
)
for ($b = 0; $b -lt $buckets.Count; $b++) {
    $bk = $buckets[$b]
    $filter = @{ LogName = $bk.Log; ProviderName = $bk.Provider; StartTime = $start }
    if ($bk.Ids) { $filter.Id = $bk.Ids }
    try {
        Get-WinEvent -FilterHashtable $filter -MaxEvents $bk.Max -ErrorAction Stop | ForEach-Object {
            $msg = ('' + $_.Message) -split "`r?`n" | Where-Object { $_.Trim() -ne '' } | Select-Object -First 1
            $props = @($_.Properties | Select-Object -First 4 | ForEach-Object { '' + $_.Value })
            Emit ([pscustomobject]@{
                k = 'e'; b = $b; t = $_.TimeCreated.ToString('o'); id = $_.Id
                p = '' + $_.ProviderName; m = ('' + $msg).Trim(); d = $props
            })
        }
    } catch {
        if (-not (Benign $_)) { Emit ([pscustomobject]@{ k = 'err'; msg = ('' + $_.Exception.Message).Trim() }) }
    }
}
$appCap = __APPCAP__
$appCrashes = 0
try {
    $appCrashes = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Windows Error Reporting'; Id = 1001; StartTime = $start } -MaxEvents $appCap -ErrorAction Stop).Count
} catch {
    if (-not (Benign $_)) { Emit ([pscustomobject]@{ k = 'err'; msg = ('' + $_.Exception.Message).Trim() }) }
}
Emit ([pscustomobject]@{ k = 'c'; app_crashes = $appCrashes; capped = ($appCrashes -ge $appCap) })
$minidir = Join-Path $env:SystemRoot 'Minidump'
Emit ([pscustomobject]@{ k = 'dir'; path = $minidir })
try {
    Get-ChildItem -Path $minidir -Filter '*.dmp' -ErrorAction Stop |
        Sort-Object LastWriteTime -Descending | Select-Object -First 10 | ForEach-Object {
            Emit ([pscustomobject]@{ k = 'd'; n = $_.FullName; s = $_.Length; t = $_.LastWriteTime.ToString('o'); a = [math]::Round(($now - $_.LastWriteTime).TotalDays, 2) })
        }
} catch {}
$memdmp = Join-Path $env:SystemRoot 'MEMORY.DMP'
if (Test-Path $memdmp) {
    $f = Get-Item $memdmp
    Emit ([pscustomobject]@{ k = 'm'; n = $f.FullName; s = $f.Length; t = $f.LastWriteTime.ToString('o'); a = [math]::Round(($now - $f.LastWriteTime).TotalDays, 2) })
}
"#;

pub fn ps_script(days: u32) -> String {
    PS_TEMPLATE
        .replace("__DAYS__", &days.to_string())
        .replace("__APPCAP__", &APP_CRASH_CAP.to_string())
}

/// Bucket index, event id and EventData → what the event proves.
fn class_for_bucket(bucket: u32, id: u32, props: &[String]) -> Option<EventClass> {
    match bucket {
        0 | 1 => Some(EventClass::Bugcheck),
        2 => {
            // Kernel-Power 41: property [0] is BugcheckCode; 0 means nothing bugchecked.
            let code = props.first().and_then(|s| parse_code(s)).unwrap_or(0);
            Some(if code != 0 {
                EventClass::CrashThenReset
            } else {
                EventClass::PowerLoss
            })
        }
        3 => Some(EventClass::UnexpectedShutdown),
        4 => Some(match id {
            1 | 18 | 20 => EventClass::WheaFatal,
            17 | 19 | 46 | 47 => EventClass::WheaCorrected,
            _ => EventClass::WheaOther,
        }),
        5 => Some(EventClass::Tdr),
        _ => None,
    }
}

/// Decimal (Kernel-Power BugcheckCode) or `0x…`-prefixed hex, with a trailing
/// parameter list tolerated: `0x0000009f (0x03, …)`.
fn parse_code(raw: &str) -> Option<u32> {
    let token = raw.trim().split([' ', '(', ',']).next()?.trim();
    if token.is_empty() {
        return None;
    }
    if let Some(hex) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    token.parse::<u32>().ok()
}

/// Drop the bidi marks Windows embeds in date/time text (EventLog 6008 carries them).
fn strip_bidi_marks(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}'))
        .collect()
}

/// First `0x`-prefixed hex token in a message — fallback when EventData carries no code.
fn code_from_message(msg: &str) -> Option<u32> {
    msg.split_whitespace()
        .find(|t| t.starts_with("0x") || t.starts_with("0X"))
        .and_then(parse_code)
}

/// Parse the collector's JSON-lines output. Unrecognised lines are ignored.
pub fn parse(days: u32, stdout: &str) -> BsodScan {
    let mut scan = BsodScan { days, ..Default::default() };

    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = v.get("k").and_then(|k| k.as_str()).unwrap_or_default();
        match kind {
            "e" => {
                let bucket = v.get("b").and_then(|b| b.as_u64()).unwrap_or(u64::MAX) as u32;
                let id = v.get("id").and_then(|i| i.as_u64()).unwrap_or_default() as u32;
                let props: Vec<String> = v
                    .get("d")
                    .and_then(|d| d.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|p| p.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let Some(class) = class_for_bucket(bucket, id, &props) else {
                    continue;
                };
                let message =
                    strip_bidi_marks(v.get("m").and_then(|m| m.as_str()).unwrap_or_default());
                let bugcheck = match class {
                    EventClass::CrashThenReset => props.first().and_then(|s| parse_code(s)),
                    EventClass::Bugcheck => props
                        .first()
                        .and_then(|s| parse_code(s))
                        .filter(|c| *c != 0)
                        .or_else(|| code_from_message(&message)),
                    _ => None,
                };
                let time = v
                    .get("t")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(19)
                    .collect();
                scan.events.push(BsodEvent {
                    class,
                    time,
                    id,
                    provider: v.get("p").and_then(|p| p.as_str()).unwrap_or_default().to_string(),
                    bugcheck,
                    message,
                });
            }
            "c" => {
                scan.app_crashes =
                    v.get("app_crashes").and_then(|c| c.as_u64()).unwrap_or_default() as u32;
                scan.app_crashes_capped =
                    v.get("capped").and_then(|c| c.as_bool()).unwrap_or_default();
            }
            "dir" => {
                scan.minidump_dir =
                    v.get("path").and_then(|p| p.as_str()).unwrap_or_default().to_string();
            }
            "d" | "m" => {
                let dump = DumpFile {
                    path: v.get("n").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
                    bytes: v.get("s").and_then(|s| s.as_u64()).unwrap_or_default(),
                    time: v
                        .get("t")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .chars()
                        .take(19)
                        .collect(),
                    age_days: v.get("a").and_then(|a| a.as_f64()).unwrap_or_default(),
                };
                if kind == "d" {
                    scan.minidumps.push(dump);
                } else {
                    scan.memory_dmp = Some(dump);
                }
            }
            "err" => {
                if let Some(msg) = v.get("msg").and_then(|m| m.as_str()) {
                    scan.query_errors.push(msg.to_string());
                }
            }
            _ => {}
        }
    }

    scan.events.sort_by(|a, b| b.time.cmp(&a.time));
    scan
}

const PS_ARGS: [&str; 5] = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"];

/// Run the collector and parse it. Errors only when PowerShell itself could not run.
pub fn scan_blocking(days: u32) -> anyhow::Result<BsodScan> {
    let out = std::process::Command::new("powershell")
        .args(PS_ARGS)
        .arg(ps_script(days))
        .output()?;
    finish(days, &out.stdout, &out.stderr)
}

/// Async twin of [`scan_blocking`] for the remote-script path.
pub async fn scan_async(days: u32) -> anyhow::Result<BsodScan> {
    let out = tokio::process::Command::new("powershell")
        .args(PS_ARGS)
        .arg(ps_script(days))
        .output()
        .await?;
    finish(days, &out.stdout, &out.stderr)
}

fn finish(days: u32, stdout: &[u8], stderr: &[u8]) -> anyhow::Result<BsodScan> {
    let mut scan = parse(days, &String::from_utf8_lossy(stdout));
    let stderr = String::from_utf8_lossy(stderr);
    for line in stderr.lines().filter(|l| !l.trim().is_empty()) {
        scan.query_errors.push(line.trim().to_string());
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WER app-crash flood must not register as a single bugcheck.
    #[test]
    fn wer_app_crashes_are_not_bugchecks() {
        let scan = parse(30, r#"{"k":"c","app_crashes":254,"capped":false}
{"k":"dir","path":"C:\\WINDOWS\\Minidump"}"#);
        assert_eq!(scan.bugchecks(), 0);
        assert_eq!(scan.app_crashes, 254);
        assert_eq!(scan.verdict(), BsodVerdict::Clean);
        assert!(scan.summary().starts_with("CLEAN"));
    }

    #[test]
    fn kernel_power_41_without_bugcheck_code_is_power_loss() {
        let scan = parse(
            30,
            r#"{"k":"e","b":2,"t":"2026-07-24T09:37:23.9225971-06:00","id":41,"p":"Microsoft-Windows-Kernel-Power","m":"The system has rebooted without cleanly shutting down first.","d":["0","0","0","0"]}
{"k":"e","b":3,"t":"2026-07-24T09:37:28.9127063-06:00","id":6008,"p":"EventLog","m":"The previous system shutdown was unexpected.","d":[]}"#,
        );
        assert_eq!(scan.bugchecks(), 0);
        assert_eq!(scan.count(EventClass::PowerLoss), 1);
        assert_eq!(scan.count(EventClass::UnexpectedShutdown), 1);
        assert_eq!(scan.verdict(), BsodVerdict::Warning);
    }

    #[test]
    fn kernel_power_41_with_bugcheck_code_is_a_crash() {
        let scan = parse(
            30,
            r#"{"k":"e","b":2,"t":"2026-07-20T01:02:03.0000000-06:00","id":41,"p":"Microsoft-Windows-Kernel-Power","m":"The system has rebooted without cleanly shutting down first.","d":["159","0","0","0"]}"#,
        );
        assert_eq!(scan.bugchecks(), 1);
        assert_eq!(scan.count(EventClass::CrashThenReset), 1);
        assert_eq!(scan.bugcheck_codes(), vec![0x9F]);
        assert_eq!(scan.verdict(), BsodVerdict::Error);
        assert!(scan.summary().contains("DRIVER_POWER_STATE_FAILURE"));
    }

    #[test]
    fn bugcheck_record_decodes_code_from_properties() {
        let scan = parse(
            30,
            r#"{"k":"e","b":0,"t":"2026-07-19T12:00:00.0000000-06:00","id":1001,"p":"Microsoft-Windows-WER-SystemErrorReporting","m":"The computer has rebooted from a bugcheck.","d":["0x0000001a (0x0000000000041790, 0x000000000000ffff, 0x0000000000000000, 0x0000000000000000)"]}"#,
        );
        assert_eq!(scan.bugchecks(), 1);
        assert_eq!(scan.bugcheck_codes(), vec![0x1A]);
        assert_eq!(
            scan.events[0].bugcheck_text().as_deref(),
            Some("0x0000001a MEMORY_MANAGEMENT")
        );
    }

    /// Locale-independent fallback when EventData is empty.
    #[test]
    fn bugcheck_record_falls_back_to_message_text() {
        let scan = parse(
            30,
            r#"{"k":"e","b":0,"t":"2026-07-19T12:00:00.0000000-06:00","id":1001,"p":"Microsoft-Windows-WER-SystemErrorReporting","m":"The bugcheck was: 0x00000133 (0x0000000000000001).","d":[]}"#,
        );
        assert_eq!(scan.bugcheck_codes(), vec![0x133]);
    }

    #[test]
    fn whea_ids_grade_fatal_versus_corrected() {
        let scan = parse(
            30,
            r#"{"k":"e","b":4,"t":"2026-07-18T00:00:00.0000000-06:00","id":18,"p":"Microsoft-Windows-WHEA-Logger","m":"A fatal hardware error has occurred.","d":[]}
{"k":"e","b":4,"t":"2026-07-17T00:00:00.0000000-06:00","id":47,"p":"Microsoft-Windows-WHEA-Logger","m":"A corrected hardware error has occurred.","d":[]}"#,
        );
        assert_eq!(scan.count(EventClass::WheaFatal), 1);
        assert_eq!(scan.count(EventClass::WheaCorrected), 1);
        assert_eq!(scan.verdict(), BsodVerdict::Error);
    }

    #[test]
    fn tdr_alone_warns_and_is_reported_apart_from_bugchecks() {
        let scan = parse(
            30,
            r#"{"k":"e","b":5,"t":"2026-07-16T00:00:00.0000000-06:00","id":4101,"p":"Display","m":"Display driver nvlddmkm stopped responding and has successfully recovered.","d":[]}"#,
        );
        assert_eq!(scan.bugchecks(), 0);
        assert_eq!(scan.count(EventClass::Tdr), 1);
        assert_eq!(scan.verdict(), BsodVerdict::Warning);
        assert!(scan.report_lines().iter().any(|l| l.contains("TDR / display driver resets")));
    }

    /// A minidump inside the window is bugcheck evidence even with a cleared event log.
    #[test]
    fn recent_minidump_is_error_evidence() {
        let scan = parse(
            30,
            r#"{"k":"dir","path":"C:\\WINDOWS\\Minidump"}
{"k":"d","n":"C:\\WINDOWS\\Minidump\\072026-12345-01.dmp","s":294912,"t":"2026-07-22T10:00:00.0000000-06:00","a":2.1}"#,
        );
        assert_eq!(scan.verdict(), BsodVerdict::Error);
        let old = parse(
            30,
            r#"{"k":"d","n":"C:\\WINDOWS\\Minidump\\010126-12345-01.dmp","s":294912,"t":"2026-01-01T10:00:00.0000000-06:00","a":204.0}"#,
        );
        assert_eq!(old.verdict(), BsodVerdict::Clean);
    }

    #[test]
    fn query_failure_never_reads_as_clean() {
        let scan = parse(30, r#"{"k":"err","msg":"Attempted to perform an unauthorized operation."}"#);
        assert_eq!(scan.verdict(), BsodVerdict::Warning);
        assert!(scan.report_lines().iter().any(|l| l.contains("INCOMPLETE")));
    }

    #[test]
    fn message_bidi_marks_are_stripped() {
        let scan = parse(
            30,
            "{\"k\":\"e\",\"b\":3,\"t\":\"2026-07-24T09:37:28.0000000-06:00\",\"id\":6008,\"p\":\"EventLog\",\"m\":\"The previous system shutdown at 9:36:20 AM on \u{200e}7/\u{200e}24/\u{200e}2026 was unexpected.\",\"d\":[]}",
        );
        assert_eq!(
            scan.events[0].message,
            "The previous system shutdown at 9:36:20 AM on 7/24/2026 was unexpected."
        );
    }

    #[test]
    fn script_substitutes_window_and_cap() {
        let ps = ps_script(14);
        assert!(ps.contains("$days = 14"));
        assert!(ps.contains("$appCap = 1000"));
        assert!(!ps.contains("__DAYS__"));
        // Every event query is provider-scoped.
        assert_eq!(ps.matches("Provider = '").count(), ps.matches("Log = 'System'").count());
    }
}
