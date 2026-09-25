//! SurrealQL strings for stress-test persistence. Single source of truth for
//! Rust writes and integration / `surreal validate` tests.

pub const HW_COMPONENT_UPSERT: &str = "UPSERT $id MERGE { \
        kind: $kind, vendor: $vendor, model: $model, \
        sku: ($sku ?? sku), display_name: $display, specs: ($specs ?? specs), \
        embedding: $embedding ?? embedding ?? [], \
        first_seen: (first_seen ?? time::now()), last_seen: time::now(), \
        occurrence_count: (occurrence_count ?? 0) + 1 \
    } RETURN id";

pub const STRESS_RUN_CREATE: &str =
    "CREATE $id CONTENT ($content + { embedding: $embedding })";

pub const RECORD_EXISTS: &str = "RETURN record::exists($id)";

/// In-progress runs on every machine that started before `$floor`.
pub const ORPHAN_CANDIDATES_FLEET: &str = "SELECT id, started_at, duration_planned_secs \
    FROM stress_test_run WHERE result = 'in_progress' AND started_at < $floor";

/// In-progress runs on one computer.
pub const ORPHAN_CANDIDATES_MACHINE: &str = "SELECT id, started_at, duration_planned_secs \
    FROM stress_test_run WHERE result = 'in_progress' AND computer = $computer";

/// Newest metric timestamp, then newest event timestamp, of one run.
pub const RUN_LAST_SEEN: &str = "SELECT VALUE captured_at FROM stress_test_metric \
        WHERE run_ref = $id ORDER BY captured_at DESC LIMIT 1; \
    SELECT VALUE at FROM stress_test_event \
        WHERE run_ref = $id ORDER BY at DESC LIMIT 1;";

/// Closes a still in-progress run as aborted/crashed; returns its id only when it was closed.
pub const ORPHAN_CLOSE: &str = "UPDATE $id SET \
        result = 'aborted', \
        finish_reason = 'crashed', \
        ended_at = $ended_at, \
        duration_actual_secs = <float> duration::secs($ended_at - started_at), \
        notes = IF notes THEN string::concat(notes, ' ', $note) ELSE $note END \
    WHERE result = 'in_progress' RETURN VALUE id";
