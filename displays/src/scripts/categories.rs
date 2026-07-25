//! Script category definitions with all available scripts

use super::{ScriptCategory, ScriptItem};
use std::collections::HashMap;

/// Returns all default tuneup scripts
pub fn tuneup_scripts() -> Vec<ScriptItem> {
    vec![
        ScriptItem::new("Data Transfer", ScriptCategory::Tuneup)
            .with_description("Transfer user data to a backup location"),
        ScriptItem::new("Activate CPS", ScriptCategory::Tuneup)
            .with_description("Install and activate Webroot and SuperAntiSpyware"),
        ScriptItem::new("Activate SEB", ScriptCategory::Tuneup)
            .with_description("Install and activate SuperEasyBackup"),
        ScriptItem::new("Install Windows Updates", ScriptCategory::Tuneup)
            .with_description("Check for and install Windows updates"),
        ScriptItem::new("Disable Sleep / Hibernation", ScriptCategory::Tuneup)
            .with_description("Disable sleep and hibernation power settings"),
        ScriptItem::new("Run SuperAntiSpyware Scan", ScriptCategory::Tuneup)
            .with_description("Run a full scan with SuperAntiSpyware"),
        ScriptItem::new("Run Webroot Scan", ScriptCategory::Tuneup)
            .with_description("Run a full scan with Webroot"),
        ScriptItem::new("Run Junkware Category", ScriptCategory::Tuneup)
            .with_description("Remove all known junkware applications"),
        ScriptItem::new("Install LibreOffice", ScriptCategory::Tuneup)
            .with_description("Install LibreOffice via Ninite"),
        ScriptItem::new("Disable proxy settings", ScriptCategory::Tuneup)
            .with_description("Disable any configured proxy settings"),
        ScriptItem::new("Disable Notifications", ScriptCategory::Tuneup)
            .with_description("Disable Windows notifications and suggestions"),
        ScriptItem::new("Change SuperAntiSpyware settings", ScriptCategory::Tuneup)
            .with_description("Configure SuperAntiSpyware scheduled tasks"),
        ScriptItem::new("Disable Startup Apps", ScriptCategory::Tuneup)
            .with_description("Disable unnecessary startup applications"),
        ScriptItem::new("Unpin Copilot", ScriptCategory::Tuneup)
            .with_description("Remove Copilot from taskbar"),
        ScriptItem::new("Align Taskbar to left", ScriptCategory::Tuneup)
            .with_description("Align Windows 11 taskbar to the left"),
        ScriptItem::new("Change Timezone to Mountain", ScriptCategory::Tuneup)
            .with_description("Set system timezone to Mountain Standard Time"),
        ScriptItem::new("Disable BitLocker", ScriptCategory::Tuneup)
            .with_description("Detect and disable BitLocker encryption on all drives"),
    ]
}

