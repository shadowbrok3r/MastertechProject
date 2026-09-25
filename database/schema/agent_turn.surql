-- ============================================================
-- TABLE: agent_turn
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- A technician's instruction to a running agent_thread. Desktops write rows;
-- the admin-agent broker claims them and turns them into codex turn/start,
-- turn/steer, turn/interrupt, thread/compact/start, thread/name/set or a
-- thread close. A `queue` row waits as `queued` (or `held` after a stop or a
-- failed turn) until the running turn ends. `tech` defaults from $auth so the
-- recorded sender is the signed-in technician.

DEFINE TABLE agent_turn TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD created_at ON agent_turn TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;
DEFINE FIELD error      ON agent_turn TYPE none | string                  PERMISSIONS FULL;
-- Pictures as {name, mime, data}; the broker drops `data` and adds `path` once it stages them.
DEFINE FIELD images     ON agent_turn TYPE none | array<object>           PERMISSIONS FULL;
DEFINE FIELD kind       ON agent_turn TYPE string DEFAULT 'start'
    ASSERT $value IN ['start', 'steer', 'interrupt', 'close', 'queue', 'compact', 'rename'] PERMISSIONS FULL;
DEFINE FIELD sent_at    ON agent_turn TYPE none | datetime                PERMISSIONS FULL;
DEFINE FIELD status     ON agent_turn TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'sent', 'failed', 'queued', 'held', 'cancelled'] PERMISSIONS FULL;
DEFINE FIELD tech       ON agent_turn TYPE none | string DEFAULT $auth.email PERMISSIONS FULL;
DEFINE FIELD text       ON agent_turn TYPE string DEFAULT ''              PERMISSIONS FULL;
DEFINE FIELD thread     ON agent_turn TYPE record<agent_thread>           PERMISSIONS FULL;

DEFINE INDEX agent_turn_status_idx ON agent_turn FIELDS status;
DEFINE INDEX agent_turn_thread_idx ON agent_turn FIELDS thread;
