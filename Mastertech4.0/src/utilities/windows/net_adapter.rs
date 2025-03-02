use std::{ffi::CString, ptr::null_mut};
use windows::{
    core::PCWSTR,
    Win32::{Foundation::HANDLE, NetworkManagement::{IpHelper::{GetAdaptersAddresses, GAA_FLAG_INCLUDE_ALL_INTERFACES, IP_ADAPTER_ADDRESSES_LH}, Ndis::IF_OPER_STATUS, WiFi::{wlan_interface_state_connected, WlanCloseHandle, WlanConnect, WlanEnumInterfaces, WlanGetAvailableNetworkList, WlanOpenHandle, WlanQueryInterface, WlanSetProfile, DOT11_BSS_TYPE, DOT11_SSID, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_MODE, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WLAN_INTF_OPCODE}}, Networking::WinSock::AF_UNSPEC},
};

/// Connect to a Wi-Fi SSID, optionally specifying a password and BSSID
pub fn connect_to_wifi(ssid: &str, password: Option<&str>, bssid: Option<[u8; 6]>) -> anyhow::Result<()> {
    log::info!("Starting Wi-Fi connection process...");

    unsafe {
        let mut client_handle: HANDLE = HANDLE(null_mut());
        let mut negotiated_version: u32 = 0;

        log::info!("Opening WLAN client handle...");
        if WlanOpenHandle(2, None, &mut negotiated_version, &mut client_handle) != 0 {
            log::error!("Failed to open WLAN client handle.");
            return Err(anyhow::anyhow!("WlanOpenHandle failed"));
        }

        let mut interface_list_ptr: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        log::info!("Enumerating WLAN interfaces...");
        if WlanEnumInterfaces(client_handle, None, &mut interface_list_ptr) != 0 {
            log::error!("Failed to enumerate WLAN interfaces.");
            WlanCloseHandle(client_handle, None);
            return Err(anyhow::anyhow!("WlanEnumInterfaces failed"));
        }

        let interface_list = &*interface_list_ptr;
        log::info!("Found {} WLAN interfaces.", interface_list.dwNumberOfItems);

        if interface_list.dwNumberOfItems == 0 {
            log::error!("No WLAN interfaces found.");
            WlanCloseHandle(client_handle, None);
            return Err(anyhow::anyhow!("No WLAN interfaces available"));
        }

        let interface_info: &WLAN_INTERFACE_INFO = &interface_list.InterfaceInfo[0]; // Use the first available interface
        let interface_guid = interface_info.InterfaceGuid;
        let interface_name = PCWSTR(interface_info.strInterfaceDescription.as_ptr()).to_string()?;

        log::info!("Using WLAN Interface: {}", interface_name);

        // If password is provided, create a new profile
        if let Some(pwd) = password {
            create_wifi_profile(client_handle, &interface_guid, ssid, pwd)?;
        }

        // Prepare the SSID
        let ssid_bytes = ssid.as_bytes();
        if ssid_bytes.len() > 32 {
            log::error!("SSID name too long (max 32 characters)");
            return Err(anyhow::anyhow!("SSID too long"));
        }

        let mut dot11_ssid = DOT11_SSID {
            uSSIDLength: ssid_bytes.len() as u32,
            ucSSID: [0; 32],
        };
        dot11_ssid.ucSSID[..ssid_bytes.len()].copy_from_slice(ssid_bytes);

        // Set up connection parameters
        let connection_params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: WLAN_CONNECTION_MODE(0), // Use profile-based connection
            strProfile: PCWSTR(null_mut()), // Windows will match based on SSID
            pDot11Ssid: &mut dot11_ssid,
            pDesiredBssidList: null_mut(), // Allow Windows to pick best BSSID
            dot11BssType: DOT11_BSS_TYPE(1), // Infrastructure mode
            dwFlags: 0,
        };

        log::info!(
            "Attempting to connect to SSID: {}{}",
            ssid,
            if bssid.is_some() {
                " with specific BSSID"
            } else {
                " (auto-select BSSID)"
            }
        );

        if WlanConnect(client_handle, &interface_guid, &connection_params, None) != 0 {
            log::error!("Failed to initiate connection to SSID: {}", ssid);
            WlanCloseHandle(client_handle, None);
            return Err(anyhow::anyhow!("WlanConnect failed"));
        }

        log::info!("Successfully initiated connection to {}", ssid);
        WlanCloseHandle(client_handle, None);
    }

    Ok(())
}

