use crate::db;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use super::{Datetime, RecordId, SurrealValue};
use surrealdb_types::{Kind, Value};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct PluginUsageRef {
    pub plugin_id: String,
    pub tool_name: String,
}

/// Structured category for a `DiagnosticEntry`. Replaces the old freeform
/// category string so the AI is constrained to a known vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

impl Serialize for DiagnosticCategory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DiagnosticCategory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_str(&s))
    }
}

impl SurrealValue for DiagnosticCategory {
    fn kind_of() -> Kind {
        Kind::String
    }

    fn into_value(self) -> Value {
        Value::String(self.as_str().to_string())
    }

    fn from_value(value: Value) -> Result<Self, surrealdb::Error>
    where
        Self: Sized,
    {
        match value {
            Value::String(s) => Ok(Self::from_str(&s)),
            other => Err(surrealdb::Error::validation(
                format!("DiagnosticCategory expected string, got {other:?}"),
                None,
            )),
        }
    }
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
    /// Customer the diagnostic belongs to; `NONE` on rows written before
    /// `create_diagnostic_session` required it.
    pub customer_id: Option<RecordId>,
    /// Computer being diagnosed; `NONE` on rows written before
    /// `create_diagnostic_session` required it.
    pub computer_id: Option<RecordId>,
    /// Optional link to the in-house task record this diagnostic
    /// corresponds to (set when the computer is checked in for service).
    pub task_ref: Option<RecordId>,
    /// Optional link to the PrestaShop / in-house service order this
    /// diagnostic corresponds to.
    pub service_order: Option<RecordId>,
    pub tech: Option<String>,
    pub started_at: Datetime,
    pub ended_at: Option<Datetime>,
    /// Diagnosis-complete milestone; sessions stay open through remediation,
    /// so this is distinct from `ended_at`. First stamp wins.
    #[serde(default)]
    #[surreal(default)]
    pub diagnosed_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub diagnosed_by: Option<String>,
    /// Who asked for the diagnostic (tech name, or "customer").
    #[serde(default)]
    #[surreal(default)]
    pub requested_by: Option<String>,
    /// PCL store code the machine belongs to.
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
    /// Surface driving the session: "desktop" or "zeroclaw:<alias>".
    #[serde(default)]
    #[surreal(default)]
    pub driven_by: Option<String>,
    /// Human verdict that supersedes the inferred outcome.
    #[serde(default)]
    #[surreal(default)]
    pub outcome_override: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub outcome_override_reason: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub outcome_override_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub outcome_override_at: Option<Datetime>,
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
            customer_id: None,
            computer_id: None,
            task_ref: None,
            service_order: None,
            tech: None,
            started_at: chrono::Utc::now().into(),
            ended_at: None,
            diagnosed_at: None,
            diagnosed_by: None,
            requested_by: None,
            store: None,
            driven_by: None,
            outcome_override: None,
            outcome_override_reason: None,
            outcome_override_by: None,
            outcome_override_at: None,
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
    /// 768-dim vector from `fn::embed_text(title + detail)` on insert
    /// (`DiagnosticEntry::create`). Populated on read; not sent empty on write.
    #[serde(default)]
    pub embedding: Vec<f32>,
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
            embedding: Vec::new(),
        }
    }
}

/// Days open after which a session is reported stale by the link reaper and
/// flagged in the diagnostics page.
pub const STALE_SESSION_DAYS: i64 = 30;

/// Open-session projection holding no record-id links, so a row with a
/// malformed FK still lists. `age_secs` is the age at query time.
#[derive(Serialize, Deserialize, Debug, Clone, SurrealValue)]
pub struct OpenSessionRef {
    pub id: RecordId,
    pub connection_string: String,
    pub hostname: String,
    pub tech: Option<String>,
    pub started_at: Datetime,
    pub age_secs: i64,
}

