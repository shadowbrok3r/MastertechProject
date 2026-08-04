//! Technician-facing metadata for every stressor, run mode, and cert preset.
//!
//! Single source of truth for the copy shown in hover hints and help panels.
//! Lives here (not in a UI crate) so qc-app, Mastertech4.0, the terminal
//! renderers, the Scripts catalog, and the MCP layer all read the same strings.
//! `what` / `when` are the two halves of the older
//! `"<what>; run <when>"` script descriptions, split so a UI can show them
//! separately.

use crate::panel_config::{PanelMode, StressorChoice};

/// Hardware area a stressor loads, used to group the pickers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Subsystem {
    Cpu,
    Memory,
    Storage,
    Gpu,
    Power,
}

impl Subsystem {
    pub const ALL: [Self; 5] = [Self::Cpu, Self::Memory, Self::Storage, Self::Gpu, Self::Power];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Gpu => "GPU",
            Self::Power => "Power / Whole-system",
        }
    }

    /// One line on what this group proves, for the group header hover.
    pub fn blurb(self) -> &'static str {
        match self {
            Self::Cpu => "Core execution, cache, and pipeline behaviour under sustained load.",
            Self::Memory => "DIMM capacity, bandwidth, and data integrity.",
            Self::Storage => "Drive throughput and I/O errors under continuous access.",
            Self::Gpu => "Shader execution, VRAM, the PCIe link, and the display path.",
            Self::Power => "Delivery and cooling with more than one subsystem loaded at once.",
        }
    }

    /// Stressors in this group, in picker order.
    pub fn stressors(self) -> &'static [StressorChoice] {
        use StressorChoice as S;
        match self {
            Self::Cpu => &[
                S::Cpu,
                S::Fp,
                S::Matrix,
                S::Linpack,
                S::CpuVerify,
                S::Bitops,
                S::Branch,
                S::Prime,
                S::Hash,
                S::Cache,
                S::Prefetch,
                S::Icache,
                S::Tsc,
                S::Atomic,
                S::Mutex,
                S::Switch,
            ],
            Self::Memory => &[S::MemTest, S::Memory, S::Stream, S::Memcpy, S::Vm],
            Self::Storage => &[S::Disk],
            Self::Gpu => &[S::Gpu, S::GpuMatmul, S::GpuVram, S::GpuPcie, S::GpuDisplay],
            Self::Power => &[S::Psu, S::PsuTransient, S::Combined],
        }
    }
}

/// Everything a technician needs to pick a stressor without reading the source.
#[derive(Clone, Copy, Debug)]
pub struct StressorInfo {
    pub subsystem: Subsystem,
    /// What the load actually does.
    pub what: &'static str,
    /// The symptom or question that should make a tech reach for this.
    pub when: &'static str,
    /// Verdict rule applied to a single run, when there is one.
    pub pass: Option<&'static str>,
    /// Condition that silently weakens the result, when there is one.
    pub caveat: Option<&'static str>,
}

