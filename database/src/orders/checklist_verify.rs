//! Checklist auto-verification engine (ported from QCWizard
//! `ChecklistAutoVerifier`). Pure gating logic over a [`LiveProbe`] so it's
//! unit-testable; the WMI-backed probe lives in qc-app.

use super::checklist::{ChecklistState, ItemStatus};

/// Keys backed by live system state — re-verified at sign-off.
pub const LIVE_KEYS: &[&str] = &["os_activated", "drv_smart"];

/// SMART rollup for the drive-health item.
#[derive(Debug, Clone, Default)]
pub struct SmartSummary {
    /// False when the storage driver reports no SMART data (VMs, some controllers).
    pub queried: bool,
    pub all_healthy: bool,
    pub summary: String,
}

/// System-state evidence source. Read-only; implemented by the WMI probe on
/// Windows and a mock in tests.
pub trait LiveProbe {
    /// True when a Windows OEM (OA3) product key is present in firmware.
    fn oa3_key_present(&self) -> bool;
    fn smart(&self) -> SmartSummary;
}

fn is_unset(state: &ChecklistState, key: &str) -> bool {
    state.item(key).map(|i| i.status() == ItemStatus::Unset).unwrap_or(false)
}

/// Pre-check provable items. Only touches `Unset` items, never a tech's mark.
/// `cpu_max_c`/`gpu_max_c` come from the backing stress run (point-in-time, not
/// re-verified).
pub fn apply(state: &mut ChecklistState, probe: &dyn LiveProbe, cpu_max_c: Option<f64>, gpu_max_c: Option<f64>) {
    if is_unset(state, "os_activated") && probe.oa3_key_present() {
        state.apply_auto("os_activated", ItemStatus::Pass, "Windows OA3 firmware key present", "");
    }

    if is_unset(state, "drv_smart") {
        let s = probe.smart();
        if s.queried {
            let status = if s.all_healthy { ItemStatus::Pass } else { ItemStatus::Fail };
            state.apply_auto("drv_smart", status, "SMART (MSStorageDriver_FailurePredictStatus)", &s.summary);
        }
    }

    if is_unset(state, "st_load_temps") {
        if let (Some(cpu), Some(gpu)) = (cpu_max_c, gpu_max_c) {
            if cpu > 0.0 || gpu > 0.0 {
                let value = format!("CPU max {cpu:.0}°C / GPU max {gpu:.0}°C");
                if cpu > 0.0 && gpu > 0.0 && cpu < 95.0 && gpu < 90.0 {
                    state.apply_auto("st_load_temps", ItemStatus::Pass, "stress run", &value);
                } else {
                    state.set_value("st_load_temps", &value);
                }
            }
        }
    }
}

/// Re-verify auto-checked live items at sign-off. Items whose evidence no
/// longer holds are reset to Unset and returned so the caller can block submit.
pub fn reverify_at_signoff(state: &mut ChecklistState, probe: &dyn LiveProbe) -> Vec<String> {
    let mut stale = Vec::new();
    for &key in LIVE_KEYS {
        let Some(item) = state.item(key) else { continue };
        if !item.auto_verified() {
            continue;
        }
        let current = item.status();
        let (still_valid, evidence, value) = match key {
            "os_activated" => (
                current == ItemStatus::Pass && probe.oa3_key_present(),
                "Windows OA3 firmware key present".to_string(),
                String::new(),
            ),
            "drv_smart" => {
                let s = probe.smart();
                let expected = if s.all_healthy { ItemStatus::Pass } else { ItemStatus::Fail };
                (s.queried && current == expected, "SMART (MSStorageDriver_FailurePredictStatus)".to_string(), s.summary)
            }
            _ => (true, String::new(), String::new()),
        };

        if still_valid {
            state.apply_auto(key, current, &evidence, &value);
        } else {
            state.reset_for_recheck(key, "Changed since the auto-check — fix and re-verify before sign-off.");
            stale.push(key.to_string());
        }
    }
    stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::checklist::ChecklistKind;

    struct MockProbe {
        oa3: bool,
        smart: SmartSummary,
    }
    impl LiveProbe for MockProbe {
        fn oa3_key_present(&self) -> bool {
            self.oa3
        }
        fn smart(&self) -> SmartSummary {
            self.smart.clone()
        }
    }

    fn smart_ok() -> SmartSummary {
        SmartSummary { queried: true, all_healthy: true, summary: "SMART OK — 1 drive(s)".into() }
    }

    #[test]
    fn apply_only_touches_unset_items() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.set_status("os_activated", ItemStatus::Fail);
        c.set_note("os_activated", "no key");
        let probe = MockProbe { oa3: true, smart: smart_ok() };
        apply(&mut c, &probe, Some(70.0), Some(65.0));
        assert_eq!(c.item("os_activated").unwrap().status(), ItemStatus::Fail);
        assert_eq!(c.item("drv_smart").unwrap().status(), ItemStatus::Pass);
        assert!(c.item("drv_smart").unwrap().auto_verified());
        assert_eq!(c.item("st_load_temps").unwrap().status(), ItemStatus::Pass);
    }

    #[test]
    fn smart_failure_auto_fails() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        let probe = MockProbe {
            oa3: false,
            smart: SmartSummary { queried: true, all_healthy: false, summary: "SMART FAILURE PREDICTED: drive0".into() },
        };
        apply(&mut c, &probe, None, None);
        let drv = c.item("drv_smart").unwrap();
        assert_eq!(drv.status(), ItemStatus::Fail);
        assert!(drv.note.contains("FAILURE"));
    }

    #[test]
    fn hot_temps_capture_value_without_passing() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        let probe = MockProbe { oa3: false, smart: SmartSummary::default() };
        apply(&mut c, &probe, Some(99.0), Some(85.0));
        let t = c.item("st_load_temps").unwrap();
        assert_eq!(t.status(), ItemStatus::Unset);
        assert!(t.value.contains("99"));
    }

    #[test]
    fn unqueryable_smart_leaves_item_unset() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        let probe = MockProbe { oa3: false, smart: SmartSummary { queried: false, ..Default::default() } };
        apply(&mut c, &probe, None, None);
        assert_eq!(c.item("drv_smart").unwrap().status(), ItemStatus::Unset);
    }

    #[test]
    fn reverify_resets_drifted_activation() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        let live = MockProbe { oa3: true, smart: smart_ok() };
        apply(&mut c, &live, None, None);
        assert_eq!(c.item("os_activated").unwrap().status(), ItemStatus::Pass);

        let drifted = MockProbe { oa3: false, smart: smart_ok() };
        let stale = reverify_at_signoff(&mut c, &drifted);
        assert!(stale.contains(&"os_activated".to_string()));
        assert_eq!(c.item("os_activated").unwrap().status(), ItemStatus::Unset);
    }

    #[test]
    fn reverify_keeps_still_valid_items() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        let probe = MockProbe { oa3: true, smart: smart_ok() };
        apply(&mut c, &probe, None, None);
        let stale = reverify_at_signoff(&mut c, &probe);
        assert!(stale.is_empty());
        assert_eq!(c.item("os_activated").unwrap().status(), ItemStatus::Pass);
        assert_eq!(c.item("drv_smart").unwrap().status(), ItemStatus::Pass);
    }
}
