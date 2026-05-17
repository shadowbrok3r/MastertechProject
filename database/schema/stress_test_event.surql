-- ============================================================
-- TABLE: stress_test_event         last changed: Stress-test tranche (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Discrete events surfaced during (or shortly after) a stress run:
-- stage transitions, WHEA hits, TDRs, throttle threshold crossings,
-- disk I/O errors, BSODs, kernel-power 41 shutdowns, operator notes.
-- Lower-volume than `stress_test_metric` (typically <100 per run) so
-- keys stay UUID-based; lookups go through (run_ref, at) and (kind).
-- Rust source of truth: `database::schema::stress_test::StressTestEvent`.

DEFINE TABLE stress_test_event TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- Pull a single run's event timeline.
DEFINE INDEX stevt_run_time ON stress_test_event FIELDS run_ref, at;

-- "Find every WHEA event on this CPU model across every test."
-- `kind` is one of: 'stage_started' | 'stage_finished' | 'disk_io_error' |
-- 'whea_hit' | 'tdr' | 'thermal_throttle' | 'vrm_throttle' | 'memory_error' |
-- 'bsod' | 'unexpected_shutdown' | 'operator_note' | 'custom'.
DEFINE INDEX stevt_kind ON stress_test_event FIELDS kind;

DEFINE FIELD run_ref ON stress_test_event TYPE record<stress_test_run>     PERMISSIONS FULL;
DEFINE FIELD at      ON stress_test_event TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD kind    ON stress_test_event TYPE string                       PERMISSIONS FULL;
-- Where the event came from: "stress-kit", "hwsampler", "windows-event-log",
-- "operator". Lets us trust some sources more than others when reconciling.
DEFINE FIELD source  ON stress_test_event TYPE string                       PERMISSIONS FULL;
-- Vendor-specific code (BSOD bugcheck, WHEA error code, SMART error id, …).
DEFINE FIELD code    ON stress_test_event TYPE none | string                PERMISSIONS FULL;
DEFINE FIELD detail  ON stress_test_event TYPE string DEFAULT ''            PERMISSIONS FULL;
-- Structured payload — varies by event kind, schema-free on purpose.
DEFINE FIELD data    ON stress_test_event TYPE none | object                PERMISSIONS FULL;
