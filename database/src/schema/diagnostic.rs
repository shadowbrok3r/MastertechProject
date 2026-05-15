use crate::DATABASE;
use serde::{Deserialize, Serialize};
use super::{Datetime, RecordId, SurrealValue};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct PluginUsageRef {
    pub plugin_id: String,
    pub tool_name: String,
}

/// Structured category for a `DiagnosticEntry`. Replaces the old freeform
/// category string so the AI is constrained to a known vocabulary.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[surreal(untagged)]
pub enum DiagnosticCategory {
    /// A discovered issue (errors found, anomalies, root causes)
    Finding,
    /// A step taken (script run, change applied, fix attempted)
    Action,
    /// A free-form observation that isn't a finding or an action
    Note,
    /// A hard error encountered during diagnosis (tool failed, command crashed)
    Error,
    /// Snapshot of system specs / hardware state
    SystemInfo,
    /// Network configuration, connectivity test results
    NetworkInfo,
    /// Security-related alert (malware detected, suspicious process, etc.)
    SecurityAlert,
    /// Performance observation (slow disk, high CPU, etc.)
    PerformanceNote,
    /// Note captured from the customer (intake info, reported symptom)
    CustomerNote,
    /// Recommended next step / follow-up action for the tech or customer
    Recommendation,
}

impl Default for DiagnosticCategory {
    fn default() -> Self { Self::Note }
}

impl DiagnosticCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Finding => "finding",
            Self::Action => "action",
            Self::Note => "note",
            Self::Error => "error",
            Self::SystemInfo => "system_info",
            Self::NetworkInfo => "network_info",
            Self::SecurityAlert => "security_alert",
            Self::PerformanceNote => "performance_note",
            Self::CustomerNote => "customer_note",
            Self::Recommendation => "recommendation",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "finding" => Self::Finding,
            "action" => Self::Action,
            "error" => Self::Error,
            "system_info" | "systeminfo" | "system info" => Self::SystemInfo,
            "network_info" | "networkinfo" | "network info" => Self::NetworkInfo,
            "security_alert" | "securityalert" | "security alert" => Self::SecurityAlert,
            "performance_note" | "performancenote" | "performance note" => Self::PerformanceNote,
            "customer_note" | "customernote" | "customer note" => Self::CustomerNote,
            "recommendation" => Self::Recommendation,
            _ => Self::Note,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Finding,
            Self::Action,
            Self::Note,
            Self::Error,
            Self::SystemInfo,
            Self::NetworkInfo,
            Self::SecurityAlert,
            Self::PerformanceNote,
            Self::CustomerNote,
            Self::Recommendation,
        ]
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct DiagnosticSession {
    pub id: RecordId,
    pub connection_string: String,
    pub hostname: String,
    pub customer_name: Option<String>,
    /// Required: every diagnostic must belong to a known customer. The AI
    /// must look this up via MCP tools (e.g. `find_customer_by_email`,
    /// or via the `connected_client.computer.customer` graph) before
    /// creating a session.
    pub customer_id: RecordId,
    /// Required: every diagnostic must reference the computer being
    /// diagnosed. Resolve via `connected_client.computer` or
    /// `get_computer_details`.
    pub computer_id: RecordId,
    /// Optional link to the in-house task record this diagnostic
    /// corresponds to (set when the computer is checked in for service).
    pub task_ref: Option<RecordId>,
    /// Optional link to the PrestaShop / in-house service order this
    /// diagnostic corresponds to.
    pub service_order: Option<RecordId>,
    pub tech: Option<String>,
    pub started_at: Datetime,
    pub ended_at: Option<Datetime>,
    pub summary: Option<String>,
    pub status: String,
    pub tags: Vec<String>,
}

