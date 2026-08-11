//! Root-gated approval queue for SurrealQL mutations submitted over MCP.
//!
//! `query_surrealdb` stays read-only. Anything that writes goes through this
//! table: the requester creates a `pending` row carrying the statement, why
//! it is being run, and a best-effort row-count preview; a Root operator
//! approves or denies it from the admin console; the requester executes only
//! after reading back `approved`.
//!
//! DDL is rejected before a row is ever written — see [`StatementKind::parse`].
//! Letting `DEFINE`/`REMOVE` through here would move the live schema without
//! touching `database/schema/*.surql`, and surrealkit's snapshot/rollout state
//! would silently drift from the database it is supposed to describe.

use serde::{Deserialize, Serialize};
use surrealdb_types::Datetime;

use crate::db;

use super::{random_record_id, RecordId, RecordIdExt, SurrealValue, SQL_APPROVAL_TABLE};

/// How long a request stays actionable before it is treated as expired.
pub const APPROVAL_TTL_SECS: i64 = 900;

/// Statement classes this gate accepts, plus the DDL/unknown rejections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Create,
    Update,
    Upsert,
    Delete,
    Insert,
    Relate,
}

impl StatementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Upsert => "upsert",
            Self::Delete => "delete",
            Self::Insert => "insert",
            Self::Relate => "relate",
        }
    }

    /// True for the classes that can destroy data, which the modal flags.
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Delete | Self::Update | Self::Upsert)
    }

    /// Classifies a statement, rejecting DDL and anything unrecognized.
    ///
    /// Read-only verbs are rejected too: they belong on `query_surrealdb`,
    /// which needs no approval, and accepting them here would train the
    /// operator to approve harmless requests by reflex.
    pub fn parse(statement: &str) -> Result<Self, String> {
        let head = statement
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();

        match head.as_str() {
            "CREATE" => Ok(Self::Create),
            "UPDATE" => Ok(Self::Update),
            "UPSERT" => Ok(Self::Upsert),
            "DELETE" => Ok(Self::Delete),
            "INSERT" => Ok(Self::Insert),
            "RELATE" => Ok(Self::Relate),
            "DEFINE" | "REMOVE" | "ALTER" | "REBUILD" => Err(format!(
                "{head} is schema DDL and is not accepted here. Applying it this way moves the \
                 live database without touching database/schema/*.surql, so surrealkit's \
                 snapshot and __rollout state drift from the schema they describe. Use the \
                 surrealkit rollout flow instead."
            )),
            "SELECT" | "RETURN" => Err(format!(
                "{head} needs no approval — use query_surrealdb, which is read-only."
            )),
            "" => Err("Empty statement.".to_string()),
            other => Err(format!(
                "Unrecognized statement `{other}`. Accepted: CREATE, UPDATE, UPSERT, DELETE, \
                 INSERT, RELATE."
            )),
        }
    }
}

/// Lifecycle of one request. Anything past `Pending` is terminal for the
/// operator; only the requester moves `Approved` on to `Executed`/`Failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    /// Claimed by a requester that is running it now. Exists so two concurrent
    /// pollers cannot both execute the same approved statement.
    Executing,
    Denied,
    Executed,
    Failed,
    Expired,
}

impl ApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Denied => "denied",
            Self::Executed => "executed",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "approved" => Self::Approved,
            "executing" => Self::Executing,
            "denied" => Self::Denied,
            "executed" => Self::Executed,
            "failed" => Self::Failed,
            "expired" => Self::Expired,
            _ => Self::Pending,
        }
    }

    /// True once the requester should stop polling.
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, SurrealValue)]
pub struct SqlApproval {
    pub id: RecordId,
    /// Verbatim statement the requester wants to run.
    pub statement: String,
    /// Operator-facing justification, shown in the modal.
    pub reason: String,
    /// [`StatementKind::as_str`] of the parsed statement.
    #[serde(default)]
    #[surreal(default)]
    pub statement_kind: String,
    /// Table the statement targets, when it could be extracted.
    #[serde(default)]
    #[surreal(default)]
    pub target_table: Option<String>,
    /// Rows a matching SELECT counted before the request was raised.
    #[serde(default)]
    #[surreal(default)]
    pub impact_rows: Option<i64>,
    /// Why the count is missing, or extra context when it is present.
    #[serde(default)]
    #[surreal(default)]
    pub impact_note: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub requested_by: Option<RecordId>,
    /// Human label for the requester, e.g. `Claude (MCP) as Logan Lees`.
    #[serde(default)]
    #[surreal(default)]
    pub requested_label: String,
    #[serde(default)]
    #[surreal(default)]
    pub origin_host: String,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub decided_by: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub decided_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub deny_reason: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub result_summary: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub executed_at: Option<Datetime>,
    pub created_at: Datetime,
    #[serde(default)]
    #[surreal(default)]
    pub expires_at: Option<Datetime>,
}

