use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceRegistryPropertyW, CM_DEVNODE_STATUS_FLAGS, CM_PROB,
    CR_SUCCESS, DIGCF_ALLCLASSES, DIGCF_PRESENT, SPDRP_DEVICEDESC, SPDRP_DRIVER, SP_DEVINFO_DATA,
};
use windows::Win32::Foundation::MAX_PATH;
use windows::core::{PWSTR, Result};

#[derive(Debug)]
pub struct DeviceProblem {
    _instance_id: String,
    _description: String,
    _problem_code: &'static str,
    _driver_desc: Option<String>,
}

pub fn enum_problem_devices() -> Result<Vec<DeviceProblem>> {
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

            if cm_result == CR_SUCCESS && problem_code != CM_PROB(0) {
                // Get device instance ID
                let mut instance_id = [0u16; MAX_PATH as usize];
                if SetupDiGetDeviceInstanceIdW(
                    hdevinfo,
                    &devinfo_data,
                    Some(&mut instance_id),
                    None,
                )
                .is_ok()
                {
                    let instance_id_str = PWSTR(&mut instance_id as *mut _).to_string()?;

                    // Get device description
                    let mut desc = [0u16; MAX_PATH as usize];
                    let desc_str = if SetupDiGetDeviceRegistryPropertyW(
                        hdevinfo,
                        &devinfo_data,
                        SPDRP_DEVICEDESC,
                        None,
                        Some(std::slice::from_raw_parts_mut(
                            desc.as_mut_ptr() as *mut u8,
                            desc.len() * 2,
                        )),
                        None,
                    )
                    .is_ok()
                    {
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
                        Some(std::slice::from_raw_parts_mut(
                            driver_desc.as_mut_ptr() as *mut u8,
                            driver_desc.len() * 2,
                        )),
                        None,
                    )
                    .is_ok()
                    {
                        PWSTR(&mut driver_desc as *mut _).to_string().ok()
                    } else {
                        None
                    };

                    log::info!(
                        "Problem device: {} ({}), Code: {}",
                        desc_str,
                        instance_id_str,
                        problem_code.0
                    );

                    devices.push(DeviceProblem {
                        _instance_id: instance_id_str,
                        _description: desc_str,
                        _problem_code: problem_code_to_description(problem_code.0),
                        _driver_desc: driver_str,
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

mod test {
    #[cfg(test)]
    mod tests {
        use crate::utilities::windows::drivers::enum_problem_devices;


        #[test]
        fn test_problem_code_to_description() {
           println!("Device problems: {:?}", enum_problem_devices());
        }
    }

}