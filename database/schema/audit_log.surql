-- ============================================================
-- TABLE: audit_log
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Read-only audit trail; rows are written out-of-band, never via app sessions.

DEFINE TABLE audit_log TYPE ANY SCHEMALESS
    PERMISSIONS
        FOR select FULL,
        FOR create, update, delete NONE;