/// Metadata for one stressor.
pub fn info_for(choice: StressorChoice) -> StressorInfo {
    use StressorChoice as S;
    match choice {
        // -- CPU -------------------------------------------------------------
        S::Cpu => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Float-op burst loop across every core.",
            when: "You want a quick all-core load and thermal check.",
            pass: Some("≥ 50 Mop/s sustained"),
            caveat: None,
        },
        S::Fp => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "FMA chains — the hottest per-core load in the kit.",
            when: "Checking a cooler mount or chasing a thermal complaint.",
            pass: Some("≥ 200 Mflop/s sustained"),
            caveat: None,
        },
        S::Matrix => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Square f32 matrix multiply.",
            when: "SIMD/FP throughput looks low for the part.",
            pass: Some("≥ 200 Mflop/s sustained"),
            caveat: None,
        },
        S::Linpack => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "LU solves with a residual check after every solve.",
            when: "You need heavy FP load with correctness actually verified.",
            pass: Some("All residuals under the HPL threshold (16)"),
            caveat: None,
        },
        S::CpuVerify => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Runs the same work twice and compares digests.",
            when: "Results are wrong but no WHEA errors are logged.",
            pass: Some("Zero digest divergences"),
            caveat: None,
        },
        S::Bitops => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Hot-loop popcount, count-leading-zeros, and rotates.",
            when: "Integer results look wrong.",
            pass: Some("≥ 50 Mop/s sustained"),
            caveat: None,
        },
        S::Branch => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Data-dependent branches the predictor cannot learn.",
            when: "A fault needs unpredictable control flow to show up.",
            pass: Some("≥ 100 Mbranch/s sustained"),
            caveat: None,
        },
        S::Prime => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Prime sieve at low memory pressure.",
            when: "You want a light long soak that can sit on the bench.",
            pass: Some("≥ 0.5 Mprime/s sustained"),
            caveat: None,
        },
        S::Hash => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "FNV-1a over a 1 MiB buffer.",
            when: "You want steady mixed integer and memory load.",
            pass: Some("≥ 50 MiB/s sustained"),
            caveat: None,
        },
        S::Cache => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Cache-line thrash with explicit clflush.",
            when: "Instability tracks cache or fabric clocks.",
            pass: Some("≥ 50 Mref/s sustained"),
            caveat: None,
        },
        S::Prefetch => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Sequential and striped reads against the prefetcher.",
            when: "Sequential memory reads underperform.",
            pass: Some("≥ 50 Mref/s sustained"),
            caveat: None,
        },
        S::Icache => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Indirect calls through a 64-function table.",
            when: "Crashes look like instruction-fetch faults.",
            pass: Some("≥ 5 Mcall/s sustained"),
            caveat: None,
        },
        S::Tsc => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "rdtsc read-rate loop.",
            when: "Timers or timestamps drift.",
            pass: Some("≥ 5 Mread/s sustained"),
            caveat: None,
        },
        S::Atomic => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Every core contending on one AtomicU64.",
            when: "Multi-core synchronisation faults or hangs.",
            pass: Some("≥ 5 Mop/s sustained"),
            caveat: None,
        },
        S::Mutex => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Threads contending on a single mutex.",
            when: "Lock-heavy workloads stall.",
            pass: Some("≥ 0.5 Mop/s sustained"),
            caveat: None,
        },
        S::Switch => StressorInfo {
            subsystem: Subsystem::Cpu,
            what: "Threads ping-ponging on condvars to force context switches.",
            when: "The machine lags with many threads running.",
            pass: Some("≥ 0.05 Mctxsw/s sustained"),
            caveat: None,
        },

        // -- Memory ----------------------------------------------------------
        S::MemTest => StressorInfo {
            subsystem: Subsystem::Memory,
            what: "MemTest86-style pattern write then verify.",
            when: "You suspect bad RAM.",
            pass: Some("Zero mismatches across all patterns"),
            caveat: Some("Only covers the heap it can allocate, not memory held by the OS."),
        },
        S::Memory => StressorInfo {
            subsystem: Subsystem::Memory,
            what: "Heap fill and touch up to the configured RAM cap.",
            when: "Pressuring capacity — this does not verify data.",
            pass: Some("≥ 2 MiB/s sustained"),
            caveat: Some(
                "Raise the cap with care: a cap near total RAM on a machine with a small page file will make the whole OS unresponsive.",
            ),
        },
        S::Stream => StressorInfo {
            subsystem: Subsystem::Memory,
            what: "STREAM copy, scale, add, and triad.",
            when: "RAM bandwidth or channel setup is the suspect.",
            pass: Some("≥ 5 GB/s sustained"),
            caveat: Some("Well below the rated figure usually means single-channel or a missing XMP/EXPO profile."),
        },
        S::Memcpy => StressorInfo {
            subsystem: Subsystem::Memory,
            what: "Bulk memcpy between paired buffers.",
            when: "Large copies or file operations are slow.",
            pass: Some("≥ 2 GB/s sustained"),
            caveat: None,
        },
        S::Vm => StressorInfo {
            subsystem: Subsystem::Memory,
            what: "Page-touch churn across the working set.",
            when: "A low-RAM machine feels slow.",
            pass: Some("≥ 200 MiB/s sustained"),
            caveat: None,
        },

        // -- Storage ---------------------------------------------------------
        S::Disk => StressorInfo {
            subsystem: Subsystem::Storage,
            what: "Temp-file write and read loop.",
            when: "A drive is slow, dropping out, or logging errors.",
            pass: Some("≥ 5 MiB/s sustained; any I/O error fails the run"),
            caveat: Some("Writes to the system drive unless the run is pointed elsewhere."),
        },

        // -- GPU -------------------------------------------------------------
        S::Gpu => StressorInfo {
            subsystem: Subsystem::Gpu,
            what: "Compute-shader FMA and scattered-load hammer.",
            when: "A GPU hangs, resets, or black-screens under load.",
            pass: Some("≥ 100 GFLOPS sustained; no TDR"),
            caveat: Some("A weak or old card can miss the floor while being perfectly healthy — read the throughput, not just pass/fail."),
        },
        S::GpuMatmul => StressorInfo {
            subsystem: Subsystem::Gpu,
            what: "GPU NxN fp32 matrix multiply.",
            when: "GPU throughput looks low for the card.",
            pass: Some("≥ 100 GFLOPS sustained; no TDR"),
            caveat: None,
        },
        S::GpuVram => StressorInfo {
            subsystem: Subsystem::Gpu,
            what: "VRAM write-verify pattern walker.",
            when: "Artifacts or VRAM corruption are suspected.",
            pass: Some("≥ 1000 MiB/s sustained; zero mismatches"),
            caveat: None,
        },
        S::GpuPcie => StressorInfo {
            subsystem: Subsystem::Gpu,
            what: "CPU↔GPU buffer round-trip with full readback verify.",
            when: "The link drops or data comes back bad.",
            pass: Some("≥ 1 GB/s sustained; zero mismatches"),
            caveat: Some("Well under 1 GB/s can mean the card negotiated fewer lanes than it should."),
        },
        S::GpuDisplay => StressorInfo {
            subsystem: Subsystem::Gpu,
            what: "A real swapchain per attached output, presented continuously with periodic surface reconfiguration and desktop mode changes.",
            when: "A machine black-screens, drops a monitor, or hangs on a mode change rather than under compute load.",
            pass: Some("Sustained present rate with no present timeouts, lost surfaces, or watchdog live dumps"),
            caveat: Some("The only stressor that exercises the display and flip-queue path. With a single output attached the result is weak — attach a second display to make it meaningful."),
        },

        // -- Power / whole-system -------------------------------------------
        S::Psu => StressorInfo {
            subsystem: Subsystem::Power,
            what: "All-core FMA plus GPU compute at the same time.",
            when: "A machine dies only under heavy load.",
            pass: Some("Run completes; no WHEA delta; no thermal runaway"),
            caveat: Some("With no GPU present this degrades to a CPU-only load and proves much less."),
        },
        S::PsuTransient => StressorInfo {
            subsystem: Subsystem::Power,
            what: "Square-wave GPU bursts over a continuous all-core CPU load to drive 12V rail transients.",
            when: "A PC dies under load spikes but survives steady load.",
            pass: Some("Run completes; no WHEA delta; no rail droop or shutdown across the load steps"),
            caveat: Some("A reboot or power-off mid-run is the finding, not a tool failure. With no GPU present no transient is generated."),
        },
        S::Combined => StressorInfo {
            subsystem: Subsystem::Power,
            what: "Fused CPU, RAM, and GPU load in one stressor.",
            when: "You want whole-system load and bench time is short.",
            pass: Some("Runs to completion with no errors or TDR"),
            caveat: None,
        },
    }
}

