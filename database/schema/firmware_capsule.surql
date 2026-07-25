-- ============================================================
-- TABLE: firmware_capsule         last changed: BIOS capsule rail (2026-07-25)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Every vendor BIOS capsule the fleet is allowed to flash. The
-- firmware app is never handed an operator-supplied URL: a
-- bios_update command names a `capsule_id`, the orchestrator
-- rewrites that to its own /api/v1/firmware/capsules/{id} route,
-- and the bytes are served from the `capsules` bucket. So this
-- table is the complete allow-list of flashable images.
--
-- The row is keyed by `capsule_id` (not a random uuid) so lookups
-- are O(1) and a duplicate publish is an upsert, never a second row.
--
-- `sha256` is verified on every fetch — a bucket swap cannot
-- silently change what a machine flashes.
--
-- The `capsules` bucket itself is defined in code, not here: its
-- backend path differs per environment, so `FirmwareCapsule::publish`
-- calls `define_bucket` (IF NOT EXISTS) the same way user buckets do.
--
-- Rust source of truth: `database::schema::firmware::FirmwareCapsule`.

DEFINE TABLE firmware_capsule TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

-- Operator-facing key; also the record id, so this is a uniqueness assertion.
DEFINE INDEX fwcap_capsule_id     ON firmware_capsule FIELDS capsule_id UNIQUE;

-- "Which capsules exist for this board" — the picker query.
DEFINE INDEX fwcap_board_product  ON firmware_capsule FIELDS board_product;

-- Cross-check a capsule against the ESRT fw_class a machine reported.
DEFINE INDEX fwcap_fw_class       ON firmware_capsule FIELDS fw_class;

-- Newest-first listing.
DEFINE INDEX fwcap_published_at   ON firmware_capsule FIELDS published_at;

-- --- Identification ---
DEFINE FIELD capsule_id       ON firmware_capsule TYPE string              PERMISSIONS FULL;
DEFINE FIELD board_product    ON firmware_capsule TYPE string              PERMISSIONS FULL;
DEFINE FIELD fw_class         ON firmware_capsule TYPE string              PERMISSIONS FULL;

-- --- Versioning (compared against the ESRT system-firmware entry) ---
DEFINE FIELD version          ON firmware_capsule TYPE int                 PERMISSIONS FULL;
DEFINE FIELD lowest_supported ON firmware_capsule TYPE option<int>         PERMISSIONS FULL;

-- --- Artifact ---
DEFINE FIELD sha256           ON firmware_capsule TYPE string              PERMISSIONS FULL;
DEFINE FIELD size_bytes       ON firmware_capsule TYPE int                 PERMISSIONS FULL;
DEFINE FIELD bucket_path      ON firmware_capsule TYPE option<string>      PERMISSIONS FULL;

-- --- Provenance ---
DEFINE FIELD published_by     ON firmware_capsule TYPE string              PERMISSIONS FULL;
DEFINE FIELD published_at     ON firmware_capsule TYPE datetime            PERMISSIONS FULL;
DEFINE FIELD notes            ON firmware_capsule TYPE option<string>      PERMISSIONS FULL;
