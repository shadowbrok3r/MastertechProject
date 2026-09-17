-- ============================================================
-- TABLE: agent_turn
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- A technician's instruction to a running agent_thread. Desktops write rows;
-- the admin-agent broker claims them and turns them into codex turn/start,
-- turn/steer, turn/interrupt or a thread close. `tech` defaults from $auth so
-- the recorded sender is the signed-in technician.

DEFINE TABLE agent_turn TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD created_at ON agent_turn TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;
DEFINE FIELD error      ON agent_turn TYPE none | string                  PERMISSIONS FULL;
DEFINE FIELD kind       ON agent_turn TYPE string DEFAULT 'start'
    ASSERT $value IN ['start', 'steer', 'interrupt', 'close'] PERMISSIONS FULL;
DEFINE FIELD sent_at    ON agent_turn TYPE none | datetime                PERMISSIONS FULL;
DEFINE FIELD status     ON agent_turn TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'sent', 'failed'] PERMISSIONS FULL;
DEFINE FIELD tech       ON agent_turn TYPE none | string DEFAULT $auth.email PERMISSIONS FULL;
DEFINE FIELD text       ON agent_turn TYPE string DEFAULT ''              PERMISSIONS FULL;
DEFINE FIELD thread     ON agent_turn TYPE record<agent_thread>           PERMISSIONS FULL;

DEFINE INDEX agent_turn_status_idx ON agent_turn FIELDS status;
DEFINE INDEX agent_turn_thread_idx ON agent_turn FIELDS thread;
