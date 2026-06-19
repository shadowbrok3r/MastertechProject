//! WMI-backed [`LiveProbe`] for checklist auto-verify. The gating engine
//! (`apply` / `reverify_at_signoff`) lives in `database::orders::checklist_verify`
//! so it's testable without WMI; this module only reads system state.

pub use database::orders::checklist_verify::{apply, reverify_at_signoff, LiveProbe, SmartSummary};

#[cfg(windows)]
pub struct WmiProbe;

#[cfg(windows)]
impl WmiProbe {
    pub fn new() -> Self {
        WmiProbe
    }
}

#[cfg(windows)]
impl LiveProbe for WmiProbe {
    fn oa3_key_present(&self) -> bool {
        use serde::Deserialize;
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Sls {
            #[serde(rename = "OA3xOriginalProductKey")]
            oa3x_original_product_key: Option<String>,
        }
        let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2") else {
            return false;
        };
        let rows: Vec<Sls> = wmi.query().unwrap_or_default();
        rows.iter()
            .any(|r| r.oa3x_original_product_key.as_deref().map(|k| !k.trim().is_empty()).unwrap_or(false))
    }

    fn smart(&self) -> SmartSummary {
        use serde::Deserialize;
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct FailurePredict {
            instance_name: String,
            predict_failure: bool,
        }
        let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\WMI") else {
            return SmartSummary { queried: false, all_healthy: false, summary: "SMART query failed".into() };
        };
        let rows: Vec<FailurePredict> = match wmi.query() {
            Ok(r) => r,
            Err(_) => return SmartSummary { queried: false, all_healthy: false, summary: "SMART query failed".into() },
        };
        if rows.is_empty() {
            return SmartSummary { queried: false, all_healthy: false, summary: "SMART not reported by the storage driver".into() };
        }
        let failing: Vec<&str> = rows.iter().filter(|r| r.predict_failure).map(|r| r.instance_name.as_str()).collect();
        let ok = failing.is_empty();
        let summary = if ok {
            format!("SMART OK — {} drive(s), no predicted failures", rows.len())
        } else {
            format!("SMART FAILURE PREDICTED: {}", failing.join("; "))
        };
        SmartSummary { queried: true, all_healthy: ok, summary }
    }
}

#[cfg(not(windows))]
pub struct WmiProbe;

#[cfg(not(windows))]
impl WmiProbe {
    pub fn new() -> Self {
        WmiProbe
    }
}

#[cfg(not(windows))]
impl LiveProbe for WmiProbe {
    fn oa3_key_present(&self) -> bool {
        false
    }
    fn smart(&self) -> SmartSummary {
        SmartSummary { queried: false, all_healthy: false, summary: "SMART unavailable on this platform".into() }
    }
}
