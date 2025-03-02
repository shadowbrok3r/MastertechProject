


// use windows::Win32::System::Registry::{RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, REG_SAM_FLAGS, REG_VALUE_TYPE};
// use windows::Win32::Foundation::ERROR_SUCCESS;
// use windows::core::PCWSTR;
// use windows_core::w;
// const PUSH_NOTIFICATIONS_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\PushNotifications");
// const CONTENT_DELIVERY_MANAGER_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager");
// const EXPLORER_ADVANCED_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");

// // Helper function to read the registry value
// fn read_registry_value(key: PCWSTR, value_name: &str) -> anyhow::Result<u32> {
//     unsafe {
//         if !does_registry_key_exist(key) {
//             return Err(anyhow::anyhow!("Registry key does not exist: {key:?}//{value_name}"));
//         }
//         let mut h_key = HKEY_LOCAL_MACHINE; //HKEY_CURRENT_USER;
//         let mut value: u32 = 0;
//         // let key = .as_ptr();
//         let result = RegOpenKeyExW(
//             h_key,
//             key,
//             None,
//             REG_SAM_FLAGS(0xF003F), // KEY_QUERY_VALUE
//             &mut h_key,
//         );

//         if result != ERROR_SUCCESS {
//             return Err(anyhow::anyhow!("Failed to open registry key -> {result:?}"));
//         }

//         let value_name = PCWSTR::from_raw(value_name.encode_utf16().collect::<Vec<u16>>().as_ptr());
        
//         // Prepare pointers for registry data and data type
//         let mut value_type: REG_VALUE_TYPE = REG_VALUE_TYPE(0);
//         let mut data_size: u32 = 4; // Size for u32

//         // Read the value from the registry
//         let result = RegGetValueW(
//             h_key,
//             PCWSTR::null(),
//             value_name,
//             REG_ROUTINE_FLAGS(0xFFFF), // No flags
//             Some(&mut value_type), // Option<&mut REG_VALUE_TYPE>
//             Some(&mut value as *mut u32 as *mut std::ffi::c_void), // Option<&mut u32> as a raw pointer to `c_void`
//             Some(&mut data_size), // Option<&mut u32> for the size
//         );


//         RegCloseKey(h_key).ok()?;

//         if result != ERROR_SUCCESS {
//             return Err(anyhow::anyhow!("Failed to read registry value -> {result:?}"));
//         }

//         Ok(value)
//     }
// }

// // Helper function to check if the registry key exists
// fn does_registry_key_exist(key: PCWSTR) -> bool {
//     unsafe {
//         let mut h_key = HKEY_LOCAL_MACHINE;
//         let result = RegOpenKeyExW(
//             h_key,
//             key,
//             None,
//             REG_SAM_FLAGS(0xF003F), // KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE
//             &mut h_key,
//         );
//         log::info!("\n\n{key:?} RESULT: {result:?}\n{h_key:?}");
//         result == ERROR_SUCCESS
//     }
// }


// // Function to check if push notifications are disabled
// pub fn is_push_notifications_disabled() -> anyhow::Result<bool> {
//     let value = read_registry_value(PUSH_NOTIFICATIONS_KEY, "Enabled")?;
//     Ok(value == 0)
// }

// // Function to check if the Windows experience is disabled
// pub fn is_windows_experience_disabled() -> anyhow::Result<bool> {
//     let value = read_registry_value(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-338388444284599")?;
//     Ok(value == 0)
// }

// // Function to check if tips and suggestions are disabled
// pub fn is_tips_and_suggestions_disabled() -> anyhow::Result<bool> {
//     let value = read_registry_value(EXPLORER_ADVANCED_KEY, "EnableBalloonTips")?;
//     Ok(value == 0)
// }


// fn disable_push_notifications() -> anyhow::Result<(), anyhow::Error> {
//     unsafe {
//         let mut h_key = HKEY_CURRENT_USER;
//         let result = RegOpenKeyExW(
//             h_key,
//             PUSH_NOTIFICATIONS_KEY,
//             Some(0),
//             REG_SAM_FLAGS(0x20006), // KEY_SET_VALUE
//             &mut h_key,
//         );
//         if result != ERROR_SUCCESS {
//             return Err(anyhow::anyhow!("disable_push_notifications -> {result:?}"));
//         }

