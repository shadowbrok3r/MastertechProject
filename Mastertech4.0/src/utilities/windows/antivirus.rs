use windows::{
    core::GUID,
    Win32::System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED
        }, SecurityCenter::{IWSCProductList, WSC_SECURITY_PRODUCT_STATE, WSC_SECURITY_PROVIDER}
    },
};
use database::schema::{InstalledSecurityProduct, SecurityProductSource};

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
        let _ = CoInitializeEx(Some(std::ptr::null_mut()), COINIT_MULTITHREADED).map(|| {});
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

/// Bare WMI tuple — what the synchronous `IWSCProductList`
/// enumeration knows about a registered AV: name + a state value
/// whose lower bits we decode to `active`. The full enrichment
/// (vendor, version, etc.) happens later by joining with the
/// registry walk.
struct WmiProduct {
    name: String,
    active: Option<bool>,
}

/// Enumerate antivirus products registered with Windows Security
/// Center 2. Synchronous because the underlying COM interface is
/// — callers wrap this in `tokio::task::spawn_blocking` so the
/// async runtime doesn't get stuck on the COM call.
///
/// `state` is the raw `ProductState()` u32. Per the
/// `WSC_SECURITY_PRODUCT_STATE` constants we treat the
/// "on" sentinel (0x0) as `active=true`; "off" or "snoozed" as
/// `active=false`. The "expired" / "unknown" states are reported
/// as `active=None` so the admin UI can render a "—".
fn gather_wmi_security_products() -> Vec<WmiProduct> {
    let mut out = Vec::new();
    unsafe {
        let _ = CoInitializeEx(Some(std::ptr::null_mut()), COINIT_MULTITHREADED).map(|| {});
        let product_list: IWSCProductList = match CoCreateInstance(
            &CLSID_WSC_PRODUCT_LIST,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("gather_wmi_security_products: CoCreateInstance failed: {e:?}");
                CoUninitialize();
                return out;
            }
        };

        if let Err(e) =
            product_list.Initialize(WSC_SECURITY_PROVIDER(WSC_SECURITY_PROVIDER_ANTIVIRUS))
        {
            log::warn!("gather_wmi_security_products: Initialize failed: {e:?}");
            CoUninitialize();
            return out;
        }

        let count = product_list.Count().unwrap_or(0);
        for i in 0..count {
            let product = match product_list.get_Item(i as u32) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("gather_wmi_security_products: get_Item({i}) failed: {e:?}");
                    continue;
                }
            };
            let name = product.ProductName().map(|s| s.to_string()).unwrap_or_default();
            let state = product.ProductState().ok();
            let active = state.map(|s| s == WSC_SECURITY_PRODUCT_STATE(WSC_SECURITY_PRODUCT_STATE_ON));
            if name.is_empty() {
                continue;
            }
            out.push(WmiProduct { name, active });
        }

        CoUninitialize();
    }
    out
}

/// Raw row from the Windows registry's Uninstall key walk —
/// matched against WMI names case-insensitively to enrich them
/// with `version` / `vendor`.
#[derive(Debug, Clone)]
struct RegistryUninstallEntry {
    display_name: String,
    display_version: Option<String>,
    publisher: Option<String>,
}