impl OpenSessionRef {
    pub fn age_days(&self) -> i64 {
        self.age_secs / 86_400
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiagnosticSessionFull {
    #[serde(flatten)]
    pub session: DiagnosticSession,
    pub entries: Vec<DiagnosticEntry>,
}

impl DiagnosticSession {
    /// Days open, for a still-open session past [`STALE_SESSION_DAYS`].
    pub fn stale_days(&self) -> Option<i64> {
        if self.status != "open" {
            return None;
        }
        let started = chrono::DateTime::<chrono::Utc>::from(self.started_at);
        let days = (chrono::Utc::now() - started).num_days();
        (days >= STALE_SESSION_DAYS).then_some(days)
    }

    pub async fn create(session: &Self) -> anyhow::Result<RecordId> {
        let mut s = session.clone();
        s.id = super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE);
        s.started_at = chrono::Utc::now().into();
        s.status = "open".to_string();

        let created: Option<Self> = db()
            .create(s.id.clone())
            .content(s.clone())
            .await?;

        Ok(created.map(|c| c.id).unwrap_or(s.id))
    }

    /// Other sessions still open on the same client, newest first.
    pub async fn other_open_for_client(
        connection_string: &str,
        exclude: &RecordId,
    ) -> anyhow::Result<Vec<String>> {
        use super::RecordIdExt;

        let mut res = db()
            .query(
                "SELECT id, started_at FROM diagnostic_session                  WHERE connection_string = $cs AND status = 'open' AND id != $exclude                  ORDER BY started_at DESC LIMIT 5",
            )
            .bind(("cs", connection_string.to_string()))
            .bind(("exclude", exclude.clone()))
            .await?;
        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        Ok(rows
            .iter()
            .filter_map(|r| r.get("id").and_then(serde_json::Value::as_str))
            .map(|s| {
                super::entity_link::parse_record_id(s, super::DIAGNOSTIC_SESSION_TABLE).key_string()
            })
            .collect())
    }

    pub async fn close(session_id: &str, status: &str, summary: &str, tags: Option<&[String]>) -> anyhow::Result<()> {
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
        let mut query_str = String::from(
            "UPDATE $sid SET status = $status, summary = $summary, ended_at = time::now()"
        );
        if tags.is_some() {
            query_str.push_str(", tags = $tags");
        }
        let dbh = db();
        let mut q = dbh.query(&query_str)
            .bind(("sid", sid))
            .bind(("status", status.to_string()))
            .bind(("summary", summary.to_string()));
        if let Some(t) = tags {
            q = q.bind(("tags", t.to_vec()));
        }
        q.await?;
        Ok(())
    }

    /// Stamps the diagnosis-complete milestone; the first stamp wins.
    /// Returns the session's diagnosed_at after the update.
    pub async fn mark_diagnosed(session_id: &str, by: &str) -> anyhow::Result<Option<Datetime>> {
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
        let mut res = db()
            .query(
                "UPDATE $sid SET diagnosed_at = diagnosed_at ?? time::now(), \
                 diagnosed_by = diagnosed_by ?? $by RETURN VALUE diagnosed_at",
            )
            .bind(("sid", sid))
            .bind(("by", by.to_string()))
            .await?;
        let at: Option<Datetime> = res.take::<Vec<Datetime>>(0)?.into_iter().next();
        Ok(at)
    }

    /// Marks a task as AI-worked; the first origin wins.
    pub async fn stamp_task_origin_ai(task: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE $task SET origin = origin ?? 'ai'")
            .bind(("task", task.clone()))
            .await?;
        Ok(())
    }

    /// Fetch one session row by key.
    pub async fn get(session_id: &str) -> anyhow::Result<Option<Self>> {
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
        Ok(db().select(sid).await?)
    }

    pub async fn get_full(session_id: &str) -> anyhow::Result<Option<DiagnosticSessionFull>> {
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
        let session: Option<DiagnosticSession> = db().select(sid.clone()).await?;
        let Some(session) = session else { return Ok(None) };

        let entries: Vec<DiagnosticEntry> = db()
            .query("SELECT * FROM diagnostic_entry WHERE session_ref == $sid ORDER BY timestamp ASC")
            .bind(("sid", sid))
            .await?
            .take(0)?;

        Ok(Some(DiagnosticSessionFull { session, entries }))
    }

    pub async fn list_all(start: i32) -> anyhow::Result<Vec<Self>> {
        let sessions: Vec<Self> = db()
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
        let sessions: Vec<Self> = db()
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
        let dbh = db();
        let mut q = dbh.query(&sql);
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
        let dbh = db();
        let mut q = dbh.query(&sql).bind(("sid", session_id.clone()));
        if let Some(t) = task_ref { q = q.bind(("task", t.clone())); }
        if let Some(s) = service_order { q = q.bind(("svc", s.clone())); }
        q.await?;
        if let Some(t) = task_ref {
            if let Err(e) = Self::stamp_task_origin_ai(t).await {
                log::warn!("link_to_task: origin stamp failed: {e}");
            }
        }
        Ok(())
    }

    /// Every open session, newest first, as primitive-only projections.
    /// Used by the fleet-wide link reaper: no record-id field means a row with
    /// a malformed FK still lists, and only its own `get` can fail.
    pub async fn list_open_refs(limit: u32) -> anyhow::Result<Vec<OpenSessionRef>> {
        // started_at must stay in the projection — ORDER BY only accepts
        // selected idioms.
        let refs: Vec<OpenSessionRef> = db()
            .query(
                "SELECT id, connection_string, hostname, tech, started_at, \
                 duration::secs(time::now() - started_at) AS age_secs \
                 FROM diagnostic_session WHERE status == 'open' \
                 ORDER BY started_at DESC LIMIT $limit",
            )
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(refs)
    }

    /// Newest open session for a connected client: by `connection_string`,
    /// else by the linked computer when provided.
    pub async fn latest_open_for_connection(
        connection_string: &str,
        computer_id: Option<&RecordId>,
    ) -> anyhow::Result<Option<Self>> {
        let mut sessions: Vec<Self> = db()
            .query(
                "SELECT * FROM diagnostic_session WHERE status == 'open' \
                 AND connection_string == $cs ORDER BY started_at DESC LIMIT 1",
            )
            .bind(("cs", connection_string.to_string()))
            .await?
            .take(0)?;
        if sessions.is_empty() {
            if let Some(c) = computer_id {
                sessions = db()
                    .query(
                        "SELECT * FROM diagnostic_session WHERE status == 'open' \
                         AND computer_id == $c ORDER BY started_at DESC LIMIT 1",
                    )
                    .bind(("c", c.clone()))
                    .await?
                    .take(0)?;
            }
        }
        Ok(sessions.into_iter().next())
    }

    /// The service `task` a session artifact should attach to when the session
    /// isn't linked yet: the session's service_order, else the newest service
    /// order for the session's computer, then that order's task (a
    /// not-yet-completed one preferred, newest otherwise). Returns
    /// `(task_ref, service_order)` so the caller can persist the link.
    pub async fn resolve_open_service_task(
        &self,
    ) -> anyhow::Result<Option<(RecordId, RecordId)>> {
        let tasks: Vec<super::LiveTaskPayload> = match self.service_order.clone() {
            Some(so) => db()
                .query(
                    "SELECT * FROM task WHERE service_ticket == $so \
                     ORDER BY completed ASC, created_at DESC LIMIT 1",
                )
                .bind(("so", so))
                .await?
                .take(0)?,
            None => {
                let Some(computer) = self.computer_id.clone() else { return Ok(None) };
                db()
                    .query(
                        "SELECT * FROM task WHERE service_ticket.computer == $c \
                         ORDER BY completed ASC, created_at DESC LIMIT 1",
                    )
                    .bind(("c", computer))
                    .await?
                    .take(0)?
            }
        };
        let Some(task) = tasks.into_iter().next() else { return Ok(None) };
        // Link the session's service_order when set, else the task's own ticket.
        let Some(service_order) = self.service_order.clone().or_else(|| task.service_ticket.clone())
        else { return Ok(None) };
        Ok(Some((task.id, service_order)))
    }

    /// Sessions matching the filters, newest first. `query` narrows only when
    /// supplied: ANDing it unconditionally hid history whenever a caller passed
    /// a term that the stored summary happened not to contain.
    pub async fn search(
        query: Option<&str>,
        hostname: Option<&str>,
        customer_name: Option<&str>,
        connection_string: Option<&str>,
    ) -> anyhow::Result<Vec<Self>> {
        let q = query
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);

        let mut conditions: Vec<String> = Vec::new();
        if q.is_some() {
            conditions.push(
                "(string::lowercase(summary ?? '') CONTAINS $q \
                 OR string::lowercase(hostname) CONTAINS $q \
                 OR string::lowercase(customer_name ?? '') CONTAINS $q \
                 OR string::lowercase(connection_string) CONTAINS $q)"
                    .to_string(),
            );
        }
        if hostname.is_some() {
            conditions.push("hostname == $host".to_string());
        }
        if customer_name.is_some() {
            conditions.push("string::lowercase(customer_name ?? '') CONTAINS $cust".to_string());
        }
        if connection_string.is_some() {
            conditions.push("connection_string == $conn".to_string());
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT * FROM diagnostic_session {where_clause} ORDER BY started_at DESC LIMIT 50"
        );

        let sessions: Vec<Self> = db()
            .query(&sql)
            .bind(("q", q.unwrap_or_default()))
            .bind(("host", hostname.unwrap_or("").to_string()))
            .bind(("cust", customer_name.unwrap_or("").to_lowercase()))
            .bind(("conn", connection_string.unwrap_or("").to_string()))
            .await?
            .take(0)?;

        Ok(sessions)
    }
}

