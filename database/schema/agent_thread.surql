-- ============================================================
-- TABLE: agent_thread
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One Codex agent session about one customer machine, owned by the
-- admin-agent broker. Created when a tech accepts the AI-help offer
-- (assist_request), it carries the codex thread id so the broker can resume
-- after a restart and every Mastertech instance can list the same sessions.
-- Only the broker (system user) creates rows or writes session state; record users may only fill service links.

DEFINE TABLE agent_thread TYPE ANY SCHEMALESS PERMISSIONS FOR select FULL, FOR update WHERE $access = 'user', FOR create, delete NONE;

DEFINE FIELD assist_request     ON agent_thread TYPE none | record<assist_request>     PERMISSIONS FOR select, create FULL, FOR update NONE;
-- The agent's current step (idle, thinking, writing, tool:<name>, compacting...), written at most every 2 s.
DEFINE FIELD activity           ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD assignee           ON agent_thread TYPE none | record<user>               PERMISSIONS FOR select, create FULL, FOR update NONE;
-- Root toggle that lets the agent run shell on the broker host; off by default.
DEFINE FIELD allow_box_shell    ON agent_thread TYPE bool DEFAULT false
    PERMISSIONS FOR select, create FULL, FOR update WHERE $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD broker_node        ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD closed_at          ON agent_thread TYPE none | datetime                   PERMISSIONS FOR select, create FULL, FOR update NONE;
-- Thread id inside the codex app-server; set once thread/start returns.
DEFINE FIELD codex_thread_id    ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD computer           ON agent_thread TYPE none | record<computer>           PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD connection_string  ON agent_thread TYPE string                            PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD created_at         ON agent_thread TYPE datetime DEFAULT time::now()      PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD customer           ON agent_thread TYPE none | record<customer>           PERMISSIONS FULL;
DEFINE FIELD diagnostic_session ON agent_thread TYPE none | record<diagnostic_session> PERMISSIONS FOR select, create FULL, FOR update NONE;
-- Provenance stamp the broker asks the agent to use on diagnostic records.
DEFINE FIELD driven_by          ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD error              ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD hostname           ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD last_event_at      ON agent_thread TYPE none | datetime                   PERMISSIONS FOR select, create FULL, FOR update NONE;
-- Highest agent_event.seq written so far; the broker resumes numbering from it.
DEFINE FIELD last_seq           ON agent_thread TYPE none | int                        PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD model              ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD provider           ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD requested_by       ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD service_number     ON agent_thread TYPE none | string                     PERMISSIONS FULL;
DEFINE FIELD service_order      ON agent_thread TYPE none | record<service_order>      PERMISSIONS FULL;
DEFINE FIELD status             ON agent_thread TYPE string DEFAULT 'queued'
    ASSERT $value IN ['queued', 'starting', 'idle', 'running', 'waiting_approval', 'closed', 'failed']
    PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD store              ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD title              ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD tokens_used        ON agent_thread TYPE none | int                        PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD tokens_window      ON agent_thread TYPE none | int                        PERMISSIONS FOR select, create FULL, FOR update NONE;
-- How the tools reach codex: 'dynamic' (client tools run in the broker) or 'mcp'.
DEFINE FIELD tool_path          ON agent_thread TYPE none | string                     PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD updated_at         ON agent_thread TYPE none | datetime                   PERMISSIONS FULL;

DEFINE INDEX agent_thread_status_idx     ON agent_thread FIELDS status;
DEFINE INDEX agent_thread_assignee_idx   ON agent_thread FIELDS assignee;
DEFINE INDEX agent_thread_connection_idx ON agent_thread FIELDS connection_string;
DEFINE INDEX agent_thread_codex_idx      ON agent_thread FIELDS codex_thread_id;

-- Tells the requesting tech their session finished.
DEFINE EVENT agent_thread_closed ON agent_thread
    WHEN $event = 'UPDATE' AND $before.status != 'closed' AND $after.status = 'closed' AND $value.assignee != NONE
    THEN {
        CREATE notification CONTENT {
            notification_description: string::concat('AI diagnostic finished for ', $value.service_number ?? $value.connection_string),
            notification_type: 'Agent Done',
            status: 'Unread',
            user: $value.assignee
        };
    };
