-- ============================================================
-- TABLE: task_note
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE task_note TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX note_idx ON task_note FIELDS task_id, id_customer_thread, id_customer_message, service_number;

DEFINE FIELD created_at          ON task_note TYPE datetime DEFAULT time::now()       PERMISSIONS FULL;
DEFINE FIELD id_customer_message ON task_note TYPE none | string DEFAULT NONE          PERMISSIONS FULL;
DEFINE FIELD id_customer_thread  ON task_note TYPE none | string DEFAULT NONE          PERMISSIONS FULL;
DEFINE FIELD note                ON task_note TYPE string                              PERMISSIONS FULL;
DEFINE FIELD private             ON task_note TYPE bool DEFAULT false                  PERMISSIONS FULL;
DEFINE FIELD service_number      ON task_note TYPE none | string DEFAULT ''            PERMISSIONS FULL;
DEFINE FIELD task_id             ON task_note TYPE record<task>                        PERMISSIONS FULL;
DEFINE FIELD user                ON task_note TYPE record<user> DEFAULT $auth.id       PERMISSIONS FULL;
DEFINE FIELD username            ON task_note TYPE string DEFAULT ''                   PERMISSIONS FULL;
