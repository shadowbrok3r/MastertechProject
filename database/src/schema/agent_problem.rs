//! AI requests and agent sessions that went wrong, across every technician, for Root.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, RecordIdExt, SurrealValue};
use crate::db;

/// Start of the error row written when codex retries a failed call by itself.
pub const RETRYING_PREFIX: &str = "Agent hit a transient error and is retrying";
/// Error row written when the broker's link to the agent host drops and it reconnects.
pub const RECONNECTING_TEXT: &str = "Connection to the agent host dropped; reconnecting.";
/// Start of the error row written when a turn ends on an error.
pub const AGENT_ERROR_PREFIX: &str = "Agent error: ";

/// Longest headline drawn from an unrecognised error, in characters.
const HEADLINE_MAX_CHARS: usize = 120;

/// Problems since `$window` ago, then the requests and sessions stuck right now.
const PROBLEMS_SQL: &str = "LET $since = time::now() - type::duration($window); \
     SELECT id, requested_by, store, hostname, connection_string, service_number, title, status, \
         (SELECT id, text, created_at FROM agent_event \
          WHERE thread = $parent.id AND kind = 'error' AND created_at >= $since) AS errors \
     FROM agent_thread WHERE last_event_at >= $since OR updated_at >= $since; \
     SELECT id, requested_by, store, hostname, connection_string, service_number, title, status, error, \
         (closed_at ?? updated_at) AS at \
     FROM agent_thread WHERE status = 'failed' AND (closed_at ?? updated_at) >= $since; \
     SELECT id, requested_by, store, hostname, connection_string, service_number, title, status, error, \
         updated_at AS at \
     FROM agent_thread WHERE status IN ['queued', 'starting'] AND updated_at < time::now() - 10m; \
     SELECT id, requested_by, store, hostname, connection_string, service_number, status, dispatch_error, \
         (finished_at ?? created_at) AS at \
     FROM assist_request WHERE status IN ['failed', 'declined'] AND (finished_at ?? created_at) >= $since; \
     SELECT id, requested_by, store, hostname, connection_string, service_number, status, dispatch_error, \
         created_at AS at \
     FROM assist_request WHERE status = 'pending' AND created_at < time::now() - 2m;";

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProblemKind {
    /// A turn ended on an error, or a message or stop never reached the agent.
    AgentError,
    /// The session failed and closed.
    SessionFailed,
    /// The session has sat queued or starting for over ten minutes.
    Stuck,
    /// The broker could not open a session for the request.
    RequestFailed,
    /// The request was given up on before any broker claimed it.
    NotPickedUp,
    /// The request has waited over two minutes for a broker to claim it.
    Waiting,
}

impl ProblemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AgentError => "Agent error",
            Self::SessionFailed => "Session failed",
            Self::Stuck => "Session stuck",
            Self::RequestFailed => "Request failed",
            Self::NotPickedUp => "Never picked up",
            Self::Waiting => "Waiting for admin-agent",
        }
    }

    /// True for a state that clears once the broker catches up.
    pub fn is_ongoing(self) -> bool {
        matches!(self, Self::Stuck | Self::Waiting)
    }
}

