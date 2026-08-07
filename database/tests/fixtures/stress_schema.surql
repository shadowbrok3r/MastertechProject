-- Minimal schema for stress persistence query tests (in-memory SurrealDB).

DEFINE TABLE computer TYPE ANY SCHEMALESS;

DEFINE TABLE hardware_component TYPE ANY SCHEMALESS;
DEFINE FIELD kind ON hardware_component TYPE string;
DEFINE FIELD vendor ON hardware_component TYPE string;
DEFINE FIELD model ON hardware_component TYPE string;
DEFINE FIELD display_name ON hardware_component TYPE string;
DEFINE FIELD embedding ON hardware_component TYPE array<float>;

DEFINE TABLE stress_test_run TYPE ANY SCHEMALESS;
DEFINE FIELD computer ON stress_test_run TYPE record<computer>;
DEFINE FIELD target_kind ON stress_test_run TYPE string;
DEFINE FIELD tool ON stress_test_run TYPE object;
DEFINE FIELD tool_label ON stress_test_run TYPE string;
DEFINE FIELD failure_mode ON stress_test_run TYPE object;
DEFINE FIELD failure_kind ON stress_test_run TYPE string;
DEFINE FIELD result ON stress_test_run TYPE string;
DEFINE FIELD summary ON stress_test_run TYPE object;
DEFINE FIELD embedding ON stress_test_run TYPE none | array<float>;

DEFINE TABLE stress_test_metric TYPE ANY SCHEMALESS;
DEFINE FIELD run_ref ON stress_test_metric TYPE record<stress_test_run>;
DEFINE FIELD captured_at ON stress_test_metric TYPE datetime;
DEFINE INDEX idx_metric_run_time ON stress_test_metric FIELDS run_ref, captured_at;

DEFINE TABLE benchmark_result TYPE ANY SCHEMALESS;
