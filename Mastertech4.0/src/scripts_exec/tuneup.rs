//! Tune-up changes to this machine.

use std::time::Instant;

use displays::scripts::catalog::ScriptDef;
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

use super::{not_implemented, powershell};

/// Ids this executor claims.
const HANDLED: &[&str] = &[
    "disable-sleep-hibernation",
    "run-superantispyware-scan",
    "run-webroot-scan",
    "disable-notifications",
    "disable-startup-apps",
    "unpin-copilot",
    "align-taskbar-to-left",
    "change-timezone-to-mountain",
    "disable-bitlocker",
    "disable-proxy-settings",
    "change-superantispyware-settings",
];

pub struct TuneupExecutor;

impl ScriptExecutor for TuneupExecutor {
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
            let (result, exit_code) = run(&def, &ctx);
            let mut outcome =
                ScriptOutcome::plain(def.id.clone(), run_token, result, started.elapsed());
            outcome.exit_code = exit_code;
            let _ = tx.send(outcome);
        });

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

fn run(def: &ScriptDef, ctx: &ScriptContext) -> (ScriptResult, Option<i32>) {
    match def.id.as_str() {
        "disable-sleep-hibernation" => (disable_sleep(ctx, def), None),
        "run-superantispyware-scan" => (sas_scan(ctx, def), None),
        "run-webroot-scan" => (webroot_scan(ctx, def), None),
        "disable-notifications" => (disable_notifications(ctx, def), None),
        "disable-startup-apps" => (disable_startup_apps(ctx, def), None),
        "unpin-copilot" => (unpin_copilot(ctx, def), None),
        "align-taskbar-to-left" => (align_taskbar(ctx, def), None),
        "change-timezone-to-mountain" => change_timezone(ctx, def),
        "disable-bitlocker" => disable_bitlocker(ctx, def),
        "disable-proxy-settings" => (
            not_implemented(ctx, def, "Proxy settings disable not yet implemented"),
            None,
        ),
        "change-superantispyware-settings" => (sas_settings(ctx, def), None),
        other => (
            ScriptResult::Error(format!("tuneup executor does not run '{other}'")),
            None,
        ),
    }
}

