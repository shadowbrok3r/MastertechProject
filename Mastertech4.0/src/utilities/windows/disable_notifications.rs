


use windows::Win32::System::Registry::{RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, REG_SAM_FLAGS, REG_VALUE_TYPE};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::core::PCWSTR;
use windows_core::w;
const PUSH_NOTIFICATIONS_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\PushNotifications");
const CONTENT_DELIVERY_MANAGER_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager");
const EXPLORER_ADVANCED_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");

// Helper function to read the registry value
fn read_registry_value(key: PCWSTR, value_name: &str) -> anyhow::Result<u32> {
    unsafe {
        if !does_registry_key_exist(key) {
            return Err(anyhow::anyhow!("Registry key does not exist: {key:?}//{value_name}"));
        }
        let mut h_key = HKEY_LOCAL_MACHINE; //HKEY_CURRENT_USER;
        let mut value: u32 = 0;
        // let key = .as_ptr();
        let result = RegOpenKeyExW(
            h_key,
            key,
            None,
            REG_SAM_FLAGS(0xF003F), // KEY_QUERY_VALUE
            &mut h_key,
        );

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("Failed to open registry key -> {result:?}"));
        }

        let value_name = PCWSTR::from_raw(value_name.encode_utf16().collect::<Vec<u16>>().as_ptr());
        
        // Prepare pointers for registry data and data type
        let mut value_type: REG_VALUE_TYPE = REG_VALUE_TYPE(0);
        let mut data_size: u32 = 4; // Size for u32

        // Read the value from the registry
        let result = RegGetValueW(
            h_key,
            PCWSTR::null(),
            value_name,
            REG_ROUTINE_FLAGS(0xFFFF), // No flags
            Some(&mut value_type), // Option<&mut REG_VALUE_TYPE>
            Some(&mut value as *mut u32 as *mut std::ffi::c_void), // Option<&mut u32> as a raw pointer to `c_void`
            Some(&mut data_size), // Option<&mut u32> for the size
        );


        RegCloseKey(h_key).ok()?;

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("Failed to read registry value -> {result:?}"));
        }

        Ok(value)
    }
}

// Helper function to check if the registry key exists
fn does_registry_key_exist(key: PCWSTR) -> bool {
    unsafe {
        let mut h_key = HKEY_LOCAL_MACHINE;
        let result = RegOpenKeyExW(
            h_key,
            key,
            None,
            REG_SAM_FLAGS(0xF003F), // KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE
            &mut h_key,
        );
        log::info!("\n\n{key:?} RESULT: {result:?}\n{h_key:?}");
        result == ERROR_SUCCESS
    }
}


// Function to check if push notifications are disabled
pub fn is_push_notifications_disabled() -> anyhow::Result<bool> {
    let value = read_registry_value(PUSH_NOTIFICATIONS_KEY, "Enabled")?;
    Ok(value == 0)
}

// Function to check if the Windows experience is disabled
pub fn is_windows_experience_disabled() -> anyhow::Result<bool> {
    let value = read_registry_value(CONTENT_DELIVERY_MANAGER_KEY, "SubscribedContent-338388444284599")?;
    Ok(value == 0)
}

// Function to check if tips and suggestions are disabled
pub fn is_tips_and_suggestions_disabled() -> anyhow::Result<bool> {
    let value = read_registry_value(EXPLORER_ADVANCED_KEY, "EnableBalloonTips")?;
    Ok(value == 0)
}


fn disable_push_notifications() -> anyhow::Result<(), anyhow::Error> {
    unsafe {
        let mut h_key = HKEY_CURRENT_USER;
        let result = RegOpenKeyExW(
            h_key,
            PUSH_NOTIFICATIONS_KEY,
            Some(0),
            REG_SAM_FLAGS(0x20006), // KEY_SET_VALUE
            &mut h_key,
        );
        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("disable_push_notifications -> {result:?}"));
        }

        // Disable push notifications by setting a value in the registry
        let value_name = w!("Enabled");
        let value = 0u32; // 0 means disabled
        RegSetValueExW(
            h_key, 
            value_name, 
            Some(0), 
            REG_VALUE_TYPE(4), 
            Some(&value.to_le_bytes())
        ).ok()?;

        RegCloseKey(h_key).ok()?;

        Ok(())
    }
}

fn disable_windows_experience() -> anyhow::Result<(), anyhow::Error> {
    unsafe {
        let mut h_key = HKEY_CURRENT_USER;
        let result = RegOpenKeyExW(
            h_key,
            CONTENT_DELIVERY_MANAGER_KEY,
            Some(0),
            REG_SAM_FLAGS(0x20006), // KEY_SET_VALUE
            &mut h_key,
        );
        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("disable_windows_experience -> {result:?}"));
        }

        // Disable the "Welcome Experience"
        let value_name = w!("SubscribedContent-338388444284599");
        let value = 0u32; // Disable
        RegSetValueExW(
            h_key, 
            value_name, 
            Some(0), 
            REG_VALUE_TYPE(4), 
            Some(&value.to_le_bytes())
        ).ok()?;

        RegCloseKey(h_key).ok()?;

        Ok(())
    }
}

fn disable_tips_and_suggestions() -> anyhow::Result<(), anyhow::Error> {
    unsafe {
        let mut h_key = HKEY_CURRENT_USER;
        let result = RegOpenKeyExW(
            h_key,
            EXPLORER_ADVANCED_KEY,
            Some(0),
            REG_SAM_FLAGS(0x20006), // KEY_SET_VALUE
            &mut h_key,
        );

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("disable_tips_and_suggestions -> {result:?}"));
        }

        // Disable tips and suggestions
        let value_name = PCWSTR::from_raw("EnableBalloonTips".encode_utf16().collect::<Vec<u16>>().as_ptr());
        let value = 0u32; // Disable
        RegSetValueExW(
            h_key, 
            value_name, 
            Some(0), 
            REG_VALUE_TYPE(4), 
            Some(&value.to_le_bytes())
        ).ok()?;

        RegCloseKey(h_key).ok()?;

        Ok(())
    }
}