/// One failure, from an error row, a session or a request.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProblem {
    /// Stable per source row: `event:`, `thread:` or `request:` and its key.
    pub key: String,
    pub kind: ProblemKind,
    pub at: Option<Datetime>,
    pub requested_by: Option<String>,
    pub store: Option<String>,
    pub hostname: Option<String>,
    pub connection_string: String,
    pub service_number: Option<String>,
    /// Session title, when a session exists.
    pub title: Option<String>,
    pub thread: Option<RecordId>,
    /// The session's status when last read.
    pub thread_status: Option<String>,
    pub request: Option<RecordId>,
    /// The error as recorded.
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
struct ErrorRow {
    id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    text: String,
    #[serde(default)]
    #[surreal(default)]
    created_at: Option<Datetime>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
struct ThreadErrors {
    id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    requested_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    hostname: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    connection_string: String,
    #[serde(default)]
    #[surreal(default)]
    service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    title: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    status: String,
    #[serde(default)]
    #[surreal(default)]
    errors: Vec<ErrorRow>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
struct ThreadRow {
    id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    requested_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    hostname: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    connection_string: String,
    #[serde(default)]
    #[surreal(default)]
    service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    title: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    status: String,
    #[serde(default)]
    #[surreal(default)]
    error: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    at: Option<Datetime>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
struct RequestRow {
    id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    requested_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    hostname: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    connection_string: String,
    #[serde(default)]
    #[surreal(default)]
    service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    status: String,
    #[serde(default)]
    #[surreal(default)]
    dispatch_error: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    at: Option<Datetime>,
}

/// True for an error row the agent recovers from by itself.
pub fn is_transient(text: &str) -> bool {
    let text = text.trim();
    text.starts_with(RETRYING_PREFIX) || text == RECONNECTING_TEXT
}

/// The error rows of `thread` that stopped the agent or lost a message.
fn error_problems(thread: ThreadErrors) -> Vec<AgentProblem> {
    let ThreadErrors {
        id,
        requested_by,
        store,
        hostname,
        connection_string,
        service_number,
        title,
        status,
        errors,
    } = thread;
    errors
        .into_iter()
        .filter(|e| !is_transient(&e.text))
        .map(|e| AgentProblem {
            key: format!("event:{}", e.id.key_string()),
            kind: ProblemKind::AgentError,
            at: e.created_at,
            requested_by: requested_by.clone(),
            store: store.clone(),
            hostname: hostname.clone(),
            connection_string: connection_string.clone(),
            service_number: service_number.clone(),
            title: title.clone(),
            thread: Some(id.clone()),
            thread_status: Some(status.clone()),
            request: None,
            message: e.text,
        })
        .collect()
}

fn thread_problem(row: ThreadRow, kind: ProblemKind) -> AgentProblem {
    let message = match kind {
        ProblemKind::Stuck => row.status.clone(),
        _ => row.error.unwrap_or_default(),
    };
    AgentProblem {
        key: format!("thread:{}", row.id.key_string()),
        kind,
        at: row.at,
        requested_by: row.requested_by,
        store: row.store,
        hostname: row.hostname,
        connection_string: row.connection_string,
        service_number: row.service_number,
        title: row.title,
        thread: Some(row.id),
        thread_status: Some(row.status),
        request: None,
        message,
    }
}

fn request_problem(row: RequestRow) -> AgentProblem {
    let kind = match row.status.as_str() {
        "pending" => ProblemKind::Waiting,
        "declined" => ProblemKind::NotPickedUp,
        _ => ProblemKind::RequestFailed,
    };
    AgentProblem {
        key: format!("request:{}", row.id.key_string()),
        kind,
        at: row.at,
        requested_by: row.requested_by,
        store: row.store,
        hostname: row.hostname,
        connection_string: row.connection_string,
        service_number: row.service_number,
        title: None,
        thread: None,
        thread_status: None,
        request: Some(row.id),
        message: row.dispatch_error.unwrap_or_default(),
    }
}

/// Plain-English summary of what went wrong.
pub fn headline(kind: ProblemKind, message: &str) -> String {
    let raw = message.trim();
    let text = raw.strip_prefix(AGENT_ERROR_PREFIX).unwrap_or(raw);
    let lower = text.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let line = match kind {
        ProblemKind::Waiting => "Waiting for admin-agent to pick it up",
        ProblemKind::NotPickedUp => "Never picked up; was admin-agent running?",
        ProblemKind::Stuck if text == "queued" => "Waiting for a free AI slot",
        ProblemKind::Stuck => "Never finished starting",
        ProblemKind::RequestFailed if says(&["already working on this computer"]) => {
            "Another AI session was already working on this PC"
        }
        ProblemKind::RequestFailed
            if says(&[
                "error sending request",
                "unauthorized",
                "connection refused",
            ]) =>
        {
            "Could not reach the AI service"
        }
        ProblemKind::RequestFailed => "Could not open a session",
        ProblemKind::SessionFailed if says(&["codex daemon"]) => "Could not reach the AI host",
        ProblemKind::SessionFailed if says(&["tool host"]) => "The tool host failed to start",
        ProblemKind::SessionFailed if says(&["thread start"]) => "The agent could not start",
        ProblemKind::SessionFailed if says(&["plugin manager"]) => "The broker was not ready",
        ProblemKind::SessionFailed if text.is_empty() => "The session failed",
        ProblemKind::AgentError | ProblemKind::SessionFailed => {
            if text.starts_with("Could not deliver")
                || text.starts_with("Could not send a queued message")
            {
                "A technician's message never reached the agent"
            } else if text.starts_with("Could not resume") {
                "Could not resume the session; the agent started over"
            } else if text.starts_with("Stop did not reach") {
                "A stop never reached the agent"
            } else if text.starts_with("Could not start compaction") {
                "Could not compact the conversation"
            } else if says(&["429", "too many requests", "nodes are busy"]) {
                "The AI servers were busy"
            } else if says(&["409", "conversation state"]) {
                "The AI server lost the conversation"
            } else if says(&[
                "stream disconnected",
                "stream closed",
                "error sending request",
                "connection reset",
                "timed out",
            ]) {
                "Lost the connection to the AI server"
            } else if says(&[
                "badrequest",
                "400 bad request",
                "context length",
                "maximum context",
            ]) {
                "The AI server rejected the request"
            } else {
                return first_line(text);
            }
        }
    };
    line.to_string()
}

/// The first line of `text`, cut to [`HEADLINE_MAX_CHARS`] characters.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Unknown error");
    if line.chars().count() <= HEADLINE_MAX_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(HEADLINE_MAX_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

impl AgentProblem {
    /// Problems from the last `window` (a duration such as `7d`) and the ones ongoing, newest first.
    pub async fn list(window: &str) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(PROBLEMS_SQL)
            .bind(("window", window.to_string()))
            .await?;
        let with_errors: Vec<ThreadErrors> = res.take(1)?;
        let failed: Vec<ThreadRow> = res.take(2)?;
        let stuck: Vec<ThreadRow> = res.take(3)?;
        let requests: Vec<RequestRow> = res.take(4)?;
        let waiting: Vec<RequestRow> = res.take(5)?;
        let mut problems: Vec<Self> = with_errors.into_iter().flat_map(error_problems).collect();
        problems.extend(
            failed
                .into_iter()
                .map(|r| thread_problem(r, ProblemKind::SessionFailed)),
        );
        problems.extend(
            stuck
                .into_iter()
                .map(|r| thread_problem(r, ProblemKind::Stuck)),
        );
        problems.extend(requests.into_iter().chain(waiting).map(request_problem));
        sort_newest_first(&mut problems);
        Ok(problems)
    }

    pub fn headline(&self) -> String {
        headline(self.kind, &self.message)
    }

    /// The session title, else the service number and machine, else what the session is about.
    pub fn subject(&self) -> String {
        if let Some(title) = self
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            return title.to_string();
        }
        let host = self
            .hostname
            .as_deref()
            .map(str::trim)
            .filter(|h| !h.is_empty());
        match (self.service_number.as_deref(), host) {
            (Some(sn), Some(host)) => format!("#{sn} {host}"),
            (Some(sn), None) => format!("#{sn}"),
            (None, Some(host)) => host.to_string(),
            (None, None) if super::agent_thread::is_voice(&self.connection_string) => {
                "Voice question".to_string()
            }
            (None, None) if super::agent_thread::is_general(&self.connection_string) => {
                "Records chat".to_string()
            }
            (None, None) => self.connection_string.clone(),
        }
    }

