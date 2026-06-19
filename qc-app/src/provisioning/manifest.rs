//! Company provisioning manifest: the data that drives the runner (DMI fields,
//! ordered steps, conditional software). Loaded from an on-disk override, else
//! an embedded TOML, else a code-built default per company.

use serde::Deserialize;

use super::company::Company;

// `company`/`display_studio`/`software` are the parsed forward contract (studio
// display mode + the G4 software matrix) consumed in a later phase; `steps` +
// `dmi` drive the P0–P1 panel today.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct CompanyManifest {
    pub company: String,
    #[serde(default)]
    pub display_studio: bool,
    #[serde(default)]
    pub dmi: DmiManifest,
    #[serde(default)]
    pub steps: Vec<StepSpec>,
    #[serde(default)]
    pub software: Vec<SoftwareSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DmiManifest {
    /// SMBIOS manufacturer; `None` skips manufacturer writes.
    pub manufacturer: Option<String>,
    #[serde(default = "default_sku")]
    pub system_sku: String,
    /// `{config_name} {baseboard_product}` template for SYSTEM_PRODUCT.
    #[serde(default = "default_product_template")]
    pub product_template: String,
}

fn default_sku() -> String {
    "0001".to_string()
}
fn default_product_template() -> String {
    "{config_name} {baseboard_product}".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepSpec {
    /// `branding | dmi | core_isolation | timezone | open_tools | chipset | display | software`.
    pub kind: String,
}

/// Conditional software install descriptor (used by the later G4 phase).
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct SoftwareSpec {
    pub id: String,
    /// Share-relative directory + file, or `internal:<fn>` sentinel.
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub file: String,
    /// `exe | msi | cmd | internal`.
    #[serde(default)]
    pub installer: String,
    #[serde(default)]
    pub args: String,
    /// Condition DSL: `always | overclock | has_sound_card | has_capture_card |
    /// cooler_sku:<sku> | fans_contains:<s> | mobo_mfr:<MFR>`.
    #[serde(default)]
    pub when: String,
}

impl CompanyManifest {
    /// Standard non-destructive P0–P1 step sequence.
    fn default_steps() -> Vec<StepSpec> {
        ["branding", "dmi", "core_isolation", "timezone", "open_tools", "chipset", "display"]
            .into_iter()
            .map(|k| StepSpec { kind: k.to_string() })
            .collect()
    }

    /// Code-built manifest for a company (used when no TOML is present).
    pub fn default_for(company: Company) -> CompanyManifest {
        let display_studio = matches!(
            company,
            Company::Bimbox | Company::VrChat | Company::ColdIron | Company::NvidiaStudio
        );
        CompanyManifest {
            company: company.label().to_string(),
            display_studio,
            dmi: DmiManifest {
                manufacturer: company.dmi_manufacturer().map(str::to_string),
                system_sku: default_sku(),
                product_template: default_product_template(),
            },
            steps: Self::default_steps(),
            software: Vec::new(),
        }
    }
}

/// On-disk override path for a company manifest.
fn override_path(company: Company) -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("provisioning").join(format!("{}.toml", company.manifest_key())))
}

/// Embedded canonical manifests shipped with the binary.
fn embedded(company: Company) -> Option<&'static str> {
    match company {
        Company::Pcl => Some(include_str!("manifests/pcl.toml")),
        _ => None,
    }
}

/// Resolve a company's manifest: on-disk override → embedded TOML → code default.
pub fn load(company: Company) -> CompanyManifest {
    if let Some(path) = override_path(company) {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            match toml::from_str::<CompanyManifest>(&raw) {
                Ok(m) => return m,
                Err(e) => log::warn!("provisioning: bad manifest override {}: {e}", path.display()),
            }
        }
    }
    if let Some(raw) = embedded(company) {
        match toml::from_str::<CompanyManifest>(raw) {
            Ok(m) => return m,
            Err(e) => log::error!("provisioning: embedded {} manifest failed to parse: {e}", company.manifest_key()),
        }
    }
    CompanyManifest::default_for(company)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pcl_parses() {
        let raw = embedded(Company::Pcl).expect("pcl embedded");
        let m: CompanyManifest = toml::from_str(raw).expect("pcl manifest parses");
        assert_eq!(m.dmi.manufacturer.as_deref(), Some("PCL"));
        assert!(!m.steps.is_empty());
        assert!(m.steps.iter().any(|s| s.kind == "dmi"));
    }

    #[test]
    fn default_for_sets_studio_and_mfr() {
        let x = CompanyManifest::default_for(Company::NvidiaStudio);
        assert!(x.display_studio);
        assert_eq!(x.dmi.manufacturer.as_deref(), Some("NVIDIA_STUDIO"));
        let pcl = CompanyManifest::default_for(Company::Pcl);
        assert!(!pcl.display_studio);
    }

    #[test]
    fn load_falls_back_to_default() {
        // No override on disk in CI → embedded (PCL) or code default.
        let m = load(Company::Xidax);
        assert_eq!(m.dmi.manufacturer.as_deref(), Some("XIDAX"));
    }
}
