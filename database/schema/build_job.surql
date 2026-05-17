-- ============================================================
-- TABLE: build_job                last changed: Slice 4 (2026-05-17)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
--
-- A pending or completed remote `cargo build` request. The lifecycle:
--
--   1. MCP `plugin_compile_remote` CREATEs a row with status='pending'
--      and (optionally) `assigned_worker_id` pinning a specific worker.
--   2. Workers run `LIVE SELECT * FROM build_job WHERE status='pending'
--      AND (assigned_worker_id = $me OR assigned_worker_id == NONE)`
--      and atomically claim by UPDATE ... WHERE status='pending'.
--   3. Worker writes back status='done' (with `wasm_bytes`) or
--      status='failed' (with stderr).
--   4. MCP `plugin_compile_status` SELECTs the row, copies bytes into
--      the local ArtifactStore on first 'done' read.
--
-- This replaces no existing data — it's purely additive alongside the
-- WS-based BuilderWire transport, which is retained as fallback.

DEFINE TABLE build_job TYPE NORMAL SCHEMALESS CHANGEFEED 1w PERMISSIONS FULL;

DEFINE EVENT created_at_default ON build_job
    WHEN $event = 'CREATE'
    THEN {
        UPDATE $value.id SET created_at = time::now();
    };

DEFINE EVENT updated_at_touch ON build_job
    WHEN $event = 'UPDATE'
    THEN {
        UPDATE $this.id SET updated_at = time::now();
    };

DEFINE FIELD plugin_id           ON build_job TYPE string                                                             PERMISSIONS FULL;
DEFINE FIELD cargo_toml          ON build_job TYPE string                                                             PERMISSIONS FULL;
DEFINE FIELD lib_rs              ON build_job TYPE string                                                             PERMISSIONS FULL;
DEFINE FIELD target              ON build_job TYPE string DEFAULT 'wasm32-wasip1'                                     PERMISSIONS FULL;
DEFINE FIELD profile             ON build_job TYPE string DEFAULT 'release'                                           PERMISSIONS FULL;
-- Lifecycle state. Workers atomically transition pending → claimed via
-- a guarded UPDATE so two workers can't double-claim the same job.
DEFINE FIELD status              ON build_job TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'claimed', 'done', 'failed']                                                         PERMISSIONS FULL;
-- Optional pin: if set at create time, only this worker may claim
-- this job. None means any worker advertising the requested target
-- can pick it up.
DEFINE FIELD assigned_worker_id  ON build_job TYPE none | record<connected_client>                                    PERMISSIONS FULL;
DEFINE FIELD claimed_worker_id   ON build_job TYPE none | record<connected_client>                                    PERMISSIONS FULL;
DEFINE FIELD claimed_at          ON build_job TYPE none | datetime                                                    PERMISSIONS FULL;
-- The compiled .wasm payload, stored inline. Plugin WASM is typically
-- ≤ a few hundred KB; if it gets larger we can shift to the file
-- bucket the same way plugin_registry does.
DEFINE FIELD wasm_bytes          ON build_job TYPE none | bytes                                                       PERMISSIONS FULL;
DEFINE FIELD stdout              ON build_job TYPE string DEFAULT ''                                                  PERMISSIONS FULL;
DEFINE FIELD stderr              ON build_job TYPE string DEFAULT ''                                                  PERMISSIONS FULL;
DEFINE FIELD duration_ms         ON build_job TYPE number DEFAULT 0                                                   PERMISSIONS FULL;
DEFINE FIELD created_at          ON build_job TYPE datetime DEFAULT time::now()                                       PERMISSIONS FULL;
DEFINE FIELD updated_at          ON build_job TYPE datetime DEFAULT time::now()                                       PERMISSIONS FULL;