impl Default for SqlApproval {
    fn default() -> Self {
        Self {
            id: random_record_id(SQL_APPROVAL_TABLE),
            statement: String::new(),
            reason: String::new(),
            statement_kind: String::new(),
            target_table: None,
            impact_rows: None,
            impact_note: None,
            requested_by: None,
            requested_label: String::new(),
            origin_host: String::new(),
            status: ApprovalStatus::Pending.as_str().to_string(),
            decided_by: None,
            decided_at: None,
            deny_reason: None,
            result_summary: None,
            executed_at: None,
            created_at: Datetime::now(),
            expires_at: None,
        }
    }
}

impl SqlApproval {
    pub fn status_enum(&self) -> ApprovalStatus {
        ApprovalStatus::from_str(&self.status)
    }

    pub fn kind_is_destructive(&self) -> bool {
        matches!(self.statement_kind.as_str(), "delete" | "update" | "upsert")
    }

    /// Seconds left before this request expires; 0 once it has lapsed.
    pub fn secs_remaining(&self) -> i64 {
        let Some(expires) = self.expires_at.as_ref() else {
            return 0;
        };
        let now = Datetime::now();
        let delta = expires.timestamp() - now.timestamp();
        delta.max(0)
    }

    /// Writes the pending row and notifies Root via the table's CREATE event.
    ///
    /// Expiry is stamped by the database, not the caller, so the countdown the
    /// operator sees and the deadline the poller enforces both read the same
    /// clock as `time::now()` in [`Self::expire_stale`].
    pub async fn submit(&self) -> anyhow::Result<RecordId> {
        db()
            .query("CREATE $id CONTENT $row")
            .bind(("id", self.id.clone()))
            .bind(("row", self.clone()))
            .await?
            .check()?;

        db()
            .query(format!(
                "UPDATE $id SET expires_at = time::now() + {APPROVAL_TTL_SECS}s"
            ))
            .bind(("id", self.id.clone()))
            .await?
            .check()?;

        log::info!(
            "sql_approval {} submitted ({}): {}",
            self.id.key_string(),
            self.statement_kind,
            self.reason
        );
        Ok(self.id.clone())
    }

    pub async fn fetch(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let row: Option<Self> = db()
            .query("SELECT * FROM $id")
            .bind(("id", id.clone()))
            .await?
            .check()?
            .take(0)?;
        Ok(row)
    }

    /// Root decision. `approved = false` records a denial with an optional note.
    pub async fn decide(
        id: &RecordId,
        approved: bool,
        decided_by: Option<RecordId>,
        deny_reason: Option<String>,
    ) -> anyhow::Result<()> {
        let status = if approved {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Denied
        };
        db()
            .query(
                "UPDATE $id SET status = $status, decided_by = $by, deny_reason = $why \
                 WHERE status = 'pending'",
            )
            .bind(("id", id.clone()))
            .bind(("status", status.as_str().to_string()))
            .bind(("by", decided_by))
            .bind(("why", deny_reason))
            .await?
            .check()?;
        Ok(())
    }

    /// Records the outcome after the requester ran an approved statement.
    pub async fn record_result(
        id: &RecordId,
        ok: bool,
        summary: impl Into<String>,
    ) -> anyhow::Result<()> {
        let status = if ok {
            ApprovalStatus::Executed
        } else {
            ApprovalStatus::Failed
        };
        db()
            .query("UPDATE $id SET status = $status, result_summary = $summary, executed_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("status", status.as_str().to_string()))
            .bind(("summary", summary.into()))
            .await?
            .check()?;
        Ok(())
    }

    /// Lapses every pending request past its expiry so neither the modal nor
    /// a poller keeps offering a stale statement.
    pub async fn expire_stale() -> anyhow::Result<()> {
        db()
            .query(
                "UPDATE sql_approval SET status = 'expired' \
                 WHERE status = 'pending' AND expires_at != NONE AND expires_at < time::now()",
            )
            .await?
            .check()?;
        Ok(())
    }

    /// Pending, unexpired requests — the initial fill for the approval modal
    /// before the live stream takes over.
    pub async fn list_pending() -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query(
                "SELECT * FROM sql_approval WHERE status = 'pending' \
                 AND (expires_at = NONE OR expires_at > time::now()) ORDER BY created_at",
            )
            .await?
            .check()?
            .take(0)?;
        Ok(rows)
    }
}

