//! Runs a remote script request through the shared executors.
//!
//! A remote run reports only PASS or FAIL, so each outcome is mapped to what the
//! Admin Console and the AI saw from the original remote code. Scripts not listed
//! here keep that original code in `terminal_mode::websockets`.

use std::time::Duration;

use displays::scripts::ScriptCategory;
use displays::scripts::catalog::{CATALOG, ScriptDef, Surface};
use displays::scripts::executor::{CancelToken, ScriptContext, ScriptResult};
use displays::scripts::{ScriptChannels, ScriptLogEntry};

/// Set to `1` to run every remote script through the original remote code.
pub const LEGACY_ENV: &str = "MASTERTECH_SCRIPTS_LEGACY_REMOTE";

/// Ids a remote request runs through the shared executors, beyond the junkware and stress families.
const PORTED: &[&str] = &[
    "windows-version",
    "is-windows-activated",
    "is-supereasybackup-installed",
    "is-webroot-installed",
    "is-superantispyware-installed",
    "any-recent-blue-screens",
    "are-there-scheduled-tasks-for-it",
    "disable-sleep-hibernation",
    "align-taskbar-to-left",
    "run-superantispyware-scan",
    "run-webroot-scan",
    "disable-bitlocker",
    "change-timezone-to-mountain",
    "activate-seb",
    "install-libreoffice",
    "activate-cps",
    "run-junkware-category",
    "is-hibernation-sleep-enabled",
    "check-updates",
    "install-windows-updates",
    "unpin-copilot",
    "disable-notifications",
    "disable-startup-apps",
    "change-superantispyware-settings",
    "run-prechecks",
    "activate-webroot",
    "activate-superanti",
];

/// A warning is a finding for these, which the remote code reported as FAIL.
const WARNING_FAILS: &[&str] = &["scan-for-browser-hijackers"];

pub struct RemoteRun {
    pub passed: bool,
    pub reboot_recommended: bool,
}

/// The catalog entry to run through the shared executors, or `None` to keep the remote code.
pub fn ported(name: &str) -> Option<&'static ScriptDef> {
    if std::env::var(LEGACY_ENV).is_ok_and(|v| v.trim() == "1") {
        return None;
    }
    let def = CATALOG
        .id_for_legacy_name(name)
        .and_then(|id| CATALOG.get(id))?;
    let listed = PORTED.contains(&def.id.as_str())
        || matches!(
            def.category(),
            ScriptCategory::JunkwareRemoval | ScriptCategory::StressTests
        );
    (listed && super::registry().find(&def.id).is_some()).then_some(def)
}

pub fn context(
    service_number: &str,
    customer_email: &str,
    diagnostic_session_id: &str,
) -> ScriptContext {
    let present = |s: &str| (!s.trim().is_empty()).then(|| s.to_owned());
    ScriptContext {
        service_number: present(service_number),
        customer_email: present(customer_email),
        diagnostic_session_id: present(diagnostic_session_id),
        surface: Some(Surface::Remote),
        channels: ScriptChannels::default(),
    }
}

/// Runs `def`, forwarding each log line through `log` until it reports.
///
/// Polls rather than blocks, because this runs on the client's session loop.
pub async fn run(
    def: &ScriptDef,
    ctx: ScriptContext,
    cancel: CancelToken,
    log: impl Fn(String) + Send + Sync,
) -> RemoteRun {
    // A remote stress run must link to a service order.
    if def.category() == ScriptCategory::StressTests && ctx.service_number.is_none() {
        log(format!(
            "{}: service_number is required so stress_test_run carries service_order / customer / computer linkage \u{2014} aborting.",
            def.name
        ));
        return RemoteRun {
            passed: false,
            reboot_recommended: false,
        };
    }

    let log_rx = ctx.channels.log_rx.clone();
    let handle = super::registry().spawn(def, &ctx, 0, cancel);
    drop(ctx);

    let forward = || {
        while let Ok(entry) = log_rx.try_recv() {
            log(line(&entry));
        }
    };

    loop {
        forward();
        match handle.done.try_recv() {
            Ok(outcome) => {
                forward();
                return RemoteRun {
                    passed: passed(def, &outcome.result),
                    reboot_recommended: outcome.reboot_recommended,
                };
            }
            Err(crossbeam::channel::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                forward();
                log(format!("{}: worker stopped without reporting", def.name));
                return RemoteRun {
                    passed: false,
                    reboot_recommended: false,
                };
            }
        }
    }
}

fn line(entry: &ScriptLogEntry) -> String {
    entry.message.clone()
}

/// PASS or FAIL for a remote run.
fn passed(def: &ScriptDef, result: &ScriptResult) -> bool {
    match result {
        ScriptResult::Success(_) => true,
        ScriptResult::Warning(_) => {
            !(WARNING_FAILS.contains(&def.id.as_str())
                || def.category() == ScriptCategory::StressTests)
        }
        ScriptResult::Error(_) | ScriptResult::Skipped(_) => false,
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    use displays::scripts::id::ScriptId;

    fn def(id: &str) -> &'static ScriptDef {
        CATALOG.get(&ScriptId::new(id)).expect("catalog entry")
    }

    #[test]
    fn every_ported_id_has_an_executor() {
        let registry = super::super::registry();
        for id in PORTED {
            let def = def(id);
            assert!(registry.find(&def.id).is_some(), "{id} has no executor");
        }
    }

    /// Data Transfer needs the picker on the machine itself.
    #[test]
    fn data_transfer_keeps_its_remote_refusal() {
        assert!(ported("Data Transfer").is_none());
    }

    #[test]
    fn stress_is_ported_but_benchmarks_are_not() {
        assert!(ported(&def("stress-cpu").name).is_some());
        assert!(ported("Benchmark Suite").is_none());
    }

    #[test]
    fn junkware_is_ported_by_family() {
        assert!(ported("Wave Browser").is_some());
        assert!(ported("Uninstall Microsoft 365").is_some());
    }

    /// The remote code reported a script that ran as PASS, even when its answer was negative.
    #[test]
    fn a_negative_answer_still_passes() {
        let warning = ScriptResult::Warning("webroot not installed".into());
        assert!(passed(def("is-webroot-installed"), &warning));
        assert!(passed(def("is-windows-activated"), &warning));
    }

    #[test]
    fn a_finding_fails() {
        let warning = ScriptResult::Warning("2 hijack finding(s)".into());
        assert!(!passed(def("scan-for-browser-hijackers"), &warning));
        assert!(passed(def("remove-browser-hijackers"), &warning));
    }

    #[test]
    fn an_aborted_stress_run_fails() {
        let warning = ScriptResult::Warning("aborted".into());
        assert!(!passed(def("stress-cpu"), &warning));
    }

    #[test]
    fn skips_and_errors_fail() {
        let def = def("activate-cps");
        assert!(!passed(
            def,
            &ScriptResult::Skipped("Service number required".into())
        ));
        assert!(!passed(
            def,
            &ScriptResult::Error("SAS install failed".into())
        ));
        assert!(passed(def, &ScriptResult::Success("licensed".into())));
    }
}
