//! Junkware removal for this machine.
//!
//! Nine of these are the same uninstall keyed on a different publisher string, so
//! the executor claims the whole category and looks the publisher up by id.

use std::time::Instant;

use displays::scripts::ScriptCategory;
use displays::scripts::catalog::{CATALOG, ScriptDef};
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

use super::powershell;

#[cfg(target_os = "windows")]
use crate::utilities::scripts::InstalledProgram;

/// Publisher substring each removal matches an uninstall entry on.
const PUBLISHERS: &[(&str, &str)] = &[
    ("onelaunch", "onelaunch"),
    ("webnavigator-browser", "webnavigator"),
    ("wave-browser", "wavesor"),
    ("clear-browser", "clear browser"),
    ("shift-browser", "shift technologies"),
    ("avast-browser", "avast"),
    ("mcaffee-safe", "mcafee"),
    ("driver-support", "driver support"),
    ("winzip", "winzip"),
    ("junk-eset", "eset"),
];

pub struct JunkwareExecutor;

impl ScriptExecutor for JunkwareExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        CATALOG
            .get(id)
            .is_some_and(|def| def.category() == ScriptCategory::JunkwareRemoval)
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
        "uninstall-microsoft-365" => powershell::logged(
            ctx,
            def,
            "Searching for Microsoft 365 / Office installations...",
            UNINSTALL_MICROSOFT_365,
            "Microsoft 365 uninstall script completed",
        ),
        "uninstall-onedrive" => powershell::logged(
            ctx,
            def,
            "Uninstalling OneDrive...",
            UNINSTALL_ONEDRIVE,
            "OneDrive uninstall completed",
        ),
        "disable-onedrive-startup" => powershell::logged(
            ctx,
            def,
            "Disabling OneDrive startup...",
            DISABLE_ONEDRIVE_STARTUP,
            "OneDrive startup disabled",
        ),
        "disable-edge-startup-boost" => powershell::logged(
            ctx,
            def,
            "Disabling Edge startup boost and background running...",
            DISABLE_EDGE_STARTUP_BOOST,
            "Edge startup boost disabled",
        ),
        "scan-for-browser-hijackers" => (browser_hijack(ctx, def, false), None),
        "remove-browser-hijackers" => (browser_hijack(ctx, def, true), None),
        _ => (remove_program(ctx, def), None),
    }
}

/// The publisher an id matches on, defaulting to the display name.
fn publisher_for(def: &ScriptDef) -> String {
    PUBLISHERS
        .iter()
        .find(|(id, _)| *id == def.id.as_str())
        .map(|(_, publisher)| (*publisher).to_string())
        .unwrap_or_else(|| def.name.to_lowercase())
}

#[cfg(target_os = "windows")]
fn remove_program(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(category.clone(), name, format!("Searching for {name}..."));

    let publisher_match = publisher_for(def);

    if let Ok(programs) = InstalledProgram::get_installed_programs() {
        for program in &programs {
            let Some(publisher) = program.publisher.as_ref() else {
                continue;
            };
            if !publisher.to_lowercase().contains(&publisher_match) {
                continue;
            }

            ctx.log_info(
                category.clone(),
                name,
                format!("Found {name}, attempting uninstall..."),
            );

            return match program.uninstall() {
                Ok(_) => {
                    let msg = format!("Uninstalled {name}");
                    ctx.log_success(category, name, msg.clone());
                    ScriptResult::Success(msg)
                }
                Err(e) => {
                    let msg = format!("Failed to uninstall {name}: {e}");
                    ctx.log_error(category, name, msg.clone());
                    ScriptResult::Error(msg)
                }
            };
        }
    }

    // Absent junkware is the expected outcome, not a failure.
    let msg = format!("{name} not found (OK)");
    ctx.log_info(category, name, msg.clone());
    ScriptResult::Success(msg)
}

