//! ZeroClaw gateway client: agents, their sessions and transcripts, and scheduled automations.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use database::schema::ZeroclawGateway;
use serde::Deserialize;
use serde_json::Value;

/// Largest response body read from the gateway.
const BODY_MAX: usize = 24 * 1024 * 1024;
/// Longest output kept from one run.
const OUTPUT_MAX: usize = 16 * 1024;
/// Stored messages a transcript shows, the newest ones.
const TRANSCRIPT_KEEP: usize = 200;
const RUNS_MAX: usize = 100;
/// The gateway runs the job before it answers `POST /api/cron/{id}/run`.
const RUN_NOW_TIMEOUT: Duration = Duration::from_secs(600);
/// Prefix of the sessions an automation delivers its runs into.
const AUTOMATION_SESSION: &str = "cron_";

fn client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c);
    }
    let c = reqwest::Client::builder()
        .user_agent("mastertech-zeroclaw-viewer")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let _ = CLIENT.set(c);
    CLIENT.get().ok_or_else(|| anyhow!("HTTP client unavailable"))
}

async fn call(gw: &ZeroclawGateway, method: reqwest::Method, path: &str, timeout: Option<Duration>) -> Result<Value> {
    let url = format!("{}{path}", gw.url.trim_end_matches('/'));
    let mut request = client()?.request(method, url).bearer_auth(&gw.token);
    if let Some(timeout) = timeout {
        request = request.timeout(timeout);
    }
    let mut response = request.send().await?;
    let status = response.status();
    let limit = if status.is_success() { BODY_MAX } else { 4096 };
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > limit {
            return Err(anyhow!("response too large from {path}"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let why = body.get("error").and_then(Value::as_str).map(|m| format!(": {}", m.chars().take(400).collect::<String>()));
        return Err(anyhow!("HTTP {} from {path}{}", status.as_u16(), why.unwrap_or_default()));
    }
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {path}"))
}

async fn get(gw: &ZeroclawGateway, path: &str) -> Result<Value> {
    call(gw, reqwest::Method::GET, path, None).await
}

/// One path component, percent-encoded.
fn component(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Agent aliases the gateway is configured with.
pub async fn agents(gw: &ZeroclawGateway) -> Result<Vec<String>> {
    Ok(parse_agents(&get(gw, "/api/config/agent-options").await?))
}

fn parse_agents(v: &Value) -> Vec<String> {
    v.get("agents")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// One conversation as the session list shows it; `keys` are every store the gateway holds under its id.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub agent: String,
    pub name: String,
    pub messages: u64,
    pub last_activity: String,
    pub keys: Vec<String>,
}

impl SessionRow {
    /// A session an automation writes its results into.
    pub fn is_automation(&self) -> bool {
        self.id.starts_with(AUTOMATION_SESSION)
    }

    pub fn label(&self) -> &str {
        if self.name.is_empty() { &self.id } else { &self.name }
    }
}

/// Every non-empty session, newest first, with twin stores under one id merged into one row.
pub async fn sessions(gw: &ZeroclawGateway) -> Result<Vec<SessionRow>> {
    Ok(parse_sessions(&get(gw, "/api/sessions").await?))
}

fn parse_sessions(v: &Value) -> Vec<SessionRow> {
    let mut rows: Vec<SessionRow> = Vec::new();
    for s in v.get("sessions").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = s.get("session_id").and_then(Value::as_str) else { continue };
        let count = s.get("message_count").and_then(Value::as_u64).unwrap_or(0);
        if count == 0 {
            continue;
        }
        let agent = s.get("agent_alias").and_then(Value::as_str).unwrap_or_default().to_string();
        let at = s.get("last_activity").and_then(Value::as_str).unwrap_or_default().to_string();
        let name = s.get("name").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
        let key = s.get("session_key").and_then(Value::as_str).unwrap_or(id).to_string();
        match rows.iter_mut().find(|r| r.id == id && r.agent == agent) {
            Some(r) => {
                r.messages += count;
                if at > r.last_activity {
                    r.last_activity = at;
                }
                if r.name.is_empty() {
                    r.name = name;
                }
                if !r.keys.contains(&key) {
                    r.keys.push(key);
                }
            }
            None => rows.push(SessionRow { id: id.to_string(), agent, name, messages: count, last_activity: at, keys: vec![key] }),
        }
    }
    rows.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    rows
}

/// Ids of the sessions with a turn running now.
pub async fn running(gw: &ZeroclawGateway) -> Result<Vec<String>> {
    Ok(parse_running(&get(gw, "/api/sessions/running").await?))
}

fn parse_running(v: &Value) -> Vec<String> {
    v.get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.get("session_id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// One transcript row read back from a stored message.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User { text: String, at: String },
    Agent { text: String, at: String },
    Reasoning(String),
    ToolCall { name: String, arguments: Value },
    ToolOutput(String),
}

/// A session's stored messages from every store it has, oldest first, as transcript rows.
pub async fn transcript(gw: &ZeroclawGateway, row: &SessionRow) -> Result<Vec<Item>> {
    let keys: Vec<&str> = if row.keys.is_empty() { vec![row.id.as_str()] } else { row.keys.iter().map(String::as_str).collect() };
    let mut messages: Vec<Value> = Vec::new();
    let mut failed = None;
    for key in keys {
        match get(gw, &format!("/api/sessions/{}/messages", component(key))).await {
            Ok(v) => messages.extend(v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default()),
            Err(e) => failed = Some(e),
        }
    }
    if let (true, Some(e)) = (messages.is_empty(), failed) {
        return Err(e);
    }
    messages.sort_by(|a, b| stamp(a).cmp(stamp(b)));
    Ok(parse_transcript(&messages))
}

fn stamp(m: &Value) -> &str {
    m.get("created_at").and_then(Value::as_str).unwrap_or("")
}

fn parse_transcript(messages: &[Value]) -> Vec<Item> {
    let mut out = Vec::new();
    for m in messages.iter().skip(messages.len().saturating_sub(TRANSCRIPT_KEEP)) {
        let Some(raw) = m.get("content").and_then(Value::as_str) else { continue };
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        let at = stamp(m).to_string();
        let stored = unwrap_stored(role, raw);
        if let Some(r) = stored.reasoning {
            out.push(Item::Reasoning(r));
        }
        for (name, arguments) in stored.calls {
            out.push(Item::ToolCall { name, arguments });
        }
        let text = strip_date_stamp(&strip_inline_images(&stored.text)).trim().to_string();
        if text.is_empty() {
            continue;
        }
        out.push(match role {
            "user" => Item::User { text, at },
            "tool" => Item::ToolOutput(text),
            _ => Item::Agent { text, at },
        });
    }
    out
}

/// What one stored message unpacks to.
struct Stored {
    text: String,
    reasoning: Option<String>,
    calls: Vec<(String, Value)>,
}

/// Unpacks the `{"content","reasoning_content","tool_calls"}` envelope assistant and tool messages are stored in.
fn unwrap_stored(role: &str, raw: &str) -> Stored {
    let plain = || Stored { text: raw.to_string(), reasoning: None, calls: Vec::new() };
    if role == "user" || !raw.trim_start().starts_with('{') {
        return plain();
    }
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(raw) else { return plain() };
    if !o.contains_key("content") {
        return plain();
    }
    let text = o.get("content").and_then(Value::as_str).unwrap_or("").to_string();
    let reasoning = o
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let calls = o
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let name = c.get("name").or_else(|| c.pointer("/function/name")).and_then(Value::as_str)?.to_string();
                    let arguments = match c.get("arguments").or_else(|| c.pointer("/function/arguments")) {
                        Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or_else(|_| Value::String(s.clone())),
                        Some(v) => v.clone(),
                        None => Value::Null,
                    };
                    Some((name, arguments))
                })
                .collect()
        })
        .unwrap_or_default();
    Stored { text, reasoning, calls }
}

