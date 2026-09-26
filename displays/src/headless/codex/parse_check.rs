//! Parse-only check, on the client, of a PowerShell job's script before it is sent for approval.

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

/// Seconds the broker waits for the check job to finish.
pub const WAIT_SECS: u64 = 30;
/// Parse errors spelled out in a refusal; the rest are counted.
const LISTED: usize = 5;
/// Where the target script's base64 goes in [`CHECK_SCRIPT`].
const PLACEHOLDER: &str = "@SCRIPT@";

/// Windows PowerShell 5.1 script that prints the parse errors of the embedded script as a JSON array.
const CHECK_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$source = [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('@SCRIPT@'))
$tokens = $null
$errors = $null
[void][System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
$found = @()
foreach ($e in $errors) {
    if ($null -ne $e) {
        $found += [pscustomobject]@{ line = $e.Extent.StartLineNumber; column = $e.Extent.StartColumnNumber; message = $e.Message }
    }
}
$json = ConvertTo-Json -Compress -InputObject @($found)
[regex]::Replace($json, '[^\x00-\x7F]', { param($m) '\u{0:x4}' -f [int][char]$m.Value })
"#;

/// One error the PowerShell parser reported.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ParseProblem {
    pub line: u64,
    pub column: u64,
    pub message: String,
}

/// A check job for one `remote_exec_start` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    connection_string: String,
    shell: &'static str,
    script: String,
}

impl Check {
    /// The check for a PowerShell job with a script; `None` for cmd, redacted or empty jobs.
    pub fn for_job(arguments: &Value, fallback_connection: &str) -> Option<Self> {
        let shell = powershell_of(arguments)?;
        if arguments.get("redact").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let script = arguments.get("script").and_then(Value::as_str).filter(|s| !s.trim().is_empty())?;
        let connection_string = arguments
            .get("connection_string")
            .and_then(Value::as_str)
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(fallback_connection);
        Some(Self { connection_string: connection_string.to_string(), shell, script: script.to_string() })
    }

    /// `remote_exec_start` arguments of the read-only check job, run by the job's own shell.
    pub fn start_arguments(&self) -> Value {
        json!({
            "connection_string": self.connection_string,
            "script": check_script(&self.script),
            "shell": self.shell,
            "risk": "read",
            "tech": "codex-broker",
            "reason": "Parse-only check of the agent's script before it is sent for approval",
            "timeout_secs": 120,
        })
    }

    /// `remote_exec_wait` arguments for the check job.
    pub fn wait_arguments(&self, job_id: &str) -> Value {
        json!({
            "connection_string": self.connection_string,
            "job_id": job_id,
            "timeout_secs": WAIT_SECS,
            "poll_interval_secs": 1,
        })
    }
}

/// `powershell` or `pwsh` for a job run by PowerShell, with `powershell` as the default.
fn powershell_of(arguments: &Value) -> Option<&'static str> {
    match arguments.get("shell") {
        None | Some(Value::Null) => Some("powershell"),
        Some(Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "powershell" | "ps" | "ps1" => Some("powershell"),
            "pwsh" => Some("pwsh"),
            _ => None,
        },
        Some(_) => None,
    }
}

/// The check script with `script` embedded as UTF-8 base64.
pub fn check_script(script: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());
    CHECK_SCRIPT.replace(PLACEHOLDER, &encoded)
}

/// The job id in a `remote_exec_start` result.
pub fn job_id(start_output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(start_output.trim()).ok()?;
    value.get("job_id").and_then(Value::as_str).map(str::to_string)
}

/// The parse errors in a `remote_exec_wait` result; `None` when it carries no check output.
pub fn problems(wait_output: &str) -> Option<Vec<ParseProblem>> {
    let value: Value = serde_json::from_str(wait_output.trim()).ok()?;
    parse_stdout(value.get("stdout").and_then(Value::as_str)?)
}

/// The parse errors the check printed as its last line of output.
pub fn parse_stdout(stdout: &str) -> Option<Vec<ParseProblem>> {
    let line = stdout
        .lines()
        .rev()
        .map(|l| l.trim_start_matches('\u{feff}').trim())
        .find(|l| !l.is_empty())?;
    let items = match serde_json::from_str::<Value>(line).ok()? {
        Value::Array(items) => items,
        Value::Object(mut wrapped) => match wrapped.remove("value") {
            Some(Value::Array(items)) => items,
            _ => return None,
        },
        _ => return None,
    };
    items.into_iter().map(|item| serde_json::from_value(item).ok()).collect()
}

