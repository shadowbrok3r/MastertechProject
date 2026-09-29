-- ============================================================
-- TABLE: agent_thread_archive
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One user's archive of an agent session: hidden from that user's session list, still listed for everyone else.
-- The record id is [user, thread], so each user archives a session at most once.

DEFINE TABLE agent_thread_archive TYPE NORMAL SCHEMAFULL
    PERMISSIONS FOR select, delete WHERE $access = 'user' AND user = $auth.id,
        FOR create WHERE $access = 'user' AND $auth.id != NONE AND user = $auth.id,
        FOR update NONE;

DEFINE FIELD thread      ON agent_thread_archive TYPE record<agent_thread>   PERMISSIONS FULL;
DEFINE FIELD user        ON agent_thread_archive TYPE record<user> DEFAULT $auth.id PERMISSIONS FULL;
DEFINE FIELD archived_at ON agent_thread_archive TYPE datetime DEFAULT time::now() PERMISSIONS FULL;

DEFINE INDEX agent_thread_archive_user_idx ON agent_thread_archive FIELDS user;
