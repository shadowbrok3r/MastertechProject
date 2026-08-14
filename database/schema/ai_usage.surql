-- ============================================================
-- TABLE: ai_usage
-- One row per Claude Code session that performed diagnostic work,
-- written by the SessionEnd hook (~/.claude/hooks/ai_usage_hook.py).
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE ai_usage TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD claude_session_id ON ai_usage TYPE none | string PERMISSIONS FULL;
DEFINE FIELD at                ON ai_usage TYPE none | string PERMISSIONS FULL;
DEFINE FIELD session_refs      ON ai_usage TYPE none | array<string> PERMISSIONS FULL;
DEFINE FIELD session_refs.*    ON ai_usage TYPE string PERMISSIONS FULL;
DEFINE FIELD cost_usd          ON ai_usage TYPE none | number PERMISSIONS FULL;
DEFINE FIELD models            ON ai_usage TYPE none | object PERMISSIONS FULL;
DEFINE FIELD source            ON ai_usage TYPE none | string PERMISSIONS FULL;