impl DiagnosticEntry {
    fn embed_source(&self) -> String {
        format!("{} {}", self.title.trim(), self.detail.trim())
    }

    pub async fn list_all(start: i32) -> anyhow::Result<Vec<Self>> {
        let entries: Vec<Self> = db()
            .query("SELECT * FROM diagnostic_entry ORDER BY timestamp DESC LIMIT 200 START $start")
            .bind(("start", start))
            .await?
            .take(0)?;
        Ok(entries)
    }

    /// Rewrite an entry's detail and recompute its embedding from title+detail.
    /// A plain `UPDATE SET detail` would leave the HNSW vector encoding the old
    /// text (embeddings are app-computed at write time, no DB event), so this
    /// keeps the vector consistent; on embed failure it stores NONE (honest,
    /// and the backfill picks it up) rather than a stale vector.
    pub async fn update_detail(entry_id: &RecordId, detail: &str) -> anyhow::Result<()> {
        let existing: Option<Self> = db().select(entry_id.clone()).await?;
        let title = existing.map(|e| e.title).unwrap_or_default();
        let source = format!("{} {}", title.trim(), detail.trim());
        let embedding = super::utilities::embed_text(&source).await.ok();
        db()
            .query("UPDATE $id SET detail = $detail, embedding = $embedding")
            .bind(("id", entry_id.clone()))
            .bind(("detail", detail.to_string()))
            .bind(("embedding", embedding))
            .await?;
        Ok(())
    }

