-- ============================================================
-- TABLE: zeroclaw_audit
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Append-only tool-call trail POSTed by ZeroClaw's webhook-audit hook to the
-- axum orchestrator. Axum signs in with guest credentials, so create is FULL;
-- rows are immutable once written. Payload fields beyond the two indexed below
-- (event, timestamp, success, duration_ms, error, args) ride in schemaless.

DEFINE TABLE zeroclaw_audit TYPE ANY SCHEMALESS CHANGEFEED 30d
    PERMISSIONS
        FOR select, create FULL,
        FOR update, delete NONE;

DEFINE FIELD created_at ON zeroclaw_audit TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD tool ON zeroclaw_audit TYPE option<string> PERMISSIONS FULL;

DEFINE INDEX zca_created_at ON zeroclaw_audit FIELDS created_at;
DEFINE INDEX zca_tool ON zeroclaw_audit FIELDS tool;
