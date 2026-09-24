//! `wait`: a broker-side pause that returns early when a machine condition holds.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::tools::{ToolHost, ToolOutcome};

pub const TOOL_NAME: &str = "wait";
/// Longest single wait.
pub const MAX_SECS: u64 = 600;
/// Gap between two checks of a condition.
pub const CHECK_EVERY: Duration = Duration::from_secs(5);
/// Probe timeout handed to `remote_channel_health` on each check.
const PROBE_SECS: u64 = 3;

/// What ends a wait before its time is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Until {
    Elapsed,
    ClientOnline,
    ClientOffline,
    ExecDone,
}

impl Until {
    fn label(self) -> &'static str {
        match self {
            Until::Elapsed => "time",
            Until::ClientOnline => "client_online",
            Until::ClientOffline => "client_offline",
            Until::ExecDone => "exec_done",
        }
    }
}

/// A validated `wait` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitSpec {
    pub seconds: u64,
    pub until: Until,
    pub job_id: Option<String>,
}

impl WaitSpec {
    /// Reads the tool arguments; `seconds` is clamped to 1..=MAX_SECS.
    pub fn parse(arguments: &Value) -> Result<Self, String> {
        let seconds = arguments
            .get("seconds")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0).round() as u64)))
            .ok_or("seconds is required (1-600)")?
            .clamp(1, MAX_SECS);
        let until = match arguments.get("until").and_then(Value::as_str).map(str::trim) {
            None | Some("") | Some("time") => Until::Elapsed,
            Some("client_online") => Until::ClientOnline,
            Some("client_offline") => Until::ClientOffline,
            Some("exec_done") => Until::ExecDone,
            Some(other) => {
                return Err(format!("unknown until {other:?}; use client_online, client_offline or exec_done"));
            }
        };
        let job_id = ["job_id", "exec_id"]
            .iter()
            .find_map(|k| arguments.get(*k).and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if until == Until::ExecDone && job_id.is_none() {
            return Err("job_id is required with until=exec_done".into());
        }
        Ok(Self { seconds, until, job_id })
    }

    pub fn needs_machine(&self) -> bool {
        self.until != Until::Elapsed
    }
}

/// `thread/start.dynamicTools` entry for `wait`.
pub fn tool_spec() -> Value {
    json!({
        "type": "function",
        "name": TOOL_NAME,
        "description": "Let time pass on the broker, without touching the machine and without an approval. \
            With `until` it checks about every 5 s and returns as soon as the condition holds: client_offline \
            (the machine dropped, e.g. after remote_reboot_client), client_online (remote_channel_health is \
            healthy again), exec_done (the remote_exec job named by job_id finished). A message or a stop from \
            the technician ends the wait early. Use this instead of a sleep job on the machine or repeated \
            remote_channel_health calls.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "seconds": { "type": "integer", "minimum": 1, "maximum": MAX_SECS, "description": "Longest time to wait." },
                "until": {
                    "type": "string",
                    "enum": ["client_online", "client_offline", "exec_done"],
                    "description": "Return as soon as this holds; omit to wait the full time."
                },
                "job_id": { "type": "string", "description": "remote_exec job to watch; required with exec_done." }
            },
            "required": ["seconds"]
        },
        "deferLoading": false,
    })
}

/// How a `remote_channel_health` verdict answers a client condition; `None` when it answers neither way.
pub fn verdict_meets(until: Until, verdict: &str) -> Option<bool> {
    let online = match verdict {
        "healthy" => true,
        "no_session" | "degraded" => false,
        _ => return None,
    };
    match until {
        Until::ClientOnline => Some(online),
        Until::ClientOffline => Some(!online),
        _ => None,
    }
}

/// True for a RemoteExec job state that is finished.
pub fn job_finished(state: &str) -> bool {
    !matches!(state, "Queued" | "Running" | "")
}

/// The result of one condition check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    Met(String),
    Pending(String),
}

/// A source of condition checks.
pub trait Observe {
    fn observe(&self, spec: &WaitSpec) -> impl Future<Output = Seen> + Send;
}

/// Checks a condition through the session's own tools, in process.
pub struct SessionTools {
    pub tools: Arc<ToolHost>,
    pub connection_string: String,
}

