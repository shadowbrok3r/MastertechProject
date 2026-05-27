//! SurrealQL strings for stress-test persistence. Single source of truth for
//! Rust writes and integration / `surreal validate` tests.

pub const HW_COMPONENT_UPSERT: &str = "UPSERT $id MERGE { \
        kind: $kind, vendor: $vendor, model: $model, \
        sku: $sku, display_name: $display, specs: $specs, \
        embedding: fn::embed_text($embed_src), \
        first_seen: time::now(), last_seen: time::now(), \
        occurrence_count: (occurrence_count ?? 0) + 1 \
    } RETURN id";

pub const STRESS_RUN_CREATE: &str =
    "CREATE $id CONTENT ($content + { embedding: fn::embed_text($embed_src) })";

pub const RECORD_EXISTS: &str = "RETURN record::exists($id)";
