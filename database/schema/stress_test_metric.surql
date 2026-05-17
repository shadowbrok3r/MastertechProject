-- ============================================================
-- TABLE: stress_test_metric        last changed: Stress-test tranche (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One row per telemetry tick during a stress run. Default cadence is
-- 1 Hz (qc-app's HwSampler), so a 1-hour scenario writes ~3,600 rows.
-- Keep field count tight so storage and indexing stay cheap. Mirrors
-- the snapshot shape stress-kit's TelemetryAgent already produces.
-- Rust source of truth: `database::schema::stress_test::StressTestMetric`.

DEFINE TABLE stress_test_metric TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- Range-scan a single run: WHERE run_ref = $r AND captured_at >= $t1.
DEFINE INDEX stmetric_run_time ON stress_test_metric FIELDS run_ref, captured_at;

DEFINE FIELD run_ref            ON stress_test_metric TYPE record<stress_test_run>     PERMISSIONS FULL;
DEFINE FIELD captured_at        ON stress_test_metric TYPE datetime DEFAULT time::now() PERMISSIONS FULL;

-- Scenario stage context — NONE for single-tool runs.
DEFINE FIELD stage_index        ON stress_test_metric TYPE none | number               PERMISSIONS FULL;
DEFINE FIELD stage_label        ON stress_test_metric TYPE none | string               PERMISSIONS FULL;

-- Per-core sample array. Element shape:
--   { index, brand, usage_pct, freq_mhz, temp_c }
-- temp_c is almost always NONE on Windows (sysinfo limitation).
DEFINE FIELD cores              ON stress_test_metric TYPE array<object> DEFAULT []   PERMISSIONS FULL;
DEFINE FIELD cores.*            ON stress_test_metric TYPE object                      PERMISSIONS FULL;

-- Memory snapshot (used_pct + raw MB; page_file_used_pct is Win-only).
DEFINE FIELD memory_used_pct    ON stress_test_metric TYPE none | number               PERMISSIONS FULL;
DEFINE FIELD memory_used_mb     ON stress_test_metric TYPE none | number               PERMISSIONS FULL;
DEFINE FIELD page_file_used_pct ON stress_test_metric TYPE none | number               PERMISSIONS FULL;

-- Disk throughput delta samples. Element shape:
--   { name, read_mb_per_s, write_mb_per_s }
DEFINE FIELD disks              ON stress_test_metric TYPE array<object> DEFAULT []   PERMISSIONS FULL;
DEFINE FIELD disks.*            ON stress_test_metric TYPE object                      PERMISSIONS FULL;

-- Network throughput in megabits/sec. Element shape:
--   { name, rx_mbps, tx_mbps }
DEFINE FIELD networks           ON stress_test_metric TYPE array<object> DEFAULT []   PERMISSIONS FULL;
DEFINE FIELD networks.*         ON stress_test_metric TYPE object                      PERMISSIONS FULL;

-- Stress-kit `Metrics.throughput` value at this tick (units depend on
-- the active stressor — see throughput_unit).
DEFINE FIELD throughput         ON stress_test_metric TYPE none | number               PERMISSIONS FULL;
DEFINE FIELD throughput_unit    ON stress_test_metric TYPE none | string               PERMISSIONS FULL;

-- WHEA delta from the most recent 5-second WHEA-Logger scan; populated
-- only on ticks that overlap a scan, so most rows have this as NONE.
DEFINE FIELD whea_delta_count   ON stress_test_metric TYPE none | number               PERMISSIONS FULL;

-- Stress-kit `Metrics.last_error` from this tick (only the disk
-- stressor sets this today, but the column exists for future stressors).
DEFINE FIELD last_error         ON stress_test_metric TYPE none | string               PERMISSIONS FULL;
