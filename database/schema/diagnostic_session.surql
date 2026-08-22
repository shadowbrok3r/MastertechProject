-- ============================================================
-- TABLE: diagnostic_session
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE diagnostic_session TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD computer_id       ON diagnostic_session TYPE none | record<computer>          PERMISSIONS FULL;
DEFINE FIELD connection_string ON diagnostic_session TYPE string                           PERMISSIONS FULL;
DEFINE FIELD customer_id       ON diagnostic_session TYPE none | record<customer>          PERMISSIONS FULL;
DEFINE FIELD customer_name     ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD diagnosed_at      ON diagnostic_session TYPE none | datetime                  PERMISSIONS FULL;
DEFINE FIELD diagnosed_by      ON diagnostic_session TYPE none | string ASSERT $value == NONE OR string::matches($value, '^(mcp|zeroclaw|cron|tech|legacy)/[a-z0-9][a-z0-9._-]*(@[a-z0-9][a-z0-9._:-]*)?(#[a-z0-9][a-z0-9._-]*)?$') PERMISSIONS FULL;
DEFINE FIELD driven_by         ON diagnostic_session TYPE none | string ASSERT $value == NONE OR string::matches($value, '^(mcp|zeroclaw|cron|tech|legacy)/[a-z0-9][a-z0-9._-]*(@[a-z0-9][a-z0-9._:-]*)?(#[a-z0-9][a-z0-9._-]*)?$') PERMISSIONS FULL;
DEFINE FIELD ended_at          ON diagnostic_session TYPE none | datetime                  PERMISSIONS FULL;
DEFINE FIELD hostname          ON diagnostic_session TYPE string                           PERMISSIONS FULL;
DEFINE FIELD outcome_override        ON diagnostic_session TYPE none | string ASSERT $value == NONE OR $value IN ['confirmed_fixed', 'comeback', 'excluded'] PERMISSIONS FULL;
DEFINE FIELD outcome_override_at     ON diagnostic_session TYPE none | datetime PERMISSIONS FULL;
DEFINE FIELD outcome_override_by     ON diagnostic_session TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD outcome_override_reason ON diagnostic_session TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD requested_by      ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD service_order     ON diagnostic_session TYPE none | record<service_order>     PERMISSIONS FULL;
DEFINE FIELD started_at        ON diagnostic_session TYPE datetime DEFAULT time::now()      PERMISSIONS FULL;
DEFINE FIELD status            ON diagnostic_session TYPE string ASSERT $value IN ['open', 'resolved', 'escalated', 'abandoned'] PERMISSIONS FULL;
DEFINE FIELD store             ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD summary           ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD tags              ON diagnostic_session TYPE array<string>                    PERMISSIONS FULL;
DEFINE FIELD tags.*            ON diagnostic_session TYPE string                           PERMISSIONS FULL;
DEFINE FIELD task_ref          ON diagnostic_session TYPE none | record<task>              PERMISSIONS FULL;
DEFINE FIELD tech              ON diagnostic_session TYPE none | string DEFAULT $auth.email PERMISSIONS FULL;
-- zeroclaw session key for the technician conversation this came out of.
DEFINE FIELD zeroclaw_session ON diagnostic_session TYPE none | string                    PERMISSIONS FULL;
