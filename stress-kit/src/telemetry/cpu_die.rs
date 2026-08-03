//! CPU die temperature from the processor's own sensor.
//!
//! Intel exposes per-core and package digital thermal sensors as MSRs; AMD Zen
//! exposes Tctl through the SMU over SMN. Both need kernel-mode access, which
//! comes from whichever [`crate::lowlevel`] backend is live — this module owns
//! only the register decode and plausibility limits.

use std::time::{Duration, Instant};

use crate::lowlevel::LowLevelAccess;

use super::{CpuDieReader, CpuDieThermal};

const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const IA32_THERM_STATUS: u32 = 0x19C;
const IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;

/// SMN address of the Zen thermal controller's current-temperature register.
const AMD_SMN_THM_CUR_TEMP: u32 = 0x0005_9800;

const MIN_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Age of the last successful read at which cached temps are dropped.
const STALE_AFTER: Duration = Duration::from_secs(3);
/// Affinity masks are `usize`-wide, so no more cores than that can be pinned.
const MAX_CORES: usize = 64;
/// Floor for a die reading; the degenerate Intel readout (== TjMax) lands on
/// exactly 0 °C, and no powered die sits below a serviceable room's ambient.
const CPU_MIN_PLAUSIBLE_C: f32 = 5.0;
const CPU_MAX_PLAUSIBLE_C: f32 = 125.0;

const _: () = assert!(
    STALE_AFTER.as_millis() >= MIN_POLL_INTERVAL.as_millis(),
    "cache would expire before the next read could refresh it"
);

#[derive(Clone, Copy, PartialEq)]
enum Vendor {
    Intel,
    Amd,
    Other,
}

pub struct CpuDieMonitor {
    access: LowLevelAccess,
    vendor: Vendor,
    tj_max: u32,
    cached: Option<CpuDieThermal>,
    last_polled: Instant,
    /// Timestamp of the last read that produced a die value.
    last_good: Instant,
}

impl CpuDieMonitor {
    /// `None` when the live backend cannot reach this vendor's die sensor, or
    /// when no plausible reading came back on the first try.
    pub fn open(access: LowLevelAccess) -> Option<Self> {
        let vendor = detect_vendor();
        let caps = access.capabilities();
        let reachable = match vendor {
            Vendor::Intel => caps.msr,
            Vendor::Amd => caps.smn,
            Vendor::Other => false,
        };
        if !reachable {
            log::info!(
                "stress-kit/cpu-die: no die sensor path on this backend for this CPU; \
                 CPU die temperature unavailable"
            );
            return None;
        }

        let mut me = Self {
            access,
            vendor,
            tj_max: 100,
            cached: None,
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
            last_good: Instant::now(),
        };
        if vendor == Vendor::Intel {
            me.tj_max = me.read_tjmax();
        }
        me.cached = me.read_all();
        me.last_good = Instant::now();
        match me.cached.as_ref() {
            Some(die) => log::info!(
                "stress-kit/cpu-die: {:?} sensor, package {:?}, {} per-core value(s)",
                die.reader,
                die.package_c,
                die.core_temp_count()
            ),
            None => log::warn!(
                "stress-kit/cpu-die: backend is live but no plausible CPU die reading; temps stay \
                 absent"
            ),
        }
        Some(me)
    }

    /// Latest die readings, throttled to [`MIN_POLL_INTERVAL`]. A failed read
    /// keeps the prior cache only until it is [`STALE_AFTER`] old; once the
    /// backend is proven gone the cache is dropped for the rest of the run.
    pub fn poll(&mut self) -> Option<CpuDieThermal> {
        self.drop_stale_cache();
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        match self.read_all() {
            Some(die) => {
                self.last_good = Instant::now();
                self.cached = Some(die);
            }
            None => {
                if self.access.confirm_lost("CPU die read") {
                    self.cached = None;
                } else {
                    self.drop_stale_cache();
                }
            }
        }
        self.cached.clone()
    }