    /// Whether `email` asked for the work that failed, ignoring case.
    pub fn requested_by_user(&self, email: &str) -> bool {
        self.requested_by
            .as_deref()
            .is_some_and(|r| r.trim().eq_ignore_ascii_case(email.trim()))
    }

    /// Whether the problem's session was still open when read.
    pub fn thread_is_open(&self) -> bool {
        self.thread_status
            .as_deref()
            .is_some_and(|s| super::agent_thread::AGENT_THREAD_OPEN_STATUSES.contains(&s))
    }
}

/// Newest first; a problem with no time sorts last.
fn sort_newest_first(problems: &mut [AgentProblem]) {
    problems.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.key.cmp(&b.key)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(key: &str, kind: ProblemKind, at: Option<i64>) -> AgentProblem {
        AgentProblem {
            key: key.to_string(),
            kind,
            at: at.and_then(|s| Datetime::from_timestamp(1_790_000_000 + s, 0)),
            requested_by: Some("tech@example.com".to_string()),
            store: Some("RIV".to_string()),
            hostname: None,
            connection_string: "general:tech@example.com".to_string(),
            service_number: None,
            title: None,
            thread: None,
            thread_status: None,
            request: None,
            message: String::new(),
        }
    }

    fn thread_errors(texts: &[&str]) -> ThreadErrors {
        ThreadErrors {
            id: RecordId::new("agent_thread", "t1"),
            requested_by: Some("tech@example.com".to_string()),
            store: Some("MUR".to_string()),
            hostname: Some("DESKTOP-1".to_string()),
            connection_string: "DESKTOP-1:abc".to_string(),
            service_number: Some("2155113".to_string()),
            title: Some("#2155113 DESKTOP-1".to_string()),
            status: "idle".to_string(),
            errors: texts
                .iter()
                .enumerate()
                .map(|(n, text)| ErrorRow {
                    id: RecordId::new("agent_event", format!("e{n}").as_str()),
                    text: text.to_string(),
                    created_at: None,
                })
                .collect(),
        }
    }

    #[test]
    fn retries_and_reconnects_are_not_problems() {
        let thread = thread_errors(&[
            "Agent hit a transient error and is retrying: Reconnecting... 1/5",
            RECONNECTING_TEXT,
            "Agent error: exceeded retry limit, last status: 429 Too Many Requests",
            "Could not deliver the technician's message: turn/steer failed",
        ]);
        let problems = error_problems(thread);
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["event:e2", "event:e3"]);
        assert!(problems.iter().all(|p| p.kind == ProblemKind::AgentError));
        assert_eq!(
            problems[0].thread,
            Some(RecordId::new("agent_thread", "t1"))
        );
        assert_eq!(problems[0].subject(), "#2155113 DESKTOP-1");
        assert!(problems[0].thread_is_open());
    }