impl Observe for SessionTools {
    async fn observe(&self, spec: &WaitSpec) -> Seen {
        let cs = self.connection_string.as_str();
        match spec.until {
            Until::Elapsed => Seen::Pending(String::new()),
            Until::ClientOnline | Until::ClientOffline => {
                let args = json!({ "connection_string": cs, "probe_timeout_secs": PROBE_SECS });
                let outcome = self.tools.call("remote_channel_health", args, false).await;
                let verdict = serde_json::from_str::<Value>(&outcome.text)
                    .ok()
                    .and_then(|v| v.get("verdict").and_then(Value::as_str).map(str::to_string));
                match verdict {
                    Some(v) if verdict_meets(spec.until, &v) == Some(true) => {
                        Seen::Met(format!("remote_channel_health verdict {v}"))
                    }
                    Some(v) => Seen::Pending(format!("last verdict {v}")),
                    None => Seen::Pending(format!("last check failed: {}", clip(&outcome.text))),
                }
            }
            Until::ExecDone => {
                let job_id = spec.job_id.as_deref().unwrap_or_default();
                let args = json!({ "connection_string": cs, "job_id": job_id, "max_bytes": 0 });
                let outcome = self.tools.call("remote_exec_tail", args, false).await;
                job_seen(job_id, &outcome)
            }
        }
    }
}

/// Reads a `remote_exec_tail` result as an `exec_done` check.
fn job_seen(job_id: &str, outcome: &ToolOutcome) -> Seen {
    if !outcome.success && outcome.text.contains("not retaining job") {
        return Seen::Met(format!("the client no longer holds job {job_id}"));
    }
    let snapshot = serde_json::from_str::<Value>(&outcome.text).ok();
    let state = snapshot.as_ref().and_then(|v| v.get("state").and_then(Value::as_str)).unwrap_or("");
    if job_finished(state) {
        let exit = snapshot
            .as_ref()
            .and_then(|v| v.pointer("/exit/exit_code"))
            .filter(|c| !c.is_null())
            .map(|c| format!(", exit code {c}"))
            .unwrap_or_default();
        Seen::Met(format!(
            "job {job_id} ended {state}{exit}; read its output with remote_exec_tail {{job_id, from_seq: 0}}"
        ))
    } else if state.is_empty() {
        Seen::Pending(format!("last check failed: {}", clip(&outcome.text)))
    } else {
        Seen::Pending(format!("job {job_id} still {state}"))
    }
}

fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(200).collect();
    if text.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// Waits until the condition holds, the time is up, or `cancel` fires, checking every `every`.