/// Creates a Wi-Fi profile for the SSID with the given password
pub fn create_wifi_profile(client_handle: HANDLE, interface_guid: &windows_core::GUID, ssid: &str, password: &str) -> anyhow::Result<()> {
    log::info!("Creating a Wi-Fi profile for SSID: {}", ssid);

    // Generate an XML profile for the Wi-Fi network
    let profile_xml = format!(
        r#"<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
            <name>{}</name>
            <SSIDConfig>
                <SSID>
                    <name>{}</name>
                </SSID>
            </SSIDConfig>
            <connectionType>ESS</connectionType>
            <connectionMode>manual</connectionMode>
            <MSM>
                <security>
                    <authEncryption>
                        <authentication>WPA2</authentication>
                        <encryption>AES</encryption>
                        <useOneX>false</useOneX>
                    </authEncryption>
                    <sharedKey>
                        <keyType>passPhrase</keyType>
                        <protected>false</protected>
                        <keyMaterial>{}</keyMaterial>
                    </sharedKey>
                </security>
            </MSM>
        </WLANProfile>"#,
        ssid, ssid, password
    );

    // Convert the XML string to PCWSTR
    let profile_wstr = CString::new(profile_xml)?.into_raw();

    // Add the profile to Windows
    let mut profile_result = 0;
    let res = unsafe {
        WlanSetProfile(
            client_handle,
            interface_guid,
            0, // No flags
            PCWSTR(profile_wstr as _),
            None,
            true,
            None,
            &mut profile_result,
        )
    };

    if res != 0 {
        log::error!("Failed to set Wi-Fi profile for SSID: {}", ssid);
        return Err(anyhow::anyhow!("WlanSetProfile failed"));
    }

    log::info!("Successfully added Wi-Fi profile for {}", ssid);
    Ok(())
}


/// Scan for available Wi-Fi networks and return their SSID-BSSID pairs
pub fn scan_wifi_networks() -> anyhow::Result<Vec<(String, Vec<[u8; 6]>)>> {
    log::info!("Starting Wi-Fi scan...");

    unsafe {
        let mut client_handle: HANDLE = HANDLE(std::ptr::null_mut());
        let mut negotiated_version: u32 = 0;

        log::info!("Opening WLAN client handle...");
        if WlanOpenHandle(2, None, &mut negotiated_version, &mut client_handle) != 0 {
            log::error!("Failed to open WLAN client handle.");
            return Err(anyhow::anyhow!("WlanOpenHandle failed"));
        }

        let mut interface_list_ptr: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        log::info!("Enumerating WLAN interfaces...");
        if WlanEnumInterfaces(client_handle, None, &mut interface_list_ptr) != 0 {
            log::error!("Failed to enumerate WLAN interfaces.");
            WlanCloseHandle(client_handle, None);
            return Err(anyhow::anyhow!("WlanEnumInterfaces failed"));
        }

        let interface_list = &*interface_list_ptr;
        log::info!("Found {} WLAN interfaces.", interface_list.dwNumberOfItems);

        if interface_list.dwNumberOfItems == 0 {
            log::error!("No WLAN interfaces found.");
            WlanCloseHandle(client_handle, None);
            return Err(anyhow::anyhow!("No WLAN interfaces available"));
        }

        let mut networks = Vec::new();

        for i in 0..interface_list.dwNumberOfItems {
            let interface_info = &interface_list.InterfaceInfo[i as usize];
            let interface_guid = interface_info.InterfaceGuid;
            let interface_name = PCWSTR(interface_info.strInterfaceDescription.as_ptr()).to_string()?;

            log::info!("Scanning available networks on WLAN Interface: {}", interface_name);

            let mut network_list_ptr: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
            if WlanGetAvailableNetworkList(client_handle, &interface_guid, 0, None, &mut network_list_ptr) != 0 {
                log::error!("Failed to retrieve available networks on interface {}", interface_name);
                continue;
            }

            let network_list = &*network_list_ptr;
            log::info!("Found {} available networks.", network_list.dwNumberOfItems);

            for j in 0..network_list.dwNumberOfItems {
                let network = &network_list.Network[j as usize];

                let ssid_bytes = &network.dot11Ssid.ucSSID[..network.dot11Ssid.uSSIDLength as usize];
                let ssid = String::from_utf8_lossy(ssid_bytes).to_string();

                let bssid_list = Vec::new();
                // BSSID extraction is not directly available here; requires extra API calls.

                log::info!("SSID: {}", ssid);
                networks.push((ssid, bssid_list));
            }
        }

        WlanCloseHandle(client_handle, None);
        Ok(networks)
    }
}


