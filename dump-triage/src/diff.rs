//! Cross-dump comparison of typed triage results.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::KernelDumpTriage;

/// One scalar field's before/after across two dumps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "facet", derive(facet::Facet))]
pub struct FieldChange {
    pub before: Option<String>,
    pub after: Option<String>,
    pub changed: bool,
}

/// Structured difference between a baseline dump and another dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "facet", derive(facet::Facet))]
pub struct TriageDiff {
    pub bugcheck_code: FieldChange,
    pub blamed_module: FieldChange,
    pub rip_module: FieldChange,
    pub drivers_added: Vec<String>,
    pub drivers_removed: Vec<String>,
    pub drivers_rebased: Vec<String>,
    pub common_driver_count: u32,
}

fn field_change(before: Option<String>, after: Option<String>) -> FieldChange {
    let changed = before != after;
    FieldChange { before, after, changed }
}

/// Map of lowercased driver name to load base.
fn driver_bases(t: &KernelDumpTriage) -> HashMap<String, u64> {
    t.drivers
        .iter()
        .map(|d| (d.name.to_ascii_lowercase(), d.base))
        .collect()
}

/// Compare `other` against `baseline`.
pub fn diff_triages(baseline: &KernelDumpTriage, other: &KernelDumpTriage) -> TriageDiff {
    let base_map = driver_bases(baseline);
    let other_map = driver_bases(other);

    let mut drivers_added: Vec<String> = other_map
        .keys()
        .filter(|k| !base_map.contains_key(*k))
        .cloned()
        .collect();
    let mut drivers_removed: Vec<String> = base_map
        .keys()
        .filter(|k| !other_map.contains_key(*k))
        .cloned()
        .collect();
    let mut drivers_rebased: Vec<String> = base_map
        .iter()
        .filter(|(k, b)| other_map.get(*k).is_some_and(|ob| ob != *b))
        .map(|(k, _)| k.clone())
        .collect();
    let common_driver_count = base_map.keys().filter(|k| other_map.contains_key(*k)).count() as u32;

    drivers_added.sort();
    drivers_removed.sort();
    drivers_rebased.sort();

    TriageDiff {
        bugcheck_code: field_change(
            Some(baseline.bugcheck_code.clone()),
            Some(other.bugcheck_code.clone()),
        ),
        blamed_module: field_change(baseline.blamed_module.clone(), other.blamed_module.clone()),
        rip_module: field_change(baseline.rip_module.clone(), other.rip_module.clone()),
        drivers_added,
        drivers_removed,
        drivers_rebased,
        common_driver_count,
    }
}

/// Diff every dump against the newest by `system_time_unix` (baseline = index of
/// max time, ties → first). Output is index-aligned with `dumps`; the baseline's
/// own entry is a self-diff with no changes. Empty for fewer than two dumps.
pub fn baseline_diffs(dumps: &[KernelDumpTriage]) -> Vec<TriageDiff> {
    if dumps.len() < 2 {
        return Vec::new();
    }
    let mut baseline_idx = 0;
    let mut best = dumps[0].system_time_unix.unwrap_or(i64::MIN);
    for (i, d) in dumps.iter().enumerate().skip(1) {
        let t = d.system_time_unix.unwrap_or(i64::MIN);
        if t > best {
            best = t;
            baseline_idx = i;
        }
    }
    dumps
        .iter()
        .map(|d| diff_triages(&dumps[baseline_idx], d))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DriverEntry;

    fn driver(name: &str, base: u64) -> DriverEntry {
        DriverEntry {
            name: name.to_string(),
            path: format!("\\SystemRoot\\{name}"),
            base,
            size: 0x10000,
            timestamp: None,
        }
    }

    fn triage(bugcheck: &str, time: Option<i64>, drivers: Vec<DriverEntry>) -> KernelDumpTriage {
        KernelDumpTriage {
            dump_type: 4,
            dump_type_name: "triage_minidump".to_string(),
            bugcheck_code: bugcheck.to_string(),
            bugcheck_name: "TEST".to_string(),
            bugcheck_parameters: Vec::new(),
            parameter_notes: Vec::new(),
            rip: None,
            rsp: None,
            exception_code: None,
            number_processors: 8,
            registers: Vec::new(),
            system_time_unix: time,
            uptime_secs: None,
            comment: None,
            drivers,
            rip_module: None,
            rip_in_kernel_image: false,
            blamed_module: None,
            scanned_stack: Vec::new(),
            rip_region: None,
            rsp_region: None,
        }
    }

    #[test]
    fn diff_reports_driver_set_changes() {
        let base = triage(
            "0x133",
            Some(200),
            vec![driver("ntoskrnl.exe", 0x1000), driver("rtwlane.sys", 0x2000)],
        );
        let other = triage(
            "0xd1",
            Some(100),
            vec![driver("ntoskrnl.exe", 0x9000), driver("nvlddmkm.sys", 0x3000)],
        );
        let d = diff_triages(&base, &other);
        assert_eq!(d.drivers_added, vec!["nvlddmkm.sys".to_string()]);
        assert_eq!(d.drivers_removed, vec!["rtwlane.sys".to_string()]);
        assert_eq!(d.drivers_rebased, vec!["ntoskrnl.exe".to_string()]);
        assert_eq!(d.common_driver_count, 1);
        assert!(d.bugcheck_code.changed);
        assert_eq!(d.bugcheck_code.before.as_deref(), Some("0x133"));
        assert_eq!(d.bugcheck_code.after.as_deref(), Some("0xd1"));
    }

    #[test]
    fn baseline_is_newest_by_time_ties_first() {
        let a = triage("0x1", Some(50), vec![driver("a.sys", 0x1000)]);
        let b = triage("0x2", Some(300), vec![driver("a.sys", 0x1000), driver("b.sys", 0x2000)]);
        let c = triage("0x3", Some(300), vec![driver("a.sys", 0x1000)]);
        let diffs = baseline_diffs(&[a, b, c]);
        assert_eq!(diffs.len(), 3);
        // Baseline is index 1 (first of the tied max time 300); its self-diff is empty.
        assert!(!diffs[1].bugcheck_code.changed);
        assert!(diffs[1].drivers_added.is_empty());
        // Index 0 misses b.sys relative to the baseline.
        assert_eq!(diffs[0].drivers_removed, vec!["b.sys".to_string()]);
        assert!(diffs[0].bugcheck_code.changed);
    }

    #[test]
    fn baseline_diffs_empty_below_two() {
        assert!(baseline_diffs(&[]).is_empty());
        assert!(baseline_diffs(&[triage("0x1", Some(1), vec![])]).is_empty());
    }
}