pub async fn run(
    observer: impl Observe,
    spec: WaitSpec,
    every: Duration,
    mut cancel: oneshot::Receiver<String>,
) -> ToolOutcome {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(spec.seconds);
    let mut last = String::new();
    loop {
        if spec.needs_machine() {
            match observer.observe(&spec).await {
                Seen::Met(note) => {
                    return ToolOutcome::ok(format!(
                        "{} after {}s: {note}.",
                        spec.until.label(),
                        started.elapsed().as_secs()
                    ));
                }
                Seen::Pending(note) => last = note,
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return ToolOutcome::ok(match spec.until {
                Until::Elapsed => format!("waited {}s.", started.elapsed().as_secs()),
                until => format!(
                    "waited {}s; {} did not happen ({last}).",
                    started.elapsed().as_secs(),
                    until.label()
                ),
            });
        }
        let step = if spec.needs_machine() { every.min(deadline - now) } else { deadline - now };
        tokio::select! {
            _ = tokio::time::sleep(step) => {}
            reason = &mut cancel => {
                let why = reason.unwrap_or_else(|_| "the session ended".to_string());
                return ToolOutcome::ok(format!("wait stopped after {}s: {why}.", started.elapsed().as_secs()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Hands out scripted checks, then `Pending` forever.
    struct Scripted(Mutex<Vec<Seen>>);

    impl Scripted {
        fn new(mut seen: Vec<Seen>) -> Self {
            seen.reverse();
            Self(Mutex::new(seen))
        }
    }

    impl Observe for Scripted {
        async fn observe(&self, _spec: &WaitSpec) -> Seen {
            let next = self.0.lock().ok().and_then(|mut v| v.pop());
            next.unwrap_or_else(|| Seen::Pending("last verdict no_session".into()))
        }
    }

    fn spec(seconds: u64, until: Until) -> WaitSpec {
        WaitSpec { seconds, until, job_id: None }
    }

    const FAST: Duration = Duration::from_millis(10);

    #[tokio::test]
    async fn a_condition_met_on_a_later_check_ends_the_wait_early() {
        let checks = Scripted::new(vec![
            Seen::Pending("last verdict degraded".into()),
            Seen::Pending("last verdict no_session".into()),
            Seen::Met("remote_channel_health verdict healthy".into()),
        ]);
        let (_tx, rx) = oneshot::channel();
        let started = Instant::now();
        let out = run(checks, spec(30, Until::ClientOnline), FAST, rx).await;
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(out.success);
        assert!(out.text.starts_with("client_online after 0s: remote_channel_health verdict healthy"), "{}", out.text);
    }

    #[tokio::test]
    async fn the_time_limit_reports_the_last_check() {
        let (_tx, rx) = oneshot::channel();
        let out = run(Scripted::new(Vec::new()), spec(1, Until::ClientOnline), FAST, rx).await;
        assert_eq!(out.text, "waited 1s; client_online did not happen (last verdict no_session).");
    }

    #[tokio::test]
    async fn a_cancel_ends_the_wait_with_its_reason() {
        let (tx, rx) = oneshot::channel();
        let wait = tokio::spawn(run(Scripted::new(Vec::new()), spec(600, Until::Elapsed), CHECK_EVERY, rx));
        tx.send("the technician sent a message".into()).ok();
        let out = wait.await.expect("wait task");
        assert_eq!(out.text, "wait stopped after 0s: the technician sent a message.");
    }

    #[tokio::test]
    async fn a_dropped_runner_ends_the_wait() {
        let (tx, rx) = oneshot::channel::<String>();
        drop(tx);
        let out = run(Scripted::new(Vec::new()), spec(600, Until::ClientOffline), FAST, rx).await;
        assert!(out.text.ends_with("the session ended."), "{}", out.text);
    }

    #[test]
    fn arguments_parse_with_defaults_clamps_and_aliases() {
        let plain = WaitSpec::parse(&json!({ "seconds": 90 })).unwrap();
        assert_eq!(plain, spec(90, Until::Elapsed));
        assert!(!plain.needs_machine());
        assert_eq!(WaitSpec::parse(&json!({ "seconds": 5000 })).unwrap().seconds, MAX_SECS);
        assert_eq!(WaitSpec::parse(&json!({ "seconds": 0 })).unwrap().seconds, 1);
        assert_eq!(WaitSpec::parse(&json!({ "seconds": 12.6 })).unwrap().seconds, 13);
        let online = WaitSpec::parse(&json!({ "seconds": 300, "until": "client_online" })).unwrap();
        assert_eq!(online.until, Until::ClientOnline);
        assert!(online.needs_machine());
        let job = WaitSpec::parse(&json!({ "seconds": 60, "until": "exec_done", "exec_id": "job-7" })).unwrap();
        assert_eq!(job.job_id.as_deref(), Some("job-7"));
    }

    #[test]
    fn bad_arguments_are_refused_with_the_fix() {
        assert!(WaitSpec::parse(&json!({})).unwrap_err().contains("seconds"));
        assert!(WaitSpec::parse(&json!({ "seconds": 10, "until": "exec_done" })).unwrap_err().contains("job_id"));
        assert!(WaitSpec::parse(&json!({ "seconds": 10, "until": "forever" })).unwrap_err().contains("client_online"));
    }

    #[test]
    fn channel_verdicts_answer_the_client_conditions() {
        assert_eq!(verdict_meets(Until::ClientOnline, "healthy"), Some(true));
        assert_eq!(verdict_meets(Until::ClientOnline, "no_session"), Some(false));
        assert_eq!(verdict_meets(Until::ClientOffline, "no_session"), Some(true));
        assert_eq!(verdict_meets(Until::ClientOffline, "degraded"), Some(true));
        assert_eq!(verdict_meets(Until::ClientOffline, "healthy"), Some(false));
        assert_eq!(verdict_meets(Until::ClientOnline, "one_way_alive_round_trips_dead"), None);
    }

    #[test]
    fn job_snapshots_answer_exec_done() {
        let running = ToolOutcome::ok(json!({ "state": "Running" }).to_string());
        assert_eq!(job_seen("job-1", &running), Seen::Pending("job job-1 still Running".into()));
        let done = ToolOutcome::ok(json!({ "state": "Succeeded", "exit": { "exit_code": 0 } }).to_string());
        assert!(matches!(job_seen("job-1", &done), Seen::Met(t) if t.starts_with("job job-1 ended Succeeded, exit code 0")));
        let gone = ToolOutcome::failure("tool `remote_exec_tail` failed: client is not retaining job job-1.".into());
        assert!(matches!(job_seen("job-1", &gone), Seen::Met(_)));
        assert!(job_finished("Orphaned") && !job_finished("Queued"));
    }

    #[test]
    fn the_spec_offers_the_three_conditions() {
        let spec = tool_spec();
        assert_eq!(spec["name"], TOOL_NAME);
        assert_eq!(spec["inputSchema"]["properties"]["until"]["enum"].as_array().map(Vec::len), Some(3));
        assert_eq!(spec["inputSchema"]["required"], json!(["seconds"]));
    }
}