/// The tool failure sent to the agent for a script that does not parse.
pub fn refusal(problems: &[ParseProblem]) -> String {
    let mut listed: Vec<String> = problems
        .iter()
        .take(LISTED)
        .map(|p| format!("line {}:{} {}", p.line, p.column, p.message.trim()))
        .collect();
    if problems.len() > LISTED {
        listed.push(format!("and {} more", problems.len() - LISTED));
    }
    format!("The script does not parse, so it was not sent for approval: {}", listed.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(line: u64, column: u64, message: &str) -> ParseProblem {
        ParseProblem { line, column, message: message.to_string() }
    }

    #[test]
    fn the_check_job_of_a_state_changing_script_passes_the_read_guard() {
        let job = json!({
            "connection_string": "DESKTOP-3LF8CBD:f075c9a24",
            "script": "icacls C:\\Users\\Owner\\OneDrive /remove:d Everyone\nStart-Process OneDrive.exe\nRemove-Item C:\\x -Recurse",
            "risk": "mutate",
        });
        let check = Check::for_job(&job, "fallback").expect("a PowerShell job gets a check");
        let start = check.start_arguments();
        assert_eq!(start["risk"], "read");
        let script = start["script"].as_str().expect("check script");
        let flagged = crate::plugins::mcp_bridge::state_changing_commands(script);
        assert!(flagged.is_empty(), "the read guard would refuse the check job: {flagged:?}");
    }

    #[test]
    fn the_check_embeds_the_script_as_utf8_base64() {
        let script = "Write-Output 'caf\u{e9}'\n";
        let check = check_script(script);
        let encoded = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());
        assert!(check.contains(&format!("FromBase64String('{encoded}')")), "{check}");
        assert!(!check.contains(PLACEHOLDER));
        assert!(check.contains("[System.Management.Automation.Language.Parser]::ParseInput"));
        assert!(check.contains("ConvertTo-Json -Compress -InputObject @($found)"));
        assert!(check.is_ascii());
    }

    #[test]
    fn powershell_jobs_are_checked_with_their_own_shell() {
        let cs = "PC-1:abc";
        let default = Check::for_job(&json!({ "script": "Get-Date" }), cs).expect("default shell is powershell");
        assert_eq!(default.start_arguments()["shell"], "powershell");
        assert_eq!(default.start_arguments()["connection_string"], cs);
        assert_eq!(default.start_arguments()["risk"], "read");
        let pwsh = Check::for_job(&json!({ "script": "Get-Date", "shell": " PWSH ", "connection_string": "pc-1:abc" }), cs)
            .expect("pwsh is checked");
        assert_eq!(pwsh.start_arguments()["shell"], "pwsh");
        assert_eq!(pwsh.wait_arguments("job-1")["connection_string"], "pc-1:abc");
        assert_eq!(pwsh.wait_arguments("job-1")["timeout_secs"], WAIT_SECS);
        assert!(Check::for_job(&json!({ "script": "dir", "shell": "cmd" }), cs).is_none());
        assert!(Check::for_job(&json!({ "script": "  " }), cs).is_none());
        assert!(Check::for_job(&json!({ "script": "Get-Date", "redact": true }), cs).is_none());
    }

    #[test]
    fn clean_output_reads_as_no_problems() {
        assert_eq!(parse_stdout("[]\r\n"), Some(Vec::new()));
        assert_eq!(parse_stdout("\u{feff}[]"), Some(Vec::new()));
    }

    #[test]
    fn errors_are_read_from_the_last_line() {
        let out = "noise\n[{\"line\":1,\"column\":12,\"message\":\"Missing closing \\u0027}\\u0027 in statement block or type definition.\"}]\n";
        assert_eq!(
            parse_stdout(out),
            Some(vec![problem(1, 12, "Missing closing '}' in statement block or type definition.")])
        );
        let wrapped = r#"{"value":[{"line":2,"column":6,"message":"The string is missing the terminator: '."}],"Count":1}"#;
        assert_eq!(parse_stdout(wrapped), Some(vec![problem(2, 6, "The string is missing the terminator: '.")]));
    }

    #[test]
    fn output_without_a_result_is_not_a_verdict() {
        assert_eq!(parse_stdout(""), None);
        assert_eq!(parse_stdout("Access is denied."), None);
        assert_eq!(parse_stdout("{\"line\":1}"), None);
        assert_eq!(parse_stdout("[{\"line\":\"x\"}]"), None);
        assert_eq!(problems("not json"), None);
        assert_eq!(problems(r#"{"state":"Running","stdout":""}"#), None);
    }

    #[test]
    fn job_results_yield_the_job_id_and_the_problems() {
        assert_eq!(job_id(r#"{"state":"Running","job_id":"job-42"}"#).as_deref(), Some("job-42"));
        assert_eq!(job_id("{}"), None);
        let waited = json!({ "state": "Exited", "exit_code": 0, "stdout": "[{\"line\":3,\"column\":1,\"message\":\"Unexpected token '}'.\"}]\r\n" });
        assert_eq!(problems(&waited.to_string()), Some(vec![problem(3, 1, "Unexpected token '}'.")]));
    }

    #[test]
    fn a_refusal_lists_the_first_errors_and_counts_the_rest() {
        let one = refusal(&[problem(3, 14, "Missing closing '}'. ")]);
        assert_eq!(one, "The script does not parse, so it was not sent for approval: line 3:14 Missing closing '}'.");
        let many: Vec<ParseProblem> = (1..=7).map(|n| problem(n, 1, "Unexpected token")).collect();
        let text = refusal(&many);
        assert!(text.contains("line 1:1 Unexpected token; line 2:1"), "{text}");
        assert!(text.ends_with("; and 2 more"), "{text}");
        assert!(!text.contains("line 6:1"));
    }
}
