use crate::DATABASE;
use serde::{Deserialize, Serialize};
use super::{Datetime, RecordId, SurrealValue};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct PluginUsageRef {
    pub plugin_id: String,
    pub tool_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct DiagnosticSession {
    pub id: RecordId,
    pub connection_string: String,
    pub hostname: String,
    pub customer_name: Option<String>,
    pub customer_id: Option<RecordId>,
    pub computer_id: Option<RecordId>,
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
            customer_id: None,
            computer_id: None,
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
    pub category: String,
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
            category: "note".to_string(),
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
