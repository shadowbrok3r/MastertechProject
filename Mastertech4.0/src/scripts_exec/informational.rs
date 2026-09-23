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
use crate::utilities::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask};

/// Ids this executor claims. Everything else stays on the legacy path until it
/// is ported, which is what lets the two coexist.
const HANDLED: &[&str] = &[
    "windows-version",
    "is-windows-activated",
    "is-supereasybackup-installed",
    "is-webroot-installed",
    "is-superantispyware-installed",
    "is-hibernation-sleep-enabled",
    "are-there-scheduled-tasks-for-it",
    "any-recent-blue-screens",
    "when-was-the-last-service-date",
    "network-status",
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
        "are-there-scheduled-tasks-for-it" => check_sas_scheduled_tasks(ctx, def),
        "any-recent-blue-screens" => bsod_scan(ctx, def),
        "network-status" => network_status(ctx, def),
        "when-was-the-last-service-date" => {
            super::not_implemented(ctx, def, "Service date check not yet implemented")
        }
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
/// Reports the active plan and each sleep, hibernate and display timeout on AC and DC.
fn check_power(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    match super::powershell::run(POWER_REPORT) {
        Ok(output) => {
            let lines: Vec<String> = output
                .lines
                .into_iter()
                .filter(|line| !line.trim().is_empty())
                .collect();
            let enabled = lines
                .iter()
                .any(|line| line.contains("ENABLED on at least one setting"));
            let summary = lines.last().cloned().unwrap_or_default();
            for line in lines {
                ctx.log_info(category.clone(), name, line);
            }
            if enabled {
                ScriptResult::Warning(summary)
            } else {
                ScriptResult::Success(summary)
            }
        }
        Err(failure) => {
            if let Some(code) = failure.exit_code {
                ctx.log_info(category.clone(), name, format!("Exit code: {code:?}"));
            }
            let msg = format!("Error querying power options: {}", failure.message);
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn check_sas_scheduled_tasks(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Checking scheduled tasks...");

    match ScheduledTask::list_tasks() {
        Ok(tasks) => {
            let sas_tasks: Vec<String> = tasks
                .iter()
                .filter_map(|t| t.task_name.clone())
                .filter(|n| n.contains("SUPERAntiSpyware"))
                .collect();

            if sas_tasks.is_empty() {
                let msg = "No SAS scheduled tasks found";
                ctx.log_warning(category, name, msg);
                return ScriptResult::Warning(msg.into());
            }

            let msg = format!("Found {} SAS scheduled task(s)", sas_tasks.len());
            ctx.log_success(category.clone(), name, msg.clone());
            for task in &sas_tasks {
                ctx.log_info(category.clone(), name, format!("  • {task}"));
            }
            ScriptResult::Success(msg)
        }
        Err(e) => {
            let msg = format!("Failed to get tasks: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn bsod_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::bsod_scan::{self, BsodVerdict};

    let (category, name) = (def.category(), def.name.as_str());
    match bsod_scan::scan_blocking(bsod_scan::DEFAULT_DAYS) {
        Ok(scan) => {
            let verdict = scan.verdict();
            let mut lines = scan.report_lines();
            // The last line is the verdict, and it carries the entry severity.
            let summary = lines.pop().unwrap_or_default();
            for line in lines {
                ctx.log_info(category.clone(), name, line);
            }
            match verdict {
                BsodVerdict::Error => {
                    ctx.log_error(category, name, summary.clone());
                    ScriptResult::Error(summary)
                }
                BsodVerdict::Warning => {
                    ctx.log_warning(category, name, summary.clone());
                    ScriptResult::Warning(summary)
                }
                BsodVerdict::Clean => {
                    ctx.log_success(category, name, summary.clone());
                    ScriptResult::Success(summary)
                }
            }
        }
        Err(e) => {
            let msg = format!("BSOD check failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

/// Wi-Fi networks in range, WLAN connection state and the adapter list.
#[cfg(target_os = "windows")]
fn network_status(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::windows::net_adapter::{
        check_network_adapters, get_wlan_status, scan_wifi_networks,
    };

    let (category, name) = (def.category(), def.name.as_str());
    let mut failures = Vec::new();

    match scan_wifi_networks() {
        Ok(networks) => ctx.log_info(
            category.clone(),
            name,
            format!("wifi networks visible: {}", networks.len()),
        ),
        Err(e) => {
            let msg = format!("wifi scan error: {e}");
            ctx.log_error(category.clone(), name, msg.clone());
            failures.push(msg);
        }
    }
    // A WLAN status error is logged, not counted as a failure.
    match get_wlan_status() {
        Ok(()) => ctx.log_info(category.clone(), name, "wlan status: OK"),
        Err(e) => ctx.log_info(category.clone(), name, format!("wlan status: {e:?}")),
    }
    match check_network_adapters() {
        Ok(adapters) => ctx.log_info(
            category.clone(),
            name,
            format!("network adapters: {adapters:?}"),
        ),
        Err(e) => {
            let msg = format!("adapter check error: {e}");
            ctx.log_error(category.clone(), name, msg.clone());
            failures.push(msg);
        }
    }

    if failures.is_empty() {
        ctx.log_success(category, name, "Network checks complete");
        ScriptResult::Success("Network checks complete".into())
    } else {
        ScriptResult::Error(failures.join("; "))
    }
}

#[cfg(not(target_os = "windows"))]
fn network_status(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
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
fn check_sas_scheduled_tasks(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn bsod_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn unsupported(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

#[cfg(target_os = "windows")]
const POWER_REPORT: &str = r#"
Write-Output ((powercfg /getactivescheme) -join '')
$settings = @(
    @{ Name='Sleep after';      Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='29f6c1db-86da-48c5-9fdb-f2b67b1f44da'; Units='Seconds' },
    @{ Name='Hibernate after';  Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='9d7815a6-7ee4-497e-8888-515a05f02364'; Units='Seconds' },
    @{ Name='Hybrid sleep';     Sub='238c9fa8-0aad-41ed-83f4-97be242c8f20'; Guid='94ac6d29-73ce-41a6-809f-6363ba21b47e'; Units='OnOff'   },
    @{ Name='Turn off display'; Sub='7516b95f-f776-4464-8c53-06167f40cc99'; Guid='3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e'; Units='Seconds' }
)
$anyEnabled = $false
foreach ($s in $settings) {
    $out = powercfg /query SCHEME_CURRENT $s.Sub $s.Guid 2>$null
    $acMatch = $out | Select-String 'Current AC Power Setting Index:\s*(0x[0-9a-fA-F]+)'
    $dcMatch = $out | Select-String 'Current DC Power Setting Index:\s*(0x[0-9a-fA-F]+)'
    $ac = if ($acMatch) { $acMatch.Matches[0].Groups[1].Value } else { '0x00000000' }
    $dc = if ($dcMatch) { $dcMatch.Matches[0].Groups[1].Value } else { '0x00000000' }
    if ($s.Units -eq 'Seconds') {
        $acVal = [uint32]$ac
        $dcVal = [uint32]$dc
        Write-Output ("{0}: AC={1}s  DC={2}s" -f $s.Name, $acVal, $dcVal)
        if ($acVal -gt 0 -or $dcVal -gt 0) { $anyEnabled = $true }
    } else {
        $acOn = if ($ac -ne '0x00000000') { 'On' } else { 'Off' }
        $dcOn = if ($dc -ne '0x00000000') { 'On' } else { 'Off' }
        Write-Output ("{0}: AC={1}  DC={2}" -f $s.Name, $acOn, $dcOn)
        if ($acOn -eq 'On' -or $dcOn -eq 'On') { $anyEnabled = $true }
    }
}
$states = (powercfg /availablesleepstates 2>&1) -join "`n"
if ($states -match 'Hibernate') { Write-Output 'Hibernation available: YES' } else { Write-Output 'Hibernation available: NO' }
if ($anyEnabled) { Write-Output 'Sleep/Hibernation: ENABLED on at least one setting' } else { Write-Output 'Sleep/Hibernation: all timeouts at 0 (disabled)' }
"#;

#[cfg(test)]
mod informational_executor_tests {
    use super::*;
    use displays::scripts::catalog::{CATALOG, Surface};

    /// Catalog-known but implemented nowhere, so it is claimed only to turn a
    /// legacy name into a declarative skip.
    const STUBS: &[&str] = &["when-was-the-last-service-date"];

    /// Run only as a step of a composite.
    const STEPS: &[&str] = &["network-status"];

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
                def.offered_on(Surface::Egui) || STUBS.contains(id) || STEPS.contains(id),
                "{id} is claimed but never offered in the tab"
            );
        }
    }

    #[test]
    fn the_stubs_are_offered_nowhere() {
        for id in STUBS {
            let def = CATALOG.get(&ScriptId::new(*id)).expect("catalog entry");
            assert!(!def.offered_on(Surface::Egui), "{id} is offered in the tab");
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
