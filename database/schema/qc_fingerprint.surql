-- ============================================================
-- TABLE: qc_fingerprint            last changed: 2026-07-28
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Pre-OS hardware fingerprint written by axum_server on
-- POST /api/v1/qc/fingerprint (HTTP, TCP and UDP ingest all land here).
--
-- The table was previously never defined, so SurrealDB auto-created it on
-- first write with the default `PERMISSIONS NONE`. axum_server authenticates
-- as the `guest` record-access user, so every UPSERT was permission-filtered
-- into a no-op that returned an empty result set instead of an error.
-- PERMISSIONS FULL matches every other axum-ingest table.
--
-- `raw` is deliberately left undefined so the SCHEMALESS table keeps the
-- agent's document verbatim.
--
-- Rust source of truth: `database::schema::qc_fingerprint::HardwareFingerprint`.

DEFINE TABLE qc_fingerprint TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- Lookup by serial without going through the record id.
DEFINE INDEX qcfp_serial      ON qc_fingerprint FIELDS serial;

-- Time-window scans for the fleet dashboard.
DEFINE INDEX qcfp_captured_at ON qc_fingerprint FIELDS captured_at;

DEFINE FIELD serial      ON qc_fingerprint TYPE string   DEFAULT ''          PERMISSIONS FULL;
DEFINE FIELD uuid        ON qc_fingerprint TYPE string   DEFAULT ''          PERMISSIONS FULL;
DEFINE FIELD captured_at ON qc_fingerprint TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD cpu_model   ON qc_fingerprint TYPE string   DEFAULT ''          PERMISSIONS FULL;
DEFINE FIELD cpu_cores   ON qc_fingerprint TYPE number   DEFAULT 0           PERMISSIONS FULL;
DEFINE FIELD ram_bytes   ON qc_fingerprint TYPE number   DEFAULT 0           PERMISSIONS FULL;
DEFINE FIELD dimm_count  ON qc_fingerprint TYPE number   DEFAULT 0           PERMISSIONS FULL;
DEFINE FIELD disk_count  ON qc_fingerprint TYPE number   DEFAULT 0           PERMISSIONS FULL;
DEFINE FIELD win11_ready ON qc_fingerprint TYPE bool     DEFAULT false       PERMISSIONS FULL;
