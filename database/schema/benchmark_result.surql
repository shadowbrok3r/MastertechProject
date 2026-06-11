-- ============================================================
-- TABLE: benchmark_result           last changed: Benchmark tranche (2026-06-10)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One scored benchmark execution against one computer. Stressor-backed
-- benchmarks link the full stress_test_run via `run_ref`; one-shot
-- measurements (memory_latency ladder) persist standalone. `kind_label`
-- is the stable lowercase discriminant ("cpu_single", "memory_bandwidth",
-- "linpack", ...) that baseline queries group on.
-- Rust source of truth: `database::schema::benchmark::BenchmarkResult`.

DEFINE TABLE benchmark_result TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- "Scores for this machine" — primary lookup path.
DEFINE INDEX bench_computer    ON benchmark_result FIELDS computer;
-- "Population scores for this benchmark" — cross-machine comparisons.
DEFINE INDEX bench_kind        ON benchmark_result FIELDS kind_label;
DEFINE INDEX bench_captured_at ON benchmark_result FIELDS captured_at;
DEFINE INDEX bench_run_ref     ON benchmark_result FIELDS run_ref;

DEFINE FIELD computer       ON benchmark_result TYPE record<computer>              PERMISSIONS FULL;
DEFINE FIELD run_ref        ON benchmark_result TYPE none | record<stress_test_run> PERMISSIONS FULL;
DEFINE FIELD kind           ON benchmark_result TYPE string                         PERMISSIONS FULL;
DEFINE FIELD kind_label     ON benchmark_result TYPE string                         PERMISSIONS FULL;
-- Steady-state score; unit names the scale ("Mflop/s", "GB/s", "ns", ...).
-- memory_latency is lower-is-better; throughput kinds are higher-is-better.
DEFINE FIELD score          ON benchmark_result TYPE number                         PERMISSIONS FULL;
DEFINE FIELD unit           ON benchmark_result TYPE string                         PERMISSIONS FULL;
DEFINE FIELD peak           ON benchmark_result TYPE none | number                  PERMISSIONS FULL;
DEFINE FIELD low            ON benchmark_result TYPE none | number                  PERMISSIONS FULL;
DEFINE FIELD samples        ON benchmark_result TYPE int DEFAULT 0                  PERMISSIONS FULL;
DEFINE FIELD threads        ON benchmark_result TYPE int DEFAULT 0                  PERMISSIONS FULL;
DEFINE FIELD duration_secs  ON benchmark_result TYPE number DEFAULT 0               PERMISSIONS FULL;
-- Non-zero errors invalidate the score for baseline purposes.
DEFINE FIELD errors         ON benchmark_result TYPE int DEFAULT 0                  PERMISSIONS FULL;
DEFINE FIELD max_temp_c     ON benchmark_result TYPE none | number                  PERMISSIONS FULL;
DEFINE FIELD avg_temp_c     ON benchmark_result TYPE none | number                  PERMISSIONS FULL;
-- Ladder points for memory_latency: [{size_kb, latency_ns, read_gb_per_s}, ...].
DEFINE FIELD detail         ON benchmark_result TYPE none | array | object          PERMISSIONS FULL;
DEFINE FIELD hostname       ON benchmark_result TYPE none | string                  PERMISSIONS FULL;
DEFINE FIELD machine_id     ON benchmark_result TYPE none | string                  PERMISSIONS FULL;
DEFINE FIELD captured_at    ON benchmark_result TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;
DEFINE FIELD tags           ON benchmark_result TYPE array<string> DEFAULT []       PERMISSIONS FULL;
DEFINE FIELD tags.*         ON benchmark_result TYPE string                         PERMISSIONS FULL;
DEFINE FIELD notes          ON benchmark_result TYPE none | string                  PERMISSIONS FULL;
