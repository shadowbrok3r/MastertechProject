-- ============================================================
-- TABLE: agent_approval
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- A tool call or question the codex agent needs a technician to decide.
-- Decided only by an active assignee or an active Root; the broker (system user) bypasses these rules.
-- Identity fields are frozen for record users, and `decided_by` must be the deciding user.

DEFINE TABLE agent_approval TYPE ANY SCHEMALESS
    PERMISSIONS FOR select, update WHERE $access = 'user' AND $auth.active = true AND (($auth.id != NONE AND assignee = $auth.id) OR $auth.authorization = 'Root'), FOR create, delete NONE;

DEFINE FIELD answers           ON agent_approval TYPE none | object             PERMISSIONS FULL;
DEFINE FIELD arguments         ON agent_approval TYPE none | object             PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD assignee          ON agent_approval TYPE none | record<user>       PERMISSIONS FOR select, create FULL, FOR update NONE;
-- JSON-RPC id of the codex request, echoed back verbatim (string or number).
DEFINE FIELD codex_request_id  ON agent_approval TYPE string                    PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD connection_string ON agent_approval TYPE none | string             PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD decided_at        ON agent_approval TYPE none | datetime           PERMISSIONS FULL;
DEFINE FIELD decided_by        ON agent_approval TYPE none | record<user>
    ASSERT $access != 'user' OR $value = $before OR $value = NONE OR $value = $auth.id PERMISSIONS FULL;
DEFINE FIELD deny_note         ON agent_approval TYPE none | string             PERMISSIONS FULL;
DEFINE FIELD expires_at        ON agent_approval TYPE none | datetime           PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD kind              ON agent_approval TYPE string
    ASSERT $value IN ['tool_call', 'command', 'file_change', 'permissions', 'question', 'elicitation'] PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD method            ON agent_approval TYPE string                    PERMISSIONS FOR select, create FULL, FOR update NONE;
-- Raw codex request params, so a client can render a shape it does not know.
DEFINE FIELD params            ON agent_approval TYPE none | object             PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD questions         ON agent_approval TYPE none | array              PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD requested_at      ON agent_approval TYPE datetime DEFAULT time::now() PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD response_sent     ON agent_approval TYPE none | object             PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD sent_to_codex_at  ON agent_approval TYPE none | datetime           PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD server            ON agent_approval TYPE none | string             PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD status            ON agent_approval TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'accepted', 'accepted_for_session', 'accepted_all_for_session', 'declined', 'cancelled', 'answered', 'expired', 'auto_declined', 'auto_accepted', 'resolved_elsewhere', 'failed'] PERMISSIONS FULL;
DEFINE FIELD store             ON agent_approval TYPE none | string             PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD summary           ON agent_approval TYPE string                    PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD thread            ON agent_approval TYPE record<agent_thread>      PERMISSIONS FOR select, create FULL, FOR update NONE;
DEFINE FIELD tool              ON agent_approval TYPE none | string             PERMISSIONS FOR select, create FULL, FOR update NONE;

DEFINE INDEX agent_approval_status_idx   ON agent_approval FIELDS status;
DEFINE INDEX agent_approval_thread_idx   ON agent_approval FIELDS thread;
DEFINE INDEX agent_approval_assignee_idx ON agent_approval FIELDS assignee, status;

-- Rings the assigned tech's bell when the agent needs a decision.
DEFINE EVENT agent_approval_requested ON agent_approval
    WHEN $event = 'CREATE' AND $value.status = 'pending' AND $value.assignee != NONE
    THEN {
        CREATE notification CONTENT {
            notification_description: string::concat('AI agent asks: ', $value.summary, ' (', $value.connection_string ?? '', ')'),
            notification_type: 'Agent Approval',
            status: 'Unread',
            user: $value.assignee
        };
    };

-- Stamps the moment a pending row left the pending state.
DEFINE EVENT agent_approval_decided ON agent_approval
    WHEN $event = 'UPDATE' AND $before.status = 'pending' AND $after.status != 'pending' AND $after.decided_at = NONE
    THEN {
        UPDATE $value.id SET decided_at = time::now();
    };
