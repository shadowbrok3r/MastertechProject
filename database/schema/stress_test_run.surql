-- ============================================================
-- TABLE: stress_test_run           last changed: Stress-test tranche (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One execution of one stress tool against one computer. Carries
-- run metadata, the tool config, BIOS/driver context, an outcome
-- verdict, and a summary rollup of telemetry observed during the
-- run. Telemetry samples themselves live in `stress_test_metric`
-- (linked via `run_ref`); discrete events in `stress_test_event`.
-- Rust source of truth: `database::schema::stress_test::StressTestRun`.

DEFINE TABLE stress_test_run TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

-- "History of this machine" — primary AI lookup path.
DEFINE INDEX strun_computer         ON stress_test_run FIELDS computer;

-- "Every run that exercised this canonical chip" — drives the
-- cross-shop analytics ("for every RTX 4070 SUPER ever, what shows up?").
DEFINE INDEX strun_target_component ON stress_test_run FIELDS target_component;

-- Pull every run for a ticket / diagnostic session / task.
DEFINE INDEX strun_service_order    ON stress_test_run FIELDS service_order;
DEFINE INDEX strun_session          ON stress_test_run FIELDS session_ref;
DEFINE INDEX strun_task             ON stress_test_run FIELDS task_ref;

-- `tool_label` is the lowercase snake_case discriminant maintained by
-- `TestTool::label()` ("prime95", "occt", "stresskit:cpu", "memtest86", …).
-- Lets analytic queries filter on tool family without destructuring the
-- structured `tool` object.
DEFINE INDEX strun_tool_label       ON stress_test_run FIELDS tool_label;
DEFINE INDEX strun_target_kind      ON stress_test_run FIELDS target_kind;
DEFINE INDEX strun_result           ON stress_test_run FIELDS result;
DEFINE INDEX strun_failure_kind     ON stress_test_run FIELDS failure_kind;
DEFINE INDEX strun_started_at       ON stress_test_run FIELDS started_at;
DEFINE INDEX strun_tags             ON stress_test_run FIELDS tags;

-- HNSW vector index for "find runs that look like this one" (root-cause
-- triage, regression hunts, anomaly clustering). Dimension matches the
-- sentence-transformer family we use elsewhere in the codebase
-- (`all-mpnet-base-v2`-class, 768-dim). Cosine distance because embeddings
-- come back L2-normalized. EFC / M tuned per the surrealdb-vector skill's
-- "stress-test sized table" defaults — re-tune once the table grows past
-- a few million rows.
DEFINE INDEX strun_embedding ON stress_test_run FIELDS embedding
    HNSW DIMENSION 768 DIST COSINE TYPE F32 EFC 150 M 12 M0 24;

-- --- Links ---
DEFINE FIELD computer            ON stress_test_run TYPE record<computer>                           PERMISSIONS FULL;
DEFINE FIELD service_order       ON stress_test_run TYPE none | record<service_order>              PERMISSIONS FULL;
DEFINE FIELD session_ref         ON stress_test_run TYPE none | record<diagnostic_session>         PERMISSIONS FULL;
DEFINE FIELD task_ref            ON stress_test_run TYPE none | record<task>                       PERMISSIONS FULL;

-- --- What was tested ---
DEFINE FIELD target_kind         ON stress_test_run TYPE string                                     PERMISSIONS FULL;
DEFINE FIELD target_component    ON stress_test_run TYPE none | record<hardware_component>         PERMISSIONS FULL;
DEFINE FIELD touched_components  ON stress_test_run TYPE array<record<hardware_component>> DEFAULT [] PERMISSIONS FULL;
DEFINE FIELD touched_components.* ON stress_test_run TYPE record<hardware_component>               PERMISSIONS FULL;

-- --- Tool & run config ---
DEFINE FIELD tool                ON stress_test_run TYPE object                                     PERMISSIONS FULL;
DEFINE FIELD tool_label          ON stress_test_run TYPE string                                     PERMISSIONS FULL;
DEFINE FIELD tool_version        ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD preset_label        ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD scenario_stages    ON stress_test_run TYPE array<object> DEFAULT []                   PERMISSIONS FULL;
DEFINE FIELD scenario_stages.*  ON stress_test_run TYPE object                                     PERMISSIONS FULL;

-- --- Timing ---
DEFINE FIELD started_at          ON stress_test_run TYPE datetime DEFAULT time::now()              PERMISSIONS FULL;
DEFINE FIELD ended_at            ON stress_test_run TYPE none | datetime                            PERMISSIONS FULL;
-- duration_planned_secs is u64 in Rust; SurrealDB `int` matches that.
-- duration_actual_secs is f64 in Rust — must be float so the
-- `Option<f64>` deserializer never sees an integer-typed value.
DEFINE FIELD duration_planned_secs ON stress_test_run TYPE none | int                              PERMISSIONS FULL;
DEFINE FIELD duration_actual_secs  ON stress_test_run TYPE none | float                            PERMISSIONS FULL;

-- --- Environment ---
DEFINE FIELD tech                ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD hostname            ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD machine_id          ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD ambient_temp_c      ON stress_test_run TYPE none | number                              PERMISSIONS FULL;
DEFINE FIELD environment_notes   ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
-- Relaxed to `none | object` so callers that don't fill these in (most
-- runs, since both Rust structs are all-Option fields that serialize to NONE
-- when default-constructed) can still write the row. Previously these were
-- required `TYPE object` and every MCP-driven run failed with "Expected
-- `object` but found `NONE`" before any record landed.
DEFINE FIELD bios_settings       ON stress_test_run TYPE none | object                              PERMISSIONS FULL;
DEFINE FIELD driver_versions     ON stress_test_run TYPE none | object                              PERMISSIONS FULL;

