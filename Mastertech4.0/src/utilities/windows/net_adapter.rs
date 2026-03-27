use windows::{
    core::PCWSTR,
    Win32::{Foundation::HANDLE, NetworkManagement::{IpHelper::{GetAdaptersAddresses, GAA_FLAG_INCLUDE_ALL_INTERFACES, IP_ADAPTER_ADDRESSES_LH}, Ndis::IF_OPER_STATUS, WiFi::{wlan_interface_state_connected, WlanCloseHandle, WlanConnect, WlanEnumInterfaces, WlanGetAvailableNetworkList, WlanOpenHandle, WlanQueryInterface, WlanReasonCodeToString, WlanSetProfile, DOT11_BSS_TYPE, DOT11_SSID, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_MODE, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WLAN_INTF_OPCODE}}, Networking::WinSock::AF_UNSPEC},
};
use windows_core::w;
use std::ptr::null_mut;

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

        // Use the profile name explicitly
        let profile_name = w!("PCL5");

        let connection_params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: WLAN_CONNECTION_MODE(0),
            strProfile: profile_name,
            pDot11Ssid: &mut dot11_ssid,
            pDesiredBssidList: null_mut(),
            dot11BssType: DOT11_BSS_TYPE(1),
            dwFlags: 0,
        };

        log::info!(
            "Attempting to connect to SSID: {}{}",
            ssid,
            if bssid.is_some() { " with specific BSSID" } else { " (auto-select BSSID)" }
        );

        let connect_result = WlanConnect(client_handle, &interface_guid, &connection_params, None);
        if connect_result != 0 {
            let error_desc = match connect_result {
                87 => "ERROR_INVALID_PARAMETER",
                5 => "ERROR_ACCESS_DENIED",
                1169 => "ERROR_NO_MATCH",
                _ => "Unknown error",
            };
            log::error!(
                "Failed to initiate connection to SSID: {} (Error code: {} - {})",
                ssid,
                connect_result,
                error_desc
            );
            return Err(anyhow::anyhow!("WlanConnect failed with error code: {} - {}", connect_result, error_desc));
        }

        log::info!("Successfully initiated connection to {}", ssid);
    }

    Ok(())
}

/// Creates a Wi-Fi profile for the SSID with the given password
pub fn create_wifi_profile(client_handle: HANDLE, interface_guid: &windows_core::GUID, ssid: &str, _password: &str) -> anyhow::Result<()> {
    log::info!("Creating a Wi-Fi profile for SSID: {}", ssid);
    // WPA2-PSK profile with hex SSID
    let profile_xml = w!(r#"<?xml version="1.0"?>
    <WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
        <name>PCL5</name>
        <SSIDConfig>
            <SSID>
                <hex>50436C6170746F7073352E30</hex>
                <name>PCL5</name>
            </SSID>
        </SSIDConfig>
        <connectionType>ESS</connectionType>
        <connectionMode>auto</connectionMode>
        <MSM>
            <security>
                <authEncryption>
                    <authentication>WPA2PSK</authentication>
                    <encryption>AES</encryption>
                    <useOneX>false</useOneX>
                </authEncryption>
                <sharedKey>
                    <keyType>passPhrase</keyType>
                    <protected>false</protected>
                    <keyMaterial>bestburger</keyMaterial>
                </sharedKey>
            </security>
        </MSM>
    </WLANProfile>"#);

    // Add the profile
    let mut profile_result = 0;
    let res = unsafe {
        WlanSetProfile(
            client_handle,
            interface_guid,
            0, // All-user profile
            profile_xml,
            PCWSTR::null(), // Default security descriptor
            true,
            None,
            &mut profile_result,
        )
    };
    
    // Reclaim the pointer to avoid memory leak
    // unsafe { let _ = CString::from_raw(profile_wstr); }

    if res != 0 {
        // Allocate a buffer for the reason string (256 WCHARs = 512 bytes)
        let buffer_size = 256;
        let mut reason_buffer: Vec<u16> = vec![0; buffer_size as usize];
        
        // Get reason string using profile_result (if populated)
        let reason_res = unsafe {
            WlanReasonCodeToString(
                profile_result,               // Error code (e.g., 1206)
                &mut reason_buffer,      // Buffer size in WCHARs
                None,   // Mutable buffer
            )
        };

        let reason_string = if reason_res == 0 && profile_result != 0 {
            let null_pos = reason_buffer.iter().position(|&x| x == 0).unwrap_or(buffer_size as usize);
            String::from_utf16(&reason_buffer[..null_pos]).unwrap_or_else(|_| "Failed to decode reason string".to_string())
        } else {
            format!("No specific reason available (WlanReasonCodeToString error: {}, profile_result: {})", reason_res, profile_result)
        };

        // Map system error code for clarity
        let error_desc = match res {
            87 => "ERROR_INVALID_PARAMETER",
            5 => "ERROR_ACCESS_DENIED",
            1206 => "ERROR_BAD_PROFILE",
            _ => "Unknown error",
        };

        log::error!(
            "Failed to set Wi-Fi profile for SSID: {}\nProfile: {:?}\nError code: {} ({})\nProfile result: {}\nReason: {}",
            ssid,
            profile_xml,
            res,
            error_desc,
            profile_result,
            reason_string
        );
        return Err(anyhow::anyhow!("WlanSetProfile failed: {res:?}\n{profile_result:?}"));
    }

    log::info!("Successfully added Wi-Fi profile for {ssid}\nProfile: {profile_xml:?}");
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
            // SAFETY: InterfaceInfo is a flexible array in C, access via pointer arithmetic
            let interface_info = &*interface_list.InterfaceInfo.as_ptr().add(i as usize);
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
                // SAFETY: Network is a flexible array in C, access via pointer arithmetic
                let network = &*network_list.Network.as_ptr().add(j as usize);

                let ssid_bytes = &network.dot11Ssid.ucSSID[..network.dot11Ssid.uSSIDLength as usize];
                let ssid = String::from_utf8_lossy(ssid_bytes).to_string();

                let bssid_list = Vec::new(); // Still needs BSSID implementation
                log::info!("SSID: {}", ssid);
                networks.push((ssid, bssid_list));
            }
        }

        WlanCloseHandle(client_handle, None);
        Ok(networks)
    }
}