/// Best-effort `FROM`/`INTO` target of a mutation, lowercased.
///
/// Used for the modal's impact preview only — a miss costs the operator a
/// row count, never correctness, so this stays a cheap token scan rather
/// than a parser.
pub fn extract_target(statement: &str) -> Option<String> {
    let tokens: Vec<&str> = statement.split_whitespace().collect();
    let head = tokens.first()?.to_ascii_uppercase();

    let idx = match head.as_str() {
        // CREATE <target> / UPDATE <target> / UPSERT <target> / DELETE <target>
        "CREATE" | "UPDATE" | "UPSERT" | "DELETE" => {
            // `DELETE FROM x` is also legal; skip the optional FROM.
            if tokens.get(1).map(|t| t.eq_ignore_ascii_case("FROM")) == Some(true) {
                2
            } else {
                1
            }
        }
        // INSERT INTO <target>
        "INSERT" => {
            if tokens.get(1).map(|t| t.eq_ignore_ascii_case("INTO")) == Some(true) {
                2
            } else {
                1
            }
        }
        _ => return None,
    };

    let raw = tokens.get(idx)?.trim_matches(|c| c == '`' || c == ';');
    if raw.is_empty() || raw.starts_with('$') {
        return None;
    }
    // `computer:DESKTOP-X:hash` targets one record; the table is the head.
    let table = raw.split(':').next().unwrap_or(raw);
    if table.is_empty() {
        None
    } else {
        Some(table.to_ascii_lowercase())
    }
}

/// Rewrites a mutation into a counting SELECT so the operator sees how many
/// rows it touches before approving.
///
/// Returns `Err` with an operator-facing note whenever the shape is one this
/// cannot faithfully mirror — a wrong number is worse than no number, so it
/// declines rather than guesses.
pub fn impact_query(statement: &str) -> Result<String, String> {
    let kind = StatementKind::parse(statement).map_err(|e| e.to_string())?;
    let target = extract_target(statement)
        .ok_or_else(|| "target table could not be determined".to_string())?;

    match kind {
        // These add rows; there is nothing pre-existing to count.
        StatementKind::Create | StatementKind::Insert | StatementKind::Relate => {
            Err("statement inserts new rows — nothing to count".to_string())
        }
        StatementKind::Update | StatementKind::Upsert | StatementKind::Delete => {
            let upper = statement.to_ascii_uppercase();
            // A targeted write (UPDATE computer:foo SET ...) touches one row.
            let where_at = upper.find(" WHERE ");
            match where_at {
                Some(pos) => {
                    let mut clause = statement[pos + " WHERE ".len()..].trim().to_string();
                    // Trailing modifiers are not valid on a counting SELECT.
                    for kw in [" RETURN ", " SET ", " CONTENT ", " MERGE ", " PATCH "] {
                        if let Some(cut) = clause.to_ascii_uppercase().find(kw) {
                            clause.truncate(cut);
                        }
                    }
                    let clause = clause.trim_end_matches(';').trim();
                    if clause.is_empty() {
                        return Err("WHERE clause could not be isolated".to_string());
                    }
                    Ok(format!(
                        "SELECT count() AS n FROM {target} WHERE {clause} GROUP ALL"
                    ))
                }
                None => {
                    // No WHERE: either a record-targeted write or a whole-table sweep.
                    let raw_target = statement
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .trim_matches(|c| c == '`' || c == ';');
                    if raw_target.contains(':') {
                        Ok(format!("SELECT count() AS n FROM {target} WHERE id = {raw_target} GROUP ALL"))
                    } else {
                        Ok(format!("SELECT count() AS n FROM {target} GROUP ALL"))
                    }
                }
            }
        }
    }
}

