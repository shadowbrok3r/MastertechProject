use windows_registry::*;

const UNINSTALL_KEY_64: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_KEY_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const PUSH_NOTIFICATIONS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
const CONTENT_DELIVERY_MANAGER_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const EXPLORER_ADVANCED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

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

// Function to check the value of the PushNotifications registry key
pub fn check_push_notifications() -> Result<String> {
    let key = CURRENT_USER.open(PUSH_NOTIFICATIONS_KEY)?;
    let value = key.get_u32("ToastEnabled")?;
    if value == 1 {
        match CURRENT_USER.set_u32("ToastEnabled", 0x000) {
            Ok(_) => return Ok("DISABLED Push Notifications".to_string()),
            Err(e) => return Ok(format!("Push Notifications are ENABLED, but there was an error disabling: {e:?}")),
        }
    } else {
        return Ok("Push Notifications are DISABLED.".to_string());
    }
}

// SubscribedContent-88000326Enabled // SubscribedContent-310093Enabled // SubscribedContent-338389Enabled
pub fn check_content_delivery_manager() -> Result<Vec<String>> {
    let key = CURRENT_USER.open(CONTENT_DELIVERY_MANAGER_KEY)?;
    
    let mut x = Vec::new();
    // Iterate through all the values in the registry key
    for (val_name, val_data) in key.values()? {
        // Check if the value name contains "SubscribedContent"
        if val_name.contains("SubscribedContent") {
            log::info!("check_content_delivery_manager => \nval_name: {val_name:?}\nval_data: {val_data:?}");
            let value = key.get_u32(val_name.clone())?;
            if value == 0 {
                log::info!("Key {} is DISABLED.", &val_name);
                x.push(format!("Key {} is DISABLED.", &val_name));
            } else {
                match CURRENT_USER.set_u32("SubscribedContent", 0x000) {
                    Ok(_) => x.push(format!("DISABLED SubscribedContent")),
                    Err(e) => x.push(format!("Push Notifications are ENABLED, but there was an error disabling: {e:?}")),
                }
                log::info!("Key {} is ENABLED.", &val_name);
                x.push(format!("Key {} is ENABLED.", &val_name));
            }
        } 
    }
    // If we do not find a 0 value for any "SubscribedContent" value, assume it's enabled.
    Ok(x)
}

// Function to check the value of the Explorer Advanced registry key
pub fn check_explorer_advanced() -> Result<String> {
    let key = CURRENT_USER.open(EXPLORER_ADVANCED_KEY)?;
    let mut return_key = String::new();
    // Iterate through all the values in the registry key
    for (val_name, val_data) in key.values()? {
        // Check if the value name contains "SubscribedContent" and ends with "Enabled"
        if val_name.eq("TaskbarAl") {
            log::info!("val_name: {val_name:?}\nval_data: {val_data:?}");
            // Extract the data and check if it's a u32
            if val_data == [1,0,0,0].into() {
                return_key = "TaskBar is Centered".to_string();
            } else if val_data == [0,0,0,0].into() {
                return_key = "TaskBar is Left Aligned".to_string();
            }
        } else {
            return_key = "Could not find 'TaskbarAl' key".to_string();
        }
    }
    Ok(return_key)
}
