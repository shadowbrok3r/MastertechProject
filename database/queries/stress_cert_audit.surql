-- Read-only audit of persisted stress_test_run sign-offs. Nothing here writes:
-- a flagged run is a re-test candidate, not a row to rewrite.
--
-- Runs recorded before the verdict gate in stress-runner/src/controller.rs
-- (`ended_short_of_plan` / `missing_work_evidence`) could be stored as
-- result='pass', finish_reason='completed' after ending at any fraction of
-- their planned duration: a run that dies before its stressors produce data
-- logs no rule violation precisely because it collected none.
--
-- Run with the query_surrealdb MCP tool, or `surreal sql` against the
-- surrealdb-dev instance.

-- ---------------------------------------------------------------------------
-- 1. Sign-offs with nothing behind them.
--
-- Flags a passing run that ended materially short of its plan, or that never
-- recorded throughput. The tolerance mirrors SHORT_RUN_TOLERANCE_PCT /
-- SHORT_RUN_TOLERANCE_SECS: max(3% of plan, 5s).
--
-- Reading the context columns:
--   stages_recorded       stages that reached their end. Compare against a
--                         healthy run of the same preset (query 2) — far fewer
--                         means the run died with stages unrun.
--   staged_planned_secs   planned seconds of the stages that did finish.
--   covers_recorded_stages
--                         the run covered every stage it recorded, so the
--                         shortfall is missing stages, not a truncated one.
--                         True with a full stages_recorded means the plan's
--                         `total_wall_secs` cap simply exceeded the stage list
--                         — that run is short on paper only.
-- Rows before 2026-06-13 predate scenario_stages persistence and report
-- stages_recorded = 0 regardless of how they ran.
-- ---------------------------------------------------------------------------
SELECT
    id,
    hostname,
    preset_label,
    started_at,
    result,
    finish_reason,
    duration_planned_secs,
    duration_actual_secs,
    math::round((duration_actual_secs ?? 0) / duration_planned_secs * 100)   AS pct_of_plan,
    array::len(scenario_stages)                                             AS stages_recorded,
    math::sum(scenario_stages.duration_planned_secs)                        AS staged_planned_secs,
    (duration_actual_secs ?? 0) >= math::sum(scenario_stages.duration_planned_secs)
                                                                            AS covers_recorded_stages,
    summary.peak_throughput                                                 AS peak_throughput,
    summary.max_gpu_temp_c                                                  AS max_gpu_temp_c,
    summary.max_power_w                                                     AS max_power_w,
    service_order,
    target_component
FROM stress_test_run
WHERE result = 'pass'
    AND duration_planned_secs != NONE
    AND duration_planned_secs > 0
    AND (
        (duration_actual_secs ?? 0)
            < duration_planned_secs - math::max([duration_planned_secs * 0.03, 5])
        OR summary.peak_throughput = NONE
    )
ORDER BY started_at DESC;

-- ---------------------------------------------------------------------------
-- 2. What a healthy run of each preset looks like, for comparison with the
--    stages_recorded and duration of a flagged row above.
-- ---------------------------------------------------------------------------
SELECT
    preset_label,
    count()                                     AS runs,
    math::max(array::len(scenario_stages))      AS max_stages_recorded,
    math::max(duration_actual_secs ?? 0)        AS max_duration_actual_secs,
    math::max(duration_planned_secs ?? 0)       AS max_duration_planned_secs
FROM stress_test_run
WHERE result = 'pass'
    AND preset_label != NONE
GROUP BY preset_label
ORDER BY preset_label;

-- ---------------------------------------------------------------------------
-- 3. Zombie rows: created, never finalized. The client hung, rebooted, or lost
--    the process before the finalize write. `stress_runs_reap` (MCP) closes
--    these out — it is on-demand only, nothing calls it on a schedule.
-- ---------------------------------------------------------------------------
SELECT
    id,
    hostname,
    preset_label,
    started_at,
    duration_planned_secs,
    duration_actual_secs,
    array::len(scenario_stages) AS stages_recorded
FROM stress_test_run
WHERE result = 'in_progress'
    AND started_at < time::now() - 1d
ORDER BY started_at DESC;
