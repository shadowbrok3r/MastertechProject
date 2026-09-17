//! Windows Update checks and installs.
//!
//! `install_windows_updates` blocks until the run ends and reports progress on a
//! channel, so a second thread forwards those events while the first waits for
//! the return value. There is no cancel hook: stopping the queue leaves this
//! running and starts nothing after it.

use std::time::Instant;

use displays::scripts::catalog::ScriptDef;
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

/// Ids this executor claims. The two differ only in whether they install.
const HANDLED: &[&str] = &["install-windows-updates", "check-updates"];

pub struct WindowsUpdateExecutor;

impl ScriptExecutor for WindowsUpdateExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        HANDLED.contains(&id.as_str())
    }

    fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle {
        let (tx, done) = crossbeam::channel::bounded(1);
        let def = def.clone();
        let ctx = ctx.clone();
        let started = Instant::now();

        std::thread::spawn(move || {
            let result = run(&def, &ctx);
            let _ = tx.send(ScriptOutcome::plain(
                def.id.clone(),
                run_token,
                result,
                started.elapsed(),
            ));
        });

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

#[cfg(target_os = "windows")]
fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    use crate::utilities::windows::windows_update::install_windows_updates;

    let (category, name) = (def.category(), def.name.as_str());
    let installing = def.id.as_str() == "install-windows-updates";

    ctx.log_info(
        category,
        name,
        if installing {
            "Starting Windows Updates..."
        } else {
            "Checking for Windows Updates..."
        },
    );

    let (event_tx, event_rx) = crossbeam::channel::unbounded();
    let forwarder = std::thread::spawn({
        let ctx = ctx.clone();
        let id = def.id.as_str().to_string();
        move || {
            while let Ok(event) = event_rx.recv() {
                forward(&ctx, &id, event);
            }
        }
    });

    // The call owns the sender and drops it on return, which ends the forwarder.
    let result = install_windows_updates(event_tx, installing, installing);
    let _ = forwarder.join();

    match result {
        Ok(_) if installing => ScriptResult::Success("Windows updates installed".into()),
        Ok(_) => ScriptResult::Success("Update check complete".into()),
        Err(e) => {
            // The original path discarded this, so a failed run looked like a
            // successful one that logged nothing.
            let msg = format!("Windows Updates failed: {e}");
            ctx.log_error(def.category(), def.name.as_str(), msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

/// Reproduces the tab's own pump: these lines are logged under "Windows
/// Updates" rather than the script's name.
#[cfg(target_os = "windows")]
fn forward(
    ctx: &ScriptContext,
    id: &str,
    event: crate::utilities::windows::windows_update::WindowsUpdateEvent,
) {
    use crate::utilities::windows::windows_update::WindowsUpdateEvent;
    use displays::scripts::ScriptCategory;

    let system = ScriptCategory::Custom("System".to_string());
    match event {
        WindowsUpdateEvent::UpdateLogs(log) => ctx.log_info(system, "Windows Updates", log),
        WindowsUpdateEvent::ReturnedUpdates(updates) => ctx.log_info(
            system,
            "Windows Updates",
            format!("Found {} updates", updates.updates.len()),
        ),
        WindowsUpdateEvent::DownloadPercentage(pct)
        | WindowsUpdateEvent::InstallPercentage(pct) => {
            ctx.report_progress(id, pct.max(0) as u64, 100);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

#[cfg(test)]
mod windows_update_executor_tests {
    use super::*;
    use displays::scripts::catalog::{CATALOG, Surface};

    #[test]
    fn both_claimed_ids_are_real_and_offered() {
        for id in HANDLED {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert!(
                def.offered_on(Surface::Egui),
                "{id} is claimed but never offered in the tab"
            );
        }
    }

    /// The install needs the long budget; a check that inherited it would sit
    /// for an hour before the queue gave up on it.
    #[test]
    fn the_install_and_the_check_have_different_budgets() {
        let install = CATALOG.timeout_secs("Install Windows Updates");
        let check = CATALOG.timeout_secs("Check Updates");
        assert!(install > check, "install={install:?} check={check:?}");
    }

    #[test]
    fn the_executor_claims_only_what_it_runs() {
        let executor = WindowsUpdateExecutor;
        for id in HANDLED {
            assert!(executor.handles(&ScriptId::new(*id)));
        }
        assert!(!executor.handles(&ScriptId::new("run-prechecks")));
        assert!(!executor.handles(&ScriptId::new("windows-version")));
    }
}
