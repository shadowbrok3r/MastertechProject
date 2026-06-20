//! G4 conditional software install: the per-order software matrix from the
//! company manifest. Each `SoftwareSpec` carries a `when` condition evaluated
//! against the order's `BuildSpec`; applicable entries stage from the
//! `\\winbits7` share and install silently. Windows-only execution.

use anyhow::anyhow;

use database::orders::BuildSpec;

use super::drivers;
use super::manifest::{CompanyManifest, SoftwareSpec};

/// Evaluate a spec's `when` condition against the build.
pub fn applicable(spec: &SoftwareSpec, build: &BuildSpec) -> bool {
    let when = spec.when.trim().to_ascii_lowercase();
    let extras_contain = |needle: &str| {
        build.extra.iter().any(|e| e.name.to_ascii_lowercase().contains(needle))
    };
    let model = build.model.to_ascii_lowercase();
    let mobo = build.motherboard.clone().unwrap_or_default().to_ascii_lowercase();

    match when.as_str() {
        "always" => true,
        "overclock" => {
            extras_contain("overclock") || model.contains("overclock") || model.contains(" oc ")
        }
        "has_sound_card" => extras_contain("sound"),
        "has_capture_card" => extras_contain("capture") || extras_contain("elgato"),
        _ => {
            if let Some(sku) = when.strip_prefix("cooler_sku:") {
                extras_contain(sku.trim())
            } else if let Some(s) = when.strip_prefix("fans_contains:") {
                let s = s.trim();
                extras_contain(s) || model.contains(s)
            } else if let Some(mfr) = when.strip_prefix("mobo_mfr:") {
                mobo.contains(mfr.trim())
            } else {
                log::warn!("provisioning: unknown software condition '{}' on '{}'", spec.when, spec.id);
                false
            }
        }
    }
}

/// Specs whose conditions match the build, in manifest order.
pub fn plan<'a>(manifest: &'a CompanyManifest, build: &BuildSpec) -> Vec<&'a SoftwareSpec> {
    manifest.software.iter().filter(|s| applicable(s, build)).collect()
}

/// Stage and install one spec. Returns a status line.
pub fn install(spec: &SoftwareSpec) -> anyhow::Result<String> {
    match spec.installer.as_str() {
        "internal" => {
            log::info!("provisioning: internal installer '{}' (no-op stub)", spec.id);
            Ok(format!("internal installer '{}' (no-op stub)", spec.id))
        }
        "exe" | "msi" | "cmd" | "" => {
            let relative = if spec.dir.is_empty() {
                spec.file.clone()
            } else {
                format!("{}/{}", spec.dir, spec.file)
            };
            drivers::install_relative(&relative, &spec.args)?;
            Ok(format!("Installed {}", spec.id))
        }
        other => Err(anyhow!("unknown installer kind '{}' for '{}'", other, spec.id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::orders::SlotPick;

    fn build() -> BuildSpec {
        BuildSpec {
            extra: vec![SlotPick { slot: "fans".into(), name: "Corsair iCUE Fans".into() }],
            motherboard: Some("MSI MEG X670E".into()),
            ..Default::default()
        }
    }

    fn spec(when: &str) -> SoftwareSpec {
        SoftwareSpec {
            id: "test".into(),
            dir: String::new(),
            file: String::new(),
            installer: String::new(),
            args: String::new(),
            when: when.into(),
        }
    }

    #[test]
    fn always_matches() {
        assert!(applicable(&spec("always"), &build()));
    }

    #[test]
    fn fans_contains_corsair_matches() {
        assert!(applicable(&spec("fans_contains:Corsair"), &build()));
    }

    #[test]
    fn mobo_mfr_msi_matches() {
        assert!(applicable(&spec("mobo_mfr:MSI"), &build()));
    }

    #[test]
    fn fans_contains_lian_does_not_match() {
        assert!(!applicable(&spec("fans_contains:Lian"), &build()));
    }
}
