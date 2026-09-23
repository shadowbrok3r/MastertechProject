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
    use crate::utilities::windows::net_adapter::ensure_internet_connected;
    use crate::utilities::windows::windows_update::install_windows_updates;

    let (category, name) = (def.category(), def.name.as_str());
    let installing = def.id.as_str() == "install-windows-updates";

    let checking = if installing {
        "Checking internet before Windows Updates..."
    } else {
        "Checking internet before Windows Update search..."
    };
    ctx.log_info(category.clone(), name, checking);
    match super::env::block_on(ensure_internet_connected()) {
        Some(Ok(())) => ctx.log_info(category.clone(), name, "Internet confirmed"),
        Some(Err(e)) => {
            let msg = format!("No internet: {e}");
            ctx.log_error(category, name, msg.clone());
            return ScriptResult::Error(msg);
        }
        None => {
            let msg = "No async runtime is registered for script execution";
            ctx.log_error(category, name, msg);
            return ScriptResult::Error(msg.into());
        }
    }

    let starting = if installing {
        "Starting Windows Updates (search + install)..."
    } else {
        "Searching for available Windows updates (no install)..."
    };
    ctx.log_info(category.clone(), name, starting);

    let (event_tx, event_rx) = crossbeam::channel::unbounded();
    let forwarder = std::thread::spawn({
        let ctx = ctx.clone();
        let def = def.clone();
        move || {
            let mut milestones = Milestones::default();
            while let Ok(event) = event_rx.recv() {
                forward(&ctx, &def, installing, &mut milestones, event);
            }
        }
    });

    // The call owns the sender and drops it on return, which ends the forwarder.
    let result = install_windows_updates(event_tx, installing, installing);
    let _ = forwarder.join();

    match result {
        Ok(_) => {
            let msg = if installing {
                "Windows Updates completed successfully"
            } else {
                "Windows update check finished"
            };
            ctx.log_success(category, name, msg);
            ScriptResult::Success(msg.into())
        }
        Err(e) => {
            let msg = if installing {
                format!("Windows Updates error: {e:?}")
            } else {
                format!("Windows update check error: {e:?}")
            };
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

/// Last logged quarter of each percentage, so progress reaches the log four times, not a hundred.
#[cfg(target_os = "windows")]
#[derive(Default)]
struct Milestones {
    download: Option<i32>,
    install: Option<i32>,
}

#[cfg(target_os = "windows")]
fn milestone(last: &mut Option<i32>, pct: i32) -> Option<i32> {
    let quarter = pct.clamp(0, 100) / 25 * 25;
    (*last != Some(quarter)).then(|| {
        *last = Some(quarter);
        quarter
    })
}

/// Update log lines keep the tab's "Windows Updates" label; everything else is the script's own.
#[cfg(target_os = "windows")]
fn forward(
    ctx: &ScriptContext,
    def: &ScriptDef,
    installing: bool,
    milestones: &mut Milestones,
    event: crate::utilities::windows::windows_update::WindowsUpdateEvent,
) {
    use crate::utilities::windows::windows_update::WindowsUpdateEvent;
    use displays::scripts::ScriptCategory;

    let (category, name) = (def.category(), def.name.as_str());
    match event {
        WindowsUpdateEvent::UpdateLogs(log) => ctx.log_info(
            ScriptCategory::Custom("System".to_string()),
            "Windows Updates",
            log,
        ),
        WindowsUpdateEvent::ReturnedUpdates(updates) => {
            if installing {
                ctx.log_info(
                    category.clone(),
                    name,
                    format!("{} updates processed", updates.updates.len()),
                );
                for update in &updates.updates {
                    ctx.log_info(
                        category.clone(),
                        name,
                        format!("  {} (installed: {})", update.title, update.is_installed),
                    );
                }
            } else {
                let pending: Vec<_> = updates.updates.iter().filter(|u| !u.is_installed).collect();
                ctx.log_info(
                    category.clone(),
                    name,
                    format!(
                        "{} updates returned ({} pending, {} already installed)",
                        updates.updates.len(),
                        pending.len(),
                        updates.updates.len() - pending.len()
                    ),
                );
                for update in pending {
                    ctx.log_info(
                        category.clone(),
                        name,
                        format!("  [pending] {}", update.title),
                    );
                }
            }
        }
        WindowsUpdateEvent::DownloadPercentage(pct) => {
            ctx.report_progress(def.id.as_str(), pct.max(0) as u64, 100);
            if let Some(quarter) = milestone(&mut milestones.download, pct) {
                ctx.log_info(category, name, format!("Download: {quarter}%"));
            }
        }
        WindowsUpdateEvent::InstallPercentage(pct) => {
            ctx.report_progress(def.id.as_str(), pct.max(0) as u64, 100);
            if let Some(quarter) = milestone(&mut milestones.install, pct) {
                ctx.log_info(category, name, format!("Install: {quarter}%"));
            }
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

    #[cfg(target_os = "windows")]
    #[test]
    fn progress_is_logged_once_per_quarter() {
        let mut last = None;
        let logged: Vec<i32> = [0, 3, 24, 25, 26, 50, 99, 100, 100]
            .into_iter()
            .filter_map(|pct| milestone(&mut last, pct))
            .collect();
        assert_eq!(logged, [0, 25, 50, 75, 100]);
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
