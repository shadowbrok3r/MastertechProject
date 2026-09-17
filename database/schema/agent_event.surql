-- ============================================================
-- TABLE: agent_event
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- The transcript of an agent_thread, one row per codex item (message,
-- reasoning, tool call, ...). Streaming text is upserted onto the same row
-- (id = type::record('agent_event', '<thread key>:<item id>')), so a desktop
-- following the thread with a LIVE SELECT sees each row grow, then settle
-- when `done` flips and `item` carries the authoritative item.

DEFINE TABLE agent_event TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD created_at ON agent_event TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD done       ON agent_event TYPE bool DEFAULT false            PERMISSIONS FULL;
-- Authoritative codex ThreadItem once the item completed.
DEFINE FIELD item       ON agent_event TYPE none | object                 PERMISSIONS FULL;
DEFINE FIELD item_id    ON agent_event TYPE none | string                 PERMISSIONS FULL;
DEFINE FIELD kind       ON agent_event TYPE string
    ASSERT $value IN ['user', 'agent', 'reasoning', 'tool_call', 'command', 'file_change', 'approval', 'error', 'turn_started', 'turn_completed', 'other'] PERMISSIONS FULL;
-- Broker-assigned order within the thread; codex item ids do not sort.
DEFINE FIELD seq        ON agent_event TYPE int                            PERMISSIONS FULL;
DEFINE FIELD text       ON agent_event TYPE string DEFAULT ''              PERMISSIONS FULL;
DEFINE FIELD thread     ON agent_event TYPE record<agent_thread>           PERMISSIONS FULL;
DEFINE FIELD turn_id    ON agent_event TYPE none | string                 PERMISSIONS FULL;
DEFINE FIELD updated_at ON agent_event TYPE none | datetime               PERMISSIONS FULL;

DEFINE INDEX agent_event_thread_seq_idx  ON agent_event FIELDS thread, seq;
DEFINE INDEX agent_event_thread_kind_idx ON agent_event FIELDS thread, kind;
