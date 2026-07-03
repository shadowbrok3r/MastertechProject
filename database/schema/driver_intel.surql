-- ============================================================
-- TABLES: driver_snapshot / known_bad_driver
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Driver time machine. `driver_snapshot` captures a machine's full
-- driver inventory (pnputil or WMI) at a point in time — taken at
-- intake and before service work so any driver change is diffable
-- and attributable. `known_bad_driver` is the fleet blocklist that
-- intake triage matches inventories and crash signatures against.
-- Rust source of truth: `database::schema::driver_intel`.

DEFINE TABLE driver_snapshot TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX dsnap_connection   ON driver_snapshot FIELDS connection_string;
DEFINE INDEX dsnap_computer     ON driver_snapshot FIELDS computer;
DEFINE INDEX dsnap_taken_at     ON driver_snapshot FIELDS taken_at;

DEFINE FIELD connection_string  ON driver_snapshot TYPE string                          PERMISSIONS FULL;
DEFINE FIELD computer           ON driver_snapshot TYPE none | record<computer>         PERMISSIONS FULL;
DEFINE FIELD session_ref        ON driver_snapshot TYPE none | record<diagnostic_session> PERMISSIONS FULL;
-- 'intake' | 'pre_service' | 'post_service' | 'manual'
DEFINE FIELD label              ON driver_snapshot TYPE string DEFAULT 'manual'         PERMISSIONS FULL;
-- 'pnputil' | 'wmi'
DEFINE FIELD source             ON driver_snapshot TYPE string DEFAULT 'pnputil'        PERMISSIONS FULL;
DEFINE FIELD taken_at           ON driver_snapshot TYPE datetime DEFAULT time::now()    PERMISSIONS FULL;
DEFINE FIELD driver_count       ON driver_snapshot TYPE int DEFAULT 0                   PERMISSIONS FULL;
DEFINE FIELD drivers            ON driver_snapshot TYPE array<object> DEFAULT []        PERMISSIONS FULL;
DEFINE FIELD drivers.*          ON driver_snapshot TYPE object                          PERMISSIONS FULL;
DEFINE FIELD notes              ON driver_snapshot TYPE string DEFAULT ''               PERMISSIONS FULL;

-- Fleet driver blocklist matched during triage.
DEFINE TABLE known_bad_driver TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX kbd_module         ON known_bad_driver FIELDS module;
DEFINE INDEX kbd_active         ON known_bad_driver FIELDS active;

-- File stem without extension, lowercase ('rtwlane', 'amdkmdag').
DEFINE FIELD module             ON known_bad_driver TYPE string                         PERMISSIONS FULL;
DEFINE FIELD display_name       ON known_bad_driver TYPE string DEFAULT ''              PERMISSIONS FULL;
DEFINE FIELD vendor             ON known_bad_driver TYPE string DEFAULT ''              PERMISSIONS FULL;
-- Exact-or-prefix version matchers; empty matches every version.
DEFINE FIELD bad_versions       ON known_bad_driver TYPE array<string> DEFAULT []       PERMISSIONS FULL;
DEFINE FIELD bad_versions.*     ON known_bad_driver TYPE string                         PERMISSIONS FULL;
DEFINE FIELD fixed_version      ON known_bad_driver TYPE none | string                  PERMISSIONS FULL;
DEFINE FIELD symptom            ON known_bad_driver TYPE string DEFAULT ''              PERMISSIONS FULL;
DEFINE FIELD fix                ON known_bad_driver TYPE string DEFAULT ''              PERMISSIONS FULL;
-- 'info' | 'warn' | 'critical'
DEFINE FIELD severity           ON known_bad_driver TYPE string DEFAULT 'warn'          PERMISSIONS FULL;
DEFINE FIELD signature_ref      ON known_bad_driver TYPE none | record<crash_signature> PERMISSIONS FULL;
DEFINE FIELD active             ON known_bad_driver TYPE bool DEFAULT true              PERMISSIONS FULL;
DEFINE FIELD created_at         ON known_bad_driver TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;
DEFINE FIELD updated_at         ON known_bad_driver TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;
