use windows::{
    core::{Result, GUID},
    Win32::System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED
        }, SecurityCenter::{IWSCProductList, WSC_SECURITY_PRODUCT_STATE, WSC_SECURITY_PROVIDER}
    },
};

// If you’re not using these imports later, consider removing them.
// use windows_core::*;
// use windows_x86_64_msvc::*;

// Define the GUIDs as per wscapi.h.
const CLSID_WSC_PRODUCT_LIST: GUID = GUID::from_u128(0x17072F7B_9ABE_4A74_A261_1EB76B55107A);

// Constants for the security provider and state.
// (Replace these with the actual values from the SDK if needed.)
const WSC_SECURITY_PROVIDER_ANTIVIRUS: i32 = 0x1;
const WSC_SECURITY_PRODUCT_STATE_ON: i32 = 0x0;



/// Checks for installed antivirus products and prints their status.
pub fn check_antivirus() -> anyhow::Result<Vec<String>, anyhow::Error> {
    let mut active_antivirus = Vec::new();
    unsafe {
        // Initialize COM for multithreaded usage.
        let x = CoInitializeEx(Some(std::ptr::null_mut()), COINIT_MULTITHREADED).map(|| {});
        log::info!("CoInit: {x:?}");
        // Create an instance of the product list.
        let product_list: IWSCProductList = CoCreateInstance(
            &CLSID_WSC_PRODUCT_LIST,
            None,
            CLSCTX_INPROC_SERVER,
        )?;

        // Initialize the list to only include antivirus products.
        product_list.Initialize(WSC_SECURITY_PROVIDER(WSC_SECURITY_PROVIDER_ANTIVIRUS))?;

        let count = product_list.Count()?;
        if count == 0 {
            active_antivirus.push("No antivirus products found on the system.".to_string());
        } else {
            for i in 0..count {
                let product = product_list.get_Item(i as u32)?;
                let name = product.ProductName()?;
                let state = product.ProductState()?;
                active_antivirus.push(format!("Product: {name} State: {state:?}"));
                let is_active = state == WSC_SECURITY_PRODUCT_STATE(WSC_SECURITY_PRODUCT_STATE_ON);
                log::info!(
                    "Found antivirus: {} is {}",
                    name,
                    if is_active { "active" } else { "inactive" }
                );
            }
        }

        CoUninitialize();
    }
    Ok(active_antivirus)
}
