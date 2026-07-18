-- ============================================================
-- TABLE: ai_task
-- AI-authored hands-on handoff overlay pointing at an existing task.
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE ai_task TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD task_ref               ON ai_task TYPE record<task>                 PERMISSIONS FULL;
DEFINE FIELD session_ref            ON ai_task TYPE record<diagnostic_session>   PERMISSIONS FULL;
DEFINE FIELD assignee               ON ai_task TYPE record<user>                 PERMISSIONS FULL;
DEFINE FIELD requested_by           ON ai_task TYPE record<user> DEFAULT $auth.id PERMISSIONS FULL;
DEFINE FIELD title                  ON ai_task TYPE string                       PERMISSIONS FULL;
DEFINE FIELD customer_name          ON ai_task TYPE string                       PERMISSIONS FULL;
DEFINE FIELD service_number         ON ai_task TYPE string                       PERMISSIONS FULL;
DEFINE FIELD connection_string      ON ai_task TYPE none | string                PERMISSIONS FULL;
DEFINE FIELD status                 ON ai_task TYPE string DEFAULT 'open'        PERMISSIONS FULL;
DEFINE FIELD acknowledged_at        ON ai_task TYPE none | datetime              PERMISSIONS FULL;
DEFINE FIELD review_acknowledged_at ON ai_task TYPE none | datetime              PERMISSIONS FULL;
DEFINE FIELD created_at             ON ai_task TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD completed_at           ON ai_task TYPE none | datetime              PERMISSIONS FULL;
DEFINE FIELD closed_at              ON ai_task TYPE none | datetime              PERMISSIONS FULL;

DEFINE INDEX ai_task_idx ON ai_task FIELDS task_ref, session_ref, assignee, status;

-- Notify the assigned tech when an AI task is created.
DEFINE EVENT ai_task_created ON ai_task
    WHEN $event = 'CREATE'
    THEN {
        IF $auth != NULL OR NONE {
            CREATE notification CONTENT {
                notification_description: string::concat('Computer for ', $value.customer_name, ' - ', $value.service_number, ' requires your attention in task ', $value.task_ref.task_name),
                notification_type: 'AI Attention',
                status: 'Unread',
                user: $value.assignee
            };
        };
    };

-- Notify the requesting operator exactly once when the checklist completes.
DEFINE EVENT ai_task_completed ON ai_task
    WHEN $event = 'UPDATE' AND $before.status = 'open' AND $value.status = 'awaiting_followup'
    THEN {
        CREATE notification CONTENT {
            notification_description: string::concat('AI task complete: ', $value.title, ' (', $value.customer_name, ' - ', $value.service_number, ') in task ', $value.task_ref.task_name),
            notification_type: 'AI Followup',
            status: 'Unread',
            user: $value.requested_by
        };
    };

-- Re-notify the tech when the AI task reopens, unless they reopened it themselves.
DEFINE EVENT ai_task_reopened ON ai_task
    WHEN $event = 'UPDATE' AND $before.status = 'awaiting_followup' AND $value.status = 'open'
    THEN {
        IF $value.assignee != $auth.id {
            CREATE notification CONTENT {
                notification_description: string::concat('Computer for ', $value.customer_name, ' - ', $value.service_number, ' requires your attention in task ', $value.task_ref.task_name),
                notification_type: 'AI Attention',
                status: 'Unread',
                user: $value.assignee
            };
        };
    };
