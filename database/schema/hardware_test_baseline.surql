-- ============================================================
-- TABLE: hardware_test_baseline    last changed: Stress-test tranche (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Pre-computed population stats per (target_component, tool_label).
-- SurrealDB auto-refreshes this on every write to `stress_test_run`,
-- so the AI can ask "for an RTX 4070 SUPER under FurMark, what does
-- the population look like?" in a single bounded query.
--
-- Notes
--   - Runs without a `target_component` (System / Mixed kind) are
--     excluded — the per-component pivot is the whole point of this
--     view. A separate per-(computer, tool) baseline can be added
--     later if cross-machine system-wide stats become useful.
--   - In-progress runs are excluded so live test telemetry doesn't
--     pollute the baseline.
--   - `IF expr THEN 1 ELSE 0 END` lets us count rows matching a
--     condition inside a single `math::sum` aggregation (SurrealQL
--     doesn't have COUNT(CASE WHEN)).
--   - Rust source of truth (read-only):
--     `database::schema::stress_test::HardwareTestBaseline`.

-- Dependency guard: the view below selects FROM stress_test_run.
-- surrealkit applies files alphabetically so 'h' beats 's'.
-- Declaring the bare table here lets the view compile regardless of
-- apply order; stress_test_run.surql then adds all field/index defs.
DEFINE TABLE stress_test_run TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

DEFINE TABLE hardware_test_baseline TYPE NORMAL AS
    SELECT
        target_component                                  AS component,
        tool_label                                        AS tool,
        count()                                           AS run_count,
        math::sum(IF result = 'pass'         THEN 1 ELSE 0 END) AS pass_count,
        math::sum(IF result = 'fail'         THEN 1 ELSE 0 END) AS fail_count,
        math::sum(IF result = 'aborted'      THEN 1 ELSE 0 END) AS abort_count,
        math::sum(IF result = 'inconclusive' THEN 1 ELSE 0 END) AS inconclusive_count,
        math::mean(summary.max_temp_c)                    AS avg_max_temp_c,
        math::max(summary.max_temp_c)                     AS peak_max_temp_c,
        math::mean(summary.avg_temp_c)                    AS avg_temp_c,
        math::mean(summary.max_clock_mhz)                 AS avg_max_clock_mhz,
        math::mean(summary.avg_clock_mhz)                 AS avg_clock_mhz,
        math::mean(summary.max_power_w)                   AS avg_max_power_w,
        math::mean(summary.peak_throughput)               AS avg_peak_throughput,
        math::mean(summary.whea_delta_count)              AS avg_whea_delta,
        math::sum(summary.whea_delta_count)               AS total_whea_delta,
        math::sum(summary.disk_io_errors)                 AS total_disk_io_errors,
        math::sum(summary.memory_errors)                  AS total_memory_errors,
        math::sum(IF summary.thermal_throttle_detected = true THEN 1 ELSE 0 END) AS throttle_count,
        math::sum(IF summary.bsod_detected = true             THEN 1 ELSE 0 END) AS bsod_count
    FROM stress_test_run
    WHERE target_component != NONE
        AND result != 'in_progress'
    GROUP BY component, tool;
