-- ============================================================
-- TABLE: task_history
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE task_history TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD created_at ON task_history TYPE datetime DEFAULT time::now()      PERMISSIONS FULL;
DEFINE FIELD `diff`     ON task_history TYPE object                            PERMISSIONS FULL;
DEFINE FIELD task_id    ON task_history TYPE record<task>                      PERMISSIONS FULL;
DEFINE FIELD user       ON task_history TYPE record<user> DEFAULT $auth.id     PERMISSIONS FULL;
DEFINE FIELD username   ON task_history TYPE string DEFAULT $auth.username     PERMISSIONS FULL;
