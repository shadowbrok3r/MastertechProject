use windows_registry::*;

const PUSH_NOTIFICATIONS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
const CONTENT_DELIVERY_MANAGER_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const EXPLORER_ADVANCED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const USER_NOTIFS: &str = r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; // ScoobeSystemSettingEnabled
const WINDOWS_COPILOT_KEY: &str = r"Software\Policies\Microsoft\Windows\WindowsCopilot";
const ACCOUNT_NOTIFICATIONS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\SystemSettings\AccountNotifications";

pub fn disable_notifications() -> Result<Vec<String>> {
    let mut results = Vec::new();
    let push_notifs_key = CURRENT_USER.options().read().write().open(PUSH_NOTIFICATIONS_KEY)?;

    let value = push_notifs_key.get_u32("ToastEnabled")?;
    if value == 1 {
        match push_notifs_key.set_u32("ToastEnabled", 0x000) {
            Ok(_) => results.push("DISABLED Push Notifications".to_string()),
            Err(e) => results.push(format!("Push Notifications are ENABLED, but there was an error disabling: {e:?}")),
        }
    } else {
        results.push("Push Notifications are DISABLED.".to_string());
    }

    
    let content_key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    
    // Iterate through all the values in the registry key
    for (val_name, val_data) in content_key.values()? {
        // Check if the value name contains "SubscribedContent"
        if val_name.contains("SubscribedContent") {
            log::info!("check_content_delivery_manager => \nval_name: {val_name:?}\nval_data: {val_data:?}");
            let value = content_key.get_u32(val_name.clone())?;
            if value == 1 {
                match CURRENT_USER.set_u32("SubscribedContent", 0x000) {
                    Ok(_) => results.push(format!("DISABLED SubscribedContent")),
                    Err(e) => results.push(format!("Push Notifications are ENABLED, but there was an error disabling: {e:?}")),
                }
                log::info!("Key {} is ENABLED.", &val_name);
                results.push(format!("Key {} is ENABLED.", &val_name));
            } else {
                log::info!("Key {} is DISABLED.", &val_name);
                results.push(format!("Key {} is DISABLED.", &val_name));
            }
        } 
    }

    let user_notifs_key = CURRENT_USER.options().read().write().open(USER_NOTIFS)?;

    let value = user_notifs_key.get_u32("ScoobeSystemSettingEnabled")?;
    if value == 1 {
        match user_notifs_key.set_u32("ScoobeSystemSettingEnabled", 0x000) {
            Ok(_) => results.push("DISABLED User Notifications".to_string()),
            Err(e) => results.push(format!("User Notifications are ENABLED, but there was an error disabling: {e:?}")),
        }
    } else {
        results.push("Push Notifications are DISABLED.".to_string());
    }

    // If we do not find a 0 value for any "SubscribedContent" value, assume it's enabled.
    Ok(results)
}

// Function to check the value of the Explorer Advanced registry key
pub fn align_taskbar_left() -> Result<String> {
    let key = CURRENT_USER.options().read().write().create().open(EXPLORER_ADVANCED_KEY)?;
    let return_key = &mut String::new();
    let value = key.get_u32("TaskbarAl")?;
    log::info!("Value: {value}");
    if value == 1 {
        if let Err(e) = key.set_u32("TaskbarAl", 0x000) {
            *return_key = format!("Need to create the reg key: {e:?}");
            match key.create("ToastEnabled") {
                Ok(_) => *return_key = "DISABLED Push Notifications".to_string(),
                Err(e) => *return_key = format!("Error Left Centering TaskBar: {e:?}"),
            }
        }
        match key.set_u32("TaskbarAl", 0x000) {
            Ok(_) => *return_key = format!("Left Centered TaskBar"),
            Err(e) => *return_key = format!("Error Left Centering TaskBar: {e:?}"),
        }
    } else if value == 0 {
        *return_key = "TaskBar is Left Aligned".to_string();
    }
    Ok(return_key.clone())
}

// Disable Lock Screen Notifications
pub fn disable_lockscreen_notifications() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(PUSH_NOTIFICATIONS_KEY)?;
    let value: u32 = key.get_u32("LockScreenToastEnabled")?;
    if value == 1 {
        match key.set_u32("LockScreenToastEnabled", 0x000) {
            Ok(_) => Ok("Lock Screen Notifications Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Lock Screen Notifications: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Lock Screen Notifications Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected LockScreenToastEnabled Value: {}", value))
    }
}

