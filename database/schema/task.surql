-- ============================================================
-- TABLE: task
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- No changes in Tranche 1 — included for completeness.
-- ============================================================

DEFINE TABLE task TYPE ANY SCHEMALESS CHANGEFEED 1w PERMISSIONS FULL;

DEFINE EVENT created ON task
    WHEN $event = 'CREATE'
    THEN {
        IF $auth != NULL OR NONE {
            UPDATE $value.id SET created_at = time::now();
            CREATE notification CONTENT {
                notification_description: string::concat('New Task for ', $value.assignee.name),
                notification_type: 'Task Created',
                status: 'Unread',
                user: $value.assignee
            };
        };
    };

DEFINE EVENT updated ON task
    WHEN $event == 'UPDATE'
    THEN {
        IF $before.assignee != $value.assignee {
            CREATE notification CONTENT {
                notification_description: string::concat($value.task_name, ' assigned to you'),
                notification_type: 'Task Update',
                status: 'Unread',
                user: $value.assignee
            };
        };
    };

DEFINE INDEX task_idx ON task FIELDS service_number, assignee, service_ticket, created_at;

DEFINE FIELD assignee         ON task TYPE record<user>          DEFAULT $auth.id PERMISSIONS FULL;
DEFINE FIELD completed        ON task TYPE bool                  DEFAULT false    PERMISSIONS FULL;
-- Stamped when `completed` flips true, cleared when it flips back. NONE on rows
-- completed before the field existed; readers fall back to created_at.
DEFINE FIELD completed_at     ON task TYPE none | datetime                        PERMISSIONS FULL;
DEFINE FIELD created_at       ON task TYPE datetime              DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD due_date         ON task TYPE datetime              DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD jobs             ON task TYPE none | array<record<job>>              PERMISSIONS FULL;
DEFINE FIELD jobs.*           ON task TYPE record<job>                            PERMISSIONS FULL;
DEFINE FIELD origin           ON task TYPE none | string ASSERT $value == NONE OR $value IN ['ai', 'tech'] PERMISSIONS FULL;
DEFINE FIELD priority         ON task TYPE none | string                          PERMISSIONS FULL;
DEFINE FIELD service_number   ON task TYPE none | string                          PERMISSIONS FULL;
DEFINE FIELD service_ticket   ON task TYPE none | record<service_order>           PERMISSIONS FULL;
DEFINE FIELD status           ON task TYPE string                DEFAULT 'Todo'   PERMISSIONS FULL;
DEFINE FIELD task_description ON task TYPE none | string                          PERMISSIONS FULL;
DEFINE FIELD task_name        ON task TYPE string                                  PERMISSIONS FULL;