pub fn check_network_adapters() -> anyhow::Result<Vec<String>, anyhow::Error> {
    log::info!("Starting network adapter check...");
    let mut network_adapters = Vec::new();

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

                    if adapter_type.eq("WLAN") || adapter_type.eq("Ethernet") {
                        network_adapters.push(format!(
                            "Adapter: {} | Status: {} | Type: {} | IfType: {}",
                            name_string,
                            status,
                            adapter_type,
                            adapter_ref.IfType
                        ));
                    }
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
    Ok(network_adapters)
}

pub fn get_wlan_status() -> anyhow::Result<(), anyhow::Error> {
    if is_wlan_connected() {
        log::info!("WLAN status: Connected");
        Ok(())
    } else {
        Err(anyhow::anyhow!("WLAN is not connected"))
    }
}

/// Returns true if any WLAN interface reports a connected state.
pub fn is_wlan_connected() -> bool {
    unsafe {
        let mut client_handle: HANDLE = HANDLE(std::ptr::null_mut());
        let mut negotiated_version: u32 = 0;

        if WlanOpenHandle(2, None, &mut negotiated_version, &mut client_handle) != 0 {
            return false;
        }

        let mut interface_list_ptr: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        if WlanEnumInterfaces(client_handle, None, &mut interface_list_ptr) != 0 {
            WlanCloseHandle(client_handle, None);
            return false;
        }

        let interface_list = &*interface_list_ptr;
        let mut connected = false;

        for i in 0..interface_list.dwNumberOfItems {
            let interface_info: &WLAN_INTERFACE_INFO = &interface_list.InterfaceInfo[i as usize];

            let mut data_size: u32 = 0;
            let mut data_ptr: *mut std::ffi::c_void = null_mut();
            let query_result = WlanQueryInterface(
                client_handle,
                &interface_info.InterfaceGuid,
                WLAN_INTF_OPCODE(0),
                None,
                &mut data_size,
                &mut data_ptr,
                None,
            );

            if query_result == 0 {
                let attrs = &*(data_ptr as *mut WLAN_CONNECTION_ATTRIBUTES);
                if attrs.isState == wlan_interface_state_connected {
                    connected = true;
                    break;
                }
            }
        }

        WlanCloseHandle(client_handle, None);
        connected
    }
}

/// Verifies internet connectivity, attempting to reconnect via Wi-Fi if offline.
/// Tries a quick HTTP request first; if that fails, checks WLAN and reconnects.
/// Polls up to ~15 seconds total before giving up.
pub async fn ensure_internet_connected() -> anyhow::Result<()> {
    // Quick connectivity test via HTTP
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    if client.head("http://clients3.google.com/generate_204")
        .send().await.is_ok()
    {
        return Ok(());
    }

    log::warn!("Internet check failed, attempting WiFi reconnect...");

    if !is_wlan_connected() {
        let _ = connect_to_wifi("PCL5", Some("bestburger"), None);
    }

    // Poll for connectivity (3 attempts, ~5s each)
    for attempt in 1..=3 {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        if client.head("http://clients3.google.com/generate_204")
            .send().await.is_ok()
        {
            log::info!("Internet restored after {attempt} attempt(s)");
            return Ok(());
        }

        if !is_wlan_connected() {
            let _ = connect_to_wifi("PCL5", Some("bestburger"), None);
        }
    }

    Err(anyhow::anyhow!("No internet connectivity after reconnect attempts"))
}