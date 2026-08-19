-- ============================================================
-- TABLE: zeroclaw_health
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- Single row (zeroclaw_health:gateway) UPSERTed by the box probe;
-- a stale checked_at is the down signal.
-- ============================================================

DEFINE TABLE zeroclaw_health TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD checked_at  ON zeroclaw_health TYPE datetime      PERMISSIONS FULL;
DEFINE FIELD cron_errors ON zeroclaw_health TYPE int           PERMISSIONS FULL;
DEFINE FIELD detail      ON zeroclaw_health TYPE none | string PERMISSIONS FULL;
DEFINE FIELD gateway_ok  ON zeroclaw_health TYPE bool          PERMISSIONS FULL;
DEFINE FIELD status      ON zeroclaw_health TYPE string ASSERT $value IN ['ok', 'error'] PERMISSIONS FULL;
