use windows_registry::*;

const PUSH_NOTIFICATIONS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
const CONTENT_DELIVERY_MANAGER_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const EXPLORER_ADVANCED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const USER_NOTIFS: &str = r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; // ScoobeSystemSettingEnabled
const WINDOWS_COPILOT_KEY: &str = r"Software\Policies\Microsoft\Windows\WindowsCopilot";
const ACCOUNT_NOTIFICATIONS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\SystemSettings\AccountNotifications";
const AUX_PINS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband\AuxilliaryPins";

const HVCI_KEY: &str =
    r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity";
const HVCI_VALUE: &str = "Enabled";
const CI_CONFIG_KEY: &str = r"SYSTEM\CurrentControlSet\Control\CI\Config";
const BLOCKLIST_VALUE: &str = "VulnerableDriverBlocklistEnable";

/// MDM/GPO Device Guard policy hive; re-applied over the scenario key at boot.
const DG_POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DeviceGuard";
const DG_POLICY_HVCI_VALUE: &str = "HypervisorEnforcedCodeIntegrity";
const DG_POLICY_VBS_VALUE: &str = "EnableVirtualizationBasedSecurity";
/// `Win32_DeviceGuard` security-service id for HVCI.
const DG_SERVICE_HVCI: u32 = 2;

/// Opens (creating if missing) an HKCU key and sets `value_name` to `target`.
/// A missing key or value is treated as "not yet set", never as an error.
fn set_hkcu_u32(key_path: &str, value_name: &str, target: u32, friendly: &str) -> Result<String> {
    let key = CURRENT_USER.options().read().write().create().open(key_path)?;
    let current = key.get_u32(value_name).ok();
    if current == Some(target) {
        return Ok(format!("{friendly}: already set"));
    }
    key.set_u32(value_name, target)?;
    match current {
        Some(old) => Ok(format!("{friendly}: set ({old} -> {target})")),
        None => Ok(format!("{friendly}: created and set to {target}")),
    }
}

/// Flattens a registry result into a loggable line.
fn report(result: Result<String>) -> String {
    result.unwrap_or_else(|e| format!("registry error: {e:?}"))
}

pub fn disable_notifications() -> Result<Vec<String>> {
    let mut results = Vec::new();
    results.push(report(set_hkcu_u32(PUSH_NOTIFICATIONS_KEY, "ToastEnabled", 0, "Push Notifications (off)")));

    // Zero every SubscribedContent-* value present under ContentDeliveryManager.
    match CURRENT_USER.options().read().write().create().open(CONTENT_DELIVERY_MANAGER_KEY) {
        Ok(content_key) => match content_key.values() {
            Ok(values) => {
                for (val_name, _) in values {
                    if !val_name.contains("SubscribedContent") {
                        continue;
                    }
                    match content_key.get_u32(&val_name) {
                        Ok(0) => results.push(format!("{val_name}: already disabled")),
                        _ => match content_key.set_u32(&val_name, 0) {
                            Ok(_) => results.push(format!("{val_name}: disabled")),
                            Err(e) => results.push(format!("{val_name}: error disabling: {e:?}")),
                        },
                    }
                }
            }
            Err(e) => results.push(format!("ContentDeliveryManager values: {e:?}")),
        },
        Err(e) => results.push(format!("ContentDeliveryManager: {e:?}")),
    }

    results.push(report(set_hkcu_u32(USER_NOTIFS, "ScoobeSystemSettingEnabled", 0, "'Finish setting up your device' prompts (off)")));
    Ok(results)
}

pub fn align_taskbar_left() -> Result<Vec<String>> {
    Ok(vec![report(set_hkcu_u32(EXPLORER_ADVANCED_KEY, "TaskbarAl", 0, "Taskbar alignment (left)"))])
}

pub fn disable_lockscreen_notifications() -> Result<String> {
    set_hkcu_u32(PUSH_NOTIFICATIONS_KEY, "LockScreenToastEnabled", 0, "Lock screen notifications (off)")
}

/// Registry side of Copilot removal: taskbar button, PWA pin, and policy.
/// The pinned Copilot Store app itself is removed by
/// `crate::utilities::scripts::remove_copilot_appx`.
pub fn disable_copilot() -> Result<Vec<String>> {
    Ok(vec![
        report(set_hkcu_u32(AUX_PINS_KEY, "CopilotPWAPin", 0, "Copilot PWA pin (off)")),
        report(set_hkcu_u32(EXPLORER_ADVANCED_KEY, "ShowCopilotButton", 0, "Copilot taskbar button (off)")),
        report(set_hkcu_u32(WINDOWS_COPILOT_KEY, "TurnOffWindowsCopilot", 1, "Windows Copilot policy (disabled)")),
    ])
}

