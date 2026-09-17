//! Read-only checks about this machine.
//!
//! Every check here runs to a value and returns it. The old shape spawned a
//! thread that only logged, so the queue had to guess from the log when the work
//! had finished; these report completion instead.

use std::time::Instant;

use displays::scripts::catalog::ScriptDef;
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

#[cfg(target_os = "windows")]
use crate::terminal_mode::tabs::scripts::script_categories::check_windows_activation;
#[cfg(target_os = "windows")]
use crate::utilities::scripts::{AntiVirusProduct, InstalledProgram, check_power_options};

/// Ids this executor claims. Everything else stays on the legacy path until it
/// is ported, which is what lets the two coexist.
const HANDLED: &[&str] = &[
    "windows-version",
    "is-windows-activated",
    "is-supereasybackup-installed",
    "is-webroot-installed",
    "is-superantispyware-installed",
    "is-hibernation-sleep-enabled",
];

pub struct InformationalExecutor;

impl ScriptExecutor for InformationalExecutor {
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

fn run(def: &ScriptDef, ctx: &ScriptContext) -> ScriptResult {
    let category = def.category();
    let name = def.name.as_str();

    match def.id.as_str() {
        "windows-version" => {
            let version = sysinfo::System::long_os_version().unwrap_or_default();
            ctx.log_info(category, name, format!("Windows Version: {version}"));
            ScriptResult::Success(format!("Windows Version: {version}"))
        }
        "is-windows-activated" => check_activation(ctx, def),
        "is-supereasybackup-installed" => check_installed(ctx, def, "supereasybackup"),
        "is-webroot-installed" => check_installed(ctx, def, "webroot"),
        "is-superantispyware-installed" => check_installed(ctx, def, "superantispyware"),
        "is-hibernation-sleep-enabled" => check_power(ctx, def),
        other => ScriptResult::Error(format!("informational executor does not run '{other}'")),
    }
}

#[cfg(target_os = "windows")]
fn check_activation(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    match check_windows_activation() {
        Ok(status) if status.license_status == 1 => {
            ctx.log_success(category, name, "Windows is activated");
            ScriptResult::Success("Windows is activated".into())
        }
        Ok(_) => {
            ctx.log_warning(category, name, "Windows is NOT activated");
            ScriptResult::Warning("Windows is NOT activated".into())
        }
        Err(e) => {
            let msg = format!("Check failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn check_installed(ctx: &ScriptContext, def: &ScriptDef, search_term: &str) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    let search = search_term.to_lowercase();
    ctx.log_info(category.clone(), name, format!("Searching for {search}..."));

    if let Ok(programs) = InstalledProgram::get_installed_programs() {
        for program in &programs {
            let display_name = program
                .display_name
                .clone()
                .unwrap_or_default()
                .to_lowercase();
            let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
            if display_name.contains(&search) || publisher.contains(&search) {
                let found = program.display_name.clone().unwrap_or_default();
                let version = program.display_version.clone().unwrap_or_default();
                ctx.log_success(category.clone(), name, format!("{search} Found!"));
                ctx.log_info(category.clone(), name, format!("  Display Name: {found}"));
                ctx.log_info(category, name, format!("  Version: {version}"));
                return ScriptResult::Success(format!("{found} {version}"));
            }
        }
    }

    // An AV product can be present without an uninstall entry.
    if let Ok(av_products) = AntiVirusProduct::query_installed() {
        for product in &av_products {
            if product.display_name.to_lowercase().contains(&search) {
                let msg = format!("{search} Found (AV): {}", product.display_name);
                ctx.log_success(category, name, msg.clone());
                return ScriptResult::Success(msg);
            }
        }
    }

    let msg = format!("{search} not installed");
    ctx.log_warning(category, name, msg.clone());
    ScriptResult::Warning(msg)
}

#[cfg(target_os = "windows")]
fn check_power(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    match check_power_options() {
        Ok(_) => {
            ctx.log_success(category, name, "Power settings checked");
            ScriptResult::Success("Power settings checked".into())
        }
        Err(e) => {
            let msg = format!("Power settings check failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn check_activation(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn check_installed(ctx: &ScriptContext, def: &ScriptDef, _search_term: &str) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn check_power(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn unsupported(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

#[cfg(test)]
mod informational_executor_tests {
    use super::*;
    use displays::scripts::catalog::{CATALOG, Surface};

    #[test]
    fn every_claimed_id_is_a_real_informational_script() {
        for id in HANDLED {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(
                def.category(),
                displays::scripts::ScriptCategory::Informational,
                "{id} is not an informational script"
            );
            assert!(
                def.offered_on(Surface::Egui),
                "{id} is claimed but never offered in the tab"
            );
        }
    }

    #[test]
    fn the_executor_claims_only_what_it_runs() {
        let executor = InformationalExecutor;
        for id in HANDLED {
            assert!(executor.handles(&ScriptId::new(*id)));
        }
        assert!(!executor.handles(&ScriptId::new("activate-cps")));
        assert!(!executor.handles(&ScriptId::new("stress-cpu")));
        // Still on the legacy path until ported.
        assert!(!executor.handles(&ScriptId::new("run-prechecks")));
    }
}