/// Runs [`impact_query`] and returns `(rows, note)` for the modal.
pub async fn preview_impact(statement: &str) -> (Option<i64>, Option<String>) {
    let query = match impact_query(statement) {
        Ok(q) => q,
        Err(note) => return (None, Some(format!("preview unavailable — {note}"))),
    };

    #[derive(Serialize, Deserialize, SurrealValue)]
    struct Count {
        n: i64,
    }

    match db().query(&query).await {
        Ok(mut resp) => match resp.take::<Vec<Count>>(0) {
            Ok(rows) => {
                let n = rows.first().map(|c| c.n).unwrap_or(0);
                (Some(n), Some(format!("counted via `{query}`")))
            }
            Err(e) => (None, Some(format!("preview unavailable — {e}"))),
        },
        Err(e) => (None, Some(format!("preview unavailable — {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_is_rejected_with_a_surrealkit_pointer() {
        let err = StatementKind::parse("DEFINE FIELD x ON y TYPE string").unwrap_err();
        assert!(err.contains("surrealkit"), "{err}");
        assert!(StatementKind::parse("REMOVE TABLE user").is_err());
        assert!(StatementKind::parse("ALTER TABLE user").is_err());
    }

    #[test]
    fn read_only_verbs_are_pushed_to_query_surrealdb() {
        let err = StatementKind::parse("SELECT * FROM user").unwrap_err();
        assert!(err.contains("query_surrealdb"), "{err}");
    }

    #[test]
    fn dml_verbs_classify() {
        assert_eq!(
            StatementKind::parse("update computer SET x = 1").unwrap(),
            StatementKind::Update
        );
        assert_eq!(
            StatementKind::parse("  DELETE FROM task WHERE a = 1").unwrap(),
            StatementKind::Delete
        );
        assert_eq!(
            StatementKind::parse("RELATE a->b->c").unwrap(),
            StatementKind::Relate
        );
    }

    #[test]
    fn targets_extract_through_from_into_and_record_ids() {
        assert_eq!(extract_target("UPDATE computer SET a = 1").as_deref(), Some("computer"));
        assert_eq!(extract_target("DELETE FROM task WHERE x").as_deref(), Some("task"));
        assert_eq!(extract_target("INSERT INTO user { a: 1 }").as_deref(), Some("user"));
        assert_eq!(
            extract_target("UPDATE crash_verdict:`2a49ddfa` SET task_ref = $t").as_deref(),
            Some("crash_verdict")
        );
        assert_eq!(extract_target("UPDATE $var SET a = 1"), None);
    }

    #[test]
    fn impact_query_mirrors_the_where_clause() {
        let q = impact_query("UPDATE crash_sighting SET task_ref = $t WHERE computer = $c")
            .unwrap();
        assert_eq!(
            q,
            "SELECT count() AS n FROM crash_sighting WHERE computer = $c GROUP ALL"
        );
    }

    #[test]
    fn impact_query_strips_trailing_return() {
        let q = impact_query("DELETE task WHERE a = 1 RETURN BEFORE").unwrap();
        assert_eq!(q, "SELECT count() AS n FROM task WHERE a = 1 GROUP ALL");
    }

    #[test]
    fn impact_query_counts_whole_table_sweeps() {
        let q = impact_query("DELETE notification").unwrap();
        assert_eq!(q, "SELECT count() AS n FROM notification GROUP ALL");
    }

    #[test]
    fn impact_query_narrows_record_targeted_writes() {
        let q = impact_query("UPDATE crash_verdict:abc SET task_ref = $t").unwrap();
        assert_eq!(
            q,
            "SELECT count() AS n FROM crash_verdict WHERE id = crash_verdict:abc GROUP ALL"
        );
    }

    #[test]
    fn inserts_have_nothing_to_count() {
        assert!(impact_query("CREATE task CONTENT {}").is_err());
    }

    #[test]
    fn destructive_classes_are_flagged() {
        assert!(StatementKind::Delete.is_destructive());
        assert!(StatementKind::Update.is_destructive());
        assert!(!StatementKind::Create.is_destructive());
    }

    #[test]
    fn status_round_trips_and_reports_terminal() {
        assert_eq!(ApprovalStatus::from_str("approved"), ApprovalStatus::Approved);
        assert_eq!(ApprovalStatus::from_str("nonsense"), ApprovalStatus::Pending);
        assert!(!ApprovalStatus::Pending.is_terminal());
        assert!(ApprovalStatus::Denied.is_terminal());
    }
}