pub fn disable_content_delivery_allowed() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "ContentDeliveryAllowed", 0, "Content Delivery (off)")
}

pub fn disable_feature_management_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "FeatureManagementEnabled", 0, "Feature Management (off)")
}

pub fn disable_oem_preinstalled_apps_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "OemPreInstalledAppsEnabled", 0, "OEM Preinstalled Apps (off)")
}

pub fn disable_preinstalled_apps_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "PreInstalledAppsEnabled", 0, "Preinstalled Apps (off)")
}

pub fn disable_preinstalled_apps_ever_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "PreInstalledAppsEverEnabled", 0, "Preinstalled Apps Ever Enabled (off)")
}

pub fn disable_rotating_lockscreen_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "RotatingLockScreenEnabled", 0, "Rotating Lock Screen (off)")
}

pub fn disable_rotating_lockscreen_overlay_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "RotatingLockScreenOverlayEnabled", 0, "Rotating Lock Screen Overlay (off)")
}

pub fn disable_silent_installed_apps_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SilentInstalledAppsEnabled", 0, "Silent Installed Apps (off)")
}

pub fn disable_slideshow_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SlideshowEnabled", 0, "Slideshow (off)")
}

pub fn disable_soft_landing_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SoftLandingEnabled", 0, "Soft Landing tips (off)")
}

pub fn disable_subscribed_content_310093_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-310093Enabled", 0, "Subscribed Content 310093 (off)")
}

pub fn disable_subscribed_content_314563_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-314563Enabled", 0, "Subscribed Content 314563 (off)")
}

pub fn disable_subscribed_content_338388_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-338388Enabled", 0, "Subscribed Content 338388 (off)")
}

pub fn disable_subscribed_content_338389_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-338389Enabled", 0, "Subscribed Content 338389 (off)")
}

pub fn disable_subscribed_content_338393_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-338393Enabled", 0, "Subscribed Content 338393 (off)")
}

pub fn disable_subscribed_content_353694_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-353694Enabled", 0, "Subscribed Content 353694 (off)")
}

pub fn disable_subscribed_content_353696_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-353696Enabled", 0, "Subscribed Content 353696 (off)")
}

pub fn disable_subscribed_content_353698_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-353698Enabled", 0, "Subscribed Content 353698 (off)")
}

pub fn disable_subscribed_content_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContentEnabled", 0, "Subscribed Content (off)")
}

pub fn disable_system_pane_suggestions_enabled() -> Result<String> {
    set_hkcu_u32(CONTENT_DELIVERY_MANAGER_KEY, "SystemPaneSuggestionsEnabled", 0, "System Pane Suggestions (off)")
}

pub fn disable_account_notifications() -> Result<String> {
    set_hkcu_u32(ACCOUNT_NOTIFICATIONS_KEY, "EnableAccountNotifications", 0, "Account Notifications (off)")
}

// Explorer Advanced Settings
pub fn set_file_explorer_to_this_pc() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "LaunchTo", 1, "File Explorer opens to This PC")
}

pub fn show_file_extensions() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "HideFileExt", 0, "File extensions (shown)")
}

pub fn disable_folder_size_tips() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "FolderContentsInfoTip", 0, "Folder size tips (off)")
}

pub fn disable_popup_descriptions() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "ShowInfoTip", 0, "Popup descriptions (off)")
}

pub fn enable_more_pins_layout() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "Start_Layout", 1, "Start 'More pins' layout (on)")
}

pub fn disable_start_account_notifications() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "Start_AccountNotifications", 0, "Start account notifications (off)")
}

pub fn disable_recent_items_tracking() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "Start_TrackDocs", 0, "Recent items tracking (off)")
}

pub fn remove_chat_from_taskbar() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "TaskbarMn", 0, "Taskbar Chat button (off)")
}

pub fn remove_task_view_from_taskbar() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "ShowTaskViewButton", 0, "Task View button (off)")
}

pub fn remove_copilot_from_taskbar() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "ShowCopilotButton", 0, "Copilot taskbar button (off)")
}

pub fn disable_recommendations() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "Start_IrisRecommendations", 0, "Start recommendations (off)")
}

pub fn disable_taskbar_search() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "TaskbarSn", 0, "Taskbar Search (off)")
}

pub fn disable_snap_assist() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "SnapAssist", 0, "Snap Assist (off)")
}

