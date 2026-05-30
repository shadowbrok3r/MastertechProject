-- ============================================================
-- TABLE: notification
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE notification TYPE ANY SCHEMALESS CHANGEFEED 2d PERMISSIONS FULL;

DEFINE FIELD created_at               ON notification TYPE none | datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD notification_description  ON notification TYPE string                             PERMISSIONS FULL;
DEFINE FIELD notification_type         ON notification TYPE string                             PERMISSIONS FULL;
DEFINE FIELD status                    ON notification TYPE string DEFAULT 'Unread'            PERMISSIONS FULL;
DEFINE FIELD user                      ON notification TYPE record<user>                       PERMISSIONS FULL;
