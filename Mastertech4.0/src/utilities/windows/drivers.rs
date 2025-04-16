use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceRegistryPropertyW, CM_DEVNODE_STATUS_FLAGS, CM_PROB, CR_SUCCESS, DIGCF_ALLCLASSES, DIGCF_PRESENT, SPDRP_DEVICEDESC, SPDRP_DRIVER, SP_DEVINFO_DATA
};
use windows::Win32::Foundation::MAX_PATH;
use windows::core::{PWSTR, Result};

#[derive(Debug)]
struct DeviceProblem {
    instance_id: String,
    description: String,
    problem_code: u32,
    driver_desc: Option<String>,
}

fn enum_problem_devices() -> Result<Vec<DeviceProblem>> {
    unsafe {
        // Create device information set for all present devices
        let hdevinfo = SetupDiGetClassDevsW(None, None, None, DIGCF_ALLCLASSES | DIGCF_PRESENT)?;
        let mut devices = Vec::new();
        let mut index = 0;
        let mut devinfo_data = SP_DEVINFO_DATA::default();
        devinfo_data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;

        // Enumerate devices
        while SetupDiEnumDeviceInfo(hdevinfo, index, &mut devinfo_data).is_ok() {
            let mut status = CM_DEVNODE_STATUS_FLAGS(0);
            let mut problem_code = CM_PROB(0);
            let cm_result = CM_Get_DevNode_Status(
                &mut status,
                &mut problem_code,
                devinfo_data.DevInst,
                0,
            );

            if cm_result == CR_SUCCESS && problem_code != 0 {
                // Get device instance ID
                let mut instance_id = [0u16; MAX_PATH as usize];
                if SetupDiGetDeviceInstanceIdW(
                    hdevinfo,
                    &devinfo_data,
                    Some(&mut instance_id),
                    None,
                ).is_ok() {
                    let instance_id_str = PWSTR(&mut instance_id as *mut _).to_string()?;

                    // Get device description
                    let mut desc = [0u16; MAX_PATH as usize];
                    let desc_str = if SetupDiGetDeviceRegistryPropertyW(
                        hdevinfo,
                        &devinfo_data,
                        SPDRP_DEVICEDESC,
                        None,
                        Some(&mut desc as *mut _ as *mut u8),
                        Some((MAX_PATH * 2) as u32),
                    ).is_ok() {
                        PWSTR(&mut desc as *mut _).to_string().unwrap_or_default()
                    } else {
                        String::from("Unknown")
                    };

                    // Get driver description (optional)
                    let mut driver_desc = [0u16; MAX_PATH as usize];
                    let driver_str = if SetupDiGetDeviceRegistryPropertyW(
                        hdevinfo,
                        &devinfo_data,
                        SPDRP_DRIVER,
                        None,
                        &mut driver_desc as *mut _ as *mut u8,
                        (MAX_PATH * 2) as u32,
                    ).is_ok() {
                        PWSTR(&mut driver_desc as *mut _).to_string().ok()
                    } else {
                        None
                    };

                    log::info!(
                        "Problem device: {} ({}), Code: {:?}",
                        desc_str,
                        instance_id_str,
                        problem_code
                    );

                    devices.push(DeviceProblem {
                        instance_id: instance_id_str,
                        description: desc_str,
                        problem_code: problem_code.0,
                        driver_desc,
                    });
                }
            }

            index += 1;
        }

        // Clean up
        SetupDiDestroyDeviceInfoList(hdevinfo)?;
        Ok(devices)
    }
}

fn problem_code_to_description(code: u32) -> &'static str {
    match code {
        28 => "The drivers for this device are not installed.",
        22 => "This device is disabled.",
        29 => "The device is disabled by hardware.",
        1 => "This device is not configured correctly.",
        10 => "The device cannot start.",
        _ => "Unknown problem.",
    }
}