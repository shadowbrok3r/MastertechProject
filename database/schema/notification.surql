-- ============================================================
-- TABLE: notification             last changed: ai_assistant_tools
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE notification TYPE ANY SCHEMALESS CHANGEFEED 2d PERMISSIONS FULL;

DEFINE FIELD created_at               ON notification TYPE none | datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD notification_description  ON notification TYPE string                             PERMISSIONS FULL;
DEFINE FIELD notification_type         ON notification TYPE string                             PERMISSIONS FULL;
DEFINE FIELD status                    ON notification TYPE string DEFAULT 'Unread'            PERMISSIONS FULL;
DEFINE FIELD user                      ON notification TYPE record<user>                       PERMISSIONS FULL;
DEFINE FIELD task                      ON notification TYPE none | record<task>                PERMISSIONS FULL;
DEFINE FIELD from_user                 ON notification TYPE none | record<user>                PERMISSIONS FULL;
-- Status 'Snoozed' returns to 'Unread' at this time.
DEFINE FIELD snooze_until              ON notification TYPE none | datetime                    PERMISSIONS FULL;

DEFINE INDEX notification_snooze ON notification FIELDS snooze_until;

-- Stamps created_at on insert.
DEFINE EVENT created ON notification WHEN $event = 'CREATE' THEN {
    UPDATE $value.id SET created_at = time::now()
};
