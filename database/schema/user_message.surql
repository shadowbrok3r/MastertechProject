-- ============================================================
-- TABLE: user_message
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE user_message TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD content    ON user_message TYPE any                                 PERMISSIONS FULL;
DEFINE FIELD created_at ON user_message TYPE datetime DEFAULT time::now()         PERMISSIONS FULL;
DEFINE FIELD thread_id  ON user_message TYPE record<chat_thread>                  PERMISSIONS FULL;
DEFINE FIELD user       ON user_message TYPE record<user> DEFAULT $auth.id        PERMISSIONS FULL;