    /// Clears the cache when the last successful read is [`STALE_AFTER`] old.
    fn drop_stale_cache(&mut self) {
        let age = self.last_good.elapsed();
        if self.cached.is_none() || age < STALE_AFTER {
            return;
        }
        log::warn!(
            "stress-kit/cpu-die: no CPU die reading for {age:.1?} while the backend still answers; \
             dropping cached temps instead of republishing them"
        );
        self.cached = None;
    }

    fn read_all(&self) -> Option<CpuDieThermal> {
        match self.vendor {
            Vendor::Intel => self.read_intel(),
            Vendor::Amd => self.read_amd(),
            Vendor::Other => None,
        }
    }

    /// Package DTS plus one DTS read per logical core; a core whose sensor does
    /// not answer stays `None` in its own slot.
    fn read_intel(&self) -> Option<CpuDieThermal> {
        let msr = self.access.msr()?;
        let package_c =
            dts_temp(msr.read_msr(IA32_PACKAGE_THERM_STATUS), self.tj_max).and_then(plausible_cpu_temp);
        let count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(MAX_CORES);
        let cores: Vec<Option<f32>> = (0..count)
            .map(|core| {
                dts_temp(msr.read_msr_on(core, IA32_THERM_STATUS), self.tj_max)
                    .and_then(plausible_cpu_temp)
            })
            .collect();

        let any_core = cores.iter().any(Option::is_some);
        if package_c.is_none() && !any_core {
            return None;
        }
        Some(CpuDieThermal {
            package_c,
            cores: if any_core { cores } else { Vec::new() },
            reader: CpuDieReader::IntelDts,
        })
    }

    fn read_amd(&self) -> Option<CpuDieThermal> {
        Some(CpuDieThermal {
            package_c: Some(self.read_amd_tctl()?),
            cores: Vec::new(),
            reader: CpuDieReader::AmdTctl,
        })
    }

    fn read_tjmax(&self) -> u32 {
        match self.access.msr().and_then(|m| m.read_msr(MSR_TEMPERATURE_TARGET)) {
            Some(v) => tjmax_from_msr(v),
            None => 100,
        }
    }

    fn read_amd_tctl(&self) -> Option<f32> {
        let raw = self.access.smn()?.read_smn(AMD_SMN_THM_CUR_TEMP)?;
        tctl_temp(raw).and_then(plausible_cpu_temp)
    }
}

/// TjMax from `MSR_TEMPERATURE_TARGET` bits 23:16; 100 °C when the field reads
/// zero.
fn tjmax_from_msr(value: u64) -> u32 {
    let tj = ((value as u32) >> 16) & 0xFF;
    if tj == 0 { 100 } else { tj }
}

/// `TjMax - readout` from a thermal-status MSR value; `None` when the valid bit
/// (31) is clear. Readout is bits 22:16.
fn dts_temp(msr_value: Option<u64>, tj_max: u32) -> Option<f32> {
    let eax = msr_value? as u32;
    if eax & (1 << 31) == 0 {
        return None;
    }
    let readout = (eax >> 16) & 0x7F;
    Some(tj_max.saturating_sub(readout) as f32)
}

/// Zen Tctl from `THM_CUR_TEMP`: bits 31:21 in 0.125 °C steps, less a 49 °C
/// offset when the range-select bits are set. `None` for a dead register.
fn tctl_temp(raw: u32) -> Option<f32> {
    if raw == 0 || raw == 0xFFFF_FFFF {
        return None;
    }
    let steps = (raw >> 21) & 0x7FF;
    let mut temp = steps as f32 * 0.125;
    if raw & 0x8_0000 != 0 || raw & 0x3_0000 != 0 {
        temp -= 49.0;
    }
    Some(temp)
}