    #[test]
    fn a_failed_session_is_not_open() {
        let row = ThreadRow {
            id: RecordId::new("agent_thread", "t2"),
            requested_by: None,
            store: None,
            hostname: None,
            connection_string: "PC-2:abc".to_string(),
            service_number: None,
            title: None,
            status: "failed".to_string(),
            error: Some("codex daemon: refused".to_string()),
            at: None,
        };
        let problem = thread_problem(row, ProblemKind::SessionFailed);
        assert_eq!(problem.key, "thread:t2");
        assert_eq!(problem.message, "codex daemon: refused");
        assert!(!problem.thread_is_open());
    }

    #[test]
    fn recorded_errors_read_as_plain_english() {
        let cases = [
            (
                "Agent error: exceeded retry limit, last status: 429 Too Many Requests",
                "The AI servers were busy",
            ),
            (
                "Agent error: stream disconnected before completion: error sending request for url (http://127.0.0.1:4000/v1/responses)",
                "Lost the connection to the AI server",
            ),
            (
                "Agent error: unexpected status 409 Conflict: Conversation state is unavailable; resend the conversation history",
                "The AI server lost the conversation",
            ),
            (
                "Agent error: {\"error\":{\"message\":\"litellm.BadRequestError: OpenAIException - Unterminated string\"}}",
                "The AI server rejected the request",
            ),
            (
                "Could not deliver the technician's message: turn/steer failed: missing field `expectedTurnId`",
                "A technician's message never reached the agent",
            ),
            (
                "Could not resume the previous agent thread (gone); starting a new one.",
                "Could not resume the session; the agent started over",
            ),
        ];
        for (message, want) in cases {
            assert_eq!(
                headline(ProblemKind::AgentError, message),
                want,
                "{message}"
            );
        }
    }

