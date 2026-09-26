//! A decision the codex agent waits on; only the session's assignee or an active Root may record it.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue, User};
use crate::db;

pub const AGENT_APPROVAL_TABLE: &str = "agent_approval";

/// Tools whose approval never carries over to the rest of the session.
pub const NEVER_REMEMBER_TOOLS: &[&str] = &["remote_reboot_client", "remote_exec_start"];

/// Statuses only a technician's decision writes.
pub const HUMAN_DECISIONS: [&str; 5] = ["accepted", "accepted_for_session", "declined", "cancelled", "answered"];

/// Records an active `$auth`'s decision on a pending, unexpired row it owns, or on any such row for a Root.
pub const DECIDE_SQL: &str = "UPDATE $id SET status = $status, decided_by = $auth.id, deny_note = $note, \
     answers = $answers ?? answers, decided_at = time::now() \
     WHERE status = 'pending' AND (expires_at = NONE OR expires_at > time::now()) \
     AND $auth.active = true AND (($auth.id != NONE AND assignee = $auth.id) OR $auth.authorization = 'Root') \
     RETURN AFTER";

/// Pending rows assigned to an active `$auth`, oldest first.
pub const LIST_PENDING_MINE_SQL: &str = "SELECT * FROM agent_approval WHERE status = 'pending' \
     AND $auth.id != NONE AND $auth.active = true AND assignee = $auth.id ORDER BY requested_at ASC";

/// True when `$by` is active and is `$owner` or a Root.
pub const DECIDER_ALLOWED_SQL: &str =
    "RETURN $by != NONE AND $by.active = true AND ($by = $owner OR $by.authorization = 'Root')";

/// Returns a row to pending while it still holds `$status` from `$by`.
pub const REOPEN_SQL: &str = "UPDATE $id SET status = 'pending', decided_by = NONE, decided_at = NONE, \
     deny_note = NONE, answers = NONE WHERE status = $status AND decided_by = $by RETURN VALUE id";

/// How a pending approval reaches one viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAudience {
    /// The blocking decision modal.
    Modal,
    /// A toast that opens the modal on request.
    Toast,
    Hidden,
}

/// An active signed-in user as approval routing sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalViewer {
    pub id: RecordId,
    pub root: bool,
}

impl ApprovalViewer {
    /// `None` for an inactive user, who neither decides nor steers.
    pub fn of(user: &User) -> Option<Self> {
        user.is_active().then(|| Self { id: user.get_id(), root: user.is_admin() })
    }

    /// The signed-in user's viewer; `None` while the user lock is busy.
    pub fn signed_in() -> Option<Option<Self>> {
        crate::CURRENT_USER_INFO.try_lock().ok().map(|user| user.as_ref().and_then(Self::of))
    }

    /// Whether this viewer may send turns into a session owned by `owner`.
    pub fn may_steer(&self, owner: Option<&RecordId>) -> bool {
        approval_audience(owner, Some(self)) != ApprovalAudience::Hidden
    }
}