/// What a run mode does and when to choose it.
#[derive(Clone, Copy, Debug)]
pub struct ModeInfo {
    pub label: &'static str,
    pub what: &'static str,
    pub when: &'static str,
}

pub fn mode_info(mode: PanelMode) -> ModeInfo {
    match mode {
        PanelMode::Single => ModeInfo {
            label: "Single",
            what: "One stressor, one set of knobs.",
            when: "Isolating a specific subsystem, or re-testing one thing after a swap.",
        },
        PanelMode::Scenario => ModeInfo {
            label: "Scenario",
            what: "Your own ordered list of stages, each with its own stressor and duration.",
            when: "Reproducing a specific sequence, or building a soak that is not a cert tier.",
        },
        PanelMode::QcBenchmark => ModeInfo {
            label: "QC Benchmark",
            what: "Curated 8-stage CPU and RAM burn-in: cpu, matrix, fp, stream, cache, branch, memory, vm.",
            when: "The standard bench-wide burn-in at intake.",
        },
        PanelMode::Certification => ModeInfo {
            label: "Certification",
            what: "Bronze through Platinum tiers with per-stage verdict rules on temps, WHEA, TDR, and throughput stability.",
            when: "Signing a machine off for sale or delivery.",
        },
        PanelMode::Concurrent => ModeInfo {
            label: "Concurrent",
            what: "Several stressors at once, each in its own lane, sharing one duration.",
            when: "A fault only appears with more than one subsystem busy.",
        },
    }
}

