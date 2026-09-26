//! One Codex agent session about one customer machine.
//!
//! The admin-agent broker owns every row: it creates one when a tech accepts the
//! AI-help offer, records the codex thread id so a restart can resume the thread,
//! and keeps `status` current so any Mastertech instance can list live sessions
//! without talking to codex.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const AGENT_THREAD_TABLE: &str = "agent_thread";

/// Statuses a thread moves through; `closed` and `failed` are terminal.
pub const AGENT_THREAD_OPEN_STATUSES: [&str; 5] =
    ["queued", "starting", "idle", "running", "waiting_approval"];

/// Threads of the signed-in technician: assigned to them, or asked for with their email.
const SIGNED_IN_TECH_THREADS: &str =
    "$auth != NONE AND (assignee = $auth.id OR requested_by = $auth.email)";

/// Longest title a rename stores, in characters.
const TITLE_MAX_CHARS: usize = 80;

/// Fills the unset service order, customer and service number of `$cs`'s open threads on `$sn` or none.
pub const ADOPT_THREAD_LINKS_SQL: &str = "UPDATE agent_thread SET service_order = service_order ?? $so, \
     customer = customer ?? $cust, service_number = service_number ?? $sn, updated_at = time::now() \
     WHERE connection_string = $cs AND status NOT IN ['closed', 'failed'] \
     AND (service_number = NONE OR service_number = $sn) \
     AND (service_order = NONE OR (customer = NONE AND $cust != NONE)) \
     RETURN VALUE id";

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

    /// Stores a new title.
    pub async fn set_title(id: &RecordId, title: &str) -> anyhow::Result<()> {
        db().query("UPDATE $id SET title = $title, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("title", title.to_string()))
            .await?
            .check()?;
        Ok(())
    }

    /// Inserts a `starting` row; the assignee is the user whose email matches the requester.
    pub async fn create(new: &NewAgentThread) -> anyhow::Result<RecordId> {
        let mut res = db()
            .query(
                "LET $assignee = (SELECT VALUE id FROM user WHERE email = $requested_by LIMIT 1)[0]; \
                 CREATE agent_thread CONTENT { status: 'starting', assist_request: $assist_request, \
                 connection_string: $cs, hostname: $hostname, service_number: $sn, store: $store, \
                 requested_by: $requested_by, assignee: $assignee, service_order: $service_order, \
                 computer: $computer, customer: $customer, model: $model, provider: $provider, \
                 driven_by: $driven_by, tool_path: $tool_path, broker_node: $broker_node, title: $title, \
                 updated_at: time::now() } RETURN VALUE id",
            )
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

    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    pub async fn set_codex_thread(id: &RecordId, codex_thread_id: &str) -> anyhow::Result<()> {
        db().query("UPDATE $id SET codex_thread_id = $t, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("t", codex_thread_id.to_string()))
            .await?;
        Ok(())
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
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
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
