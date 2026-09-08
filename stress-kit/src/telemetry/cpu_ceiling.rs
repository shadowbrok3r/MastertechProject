//! The CPU's own thermal ceiling, and the CPUID identity behind it.
//!
//! A modern part runs *at* its ceiling under sustained all-core load and
//! clock-throttles to hold there, so the ceiling is a design point, not a fault
//! threshold: grading a flat number against it fails every healthy part whose
//! ceiling sits at or below that number.
//!
//! Intel publishes the value in `MSR_TEMPERATURE_TARGET`, so a live backend
//! reads it directly. AMD publishes nothing readable, so desktop Zen parts come
//! from a table keyed on CPUID family/model. Anything not listed reports no
//! ceiling and the caller keeps its configured limit.

use serde::{Deserialize, Serialize};

/// CPU vendor from CPUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Other,
}

/// Which source produced a ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuCeilingSource {
    /// Intel `MSR_TEMPERATURE_TARGET`; the part's own published value.
    IntelMsr,
    /// AMD desktop Zen table keyed on CPUID family/model.
    AmdModel,
}

/// Temperature the part's firmware throttles to hold.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CpuThermalCeiling {
    pub limit_c: f32,
    pub source: CpuCeilingSource,
}

/// CPUID family, model, and brand string of the running CPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuIdentity {
    pub vendor: CpuVendor,
    pub family: u8,
    pub model: u8,
    pub brand: String,
}

/// Identity of the running CPU; `None` off x86 or without the CPUID reader.
pub fn identity() -> Option<CpuIdentity> {
    cpuid_identity()
}

/// Vendor of the running CPU.
pub fn vendor() -> CpuVendor {
    cpuid_identity().map_or(CpuVendor::Other, |id| id.vendor)
}

/// Ceiling for the running CPU. `msr_tjmax_c` is the value a live backend read
/// from `MSR_TEMPERATURE_TARGET`; pass `None` when no backend answered.
pub fn detect(msr_tjmax_c: Option<u32>) -> Option<CpuThermalCeiling> {
    ceiling_for(cpuid_identity().as_ref()?, msr_tjmax_c)
}

/// [`detect`] against a supplied identity, so the table is testable off-target.
pub fn ceiling_for(id: &CpuIdentity, msr_tjmax_c: Option<u32>) -> Option<CpuThermalCeiling> {
    match id.vendor {
        CpuVendor::Intel => msr_tjmax_c.map(|t| CpuThermalCeiling {
            limit_c: t as f32,
            source: CpuCeilingSource::IntelMsr,
        }),
        CpuVendor::Amd => amd_ceiling(id.family, id.model, &id.brand).map(|limit_c| {
            CpuThermalCeiling {
                limit_c,
                source: CpuCeilingSource::AmdModel,
            }
        }),
        CpuVendor::Other => None,
    }
}

/// Ceilings for the desktop Zen parts this shop builds and certifies. Every
/// other AMD part returns `None` rather than a guess: a wrong ceiling either
/// fails a healthy machine or clears a cooked one, and mobile and server dies
/// in these same families run to different limits that CPUID cannot separate.
///
/// Extend by adding the family/model pair, not by widening a range.
fn amd_ceiling(family: u8, model: u8, brand: &str) -> Option<f32> {
    let x3d = brand.to_ascii_uppercase().contains("X3D");
    match (family, model) {
        // Zen, Zen+, Zen 2 — Ryzen 1000/2000/3000 and Threadripper 1000-3000.
        (0x17, _) => Some(95.0),
        // Zen 3 Vermeer — Ryzen 5000 desktop, 5800X3D included.
        (0x19, 0x21) => Some(90.0),
        // Zen 4 Raphael — Ryzen 7000 desktop; the X3D parts cap lower.
        (0x19, 0x61) => Some(if x3d { 89.0 } else { 95.0 }),
        // Zen 5 Granite Ridge — Ryzen 9000 desktop, 9800X3D included.
        (0x1A, 0x44) => Some(95.0),
        _ => None,
    }
}

