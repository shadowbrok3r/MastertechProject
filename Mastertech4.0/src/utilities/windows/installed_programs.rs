use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, REG_SAM_FLAGS, REG_VALUE_TYPE};
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
// use windows_registry::*;

const UNINSTALL_KEY_64: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_KEY_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";


// Helper function to read the "DisplayName" of an installed program from the registry
fn get_installed_programs_from_registry(key: &str) -> anyhow::Result<Vec<String>> {
    unsafe {
        let mut h_key = HKEY_LOCAL_MACHINE;
        let mut program_names = Vec::new();

        // Open the registry key for installed programs
        let result = RegOpenKeyExW(
            h_key,
            PCWSTR::from_raw(key.encode_utf16().collect::<Vec<u16>>().as_ptr()),
            Some(0),
            REG_SAM_FLAGS(0x20019), // KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE
            &mut h_key,
        );

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("Failed to open registry key -> {result:?}"));
        }

        let mut index = 0;
        loop {
            let mut subkey_name: Vec<u16> = vec![0; 256]; // Buffer to hold the subkey name
            let mut subkey_len = subkey_name.len() as u32; // Maximum length of the subkey name
            let result = RegEnumKeyExW(
                h_key,
                index,
                Some(windows_core::PWSTR(subkey_name.as_mut_ptr())),
                &mut subkey_len,
                Some(std::ptr::null_mut()), // Reserved for system use
                Some(windows_core::PWSTR(std::ptr::null_mut())), // No class name needed
                Some(std::ptr::null_mut()), // No filetime needed
                Some(std::ptr::null_mut()), // No last write time needed
            );

            if result != ERROR_SUCCESS {
                break;
            }

            // Convert the subkey name (program) to a string
            let program_name = String::from_utf16_lossy(&subkey_name[0..(subkey_len as usize)]);

            // Now read the "DisplayName" value for the program (subkey)
            let mut program_display_name: Vec<u16> = vec![0; 256];
            let mut value_type = REG_VALUE_TYPE(0);
            let mut data_size = program_display_name.len() as u32;
            
            let result = RegQueryValueExW(
                h_key,
                PCWSTR::from_raw("DisplayName".encode_utf16().collect::<Vec<u16>>().as_ptr()),
                None,
                Some(&mut value_type),
                Some(program_display_name.as_mut_ptr() as *mut u8),
                Some(&mut data_size),
            );

            if result == ERROR_SUCCESS {
                // Convert the DisplayName to a string and add it to the list
                let display_name = String::from_utf16_lossy(&program_display_name[0..(data_size as usize)]);
                program_names.push(display_name);
            } else {
                // If DisplayName is missing, add the subkey name (program name) as a fallback
                program_names.push(program_name);
            }

            index += 1;
        }

        RegCloseKey(h_key).ok()?;
        Ok(program_names)
    }
}

/// Function to get all installed programs from both 64-bit and 32-bit registry keys
pub fn get_installed_program_names() -> anyhow::Result<Vec<String>> {
    let mut program_names = Vec::new();

    // Get programs from the 64-bit registry key
    let programs_64 = get_installed_programs_from_registry(UNINSTALL_KEY_64)?;
    program_names.extend(programs_64);

    // Get programs from the 32-bit registry key
    let programs_32 = get_installed_programs_from_registry(UNINSTALL_KEY_32)?;
    program_names.extend(programs_32);

    Ok(program_names)
}



// // Function to get all installed programs from both 64-bit and 32-bit registry keys
// pub fn get_installed_program_names() -> Result<Vec<String>> {
//     let mut program_names = Vec::new();

//     // Check in HKEY_LOCAL_MACHINE for 64-bit programs
//     let programs_64 = get_installed_programs_from_registry(LOCAL_MACHINE, UNINSTALL_KEY_64)?;
//     program_names.extend(programs_64);

//     // Check in HKEY_LOCAL_MACHINE for 32-bit programs
//     let programs_32 = get_installed_programs_from_registry(LOCAL_MACHINE, UNINSTALL_KEY_32)?;
//     program_names.extend(programs_32);

//     // Check in CURRENT_USER for 64-bit programs
//     let programs_64_current_user = get_installed_programs_from_registry(CURRENT_USER, UNINSTALL_KEY_64)?;
//     program_names.extend(programs_64_current_user);

//     // Check in CURRENT_USER for 32-bit programs
//     let programs_32_current_user = get_installed_programs_from_registry(CURRENT_USER, UNINSTALL_KEY_32)?;
//     program_names.extend(programs_32_current_user);

//     Ok(program_names)
// }

// // Helper function to get installed programs from a specific registry key (handles both HKLM and CURRENT_USER)
// pub fn get_installed_programs_from_registry(root_key: &Key, reg_path: &str) -> Result<Vec<String>> {
//     // Open the registry key
//     let key = root_key.open(reg_path)?;
//     // log::info!("key_name: {:?}", key.keys()?.collect::<Vec<String>>());
//     let mut program_names = Vec::new();

//     for key_name in key.keys()? {
//         log::info!("key_name: {key_name:?}");
//         let new_key = root_key.open(format!("{reg_path}\\{key_name}"))?;
//         if let Ok(name) = new_key.get_string("DisplayName") {
//             program_names.push(name);
//         }
//     }

//     Ok(program_names)
// }