// Disable Copilot
pub fn disable_copilot() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(WINDOWS_COPILOT_KEY)?;
    let value: u32 = key.get_u32("TurnOffWindowsCopilot").unwrap_or(0);
    if value == 0 {
        match key.set_u32("TurnOffWindowsCopilot", 1) {
            Ok(_) => Ok("Copilot Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Copilot: {:?}", e)),
        }
    } else if value == 1 {
        Ok("Copilot Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected TurnOffWindowsCopilot Value: {}", value))
    }
}

// Disable Advertising & Promotional - All 20 keys
pub fn disable_content_delivery_allowed() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("ContentDeliveryAllowed")?;
    if value == 1 {
        match key.set_u32("ContentDeliveryAllowed", 0x000) {
            Ok(_) => Ok("Content Delivery Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Content Delivery: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Content Delivery Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected ContentDeliveryAllowed Value: {}", value))
    }
}

pub fn disable_feature_management_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("FeatureManagementEnabled")?;
    if value == 1 {
        match key.set_u32("FeatureManagementEnabled", 0x000) {
            Ok(_) => Ok("Feature Management Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Feature Management: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Feature Management Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected FeatureManagementEnabled Value: {}", value))
    }
}

pub fn disable_oem_preinstalled_apps_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("OemPreInstalledAppsEnabled")?;
    if value == 1 {
        match key.set_u32("OemPreInstalledAppsEnabled", 0x000) {
            Ok(_) => Ok("OEM Preinstalled Apps Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling OEM Preinstalled Apps: {:?}", e)),
        }
    } else if value == 0 {
        Ok("OEM Preinstalled Apps Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected OemPreInstalledAppsEnabled Value: {}", value))
    }
}

pub fn disable_preinstalled_apps_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("PreInstalledAppsEnabled")?;
    if value == 1 {
        match key.set_u32("PreInstalledAppsEnabled", 0x000) {
            Ok(_) => Ok("Preinstalled Apps Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Preinstalled Apps: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Preinstalled Apps Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected PreInstalledAppsEnabled Value: {}", value))
    }
}

pub fn disable_preinstalled_apps_ever_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("PreInstalledAppsEverEnabled")?;
    if value == 1 {
        match key.set_u32("PreInstalledAppsEverEnabled", 0x000) {
            Ok(_) => Ok("Preinstalled Apps Ever Enabled Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Preinstalled Apps Ever Enabled: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Preinstalled Apps Ever Enabled Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected PreInstalledAppsEverEnabled Value: {}", value))
    }
}

pub fn disable_rotating_lockscreen_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("RotatingLockScreenEnabled")?;
    if value == 1 {
        match key.set_u32("RotatingLockScreenEnabled", 0x000) {
            Ok(_) => Ok("Rotating Lock Screen Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Rotating Lock Screen: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Rotating Lock Screen Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected RotatingLockScreenEnabled Value: {}", value))
    }
}

pub fn disable_rotating_lockscreen_overlay_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("RotatingLockScreenOverlayEnabled")?;
    if value == 1 {
        match key.set_u32("RotatingLockScreenOverlayEnabled", 0x000) {
            Ok(_) => Ok("Rotating Lock Screen Overlay Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Rotating Lock Screen Overlay: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Rotating Lock Screen Overlay Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected RotatingLockScreenOverlayEnabled Value: {}", value))
    }
}

pub fn disable_silent_installed_apps_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SilentInstalledAppsEnabled")?;
    if value == 1 {
        match key.set_u32("SilentInstalledAppsEnabled", 0x000) {
            Ok(_) => Ok("Silent Installed Apps Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Silent Installed Apps: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Silent Installed Apps Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SilentInstalledAppsEnabled Value: {}", value))
    }
}

pub fn disable_slideshow_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SlideshowEnabled")?;
    if value == 1 {
        match key.set_u32("SlideshowEnabled", 0x000) {
            Ok(_) => Ok("Slideshow Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Slideshow: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Slideshow Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SlideshowEnabled Value: {}", value))
    }
}

