-- ============================================================
-- TABLES: crash_signature / crash_sighting / crash_verdict
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Fleet crash-signature intelligence. One `crash_signature` row per
-- normalized (bugcheck_code, module) pair, upserted in place with a
-- deterministic id. Every decoded dump appends a `crash_sighting`;
-- techs/AI record `crash_verdict` rows (diagnosis + fix) against the
-- signature so the next machine that crashes the same way surfaces
-- the known answer immediately.
-- Rust source of truth: `database::schema::crash_intel`.

DEFINE TABLE crash_signature TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX csig_module        ON crash_signature FIELDS module;
DEFINE INDEX csig_bugcheck      ON crash_signature FIELDS bugcheck_code;
DEFINE INDEX csig_last_seen     ON crash_signature FIELDS last_seen;

DEFINE FIELD bugcheck_code      ON crash_signature TYPE string                          PERMISSIONS FULL;
DEFINE FIELD bugcheck_name      ON crash_signature TYPE string DEFAULT ''               PERMISSIONS FULL;
DEFINE FIELD module             ON crash_signature TYPE string                          PERMISSIONS FULL;
DEFINE FIELD offsets            ON crash_signature TYPE array<string> DEFAULT []        PERMISSIONS FULL;
DEFINE FIELD offsets.*          ON crash_signature TYPE string                          PERMISSIONS FULL;
DEFINE FIELD module_versions    ON crash_signature TYPE array<string> DEFAULT []        PERMISSIONS FULL;
DEFINE FIELD module_versions.*  ON crash_signature TYPE string                          PERMISSIONS FULL;
DEFINE FIELD failure_buckets    ON crash_signature TYPE array<string> DEFAULT []        PERMISSIONS FULL;
DEFINE FIELD failure_buckets.*  ON crash_signature TYPE string                          PERMISSIONS FULL;
DEFINE FIELD machines           ON crash_signature TYPE array<string> DEFAULT []        PERMISSIONS FULL;
DEFINE FIELD machines.*         ON crash_signature TYPE string                          PERMISSIONS FULL;
DEFINE FIELD sighting_count     ON crash_signature TYPE int DEFAULT 0                   PERMISSIONS FULL;
DEFINE FIELD first_seen         ON crash_signature TYPE datetime DEFAULT time::now()    PERMISSIONS FULL;
DEFINE FIELD last_seen          ON crash_signature TYPE datetime DEFAULT time::now()    PERMISSIONS FULL;
DEFINE FIELD latest_verdict     ON crash_signature TYPE none | record<crash_verdict>    PERMISSIONS FULL;
DEFINE FIELD tags               ON crash_signature TYPE array<string> DEFAULT []        PERMISSIONS FULL;
DEFINE FIELD tags.*             ON crash_signature TYPE string                          PERMISSIONS FULL;

-- One row per decoded dump occurrence.
DEFINE TABLE crash_sighting TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX csight_signature   ON crash_sighting FIELDS signature;
DEFINE INDEX csight_connection  ON crash_sighting FIELDS connection_string;
DEFINE INDEX csight_computer    ON crash_sighting FIELDS computer;
DEFINE INDEX csight_created     ON crash_sighting FIELDS created_at;

DEFINE FIELD signature          ON crash_sighting TYPE record<crash_signature>          PERMISSIONS FULL;
DEFINE FIELD connection_string  ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD computer           ON crash_sighting TYPE none | record<computer>          PERMISSIONS FULL;
DEFINE FIELD session_ref        ON crash_sighting TYPE none | record<diagnostic_session> PERMISSIONS FULL;
DEFINE FIELD task_ref           ON crash_sighting TYPE none | record<task>              PERMISSIONS FULL;
DEFINE FIELD dump_name          ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD dump_kind          ON crash_sighting TYPE string DEFAULT 'minidump'        PERMISSIONS FULL;
DEFINE FIELD dump_time          ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD offset             ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD module_version     ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD failure_bucket     ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD process_name       ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD caused_by          ON crash_sighting TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD raw_excerpt        ON crash_sighting TYPE string DEFAULT ''                PERMISSIONS FULL;
DEFINE FIELD created_at         ON crash_sighting TYPE datetime DEFAULT time::now()     PERMISSIONS FULL;

-- Diagnosis + fix recorded against a signature.
DEFINE TABLE crash_verdict TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX cverd_signature    ON crash_verdict FIELDS signature;
DEFINE INDEX cverd_created      ON crash_verdict FIELDS created_at;

DEFINE FIELD signature          ON crash_verdict TYPE record<crash_signature>           PERMISSIONS FULL;
DEFINE FIELD verdict            ON crash_verdict TYPE string                            PERMISSIONS FULL;
DEFINE FIELD fix                ON crash_verdict TYPE string DEFAULT ''                 PERMISSIONS FULL;
-- 'low' | 'medium' | 'high' | 'confirmed'
DEFINE FIELD confidence         ON crash_verdict TYPE string DEFAULT 'medium'           PERMISSIONS FULL;
DEFINE FIELD author             ON crash_verdict TYPE string DEFAULT ''                 PERMISSIONS FULL;
-- 'tech' | 'ai' | 'autopilot'
DEFINE FIELD source             ON crash_verdict TYPE string DEFAULT 'tech'             PERMISSIONS FULL;
DEFINE FIELD task_ref           ON crash_verdict TYPE none | record<task>               PERMISSIONS FULL;
DEFINE FIELD created_at         ON crash_verdict TYPE datetime DEFAULT time::now()      PERMISSIONS FULL;
