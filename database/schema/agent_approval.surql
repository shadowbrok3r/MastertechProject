-- ============================================================
-- TABLE: agent_approval
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- A question the codex agent cannot answer itself: permission to run a
-- machine-touching tool, or a structured question for the technician. The
-- broker creates the row and waits; any Mastertech instance may decide it.
-- `decided_by` is written from $auth by the deciding client, never taken from
-- the request body, so the decision is attributable.

DEFINE TABLE agent_approval TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD answers           ON agent_approval TYPE none | object             PERMISSIONS FULL;
DEFINE FIELD arguments         ON agent_approval TYPE none | object             PERMISSIONS FULL;
DEFINE FIELD assignee          ON agent_approval TYPE none | record<user>       PERMISSIONS FULL;
-- JSON-RPC id of the codex request, echoed back verbatim (string or number).
DEFINE FIELD codex_request_id  ON agent_approval TYPE string                    PERMISSIONS FULL;
DEFINE FIELD connection_string ON agent_approval TYPE none | string             PERMISSIONS FULL;
DEFINE FIELD decided_at        ON agent_approval TYPE none | datetime           PERMISSIONS FULL;
DEFINE FIELD decided_by        ON agent_approval TYPE none | record<user>       PERMISSIONS FULL;
DEFINE FIELD deny_note         ON agent_approval TYPE none | string             PERMISSIONS FULL;
DEFINE FIELD expires_at        ON agent_approval TYPE none | datetime           PERMISSIONS FULL;
DEFINE FIELD kind              ON agent_approval TYPE string
    ASSERT $value IN ['tool_call', 'command', 'file_change', 'permissions', 'question', 'elicitation'] PERMISSIONS FULL;
DEFINE FIELD method            ON agent_approval TYPE string                    PERMISSIONS FULL;
-- Raw codex request params, so a client can render a shape it does not know.
DEFINE FIELD params            ON agent_approval TYPE none | object             PERMISSIONS FULL;
DEFINE FIELD questions         ON agent_approval TYPE none | array              PERMISSIONS FULL;
DEFINE FIELD requested_at      ON agent_approval TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD response_sent     ON agent_approval TYPE none | object             PERMISSIONS FULL;
DEFINE FIELD sent_to_codex_at  ON agent_approval TYPE none | datetime           PERMISSIONS FULL;
DEFINE FIELD server            ON agent_approval TYPE none | string             PERMISSIONS FULL;
DEFINE FIELD status            ON agent_approval TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'accepted', 'accepted_for_session', 'accepted_all_for_session', 'declined', 'cancelled', 'answered', 'expired', 'auto_declined', 'auto_accepted', 'resolved_elsewhere', 'failed'] PERMISSIONS FULL;
DEFINE FIELD store             ON agent_approval TYPE none | string             PERMISSIONS FULL;
DEFINE FIELD summary           ON agent_approval TYPE string                    PERMISSIONS FULL;
DEFINE FIELD thread            ON agent_approval TYPE record<agent_thread>      PERMISSIONS FULL;
DEFINE FIELD tool              ON agent_approval TYPE none | string             PERMISSIONS FULL;

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