    pub async fn create(entry: &Self) -> anyhow::Result<RecordId> {
        let mut e = entry.clone();
        e.id = super::random_record_id(super::DIAGNOSTIC_ENTRY_TABLE);
        e.timestamp = chrono::Utc::now().into();

        super::utilities::spawn_embedding_backfill();

        // Embedding is optional in the schema; store the entry without one on failure.
        let embedding = match super::utilities::embed_text(&e.embed_source()).await {
            Ok(v) => Some(v),
            Err(err) => {
                log::error!("embed_text failed; storing diagnostic_entry without embedding: {err:?}");
                None
            }
        };

        db()
            .query(
                "CREATE $id CONTENT {
                    session_ref: $session_ref,
                    timestamp: $timestamp,
                    category: $category,
                    title: $title,
                    detail: $detail,
                    data: $data,
                    plugins_used: $plugins_used,
                    embedding: $embedding
                }",
            )
            .bind(("id", e.id.clone()))
            .bind(("session_ref", e.session_ref.clone()))
            .bind(("timestamp", e.timestamp))
            .bind(("category", e.category))
            .bind(("title", e.title.clone()))
            .bind(("detail", e.detail.clone()))
            .bind(("data", e.data.clone()))
            .bind(("plugins_used", e.plugins_used.clone()))
            .bind(("embedding", embedding))
            .await?;

        Ok(e.id)
    }
}
