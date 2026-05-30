-- ============================================================
-- TABLE: job
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE job TYPE NORMAL SCHEMALESS PERMISSIONS NONE;

DEFINE FIELD computer ON job TYPE record<computer> PERMISSIONS FULL;