/// Returns all stress test scripts (singles + presets).
///
/// Persisted via `stress-runner`: every entry creates `stress_test_run`,
/// `stress_test_event`, `stress_test_metric`, and `hardware_component` rows.
pub fn stress_tests_scripts() -> Vec<ScriptItem> {
    vec![
        // Presets
        ScriptItem::new("Cert: Bronze", ScriptCategory::StressTests)
            .with_description("Bronze certification (~1.5h): CPU FMA, CPU verify, 80% RAM pattern-verify, GPU compute + matmul")
            .with_pass_criteria("All stages pass the cert rules (temps, WHEA, throughput stability)")
            .with_error_criteria("Any rule violation, WHEA delta, or stressor error"),
        ScriptItem::new("Cert: Silver", ScriptCategory::StressTests)
            .with_description("Silver certification (~3.5h): CPU FMA + matrix, CPU verify, 80% RAM pattern-verify, GPU compute + matmul, PSU combined load")
            .with_pass_criteria("All stages pass the cert rules (temps, WHEA, throughput stability)")
            .with_error_criteria("Any rule violation, WHEA delta, or stressor error"),
        ScriptItem::new("Cert: Gold", ScriptCategory::StressTests)
            .with_description("Gold certification (~8h): CPU FMA/matrix/integer/verify, Linpack, 80% RAM pattern-verify, GPU compute + matmul, 80% VRAM verify, PSU combined load")
            .with_pass_criteria("All stages pass the cert rules (temps, WHEA, throughput stability)")
            .with_error_criteria("Any rule violation, WHEA delta, or stressor error"),
        ScriptItem::new("Cert: Platinum", ScriptCategory::StressTests)
            .with_description("Platinum certification (~12h): extended CPU mix, Linpack, 2h RAM pattern-verify, GPU compute/matmul/VRAM/PCIe, disk, PSU combined load")
            .with_pass_criteria("All stages pass the cert rules (temps, WHEA, throughput stability)")
            .with_error_criteria("Any rule violation, WHEA delta, or stressor error"),
        ScriptItem::new("Power Virus", ScriptCategory::StressTests)
            .with_description("Power virus (30m): concurrent CPU FMA + GPU compute at max load for PSU and thermal validation")
            .with_pass_criteria("Run completes within temp limits; no WHEA, no TDR")
            .with_error_criteria("Any rule violation, WHEA delta, or TDR event"),
        ScriptItem::new("GPU Stress Test", ScriptCategory::StressTests)
            .with_description("4-stage GPU probe: compute → matmul → VRAM write-verify → PCIe round-trip")
            .with_pass_criteria("All stages above throughput floors; no TDR, no ECC errors, no VRAM mismatches")
            .with_warning_criteria("Corrected ECC errors or PCIe replay deltas")
            .with_error_criteria("Uncorrected ECC, new TDR event, or VRAM mismatch"),
        ScriptItem::new("QC Benchmark", ScriptCategory::StressTests)
            .with_description("8-stage CPU/memory burn-in: cpu → matrix → fp → stream → cache → branch → memory → vm")
            .with_pass_criteria("Every stage above qc_floor_for(stressor)")
            .with_warning_criteria("Stage throughput between 0.9× and 1.0× floor")
            .with_error_criteria("Any stage below floor, WHEA delta, or BSOD"),
        // Verified tests (count real data errors, not just load)
        ScriptItem::new("Memory Test", ScriptCategory::StressTests)
            .with_description("RAM pattern write/verify: moving inversions, walking ones, address-in-address, random (MemTest86-style); reports MiB/s")
            .with_pass_criteria("Zero mismatches across all patterns")
            .with_error_criteria("Any data mismatch — treat as faulty RAM"),
        ScriptItem::new("Stress: CPU Verify", ScriptCategory::StressTests)
            .with_description("Deterministic integer+FP workload executed twice per seed with digest compare; reports Mop/s")
            .with_pass_criteria("Zero digest divergences")
            .with_error_criteria("Any divergence — silent data corruption under load"),
        ScriptItem::new("Stress: Linpack", ScriptCategory::StressTests)
            .with_description("Repeated LU solves with partial pivoting + HPL residual check; reports GFLOPS")
            .with_pass_criteria("All residuals under the HPL threshold (16)")
            .with_error_criteria("Any residual breach — compute error under load"),
        ScriptItem::new("Stress: PSU", ScriptCategory::StressTests)
            .with_description("All-core FMA chains + GPU compute shader simultaneously for max power draw; reports combined GFLOPS")
            .with_pass_criteria("Run completes; no WHEA delta; no thermal runaway")
            .with_warning_criteria("No GPU present — CPU-only load")
            .with_error_criteria("WHEA events or reboot during the run"),
        ScriptItem::new("Stress: PSU Transient", ScriptCategory::StressTests)
            .with_description("Square-wave load step: all-core FMA held steady while the GPU pulses 100 ms on / 100 ms off to hammer the rails with repeated transients; reports combined GFLOPS (~half of steady-state PSU by design)")
            .with_pass_criteria("Run completes; no WHEA delta; no rail droop or shutdown across the load steps")
            .with_warning_criteria("No GPU present — CPU-only load, so no transient is generated")
            .with_error_criteria("Reboot, shutdown, or power-off during the run (an unstable rail under load stepping — the point of this test), WHEA events, or a TDR"),
        // CPU singles
        ScriptItem::new("Stress: CPU", ScriptCategory::StressTests)
            .with_description("Float-op burst loop; reports Mop/s")
            .with_pass_criteria("≥ 50 Mop/s sustained"),
        ScriptItem::new("Stress: Matrix", ScriptCategory::StressTests)
            .with_description("Square f32 matmul; reports Mflop/s")
            .with_pass_criteria("≥ 200 Mflop/s sustained"),
        ScriptItem::new("Stress: FP/FMA", ScriptCategory::StressTests)
            .with_description("Independent FMA chains; reports Mflop/s")
            .with_pass_criteria("≥ 200 Mflop/s sustained"),
        ScriptItem::new("Stress: Bitops", ScriptCategory::StressTests)
            .with_description("Hot-loop popcount/ctlz/cttz/rotate; reports Mop/s")
            .with_pass_criteria("≥ 50 Mop/s sustained"),
        ScriptItem::new("Stress: Branch", ScriptCategory::StressTests)
            .with_description("Data-dependent branches to fuzz the branch predictor; reports Mbranch/s")
            .with_pass_criteria("≥ 100 Mbranch/s sustained"),
        ScriptItem::new("Stress: Prime", ScriptCategory::StressTests)
            .with_description("Sieve of Eratosthenes; reports Mprime/s")
            .with_pass_criteria("≥ 0.5 Mprime/s sustained"),
        ScriptItem::new("Stress: Hash", ScriptCategory::StressTests)
            .with_description("FNV-1a hashing over 1 MiB buffer; reports MiB/s")
            .with_pass_criteria("≥ 50 MiB/s sustained"),
        // Cache / pipeline
        ScriptItem::new("Stress: Cache", ScriptCategory::StressTests)
            .with_description("Cache-line thrash with prefetch/clflush; reports Mref/s")
            .with_pass_criteria("≥ 50 Mref/s sustained"),
        ScriptItem::new("Stress: Prefetch", ScriptCategory::StressTests)
            .with_description("Sequential + striped reads exercising the HW prefetcher; reports Mref/s")
            .with_pass_criteria("≥ 50 Mref/s sustained"),
        ScriptItem::new("Stress: I-Cache", ScriptCategory::StressTests)
            .with_description("Indirect calls through a 64-fn table; reports Mcall/s")
            .with_pass_criteria("≥ 5 Mcall/s sustained"),
        ScriptItem::new("Stress: TSC", ScriptCategory::StressTests)
            .with_description("rdtsc read rate; reports Mread/s")
            .with_pass_criteria("≥ 5 Mread/s sustained"),
        // Concurrency
        ScriptItem::new("Stress: Atomic", ScriptCategory::StressTests)
            .with_description("Many cores fighting over one AtomicU64; reports Mop/s")
            .with_pass_criteria("≥ 5 Mop/s sustained"),
        ScriptItem::new("Stress: Mutex", ScriptCategory::StressTests)
            .with_description("Many threads contending on a single mutex; reports Mop/s")
            .with_pass_criteria("≥ 0.5 Mop/s sustained"),
        ScriptItem::new("Stress: Context Switch", ScriptCategory::StressTests)
            .with_description("Paired threads ping-ponging on condvars; reports Mctxsw/s")
            .with_pass_criteria("≥ 0.05 Mctxsw/s sustained"),
        // Memory
        ScriptItem::new("Stress: Memory", ScriptCategory::StressTests)
            .with_description("Heap fill/touch under StressConfig::memory_cap_mb; reports MiB/s")
            .with_pass_criteria("≥ 2 MiB/s sustained"),
        ScriptItem::new("Stress: Memcpy", ScriptCategory::StressTests)
            .with_description("Bulk memcpy of paired buffers; reports GB/s")
            .with_pass_criteria("≥ 2 GB/s sustained"),
        ScriptItem::new("Stress: Stream", ScriptCategory::StressTests)
            .with_description("STREAM-style copy/scale/add/triad; reports GB/s")
            .with_pass_criteria("≥ 5 GB/s sustained"),
        ScriptItem::new("Stress: VM", ScriptCategory::StressTests)
            .with_description("Page-touch + churn to pressure working set / page file; reports MiB/s")
            .with_pass_criteria("≥ 200 MiB/s sustained"),
        // Disk
        ScriptItem::new("Stress: Disk", ScriptCategory::StressTests)
            .with_description("Temp-file write/read under StressConfig::disk_file_mb; reports MiB/s")
            .with_pass_criteria("≥ 5 MiB/s sustained")
            .with_error_criteria("Any I/O error during the run"),
        // GPU singles
        ScriptItem::new("Stress: GPU Compute", ScriptCategory::StressTests)
            .with_description("GPU compute-shader FMA + scattered-load hammer; reports GFLOPS")
            .with_pass_criteria("≥ 100 GFLOPS sustained; no TDR")
            .with_error_criteria("TDR event or shader compile failure"),
        ScriptItem::new("Stress: GPU Matmul", ScriptCategory::StressTests)
            .with_description("GPU NxN fp32 matmul; reports GFLOPS")
            .with_pass_criteria("≥ 100 GFLOPS sustained; no TDR")
            .with_error_criteria("TDR event during the run"),
        ScriptItem::new("Stress: GPU VRAM", ScriptCategory::StressTests)
            .with_description("GPU VRAM write-verify pattern walker; reports MiB/s, mismatches via last_error")
            .with_pass_criteria("≥ 1000 MiB/s sustained; no mismatches")
            .with_error_criteria("VRAM mismatch surfaced via last_error"),
        ScriptItem::new("Stress: GPU PCIe", ScriptCategory::StressTests)
            .with_description("CPU↔GPU buffer round-trip with full readback verify; reports GB/s, mismatches counted")
            .with_pass_criteria("≥ 1 GB/s sustained; zero mismatches")
            .with_warning_criteria("PCIe replay deltas during the run")
            .with_error_criteria("Any round-trip data mismatch"),
        // Whole-system combined load
        ScriptItem::new("Stress: Combined", ScriptCategory::StressTests)
            .with_description("Single fused stressor: CPU FMA + RAM bandwidth + GPU compute at once; reports combined CPU+GPU GFLOPS")
            .with_pass_criteria("Runs to completion with no errors or TDR")
            .with_error_criteria("GPU device loss or any reported error"),
        ScriptItem::new("Concurrent: CPU+RAM+GPU", ScriptCategory::StressTests)
            .with_description("Runs CPU, RAM, and GPU stressors simultaneously as independent lanes, each with its own throughput")
            .with_pass_criteria("All lanes run to completion with no errors")
            .with_error_criteria("Any lane reports errors or aborts"),
        // Scored benchmarks — each persists a benchmark_result row (plus the
        // backing stress_test_run) for cross-machine score comparison.
        ScriptItem::new("Benchmark Suite", ScriptCategory::StressTests)
            .with_description("Standard scored suite: cpu single/multi, matrix, linpack, memory bandwidth, memcpy, latency ladder, disk (+ GPU kinds when present); ~15 s each")
            .with_pass_criteria("All benchmarks complete with zero errors; scores persisted")
            .with_warning_criteria("Kind reporting status no_samples (zero throughput ticks; row persisted unscored, suite still passes)")
            .with_error_criteria("Any benchmark errors"),
        ScriptItem::new("Benchmark: CPU Single", ScriptCategory::StressTests)
            .with_description("Single-thread FMA throughput score (Mflop/s)"),
        ScriptItem::new("Benchmark: CPU Multi", ScriptCategory::StressTests)
            .with_description("All-thread FMA throughput score (Mflop/s)"),
        ScriptItem::new("Benchmark: Matrix Single", ScriptCategory::StressTests)
            .with_description("Single-thread matmul score (Mflop/s)"),
        ScriptItem::new("Benchmark: Matrix Multi", ScriptCategory::StressTests)
            .with_description("All-thread matmul score (Mflop/s)"),
        ScriptItem::new("Benchmark: Linpack", ScriptCategory::StressTests)
            .with_description("LU-solve GFLOPS score with residual verification")
            .with_error_criteria("Any residual breach during measurement"),
        ScriptItem::new("Benchmark: Memory Bandwidth", ScriptCategory::StressTests)
            .with_description("STREAM copy/scale/add/triad bandwidth score (GB/s)"),
        ScriptItem::new("Benchmark: Memcpy", ScriptCategory::StressTests)
            .with_description("Bulk memcpy bandwidth score (GB/s)"),
        ScriptItem::new("Benchmark: Memory Latency", ScriptCategory::StressTests)
            .with_description("Pointer-chase ladder 4 KiB → 128 MiB; score is RAM ns/access (lower is better), full ladder in detail"),
        ScriptItem::new("Benchmark: Disk", ScriptCategory::StressTests)
            .with_description("Temp-file write+sync+read cycle score (MiB/s)")
            .with_error_criteria("Any I/O error during measurement"),
        ScriptItem::new("Benchmark: GPU Compute", ScriptCategory::StressTests)
            .with_description("Compute-shader FMA throughput score (GFLOPS)"),
        ScriptItem::new("Benchmark: GPU Matmul", ScriptCategory::StressTests)
            .with_description("GPU NxN matmul throughput score (GFLOPS)"),
        ScriptItem::new("Benchmark: GPU VRAM", ScriptCategory::StressTests)
            .with_description("VRAM write+verify bandwidth score (MiB/s)")
            .with_error_criteria("Any VRAM mismatch during measurement"),
        ScriptItem::new("Benchmark: GPU PCIe", ScriptCategory::StressTests)
            .with_description("CPU↔GPU verified round-trip bandwidth score (GB/s)")
            .with_error_criteria("Any round-trip mismatch during measurement"),
    ]
}