/// Cert tier copy, keyed by the preset names in [`crate::CERT_PRESET_NAMES`].
pub fn cert_preset_info(preset: &str) -> Option<ModeInfo> {
    let info = match preset {
        "bronze" => ModeInfo {
            label: "Bronze",
            what: "~1.5h: CPU, RAM, and GPU verified.",
            when: "Minimum sign-off on a stock build.",
        },
        "silver" => ModeInfo {
            label: "Silver",
            what: "~3.5h: CPU, RAM, GPU, and PSU load.",
            when: "Standard sign-off on a gaming build.",
        },
        "gold" => ModeInfo {
            label: "Gold",
            what: "~8h: CPU mix, Linpack, RAM, GPU, VRAM, and PSU.",
            when: "High-end or overclocked builds.",
        },
        "platinum" => ModeInfo {
            label: "Platinum",
            what: "~12h: full CPU, RAM, GPU, disk, and PSU mix.",
            when: "Flagships, or chasing an intermittent fault.",
        },
        "power-virus" | "power_virus" => ModeInfo {
            label: "Power Virus",
            what: "Concurrent CPU and GPU at max load for 30 minutes.",
            when: "Proving the PSU and cooling survive the worst case.",
        },
        _ => return None,
    };
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stressor_has_metadata_and_one_group() {
        for choice in StressorChoice::ALL {
            let info = info_for(choice);
            assert!(!info.what.is_empty(), "{choice:?} missing `what`");
            assert!(!info.when.is_empty(), "{choice:?} missing `when`");

            let groups = Subsystem::ALL
                .iter()
                .filter(|g| g.stressors().contains(&choice))
                .count();
            assert_eq!(groups, 1, "{choice:?} must appear in exactly one subsystem group");
        }
    }

    #[test]
    fn group_membership_matches_declared_subsystem() {
        for group in Subsystem::ALL {
            for choice in group.stressors() {
                assert_eq!(
                    info_for(*choice).subsystem,
                    group,
                    "{choice:?} listed under {group:?} but declares a different subsystem"
                );
            }
        }
    }

    #[test]
    fn groups_cover_every_stressor() {
        let grouped: usize = Subsystem::ALL.iter().map(|g| g.stressors().len()).sum();
        assert_eq!(
            grouped,
            StressorChoice::ALL.len(),
            "subsystem groups must cover StressorChoice::ALL exactly"
        );
    }

    #[test]
    fn cert_preset_names_all_have_copy() {
        for name in crate::CERT_PRESET_NAMES {
            assert!(
                cert_preset_info(name).is_some(),
                "cert preset '{name}' has no technician copy"
            );
        }
    }
}