    #[test]
    fn requests_and_sessions_read_as_plain_english() {
        assert_eq!(
            headline(ProblemKind::NotPickedUp, "the client stopped waiting"),
            "Never picked up; was admin-agent running?"
        );
        assert_eq!(
            headline(ProblemKind::Waiting, ""),
            "Waiting for admin-agent to pick it up"
        );
        assert_eq!(
            headline(
                ProblemKind::RequestFailed,
                "another AI session (#2155485 JeffsComputer) is already working on this computer for joshua; stop or close it first"
            ),
            "Another AI session was already working on this PC"
        );
        assert_eq!(
            headline(
                ProblemKind::RequestFailed,
                "gateway 401 Unauthorized: pair first"
            ),
            "Could not reach the AI service"
        );
        assert_eq!(
            headline(
                ProblemKind::SessionFailed,
                "codex daemon: connection refused"
            ),
            "Could not reach the AI host"
        );
        assert_eq!(
            headline(ProblemKind::SessionFailed, ""),
            "The session failed"
        );
        assert_eq!(
            headline(ProblemKind::Stuck, "queued"),
            "Waiting for a free AI slot"
        );
        assert_eq!(
            headline(ProblemKind::Stuck, "starting"),
            "Never finished starting"
        );
    }

    #[test]
    fn an_unknown_error_keeps_its_first_line_clipped() {
        assert_eq!(
            headline(ProblemKind::AgentError, "Agent error: something new\nmore"),
            "something new"
        );
        let long = format!("Agent error: {}", "x".repeat(300));
        let line = headline(ProblemKind::AgentError, &long);
        assert_eq!(line.chars().count(), HEADLINE_MAX_CHARS);
        assert!(line.ends_with('\u{2026}'));
    }

    #[test]
    fn request_statuses_map_to_kinds() {
        let row = |status: &str| RequestRow {
            id: RecordId::new("assist_request", "r1"),
            requested_by: None,
            store: None,
            hostname: Some("PC-1".to_string()),
            connection_string: "PC-1:abc".to_string(),
            service_number: None,
            status: status.to_string(),
            dispatch_error: Some("the client stopped waiting".to_string()),
            at: None,
        };
        assert_eq!(request_problem(row("pending")).kind, ProblemKind::Waiting);
        assert_eq!(
            request_problem(row("declined")).kind,
            ProblemKind::NotPickedUp
        );
        let failed = request_problem(row("failed"));
        assert_eq!(failed.kind, ProblemKind::RequestFailed);
        assert_eq!(failed.key, "request:r1");
        assert_eq!(failed.subject(), "PC-1");
    }

    #[test]
    fn subjects_name_records_and_voice_sessions() {
        let mut p = problem("a", ProblemKind::AgentError, None);
        assert_eq!(p.subject(), "Records chat");
        p.connection_string = "general:voice:tech@example.com".to_string();
        assert_eq!(p.subject(), "Voice question");
        p.service_number = Some("2155808".to_string());
        assert_eq!(p.subject(), "#2155808");
    }

    #[test]
    fn newest_sorts_first_and_untimed_last() {
        let mut problems = vec![
            problem("old", ProblemKind::AgentError, Some(1)),
            problem("untimed", ProblemKind::Waiting, None),
            problem("new", ProblemKind::SessionFailed, Some(9)),
        ];
        sort_newest_first(&mut problems);
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["new", "old", "untimed"]);
    }

    #[test]
    fn the_requester_matches_ignoring_case() {
        let p = problem("a", ProblemKind::AgentError, None);
        assert!(p.requested_by_user(" Tech@Example.com "));
        assert!(!p.requested_by_user("other@example.com"));
    }
}