/// Replaces each inline `[IMAGE:data:…]` payload with a short marker.
fn strip_inline_images(s: &str) -> String {
    const OPEN: &str = "[IMAGE:data:";
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(OPEN) {
        out.push_str(&rest[..i]);
        out.push_str("[image]");
        match rest[i + OPEN.len()..].find(']') {
            Some(end) => rest = &rest[i + OPEN.len() + end + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drops the `[CURRENT DATE & TIME: …]` or `[2026-09-11 05:36:28 +00:00]` preamble the gateway prepends.
fn strip_date_stamp(s: &str) -> String {
    let t = s.trim_start();
    let Some(rest) = t.strip_prefix('[') else { return s.to_string() };
    let labelled = rest.starts_with("CURRENT DATE & TIME:");
    let dated = rest.len() > 4 && rest[..4].chars().all(|c| c.is_ascii_digit()) && rest[4..].starts_with('-');
    if !labelled && !dated {
        return s.to_string();
    }
    match rest.find(']') {
        Some(i) => rest[i + 1..].trim_start().to_string(),
        None => s.to_string(),
    }
}

/// What a run's status word says about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Degraded,
    Failed,
    Skipped,
    Unknown,
}

impl Outcome {
    pub fn read(status: Option<&str>) -> Outcome {
        match status.unwrap_or_default().trim() {
            "ok" | "success" => Outcome::Ok,
            "degraded" => Outcome::Degraded,
            "error" | "failed" => Outcome::Failed,
            "skipped" => Outcome::Skipped,
            _ => Outcome::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Outcome::Ok => "Succeeded",
            Outcome::Degraded => "Ran, delivery failed",
            Outcome::Failed => "Failed",
            Outcome::Skipped => "Skipped",
            Outcome::Unknown => "Not run yet",
        }
    }
}

/// One scheduled job, from `GET /api/cron`.
#[derive(Clone, Debug, Deserialize)]
pub struct Automation {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub expression: String,
    #[serde(default)]
    pub agent_alias: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub next_run: Option<String>,
    #[serde(default)]
    pub last_run: Option<String>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_output: Option<String>,
}

impl Automation {
    pub fn label(&self) -> &str {
        self.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&self.id)
    }

    pub fn outcome(&self) -> Outcome {
        Outcome::read(self.last_status.as_deref())
    }
}

/// One past run, from `GET /api/cron/{id}/runs`.
#[derive(Clone, Debug, Deserialize)]
pub struct Run {
    pub id: i64,
    pub job_id: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

impl Run {
    pub fn outcome(&self) -> Outcome {
        Outcome::read(Some(&self.status))
    }
}

/// Text trimmed to what is kept for one run.
fn clamp(text: &str) -> String {
    if text.chars().count() <= OUTPUT_MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(OUTPUT_MAX).collect();
    format!("{cut}\u{2026}")
}

/// Every automation the gateway schedules, with the outcome of its last run.
pub async fn automations(gw: &ZeroclawGateway) -> Result<Vec<Automation>> {
    parse_jobs(get(gw, "/api/cron").await?)
}

fn parse_jobs(value: Value) -> Result<Vec<Automation>> {
    let mut jobs: Vec<Automation> = serde_json::from_value(value.get("jobs").cloned().unwrap_or(Value::Null))?;
    for job in &mut jobs {
        anyhow::ensure!(!job.id.is_empty() && job.id.len() <= 128, "invalid automation id");
        if let Some(out) = job.last_output.as_mut() {
            *out = clamp(out);
        }
    }
    Ok(jobs)
}

/// One automation's recent runs, newest first.
pub async fn runs(gw: &ZeroclawGateway, job: &str, limit: usize) -> Result<Vec<Run>> {
    let path = format!("/api/cron/{}/runs?limit={}", component(job), limit.clamp(1, RUNS_MAX));
    parse_runs(get(gw, &path).await?, job)
}

fn parse_runs(value: Value, job: &str) -> Result<Vec<Run>> {
    let mut runs: Vec<Run> = serde_json::from_value(value.get("runs").cloned().unwrap_or(Value::Null))?;
    for run in &mut runs {
        anyhow::ensure!(run.job_id == job, "run history belongs to another automation");
        if let Some(out) = run.output.as_mut() {
            *out = clamp(out);
        }
    }
    Ok(runs)
}

/// Runs one automation now and answers with whether it succeeded and what it output.
pub async fn run_now(gw: &ZeroclawGateway, job: &str) -> Result<(Outcome, String)> {
    let path = format!("/api/cron/{}/run", component(job));
    let v = call(gw, reqwest::Method::POST, &path, Some(RUN_NOW_TIMEOUT)).await?;
    Ok(parse_run_now(&v))
}

fn parse_run_now(v: &Value) -> (Outcome, String) {
    let success = v.get("success").and_then(Value::as_bool).unwrap_or(false);
    let status = v.get("status").and_then(Value::as_str).unwrap_or(if success { "ok" } else { "error" });
    (Outcome::read(Some(status)), clamp(v.get("output").and_then(Value::as_str).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn twin_stores_merge_into_one_newest_first_session() {
        let v = json!({"sessions": [
            {"session_id": "a", "agent_alias": "tech_chat", "session_key": "a", "message_count": 2, "last_activity": "2026-09-26T10:00:00Z"},
            {"session_id": "b", "agent_alias": "tech_chat", "session_key": "b", "message_count": 0, "last_activity": "2026-09-26T12:00:00Z"},
            {"session_id": "a", "agent_alias": "tech_chat", "session_key": "gw_a", "message_count": 3, "last_activity": "2026-09-26T11:00:00Z", "name": "Owner-PC"},
            {"session_id": "cron_shelf_triage", "agent_alias": "tech_chat", "message_count": 4, "last_activity": "2026-09-26T09:00:00Z"},
        ]});
        let rows = parse_sessions(&v);
        assert_eq!(rows.len(), 2, "the empty session is skipped");
        assert_eq!(rows[0].id, "a");
        assert_eq!((rows[0].messages, rows[0].label()), (5, "Owner-PC"));
        assert_eq!(rows[0].keys, vec!["a".to_string(), "gw_a".to_string()]);
        assert_eq!(rows[0].last_activity, "2026-09-26T11:00:00Z");
        assert!(rows[1].is_automation());
        assert_eq!(rows[1].label(), "cron_shelf_triage");
    }

    #[test]
    fn stored_envelopes_unpack_into_reasoning_calls_and_text() {
        let assistant = json!({"content": "Done.", "reasoning_content": " thinking ",
            "tool_calls": [{"function": {"name": "mastertech__list_waiting_services", "arguments": "{\"limit\":10}"}}]});
        let messages = vec![
            json!({"role": "user", "content": "[CURRENT DATE & TIME: 2026-09-26 10:00] check the shelf", "created_at": "1"}),
            json!({"role": "assistant", "content": assistant.to_string(), "created_at": "2"}),
            json!({"role": "tool", "content": "{\"content\": \"3 services\", \"tool_call_id\": \"x\"}", "created_at": "3"}),
            json!({"role": "assistant", "content": "see [IMAGE:data:image/png;base64,AAAA] here", "created_at": "4"}),
        ];
        assert_eq!(
            parse_transcript(&messages),
            vec![
                Item::User { text: "check the shelf".into(), at: "1".into() },
                Item::Reasoning("thinking".into()),
                Item::ToolCall { name: "mastertech__list_waiting_services".into(), arguments: json!({"limit": 10}) },
                Item::Agent { text: "Done.".into(), at: "2".into() },
                Item::ToolOutput("3 services".into()),
                Item::Agent { text: "see [image] here".into(), at: "4".into() },
            ]
        );
    }

    #[test]
    fn a_plain_bracketed_line_keeps_its_text() {
        assert_eq!(strip_date_stamp("[note] keep me"), "[note] keep me");
        assert_eq!(strip_date_stamp("[2026-09-11 05:36:28 +00:00] hi"), "hi");
    }

    #[test]
    fn automations_and_runs_parse_and_reject_foreign_history() {
        let jobs = parse_jobs(json!({"jobs": [
            {"id": "shelf_triage", "name": "Shelf triage", "expression": "15 10,15 * * *", "agent_alias": "shelf_triage",
             "enabled": true, "last_status": "ok", "last_output": "2155485 75 ..."},
            {"id": "bare"}]}))
        .expect("jobs");
        assert_eq!((jobs[0].label(), jobs[0].outcome()), ("Shelf triage", Outcome::Ok));
        assert_eq!((jobs[1].label(), jobs[1].outcome()), ("bare", Outcome::Unknown));
        assert!(parse_jobs(json!({"jobs": [{"id": ""}]})).is_err());
        let runs = json!({"runs": [{"id": 4, "job_id": "shelf_triage", "started_at": "a", "status": "error", "output": "boom", "duration_ms": 12}]});
        assert!(parse_runs(runs.clone(), "other").is_err());
        let ours = parse_runs(runs, "shelf_triage").expect("runs");
        assert_eq!((ours[0].outcome(), ours[0].duration_ms), (Outcome::Failed, Some(12)));
    }

    #[test]
    fn a_run_now_answer_reads_its_status_or_success_flag() {
        assert_eq!(parse_run_now(&json!({"success": true, "output": "ok"})), (Outcome::Ok, "ok".to_string()));
        assert_eq!(parse_run_now(&json!({"success": false})).0, Outcome::Failed);
        assert_eq!(parse_run_now(&json!({"success": true, "status": "degraded"})).0, Outcome::Degraded);
        assert_eq!(parse_agents(&json!({"agents": ["tech_chat", 3, "sweeper"]})), vec!["tech_chat", "sweeper"]);
        assert_eq!(parse_running(&json!({"sessions": [{"session_id": "a"}, {"x": 1}]})), vec!["a"]);
    }

    #[test]
    fn long_outputs_are_bounded() {
        let big = "x".repeat(OUTPUT_MAX * 2);
        let jobs = parse_jobs(json!({"jobs": [{"id": "j", "last_output": big}]})).expect("jobs");
        assert_eq!(jobs[0].last_output.as_deref().map(|o| o.chars().count()), Some(OUTPUT_MAX + 1));
    }
}
