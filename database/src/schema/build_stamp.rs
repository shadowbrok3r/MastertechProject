//! Process build stamp that stress rows created without a `tool_version` receive.

use std::sync::OnceLock;

static PROCESS_BUILD: OnceLock<String> = OnceLock::new();

/// Stamp used until a build is registered: this database crate's version and build hash.
pub const UNREGISTERED_BUILD: &str = concat!(
    "unregistered; database ",
    env!("CARGO_PKG_VERSION"),
    "+",
    env!("BUILD_HASH")
);

/// Registers this process's build stamp; the first non-blank call wins.
pub fn register_process_build(stamp: &str) {
    let stamp = stamp.trim();
    if !stamp.is_empty() {
        let _ = PROCESS_BUILD.set(stamp.to_owned());
    }
}

/// The registered build stamp, or [`UNREGISTERED_BUILD`].
pub fn process_build() -> &'static str {
    PROCESS_BUILD
        .get()
        .map_or(UNREGISTERED_BUILD, String::as_str)
}

/// Fills a missing or blank `tool_version` with [`process_build`].
pub(crate) fn fill_tool_version(tool_version: &mut Option<String>) {
    fill_with(tool_version, process_build());
}

fn fill_with(tool_version: &mut Option<String>, build: &str) {
    if tool_version.as_deref().is_none_or(|v| v.trim().is_empty()) {
        *tool_version = Some(build.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_blank_version_takes_the_build() {
        let mut missing = None;
        fill_with(&mut missing, "MasterTech 0.0.0+0000000.000000");
        assert_eq!(missing.as_deref(), Some("MasterTech 0.0.0+0000000.000000"));

        let mut blank = Some("  ".to_string());
        fill_with(&mut blank, "MasterTech 0.0.0+0000000.000000");
        assert_eq!(blank.as_deref(), Some("MasterTech 0.0.0+0000000.000000"));
    }

    #[test]
    fn an_explicit_version_is_kept() {
        let mut recorded = Some("backfill; recorded by admin-agent 0.0.0+0000000".to_string());
        fill_with(&mut recorded, "MasterTech 0.0.0+0000000.000000");
        assert_eq!(
            recorded.as_deref(),
            Some("backfill; recorded by admin-agent 0.0.0+0000000")
        );
    }

    #[test]
    fn stress_rows_take_the_process_build() {
        use crate::schema::{
            BenchmarkKind, BenchmarkResult, EventKind, RecordId, StressTestEvent, StressTestRun,
            TargetKind, TestTool,
        };
        let computer = RecordId::new("computer", "DESKTOP-TEST:0000000");
        let run = StressTestRun::new_for(
            computer.clone(),
            TestTool::StressKit {
                stressor: "cpu".into(),
            },
            TargetKind::Cpu,
        )
        .stamped();
        assert_eq!(run.tool_version.as_deref(), Some(process_build()));

        let event =
            StressTestEvent::new(run.id.clone(), EventKind::Custom, "stress-runner").stamped();
        assert_eq!(event.tool_version, run.tool_version);

        let bench =
            BenchmarkResult::new(computer, BenchmarkKind::MemoryLatency, 80.0, "ns").stamped();
        assert_eq!(bench.tool_version, run.tool_version);
    }

    #[test]
    fn the_unregistered_stamp_names_the_database_build() {
        let rest = UNREGISTERED_BUILD
            .strip_prefix("unregistered; database ")
            .expect("unregistered prefix");
        let (version, hash) = rest.split_once('+').expect("version+hash");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert!(!hash.is_empty());
    }
}