/// Modal for the owner, a toast for any other active Root, nothing for anyone else.
pub fn approval_audience(owner: Option<&RecordId>, viewer: Option<&ApprovalViewer>) -> ApprovalAudience {
    match viewer {
        Some(v) if owner == Some(&v.id) => ApprovalAudience::Modal,
        Some(v) if v.root => ApprovalAudience::Toast,
        _ => ApprovalAudience::Hidden,
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentApproval {
    pub id: RecordId,
    pub thread: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub kind: String,
    #[serde(default)]
    #[surreal(default)]
    pub method: String,
    #[serde(default)]
    #[surreal(default)]
    pub codex_request_id: String,
    #[serde(default)]
    #[surreal(default)]
    pub summary: String,
    #[serde(default)]
    #[surreal(default)]
    pub server: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub tool: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub arguments: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub questions: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub answers: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub response_sent: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub assignee: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub connection_string: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub requested_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub expires_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub decided_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub sent_to_codex_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub decided_by: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub deny_note: Option<String>,
}

/// What the broker knows when codex asks.
#[derive(Debug, Clone, Default)]
pub struct NewAgentApproval {
    pub thread: Option<RecordId>,
    pub kind: String,
    pub method: String,
    pub codex_request_id: String,
    pub summary: String,
    pub server: Option<String>,
    pub tool: Option<String>,
    pub arguments: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
    pub questions: Option<serde_json::Value>,
    pub assignee: Option<RecordId>,
    pub connection_string: Option<String>,
    pub store: Option<String>,
    pub ttl_secs: u64,
}

/// Result of a decision attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentDecideOutcome {
    Recorded,
    Missing,
    /// Someone (or the clock) got there first; carries the status that held.
    AlreadyResolved(String),
    /// Still pending, but the signed-in user is neither its assignee nor an active Root.
    NotPermitted,
}

impl AgentApproval {
    pub fn is_pending(&self) -> bool {
        self.status == "pending"
    }

    /// True for a status only a technician's decision writes.
    pub fn is_human_decision(&self) -> bool {
        HUMAN_DECISIONS.contains(&self.status.as_str())
    }

    /// How this row reaches `viewer`.
    pub fn audience_for(&self, viewer: Option<&ApprovalViewer>) -> ApprovalAudience {
        approval_audience(self.assignee.as_ref(), viewer)
    }

    /// Why a decision on this row wrote nothing.
    fn refusal(&self) -> AgentDecideOutcome {
        match self.status.as_str() {
            "pending" if self.expires_at.is_none() || self.secs_remaining() > 0 => AgentDecideOutcome::NotPermitted,
            "pending" => AgentDecideOutcome::AlreadyResolved("expired".to_string()),
            other => AgentDecideOutcome::AlreadyResolved(other.to_string()),
        }
    }

    /// Whether "approve for this session" applies: a tool call outside [`NEVER_REMEMBER_TOOLS`].
    pub fn may_approve_for_session(&self) -> bool {
        self.kind != "question" && !self.tool.as_deref().is_some_and(|t| NEVER_REMEMBER_TOOLS.contains(&t))
    }

    /// Seconds left before this request expires; 0 once it has lapsed or has no deadline.
    pub fn secs_remaining(&self) -> i64 {
        let Some(expires) = self.expires_at.as_ref() else { return 0 };
        (expires.timestamp() - Datetime::now().timestamp()).max(0)
    }

    pub async fn create(new: &NewAgentApproval) -> anyhow::Result<RecordId> {
        let thread = new.thread.clone().ok_or_else(|| anyhow::anyhow!("approval needs a thread"))?;
        let mut res = db()
            .query(
                "CREATE agent_approval CONTENT { thread: $thread, kind: $kind, method: $method, \
                 codex_request_id: $rid, summary: $summary, server: $server, tool: $tool, \
                 arguments: $arguments, params: $params, questions: $questions, assignee: $assignee, \
                 connection_string: $cs, store: $store, status: 'pending', \
                 expires_at: time::now() + type::duration($ttl) } RETURN VALUE id",
            )
            .bind(("thread", thread))
            .bind(("kind", new.kind.clone()))
            .bind(("method", new.method.clone()))
            .bind(("rid", new.codex_request_id.clone()))
            .bind(("summary", new.summary.chars().take(400).collect::<String>()))
            .bind(("server", new.server.clone()))
            .bind(("tool", new.tool.clone()))
            .bind(("arguments", new.arguments.clone()))
            .bind(("params", new.params.clone()))
            .bind(("questions", new.questions.clone()))
            .bind(("assignee", new.assignee.clone()))
            .bind(("cs", new.connection_string.clone()))
            .bind(("store", new.store.clone()))
            .bind(("ttl", format!("{}s", new.ttl_secs.max(1))))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("agent_approval was not created"))
    }

    pub async fn fetch(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// Records the signed-in user's decision if the row is still pending, unexpired and theirs to decide.
    pub async fn decide(
        id: &RecordId,
        status: &str,
        deny_note: Option<String>,
        answers: Option<serde_json::Value>,
    ) -> anyhow::Result<AgentDecideOutcome> {
        let won: Option<Self> = db()
            .query(DECIDE_SQL)
            .bind(("id", id.clone()))
            .bind(("status", status.to_string()))
            .bind(("note", deny_note))
            .bind(("answers", answers))
            .await?
            .check()?
            .take(0)?;
        if won.is_some() {
            return Ok(AgentDecideOutcome::Recorded);
        }
        Ok(Self::fetch(id).await?.map_or(AgentDecideOutcome::Missing, |row| row.refusal()))
    }

    /// Whether `by` may decide a row owned by `owner`: the owner or an active Root.
    pub async fn decider_allowed(by: Option<&RecordId>, owner: Option<&RecordId>) -> anyhow::Result<bool> {
        let allowed: Option<bool> = db()
            .query(DECIDER_ALLOWED_SQL)
            .bind(("by", by.cloned()))
            .bind(("owner", owner.cloned()))
            .await?
            .check()?
            .take(0)?;
        Ok(allowed.unwrap_or(false))
    }

    /// Returns a decision to pending; false when the row no longer holds `status` from `by`.
    pub async fn reopen(id: &RecordId, status: &str, by: Option<&RecordId>) -> anyhow::Result<bool> {
        let ids: Vec<RecordId> = db()
            .query(REOPEN_SQL)
            .bind(("id", id.clone()))
            .bind(("status", status.to_string()))
            .bind(("by", by.cloned()))
            .await?
            .check()?
            .take(0)?;
        Ok(!ids.is_empty())
    }

    /// The broker's own resolution (auto policy, expiry, or a reply it sent).
    pub async fn resolve_by_broker(
        id: &RecordId,
        status: &str,
        response: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        db().query(
            "UPDATE $id SET status = IF status = 'pending' THEN $status ELSE status END, \
             response_sent = $response ?? response_sent, sent_to_codex_at = time::now(), \
             decided_at = decided_at ?? time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("status", status.to_string()))
        .bind(("response", response))
        .await?;
        Ok(())
    }

    /// Flips pending rows past their deadline to `expired`.
    pub async fn expire_stale() -> anyhow::Result<()> {
        db().query(
            "UPDATE agent_approval SET status = 'expired', decided_at = time::now() \
             WHERE status = 'pending' AND expires_at != NONE AND expires_at < time::now()",
        )
        .await?;
        Ok(())
    }

    /// Fails every pending decision of a thread whose broker died before relaying it.
    pub async fn fail_pending_for_thread(thread: &RecordId, note: &str) -> anyhow::Result<usize> {
        let mut res = db()
            .query(
                "UPDATE agent_approval SET status = 'failed', deny_note = $note, decided_at = time::now() \
                 WHERE thread = $thread AND status = 'pending' RETURN VALUE id",
            )
            .bind(("thread", thread.clone()))
            .bind(("note", note.to_string()))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        Ok(ids.len())
    }

    /// Every open decision, oldest first.
    pub async fn list_pending() -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM agent_approval WHERE status = 'pending' ORDER BY requested_at ASC")
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// Open decisions of one thread, oldest first.
    pub async fn list_pending_for_thread(thread: &RecordId) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_approval WHERE thread = $thread AND status = 'pending' \
                 ORDER BY requested_at ASC",
            )
            .bind(("thread", thread.clone()))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// Open decisions assigned to the signed-in user, oldest first.
    pub async fn list_pending_mine() -> anyhow::Result<Vec<Self>> {
        let mut res = db().query(LIST_PENDING_MINE_SQL).await?;
        Ok(res.take(0).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(kind: &str, tool: Option<&str>) -> AgentApproval {
        AgentApproval {
            id: RecordId::new(AGENT_APPROVAL_TABLE, "a"),
            thread: RecordId::new("agent_thread", "t"),
            kind: kind.to_string(),
            method: String::new(),
            codex_request_id: String::new(),
            summary: String::new(),
            server: None,
            tool: tool.map(str::to_string),
            arguments: None,
            params: None,
            questions: None,
            answers: None,
            response_sent: None,
            status: "pending".to_string(),
            assignee: None,
            connection_string: None,
            store: None,
            requested_at: None,
            expires_at: None,
            decided_at: None,
            sent_to_codex_at: None,
            decided_by: None,
            deny_note: None,
        }
    }

    #[test]
    fn never_remember_tools_offer_no_session_approval() {
        for tool in NEVER_REMEMBER_TOOLS {
            assert!(!approval("tool_call", Some(tool)).may_approve_for_session(), "{tool}");
        }
    }

    #[test]
    fn other_tool_calls_offer_session_approval() {
        assert!(approval("tool_call", Some("desktop_click")).may_approve_for_session());
        assert!(approval("tool_call", None).may_approve_for_session());
    }

    #[test]
    fn questions_offer_no_session_approval() {
        assert!(!approval("question", Some("request_user_input")).may_approve_for_session());
    }

    fn viewer(key: &str, root: bool) -> ApprovalViewer {
        ApprovalViewer { id: RecordId::new("user", key), root }
    }

    fn user(key: &str, authorization: &str, active: bool) -> User {
        let mut v = serde_json::to_value(User::default()).expect("a user encodes");
        v["id"] = serde_json::to_value(RecordId::new("user", key)).expect("an id encodes");
        v["authorization"] = serde_json::json!(authorization);
        v["active"] = serde_json::json!(active);
        serde_json::from_value(v).expect("a user decodes")
    }

    fn owned_by(owner: Option<&str>) -> AgentApproval {
        let mut row = approval("tool_call", Some("desktop_click"));
        row.assignee = owner.map(|k| RecordId::new("user", k));
        row
    }

    #[test]
    fn the_owner_gets_the_modal() {
        assert_eq!(owned_by(Some("tech")).audience_for(Some(&viewer("tech", false))), ApprovalAudience::Modal);
    }

    #[test]
    fn a_root_who_owns_the_session_gets_the_modal() {
        assert_eq!(owned_by(Some("boss")).audience_for(Some(&viewer("boss", true))), ApprovalAudience::Modal);
    }

    #[test]
    fn an_active_root_who_is_not_the_owner_gets_a_toast() {
        assert_eq!(owned_by(Some("tech")).audience_for(Some(&viewer("boss", true))), ApprovalAudience::Toast);
    }

    #[test]
    fn an_inactive_user_is_no_viewer_at_all() {
        assert_eq!(ApprovalViewer::of(&user("gone", "Root", false)), None);
        let inactive_owner = ApprovalViewer::of(&user("tech", "User", false));
        assert_eq!(owned_by(Some("tech")).audience_for(inactive_owner.as_ref()), ApprovalAudience::Hidden);
        assert!(ApprovalViewer::of(&user("boss", "Root", true)).is_some_and(|v| v.root));
        assert!(ApprovalViewer::of(&user("mgr", "Manager", true)).is_some_and(|v| !v.root));
    }

    #[test]
    fn only_the_owner_or_a_root_may_steer() {
        let owner = RecordId::new("user", "tech");
        assert!(viewer("tech", false).may_steer(Some(&owner)));
        assert!(viewer("boss", true).may_steer(Some(&owner)));
        assert!(viewer("boss", true).may_steer(None));
        assert!(!viewer("mate", false).may_steer(Some(&owner)));
        assert!(!viewer("tech", false).may_steer(None));
    }

    #[test]
    fn another_tech_sees_nothing_even_in_the_same_store() {
        let mut row = owned_by(Some("tech"));
        row.store = Some("MUR".into());
        assert_eq!(row.audience_for(Some(&viewer("mate", false))), ApprovalAudience::Hidden);
    }

    #[test]
    fn nobody_signed_in_sees_nothing() {
        assert_eq!(owned_by(Some("tech")).audience_for(None), ApprovalAudience::Hidden);
        assert_eq!(owned_by(None).audience_for(None), ApprovalAudience::Hidden);
    }

    #[test]
    fn a_row_with_no_owner_is_a_root_toast_only() {
        assert_eq!(owned_by(None).audience_for(Some(&viewer("boss", true))), ApprovalAudience::Toast);
        assert_eq!(owned_by(None).audience_for(Some(&viewer("tech", false))), ApprovalAudience::Hidden);
    }

    #[test]
    fn only_technician_statuses_count_as_human_decisions() {
        let with = |status: &str| AgentApproval { status: status.into(), ..approval("tool_call", None) };
        for status in HUMAN_DECISIONS {
            assert!(with(status).is_human_decision(), "{status}");
        }
        for status in ["pending", "expired", "failed", "auto_declined", "auto_accepted", "resolved_elsewhere"] {
            assert!(!with(status).is_human_decision(), "{status}");
        }
    }

    #[test]
    fn a_refused_decision_names_why() {
        let at = |offset: i64| Datetime::from_timestamp(Datetime::now().timestamp() + offset, 0);
        let mut row = approval("tool_call", None);
        assert_eq!(row.refusal(), AgentDecideOutcome::NotPermitted);
        row.expires_at = at(60);
        assert_eq!(row.refusal(), AgentDecideOutcome::NotPermitted);
        row.expires_at = at(-60);
        assert_eq!(row.refusal(), AgentDecideOutcome::AlreadyResolved("expired".into()));
        row.status = "accepted".into();
        assert_eq!(row.refusal(), AgentDecideOutcome::AlreadyResolved("accepted".into()));
    }
}
