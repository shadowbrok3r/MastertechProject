-- ============================================================
-- TABLE: shelf_candidate
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- Ranked AI-assist candidates from the Check-in Shelf triage sweep. One row
-- per service number; a later sweep overwrites its own earlier verdict.
-- ============================================================

DEFINE TABLE shelf_candidate TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD service_number ON shelf_candidate TYPE string PERMISSIONS FULL;
DEFINE FIELD store          ON shelf_candidate TYPE none | string PERMISSIONS FULL;
DEFINE FIELD score          ON shelf_candidate TYPE int ASSERT $value >= 0 AND $value <= 100 PERMISSIONS FULL;
DEFINE FIELD reason         ON shelf_candidate TYPE string
    ASSERT string::len($value) <= 600 PERMISSIONS FULL;
DEFINE FIELD device         ON shelf_candidate TYPE none | string PERMISSIONS FULL;
DEFINE FIELD waiting_open_hours ON shelf_candidate TYPE none | number PERMISSIONS FULL;
DEFINE FIELD swept_at       ON shelf_candidate TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD notified       ON shelf_candidate TYPE bool DEFAULT false PERMISSIONS FULL;

DEFINE INDEX shelf_candidate_swept_idx ON shelf_candidate FIELDS swept_at;
