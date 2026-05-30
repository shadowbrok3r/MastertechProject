-- ============================================================
-- TABLE: ai_chat
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One row per AI-playground chat thread. SCHEMALESS so `messages`
-- can hold the serialized ChatMessage array verbatim. Record id is
-- the thread's local uuid (set via type::thing('ai_chat', <uuid>)).
-- Rust source of truth: displays::tabs::ai_playground::enhanced +
-- database::schema::user::User::{save,load}_ai_chat_thread(s).

DEFINE TABLE ai_chat TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- "My chats, newest first" — the threads dropdown query.
DEFINE INDEX ai_chat_user_idx ON ai_chat FIELDS user, updated_at;

DEFINE FIELD user       ON ai_chat TYPE record<user> DEFAULT $auth.id      PERMISSIONS FULL;
DEFINE FIELD title      ON ai_chat TYPE none | string                      PERMISSIONS FULL;
DEFINE FIELD messages   ON ai_chat TYPE array DEFAULT []                    PERMISSIONS FULL;
DEFINE FIELD updated_at ON ai_chat TYPE datetime DEFAULT time::now()        PERMISSIONS FULL;
