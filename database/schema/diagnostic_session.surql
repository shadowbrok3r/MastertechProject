-- ============================================================
-- TABLE: diagnostic_session
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE diagnostic_session TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD computer_id       ON diagnostic_session TYPE none | record<computer>          PERMISSIONS FULL;
DEFINE FIELD connection_string ON diagnostic_session TYPE string                           PERMISSIONS FULL;
DEFINE FIELD customer_id       ON diagnostic_session TYPE none | record<customer>          PERMISSIONS FULL;
DEFINE FIELD customer_name     ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD ended_at          ON diagnostic_session TYPE none | datetime                  PERMISSIONS FULL;
DEFINE FIELD hostname          ON diagnostic_session TYPE string                           PERMISSIONS FULL;
DEFINE FIELD service_order     ON diagnostic_session TYPE none | record<service_order>     PERMISSIONS FULL;
DEFINE FIELD started_at        ON diagnostic_session TYPE datetime DEFAULT time::now()      PERMISSIONS FULL;
DEFINE FIELD status            ON diagnostic_session TYPE string ASSERT $value IN ['open', 'resolved', 'escalated'] PERMISSIONS FULL;
DEFINE FIELD summary           ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD tags              ON diagnostic_session TYPE array<string>                    PERMISSIONS FULL;
DEFINE FIELD tags.*            ON diagnostic_session TYPE string                           PERMISSIONS FULL;
DEFINE FIELD task_ref          ON diagnostic_session TYPE none | record<task>              PERMISSIONS FULL;
DEFINE FIELD tech              ON diagnostic_session TYPE none | string DEFAULT $auth.email PERMISSIONS FULL;
