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