//         // Disable push notifications by setting a value in the registry
//         let value_name = w!("Enabled");
//         let value = 0u32; // 0 means disabled
//         RegSetValueExW(
//             h_key, 
//             value_name, 
//             Some(0), 
//             REG_VALUE_TYPE(4), 
//             Some(&value.to_le_bytes())
//         ).ok()?;

//         RegCloseKey(h_key).ok()?;

//         Ok(())
//     }
// }

// fn disable_windows_experience() -> anyhow::Result<(), anyhow::Error> {
//     unsafe {
//         let mut h_key = HKEY_CURRENT_USER;
//         let result = RegOpenKeyExW(
//             h_key,
//             CONTENT_DELIVERY_MANAGER_KEY,
//             Some(0),
//             REG_SAM_FLAGS(0x20006), // KEY_SET_VALUE
//             &mut h_key,
//         );
//         if result != ERROR_SUCCESS {
//             return Err(anyhow::anyhow!("disable_windows_experience -> {result:?}"));
//         }

//         // Disable the "Welcome Experience"
//         let value_name = w!("SubscribedContent-338388444284599");
//         let value = 0u32; // Disable
//         RegSetValueExW(
//             h_key, 
//             value_name, 
//             Some(0), 
//             REG_VALUE_TYPE(4), 
//             Some(&value.to_le_bytes())
//         ).ok()?;

//         RegCloseKey(h_key).ok()?;

//         Ok(())
//     }
// }

// fn disable_tips_and_suggestions() -> anyhow::Result<(), anyhow::Error> {
//     unsafe {
//         let mut h_key = HKEY_CURRENT_USER;
//         let result = RegOpenKeyExW(
//             h_key,
//             EXPLORER_ADVANCED_KEY,
//             Some(0),
//             REG_SAM_FLAGS(0x20006), // KEY_SET_VALUE
//             &mut h_key,
//         );

//         if result != ERROR_SUCCESS {
//             return Err(anyhow::anyhow!("disable_tips_and_suggestions -> {result:?}"));
//         }

//         // Disable tips and suggestions
//         let value_name = PCWSTR::from_raw("EnableBalloonTips".encode_utf16().collect::<Vec<u16>>().as_ptr());
//         let value = 0u32; // Disable
//         RegSetValueExW(
//             h_key, 
//             value_name, 
//             Some(0), 
//             REG_VALUE_TYPE(4), 
//             Some(&value.to_le_bytes())
//         ).ok()?;

//         RegCloseKey(h_key).ok()?;

//         Ok(())
//     }
// }

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
                log::info!("CONTENT_DELIVERY_MANAGER_KEY for {} is DISABLED.", &val_name);
                x.push(format!("CONTENT_DELIVERY_MANAGER_KEY for {} is DISABLED.", &val_name));
            } else {
                log::info!("CONTENT_DELIVERY_MANAGER_KEY for {} is ENABLED.", &val_name);
                x.push(format!("CONTENT_DELIVERY_MANAGER_KEY for {} is ENABLED.", &val_name));
            }
        } 
    }
    // If we do not find a 0 value for any "SubscribedContent" value, assume it's enabled.
    Ok(x)
}

// Function to check the value of the Explorer Advanced registry key
pub fn check_explorer_advanced() -> Result<String> {
    let key = CURRENT_USER.open(EXPLORER_ADVANCED_KEY)?;
    // Iterate through all the values in the registry key
    for (val_name, val_data) in key.values()? {
        
        
        // Check if the value name contains "SubscribedContent" and ends with "Enabled"
        if val_name.eq("TaskbarAl") {
            log::info!("val_name: {val_name:?}\nval_data: {val_data:?}");
            // Extract the data and check if it's a u32
            if val_data == [1,0,0,0].into() {
                log::info!("TASKBAR IS CENTERED");
                return Ok("TASKBAR IS CENTERED".to_string());
            } else if val_data == [0,0,0,0].into() {
                log::info!("TASKBAR IS LEFT ALIGNED");
                return Ok("TASKBAR IS LEFT ALIGNED".to_string());
            }
        }
    }
    Ok("Explorer Balloon Tips are enabled.".to_string())
}