pub fn disable_soft_landing_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SoftLandingEnabled")?;
    if value == 1 {
        match key.set_u32("SoftLandingEnabled", 0x000) {
            Ok(_) => Ok("Soft Landing Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Soft Landing: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Soft Landing Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SoftLandingEnabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_310093_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-310093Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-310093Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 310093 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 310093: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 310093 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-310093Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_314563_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-314563Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-314563Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 314563 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 314563: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 314563 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-314563Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_338388_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-338388Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-338388Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 338388 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 338388: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 338388 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-338388Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_338389_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-338389Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-338389Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 338389 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 338389: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 338389 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-338389Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_338393_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-338393Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-338393Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 338393 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 338393: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 338393 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-338393Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_353694_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-353694Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-353694Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 353694 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 353694: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 353694 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-353694Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_353696_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-353696Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-353696Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 353696 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 353696: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 353696 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-353696Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_353698_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContent-353698Enabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContent-353698Enabled", 0x000) {
            Ok(_) => Ok("Subscribed Content 353698 Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content 353698: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content 353698 Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContent-353698Enabled Value: {}", value))
    }
}

pub fn disable_subscribed_content_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SubscribedContentEnabled")?;
    if value == 1 {
        match key.set_u32("SubscribedContentEnabled", 0x000) {
            Ok(_) => Ok("Subscribed Content Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Subscribed Content: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Subscribed Content Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SubscribedContentEnabled Value: {}", value))
    }
}

pub fn disable_system_pane_suggestions_enabled() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(CONTENT_DELIVERY_MANAGER_KEY)?;
    let value: u32 = key.get_u32("SystemPaneSuggestionsEnabled")?;
    if value == 1 {
        match key.set_u32("SystemPaneSuggestionsEnabled", 0x000) {
            Ok(_) => Ok("System Pane Suggestions Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling System Pane Suggestions: {:?}", e)),
        }
    } else if value == 0 {
        Ok("System Pane Suggestions Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SystemPaneSuggestionsEnabled Value: {}", value))
    }
}

// Disable Account Notifications
pub fn disable_account_notifications() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(ACCOUNT_NOTIFICATIONS_KEY)?;
    let value: u32 = key.get_u32("EnableAccountNotifications")?;
    if value == 1 {
        match key.set_u32("EnableAccountNotifications", 0x000) {
            Ok(_) => Ok("Account Notifications Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Account Notifications: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Account Notifications Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected EnableAccountNotifications Value: {}", value))
    }
}

// Explorer Advanced Settings
pub fn set_file_explorer_to_this_pc() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("LaunchTo").unwrap_or(2); // Default is typically 2
    if value != 1 {
        match key.set_u32("LaunchTo", 1) {
            Ok(_) => Ok("File Explorer Set to This PC".to_string()),
            Err(e) => Ok(format!("Error Setting File Explorer to This PC: {:?}", e)),
        }
    } else {
        Ok("File Explorer Already Set to This PC".to_string())
    }
}

pub fn show_file_extensions() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("HideFileExt")?;
    if value == 1 {
        match key.set_u32("HideFileExt", 0x000) {
            Ok(_) => Ok("File Extensions Shown".to_string()),
            Err(e) => Ok(format!("Error Showing File Extensions: {:?}", e)),
        }
    } else if value == 0 {
        Ok("File Extensions Already Shown".to_string())
    } else {
        Ok(format!("Unexpected HideFileExt Value: {}", value))
    }
}

pub fn disable_folder_size_tips() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("FolderContentsInfoTip")?;
    if value == 1 {
        match key.set_u32("FolderContentsInfoTip", 0x000) {
            Ok(_) => Ok("Folder Size Tips Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Folder Size Tips: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Folder Size Tips Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected FolderContentsInfoTip Value: {}", value))
    }
}

pub fn disable_popup_descriptions() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("ShowInfoTip")?;
    if value == 1 {
        match key.set_u32("ShowInfoTip", 0x000) {
            Ok(_) => Ok("Popup Descriptions Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Popup Descriptions: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Popup Descriptions Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected ShowInfoTip Value: {}", value))
    }
}

