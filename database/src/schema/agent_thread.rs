//! One Codex agent session about one customer machine.
//!
//! The admin-agent broker owns every row: it creates one when a tech accepts the
//! AI-help offer, records the codex thread id so a restart can resume the thread,
//! and keeps `status` current so any Mastertech instance can list live sessions
//! without talking to codex.

use serde::{Deserialize, Serialize};

use super::assist::{AssistRequest, OPENER_NOTE};
use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const AGENT_THREAD_TABLE: &str = "agent_thread";

/// Statuses a thread moves through; `closed` and `failed` are terminal.
pub const AGENT_THREAD_OPEN_STATUSES: [&str; 5] =
    ["queued", "starting", "idle", "running", "waiting_approval"];

/// Open statuses of a thread that is working or waiting for a pool slot to start.
pub const AGENT_THREAD_WORKING_STATUSES: [&str; 4] = ["queued", "starting", "running", "waiting_approval"];

/// Customer names of the service orders `$sns` that link a customer.
pub const CUSTOMER_NAMES_SQL: &str = "SELECT service_number, customer.name AS name FROM service_order \
     WHERE service_number IN $sns AND customer != NONE";

#[derive(Debug, Clone, Deserialize, SurrealValue)]
struct CustomerNameRow {
    service_number: String,
    #[serde(default)]
    #[surreal(default)]
    name: Option<String>,
}

/// `(service number, customer name)` for each of `sns` whose order names a customer.
pub async fn customer_names(sns: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    if sns.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<CustomerNameRow> =
        db().query(CUSTOMER_NAMES_SQL).bind(("sns", sns.to_vec())).await?.check()?.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let name = r.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty())?;
            Some((r.service_number, name))
        })
        .collect())
}

/// A session title in the task-name form, `Customer Name - 2155035`.
pub fn customer_label(name: &str, service_number: &str) -> String {
    format!("{} - {}", name.trim(), service_number.trim())
}

/// The newest open thread for `$cs` that is working (`$working`) or was opened or asked something in the last 30 minutes.
pub const RESUMABLE_SQL: &str = "SELECT * FROM agent_thread WHERE connection_string = $cs \
     AND status NOT IN ['closed', 'failed'] \
     AND (status IN $working OR array::max(array::concat([created_at], \
         (SELECT VALUE created_at FROM agent_turn WHERE thread = $parent.id AND kind IN ['start', 'steer', 'queue']))) \
         > time::now() - 30m) \
     ORDER BY created_at DESC LIMIT 1";

/// The session MasterTech reopens when it starts on a machine: open, changed in the last three days,
/// and not archived by the signed-in user.
pub const REOPEN_SQL: &str = "SELECT * FROM agent_thread WHERE connection_string = $cs \
     AND status NOT IN ['closed', 'failed'] AND updated_at > time::now() - 3d \
     AND id NOT IN (SELECT VALUE thread FROM agent_thread_archive WHERE user = $auth.id) \
     ORDER BY updated_at DESC LIMIT 1";

/// Threads of the signed-in technician: assigned to them, or asked for with their email.
const SIGNED_IN_TECH_THREADS: &str =
    "$auth != NONE AND (assignee = $auth.id OR requested_by = $auth.email)";

/// Longest title a rename stores, in characters.
const TITLE_MAX_CHARS: usize = 80;

/// Paragraph the voice console puts ahead of every spoken question.
const VOICE_PREAMBLE: &str = "Voice mode:";

/// Fills the unset service order, customer and service number of `$cs`'s open threads on `$sn` or none.
pub const ADOPT_THREAD_LINKS_SQL: &str = "UPDATE agent_thread SET service_order = service_order ?? $so, \
     customer = customer ?? $cust, service_number = service_number ?? $sn, updated_at = time::now() \
     WHERE connection_string = $cs AND status NOT IN ['closed', 'failed'] \
     AND (service_number = NONE OR service_number = $sn) \
     AND (service_order = NONE OR (customer = NONE AND $cust != NONE)) \
     RETURN VALUE id";

/// Inserts a `starting` row whose assignee is the user matching `$requested_by`, ignoring case.
pub const CREATE_THREAD_SQL: &str = "LET $assignee = IF $requested_by THEN (SELECT VALUE id FROM user \
     WHERE string::lowercase(email) = string::lowercase($requested_by) LIMIT 1)[0] END; \
     CREATE agent_thread CONTENT { status: 'starting', assist_request: $assist_request, \
     connection_string: $cs, hostname: $hostname, service_number: $sn, store: $store, \
     requested_by: $requested_by, assignee: $assignee, service_order: $service_order, \
     computer: $computer, customer: $customer, model: $model, provider: $provider, \
     driven_by: $driven_by, tool_path: $tool_path, broker_node: $broker_node, title: $title, \
     updated_at: time::now() } RETURN VALUE id";