/// Returns all informational scripts
pub fn informational_scripts() -> Vec<ScriptItem> {
    vec![
        ScriptItem::new("Is SuperEasyBackup installed?", ScriptCategory::Informational)
            .with_pass_criteria("Installed and active")
            .with_warning_criteria("Not installed OR not active")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Is Webroot installed?", ScriptCategory::Informational)
            .with_pass_criteria("Installed and active")
            .with_warning_criteria("Not installed OR not active")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Is SuperAntiSpyware installed?", ScriptCategory::Informational)
            .with_pass_criteria("Installed and active")
            .with_warning_criteria("Not installed OR not active")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Are there scheduled tasks for it?", ScriptCategory::Informational)
            .with_description("Check if SuperAntiSpyware has scheduled tasks"),
        ScriptItem::new("Is Windows Activated?", ScriptCategory::Informational)
            .with_description("Check Windows activation status"),
        ScriptItem::new("Is Hibernation/Sleep enabled?", ScriptCategory::Informational)
            .with_description("Check power settings status"),
        ScriptItem::new("Any Recent Blue Screens?", ScriptCategory::Informational)
            .with_description("Provider-scoped 30-day scan for bugchecks, Kernel-Power 41 resets, WHEA errors, TDRs and crash dumps")
            .with_pass_criteria("No bugcheck record, no Kernel-Power 41 with a bugcheck code, no fatal WHEA, no TDR, no crash dumps in the window")
            .with_warning_criteria("Kernel-Power 41 with BugcheckCode 0 (power loss / hard reset, not a BSOD), EventLog 6008, TDR, corrected WHEA, or an incomplete event query")
            .with_error_criteria("Any bugcheck record (WER-SystemErrorReporting / BugCheck 1001), Kernel-Power 41 with a non-zero bugcheck code, fatal WHEA, or a crash dump in the window"),
        ScriptItem::new("When Was The Last Service Date?", ScriptCategory::Informational)
            .with_description("Query last service date from database"),
        ScriptItem::new("Windows Version", ScriptCategory::Informational)
            .with_pass_criteria("Windows 11")
            .with_warning_criteria("Windows 10")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Check Updates", ScriptCategory::Informational)
            .with_description("Check for available Windows updates"),
        ScriptItem::new("Run Prechecks", ScriptCategory::Informational)
            .with_description("Run all informational prechecks"),
    ]
}