/// Async PowerShell wrapper that returns *every* uninstall-key
/// entry under HKLM + HKCU (both 64-bit and Wow6432Node). Done in
/// one PS call so we pay the launch cost once.
///
/// We use PowerShell rather than direct registry-API calls because
/// the binary already shells out to PowerShell for `ListServices`
/// / `ListStartupApps`, and the JSON serialization is much less
/// fiddly than the `winreg` crate would be for a one-shot fetch.
async fn fetch_registry_uninstall_entries() -> Vec<RegistryUninstallEntry> {
    let ps_cmd = r#"
$paths = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
)
$results = foreach ($path in $paths) {
  if (Test-Path $path) {
    Get-ChildItem $path -ErrorAction SilentlyContinue | ForEach-Object {
      $props = $_ | Get-ItemProperty -ErrorAction SilentlyContinue
      if ($props.DisplayName) {
        [PSCustomObject]@{
          DisplayName    = $props.DisplayName
          DisplayVersion = $props.DisplayVersion
          Publisher      = $props.Publisher
        }
      }
    }
  }
}
$results | ConvertTo-Json -Compress -Depth 3
"#;

    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_cmd])
        .output()
        .await;

    let Ok(out) = output else {
        return Vec::new();
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // `ConvertTo-Json` collapses single-element arrays to a bare
    // object, so try both shapes.
    let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("fetch_registry_uninstall_entries: JSON parse failed: {e}");
            return Vec::new();
        }
    };
    let rows: Vec<&serde_json::Value> = match &parsed {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(_) => vec![&parsed],
        _ => return Vec::new(),
    };

    rows.into_iter()
        .filter_map(|v| {
            let display_name = v.get("DisplayName")?.as_str()?.to_string();
            Some(RegistryUninstallEntry {
                display_name,
                display_version: v.get("DisplayVersion")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                publisher: v.get("Publisher")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect()
}

/// Case-insensitive substring match used when joining the WMI
/// list against the registry. A WMI product name like
/// `"Webroot SecureAnywhere"` matches a registry entry
/// `"Webroot SecureAnywhere"` (exact), `"Webroot"` (prefix), and
/// `"Webroot SecureAnywhere Anti-Malware"` (contains) — the most
/// specific match wins.
fn registry_match<'a>(
    wmi_name: &str,
    registry: &'a [RegistryUninstallEntry],
) -> Option<&'a RegistryUninstallEntry> {
    let needle = wmi_name.to_lowercase();

    // Pick the *longest* registry name whose lowercase form
    // either contains or is contained by the WMI name. The
    // longer match is usually the more specific row (e.g. picks
    // "Webroot SecureAnywhere Anti-Malware" over a bare "Webroot"
    // uninstaller stub).
    registry
        .iter()
        .filter(|r| {
            let hay = r.display_name.to_lowercase();
            hay.contains(&needle) || needle.contains(&hay)
        })
        .max_by_key(|r| r.display_name.len())
}

/// The async entry point the websockets handler calls. Joins the
/// WMI list with the registry list and returns one
/// [`InstalledSecurityProduct`] per WMI registration. If WMI
/// returns nothing (uncommon — only on very old Windows), falls
/// back to a registry-only pass that picks rows whose name
/// matches a known AV vendor keyword (Webroot, Malwarebytes,
/// SuperAntiSpyware, Defender, etc.).
pub async fn gather_security_inventory() -> Vec<InstalledSecurityProduct> {
    let wmi = tokio::task::spawn_blocking(gather_wmi_security_products)
        .await
        .unwrap_or_default();
    let registry = fetch_registry_uninstall_entries().await;

    if !wmi.is_empty() {
        return wmi
            .into_iter()
            .map(|w| {
                let reg = registry_match(&w.name, &registry);
                InstalledSecurityProduct {
                    name: w.name.clone(),
                    vendor: reg.and_then(|r| r.publisher.clone()),
                    version: reg.and_then(|r| r.display_version.clone()),
                    active: w.active,
                    definitions_updated_at: None,
                    update_available: None,
                    source: SecurityProductSource::SecurityCenter,
                }
            })
            .collect();
    }

    // WMI gave us nothing — fall back to registry rows that look
    // like a security product. The keyword list is conservative;
    // false positives (e.g. "Microsoft Security Update for Office")
    // would clutter the admin UI more than missing entries.
    const KEYWORDS: &[&str] = &[
        "antivirus",
        "anti-virus",
        "anti virus",
        "antimalware",
        "anti-malware",
        "antispyware",
        "anti-spyware",
        "endpoint protection",
        "webroot",
        "malwarebytes",
        "superantispyware",
        "norton",
        "mcafee",
        "eset",
        "kaspersky",
        "bitdefender",
        "avast",
        "avg ",
        "windows defender",
        "microsoft defender",
        "sentinelone",
        "crowdstrike",
        "carbon black",
    ];
    registry
        .into_iter()
        .filter(|r| {
            let hay = r.display_name.to_lowercase();
            KEYWORDS.iter().any(|kw| hay.contains(kw))
        })
        .map(|r| InstalledSecurityProduct {
            name: r.display_name,
            vendor: r.publisher,
            version: r.display_version,
            // No WMI = no live state; admin UI renders "—".
            active: None,
            definitions_updated_at: None,
            update_available: None,
            source: SecurityProductSource::Registry,
        })
        .collect()
}
