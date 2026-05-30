-- ============================================================
-- TABLE: task_note_read
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Join table marking which task_note a user has read; rows carry only
-- their dynamic fields, so no field definitions are pinned here.

DEFINE TABLE task_note_read TYPE ANY SCHEMALESS PERMISSIONS FULL;