-- --- Verdict ---
-- `result` is one of: 'pass' | 'fail' | 'aborted' | 'inconclusive' | 'in_progress'.
-- `failure_mode` is the structured FailureMode enum (externally tagged object);
-- `failure_kind` is its lowercase tag mirrored for fast filtering.
DEFINE FIELD result              ON stress_test_run TYPE string DEFAULT 'in_progress'              PERMISSIONS FULL;
DEFINE FIELD finish_reason       ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD failure_mode        ON stress_test_run TYPE object                                     PERMISSIONS FULL;
DEFINE FIELD failure_kind        ON stress_test_run TYPE string DEFAULT 'none'                     PERMISSIONS FULL;

-- --- Aggregates rolled up from the metric stream ---
DEFINE FIELD summary             ON stress_test_run TYPE object                                     PERMISSIONS FULL;

-- --- Artifacts (bucket paths) ---
DEFINE FIELD raw_log_refs        ON stress_test_run TYPE array<string> DEFAULT []                  PERMISSIONS FULL;
DEFINE FIELD raw_log_refs.*      ON stress_test_run TYPE string                                     PERMISSIONS FULL;
DEFINE FIELD screenshot_refs     ON stress_test_run TYPE array<string> DEFAULT []                  PERMISSIONS FULL;
DEFINE FIELD screenshot_refs.*   ON stress_test_run TYPE string                                     PERMISSIONS FULL;

-- --- Annotations ---
DEFINE FIELD notes               ON stress_test_run TYPE none | string                              PERMISSIONS FULL;
DEFINE FIELD ai_assessment       ON stress_test_run TYPE none | object                              PERMISSIONS FULL;
DEFINE FIELD tags                ON stress_test_run TYPE array<string> DEFAULT []                  PERMISSIONS FULL;
DEFINE FIELD tags.*              ON stress_test_run TYPE string                                     PERMISSIONS FULL;

-- --- Semantic similarity ---
-- `none | array<float>` on environments with slice10; older DBs may still
-- require `array<float>`. Rust CREATE always sets `embedding` via
-- `fn::embed_text` so the HNSW index receives a 768-dim vector.
DEFINE FIELD embedding           ON stress_test_run TYPE none | array<float>                       PERMISSIONS FULL;
DEFINE FIELD embedding.*         ON stress_test_run TYPE float                                     PERMISSIONS FULL;

-- ============================================================
-- VIEW: hardware_test_baseline    (moved from hardware_test_baseline.surql 2026-06-12)
-- ============================================================
-- Pre-computed population stats per (target_component, tool_label).
-- SurrealDB auto-refreshes this on every write to `stress_test_run`.
-- Lives in this file so the view always applies after the table it
-- selects FROM; a separate file needed a duplicate DEFINE TABLE
-- stress_test_run dependency guard, which collided with surrealkit's
-- per-entity registry sync during `rollout complete`.
--
-- Notes
--   - Runs without a `target_component` (System / Mixed kind) are
--     excluded — the per-component pivot is the whole point of this
--     view.
--   - In-progress runs are excluded so live test telemetry doesn't
--     pollute the baseline.
--   - `IF expr THEN 1 ELSE 0 END` lets us count rows matching a
--     condition inside a single `math::sum` aggregation (SurrealQL
--     doesn't have COUNT(CASE WHEN)).
--   - Rust source of truth (read-only):
--     `database::schema::stress_test::HardwareTestBaseline`.

DEFINE TABLE hardware_test_baseline TYPE NORMAL AS
    SELECT
        target_component                                  AS component,
        tool_label                                        AS tool,
        count()                                           AS run_count,
        math::sum(IF result = 'pass'         THEN 1 ELSE 0 END) AS pass_count,
        math::sum(IF result = 'fail'         THEN 1 ELSE 0 END) AS fail_count,
        math::sum(IF result = 'aborted'      THEN 1 ELSE 0 END) AS abort_count,
        math::sum(IF result = 'inconclusive' THEN 1 ELSE 0 END) AS inconclusive_count,
        math::mean(summary.max_temp_c ?? 0)               AS avg_max_temp_c,
        math::max(summary.max_temp_c ?? 0)                AS peak_max_temp_c,
        math::mean(summary.avg_temp_c ?? 0)               AS avg_temp_c,
        math::mean(summary.max_clock_mhz ?? 0)            AS avg_max_clock_mhz,
        math::mean(summary.avg_clock_mhz ?? 0)            AS avg_clock_mhz,
        math::mean(summary.max_power_w ?? 0)              AS avg_max_power_w,
        math::mean(summary.peak_throughput ?? 0)          AS avg_peak_throughput,
        math::mean(summary.whea_delta_count ?? 0)         AS avg_whea_delta,
        math::sum(summary.whea_delta_count ?? 0)          AS total_whea_delta,
        math::sum(summary.disk_io_errors ?? 0)            AS total_disk_io_errors,
        math::sum(summary.memory_errors ?? 0)             AS total_memory_errors,
        math::sum(IF summary.thermal_throttle_detected = true THEN 1 ELSE 0 END) AS throttle_count,
        math::sum(IF summary.bsod_detected = true             THEN 1 ELSE 0 END) AS bsod_count
    FROM stress_test_run
    WHERE target_component != NONE
        AND result != 'in_progress'
    GROUP BY component, tool
    PERMISSIONS FULL;
