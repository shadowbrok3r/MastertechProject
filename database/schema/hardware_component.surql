-- ============================================================
-- TABLE: hardware_component        last changed: Stress-test tranche (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Normalized catalog of CPU / GPU / RAM / SSD / PSU / motherboard /
-- cooler models. One record per canonical part, keyed by
-- sha256(kind|vendor|model) so the same RTX 4070 SUPER collapses
-- to one row across every PC we've tested. Rust source of truth:
-- `database::schema::stress_test::HardwareComponent`.

DEFINE TABLE hardware_component TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- Identity uniqueness: defend against double-inserts when the
-- canonical-id hash changes (e.g. normalization rule updates).
DEFINE INDEX hw_component_identity ON hardware_component
    FIELDS kind, vendor, model UNIQUE;

-- Common faceting indexes — most queries either pick a kind
-- ("show me every GPU we know about") or search by display name.
DEFINE INDEX hw_component_kind    ON hardware_component FIELDS kind;
DEFINE INDEX hw_component_display ON hardware_component FIELDS display_name;

DEFINE FIELD kind             ON hardware_component TYPE string                PERMISSIONS FULL;
DEFINE FIELD vendor           ON hardware_component TYPE string                PERMISSIONS FULL;
DEFINE FIELD model            ON hardware_component TYPE string                PERMISSIONS FULL;
DEFINE FIELD sku              ON hardware_component TYPE none | string         PERMISSIONS FULL;
DEFINE FIELD display_name     ON hardware_component TYPE string                PERMISSIONS FULL;
-- Free-form spec object — fields differ across kinds (cores/threads
-- for CPUs, vram_gb for GPUs, capacity_gb/speed_mts/timings for RAM,
-- capacity_tb/tbw/interface for SSDs, wattage/efficiency for PSUs).
DEFINE FIELD specs            ON hardware_component TYPE none | object         PERMISSIONS FULL;
DEFINE FIELD first_seen       ON hardware_component TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD last_seen        ON hardware_component TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD occurrence_count ON hardware_component TYPE number   DEFAULT 0    PERMISSIONS FULL;
-- 768-dim embedding; set by the Rust app via fn::embed_text at write time
-- (no DB-side VALUE: it would recompute on every write and override the app).
DEFINE FIELD embedding        ON hardware_component TYPE array<float>          PERMISSIONS FULL;
DEFINE FIELD embedding.*      ON hardware_component TYPE float                 PERMISSIONS FULL;

DEFINE INDEX hw_component_embedding ON hardware_component FIELDS embedding
    HNSW DIMENSION 768 DIST COSINE TYPE F32 EFC 150 M 12 M0 24;