pub fn enable_more_pins_layout() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("Start_Layout").unwrap_or(0);
    if value != 1 {
        match key.set_u32("Start_Layout", 1) {
            Ok(_) => Ok("More Pins Layout Enabled".to_string()),
            Err(e) => Ok(format!("Error Enabling More Pins Layout: {:?}", e)),
        }
    } else {
        Ok("More Pins Layout Already Enabled".to_string())
    }
}

pub fn disable_start_account_notifications() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("Start_AccountNotifications")?;
    if value == 1 {
        match key.set_u32("Start_AccountNotifications", 0x000) {
            Ok(_) => Ok("Start Account Notifications Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Start Account Notifications: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Start Account Notifications Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected Start_AccountNotifications Value: {}", value))
    }
}

pub fn disable_recent_items_tracking() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("Start_TrackDocs")?;
    if value == 1 {
        match key.set_u32("Start_TrackDocs", 0x000) {
            Ok(_) => Ok("Recent Items Tracking Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Recent Items Tracking: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Recent Items Tracking Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected Start_TrackDocs Value: {}", value))
    }
}

pub fn remove_chat_from_taskbar() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("TaskbarMn")?;
    if value == 1 {
        match key.set_u32("TaskbarMn", 0x000) {
            Ok(_) => Ok("Chat Removed from Taskbar".to_string()),
            Err(e) => Ok(format!("Error Removing Chat from Taskbar: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Chat Already Removed from Taskbar".to_string())
    } else {
        Ok(format!("Unexpected TaskbarMn Value: {}", value))
    }
}

pub fn remove_task_view_from_taskbar() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("ShowTaskViewButton")?;
    if value == 1 {
        match key.set_u32("ShowTaskViewButton", 0x000) {
            Ok(_) => Ok("Task View Removed from Taskbar".to_string()),
            Err(e) => Ok(format!("Error Removing Task View from Taskbar: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Task View Already Removed from Taskbar".to_string())
    } else {
        Ok(format!("Unexpected ShowTaskViewButton Value: {}", value))
    }
}

pub fn remove_copilot_from_taskbar() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("ShowCopilotButton")?;
    if value == 1 {
        match key.set_u32("ShowCopilotButton", 0x000) {
            Ok(_) => Ok("Copilot Removed from Taskbar".to_string()),
            Err(e) => Ok(format!("Error Removing Copilot from Taskbar: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Copilot Already Removed from Taskbar".to_string())
    } else {
        Ok(format!("Unexpected ShowCopilotButton Value: {}", value))
    }
}

pub fn disable_recommendations() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("Start_IrisRecommendations")?;
    if value == 1 {
        match key.set_u32("Start_IrisRecommendations", 0x000) {
            Ok(_) => Ok("Recommendations Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Recommendations: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Recommendations Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected Start_IrisRecommendations Value: {}", value))
    }
}

pub fn disable_taskbar_search() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("TaskbarSn")?;
    if value == 1 {
        match key.set_u32("TaskbarSn", 0x000) {
            Ok(_) => Ok("Taskbar Search Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Taskbar Search: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Taskbar Search Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected TaskbarSn Value: {}", value))
    }
}

pub fn disable_snap_assist() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("SnapAssist")?;
    if value == 1 {
        match key.set_u32("SnapAssist", 0x000) {
            Ok(_) => Ok("Snap Assist Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Snap Assist: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Snap Assist Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected SnapAssist Value: {}", value))
    }
}

pub fn disable_di_test() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("DITest")?;
    if value == 1 {
        match key.set_u32("DITest", 0x000) {
            Ok(_) => Ok("DITest Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling DITest: {:?}", e)),
        }
    } else if value == 0 {
        Ok("DITest Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected DITest Value: {}", value))
    }
}

pub fn disable_snap_bar() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("EnableSnapBar")?;
    if value == 1 {
        match key.set_u32("EnableSnapBar", 0x000) {
            Ok(_) => Ok("Snap Bar Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Snap Bar: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Snap Bar Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected EnableSnapBar Value: {}", value))
    }
}

pub fn disable_task_groups() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("EnableTaskGroups")?;
    if value == 1 {
        match key.set_u32("EnableTaskGroups", 0x000) {
            Ok(_) => Ok("Task Groups Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Task Groups: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Task Groups Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected EnableTaskGroups Value: {}", value))
    }
}