/// True when the active user whose email is `$by`, ignoring case, is `$thread`'s assignee or a Root.
pub const MAY_STEER_SQL: &str = "LET $u = IF $by THEN (SELECT id, authorization, active FROM user \
     WHERE string::lowercase(email) = string::lowercase($by) LIMIT 1)[0] END; \
     RETURN $u != NONE AND $u.active = true AND ($u.id = $thread.assignee OR $u.authorization = 'Root')";

/// A token count in thousands, or millions past a million.
pub fn compact_tokens(n: i64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

/// `raw` as one trimmed line of at most [`TITLE_MAX_CHARS`] characters; `None` when blank.
pub fn clean_title(raw: &str) -> Option<String> {
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then(|| line.chars().take(TITLE_MAX_CHARS).collect())
}

/// Title of the session `req` opens: its first message for a voice or records session, else its machine.
pub fn session_title(req: &AssistRequest) -> Option<String> {
    if !is_general(&req.connection_string) {
        return match (&req.service_number, &req.hostname) {
            (Some(sn), Some(host)) => Some(format!("#{sn} {host}")),
            (Some(sn), None) => Some(format!("#{sn}")),
            (None, Some(host)) => Some(host.clone()),
            (None, None) => None,
        };
    }
    let asked = req
        .tech_note
        .as_deref()
        .filter(|_| req.trigger_source != "auto")
        .and_then(first_message);
    Some(match asked {
        Some(text) if is_voice(&req.connection_string) => {
            bounded_title(&format!("Voice \u{00b7} {text}"))
        }
        Some(text) => bounded_title(&text),
        None => format!(
            "General \u{00b7} {}",
            req.requested_by.as_deref().unwrap_or("technician")
        ),
    })
}

/// A request note as one line without the voice preamble, `[…]` context lines, code or backticks; `None` when blank or the opener.
fn first_message(note: &str) -> Option<String> {
    let note = note.trim();
    let body = match note.split_once("\n\n") {
        Some((first, rest)) if first.starts_with(VOICE_PREAMBLE) => rest,
        None if note.starts_with(VOICE_PREAMBLE) => "",
        _ => note,
    };
    let prose = body
        .split("```")
        .next()
        .unwrap_or_default()
        .replace('`', "");
    let text = prose
        .lines()
        .filter(|line| !is_context_line(line))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty() && text != OPENER_NOTE).then_some(text)
}

/// A whole line in brackets, such as the command bar's `[Viewing task …]`.
fn is_context_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('[') && line.ends_with(']')
}

/// `title` cut to [`TITLE_MAX_CHARS`] characters, ending in an ellipsis when cut.
fn bounded_title(title: &str) -> String {
    if title.chars().count() <= TITLE_MAX_CHARS {
        return title.to_string();
    }
    let cut: String = title.chars().take(TITLE_MAX_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// What a thread's agent is doing, as the broker last recorded it in `agent_thread.activity`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentActivity {
    #[default]
    Idle,
    Starting,
    Thinking,
    Writing,
    Tool(String),
    Command,
    Compacting,
    Approval(String),
    Retrying,
}

impl AgentActivity {
    /// The stored form: `idle`, `thinking`, `tool:<name>`, `approval:<name>` and so on.
    pub fn to_db(&self) -> String {
        match self {
            Self::Idle => "idle".into(),
            Self::Starting => "starting".into(),
            Self::Thinking => "thinking".into(),
            Self::Writing => "writing".into(),
            Self::Tool(name) => format!("tool:{name}"),
            Self::Command => "command".into(),
            Self::Compacting => "compacting".into(),
            Self::Approval(name) => format!("approval:{name}"),
            Self::Retrying => "retrying".into(),
        }
    }

    /// Reads the stored form; anything unrecognised reads as idle.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if let Some(name) = raw.strip_prefix("tool:") {
            return Self::Tool(name.to_string());
        }
        if let Some(name) = raw.strip_prefix("approval:") {
            return Self::Approval(name.to_string());
        }
        match raw {
            "starting" => Self::Starting,
            "thinking" => Self::Thinking,
            "writing" => Self::Writing,
            "command" => Self::Command,
            "compacting" => Self::Compacting,
            "retrying" => Self::Retrying,
            _ => Self::Idle,
        }
    }

    /// Words for a status line.
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "Idle".into(),
            Self::Starting => "Starting".into(),
            Self::Thinking => "Thinking".into(),
            Self::Writing => "Writing".into(),
            Self::Tool(name) if name.is_empty() => "Running a tool".into(),
            Self::Tool(name) => format!("Running {name}"),
            Self::Command => "Running a command".into(),
            Self::Compacting => "Compacting".into(),
            Self::Approval(name) if name.is_empty() => "Needs approval".into(),
            Self::Approval(name) if name == "question" => "Waiting for an answer".into(),
            Self::Approval(name) => format!("Needs approval: {name}"),
            Self::Retrying => "Retrying".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentThread {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub connection_string: String,
    #[serde(default)]
    #[surreal(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub requested_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub assignee: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub assist_request: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub service_order: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub computer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub customer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub diagnostic_session: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub codex_thread_id: Option<String>,
    /// [`tools_hash`] of the tool specs the codex thread started with.
    #[serde(default)]
    #[surreal(default)]
    pub tools_hash: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub model: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub provider: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub driven_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub tool_path: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub title: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub error: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub broker_node: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub allow_box_shell: bool,
    /// Every tool call runs without asking; only the broker writes it.
    #[serde(default)]
    #[surreal(default)]
    pub approve_all: Option<bool>,
    #[serde(default)]
    #[surreal(default)]
    pub tokens_used: Option<i64>,
    #[serde(default)]
    #[surreal(default)]
    pub tokens_window: Option<i64>,
    #[serde(default)]
    #[surreal(default)]
    pub last_seq: Option<i64>,
    /// Stored [`AgentActivity`]; read it with [`AgentThread::activity`].
    #[serde(default)]
    #[surreal(default)]
    pub activity: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub updated_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub last_event_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub closed_at: Option<Datetime>,
}