pub fn disable_di_test() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "DITest", 0, "DITest (off)")
}

pub fn disable_snap_bar() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "EnableSnapBar", 0, "Snap Bar (off)")
}

pub fn disable_task_groups() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "EnableTaskGroups", 0, "Task Groups (off)")
}

pub fn disable_snap_assist_flyout() -> Result<String> {
    set_hkcu_u32(EXPLORER_ADVANCED_KEY, "EnableSnapAssistFlyout", 0, "Snap Assist Flyout (off)")
}

/// Result of a `set_driver_protections` call, with both flags read back after the write.
#[derive(Debug, Clone)]
pub struct DriverProtectionOutcome {
    pub success: bool,
    pub hvci_enabled: Option<bool>,
    pub blocklist_enabled: Option<bool>,
    /// HVCI in `Win32_DeviceGuard.SecurityServicesRunning` after the write.
    pub hvci_running: Option<bool>,
    /// HVCI in `Win32_DeviceGuard.SecurityServicesConfigured` after the write.
    pub hvci_configured: Option<bool>,
    /// `Win32_DeviceGuard.VirtualizationBasedSecurityStatus`: 0 off, 1 configured, 2 running.
    pub vbs_status: Option<u32>,
    /// Set when the Device Guard policy hive contradicts the requested state.
    pub policy_override: Option<String>,
    /// True only when a value actually changed and the change can take effect.
    pub reboot_required: bool,
    pub message: String,
}

/// Device Guard state as Windows reports it, independent of the scenario key.
#[derive(Debug, Clone, Default)]
pub struct DeviceGuardState {
    /// HVCI listed in `Win32_DeviceGuard.SecurityServicesRunning`.
    pub hvci_running: Option<bool>,
    /// HVCI listed in `Win32_DeviceGuard.SecurityServicesConfigured`.
    pub hvci_configured: Option<bool>,
    /// `Win32_DeviceGuard.VirtualizationBasedSecurityStatus`: 0 off, 1 configured, 2 running.
    pub vbs_status: Option<u32>,
    /// Policy-hive HVCI value: 0 off, 1 on with UEFI lock, 2 on without lock.
    pub policy_hvci: Option<u32>,
    pub policy_vbs: Option<u32>,
}

/// Reads an HKLM DWORD; `None` when the key or value is absent.
fn get_hklm_u32(key_path: &str, value_name: &str) -> Option<u32> {
    LOCAL_MACHINE
        .options()
        .read()
        .open(key_path)
        .ok()?
        .get_u32(value_name)
        .ok()
}

/// Reads an HKLM DWORD as a bool; `None` when the key or value is absent.
fn get_hklm_flag(key_path: &str, value_name: &str) -> Option<bool> {
    get_hklm_u32(key_path, value_name).map(|v| v != 0)
}

/// `Win32_DeviceGuard` running services, configured services, and VBS status.
fn read_device_guard_wmi() -> Option<(Vec<u32>, Vec<u32>, Option<u32>)> {
    #[derive(serde::Deserialize)]
    struct Win32DeviceGuard {
        #[serde(rename = "SecurityServicesRunning")]
        running: Option<Vec<u32>>,
        #[serde(rename = "SecurityServicesConfigured")]
        configured: Option<Vec<u32>>,
        #[serde(rename = "VirtualizationBasedSecurityStatus")]
        vbs_status: Option<u32>,
    }
    let wmi = wmi::WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\DeviceGuard")
        .map_err(|e| log::debug!("Win32_DeviceGuard namespace unavailable: {e}"))
        .ok()?;
    let rows: Vec<Win32DeviceGuard> = wmi
        .raw_query(
            "SELECT SecurityServicesRunning, SecurityServicesConfigured, \
             VirtualizationBasedSecurityStatus FROM Win32_DeviceGuard",
        )
        .map_err(|e| log::debug!("Win32_DeviceGuard query failed: {e}"))
        .ok()?;
    let row = rows.into_iter().next()?;
    Some((
        row.running.unwrap_or_default(),
        row.configured.unwrap_or_default(),
        row.vbs_status,
    ))
}

/// Live Device Guard state: WMI security services plus the policy hive.
pub fn read_device_guard_state() -> DeviceGuardState {
    let mut state = DeviceGuardState {
        policy_hvci: get_hklm_u32(DG_POLICY_KEY, DG_POLICY_HVCI_VALUE),
        policy_vbs: get_hklm_u32(DG_POLICY_KEY, DG_POLICY_VBS_VALUE),
        ..Default::default()
    };
    if let Some((running, configured, vbs_status)) = read_device_guard_wmi() {
        state.hvci_running = Some(running.contains(&DG_SERVICE_HVCI));
        state.hvci_configured = Some(configured.contains(&DG_SERVICE_HVCI));
        state.vbs_status = vbs_status;
    }
    state
}

