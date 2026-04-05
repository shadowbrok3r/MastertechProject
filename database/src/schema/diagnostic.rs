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
    pub session: RecordId,
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
            session: super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE),
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
        let created: Option<Self> = DATABASE
            .query(
                "CREATE diagnostic_session SET \
                 connection_string = $conn, hostname = $host, \
                 customer_name = $cust_name, customer_id = $cust_id, \
                 computer_id = $comp_id, tech = $tech, \
                 started_at = time::now(), status = 'open', tags = $tags"
            )
            .bind(("conn", session.connection_string.clone()))
            .bind(("host", session.hostname.clone()))
            .bind(("cust_name", session.customer_name.clone()))
            .bind(("cust_id", session.customer_id.clone()))
            .bind(("comp_id", session.computer_id.clone()))
            .bind(("tech", session.tech.clone()))
            .bind(("tags", session.tags.clone()))
            .await?
            .take(0)?;
        Ok(created.map(|s| s.id).unwrap_or_else(|| super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE)))
    }

    pub async fn close(session_id: &str, status: &str, summary: &str, tags: Option<&[String]>) -> anyhow::Result<()> {
        let mut query_str = String::from(
            "UPDATE $sid SET status = $status, summary = $summary, ended_at = time::now()"
        );
        if tags.is_some() {
            query_str.push_str(", tags = $tags");
        }
        let sid = RecordId::new(super::DIAGNOSTIC_SESSION_TABLE, session_id);
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
        let session: Option<DiagnosticSession> = DATABASE
            .query("SELECT * FROM $sid")
            .bind(("sid", sid.clone()))
            .await?
            .take(0)?;

        let Some(session) = session else { return Ok(None) };

        let entries: Vec<DiagnosticEntry> = DATABASE
            .query("SELECT * FROM diagnostic_entry WHERE session == $sid ORDER BY timestamp ASC")
            .bind(("sid", sid))
            .await?
            .take(0)?;

        Ok(Some(DiagnosticSessionFull { session, entries }))
    }

    pub async fn search(
        query: &str,
        hostname: Option<&str>,
        customer_name: Option<&str>,
        connection_string: Option<&str>,
    ) -> anyhow::Result<Vec<Self>> {
        let q = format!("%{query}%");
        let mut conditions = vec![
            "(summary ~ $q OR hostname ~ $q OR customer_name ~ $q OR connection_string ~ $q OR tags ~ $q)".to_string()
        ];
        if hostname.is_some() {
            conditions.push("hostname == $host".to_string());
        }
        if customer_name.is_some() {
            conditions.push("customer_name ~ $cust".to_string());
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
            .bind(("cust", customer_name.map(|c| format!("%{c}%")).unwrap_or_default()))
            .bind(("conn", connection_string.unwrap_or("").to_string()))
            .await?
            .take(0)?;

        Ok(sessions)
    }
}

impl DiagnosticEntry {
    pub async fn create(entry: &Self) -> anyhow::Result<RecordId> {
        let created: Option<Self> = DATABASE
            .query(
                "CREATE diagnostic_entry SET \
                 session = $session, timestamp = time::now(), \
                 category = $cat, title = $title, detail = $detail, \
                 data = $data, plugins_used = $plugins"
            )
            .bind(("session", entry.session.clone()))
            .bind(("cat", entry.category.clone()))
            .bind(("title", entry.title.clone()))
            .bind(("detail", entry.detail.clone()))
            .bind(("data", entry.data.clone()))
            .bind(("plugins", entry.plugins_used.clone()))
            .await?
            .take(0)?;
        Ok(created.map(|e| e.id).unwrap_or_else(|| super::random_record_id(super::DIAGNOSTIC_ENTRY_TABLE)))
    }
}