impl Default for DiagnosticSession {
    fn default() -> Self {
        Self {
            id: super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE),
            connection_string: String::new(),
            hostname: String::new(),
            customer_name: None,
            customer_id: super::random_record_id(super::CUSTOMER_TABLE),
            computer_id: super::random_record_id(super::COMPUTER_TABLE),
            task_ref: None,
            service_order: None,
            tech: None,
            started_at: chrono::Utc::now().into(),
            ended_at: None,
            summary: None,
            status: "open".to_string(),
            tags: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct DiagnosticEntry {
    pub id: RecordId,
    pub session_ref: RecordId,
    pub timestamp: Datetime,
    pub category: DiagnosticCategory,
    pub title: String,
    pub detail: String,
    pub data: Option<serde_json::Value>,
    pub plugins_used: Vec<PluginUsageRef>,
}

impl Default for DiagnosticEntry {
    fn default() -> Self {
        Self {
            id: super::random_record_id(super::DIAGNOSTIC_ENTRY_TABLE),
            session_ref: super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE),
            timestamp: chrono::Utc::now().into(),
            category: DiagnosticCategory::Note,
            title: String::new(),
            detail: String::new(),
            data: None,
            plugins_used: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiagnosticSessionFull {
    #[serde(flatten)]
    pub session: DiagnosticSession,
    pub entries: Vec<DiagnosticEntry>,
}

impl DiagnosticSession {
    pub async fn create(session: &Self) -> anyhow::Result<RecordId> {
        let mut s = session.clone();
        s.id = super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE);
        s.started_at = chrono::Utc::now().into();
        s.status = "open".to_string();

        let created: Option<Self> = DATABASE
            .create(s.id.clone())
            .content(s.clone())
            .await?;

        Ok(created.map(|c| c.id).unwrap_or(s.id))
    }

    pub async fn close(session_id: &str, status: &str, summary: &str, tags: Option<&[String]>) -> anyhow::Result<()> {
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
        let mut query_str = String::from(
            "UPDATE $sid SET status = $status, summary = $summary, ended_at = time::now()"
        );
        if tags.is_some() {
            query_str.push_str(", tags = $tags");
        }
        let mut q = DATABASE.query(&query_str)
            .bind(("sid", sid))
            .bind(("status", status.to_string()))
            .bind(("summary", summary.to_string()));
        if let Some(t) = tags {
            q = q.bind(("tags", t.to_vec()));
        }
        q.await?;
        Ok(())
    }

    pub async fn get_full(session_id: &str) -> anyhow::Result<Option<DiagnosticSessionFull>> {
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
        let session: Option<DiagnosticSession> = DATABASE.select(sid.clone()).await?;
        let Some(session) = session else { return Ok(None) };

        let entries: Vec<DiagnosticEntry> = DATABASE
            .query("SELECT * FROM diagnostic_entry WHERE session_ref == $sid ORDER BY timestamp ASC")
            .bind(("sid", sid))
            .await?
            .take(0)?;

        Ok(Some(DiagnosticSessionFull { session, entries }))
    }

    pub async fn list_all(start: i32) -> anyhow::Result<Vec<Self>> {
        let sessions: Vec<Self> = DATABASE
            .query("SELECT * FROM diagnostic_session ORDER BY started_at DESC LIMIT 200 START $start")
            .bind(("start", start))
            .await?
            .take(0)?;
        Ok(sessions)
    }

    /// Fetch every diagnostic session that's been recorded against a
    /// specific `connection_string`. Used by the Admin Console's
    /// per-client "Diagnostics" popup (the button on the My Tasks
    /// connected-client card), which only knows the connection string
    /// — not the task or computer id — when the user opens it.
    ///
    /// Returns up to 50 sessions, newest first.
    pub async fn list_for_connection(
        connection_string: &str,
    ) -> anyhow::Result<Vec<Self>> {
        let sql = "SELECT * FROM diagnostic_session \
                   WHERE connection_string == $cs \
                   ORDER BY started_at DESC LIMIT 50";
        let sessions: Vec<Self> = DATABASE
            .query(sql)
            .bind(("cs", connection_string.to_string()))
            .await?
            .take(0)?;
        Ok(sessions)
    }

    /// Fetch all diagnostic sessions linked to a task (via `task_ref`) or to
    /// the same `computer_id` the task references. Used by the task modal's
    /// Diagnostics tab to show every prior diagnosis for this machine.
    pub async fn list_for_task_or_computer(
        task_id: Option<&RecordId>,
        computer_id: Option<&RecordId>,
    ) -> anyhow::Result<Vec<Self>> {
        let mut conditions: Vec<&str> = Vec::new();
        if task_id.is_some() {
            conditions.push("task_ref == $task");
        }
        if computer_id.is_some() {
            conditions.push("computer_id == $computer");
        }
        if conditions.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT * FROM diagnostic_session WHERE {} ORDER BY started_at DESC LIMIT 50",
            conditions.join(" OR ")
        );
        let mut q = DATABASE.query(&sql);
        if let Some(t) = task_id { q = q.bind(("task", t.clone())); }
        if let Some(c) = computer_id { q = q.bind(("computer", c.clone())); }
        let sessions: Vec<Self> = q.await?.take(0)?;
        Ok(sessions)
    }

    /// Set the `task_ref` and optionally `service_order` on an existing
    /// diagnostic session. Used to retroactively link a diagnostic to a
    /// service ticket once the computer is checked in.
    pub async fn link_to_task(
        session_id: &RecordId,
        task_ref: Option<&RecordId>,
        service_order: Option<&RecordId>,
    ) -> anyhow::Result<()> {
        let mut sets: Vec<&str> = Vec::new();
        if task_ref.is_some() {
            sets.push("task_ref = $task");
        }
        if service_order.is_some() {
            sets.push("service_order = $svc");
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!("UPDATE $sid SET {}", sets.join(", "));
        let mut q = DATABASE.query(&sql).bind(("sid", session_id.clone()));
        if let Some(t) = task_ref { q = q.bind(("task", t.clone())); }
        if let Some(s) = service_order { q = q.bind(("svc", s.clone())); }
        q.await?;
        Ok(())
    }

    pub async fn search(
        query: &str,
        hostname: Option<&str>,
        customer_name: Option<&str>,
        connection_string: Option<&str>,
    ) -> anyhow::Result<Vec<Self>> {
        let q = query.to_lowercase();
        let mut conditions = vec![
            "(string::lowercase(summary ?? '') CONTAINS $q \
             OR string::lowercase(hostname) CONTAINS $q \
             OR string::lowercase(customer_name ?? '') CONTAINS $q \
             OR string::lowercase(connection_string) CONTAINS $q)".to_string()
        ];
        if hostname.is_some() {
            conditions.push("hostname == $host".to_string());
        }
        if customer_name.is_some() {
            conditions.push("string::lowercase(customer_name ?? '') CONTAINS $cust".to_string());
        }
        if connection_string.is_some() {
            conditions.push("connection_string == $conn".to_string());
        }
        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT * FROM diagnostic_session WHERE {where_clause} ORDER BY started_at DESC LIMIT 25"
        );

        let sessions: Vec<Self> = DATABASE
            .query(&sql)
            .bind(("q", q))
            .bind(("host", hostname.unwrap_or("").to_string()))
            .bind(("cust", customer_name.unwrap_or("").to_lowercase()))
            .bind(("conn", connection_string.unwrap_or("").to_string()))
            .await?
            .take(0)?;

        Ok(sessions)
    }
}

impl DiagnosticEntry {
    pub async fn list_all(start: i32) -> anyhow::Result<Vec<Self>> {
        let entries: Vec<Self> = DATABASE
            .query("SELECT * FROM diagnostic_entry ORDER BY timestamp DESC LIMIT 200 START $start")
            .bind(("start", start))
            .await?
            .take(0)?;
        Ok(entries)
    }

    pub async fn create(entry: &Self) -> anyhow::Result<RecordId> {
        let mut e = entry.clone();
        e.id = super::random_record_id(super::DIAGNOSTIC_ENTRY_TABLE);
        e.timestamp = chrono::Utc::now().into();

        let created: Option<Self> = DATABASE
            .create(e.id.clone())
            .content(e.clone())
            .await?;

        Ok(created.map(|c| c.id).unwrap_or(e.id))
    }
}