/// Drop physically implausible readings (garbage MSR/SMU values — e.g. a VM with
/// no real thermal sensor behind the register, or a readout equal to TjMax).
fn plausible_cpu_temp(t: f32) -> Option<f32> {
    (CPU_MIN_PLAUSIBLE_C..=CPU_MAX_PLAUSIBLE_C)
        .contains(&t)
        .then_some(t)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn detect_vendor() -> Vendor {
    let v = raw_cpuid::CpuId::new()
        .get_vendor_info()
        .map(|v| v.as_str().to_string())
        .unwrap_or_default();
    if v.contains("Intel") {
        Vendor::Intel
    } else if v.contains("AMD") {
        Vendor::Amd
    } else {
        Vendor::Other
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn detect_vendor() -> Vendor {
    Vendor::Other
}

#[cfg(test)]
impl CpuDieMonitor {
    /// Monitor over a scripted backend, bypassing CPUID vendor detection.
    fn for_test(access: LowLevelAccess, vendor: Vendor, tj_max: u32) -> Self {
        Self {
            access,
            vendor,
            tj_max,
            cached: None,
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
            last_good: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lowlevel::mock::MockBackend;

    /// Thermal-status MSR value with the valid bit set and this readout.
    fn therm_status(readout: u32) -> Option<u64> {
        Some(((1u32 << 31) | (readout << 16)) as u64)
    }

    fn access_over(mock: MockBackend) -> LowLevelAccess {
        LowLevelAccess::new(Box::new(mock), "scripted", Vec::new())
    }

    #[test]
    fn a_readout_equal_to_tjmax_publishes_nothing() {
        assert_eq!(dts_temp(therm_status(100), 100), Some(0.0));
        assert_eq!(
            dts_temp(therm_status(100), 100).and_then(plausible_cpu_temp),
            None
        );
        assert_eq!(
            dts_temp(therm_status(127), 100).and_then(plausible_cpu_temp),
            None
        );
    }

    #[test]
    fn a_cleared_valid_bit_publishes_nothing() {
        assert_eq!(dts_temp(Some((30u32 << 16) as u64), 100), None);
        assert_eq!(dts_temp(None, 100), None);
    }

    #[test]
    fn a_real_readout_survives_the_floor() {
        assert_eq!(
            dts_temp(therm_status(30), 100).and_then(plausible_cpu_temp),
            Some(70.0)
        );
        assert_eq!(
            dts_temp(therm_status(0), 100).and_then(plausible_cpu_temp),
            Some(100.0)
        );
    }

    #[test]
    fn implausible_values_are_rejected_at_both_ends() {
        assert_eq!(plausible_cpu_temp(-49.0), None);
        assert_eq!(plausible_cpu_temp(0.0), None);
        assert_eq!(plausible_cpu_temp(4.9), None);
        assert_eq!(plausible_cpu_temp(5.0), Some(5.0));
        assert_eq!(plausible_cpu_temp(125.0), Some(125.0));
        assert_eq!(plausible_cpu_temp(125.1), None);
    }

    #[test]
    fn tjmax_falls_back_when_the_field_is_zero() {
        assert_eq!(tjmax_from_msr(0), 100);
        assert_eq!(tjmax_from_msr(100 << 16), 100);
        assert_eq!(tjmax_from_msr(105 << 16), 105);
        // Bits above 23 must not leak into the field.
        assert_eq!(tjmax_from_msr(0xFFFF_0000_0069_0000), 0x69);
    }

    /// Steps are bits 31:21 in 0.125 °C units.
    fn thm_cur_temp(steps: u32) -> u32 {
        steps << 21
    }

    #[test]
    fn tctl_decodes_without_the_range_offset() {
        assert_eq!(tctl_temp(thm_cur_temp(400)), Some(50.0));
        assert_eq!(tctl_temp(thm_cur_temp(720)), Some(90.0));
    }

    #[test]
    fn tctl_applies_the_offset_when_either_range_bit_is_set() {
        assert_eq!(tctl_temp(thm_cur_temp(800) | 0x8_0000), Some(51.0));
        assert_eq!(tctl_temp(thm_cur_temp(800) | 0x1_0000), Some(51.0));
        assert_eq!(tctl_temp(thm_cur_temp(800) | 0x2_0000), Some(51.0));
    }

    #[test]
    fn a_dead_smn_register_publishes_nothing() {
        assert_eq!(tctl_temp(0), None);
        assert_eq!(tctl_temp(0xFFFF_FFFF), None);
        // A raw value whose offset drags it under the floor is dropped too.
        assert_eq!(tctl_temp(thm_cur_temp(8) | 0x8_0000).and_then(plausible_cpu_temp), None);
    }

    /// Each core must be read on its own logical processor, not sampled once and
    /// copied — a backend that ignored the target would return one value here.
    #[test]
    fn intel_reads_each_core_on_its_own_processor() {
        let mock = MockBackend::full();
        mock.msrs
            .lock()
            .unwrap()
            .insert((0, IA32_PACKAGE_THERM_STATUS), therm_status(10).unwrap());
        for cpu in 0..MAX_CORES {
            mock.msrs
                .lock()
                .unwrap()
                .insert((cpu, IA32_THERM_STATUS), therm_status(20 + cpu as u32).unwrap());
        }
        let monitor = CpuDieMonitor::for_test(access_over(mock), Vendor::Intel, 100);

        let die = monitor.read_intel().expect("no die reading");
        assert_eq!(die.reader, CpuDieReader::IntelDts);
        assert_eq!(die.package_c, Some(90.0));
        assert_eq!(die.core_c(0), Some(80.0));
        assert_eq!(die.core_c(1), Some(79.0));
        assert_eq!(die.cores.len(), std::thread::available_parallelism().unwrap().get().min(MAX_CORES));
    }

    /// A package sensor that answers still publishes when no core does.
    #[test]
    fn intel_publishes_package_without_any_core() {
        let mock = MockBackend::full();
        mock.msrs
            .lock()
            .unwrap()
            .insert((0, IA32_PACKAGE_THERM_STATUS), therm_status(30).unwrap());
        let monitor = CpuDieMonitor::for_test(access_over(mock), Vendor::Intel, 100);

        let die = monitor.read_intel().expect("no die reading");
        assert_eq!(die.package_c, Some(70.0));
        assert!(die.cores.is_empty(), "no core answered, so no core slots publish");
    }

    #[test]
    fn amd_reads_tctl_over_smn() {
        let mock = MockBackend::full().with_smn(AMD_SMN_THM_CUR_TEMP, thm_cur_temp(560));
        let monitor = CpuDieMonitor::for_test(access_over(mock), Vendor::Amd, 100);

        let die = monitor.read_amd().expect("no die reading");
        assert_eq!(die.reader, CpuDieReader::AmdTctl);
        assert_eq!(die.package_c, Some(70.0));
        assert!(die.cores.is_empty(), "Zen exposes no per-core sensor");
    }

    /// A backend that stops answering ends the readings instead of republishing
    /// the last cached value.
    #[test]
    fn a_lost_backend_stops_publishing() {
        let mock = MockBackend::full().with_smn(AMD_SMN_THM_CUR_TEMP, thm_cur_temp(560));
        mock.fail_after.store(1, std::sync::atomic::Ordering::Relaxed);
        let access = access_over(mock);
        let mut monitor = CpuDieMonitor::for_test(access.clone(), Vendor::Amd, 100);

        assert_eq!(monitor.read_all().and_then(|d| d.package_c), Some(70.0));
        monitor.cached = Some(CpuDieThermal {
            package_c: Some(70.0),
            cores: Vec::new(),
            reader: CpuDieReader::AmdTctl,
        });
        monitor.last_polled = Instant::now() - MIN_POLL_INTERVAL;

        assert_eq!(monitor.poll(), None, "a dead backend must not replay its cache");
        assert!(access.status().lost.is_some(), "loss was not latched");
    }
}