#[cfg(target_os = "windows")]
fn browser_hijack(ctx: &ScriptContext, def: &ScriptDef, remove: bool) -> ScriptResult {
    use crate::utilities::windows::browser_hijack;

    let (category, name) = (def.category(), def.name.as_str());
    ctx.log_info(
        category.clone(),
        name,
        "Checking browser policies, shortcuts, autostart entries and profiles...",
    );

    let findings = browser_hijack::scan();
    for finding in &findings {
        ctx.log_info(category.clone(), name, finding.line());
    }

    if findings.is_empty() {
        ctx.log_success(category, name, "No browser hijacks found");
        return ScriptResult::Success("No browser hijacks found".into());
    }

    if !remove {
        let msg = format!(
            "{} hijack finding(s) — run Remove Browser Hijackers to clean",
            findings.len()
        );
        ctx.log_warning(category, name, msg.clone());
        return ScriptResult::Warning(msg);
    }

    for action in browser_hijack::remediate() {
        ctx.log_info(category.clone(), name, action);
    }

    let left = browser_hijack::scan();
    let unresolved = left.iter().filter(|f| f.kind.is_removable()).count();

    if unresolved > 0 {
        let msg = format!(
            "{unresolved} hijack entr(ies) survived cleanup — check permissions and rerun elevated"
        );
        ctx.log_error(category, name, msg.clone());
        ScriptResult::Error(msg)
    } else if left.is_empty() {
        ctx.log_success(category, name, "Browser hijacks removed");
        ScriptResult::Success("Browser hijacks removed".into())
    } else {
        let msg = format!(
            "Cleaned; {} profile override(s) still need a manual reset",
            left.len()
        );
        ctx.log_warning(category, name, msg.clone());
        ScriptResult::Warning(msg)
    }
}

