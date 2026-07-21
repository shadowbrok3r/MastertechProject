//! Facet-derived JSON schema for the minidump_analyze triage result and the
//! cross-dump diff, generated from the same structs the tools return.

use dump_triage::{diff::TriageDiff, KernelDumpTriage};
use facet::Facet;
use mtech_plugin_sdk::schema::tool_schema_json;

/// JSON schema of the `KernelDumpTriage` result object.
pub fn kernel_triage_result_schema() -> String {
    tool_schema_json(Some(KernelDumpTriage::SHAPE))
}

/// JSON schema of the cross-dump `TriageDiff` object.
pub fn triage_diff_schema() -> String {
    tool_schema_json(Some(TriageDiff::SHAPE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_valid_object_json() {
        let triage: serde_json::Value =
            serde_json::from_str(&kernel_triage_result_schema()).unwrap();
        assert_eq!(triage["type"], "object");
        assert!(triage["properties"]["bugcheck_code"].is_object());
        assert_eq!(triage["properties"]["drivers"]["type"], "array");

        let diff: serde_json::Value = serde_json::from_str(&triage_diff_schema()).unwrap();
        assert_eq!(diff["type"], "object");
        assert_eq!(diff["properties"]["drivers_added"]["type"], "array");
        assert!(diff["properties"]["bugcheck_code"]["properties"]["changed"].is_object());
    }
}
