//! Spec-vs-detected hardware comparison (QCWizard parity, objective rules).
//!
//! Pure engine: expected hardware from [`BuildSpec`], detected hardware as
//! plain data the caller gathers (qc-app feeds its telemetry snapshot in).
//! Matching is token-based after normalization, with capacity tolerances
//! for RAM and storage.

use serde::{Deserialize, Serialize};

use super::{BuildSpec, SpecCheckSummary, SpecDiffSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
    Match,
    Mismatch,
    NotDetected,
    NotSpecified,
}

impl CheckStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Match => "MATCH",
            Self::Mismatch => "MISMATCH",
            Self::NotDetected => "NOT DETECTED",
            Self::NotSpecified => "—",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecCheckRow {
    pub component: String,
    pub expected: String,
    pub detected: String,
    pub status: CheckStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecCheckReport {
    pub rows: Vec<SpecCheckRow>,
}

impl SpecCheckReport {
    pub fn matched(&self) -> bool {
        !self.rows.iter().any(|r| r.status == CheckStatus::Mismatch)
    }

    pub fn mismatch_count(&self) -> usize {
        self.rows.iter().filter(|r| r.status == CheckStatus::Mismatch).count()
    }

    /// Backend-portable summary embedded in QC report payloads.
    pub fn summary(&self) -> SpecCheckSummary {
        SpecCheckSummary {
            matched: self.matched(),
            diffs: self
                .rows
                .iter()
                .filter(|r| r.status != CheckStatus::Match && r.status != CheckStatus::NotSpecified)
                .map(|r| SpecDiffSummary {
                    component: r.component.clone(),
                    expected: r.expected.clone(),
                    detected: r.detected.clone(),
                    status: r.status.label().to_string(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectedDisk {
    pub name: String,
    pub total_gb: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectedHardware {
    pub cpu: String,
    pub ram_total_mb: u64,
    pub gpus: Vec<String>,
    pub disks: Vec<DetectedDisk>,
    pub os: String,
}

/// Lowercase, strip vendor decorations and clock suffixes, unify separators.
fn normalize(s: &str) -> String {
    let mut out = s.to_lowercase();
    for noise in ["(r)", "(tm)", "(c)", "®", "™"] {
        out = out.replace(noise, " ");
    }
    // "... @ 3.00ghz" suffixes from WMI/sysinfo brand strings.
    if let Some(at) = out.find('@') {
        out.truncate(at);
    }
    out = out.replace(['-', '_', ','], " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tokens that carry no model information.
const FILLER: &[&str] = &[
    "intel", "amd", "nvidia", "geforce", "radeon", "cpu", "gpu", "processor",
    "graphics", "video", "card", "edition", "series", "with", "the", "desktop",
    "laptop", "oem", "new", "gen",
];

fn significant_tokens(s: &str) -> Vec<String> {
    normalize(s)
        .split_whitespace()
        .filter(|t| !FILLER.contains(t))
        .map(str::to_string)
        .collect()
}

/// True when every significant expected token appears in the detected string.
fn name_matches(expected: &str, detected: &str) -> bool {
    let expected_tokens = significant_tokens(expected);
    if expected_tokens.is_empty() {
        return false;
    }
    let detected_norm = format!(" {} ", normalize(detected));
    expected_tokens
        .iter()
        .all(|t| detected_norm.contains(&format!(" {t} ")) || detected_norm.contains(t.as_str()))
}

/// First capacity in a product name: `32GB`, `2TB`, `1 TB`, `512 GB`.
fn parse_capacity_gb(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let number: f64 = lower[start..i].parse().unwrap_or(0.0);
            let rest = lower[i..].trim_start();
            if rest.starts_with("tb") {
                return Some(number * 1000.0);
            }
            if rest.starts_with("gb") {
                return Some(number);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Usable RAM under-reports installed size; snap detected MB to the marketing
/// size when within 12%.
fn detected_ram_gb(total_mb: u64) -> f64 {
    const LADDER: &[f64] = &[4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0, 192.0, 256.0, 384.0, 512.0];
    let raw_gb = total_mb as f64 / 1024.0;
    for size in LADDER {
        if raw_gb <= *size && (size - raw_gb) / size < 0.12 {
            return *size;
        }
    }
    raw_gb.round()
}

pub fn compare(spec: &BuildSpec, hw: &DetectedHardware) -> SpecCheckReport {
    let mut rows = Vec::new();

    // CPU
    rows.push(if spec.cpu.trim().is_empty() {
        SpecCheckRow {
            component: "CPU".into(),
            expected: String::new(),
            detected: hw.cpu.clone(),
            status: CheckStatus::NotSpecified,
        }
    } else if hw.cpu.trim().is_empty() {
        SpecCheckRow {
            component: "CPU".into(),
            expected: spec.cpu.clone(),
            detected: String::new(),
            status: CheckStatus::NotDetected,
        }
    } else {
        SpecCheckRow {
            component: "CPU".into(),
            expected: spec.cpu.clone(),
            detected: hw.cpu.clone(),
            status: if name_matches(&spec.cpu, &hw.cpu) {
                CheckStatus::Match
            } else {
                CheckStatus::Mismatch
            },
        }
    });

    // GPU — any detected adapter satisfying the spec counts.
    rows.push(if spec.gpu.trim().is_empty() {
        SpecCheckRow {
            component: "GPU".into(),
            expected: String::new(),
            detected: hw.gpus.join(" | "),
            status: CheckStatus::NotSpecified,
        }
    } else if hw.gpus.is_empty() {
        SpecCheckRow {
            component: "GPU".into(),
            expected: spec.gpu.clone(),
            detected: String::new(),
            status: CheckStatus::NotDetected,
        }
    } else {
        let hit = hw.gpus.iter().any(|g| name_matches(&spec.gpu, g));
        SpecCheckRow {
            component: "GPU".into(),
            expected: spec.gpu.clone(),
            detected: hw.gpus.join(" | "),
            status: if hit { CheckStatus::Match } else { CheckStatus::Mismatch },
        }
    });

    // RAM — capacity from the product name vs snapped usable total.
    let detected_gb = detected_ram_gb(hw.ram_total_mb);
    let detected_ram_label = format!("{detected_gb:.0} GB");
    rows.push(if spec.ram.trim().is_empty() {
        SpecCheckRow {
            component: "RAM".into(),
            expected: String::new(),
            detected: detected_ram_label,
            status: CheckStatus::NotSpecified,
        }
    } else {
        match parse_capacity_gb(&spec.ram) {
            Some(expected_gb) => SpecCheckRow {
                component: "RAM".into(),
                expected: spec.ram.clone(),
                detected: detected_ram_label,
                status: if (expected_gb - detected_gb).abs() < 0.5 {
                    CheckStatus::Match
                } else {
                    CheckStatus::Mismatch
                },
            },
            None => SpecCheckRow {
                component: "RAM".into(),
                expected: spec.ram.clone(),
                detected: detected_ram_label,
                status: CheckStatus::NotSpecified,
            },
        }
    });

    // Storage — greedy capacity match within 15% (decimal-unit drives).
    let mut used = vec![false; hw.disks.len()];
    for drive in &spec.drives {
        let Some(expected_gb) = parse_capacity_gb(&drive.name) else {
            rows.push(SpecCheckRow {
                component: format!("Storage ({})", drive.kind),
                expected: drive.name.clone(),
                detected: String::new(),
                status: CheckStatus::NotSpecified,
            });
            continue;
        };
        let candidate = hw
            .disks
            .iter()
            .enumerate()
            .filter(|(i, _)| !used[*i])
            .min_by(|(_, a), (_, b)| {
                let da = (a.total_gb - expected_gb).abs();
                let db = (b.total_gb - expected_gb).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
        match candidate {
            Some((i, disk)) if (disk.total_gb - expected_gb).abs() / expected_gb < 0.15 => {
                used[i] = true;
                rows.push(SpecCheckRow {
                    component: format!("Storage ({})", drive.kind),
                    expected: drive.name.clone(),
                    detected: format!("{} {:.0} GB", disk.name, disk.total_gb),
                    status: CheckStatus::Match,
                });
            }
            _ => rows.push(SpecCheckRow {
                component: format!("Storage ({})", drive.kind),
                expected: drive.name.clone(),
                detected: hw
                    .disks
                    .iter()
                    .map(|d| format!("{:.0} GB", d.total_gb))
                    .collect::<Vec<_>>()
                    .join(" | "),
                status: CheckStatus::Mismatch,
            }),
        }
    }

    // OS
    if let Some(expected_os) = spec.os.as_ref() {
        let status = if hw.os.is_empty() {
            CheckStatus::NotDetected
        } else if name_matches(expected_os, &hw.os) {
            CheckStatus::Match
        } else {
            CheckStatus::Mismatch
        };
        rows.push(SpecCheckRow {
            component: "OS".into(),
            expected: expected_os.clone(),
            detected: hw.os.clone(),
            status,
        });
    }

    SpecCheckReport { rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::DriveSpec;

    fn hw() -> DetectedHardware {
        DetectedHardware {
            cpu: "Intel(R) Core(TM) Ultra 7 275HX @ 3.00GHz".into(),
            ram_total_mb: 32_768 - 1_800,
            gpus: vec!["NVIDIA GeForce RTX 5070".into()],
            disks: vec![DetectedDisk { name: "C:".into(), total_gb: 2000.3 }],
            os: "Windows 11 Pro 26100".into(),
        }
    }

    #[test]
    fn cpu_tokens_survive_vendor_noise() {
        assert!(name_matches("Core Ultra 7 275HX", "Intel(R) Core(TM) Ultra 7 275HX"));
        assert!(name_matches("i7-14700K", "Intel(R) Core(TM) i7-14700K CPU @ 3.40GHz"));
        assert!(!name_matches("i9-14900K", "Intel(R) Core(TM) i7-14700K"));
    }

    #[test]
    fn gpu_brand_prefix_is_ignored() {
        assert!(name_matches("RTX 5070", "NVIDIA GeForce RTX 5070"));
        assert!(!name_matches("RTX 5070 Ti", "NVIDIA GeForce RTX 5070"));
    }

    #[test]
    fn capacity_parsing() {
        assert_eq!(parse_capacity_gb("32GB (2x16GB) DDR5 6000MHz"), Some(32.0));
        assert_eq!(parse_capacity_gb("2TB NVMe Gen4 SSD"), Some(2000.0));
        assert_eq!(parse_capacity_gb("Samsung 990 Pro 1 TB"), Some(1000.0));
        assert_eq!(parse_capacity_gb("RGB Fans"), None);
    }

    #[test]
    fn ram_snaps_to_marketing_size() {
        assert_eq!(detected_ram_gb(32_768 - 1_800), 32.0);
        assert_eq!(detected_ram_gb(16_384 - 600), 16.0);
    }

    #[test]
    fn full_compare_passes_matching_build() {
        let spec = BuildSpec {
            cpu: "Core Ultra 7 275HX".into(),
            gpu: "RTX 5070".into(),
            ram: "32GB DDR5 6000".into(),
            drives: vec![DriveSpec { name: "2TB NVMe SSD".into(), kind: "SSD".into() }],
            os: Some("Windows 11".into()),
            ..Default::default()
        };
        let report = compare(&spec, &hw());
        assert!(report.matched(), "{:#?}", report.rows);
    }

    #[test]
    fn full_compare_flags_wrong_gpu() {
        let spec = BuildSpec {
            gpu: "RTX 5090".into(),
            ..Default::default()
        };
        let report = compare(&spec, &hw());
        assert_eq!(report.mismatch_count(), 1);
        assert!(!report.matched());
    }

    #[test]
    fn missing_hardware_reports_not_detected() {
        let spec = BuildSpec {
            cpu: "Ryzen 7 9800X3D".into(),
            gpu: "RTX 5080".into(),
            ..Default::default()
        };
        let empty = DetectedHardware::default();
        let report = compare(&spec, &empty);
        assert!(report
            .rows
            .iter()
            .filter(|r| r.component == "CPU" || r.component == "GPU")
            .all(|r| r.status == CheckStatus::NotDetected));
    }
}