#[cfg(target_os = "windows")]
fn disable_sleep(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::terminal_mode::tabs::script_categories::disable_hibernation_and_sleep;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Disabling sleep and hibernation...");

    match disable_hibernation_and_sleep() {
        Ok(true) => {
            ctx.log_success(category, name, "Sleep/hibernation disabled");
            ScriptResult::Success("Sleep/hibernation disabled".into())
        }
        Ok(false) => {
            ctx.log_info(category, name, "Sleep/hibernation already disabled");
            ScriptResult::Success("Sleep/hibernation already disabled".into())
        }
        Err(e) => {
            let msg = format!("Failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn sas_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(
        category.clone(),
        name,
        "Starting SuperAntiSpyware quick scan...",
    );

    match crate::utilities::scripts::antivirus::run_sas_quick_scan() {
        Ok(messages) => {
            for message in messages {
                ctx.log_info(category.clone(), name, message);
            }
            ctx.log_success(category, name, "SAS quick scan started");
            ScriptResult::Success("SAS quick scan started".into())
        }
        Err(e) => {
            let msg = format!("SAS scan failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn webroot_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());

    match crate::utilities::scripts::antivirus::start_webroot_scan() {
        Ok(message) => {
            ctx.log_success(category, name, message.clone());
            ScriptResult::Success(message)
        }
        Err(e) => {
            let msg = format!("Webroot scan failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn disable_notifications(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::windows::registry as reg;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Disabling Windows notifications...");

    // The helpers differ in their Ok type, so each step is narrowed here.
    type RegStep = fn() -> Result<(), String>;
    let steps: &[(&str, RegStep)] = &[
        ("Push Notifications", || {
            reg::disable_notifications().map(|_| ()).map_err(err)
        }),
        ("Lockscreen Notifications", || {
            reg::disable_lockscreen_notifications()
                .map(|_| ())
                .map_err(err)
        }),
        ("Content Delivery", || {
            reg::disable_content_delivery_allowed()
                .map(|_| ())
                .map_err(err)
        }),
        ("Silent App Installs", || {
            reg::disable_silent_installed_apps_enabled()
                .map(|_| ())
                .map_err(err)
        }),
        ("Subscribed Content", || {
            reg::disable_subscribed_content_enabled()
                .map(|_| ())
                .map_err(err)
        }),
        ("System Pane Suggestions", || {
            reg::disable_system_pane_suggestions_enabled()
                .map(|_| ())
                .map_err(err)
        }),
        ("Account Notifications", || {
            reg::disable_account_notifications()
                .map(|_| ())
                .map_err(err)
        }),
        ("More Pins Layout", || {
            reg::enable_more_pins_layout().map(|_| ()).map_err(err)
        }),
        ("Start Account Notifications", || {
            reg::disable_start_account_notifications()
                .map(|_| ())
                .map_err(err)
        }),
        ("Recent Items Tracking", || {
            reg::disable_recent_items_tracking()
                .map(|_| ())
                .map_err(err)
        }),
        ("Remove Chat from Taskbar", || {
            reg::remove_chat_from_taskbar().map(|_| ()).map_err(err)
        }),
    ];

    let mut success_count = 0;
    let mut error_count = 0;
    for (label, step) in steps {
        match step() {
            Ok(_) => {
                success_count += 1;
                ctx.log_info(category.clone(), name, format!("✓ {label}"));
            }
            Err(e) => {
                error_count += 1;
                ctx.log_warning(category.clone(), name, format!("✗ {label}: {e}"));
            }
        }
    }

    let msg = format!("Completed: {success_count} succeeded, {error_count} failed");
    if error_count > 0 {
        ctx.log_error(category, name, msg.clone());
        ScriptResult::Error(msg)
    } else {
        ctx.log_success(category, name, msg.clone());
        ScriptResult::Success(msg)
    }
}

#[cfg(target_os = "windows")]
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(target_os = "windows")]
fn disable_startup_apps(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::{disable_hkcu_startup_entries, onedrive_in_use};

    let (category, name) = (def.category(), def.name.as_str());
    let mut failed = false;

    match disable_hkcu_startup_entries("msedge") {
        Ok(messages) => {
            for message in messages {
                ctx.log_info(category.clone(), name, format!("Edge: {message}"));
            }
        }
        Err(e) => {
            failed = true;
            ctx.log_error(category.clone(), name, format!("Edge startup: {e}"));
        }
    }

    if onedrive_in_use() {
        ctx.log_info(
            category.clone(),
            name,
            "OneDrive has a signed-in account; leaving its startup entry enabled.",
        );
    } else {
        match disable_hkcu_startup_entries("onedrive") {
            Ok(messages) => {
                for message in messages {
                    ctx.log_info(category.clone(), name, format!("OneDrive: {message}"));
                }
            }
            Err(e) => {
                failed = true;
                ctx.log_error(category.clone(), name, format!("OneDrive startup: {e}"));
            }
        }
        // Stop the running instance so sign-in prompts end immediately.
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "OneDrive.exe"])
            .creation_flags(0x08000000)
            .output();
        ctx.log_info(
            category.clone(),
            name,
            "OneDrive not signed in: killed OneDrive.exe",
        );
    }

    if failed {
        let msg = "Startup apps processed with errors";
        ctx.log_error(category, name, msg);
        ScriptResult::Error(msg.into())
    } else {
        ctx.log_success(category, name, "Startup apps processed");
        ScriptResult::Success("Startup apps processed".into())
    }
}

/// Unpins Copilot and removes its app package.
#[cfg(target_os = "windows")]
fn unpin_copilot(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::remove_copilot_appx;
    use crate::utilities::windows::registry::disable_copilot;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Unpinning Copilot from taskbar...");

    let mut failures = Vec::new();
    match disable_copilot() {
        Ok(results) => {
            for result in results {
                ctx.log_info(category.clone(), name, result);
            }
        }
        Err(e) => {
            let msg = format!("Failed to unpin Copilot: {e}");
            ctx.log_error(category.clone(), name, msg.clone());
            failures.push(msg);
        }
    }
    match remove_copilot_appx() {
        Ok(messages) => {
            for message in messages {
                ctx.log_info(category.clone(), name, message);
            }
        }
        Err(e) => {
            let msg = format!("Copilot app removal: {e}");
            ctx.log_error(category.clone(), name, msg.clone());
            failures.push(msg);
        }
    }

    if failures.is_empty() {
        ctx.log_success(category, name, "Copilot unpinned successfully");
        ScriptResult::Success("Copilot unpinned successfully".into())
    } else {
        ScriptResult::Error(failures.join("; "))
    }
}

/// Writes the shop's SuperAntiSpyware scheduled tasks and settings, then relaunches the tray app.
#[cfg(target_os = "windows")]
fn sas_settings(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::scripts::antivirus::{kill_sas_processes, launch_sas_tray, sas_tasks};

    let (category, name) = (def.category(), def.name.as_str());
    let sas_exe = std::path::Path::new(r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe");
    if !sas_exe.exists() {
        ctx.log_error(category, name, "SAS not installed");
        return ScriptResult::Error("SAS not installed".into());
    }

    let killed = kill_sas_processes();
    ctx.log_info(
        category.clone(),
        name,
        format!("Killed {killed} SAS processes"),
    );
    std::thread::sleep(std::time::Duration::from_secs(2));

    match sas_tasks::configure_sas_scheduled_tasks() {
        Ok((update_guid, scan_guid)) => {
            ctx.log_info(
                category.clone(),
                name,
                format!("SAS update task: {update_guid}"),
            );
            ctx.log_info(
                category.clone(),
                name,
                format!("SAS scan task: {scan_guid}"),
            );
            match launch_sas_tray() {
                Ok(()) => ctx.log_info(category.clone(), name, "Relaunched SUPERAntiSpyware"),
                Err(e) => ctx.log_warning(
                    category.clone(),
                    name,
                    format!("Could not relaunch SUPERAntiSpyware: {e}"),
                ),
            }
            ctx.log_success(category, name, "SuperAntiSpyware settings applied");
            ScriptResult::Success("SuperAntiSpyware settings applied".into())
        }
        Err(e) => {
            let msg = format!("Error: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

#[cfg(target_os = "windows")]
fn align_taskbar(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    use crate::utilities::windows::registry::align_taskbar_left;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, "Aligning taskbar to left...");

    match align_taskbar_left() {
        Ok(messages) => {
            for message in messages {
                ctx.log_info(category.clone(), name, message.trim().to_string());
            }
            ctx.log_success(category, name, "Taskbar aligned to left");
            ScriptResult::Success("Taskbar aligned to left".into())
        }
        Err(e) => {
            let msg = format!("Failed: {e}");
            ctx.log_error(category, name, msg.clone());
            ScriptResult::Error(msg)
        }
    }
}

/// Sets the timezone without logging tzutil's own output.
fn change_timezone(ctx: &ScriptContext, def: &ScriptDef) -> (ScriptResult, Option<i32>) {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(
        category.clone(),
        name,
        "Setting timezone to Mountain Standard Time...",
    );

    match powershell::run(SET_TIMEZONE) {
        Ok(output) => {
            ctx.log_success(category, name, "Timezone set to Mountain Standard Time");
            (
                ScriptResult::Success("Timezone set to Mountain Standard Time".into()),
                output.exit_code,
            )
        }
        Err(failure) => {
            let msg = format!("Failed to set timezone: {}", failure.message);
            ctx.log_error(category, name, msg.clone());
            (ScriptResult::Error(msg), failure.exit_code)
        }
    }
}

/// Reports the current BitLocker state, then decrypts every protected volume.
/// A failed status query is a warning; the decrypt still runs.
fn disable_bitlocker(ctx: &ScriptContext, def: &ScriptDef) -> (ScriptResult, Option<i32>) {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(
        category.clone(),
        name,
        "Checking BitLocker status on all drives...",
    );

    match powershell::run(BITLOCKER_STATUS) {
        Ok(output) => {
            for line in output.lines {
                ctx.log_info(category.clone(), name, line);
            }
        }
        Err(failure) => {
            ctx.log_warning(
                category.clone(),
                name,
                format!("Could not query BitLocker volumes: {}", failure.message),
            );
        }
    }

    match powershell::run(BITLOCKER_DISABLE) {
        Ok(output) => {
            for line in output.lines {
                ctx.log_info(category.clone(), name, line);
            }
            ctx.log_success(category, name, "BitLocker disable command completed");
            (
                ScriptResult::Success("BitLocker disable command completed".into()),
                output.exit_code,
            )
        }
        Err(failure) => {
            let msg = format!("Failed to disable BitLocker: {}", failure.message);
            ctx.log_error(category, name, msg.clone());
            (ScriptResult::Error(msg), failure.exit_code)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn disable_sleep(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn sas_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn webroot_scan(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn disable_notifications(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn disable_startup_apps(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn unpin_copilot(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn align_taskbar(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn sas_settings(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn unsupported(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

const SET_TIMEZONE: &str = r#"tzutil /s "Mountain Standard Time""#;

const BITLOCKER_STATUS: &str = r#"
                    $volumes = Get-BitLockerVolume -ErrorAction SilentlyContinue
                    if ($volumes) {
                        $volumes | ForEach-Object {
                            "$($_.MountPoint): $($_.VolumeStatus) / $($_.ProtectionStatus)"
                        }
                    } else {
                        "BitLocker not available or no encrypted volumes found"
                    }
                "#;

const BITLOCKER_DISABLE: &str = r#"
                    $volumes = Get-BitLockerVolume -ErrorAction SilentlyContinue |
                        Where-Object { $_.ProtectionStatus -eq 'On' -or $_.VolumeStatus -ne 'FullyDecrypted' }
                    if ($volumes) {
                        foreach ($vol in $volumes) {
                            Disable-BitLocker -MountPoint $vol.MountPoint -ErrorAction SilentlyContinue | Out-Null
                            "Disabling BitLocker on $($vol.MountPoint)"
                        }
                    } else {
                        "No BitLocker-protected volumes found"
                    }
                "#;

#[cfg(test)]
mod tuneup_executor_tests {
    use super::*;
    use displays::scripts::ScriptCategory;
    use displays::scripts::catalog::{CATALOG, Surface};

    #[test]
    fn every_claimed_id_is_a_real_tuneup_script() {
        for id in HANDLED {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(
                def.category(),
                ScriptCategory::Tuneup,
                "{id} is not a tuneup script"
            );
        }
    }

    /// The stub is catalog-known so a legacy name still resolves, but it is offered nowhere.
    #[test]
    fn the_unimplemented_stub_is_not_offered() {
        let def = CATALOG
            .get(&ScriptId::new("disable-proxy-settings"))
            .expect("catalog entry");
        assert!(!def.offered_on(Surface::Egui), "the stub is offered in the tab");
    }

    #[test]
    fn the_executor_claims_only_what_it_runs() {
        let executor = TuneupExecutor;
        for id in HANDLED {
            assert!(executor.handles(&ScriptId::new(*id)));
        }
        assert!(!executor.handles(&ScriptId::new("onelaunch")));
        assert!(!executor.handles(&ScriptId::new("stress-cpu")));
        assert!(!executor.handles(&ScriptId::new("windows-version")));
    }

    /// The install, update and composite executors run these; the tab runs Data Transfer itself.
    #[test]
    fn the_tuneup_executor_leaves_these_to_others() {
        let executor = TuneupExecutor;
        for id in [
            "data-transfer",
            "activate-cps",
            "activate-seb",
            "install-libreoffice",
            "install-windows-updates",
            "run-junkware-category",
        ] {
            assert!(!executor.handles(&ScriptId::new(id)), "{id} is claimed");
        }
    }
}