#[cfg(not(target_os = "windows"))]
fn remove_program(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn browser_hijack(ctx: &ScriptContext, def: &ScriptDef, _remove: bool) -> ScriptResult {
    unsupported(ctx, def)
}

#[cfg(not(target_os = "windows"))]
fn unsupported(ctx: &ScriptContext, def: &ScriptDef) -> ScriptResult {
    let msg = "Only available on Windows";
    ctx.log_warning(def.category(), def.name.as_str(), msg);
    ScriptResult::Skipped(msg.into())
}

const UNINSTALL_MICROSOFT_365: &str = r#"
                    $paths = @(
                        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
                        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
                        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
                    )
                    $office = $paths | ForEach-Object {
                        if (Test-Path $_) {
                            Get-ItemProperty $_ -ErrorAction SilentlyContinue |
                                Where-Object { $_.DisplayName -match "Microsoft 365|Microsoft Office" }
                        }
                    }
                    if ($office) {
                        foreach ($app in $office) {
                            if ($app.UninstallString) {
                                "Found: $($app.DisplayName) — uninstalling..."
                                $cmd = $app.UninstallString
                                if ($cmd -match "OfficeClickToRun") {
                                    & "$env:CommonProgramFiles\Microsoft Shared\ClickToRun\OfficeC2RClient.exe" /update user displaylevel=false forceappshutdown=true updatepromptuser=false
                                    Start-Sleep -Seconds 2
                                    & "$env:CommonProgramFiles\Microsoft Shared\ClickToRun\OfficeC2RClient.exe" /uninstall displaylevel=false
                                } elseif ($cmd -match "MsiExec") {
                                    $productCode = ([regex]'\{[A-F0-9-]+\}').Match($cmd).Value
                                    if ($productCode) { msiexec /x $productCode /quiet /norestart }
                                } else {
                                    Invoke-Expression "& $cmd /silent /norestart" 2>$null
                                }
                            }
                        }
                        "Microsoft 365/Office uninstall initiated"
                    } else {
                        "Microsoft 365/Office not found"
                    }
                "#;

const UNINSTALL_ONEDRIVE: &str = r#"
                    taskkill /F /IM OneDrive.exe 2>$null
                    Start-Sleep -Seconds 1
                    $setup64 = "$env:SystemRoot\SysWOW64\OneDriveSetup.exe"
                    $setup32 = "$env:SystemRoot\System32\OneDriveSetup.exe"
                    if (Test-Path $setup64) {
                        & $setup64 /uninstall
                        "OneDrive (64-bit) uninstall initiated"
                    } elseif (Test-Path $setup32) {
                        & $setup32 /uninstall
                        "OneDrive (32-bit) uninstall initiated"
                    } else {
                        "OneDriveSetup.exe not found, trying winget..."
                        winget uninstall "Microsoft.OneDrive" --silent --accept-source-agreements 2>$null
                    }
                "#;

const DISABLE_ONEDRIVE_STARTUP: &str = r#"
                    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
                    if (Get-ItemProperty -Path $runKey -Name "OneDrive" -ErrorAction SilentlyContinue) {
                        Remove-ItemProperty -Path $runKey -Name "OneDrive" -ErrorAction SilentlyContinue
                        "Removed OneDrive from HKCU Run key"
                    } else {
                        "OneDrive not found in Run key"
                    }
                    $odPolicies = "HKLM:\SOFTWARE\Policies\Microsoft\OneDrive"
                    if (-not (Test-Path $odPolicies)) { New-Item -Path $odPolicies -Force | Out-Null }
                    Set-ItemProperty -Path $odPolicies -Name "KFMBlockOptIn" -Value 1 -Type DWord
                    "OneDrive Known Folder Move blocked via policy"
                    taskkill /F /IM OneDrive.exe 2>$null
                    "OneDrive process terminated"
                "#;

const DISABLE_EDGE_STARTUP_BOOST: &str = r#"
                    $edgePolicy = "HKLM:\SOFTWARE\Policies\Microsoft\Edge"
                    if (-not (Test-Path $edgePolicy)) { New-Item -Path $edgePolicy -Force | Out-Null }
                    Set-ItemProperty -Path $edgePolicy -Name "StartupBoostEnabled" -Value 0 -Type DWord
                    "Edge StartupBoost disabled via policy"
                    Set-ItemProperty -Path $edgePolicy -Name "BackgroundModeEnabled" -Value 0 -Type DWord
                    "Edge BackgroundMode disabled via policy"
                    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
                    if (Get-ItemProperty -Path $runKey -Name "MicrosoftEdge*" -ErrorAction SilentlyContinue) {
                        Remove-ItemProperty -Path $runKey -Name "MicrosoftEdge*" -ErrorAction SilentlyContinue
                        "Removed Edge from HKCU Run key"
                    }
                    taskkill /F /IM msedge.exe 2>$null
                    "Edge process terminated"
                "#;

#[cfg(test)]
mod junkware_executor_tests {
    use super::*;
    use displays::scripts::catalog::Surface;

    /// The ids `run` dispatches somewhere other than the publisher uninstall.
    const DEDICATED: &[&str] = &[
        "uninstall-microsoft-365",
        "uninstall-onedrive",
        "disable-onedrive-startup",
        "disable-edge-startup-boost",
        "scan-for-browser-hijackers",
        "remove-browser-hijackers",
    ];

    /// A junkware entry with neither a body nor a publisher would silently match
    /// on its own display name and uninstall nothing.
    #[test]
    fn every_junkware_script_has_a_body_or_a_publisher() {
        for def in CATALOG.iter() {
            if def.category() != ScriptCategory::JunkwareRemoval {
                continue;
            }
            let id = def.id.as_str();
            assert!(
                DEDICATED.contains(&id) || PUBLISHERS.iter().any(|(known, _)| *known == id),
                "{id} has no executor body"
            );
        }
    }

    #[test]
    fn every_junkware_script_the_tab_offers_is_claimed() {
        let executor = JunkwareExecutor;
        for def in CATALOG.iter() {
            if def.category() != ScriptCategory::JunkwareRemoval || !def.offered_on(Surface::Egui) {
                continue;
            }
            assert!(executor.handles(&def.id), "{} has no executor", def.id);
        }
    }

    #[test]
    fn nothing_outside_the_family_is_claimed() {
        let executor = JunkwareExecutor;
        assert!(!executor.handles(&ScriptId::new("activate-cps")));
        assert!(!executor.handles(&ScriptId::new("windows-version")));
        assert!(!executor.handles(&ScriptId::new("stress-cpu")));
    }

    /// The publisher table is keyed by id; a typo would fall back to the display
    /// name and quietly stop matching.
    #[test]
    fn every_publisher_entry_names_a_real_junkware_script() {
        for (id, _) in PUBLISHERS {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(def.category(), ScriptCategory::JunkwareRemoval);
        }
    }

    #[test]
    fn a_script_without_a_publisher_entry_falls_back_to_its_name() {
        let def = CATALOG
            .get(&ScriptId::new("uninstall-onedrive"))
            .expect("catalog entry");
        assert_eq!(publisher_for(def), "uninstall onedrive");

        let onelaunch = CATALOG
            .get(&ScriptId::new("onelaunch"))
            .expect("catalog entry");
        assert_eq!(publisher_for(onelaunch), "onelaunch");
    }
}