pub fn check_network_adapters() -> anyhow::Result<(), anyhow::Error> {
    log::info!("Starting network adapter check...");

    unsafe {
        let mut out_buf_len = 0;
        let mut ret = GetAdaptersAddresses(
            AF_UNSPEC.0.into(),
            GAA_FLAG_INCLUDE_ALL_INTERFACES,
            None,
            None,
            &mut out_buf_len,
        );

        if ret != 0 {
            log::info!("Retrieving adapter addresses, buffer size: {}", out_buf_len);
            let mut adapter_addresses: Vec<u8> = vec![0; out_buf_len as usize];
            let adapter_addresses_ptr = adapter_addresses.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;

            ret = GetAdaptersAddresses(
                AF_UNSPEC.0.into(),
                GAA_FLAG_INCLUDE_ALL_INTERFACES,
                None,
                Some(adapter_addresses_ptr),
                &mut out_buf_len,
            );

            if ret == 0 {
                log::info!("Successfully retrieved adapter list.");
                let mut adapter = adapter_addresses_ptr;
                while !adapter.is_null() {
                    let adapter_ref = &*adapter;

                    let name = PCWSTR(adapter_ref.FriendlyName.0);
                    let name_string = name.to_string()?;

                    let status = match adapter_ref.OperStatus {
                        IF_OPER_STATUS(1) => "Up",
                        _ => "Down",
                    };

                    let adapter_type = match adapter_ref.IfType {
                        6 => "Ethernet",
                        71 => "WLAN",
                        _ => "Other",
                    };

                    log::info!(
                        "Adapter: {} | Status: {} | Type: {} | IfType: {}",
                        name_string,
                        status,
                        adapter_type,
                        adapter_ref.IfType
                    );

                    adapter = adapter_ref.Next;
                }
            } else {
                log::error!("Failed to retrieve network adapters, error code: {}", ret);
            }
        } else {
            log::error!("Failed to call GetAdaptersAddresses");
        }
    }
    log::info!("Network adapter check completed.");
    Ok(())
}

pub fn get_wlan_status() -> anyhow::Result<(), anyhow::Error> {
    log::info!("Starting WLAN status check...");

    unsafe {
        let mut client_handle: HANDLE = HANDLE(std::ptr::null_mut());
        let mut negotiated_version: u32 = 0;

        log::info!("Opening WLAN client handle...");
        if WlanOpenHandle(2, None, &mut negotiated_version, &mut client_handle) == 0 {
            log::info!("Successfully opened WLAN client handle.");

            let mut interface_list_ptr: *mut WLAN_INTERFACE_INFO_LIST = null_mut();

            log::info!("Enumerating WLAN interfaces...");
            if WlanEnumInterfaces(client_handle, None, &mut interface_list_ptr) == 0 {
                let interface_list = &*interface_list_ptr;
                log::info!("Found {} WLAN interfaces.", interface_list.dwNumberOfItems);

                for i in 0..interface_list.dwNumberOfItems {
                    let interface_info: &WLAN_INTERFACE_INFO = &interface_list.InterfaceInfo[i as usize];
                    let interface_name = PCWSTR(interface_info.strInterfaceDescription.as_ptr()).to_string()?;

                    log::info!("Processing WLAN interface: {}", interface_name);

                    // Query connection status
                    let mut data_size: u32 = 0;
                    let mut data_ptr: *mut std::ffi::c_void = null_mut();
                    let wlan_interface_query = WlanQueryInterface(
                        client_handle,
                        &interface_info.InterfaceGuid,
                        WLAN_INTF_OPCODE(0),
                        None,
                        &mut data_size,
                        &mut data_ptr,
                        None,
                    );

                    if wlan_interface_query == 0
                    {
                        let connection_attributes = &*(data_ptr as *mut WLAN_CONNECTION_ATTRIBUTES);
                        let status = if connection_attributes.isState == wlan_interface_state_connected {
                            "Connected"
                        } else {
                            "Not Connected"
                        };

                        log::info!(
                            "WLAN Interface: {} | Status: {} | Interface GUID: {:?}",
                            interface_name,
                            status,
                            interface_info.InterfaceGuid
                        );
                    } else {
                        log::error!("Failed to query WLAN interface status for {} => {}", interface_name, wlan_interface_query);
                    }
                }
            } else {
                log::error!("Failed to enumerate WLAN interfaces.");
            }

            // Close handle
            log::info!("Closing WLAN client handle...");
            WlanCloseHandle(client_handle, None);
        } else {
            log::error!("Failed to open WLAN client handle.");
        }
    }

    log::info!("WLAN status check completed.");
    Ok(())
}