/// Describes a policy-hive value that contradicts `enable`; `None` when nothing does.
fn policy_override_note(state: &DeviceGuardState, enable: bool) -> Option<String> {
    let mut hits = Vec::new();
    for (name, value) in [
        (DG_POLICY_HVCI_VALUE, state.policy_hvci),
        (DG_POLICY_VBS_VALUE, state.policy_vbs),
    ] {
        if let Some(v) = value {
            if (v != 0) != enable {
                hits.push(format!("{name}={v}"));
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(format!(
        "MDM/GPO policy at HKLM\\{DG_POLICY_KEY} contradicts this change ({}) — it is re-applied \
         at boot, and a UEFI-locked policy ignores the write entirely, so a reboot will not change \
         Device Guard",
        hits.join(", ")
    ))
}

/// Running Device Guard state as a loggable line.
fn device_guard_label(state: &DeviceGuardState) -> String {
    let hvci = match state.hvci_running {
        Some(true) => "running",
        Some(false) => "not running",
        None => "unreadable (Win32_DeviceGuard)",
    };
    let configured = match state.hvci_configured {
        Some(true) => ", configured",
        Some(false) => ", not configured",
        None => "",
    };
    match state.vbs_status {
        Some(v) => format!("Device Guard right now: HVCI {hvci}{configured}, VBS status {v}"),
        None => format!("Device Guard right now: HVCI {hvci}{configured}"),
    }
}

/// Opens (creating if missing) an HKLM key and sets `value_name` to `target`.
fn set_hklm_u32(key_path: &str, value_name: &str, target: u32) -> Result<()> {
    LOCAL_MACHINE
        .options()
        .read()
        .write()
        .create()
        .open(key_path)?
        .set_u32(value_name, target)
}

/// Current HVCI and vulnerable-driver-blocklist flags.
pub fn read_driver_protections() -> (Option<bool>, Option<bool>) {
    (
        get_hklm_flag(HVCI_KEY, HVCI_VALUE),
        get_hklm_flag(CI_CONFIG_KEY, BLOCKLIST_VALUE),
    )
}

/// Sets HVCI and the vulnerable-driver blocklist to `enable`, then reads both the
/// registry values and the live Device Guard state back.
pub fn set_driver_protections(enable: bool) -> DriverProtectionOutcome {
    let target = u32::from(enable);
    let (prior_hvci, prior_blocklist) = read_driver_protections();
    let mut errors = Vec::new();
    if let Err(e) = set_hklm_u32(HVCI_KEY, HVCI_VALUE, target) {
        errors.push(format!("{HVCI_VALUE}: {e}"));
    }
    if let Err(e) = set_hklm_u32(CI_CONFIG_KEY, BLOCKLIST_VALUE, target) {
        errors.push(format!("{BLOCKLIST_VALUE}: {e}"));
    }

    let (hvci_enabled, blocklist_enabled) = read_driver_protections();
    let applied = hvci_enabled == Some(enable) && blocklist_enabled == Some(enable);
    let write_ok = errors.is_empty() && applied;
    let changed = prior_hvci != hvci_enabled || prior_blocklist != blocklist_enabled;

    let guard = read_device_guard_state();
    let policy_override = policy_override_note(&guard, enable);
    // A policy-overridden write cannot take effect, so it is neither a success nor
    // worth a reboot.
    let success = write_ok && policy_override.is_none();
    let reboot_required = write_ok && changed && policy_override.is_none();

    let verb = if enable { "enabled" } else { "disabled" };
    let mut parts = Vec::new();
    if !errors.is_empty() {
        parts.push(format!(
            "Registry write failed — MasterTech needs to run elevated to write HKLM: {}",
            errors.join("; ")
        ));
    } else if !applied {
        parts.push(format!(
            "Write reported success but read-back disagrees (HVCI={hvci_enabled:?}, blocklist={blocklist_enabled:?})"
        ));
    } else if policy_override.is_some() {
        parts.push(format!(
            "Registry values are {verb} but the change will NOT take effect"
        ));
    } else if changed {
        parts.push(format!(
            "Memory Integrity and the Vulnerable Driver Blocklist are {verb}; reboot to apply"
        ));
    } else {
        parts.push(format!(
            "Memory Integrity and the Vulnerable Driver Blocklist were already {verb}; no reboot needed"
        ));
    }
    parts.push(device_guard_label(&guard));
    if let Some(note) = policy_override.as_ref() {
        parts.push(note.clone());
    }

    DriverProtectionOutcome {
        success,
        hvci_enabled,
        blocklist_enabled,
        hvci_running: guard.hvci_running,
        hvci_configured: guard.hvci_configured,
        vbs_status: guard.vbs_status,
        policy_override,
        reboot_required,
        message: parts.join("; "),
    }
}

/* OTHER REG TWEAKS
Windows Registry Editor Version 5.00


# Uninstall Copilot
Get-AppxPackage -Name 'Microsoft.Copilot' | Remove-AppxPackage
Get-AppxPackage -Name 'Microsoft.Windows.Ai.Copilot.Provider' | Remove-AppxPackage

; APPEARANCE AND PERSONALIZATION
; open file explorer to this pc
; show file name extensions
; disable display file size information in folder tips
; disable show pop-up description for folder and desktop items
; disable show translucent selection rectangle
; disable use drop shadows for icon labels on the desktop
; more pins personalization start
; disable show account-related notifications
; disable show recently opened items in start, jump lists and file explorer
; left taskbar alignment
; remove chat from taskbar
; remove task view from taskbar
; remove copilot from taskbar
; disable show recommendations for tips shortcuts new apps and more
[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced]
"LaunchTo"=dword:00000001
"HideFileExt"=dword:00000000
"FolderContentsInfoTip"=dword:00000000
"ShowInfoTip"=dword:00000000
"ListviewAlphaSelect"=dword:0
"ListviewShadow"=dword:0
"Start_Layout"=dword:00000001
"Start_AccountNotifications"=dword:00000000
"Start_TrackDocs"=dword:00000000
"TaskbarAl"=dword:00000000
"TaskbarMn"=dword:00000000
"ShowTaskViewButton"=dword:00000000
"ShowCopilotButton"=dword:00000000
"Start_IrisRecommendations"=dword:00000000
"TaskbarSn"=dword:00000000
"SnapAssist"=dword:00000000
"DITest"=dword:00000000
"EnableSnapBar"=dword:00000000
"EnableTaskGroups"=dword:00000000
"EnableSnapAssistFlyout"=dword:00000000

; show all taskbar icons on Windows 10
[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer]
"ShowFrequent"=dword:00000000
"ShowCloudFilesInQuickAccess"=dword:00000000
"EnableAutoTray"=dword:00000000


; --IMMERSIVE CONTROL PANEL--
; PRIVACY
; disable show me notification in the settings app
[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\SystemSettings\AccountNotifications]
"EnableAccountNotifications"=dword:00000000


; disable notifications
; Disable Notifications on Lock Screen
[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\PushNotifications]
"ToastEnabled"=dword:00000000
"LockScreenToastEnabled"=dword:00000000

; disable copilot
[HKEY_CURRENT_USER\Software\Policies\Microsoft\Windows\WindowsCopilot]
"TurnOffWindowsCopilot"=dword:00000001

; DISABLE ADVERTISING & PROMOTIONAL
[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager]
"ContentDeliveryAllowed"=dword:00000000
"FeatureManagementEnabled"=dword:00000000
"OemPreInstalledAppsEnabled"=dword:00000000
"PreInstalledAppsEnabled"=dword:00000000
"PreInstalledAppsEverEnabled"=dword:00000000
"RotatingLockScreenEnabled"=dword:00000000
"RotatingLockScreenOverlayEnabled"=dword:00000000
"SilentInstalledAppsEnabled"=dword:00000000
"SlideshowEnabled"=dword:00000000
"SoftLandingEnabled"=dword:00000000
"SubscribedContent-310093Enabled"=dword:00000000
"SubscribedContent-314563Enabled"=dword:00000000
"SubscribedContent-338388Enabled"=dword:00000000
"SubscribedContent-338389Enabled"=dword:00000000
"SubscribedContent-338393Enabled"=dword:00000000
"SubscribedContent-353694Enabled"=dword:00000000
"SubscribedContent-353696Enabled"=dword:00000000
"SubscribedContent-353698Enabled"=dword:00000000
"SubscribedContentEnabled"=dword:00000000
"SystemPaneSuggestionsEnabled"=dword:00000000

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband\AuxilliaryPins]
"MailPin"=dword:00000000
"TFLPin"=dword:00000000
"CopilotPWAPin"=dword:00000000

*/