#[cfg(all(feature = "lowlevel", any(target_arch = "x86", target_arch = "x86_64")))]
fn cpuid_identity() -> Option<CpuIdentity> {
    let cpuid = raw_cpuid::CpuId::new();
    let vendor = match cpuid.get_vendor_info().map(|v| v.as_str().to_string()) {
        Some(v) if v.contains("Intel") => CpuVendor::Intel,
        Some(v) if v.contains("AMD") => CpuVendor::Amd,
        _ => CpuVendor::Other,
    };
    let feature = cpuid.get_feature_info()?;
    Some(CpuIdentity {
        vendor,
        family: feature.family_id(),
        model: feature.model_id(),
        brand: cpuid
            .get_processor_brand_string()
            .map(|b| b.as_str().trim().to_string())
            .unwrap_or_default(),
    })
}

#[cfg(not(all(feature = "lowlevel", any(target_arch = "x86", target_arch = "x86_64"))))]
fn cpuid_identity() -> Option<CpuIdentity> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amd(family: u8, model: u8, brand: &str) -> CpuIdentity {
        CpuIdentity {
            vendor: CpuVendor::Amd,
            family,
            model,
            brand: brand.to_string(),
        }
    }

    /// The 7900X on ticket 2153882: Tjmax is 95 C and the part runs there by
    /// design, so the ceiling must read 95 rather than the flat rule limit.
    #[test]
    fn raphael_reports_its_own_ceiling() {
        let id = amd(0x19, 0x61, "AMD Ryzen 9 7900X 12-Core Processor");
        let ceiling = ceiling_for(&id, None).expect("Raphael is tabled");
        assert_eq!(ceiling.limit_c, 95.0);
        assert_eq!(ceiling.source, CpuCeilingSource::AmdModel);
    }

    #[test]
    fn raphael_x3d_caps_lower_than_the_rest_of_its_family() {
        let plain = amd(0x19, 0x61, "AMD Ryzen 7 7700X 8-Core Processor");
        let x3d = amd(0x19, 0x61, "AMD Ryzen 7 7800X3D 8-Core Processor");
        assert_eq!(ceiling_for(&plain, None).unwrap().limit_c, 95.0);
        assert_eq!(ceiling_for(&x3d, None).unwrap().limit_c, 89.0);
    }

    #[test]
    fn zen3_and_zen5_desktop_are_tabled() {
        assert_eq!(
            ceiling_for(&amd(0x19, 0x21, "AMD Ryzen 7 5800X"), None)
                .unwrap()
                .limit_c,
            90.0
        );
        assert_eq!(
            ceiling_for(&amd(0x1A, 0x44, "AMD Ryzen 9 9950X"), None)
                .unwrap()
                .limit_c,
            95.0
        );
        assert_eq!(
            ceiling_for(&amd(0x17, 0x71, "AMD Ryzen 9 3900X"), None)
                .unwrap()
                .limit_c,
            95.0
        );
    }

    /// An untabled AMD part reports nothing, so the caller keeps its configured
    /// limit instead of grading against a guess.
    #[test]
    fn an_untabled_amd_part_reports_no_ceiling() {
        assert!(ceiling_for(&amd(0x19, 0x74, "AMD Ryzen 7 8845HS"), None).is_none());
        assert!(ceiling_for(&amd(0x1A, 0x24, "AMD Ryzen AI 9 365"), None).is_none());
    }

    /// Intel takes the MSR value and nothing else; no backend means no ceiling.
    #[test]
    fn intel_comes_from_the_msr_only() {
        let id = CpuIdentity {
            vendor: CpuVendor::Intel,
            family: 6,
            model: 0xB7,
            brand: "13th Gen Intel(R) Core(TM) i7-13700K".to_string(),
        };
        let ceiling = ceiling_for(&id, Some(100)).expect("MSR value present");
        assert_eq!(ceiling.limit_c, 100.0);
        assert_eq!(ceiling.source, CpuCeilingSource::IntelMsr);
        assert!(ceiling_for(&id, None).is_none());
    }

    #[test]
    fn a_non_x86_vendor_reports_no_ceiling() {
        let id = CpuIdentity {
            vendor: CpuVendor::Other,
            family: 0,
            model: 0,
            brand: String::new(),
        };
        assert!(ceiling_for(&id, Some(100)).is_none());
    }
}
