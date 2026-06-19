//! Company derivation + identity. A company is data (branding, DMI
//! manufacturer, install manifest), not code branches — see `manifest.rs`.

use database::orders::{BackendKind, QcOrder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Company {
    None,
    Pcl,
    Xidax,
    Bimbox,
    Mhs,
    VrChat,
    NvidiaStudio,
    ColdIron,
    Intel,
}

impl Company {
    pub const ALL: [Company; 9] = [
        Company::Pcl,
        Company::Xidax,
        Company::Bimbox,
        Company::Mhs,
        Company::VrChat,
        Company::NvidiaStudio,
        Company::ColdIron,
        Company::Intel,
        Company::None,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Company::None => "None",
            Company::Pcl => "PC Laptops",
            Company::Xidax => "Xidax",
            Company::Bimbox => "Bimbox",
            Company::Mhs => "MHS",
            Company::VrChat => "VRChat",
            Company::NvidiaStudio => "Nvidia Studio",
            Company::ColdIron => "Cold Iron",
            Company::Intel => "Intel",
        }
    }

    /// Manifest file stem (`provisioning/manifests/<key>.toml`).
    pub fn manifest_key(&self) -> &'static str {
        match self {
            Company::None => "none",
            Company::Pcl => "pcl",
            Company::Xidax => "xidax",
            Company::Bimbox => "bimbox",
            Company::Mhs => "mhs",
            Company::VrChat => "vrchat",
            Company::NvidiaStudio => "nvidia_studio",
            Company::ColdIron => "cold_iron",
            Company::Intel => "intel",
        }
    }

    /// SMBIOS manufacturer string written by DMI (None ⇒ skip mfr writes).
    pub fn dmi_manufacturer(&self) -> Option<&'static str> {
        match self {
            Company::None => None,
            Company::Pcl => Some("PCL"),
            Company::Xidax => Some("XIDAX"),
            Company::Bimbox => Some("BIMBOX"),
            Company::Mhs => Some("MHS"),
            Company::VrChat => Some("VRCHAT"),
            Company::NvidiaStudio => Some("NVIDIA_STUDIO"),
            Company::ColdIron => Some("COLD_IRON"),
            Company::Intel => Some("INTEL"),
        }
    }

    #[allow(dead_code)] // override-from-string; used by tests + future load paths
    pub fn from_label(s: &str) -> Company {
        Company::ALL.into_iter().find(|c| c.label() == s).unwrap_or(Company::None)
    }

    /// Best-guess company for an order: backend default, with the QCWizard
    /// config-id override (config 154 → Nvidia Studio). The UI offers an
    /// explicit override.
    pub fn from_order(order: &QcOrder) -> Company {
        if let Some(config) = order.config.as_ref() {
            if config.id_config == "154" {
                return Company::NvidiaStudio;
            }
        }
        match order.backend {
            Some(BackendKind::Shopify) => Company::Xidax,
            Some(BackendKind::Prestashop) => Company::Pcl,
            None => Company::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::orders::{OrderConfigInfo, OrderKind, StatusInfo};

    fn order(backend: Option<BackendKind>, id_config: &str) -> QcOrder {
        QcOrder {
            backend,
            id: "1".into(),
            reference: "#1".into(),
            customer_name: String::new(),
            kind: OrderKind::Sales,
            status: StatusInfo::default(),
            config: (!id_config.is_empty()).then(|| OrderConfigInfo {
                id_config: id_config.into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn backend_defaults() {
        assert_eq!(Company::from_order(&order(Some(BackendKind::Shopify), "")), Company::Xidax);
        assert_eq!(Company::from_order(&order(Some(BackendKind::Prestashop), "")), Company::Pcl);
    }

    #[test]
    fn config_154_is_nvidia_studio() {
        assert_eq!(Company::from_order(&order(Some(BackendKind::Prestashop), "154")), Company::NvidiaStudio);
    }

    #[test]
    fn label_round_trip() {
        for c in Company::ALL {
            assert_eq!(Company::from_label(c.label()), c);
        }
    }
}