/// Everything known about a session before codex has been asked to start it.
#[derive(Debug, Clone, Default)]
pub struct NewAgentThread {
    pub assist_request: Option<RecordId>,
    pub connection_string: String,
    pub hostname: Option<String>,
    pub service_number: Option<String>,
    pub store: Option<String>,
    /// Email of the tech who asked; resolved to `assignee` on insert.
    pub requested_by: Option<String>,
    pub service_order: Option<RecordId>,
    pub computer: Option<RecordId>,
    pub customer: Option<RecordId>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub driven_by: Option<String>,
    pub tool_path: Option<String>,
    pub broker_node: Option<String>,
    pub title: Option<String>,
}

/// Thread-row fields written together; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentThreadState {
    pub status: Option<String>,
    pub error: Option<String>,
    pub last_seq: Option<i64>,
    pub tokens_used: Option<i64>,
    pub tokens_window: Option<i64>,
    pub activity: Option<String>,
}

impl AgentThreadState {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// True for a starting or running write without an error; saving it clears the stored error.
    pub fn clears_error(&self) -> bool {
        self.error.is_none() && matches!(self.status.as_deref(), Some("starting" | "running"))
    }
}

/// Connection string of a technician's session with no machine in scope.
pub fn general_connection(email: &str) -> String {
    format!("general:{}", email.trim().to_lowercase())
}

/// A session with no machine in scope: records-only tools, no remote actions.
pub fn is_general(connection_string: &str) -> bool {
    connection_string.starts_with("general:")
}