/// Returns all junkware removal scripts
pub fn junkware_scripts() -> Vec<ScriptItem> {
    vec![
        ScriptItem::new("OneLaunch", ScriptCategory::JunkwareRemoval)
            .with_description("Remove OneLaunch browser"),
        ScriptItem::new("WebNavigator Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove WebNavigator browser"),
        ScriptItem::new("Wave Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Wave Browser"),
        ScriptItem::new("Clear Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Clear Browser"),
        ScriptItem::new("Shift Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Shift Browser"),
        ScriptItem::new("Avast Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Avast Browser"),
        ScriptItem::new("Mcaffee Safe", ScriptCategory::JunkwareRemoval)
            .with_description("Remove McAfee Safe Search"),
        ScriptItem::new("Driver Support", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Driver Support utility"),
        ScriptItem::new("Winzip", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Winzip"),
        ScriptItem::new("Uninstall Microsoft 365", ScriptCategory::JunkwareRemoval)
            .with_description("Uninstall Microsoft 365 / Office apps"),
        ScriptItem::new("Uninstall OneDrive", ScriptCategory::JunkwareRemoval)
            .with_description("Uninstall Microsoft OneDrive"),
        ScriptItem::new("Disable OneDrive Startup", ScriptCategory::JunkwareRemoval)
            .with_description("Prevent OneDrive from launching at startup"),
        ScriptItem::new("Disable Edge Startup Boost", ScriptCategory::JunkwareRemoval)
            .with_description("Disable Microsoft Edge startup boost and background running"),
    ]
}

/// Get all default script categories with their scripts
pub fn get_all_categories() -> HashMap<ScriptCategory, Vec<ScriptItem>> {
    let mut categories = HashMap::new();
    categories.insert(ScriptCategory::Tuneup, tuneup_scripts());
    categories.insert(ScriptCategory::Informational, informational_scripts());
    categories.insert(ScriptCategory::JunkwareRemoval, junkware_scripts());
    categories.insert(ScriptCategory::StressTests, stress_tests_scripts());
    categories
}

/// Category display order
pub const CATEGORY_ORDER: [ScriptCategory; 4] = [
    ScriptCategory::Tuneup,
    ScriptCategory::Informational,
    ScriptCategory::JunkwareRemoval,
    ScriptCategory::StressTests,
];

/// Get category display name
pub fn category_display_name(category: &ScriptCategory) -> &'static str {
    match category {
        ScriptCategory::Tuneup => "Tuneup / QC",
        ScriptCategory::Informational => "Informational",
        ScriptCategory::JunkwareRemoval => "Junkware Removal",
        ScriptCategory::StressTests => "Stress Tests",
        ScriptCategory::UserScripts(_) => "User Scripts",
        ScriptCategory::Custom(_) => "Custom",
    }
}

/// Get category icon (for egui)
pub fn category_icon(category: &ScriptCategory) -> &'static str {
    match category {
        ScriptCategory::Tuneup => "🔧",
        ScriptCategory::Informational => "ℹ",
        ScriptCategory::JunkwareRemoval => "🗑",
        ScriptCategory::StressTests => "⚡",
        ScriptCategory::UserScripts(_) => "📜",
        ScriptCategory::Custom(_) => "⚙️",
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Every script name stress-runner can execute must be listed in this
    /// catalog, or it never appears in the terminal tab / admin console.
    #[test]
    fn stress_catalog_covers_runner_scripts() {
        let names: Vec<String> = stress_tests_scripts()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        for n in stress_runner::STRESS_SCRIPT_NAMES {
            assert!(
                names.iter().any(|x| x == n),
                "stress script missing from displays catalog: {n}"
            );
        }
        for n in stress_runner::BENCHMARK_SCRIPT_NAMES {
            assert!(
                names.iter().any(|x| x == n),
                "benchmark script missing from displays catalog: {n}"
            );
        }
        assert!(names.iter().any(|x| x == "QC Benchmark"));
    }

    /// Every benchmark script except the suite resolves to a kind.
    #[test]
    fn benchmark_scripts_resolve_to_kinds() {
        for n in stress_runner::BENCHMARK_SCRIPT_NAMES {
            let kind = stress_runner::benchmark_kind_for_script(n);
            if *n == "Benchmark Suite" {
                assert!(kind.is_none());
            } else {
                assert!(kind.is_some(), "no BenchmarkKind for script: {n}");
            }
        }
    }
}
