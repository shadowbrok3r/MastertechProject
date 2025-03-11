use windows_registry::*;

const UNINSTALL_KEY_64: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_KEY_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const PUSH_NOTIFICATIONS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
const CONTENT_DELIVERY_MANAGER_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const EXPLORER_ADVANCED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const USER_NOTIFS: &str = r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; // ScoobeSystemSettingEnabled
// Function to get all installed programs from both 64-bit and 32-bit registry keys
pub fn get_installed_program_names() -> Result<Vec<String>> {
    let mut program_names = Vec::new();

    // Check in HKEY_LOCAL_MACHINE for 64-bit programs
    let programs_64 = get_installed_programs_from_registry(LOCAL_MACHINE, UNINSTALL_KEY_64)?;
    program_names.extend(programs_64);

    // Check in HKEY_LOCAL_MACHINE for 32-bit programs
    let programs_32 = get_installed_programs_from_registry(LOCAL_MACHINE, UNINSTALL_KEY_32)?;
    program_names.extend(programs_32);

    // Check in CURRENT_USER for 64-bit programs
    let programs_64_current_user = get_installed_programs_from_registry(CURRENT_USER, UNINSTALL_KEY_64)?;
    program_names.extend(programs_64_current_user);

    // Check in CURRENT_USER for 32-bit programs
    let programs_32_current_user = get_installed_programs_from_registry(CURRENT_USER, UNINSTALL_KEY_32)?;
    program_names.extend(programs_32_current_user);

    Ok(program_names)
}

// Helper function to get installed programs from a specific registry key (handles both HKLM and CURRENT_USER)
pub fn get_installed_programs_from_registry(root_key: &Key, reg_path: &str) -> Result<Vec<String>> {
    // Open the registry key
    let key = root_key.open(reg_path)?;
    // log::info!("key_name: {:?}", key.keys()?.collect::<Vec<String>>());
    let mut program_names = Vec::new();

    for key_name in key.keys()? {
        log::info!("key_name: {key_name:?}");
        let new_key = root_key.open(format!("{reg_path}\\{key_name}"))?;
        if let Ok(name) = new_key.get_string("DisplayName") {
            program_names.push(name);
        }
    }

    Ok(program_names)
}

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
    let key = CURRENT_USER.options().read().write().open(EXPLORER_ADVANCED_KEY)?;
    let mut return_key = String::new();
    let value = key.get_u32("TaskbarAl")?;
    log::info!("Value: {value}");
    if value == 1 {
        match key.set_u32("TaskbarAl", 0x000) {
            Ok(_) => return_key = format!("Left Centered TaskBar"),
            Err(e) => return_key = format!("Error Left Centering TaskBar: {e:?}"),
        }
    } else if value == 0 {
        return_key = "TaskBar is Left Aligned".to_string();
    }
    Ok(return_key)
}