/// First 16 hex digits of the SHA-256 of the serialized tool specs.
pub fn tools_hash(specs: &[serde_json::Value]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(serde_json::to_vec(specs).unwrap_or_default());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A voice-console session: `general:voice:<email>`, or `general:voice:<guest>:<ms>` per utterance.
pub fn is_voice(connection_string: &str) -> bool {
    connection_string.starts_with("general:voice:")
}

impl AgentThread {
    pub fn is_open(&self) -> bool {
        AGENT_THREAD_OPEN_STATUSES.contains(&self.status.as_str())
    }

    /// Service number when known, else the machine.
    pub fn label(&self) -> String {
        match (&self.title, &self.service_number) {
            (Some(t), _) if !t.trim().is_empty() => t.clone(),
            (_, Some(sn)) => format!("#{sn} {}", self.hostname.clone().unwrap_or_default()).trim().to_string(),
            _ => self.connection_string.clone(),
        }
    }

    /// The rename when there is one, else `Customer Name - SN` once `customer` is known, else [`Self::label`].
    pub fn list_label(&self, customer: Option<&str>) -> String {
        if self.title.as_deref().is_some_and(|t| !t.trim().is_empty()) {
            return self.label();
        }
        let customer = customer.map(str::trim).filter(|c| !c.is_empty());
        match (customer, self.service_number.as_deref().map(str::trim).filter(|sn| !sn.is_empty())) {
            (Some(name), Some(sn)) => customer_label(name, sn),
            _ => self.label(),
        }
    }

    /// `context 45% · 58k/124k`, once both token counts are known.
    pub fn context_usage(&self) -> Option<String> {
        let used = self.tokens_used.filter(|u| *u >= 0)?;
        let window = self.tokens_window.filter(|w| *w > 0)?;
        Some(format!(
            "context {}% \u{00b7} {}/{}",
            used * 100 / window,
            compact_tokens(used),
            compact_tokens(window)
        ))
    }

    /// Used and window token counts, once both are known.
    pub fn context_tokens(&self) -> Option<(i64, i64)> {
        let used = self.tokens_used.filter(|u| *u >= 0)?;
        let window = self.tokens_window.filter(|w| *w > 0)?;
        Some((used, window))
    }

    /// Share of the context window in use, from 0 up; above 1 when the count overran the window.
    pub fn context_fraction(&self) -> Option<f32> {
        self.context_tokens()
            .map(|(used, window)| used as f32 / window as f32)
    }

    /// True while a turn runs, waits on a decision, or the session is starting its first one.
    pub fn is_busy(&self) -> bool {
        matches!(
            self.status.as_str(),
            "starting" | "running" | "waiting_approval"
        )
    }

    /// True while busy or queued for a pool slot.
    pub fn is_working(&self) -> bool {
        AGENT_THREAD_WORKING_STATUSES.contains(&self.status.as_str())
    }

    /// The error the last turn ended on, while the session sits idle after it.
    pub fn failed_turn_error(&self) -> Option<&str> {
        if self.status != "idle" {
            return None;
        }
        self.error.as_deref().map(str::trim).filter(|e| !e.is_empty())
    }

    /// The recorded activity; idle while no turn runs.
    pub fn activity(&self) -> AgentActivity {
        match self.status.as_str() {
            "running" | "waiting_approval" => self
                .activity
                .as_deref()
                .map(AgentActivity::parse)
                .unwrap_or(AgentActivity::Thinking),
            "starting" => AgentActivity::Starting,
            _ => AgentActivity::Idle,
        }
    }

    /// True while every tool call on the thread runs without asking.
    pub fn approves_all(&self) -> bool {
        self.approve_all == Some(true)
    }

    /// Records whether every tool call on the thread runs without asking.
    pub async fn set_approve_all(id: &RecordId, on: bool) -> anyhow::Result<()> {
        db().query("UPDATE $id SET approve_all = $on, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("on", on))
            .await?
            .check()?;
        Ok(())
    }

    /// Stores a new title.
    pub async fn set_title(id: &RecordId, title: &str) -> anyhow::Result<()> {
        db().query("UPDATE $id SET title = $title, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("title", title.to_string()))
            .await?
            .check()?;
        Ok(())
    }

    /// Inserts a `starting` row; the assignee is the user whose email matches the requester, ignoring case.
    pub async fn create(new: &NewAgentThread) -> anyhow::Result<RecordId> {
        let mut res = db()
            .query(CREATE_THREAD_SQL)
            .bind(("assist_request", new.assist_request.clone()))
            .bind(("cs", new.connection_string.clone()))
            .bind(("hostname", new.hostname.clone()))
            .bind(("sn", new.service_number.clone()))
            .bind(("store", new.store.clone()))
            .bind(("requested_by", new.requested_by.clone()))
            .bind(("service_order", new.service_order.clone()))
            .bind(("computer", new.computer.clone()))
            .bind(("customer", new.customer.clone()))
            .bind(("model", new.model.clone()))
            .bind(("provider", new.provider.clone()))
            .bind(("driven_by", new.driven_by.clone()))
            .bind(("tool_path", new.tool_path.clone()))
            .bind(("broker_node", new.broker_node.clone()))
            .bind(("title", new.title.clone()))
            .await?;
        let ids: Vec<RecordId> = res.take(1).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("agent_thread was not created"))
    }

    /// Whether the requester with email `requested_by` may send turns into `thread`.
    pub async fn may_steer(thread: &RecordId, requested_by: Option<&str>) -> anyhow::Result<bool> {
        let allowed: Option<bool> = db()
            .query(MAY_STEER_SQL)
            .bind(("thread", thread.clone()))
            .bind(("by", requested_by.map(str::to_string)))
            .await?
            .check()?
            .take(1)?;
        Ok(allowed.unwrap_or(false))
    }

    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// Records the codex thread and the [`tools_hash`] of the specs it started with.
    pub async fn set_codex_thread(id: &RecordId, codex_thread_id: &str, tools_hash: &str) -> anyhow::Result<()> {
        db().query("UPDATE $id SET codex_thread_id = $t, tools_hash = $h, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("t", codex_thread_id.to_string()))
            .bind(("h", tools_hash.to_string()))
            .await?;
        Ok(())
    }

    /// Whether a general session at rest started its codex thread with tool specs other than `current`.
    pub fn tools_outdated(&self, current: &str) -> bool {
        is_general(&self.connection_string)
            && !matches!(self.status.as_str(), "running" | "waiting_approval")
            && self.tools_hash.as_deref() != Some(current)
    }

    /// Moves the thread to `status`; terminal statuses also stamp `closed_at`.
    pub async fn set_status(id: &RecordId, status: &str, error: Option<&str>) -> anyhow::Result<()> {
        let state = AgentThreadState {
            status: Some(status.to_string()),
            error: error.map(str::to_string),
            ..Default::default()
        };
        Self::save_state(id, &state).await
    }

    /// Writes the set fields of `state` in one update; `last_seq` never moves backwards.
    pub async fn save_state(id: &RecordId, state: &AgentThreadState) -> anyhow::Result<()> {
        let terminal = matches!(state.status.as_deref(), Some("closed" | "failed"));
        db().query(
            "UPDATE $id SET status = $status ?? status, \
             error = IF $clear_error THEN NONE ELSE ($error ?? error) END, \
             closed_at = IF $terminal THEN time::now() ELSE closed_at END, \
             last_seq = IF $seq != NONE THEN math::max([last_seq ?? 0, $seq]) ELSE last_seq END, \
             last_event_at = IF $seq != NONE THEN time::now() ELSE last_event_at END, \
             tokens_used = $used ?? tokens_used, tokens_window = $window ?? tokens_window, \
             activity = $activity ?? activity, updated_at = time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("status", state.status.clone()))
        .bind(("error", state.error.as_ref().map(|e| e.chars().take(800).collect::<String>())))
        .bind(("clear_error", state.clears_error()))
        .bind(("terminal", terminal))
        .bind(("seq", state.last_seq))
        .bind(("used", state.tokens_used))
        .bind(("window", state.tokens_window))
        .bind(("activity", state.activity.clone()))
        .await?
        .check()?;
        Ok(())
    }

    pub async fn set_diagnostic_session(id: &RecordId, session: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE $id SET diagnostic_session = $ds, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("ds", session.clone()))
            .await?;
        Ok(())
    }

    /// Fills the service order, customer and service number a machine's open thread lacks; returns the threads written.
    pub async fn adopt_service_links(
        connection_string: &str,
        service_number: &str,
        service_order: &RecordId,
        customer: Option<&RecordId>,
    ) -> anyhow::Result<usize> {
        let mut res = db()
            .query(ADOPT_THREAD_LINKS_SQL)
            .bind(("cs", connection_string.to_string()))
            .bind(("sn", service_number.to_string()))
            .bind(("so", service_order.clone()))
            .bind(("cust", customer.cloned()))
            .await?;
        let ids: Vec<RecordId> = res.take(0)?;
        Ok(ids.len())
    }

    /// Threads the broker should be attached to, oldest first.
    pub async fn open_threads() -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE status NOT IN ['closed', 'failed'] \
                 ORDER BY created_at ASC",
            )
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// The newest open thread for `connection_string` that is working or was opened or asked something in the last 30 minutes.
    pub async fn resumable_for_connection(connection_string: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(RESUMABLE_SQL)
            .bind(("cs", connection_string.to_string()))
            .bind(("working", AGENT_THREAD_WORKING_STATUSES.map(String::from).to_vec()))
            .await?;
        let rows: Vec<Self> = res.take(0)?;
        Ok(rows.into_iter().next())
    }

    /// The live thread for a machine, if one exists.
    pub async fn active_for_connection(connection_string: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE connection_string = $cs \
                 AND status NOT IN ['closed', 'failed'] ORDER BY created_at DESC LIMIT 1",
            )
            .bind(("cs", connection_string.to_string()))
            .await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// The session to reopen when MasterTech starts on this machine; see [`REOPEN_SQL`].
    pub async fn reopen_for_connection(connection_string: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(REOPEN_SQL)
            .bind(("cs", connection_string.to_string()))
            .await?;
        let rows: Vec<Self> = res.take(0)?;
        Ok(rows.into_iter().next())
    }

    /// The newest working thread for a machine, other than `except`.
    pub async fn working_for_connection(
        connection_string: &str,
        except: Option<&RecordId>,
    ) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE connection_string = $cs AND status IN $working \
                 AND ($except = NONE OR id != $except) ORDER BY created_at DESC LIMIT 1",
            )
            .bind(("cs", connection_string.to_string()))
            .bind(("working", AGENT_THREAD_WORKING_STATUSES.map(String::from).to_vec()))
            .bind(("except", except.cloned()))
            .await?;
        let rows: Vec<Self> = res.take(0)?;
        Ok(rows.into_iter().next())
    }

    /// Every thread of a machine that is not closed or failed.
    pub async fn open_for_connection(connection_string: &str) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM agent_thread WHERE connection_string = $cs AND status NOT IN ['closed', 'failed']")
            .bind(("cs", connection_string.to_string()))
            .await?;
        Ok(res.take(0)?)
    }

    /// The newest thread for a machine regardless of status.
    pub async fn latest_for_connection(connection_string: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE connection_string = $cs \
                 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(("cs", connection_string.to_string()))
            .await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// A machine's threads regardless of status, newest first.
    pub async fn list_for_connection(connection_string: &str, limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE connection_string = $cs \
                 ORDER BY created_at DESC LIMIT $limit",
            )
            .bind(("cs", connection_string.to_string()))
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0)?)
    }

    /// Deletes the transcript, turns and approvals of threads closed longer ago
    /// than `retention` (a duration such as `30d`); the thread rows stay, stamped `purged_at`.
    pub async fn purge_closed_before(retention: &str) -> anyhow::Result<usize> {
        let mut res = db()
            .query(
                "LET $old = (SELECT VALUE id FROM agent_thread WHERE status IN ['closed', 'failed'] \
                 AND purged_at = NONE AND closed_at != NONE \
                 AND closed_at < time::now() - type::duration($retention)); \
                 DELETE agent_event WHERE thread IN $old; \
                 DELETE agent_turn WHERE thread IN $old; \
                 DELETE agent_approval WHERE thread IN $old; \
                 UPDATE agent_thread SET purged_at = time::now() WHERE id IN $old; \
                 RETURN array::len($old);",
            )
            .bind(("retention", retention.to_string()))
            .await?;
        let purged: Option<i64> = res.take(5).unwrap_or_default();
        Ok(purged.unwrap_or(0).max(0) as usize)
    }

    /// Newest threads for a session list; `include_closed` widens past the live ones.
    pub async fn list_recent(limit: usize, include_closed: bool) -> anyhow::Result<Vec<Self>> {
        let sql = if include_closed {
            "SELECT * FROM agent_thread ORDER BY created_at DESC LIMIT $limit"
        } else {
            "SELECT * FROM agent_thread WHERE status NOT IN ['closed', 'failed'] \
             ORDER BY created_at DESC LIMIT $limit"
        };
        let mut res = db().query(sql).bind(("limit", limit)).await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// `LIVE SELECT` over the signed-in technician's threads.
    pub fn live_query_for_signed_in_tech() -> String {
        format!("LIVE SELECT * FROM agent_thread WHERE {SIGNED_IN_TECH_THREADS}")
    }

    /// The signed-in technician's threads that are open or changed in the last hour, newest first.
    pub async fn list_for_signed_in_tech(limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(format!(
                "SELECT * FROM agent_thread WHERE {SIGNED_IN_TECH_THREADS} \
                 AND (status NOT IN ['closed', 'failed'] OR updated_at > time::now() - 1h) \
                 ORDER BY updated_at DESC LIMIT $limit"
            ))
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(used: Option<i64>, window: Option<i64>) -> AgentThread {
        AgentThread {
            id: RecordId::new(AGENT_THREAD_TABLE, "t"),
            status: "idle".to_string(),
            connection_string: String::new(),
            hostname: None,
            service_number: None,
            store: None,
            requested_by: None,
            assignee: None,
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            tools_hash: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
            approve_all: None,
            tokens_used: used,
            tokens_window: window,
            last_seq: None,
            activity: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    #[test]
    fn list_labels_name_the_customer_like_a_task() {
        let mut row = thread(None, None);
        row.connection_string = "DESKTOP-VD05O1K:6003b2f63".into();
        row.hostname = Some("DESKTOP-VD05O1K".into());
        assert_eq!(row.list_label(Some("Martin Empey")), "DESKTOP-VD05O1K:6003b2f63", "no service number");
        row.service_number = Some("2141021".into());
        assert_eq!(row.list_label(None), "#2141021 DESKTOP-VD05O1K");
        assert_eq!(row.list_label(Some(" ")), "#2141021 DESKTOP-VD05O1K");
        assert_eq!(row.list_label(Some(" Martin Empey ")), "Martin Empey - 2141021");
        row.title = Some("Empey data move".into());
        assert_eq!(row.list_label(Some("Martin Empey")), "Empey data move", "a rename wins");
    }

    #[test]
    fn a_general_session_at_rest_is_outdated_when_its_tools_hash_differs() {
        let specs = vec![serde_json::json!({ "name": "search_odoo_inventory", "description": "old" })];
        let current = tools_hash(&specs);
        assert_eq!(current.len(), 16);
        assert_eq!(current, tools_hash(&specs.clone()));
        assert_ne!(current, tools_hash(&[serde_json::json!({ "name": "search_odoo_inventory", "description": "new" })]));

        let mut row = thread(None, None);
        row.connection_string = general_connection("tyler.naylor@pclaptops.com");
        assert!(row.tools_outdated(&current), "a thread from before the hash was kept");
        row.tools_hash = Some(current.clone());
        assert!(!row.tools_outdated(&current));
        row.tools_hash = Some("0123456789abcdef".into());
        assert!(row.tools_outdated(&current));
        row.status = "running".into();
        assert!(!row.tools_outdated(&current), "a turn in progress keeps its thread");
        row.status = "idle".into();
        row.connection_string = "DESKTOP-K8U909G:77d56bd67".into();
        assert!(!row.tools_outdated(&current), "machine sessions keep their thread");
    }

    #[test]
    fn only_an_idle_session_reports_its_failed_turn() {
        let mut row = thread(None, None);
        assert_eq!(row.failed_turn_error(), None);
        row.error = Some(" exceeded retry limit, last status: 429 ".into());
        assert_eq!(row.failed_turn_error(), Some("exceeded retry limit, last status: 429"));
        row.error = Some("  ".into());
        assert_eq!(row.failed_turn_error(), None);
        row.error = Some("codex daemon: refused".into());
        row.status = "failed".into();
        assert_eq!(row.failed_turn_error(), None);
    }

    #[test]
    fn a_row_written_before_approve_all_existed_still_loads() {
        use surrealdb::types::Value;
        let mut row = thread(None, None);
        row.approve_all = Some(true);
        let mut v = row.into_value();
        if let Value::Object(obj) = &mut v {
            obj.remove("approve_all");
        }
        let back = AgentThread::from_value(v).expect("a row without approve_all must deserialize");
        assert_eq!(back.approve_all, None);
        assert!(!back.approves_all());
        let mut json = serde_json::to_value(thread(None, None)).expect("serializes");
        json.as_object_mut().expect("object").remove("approve_all");
        let back: AgentThread = serde_json::from_value(json).expect("serde fills a missing approve_all");
        assert_eq!(back.approve_all, None);
    }

    #[test]
    fn a_row_written_before_activity_existed_still_loads() {
        use surrealdb::types::Value;
        let mut row = thread(Some(1), Some(2));
        row.activity = Some("thinking".into());
        let mut v = row.into_value();
        if let Value::Object(obj) = &mut v {
            obj.remove("activity");
        }
        let back = AgentThread::from_value(v).expect("a row without activity must deserialize");
        assert_eq!(back.activity, None);
        assert_eq!(
            back.activity(),
            AgentActivity::Idle,
            "an idle row reads idle"
        );
    }

    #[test]
    fn activities_round_trip_through_their_stored_form() {
        for a in [
            AgentActivity::Idle,
            AgentActivity::Starting,
            AgentActivity::Thinking,
            AgentActivity::Writing,
            AgentActivity::Tool("get_client_info".into()),
            AgentActivity::Command,
            AgentActivity::Compacting,
            AgentActivity::Approval("remote_exec_start".into()),
            AgentActivity::Retrying,
        ] {
            assert_eq!(AgentActivity::parse(&a.to_db()), a);
        }
        assert_eq!(AgentActivity::parse("something new"), AgentActivity::Idle);
        assert_eq!(AgentActivity::Tool("x".into()).label(), "Running x");
    }

    #[test]
    fn a_running_row_reads_its_activity_and_an_idle_one_does_not() {
        let mut row = thread(None, None);
        row.status = "running".into();
        row.activity = Some("tool:scripts_list".into());
        assert_eq!(row.activity(), AgentActivity::Tool("scripts_list".into()));
        row.activity = None;
        assert_eq!(row.activity(), AgentActivity::Thinking);
        row.status = "idle".into();
        row.activity = Some("writing".into());
        assert_eq!(row.activity(), AgentActivity::Idle);
        assert!(!row.is_busy());
    }

    #[test]
    fn working_covers_busy_and_queued_threads_only() {
        let mut row = thread(None, None);
        for (status, working) in [
            ("queued", true),
            ("starting", true),
            ("running", true),
            ("waiting_approval", true),
            ("idle", false),
            ("closed", false),
            ("failed", false),
        ] {
            row.status = status.into();
            assert_eq!(row.is_working(), working, "{status}");
        }
    }

    #[test]
    fn the_context_fraction_needs_both_counts() {
        assert_eq!(thread(Some(50), Some(200)).context_fraction(), Some(0.25));
        assert_eq!(thread(None, Some(200)).context_fraction(), None);
        assert_eq!(thread(Some(50), Some(0)).context_fraction(), None);
    }

    #[test]
    fn titles_are_one_trimmed_line_of_bounded_length() {
        assert_eq!(clean_title("  Disk\n check  "), Some("Disk check".into()));
        assert_eq!(clean_title(" \n "), None);
        assert_eq!(
            clean_title(&"x".repeat(200)).map(|t| t.chars().count()),
            Some(TITLE_MAX_CHARS)
        );
    }

    const VOICE_NOTE: &str = "Voice mode: reply in one or two short spoken sentences for text-to-speech; \
         use tools only if necessary.\n\nWhat is the difference between an SSD and hard drive?";

    fn request(
        connection_string: &str,
        requested_by: Option<&str>,
        note: Option<&str>,
    ) -> AssistRequest {
        AssistRequest {
            id: RecordId::new("assist_request", "r"),
            status: "dispatched".into(),
            trigger_source: "chat".into(),
            machine_confirmed: false,
            connection_string: connection_string.into(),
            hostname: None,
            service_number: None,
            service_order: None,
            computer: None,
            customer: None,
            requested_by: requested_by.map(str::to_string),
            store: None,
            tech_note: note.map(str::to_string),
            agent: None,
            dispatch_error: None,
            agent_thread: None,
            fresh: false,
            filed_access: Some("user".into()),
        }
    }

    #[test]
    fn a_voice_session_is_titled_by_its_question() {
        let req = request("general:voice:guest:1790000000000", None, Some(VOICE_NOTE));
        assert_eq!(
            session_title(&req).as_deref(),
            Some("Voice \u{00b7} What is the difference between an SSD and hard drive?")
        );
        assert!(is_voice(&req.connection_string) && is_general(&req.connection_string));
        assert!(!is_voice("general:logan.lees@pclaptops.com"));
        assert!(!is_voice("JeffsComputer:663a3fd40"));
    }

    #[test]
    fn a_records_session_is_titled_by_its_first_message_on_one_line() {
        let tech = "jacob.hardy@pclaptops.com";
        let cs = general_connection(tech);
        let title = |note: &str| session_title(&request(&cs, Some(tech), Some(note)));
        assert_eq!(
            title("  What parts were\n ordered   for SO 2155485?  ").as_deref(),
            Some("What parts were ordered for SO 2155485?")
        );
        let with_context = "[Viewing task \"JeffsComputer\", service 2155485, task id t1]\n\
            [Focused client PC-1:ab12]\nIs the warranty still active?";
        assert_eq!(
            title(with_context).as_deref(),
            Some("Is the warranty still active?")
        );
        let with_code = "Why does `sfc` fail here?\n```log\nerror 0x80070005\n```";
        assert_eq!(title(with_code).as_deref(), Some("Why does sfc fail here?"));
        let mentions_voice = "Check the mic.\n\nVoice mode: is it on?";
        assert_eq!(
            title(mentions_voice).as_deref(),
            Some("Check the mic. Voice mode: is it on?")
        );
    }

    #[test]
    fn a_session_without_a_usable_first_message_keeps_the_general_title() {
        let cs = general_connection("t@x.com");
        for note in [
            None,
            Some("  \n "),
            Some(OPENER_NOTE),
            Some("Voice mode: reply briefly."),
            Some("```\ncode only\n```"),
        ] {
            assert_eq!(
                session_title(&request(&cs, Some("t@x.com"), note)).as_deref(),
                Some("General \u{00b7} t@x.com"),
                "{note:?}"
            );
        }
        let guest = request(
            "general:voice:guest:1790000000000",
            None,
            Some("Voice mode: reply briefly.\n\n  "),
        );
        assert_eq!(
            session_title(&guest).as_deref(),
            Some("General \u{00b7} technician")
        );
        let mut auto = request(&cs, Some("t@x.com"), Some("Summarize the waiting queue"));
        auto.trigger_source = "auto".into();
        assert_eq!(
            session_title(&auto).as_deref(),
            Some("General \u{00b7} t@x.com")
        );
    }

    #[test]
    fn derived_titles_stay_within_the_title_limit() {
        let long = format!(
            "{VOICE_PREAMBLE} be brief.\n\n{}",
            "why is the fan so loud ".repeat(10)
        );
        let title =
            session_title(&request("general:voice:guest:1", None, Some(&long))).expect("a title");
        assert!(title.chars().count() <= TITLE_MAX_CHARS, "{title}");
        assert!(
            title.starts_with("Voice \u{00b7} why is the fan so loud"),
            "{title}"
        );
        assert!(
            title.ends_with('\u{2026}') && !title.ends_with(" \u{2026}"),
            "{title}"
        );
        let wide = "\u{00e9}t\u{00e9} ".repeat(60);
        let title = session_title(&request("general:t@x.com", None, Some(&wide))).expect("a title");
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS);
        let exact = "x".repeat(TITLE_MAX_CHARS);
        assert_eq!(
            session_title(&request("general:t@x.com", None, Some(&exact))),
            Some(exact)
        );
    }

    #[test]
    fn machine_sessions_are_titled_by_service_number_and_host() {
        let mut req = request(
            "JeffsComputer:663a3fd40",
            Some("t@x.com"),
            Some("why is it slow"),
        );
        req.service_number = Some("2155485".into());
        req.hostname = Some("JeffsComputer".into());
        assert_eq!(
            session_title(&req).as_deref(),
            Some("#2155485 JeffsComputer")
        );
        req.hostname = None;
        assert_eq!(session_title(&req).as_deref(), Some("#2155485"));
        req.service_number = None;
        req.hostname = Some("JeffsComputer".into());
        assert_eq!(session_title(&req).as_deref(), Some("JeffsComputer"));
        req.hostname = None;
        assert_eq!(session_title(&req), None);
    }

    #[test]
    fn context_usage_reads_percent_and_thousands() {
        assert_eq!(
            thread(Some(58_000), Some(124_518)).context_usage().as_deref(),
            Some("context 46% \u{00b7} 58k/124k")
        );
    }

    #[test]
    fn context_usage_keeps_small_and_million_counts_readable() {
        assert_eq!(
            thread(Some(640), Some(1_048_576)).context_usage().as_deref(),
            Some("context 0% \u{00b7} 640/1.0M")
        );
    }

    #[test]
    fn context_usage_needs_both_counts() {
        assert_eq!(thread(None, Some(124_518)).context_usage(), None);
        assert_eq!(thread(Some(58_000), None).context_usage(), None);
        assert_eq!(thread(Some(58_000), Some(0)).context_usage(), None);
    }

    fn state(status: Option<&str>, error: Option<&str>) -> AgentThreadState {
        AgentThreadState {
            status: status.map(str::to_string),
            error: error.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn starting_or_running_again_clears_the_last_error() {
        assert!(state(Some("running"), None).clears_error());
        assert!(state(Some("starting"), None).clears_error());
        assert!(!state(Some("idle"), None).clears_error());
        assert!(!state(Some("running"), Some("429 Too Many Requests")).clears_error());
        assert!(!state(None, None).clears_error());
    }

    #[test]
    fn an_empty_state_writes_nothing() {
        assert!(AgentThreadState::default().is_empty());
        let seq_only = AgentThreadState { last_seq: Some(3), ..Default::default() };
        assert!(!seq_only.is_empty());
    }
}
