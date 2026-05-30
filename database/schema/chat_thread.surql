-- ============================================================
-- TABLE: chat_thread
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE chat_thread TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD created_at    ON chat_thread TYPE datetime DEFAULT time::now()      PERMISSIONS FULL;
DEFINE FIELD thread_users  ON chat_thread TYPE array<record<user>>              PERMISSIONS FULL;
DEFINE FIELD thread_users.* ON chat_thread TYPE record<user>                    PERMISSIONS FULL;
DEFINE FIELD user_created  ON chat_thread TYPE record<user> DEFAULT $auth.id     PERMISSIONS FULL;