pub fn disable_snap_assist_flyout() -> Result<String> {
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let value: u32 = key.get_u32("EnableSnapAssistFlyout")?;
    if value == 1 {
        match key.set_u32("EnableSnapAssistFlyout", 0x000) {
            Ok(_) => Ok("Snap Assist Flyout Disabled".to_string()),
            Err(e) => Ok(format!("Error Disabling Snap Assist Flyout: {:?}", e)),
        }
    } else if value == 0 {
        Ok("Snap Assist Flyout Already Disabled".to_string())
    } else {
        Ok(format!("Unexpected EnableSnapAssistFlyout Value: {}", value))
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

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband]
"FavoritesRemovedChanges"=dword:00000003
"FavoritesResolve"=hex:31,03,00,00,4c,00,00,00,01,14,02,00,00,00,00,00,c0,00,\
  00,00,00,00,00,46,83,00,80,00,20,00,00,00,be,33,35,e7,d1,24,db,01,be,33,35,\
  e7,d1,24,db,01,25,b3,7a,4d,05,84,da,01,97,01,00,00,00,00,00,00,01,00,00,00,\
  00,00,00,00,00,00,00,00,00,00,00,00,a0,01,3a,00,1f,80,c8,27,34,1f,10,5c,10,\
  42,aa,03,2e,e4,52,87,d6,68,26,00,01,00,26,00,ef,be,12,00,00,00,85,35,2b,d7,\
  d1,24,db,01,9b,e4,33,e7,d1,24,db,01,ab,5a,34,e7,d1,24,db,01,14,00,56,00,31,\
  00,00,00,00,00,56,59,b9,b3,11,00,54,61,73,6b,42,61,72,00,40,00,09,00,04,00,\
  ef,be,56,59,b9,b3,56,59,b9,b3,2e,00,00,00,f2,69,01,00,00,00,04,00,00,00,00,\
  00,00,00,00,00,00,00,00,00,00,00,ef,80,fc,00,54,00,61,00,73,00,6b,00,42,00,\
  61,00,72,00,00,00,16,00,0e,01,32,00,97,01,00,00,81,58,c4,3a,20,00,46,49,4c,\
  45,45,58,7e,31,2e,4c,4e,4b,00,00,7c,00,09,00,04,00,ef,be,56,59,b9,b3,56,59,\
  b9,b3,2e,00,00,00,c3,6a,01,00,00,00,02,00,00,00,00,00,00,00,00,00,52,00,00,\
  00,00,00,db,dc,91,00,46,00,69,00,6c,00,65,00,20,00,45,00,78,00,70,00,6c,00,\
  6f,00,72,00,65,00,72,00,2e,00,6c,00,6e,00,6b,00,00,00,40,00,73,00,68,00,65,\
  00,6c,00,6c,00,33,00,32,00,2e,00,64,00,6c,00,6c,00,2c,00,2d,00,32,00,32,00,\
  30,00,36,00,37,00,00,00,1c,00,22,00,00,00,1e,00,ef,be,02,00,55,00,73,00,65,\
  00,72,00,50,00,69,00,6e,00,6e,00,65,00,64,00,00,00,1c,00,12,00,00,00,2b,00,\
  ef,be,7c,4c,37,e7,d1,24,db,01,1c,00,42,00,00,00,1d,00,ef,be,02,00,4d,00,69,\
  00,63,00,72,00,6f,00,73,00,6f,00,66,00,74,00,2e,00,57,00,69,00,6e,00,64,00,\
  6f,00,77,00,73,00,2e,00,45,00,78,00,70,00,6c,00,6f,00,72,00,65,00,72,00,00,\
  00,1c,00,00,00,9a,00,00,00,1c,00,00,00,01,00,00,00,1c,00,00,00,2d,00,00,00,\
  00,00,00,00,99,00,00,00,11,00,00,00,03,00,00,00,0e,76,ea,84,10,00,00,00,00,\
  43,3a,5c,55,73,65,72,73,5c,6d,65,6d,5c,41,70,70,44,61,74,61,5c,52,6f,61,6d,\
  69,6e,67,5c,4d,69,63,72,6f,73,6f,66,74,5c,49,6e,74,65,72,6e,65,74,20,45,78,\
  70,6c,6f,72,65,72,5c,51,75,69,63,6b,20,4c,61,75,6e,63,68,5c,55,73,65,72,20,\
  50,69,6e,6e,65,64,5c,54,61,73,6b,42,61,72,5c,46,69,6c,65,20,45,78,70,6c,6f,\
  72,65,72,2e,6c,6e,6b,00,00,60,00,00,00,03,00,00,a0,58,00,00,00,00,00,00,00,\
  64,65,73,6b,74,6f,70,2d,6e,76,6a,67,69,71,33,00,1e,48,b8,ac,e6,93,44,44,85,\
  d1,06,17,eb,52,3b,ea,cc,41,5d,b0,c4,90,ef,11,b9,08,00,0c,29,5b,06,9a,1e,48,\
  b8,ac,e6,93,44,44,85,d1,06,17,eb,52,3b,ea,cc,41,5d,b0,c4,90,ef,11,b9,08,00,\
  0c,29,5b,06,9a,45,00,00,00,09,00,00,a0,39,00,00,00,31,53,50,53,b1,16,6d,44,\
  ad,8d,70,48,a7,48,40,2e,a4,3d,78,8c,1d,00,00,00,68,00,00,00,00,48,00,00,00,\
  d4,d9,2d,27,b2,34,c5,4f,ad,3b,78,a5,c4,f6,71,2d,00,00,00,00,00,00,00,00,00,\
  00,00,00
"Favorites"=hex:00,a4,01,00,00,3a,00,1f,80,c8,27,34,1f,10,5c,10,42,aa,03,2e,e4,\
  52,87,d6,68,26,00,01,00,26,00,ef,be,12,00,00,00,85,35,2b,d7,d1,24,db,01,9b,\
  e4,33,e7,d1,24,db,01,ab,5a,34,e7,d1,24,db,01,14,00,56,00,31,00,00,00,00,00,\
  56,59,b9,b3,11,00,54,61,73,6b,42,61,72,00,40,00,09,00,04,00,ef,be,56,59,b9,\
  b3,56,59,b9,b3,2e,00,00,00,f2,69,01,00,00,00,04,00,00,00,00,00,00,00,00,00,\
  00,00,00,00,00,00,ef,80,fc,00,54,00,61,00,73,00,6b,00,42,00,61,00,72,00,00,\
  00,16,00,12,01,32,00,97,01,00,00,81,58,c4,3a,20,00,46,49,4c,45,45,58,7e,31,\
  2e,4c,4e,4b,00,00,7c,00,09,00,04,00,ef,be,56,59,b9,b3,56,59,b9,b3,2e,00,00,\
  00,c3,6a,01,00,00,00,02,00,00,00,00,00,00,00,00,00,52,00,00,00,00,00,db,dc,\
  91,00,46,00,69,00,6c,00,65,00,20,00,45,00,78,00,70,00,6c,00,6f,00,72,00,65,\
  00,72,00,2e,00,6c,00,6e,00,6b,00,00,00,40,00,73,00,68,00,65,00,6c,00,6c,00,\
  33,00,32,00,2e,00,64,00,6c,00,6c,00,2c,00,2d,00,32,00,32,00,30,00,36,00,37,\
  00,00,00,1c,00,12,00,00,00,2b,00,ef,be,7c,4c,37,e7,d1,24,db,01,1c,00,42,00,\
  00,00,1d,00,ef,be,02,00,4d,00,69,00,63,00,72,00,6f,00,73,00,6f,00,66,00,74,\
  00,2e,00,57,00,69,00,6e,00,64,00,6f,00,77,00,73,00,2e,00,45,00,78,00,70,00,\
  6c,00,6f,00,72,00,65,00,72,00,00,00,1c,00,26,00,00,00,1e,00,ef,be,02,00,53,\
  00,79,00,73,00,74,00,65,00,6d,00,50,00,69,00,6e,00,6e,00,65,00,64,00,00,00,\
  1c,00,00,00,ff
"FavoritesChanges"=dword:00000002
"FavoritesVersion"=dword:00000002

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband\AuxilliaryPins]
"MailPin"=dword:00000000
"TFLPin"=dword:00000000
"CopilotPWAPin"=dword:00000000

*/