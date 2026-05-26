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
DEFINE FIELD duration_planned_secs ON stress_test_run TYPE none | number                            PERMISSIONS FULL;
DEFINE FIELD duration_actual_secs  ON stress_test_run TYPE none | number                            PERMISSIONS FULL;

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
-- `none | array<float>` so stress tests created offline (no Ollama available)
-- store NONE and the HNSW simply skips those rows. The Rust struct sends no
-- field when the Vec is empty (`skip_serializing_if = "Vec::is_empty"`), so
-- SurrealDB receives nothing and stores NONE. A backfill job or AI review pass
-- populates embeddings for completed runs.
DEFINE FIELD embedding           ON stress_test_run TYPE none | array<float>                       PERMISSIONS FULL;
DEFINE FIELD embedding.*         ON stress_test_run TYPE float                                     PERMISSIONS FULL;
