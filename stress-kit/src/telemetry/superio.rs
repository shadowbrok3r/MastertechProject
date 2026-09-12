//! SuperIO (LPC) board-voltage reader.
//!
//! Probes the 0x2E/0x4E LPC index-data slots once for a Nuvoton hardware-monitor
//! block, then samples it on a throttle. Two register layouts are read, chosen
//! by chip id: [`Family::Classic`] for the NCT67xx bank/register map, confirmed
//! by its 0x5CA3 vendor id, and [`Family::Ec6687`] for the NCT6683/6686/6687
//! EC space common on MSI AMD boards, confirmed by its 3VCC reading because it
//! has no vendor register. Each family decodes to volts itself and the grading
//! below is shared.
//!
//! Rail scaling uses the *conventional* resistor dividers — the real ratios are
//! per-board, so every reading is published with `calibrated: false`. An
//! out-of-nominal rail is reported, and so is a rail that falls below its
//! reportable floor after having once read nominal, so both a sagging and a
//! collapsed supply reach the verdict rules.

use std::time::{Duration, Instant};

use crate::lowlevel::protocol::{HWM_ADDR_OFFSET, HWM_DATA_OFFSET, HWM_WINDOW_LEN};
use crate::lowlevel::{protocol, BusLease, ConfigMode, HwmWindow, LowLevelAccess, LpcAccess, LpcSlot, SuperIoFamily};

use super::VoltageReading;

const CR_LOGICAL_DEVICE: u8 = 0x07;
const CR_CHIP_ID_HIGH: u8 = 0x20;
const CR_CHIP_ID_LOW: u8 = 0x21;
const CR_BASE_HIGH: u8 = 0x60;
const CR_BASE_LOW: u8 = 0x61;

const NUVOTON_HWM_LDN: u8 = 0x0B;

const HWM_BANK_SELECT: u8 = 0x4E;
const HWM_VENDOR_REG: u8 = 0x4F;
const NUVOTON_VENDOR_ID: u16 = 0x5CA3;

/// Voltage channels live at bank 0x04, registers 0x80.. (LHM's 0x480..0x48E).
const VOLTAGE_BANK: u8 = 0x04;
const VOLTAGE_REG_BASE: u8 = 0x80;
const VOLTAGE_CHANNELS: usize = 15;
/// Nuvoton HWM ADC step.
const LSB_VOLTS: f32 = 0.008;

const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Age of the last successful read at which cached rails are dropped.
const STALE_AFTER: Duration = Duration::from_secs(3);
/// Consecutive sub-floor reads before a proven rail publishes as collapsed.
const COLLAPSE_CONFIRM_READS: u8 = 2;

/// Inclusive voltage band.
struct Band {
    min: f32,
    max: f32,
}

impl Band {
    fn holds(&self, volts: f32) -> bool {
        (self.min..=self.max).contains(&volts)
    }
}

/// Grading properties of one rail. These are physical, so every chip family
/// shares them; only where the rail is read differs.
struct Rail {
    label: &'static str,
    /// Values outside this are physically impossible and are discarded.
    reportable: Band,
    /// Values outside this are reported and warned about, not discarded.
    nominal: Band,
}

/// Where one family reads a rail, and the divider applied to it.
struct Channel {
    /// Voltage-bank channel on [`Family::Classic`]; EC-space register on
    /// [`Family::Ec6687`].
    at: u16,
    factor: f32,
}

const RAIL_COUNT: usize = super::RAIL_LABELS.len();

// 3.14 here is a 3.3V rail's lower bound, not an approximation of PI.
#[allow(clippy::approx_constant)]
const RAILS: [Rail; RAIL_COUNT] = [
    Rail {
        label: super::RAIL_LABELS[0],
        reportable: Band { min: 0.05, max: 2.04 },
        nominal: Band { min: 0.50, max: 1.80 },
    },
    Rail {
        label: super::RAIL_LABELS[1],
        reportable: Band { min: 2.00, max: 8.00 },
        nominal: Band { min: 4.75, max: 5.25 },
    },
    // Chip supply, not the board's +3.3V PSU rail; labelled so nothing reads it as one.
    Rail {
        label: super::RAIL_LABELS[2],
        reportable: Band { min: 1.50, max: 4.08 },
        nominal: Band { min: 3.14, max: 3.47 },
    },
    Rail {
        label: super::RAIL_LABELS[3],
        reportable: Band { min: 4.00, max: 20.00 },
        nominal: Band { min: 11.40, max: 12.60 },
    },
    Rail {
        label: super::RAIL_LABELS[4],
        reportable: Band { min: 1.00, max: 4.08 },
        nominal: Band { min: 2.50, max: 3.60 },
    },
];

/// NCT67xx 0x48x layout. Channel assignment and divider are both board-specific
/// in reality, which is why every reading publishes uncalibrated.
const CLASSIC_CHANNELS: [Channel; RAIL_COUNT] = [
    Channel { at: 0, factor: 1.0 },
    Channel { at: 1, factor: 5.0 },
    Channel { at: 3, factor: 2.0 },
    Channel { at: 4, factor: 12.0 },
    Channel { at: 8, factor: 2.0 },
];

/// NCT6687 EC space (LHM's VIN map). The chip reports millivolts at the pin, so
/// only rails sitting behind a board divider carry a factor.
const EC6687_CHANNELS: [Channel; RAIL_COUNT] = [
    Channel { at: 0x124, factor: 1.0 },  // VIN2, Vcore
    Channel { at: 0x122, factor: 5.0 },  // VIN1, +5V
    Channel { at: 0x130, factor: 1.0 },  // 3VCC I/O
    Channel { at: 0x120, factor: 12.0 }, // VIN0, +12V
    Channel { at: 0x13C, factor: 1.0 },  // VBAT
];

const _: () = {
    assert!(
        (GATE_INDEXES[0] as usize) < VOLTAGE_CHANNELS
            && (GATE_INDEXES[1] as usize) < VOLTAGE_CHANNELS
    );
    assert!(
        STALE_AFTER.as_millis() >= POLL_INTERVAL.as_millis(),
        "cache would expire before the next read could refresh it"
    );
    // The gate has to be a rail we actually read, or a chip could pass the
    // liveness check on a register nothing else touches.
    assert!(
        EC6687_CHANNELS[2].at == EC_GATE_REG,
        "the EC liveness gate must be the 3VCC channel"
    );
    let mut i = 0;
    while i < RAIL_COUNT {
        assert!(
            (CLASSIC_CHANNELS[i].at as usize) < VOLTAGE_CHANNELS,
            "rail channel out of bank range"
        );
        assert!(
            RAILS[i].reportable.min <= RAILS[i].nominal.min,
            "nominal band must sit inside the reportable band"
        );
        assert!(
            RAILS[i].nominal.max <= RAILS[i].reportable.max,
            "nominal band must sit inside the reportable band"
        );
        i += 1;
    }
};

/// Chip-internal 3.3V supplies (AVCC, 3VCC) at their family-fixed channels; an
/// implausible pair means the HWM block isn't answering, not that the guessed
/// rail channels are wrong.
const GATE_INDEXES: [u8; 2] = [2, 3];
const GATE_FACTOR: f32 = 2.0;
const GATE_MIN: f32 = 1.50;
const GATE_MAX: f32 = 4.08;

/// EC-space access triplet, at these offsets inside the same monitor window the
/// classic layout uses. The page register doubles as a lock: it reads
/// [`EC_PAGE_RELEASE`] when no tool holds the block.
const EC_PAGE_OFFSET: u8 = 0x04;
const EC_INDEX_OFFSET: u8 = 0x05;
const EC_DATA_OFFSET: u8 = 0x06;
const EC_PAGE_RELEASE: u8 = 0xFF;
/// How long to wait for a peer to release the EC before forcing access.
const EC_ACCESS_WAIT: Duration = Duration::from_millis(500);

/// 3VCC I/O, the chip's own supply, is the EC liveness gate. One channel rather
/// than the classic layout's pair because it is the only rail here that is
/// chip-internal on every board. The band excludes both a floating read
/// (0xFF/0xFF decodes to 4.095 V) and a dead one.
const EC_GATE_REG: u16 = 0x130;
const EC_GATE_MIN: f32 = 2.80;
const EC_GATE_MAX: f32 = 3.60;

/// Which register layout a detected chip speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// NCT67xx: bank select, then 8-bit channels at 0x480.
    Classic,
    /// NCT6683/6686/6687: 16-bit EC space, 12-bit readings in millivolts.
    Ec6687,
}

/// (chip id high, chip id low) pairs sharing the 0x48x voltage register map.
const NUVOTON_CLASSIC: &[(u8, u8, &str)] = &[
    (0xC5, 0x60, "NCT6779D"),
    (0xC8, 0x03, "NCT6791D"),
    (0xC9, 0x11, "NCT6792D"),
    (0xD1, 0x21, "NCT6793D"),
    (0xD3, 0x52, "NCT6795D"),
    (0xD4, 0x23, "NCT6796D"),
    (0xD4, 0x28, "NCT6798D"),
    (0xD4, 0x2A, "NCT6796D-R"),
    (0xD4, 0x2B, "NCT6796D-S"),
    (0xD4, 0x51, "NCT6797D"),
    (0xD8, 0x02, "NCT6799D-R"),
];

/// (chip id high, chip id low) pairs using the NCT6687 EC-space layout. Common
/// on MSI AM4/AM5 boards, which is why AMD builds see no rails without this.
const NUVOTON_EC6687: &[(u8, u8, &str)] = &[
    (0xD5, 0x92, "NCT6687D"),
    (0xD4, 0x92, "NCT6687D-M"),
    (0xC7, 0x32, "NCT6683D"),
];

/// Per-rail plausibility state; transitions are logged once.
#[derive(Clone, Copy, PartialEq)]
enum RailState {
    /// Inside the nominal band; marks the channel proven.
    Ok,
    /// Inside the reportable band but outside nominal; published.
    OutOfNominal,
    /// First sub-floor read on a proven channel, awaiting confirmation; suppressed.
    CollapsePending,
    /// Sub-floor on a proven channel for [`COLLAPSE_CONFIRM_READS`] reads; published.
    Collapsed,
    /// Below the reportable floor on a channel that never read nominal; suppressed.
    Unmapped,
    /// No data, or above the reportable ceiling; suppressed.
    Discarded,
}

impl RailState {
    /// True for the states whose reading is published.
    fn publishes(self) -> bool {
        matches!(self, Self::Ok | Self::OutOfNominal | Self::Collapsed)
    }
}

/// One sample, already scaled to volts per rail. Families decode differently
/// enough that they hand the grader volts rather than raw registers; `None`
/// means that rail did not answer.
type Sample = [Option<f32>; RAIL_COUNT];

pub struct SuperIoMonitor {
    access: LowLevelAccess,
    hwm_base: u16,
    family: Family,
    cached: Vec<VoltageReading>,
    last_polled: Instant,
    /// Timestamp of the last read that produced at least one rail.
    last_good: Instant,
    rail_states: [RailState; RAIL_COUNT],
    /// Per-rail flag set once the channel has read inside its nominal band.
    rail_proven: [bool; RAIL_COUNT],
    /// Consecutive sub-floor reads per rail.
    rail_breaches: [u8; RAIL_COUNT],
}

impl SuperIoMonitor {
    /// Probes both LPC slots once. `None` when the backend has no LPC access,
    /// the bus is unavailable, no supported hardware monitor answers with a
    /// confirmed window, or the confirmed window publishes no rail.
    pub fn open(access: LowLevelAccess) -> Option<Self> {
        if !access.capabilities().any_lpc() {
            return None;
        }
        let detected = {
            let lpc = access.lpc()?;
            match BusLease::acquire(lpc) {
                Some(_bus) => {
                    let hit = LpcSlot::ALL.iter().find_map(|&slot| probe_slot(lpc, slot));
                    if hit.is_none() {
                        log::debug!("stress-kit/superio: no supported SuperIO hardware monitor found");
                    }
                    hit
                }
                None => {
                    log::debug!(
                        "stress-kit/superio: ISA bus unavailable; probe skipped and board \
                         voltages disabled"
                    );
                    None
                }
            }
        };
        let (chip, hwm_base, family) = detected?;

        let mut me = Self {
            access,
            hwm_base,
            family,
            cached: Vec::new(),
            last_polled: Instant::now() - POLL_INTERVAL,
            last_good: Instant::now(),
            rail_states: [RailState::Ok; RAIL_COUNT],
            rail_proven: [false; RAIL_COUNT],
            rail_breaches: [0; RAIL_COUNT],
        };
        me.cached = me.read_voltages();
        if me.cached.is_empty() {
            log::warn!(
                "stress-kit/superio: {chip} HWM @ 0x{hwm_base:04X} published no rail on the first \
                 read; board voltages disabled and the ISA bus left alone"
            );
            return None;
        }
        me.last_good = Instant::now();
        log::debug!(
            "stress-kit/superio: {chip} HWM @ 0x{hwm_base:04X}, {} rail(s) on nominal \
             (uncalibrated) dividers",
            me.cached.len()
        );
        Some(me)
    }

    /// Latest rails; throttled to [`POLL_INTERVAL`]. The cache is dropped once
    /// its last successful read is [`STALE_AFTER`] old rather than republished.
    pub fn poll(&mut self) -> Vec<VoltageReading> {
        self.drop_stale_cache();
        if self.last_polled.elapsed() < POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        let readings = self.read_voltages();
        if !readings.is_empty() {
            self.last_good = Instant::now();
            self.cached = readings;
        } else {
            self.drop_stale_cache();
        }
        self.cached.clone()
    }

    /// Clears the cache when the last successful read is [`STALE_AFTER`] old.
    fn drop_stale_cache(&mut self) {
        let age = self.last_good.elapsed();
        if self.cached.is_empty() || age < STALE_AFTER {
            return;
        }
        log::warn!(
            "stress-kit/superio: no rail read for {age:.1?} @ 0x{:04X}; dropping cached rails \
             instead of republishing them",
            self.hwm_base
        );
        self.cached.clear();
    }

    /// Samples the bank then grades it. A skipped sample restarts every
    /// consecutive-breach run, so only genuinely consecutive reads confirm a
    /// collapse.
    fn read_voltages(&mut self) -> Vec<VoltageReading> {
        match self.sample() {
            Some(sample) => self.grade(&sample),
            None => {
                self.rail_breaches = [0; RAIL_COUNT];
                Vec::new()
            }
        }
    }

    /// Channel map for the detected chip's register layout.
    fn channels(&self) -> &'static [Channel; RAIL_COUNT] {
        match self.family {
            Family::Classic => &CLASSIC_CHANNELS,
            Family::Ec6687 => &EC6687_CHANNELS,
        }
    }

    /// Reads every rail; `None` when the bus is held by a peer, the window will
    /// not open, or the family's liveness gate shows the block isn't answering.
    fn sample(&self) -> Option<Sample> {
        let lpc = self.access.lpc()?;
        let _bus = BusLease::acquire(lpc)?;
        let window = HwmWindow::open(lpc, self.hwm_base, HWM_WINDOW_LEN)?;
        match self.family {
            Family::Classic => sample_classic(&window),
            Family::Ec6687 => sample_ec6687(&window),
        }
    }

    /// Grades each rail, logging state transitions.
    fn grade(&mut self, sample: &Sample) -> Vec<VoltageReading> {
        let mut out = Vec::with_capacity(RAIL_COUNT);
        for (slot, rail) in RAILS.iter().enumerate() {
            let volts = sample[slot];
            let breached = volts.is_some_and(|v| v < rail.reportable.min);
            self.rail_breaches[slot] = if breached {
                self.rail_breaches[slot].saturating_add(1)
            } else {
                0
            };
            let state = classify_rail(rail, volts, self.rail_proven[slot], self.rail_breaches[slot]);
            self.rail_proven[slot] |= state == RailState::Ok;
            if self.rail_states[slot] != state {
                self.rail_states[slot] = state;
                log_rail_state(rail, state, volts, self.channels()[slot].at);
            }
            if let Some(volts) = volts.filter(|_| state.publishes()) {
                out.push(VoltageReading {
                    label: rail.label.to_string(),
                    volts,
                    calibrated: false,
                });
            }
        }
        out
    }
}

/// Grades one scaled reading. A sub-floor value publishes as
/// [`RailState::Collapsed`] only on a channel `proven` to carry this rail and only
/// after [`COLLAPSE_CONFIRM_READS`] consecutive breaches.
fn classify_rail(rail: &Rail, volts: Option<f32>, proven: bool, breaches: u8) -> RailState {
    match volts {
        None => RailState::Discarded,
        Some(v) if rail.nominal.holds(v) => RailState::Ok,
        Some(v) if rail.reportable.holds(v) => RailState::OutOfNominal,
        Some(v) if v >= rail.reportable.min => RailState::Discarded,
        _ if !proven => RailState::Unmapped,
        _ if breaches >= COLLAPSE_CONFIRM_READS => RailState::Collapsed,
        _ => RailState::CollapsePending,
    }
}

/// Logs a rail entering a new plausibility state. `channel` is the family's
/// register or channel, which is what an unmapped rail needs naming.
fn log_rail_state(rail: &Rail, state: RailState, volts: Option<f32>, channel: u16) {
    match state {
        RailState::Ok => log::info!(
            "stress-kit/superio: {} back inside its nominal band",
            rail.label
        ),
        RailState::OutOfNominal => log::warn!(
            "stress-kit/superio: {} reads {:.3} V, outside nominal {:.2}..{:.2} V (uncalibrated \
             divider)",
            rail.label,
            volts.unwrap_or_default(),
            rail.nominal.min,
            rail.nominal.max
        ),
        RailState::CollapsePending => log::warn!(
            "stress-kit/superio: {} read {:.3} V, below its reportable floor {:.2} V; withheld \
             until a second consecutive read confirms it",
            rail.label,
            volts.unwrap_or_default(),
            rail.reportable.min
        ),
        RailState::Collapsed => log::error!(
            "stress-kit/superio: {} collapsed to {:.3} V, below its reportable floor {:.2} V \
             (channel previously read nominal, so this is a real rail failure)",
            rail.label,
            volts.unwrap_or_default(),
            rail.reportable.min
        ),
        RailState::Unmapped => log::debug!(
            "stress-kit/superio: {} channel 0x{channel:02X} reads {:.3} V, below reportable \
             {:.2} V and never nominal; treated as unwired, not a collapse",
            rail.label,
            volts.unwrap_or_default(),
            rail.reportable.min
        ),
        RailState::Discarded => match volts {
            Some(v) if v > rail.reportable.max => log::warn!(
                "stress-kit/superio: {} reads {v:.3} V, above its reportable ceiling {:.2} V; \
                 discarded",
                rail.label,
                rail.reportable.max
            ),
            _ => log::debug!(
                "stress-kit/superio: {} discarded ({})",
                rail.label,
                volts.map_or_else(|| "no data".to_string(), |v| format!("{v:.3} V"))
            ),
        },
    }
}

/// Outcome of one Nuvoton probe at an LPC slot.
enum NuvotonProbe {
    /// Supported chip whose monitor window was confirmed.
    Found(&'static str, u16, Family),
    /// A chip answered the Nuvoton unlock, or a read/validation step failed.
    Answered,
    /// No chip id came back.
    Silent,
}

fn probe_slot(lpc: &dyn LpcAccess, slot: LpcSlot) -> Option<(&'static str, u16, Family)> {
    match probe_nuvoton(lpc, slot) {
        NuvotonProbe::Found(chip, base, family) => Some((chip, base, family)),
        NuvotonProbe::Answered => None,
        NuvotonProbe::Silent => {
            probe_ite(lpc, slot);
            None
        }
    }
}

fn probe_nuvoton(lpc: &dyn LpcAccess, slot: LpcSlot) -> NuvotonProbe {
    let (chip, base, family) = {
        let mode = ConfigMode::enter(lpc, slot, SuperIoFamily::Nuvoton);
        let (Some(id_high), Some(id_low)) =
            (mode.read_cr(CR_CHIP_ID_HIGH), mode.read_cr(CR_CHIP_ID_LOW))
        else {
            return NuvotonProbe::Answered;
        };
        if id_high == 0x00 || id_high == 0xFF {
            return NuvotonProbe::Silent;
        }
        let Some((chip, family)) = nuvoton_chip(id_high, id_low) else {
            // Logged at info: the chip id is the one fact that turns "voltages
            // unavailable" into an actionable gap, and it is otherwise lost.
            log::info!(
                "stress-kit/superio: slot 0x{:02X} chip id 0x{id_high:02X}{id_low:02X} has no \
                 reader; board voltages unavailable",
                slot.index_port()
            );
            return NuvotonProbe::Answered;
        };
        if mode.write_cr(CR_LOGICAL_DEVICE, NUVOTON_HWM_LDN).is_none() {
            log::debug!("stress-kit/superio: {chip} logical-device select failed");
            return NuvotonProbe::Answered;
        }
        let Some(base) = read_hwm_base(&mode) else {
            log::info!(
                "stress-kit/superio: {chip} hardware-monitor window unusable; board voltages \
                 disabled"
            );
            return NuvotonProbe::Answered;
        };
        (chip, base, family)
    };

    // The 0x5CA3 vendor register lives in the bank-selected space, which the EC
    // layout does not have. Those parts are confirmed instead by their 3VCC
    // gate on the first sample, which is a reading rather than a constant and
    // so proves rather more.
    if family == Family::Ec6687 {
        return NuvotonProbe::Found(chip, base, family);
    }

    match read_vendor_id(lpc, base) {
        Some(NUVOTON_VENDOR_ID) => NuvotonProbe::Found(chip, base, family),
        other => {
            log::warn!(
                "stress-kit/superio: {chip} HWM @ 0x{base:04X} vendor id {other:04X?} != \
                 0x{NUVOTON_VENDOR_ID:04X}; window unconfirmed, board voltages disabled"
            );
            NuvotonProbe::Answered
        }
    }
}

/// Identifies an ITE IT87xx and logs it; no registers are decoded. The ITE exit
/// write is armed only for an ITE-family id, since config register 0x02 is a
/// software reset on Winbond/Nuvoton parts.
fn probe_ite(lpc: &dyn LpcAccess, slot: LpcSlot) {
    let mut mode = ConfigMode::enter(lpc, slot, SuperIoFamily::Ite);
    let (Some(id_high), Some(id_low)) =
        (mode.read_cr(CR_CHIP_ID_HIGH), mode.read_cr(CR_CHIP_ID_LOW))
    else {
        return;
    };
    if !matches!(id_high, 0x85..=0x87) {
        return;
    }
    mode.exit_armed = true;
    log::info!(
        "stress-kit/superio: ITE IT{id_high:02X}{id_low:02X} at slot 0x{:02X}; voltage decode \
         unsupported (no verified scaling), skipping",
        slot.index_port()
    );
}

fn nuvoton_chip(id_high: u8, id_low: u8) -> Option<(&'static str, Family)> {
    let find = |table: &'static [(u8, u8, &'static str)]| {
        table
            .iter()
            .find(|(h, l, _)| *h == id_high && *l == id_low)
            .map(|(_, _, name)| *name)
    };
    find(NUVOTON_CLASSIC)
        .map(|name| (name, Family::Classic))
        .or_else(|| find(NUVOTON_EC6687).map(|name| (name, Family::Ec6687)))
}

/// Base address of the selected logical device, read twice; `None` unless both
/// reads agree and the window is accepted by [`sane_hwm_base`], whose normalized
/// value is returned.
fn read_hwm_base(mode: &ConfigMode<'_>) -> Option<u16> {
    let first = read_base_pair(mode)?;
    std::thread::sleep(Duration::from_millis(1));
    let second = read_base_pair(mode)?;
    if first != second {
        log::debug!("stress-kit/superio: HWM base unstable (0x{first:04X} then 0x{second:04X})");
        return None;
    }
    let Some(base) = sane_hwm_base(first) else {
        log::debug!(
            "stress-kit/superio: HWM base 0x{first:04X} is misaligned, outside the accepted \
             monitor range, or aliases a legacy ISA device"
        );
        return None;
    };
    if base != first {
        log::debug!(
            "stress-kit/superio: HWM base 0x{first:04X} reported with the +5 index offset; using \
             window start 0x{base:04X}"
        );
    }
    Some(base)
}

fn read_base_pair(mode: &ConfigMode<'_>) -> Option<u16> {
    let high = mode.read_cr(CR_BASE_HIGH)?;
    let low = mode.read_cr(CR_BASE_LOW)?;
    Some(((high as u16) << 8) | low as u16)
}

/// Normalizes a reported base to its window start, then accepts it only if the
/// window is admissible. Boards that report the base already offset by the +5
/// index register (e.g. 0x295) are masked down like LHM does.
fn sane_hwm_base(reported: u16) -> Option<u16> {
    let base = if reported & 0x0007 == HWM_ADDR_OFFSET as u16 {
        reported & !0x0007
    } else {
        reported
    };
    protocol::window_admissible(base, HWM_WINDOW_LEN).then_some(base)
}

/// Samples the NCT67xx bank layout. Gated on the two chip-internal 3.3V
/// supplies: an implausible pair means the block isn't answering, not that the
/// rail channels were guessed wrong.
fn sample_classic(window: &HwmWindow<'_>) -> Option<Sample> {
    // 0xFF is the floating-bus read and scales inside some rails' bands, so it
    // stays no-data; 0x00 is kept so a collapsed rail can still be graded.
    let mut raw = [None; VOLTAGE_CHANNELS];
    for (channel, slot) in raw.iter_mut().enumerate() {
        *slot = hwm_read(window, VOLTAGE_BANK, VOLTAGE_REG_BASE + channel as u8)
            .filter(|&r| r != 0xFF);
    }
    let live = GATE_INDEXES.iter().all(|&i| {
        raw[i as usize].is_some_and(|r| {
            let volts = r as f32 * LSB_VOLTS * GATE_FACTOR;
            (GATE_MIN..=GATE_MAX).contains(&volts)
        })
    });
    if !live {
        return None;
    }
    let mut out: Sample = [None; RAIL_COUNT];
    for (slot, channel) in CLASSIC_CHANNELS.iter().enumerate() {
        out[slot] = raw[channel.at as usize].map(|r| r as f32 * LSB_VOLTS * channel.factor);
    }
    Some(out)
}

/// Samples the NCT6687 EC space, gated on 3VCC I/O.
///
/// Waits for the block once here rather than before every byte: the wait is
/// contention handling, and paying it twelve times would stall the sampler for
/// seconds behind a peer. Each read still releases the page itself, which is
/// addressing rather than courtesy.
fn sample_ec6687(window: &HwmWindow<'_>) -> Option<Sample> {
    await_ec(window);
    let gate = ec_volts(window, EC_GATE_REG)?;
    if !(EC_GATE_MIN..=EC_GATE_MAX).contains(&gate) {
        log::debug!(
            "stress-kit/superio: EC 3VCC reads {gate:.3} V, outside \
             {EC_GATE_MIN:.2}..{EC_GATE_MAX:.2} V; the monitor block is not answering"
        );
        return None;
    }
    let mut out: Sample = [None; RAIL_COUNT];
    for (slot, channel) in EC6687_CHANNELS.iter().enumerate() {
        out[slot] = ec_volts(window, channel.at).map(|v| v * channel.factor);
    }
    Some(out)
}

/// One EC reading, from the register pair holding it.
fn ec_volts(window: &HwmWindow<'_>, reg: u16) -> Option<f32> {
    let high = ec_read(window, reg)?;
    let low = ec_read(window, reg + 1)?;
    Some(ec_decode(high, low))
}

/// 12 bits at 1 mV a step: a whole high byte, then the top nibble of the next
/// register. Unlike the bank layout there is no ADC step to apply, so a rail
/// with no board divider needs no factor at all.
fn ec_decode(high: u8, low: u8) -> f32 {
    let millivolts = ((high as u16) << 4) | (low >> 4) as u16;
    millivolts as f32 / 1000.0
}

/// One EC-space byte, addressed as a page and an index.
///
/// The trailing release is part of the addressing, not courtesy: the chip
/// latches a page/index pair only as the page register leaves
/// [`EC_PAGE_RELEASE`], so a read that skips it leaves the previous register
/// selected and every later read answers with that one's value. Done on every
/// path, including a failed select, or the block stays claimed against peers.
fn ec_read(window: &HwmWindow<'_>, addr: u16) -> Option<u8> {
    let selected = window
        .write(EC_PAGE_OFFSET, (addr >> 8) as u8)
        .and_then(|()| window.write(EC_INDEX_OFFSET, (addr & 0xFF) as u8));
    let value = selected.and_then(|()| window.read(EC_DATA_OFFSET));
    let _ = window.write(EC_PAGE_OFFSET, EC_PAGE_RELEASE);
    value
}

/// Waits for the page register to read [`EC_PAGE_RELEASE`], meaning no peer
/// holds the block. Past the timeout it claims it anyway: a tool that died
/// mid-access would otherwise lock the monitor out until reboot.
fn await_ec(window: &HwmWindow<'_>) {
    let deadline = Instant::now() + EC_ACCESS_WAIT;
    loop {
        if window.read(EC_PAGE_OFFSET) == Some(EC_PAGE_RELEASE) {
            return;
        }
        if Instant::now() >= deadline {
            log::debug!("stress-kit/superio: EC page register still held after {EC_ACCESS_WAIT:?}; forcing access");
            let _ = window.write(EC_PAGE_OFFSET, EC_PAGE_RELEASE);
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// One bank-selected hardware-monitor register read.
fn hwm_read(window: &HwmWindow<'_>, bank: u8, reg: u8) -> Option<u8> {
    window.write(HWM_ADDR_OFFSET, HWM_BANK_SELECT)?;
    window.write(HWM_DATA_OFFSET, bank)?;
    window.write(HWM_ADDR_OFFSET, reg)?;
    window.read(HWM_DATA_OFFSET)
}

/// Nuvoton vendor id: high byte from bank 0x80, low byte from bank 0x00.
fn read_vendor_id(lpc: &dyn LpcAccess, base: u16) -> Option<u16> {
    let window = HwmWindow::open(lpc, base, HWM_WINDOW_LEN)?;
    let high = hwm_read(&window, 0x80, HWM_VENDOR_REG)?;
    let low = hwm_read(&window, 0x00, HWM_VENDOR_REG)?;
    Some(((high as u16) << 8) | low as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lowlevel::mock::{LpcOp, MockBackend};

    const V12: &Rail = &RAILS[3];

    const SLOT: LpcSlot = LpcSlot::Port2E;

    fn exited(mock: &MockBackend, family: SuperIoFamily) -> bool {
        mock.ops().contains(&LpcOp::ConfigExit(SLOT, family))
    }

    #[test]
    fn v12_slot_matches_its_label() {
        assert_eq!(V12.label, "+12V");
    }

    /// Both id tables are searched, and each chip resolves to the layout it
    /// actually speaks. Reading an EC part with the bank decoder returns
    /// plausible-looking nonsense, so this mapping is load-bearing.
    #[test]
    fn chip_ids_resolve_to_their_register_layout() {
        assert_eq!(nuvoton_chip(0xC5, 0x60), Some(("NCT6779D", Family::Classic)));
        assert_eq!(nuvoton_chip(0xD8, 0x02), Some(("NCT6799D-R", Family::Classic)));
        assert_eq!(nuvoton_chip(0xD5, 0x92), Some(("NCT6687D", Family::Ec6687)));
        assert_eq!(nuvoton_chip(0xC7, 0x32), Some(("NCT6683D", Family::Ec6687)));
        assert_eq!(nuvoton_chip(0xAB, 0xCD), None);
    }

    /// 12-bit millivolts: whole high byte, top nibble of the next register.
    #[test]
    fn ec_readings_decode_as_12_bit_millivolts() {
        assert_eq!(ec_decode(0x00, 0x00), 0.0);
        // 0xCE4 = 3300 mV, a 3.3V rail.
        assert_eq!(ec_decode(0xCE, 0x40), 3.300);
        // 0x44C = 1100 mV, a plausible Vcore.
        assert_eq!(ec_decode(0x44, 0xC0), 1.100);
        // The low nibble of the low byte is not part of the reading.
        assert_eq!(ec_decode(0x44, 0xCF), ec_decode(0x44, 0xC0));
        // Full scale, which is also what a floating bus reads.
        assert_eq!(ec_decode(0xFF, 0xFF), 4.095);
    }

    /// The gate must reject both a floating bus and a dead block, or a chip
    /// that is not answering publishes a full set of fabricated rails.
    #[test]
    fn the_ec_gate_rejects_a_bus_that_is_not_answering() {
        let band = EC_GATE_MIN..=EC_GATE_MAX;
        assert!(!band.contains(&ec_decode(0xFF, 0xFF)), "floating bus passed the gate");
        assert!(!band.contains(&ec_decode(0x00, 0x00)), "dead block passed the gate");
        assert!(band.contains(&ec_decode(0xCE, 0x40)), "a real 3.3V read must pass");
    }

    /// The EC map is upstream's VIN assignment; a transposed pair here reads
    /// one rail's voltage under another rail's label and bands.
    #[test]
    fn the_ec_channel_map_matches_the_documented_registers() {
        let at = |slot: usize| EC6687_CHANNELS[slot].at;
        assert_eq!((at(0), RAILS[0].label), (0x124, "Vcore"));
        assert_eq!((at(1), RAILS[1].label), (0x122, "+5V"));
        assert_eq!((at(2), RAILS[2].label), (0x130, "3VCC (chip)"));
        assert_eq!((at(3), RAILS[3].label), (0x120, "+12V"));
        assert_eq!((at(4), RAILS[4].label), (0x13C, "VBAT"));
    }

    /// The page register must pass back through [`EC_PAGE_RELEASE`] between
    /// reads. The chip latches a page/index pair only on that transition, so
    /// selecting twice in a row leaves the first register selected and every
    /// later read answers with its value — which on a live NCT6687D board
    /// published one channel's voltage under all five rail labels.
    #[test]
    fn every_ec_read_releases_the_page_before_the_next_selects() {
        const BASE: u16 = 0x0A20;
        // The mock's window is flat, so both bytes of a pair read 0xC9, which
        // decodes to 3.228 V and clears the gate.
        let mock = MockBackend::full().with_hwm(BASE, EC_DATA_OFFSET, 0xC9);
        {
            let window = HwmWindow::open(&mock, BASE, HWM_WINDOW_LEN).expect("window opens");
            assert!(sample_ec6687(&window).is_some(), "0xC9 should clear the gate");
        }

        let mut selections = 0usize;
        let mut released = true;
        for op in mock.ops() {
            let LpcOp::WindowOut(_, EC_PAGE_OFFSET, value) = op else {
                continue;
            };
            if value == EC_PAGE_RELEASE {
                released = true;
                continue;
            }
            assert!(released, "a page was selected while the previous one was still latched");
            released = false;
            selections += 1;
        }
        // Six register pairs: the gate, then five rails.
        assert_eq!(selections, 12, "every byte should select its own register");
        assert!(released, "the block was left claimed against peers");
    }

    /// Only rails behind a board divider carry a factor: the EC reports
    /// millivolts at the pin, unlike the bank layout's ADC steps.
    #[test]
    fn only_divided_ec_rails_carry_a_factor() {
        assert_eq!(EC6687_CHANNELS[1].factor, 5.0);
        assert_eq!(EC6687_CHANNELS[3].factor, 12.0);
        for slot in [0, 2, 4] {
            assert_eq!(EC6687_CHANNELS[slot].factor, 1.0, "slot {slot}");
        }
    }

    #[test]
    fn plus_five_offset_bases_mask_down_to_the_window_start() {
        assert_eq!(sane_hwm_base(0x0295), Some(0x0290));
        assert_eq!(sane_hwm_base(0x02E5), Some(0x02E0));
        assert_eq!(sane_hwm_base(0x0A15), Some(0x0A10));
        assert_eq!(sane_hwm_base(0x0FFD), Some(0x0FF8));
    }

    #[test]
    fn aligned_bases_pass_through_unchanged() {
        assert_eq!(sane_hwm_base(0x0290), Some(0x0290));
        assert_eq!(sane_hwm_base(0x04E0), Some(0x04E0));
        assert_eq!(sane_hwm_base(0x0A00), Some(0x0A00));
    }

    #[test]
    fn masking_cannot_reach_a_reserved_or_out_of_range_window() {
        assert_eq!(sane_hwm_base(0x0205), None); // masks onto the game port
        assert_eq!(sane_hwm_base(0x03F5), None); // masks onto primary FDC/ATA
        assert_eq!(sane_hwm_base(0x0005), None); // masks below the accepted floor
        assert_eq!(sane_hwm_base(0x01F0), None); // ATA command block, under the floor
        assert_eq!(sane_hwm_base(0x0170), None); // secondary ATA, under the floor
        assert_eq!(sane_hwm_base(0x0000), None);
        assert_eq!(sane_hwm_base(0xFFFF), None);
        assert_eq!(sane_hwm_base(0x0203), None); // misaligned and not the +5 form
    }

    #[test]
    fn proven_rail_publishes_a_confirmed_floor_breach() {
        assert!(matches!(classify_rail(V12, Some(0.0), true, 1), RailState::CollapsePending));
        assert!(matches!(classify_rail(V12, Some(0.0), true, 2), RailState::Collapsed));
        assert!(matches!(classify_rail(V12, Some(3.4), true, 2), RailState::Collapsed));
        assert!(classify_rail(V12, Some(0.0), true, 2).publishes());
        assert!(!classify_rail(V12, Some(0.0), true, 1).publishes());
    }

    #[test]
    fn unproven_channel_never_publishes_a_floor_breach() {
        assert!(matches!(classify_rail(V12, Some(0.0), false, 9), RailState::Unmapped));
        assert!(!classify_rail(V12, Some(0.0), false, 9).publishes());
    }

    #[test]
    fn missing_and_overvoltage_reads_stay_suppressed() {
        assert!(matches!(classify_rail(V12, None, true, 2), RailState::Discarded));
        assert!(matches!(classify_rail(V12, Some(30.0), true, 2), RailState::Discarded));
    }

    #[test]
    fn nominal_and_sagging_rails_publish() {
        assert!(matches!(classify_rail(V12, Some(12.0), false, 0), RailState::Ok));
        assert!(matches!(classify_rail(V12, Some(10.8), false, 0), RailState::OutOfNominal));
        assert!(classify_rail(V12, Some(10.8), false, 0).publishes());
    }

    /// A chip id that never answers must still close config mode; a SuperIO left
    /// unlocked is a real field failure.
    #[test]
    fn an_unreadable_chip_id_still_exits_config_mode() {
        let mock = MockBackend::full();
        assert!(matches!(probe_nuvoton(&mock, SLOT), NuvotonProbe::Answered));
        assert!(exited(&mock, SuperIoFamily::Nuvoton));
    }

    /// Same for an id that reads fine but has no reader here.
    #[test]
    fn an_unrecognised_chip_id_still_exits_config_mode() {
        let mock = MockBackend::full()
            .with_cr(SLOT, CR_CHIP_ID_HIGH, 0xAB)
            .with_cr(SLOT, CR_CHIP_ID_LOW, 0xCD);
        assert!(matches!(probe_nuvoton(&mock, SLOT), NuvotonProbe::Answered));
        assert!(exited(&mock, SuperIoFamily::Nuvoton));
    }

    /// A supported chip whose base register is unusable exits too, after the
    /// logical-device select has already written.
    #[test]
    fn an_unusable_window_still_exits_config_mode() {
        let mock = MockBackend::full()
            .with_cr(SLOT, CR_CHIP_ID_HIGH, 0xD4)
            .with_cr(SLOT, CR_CHIP_ID_LOW, 0x28)
            .with_cr(SLOT, CR_BASE_HIGH, 0x03)
            .with_cr(SLOT, CR_BASE_LOW, 0xF0); // primary FDC/ATA, refused
        assert!(matches!(probe_nuvoton(&mock, SLOT), NuvotonProbe::Answered));
        assert!(exited(&mock, SuperIoFamily::Nuvoton));
        assert!(
            mock.ops()
                .contains(&LpcOp::ConfigWrite(SLOT, CR_LOGICAL_DEVICE, NUVOTON_HWM_LDN)),
            "logical-device select should have run before the base read"
        );
    }

    /// Config register 0x02 is a software reset on Nuvoton parts, so the ITE exit
    /// write must stay disarmed unless an ITE chip id actually answered.
    #[test]
    fn a_silent_slot_never_writes_the_ite_exit_register() {
        let mock = MockBackend::full()
            .with_cr(SLOT, CR_CHIP_ID_HIGH, 0x00)
            .with_cr(SLOT, CR_CHIP_ID_LOW, 0x00);
        assert!(matches!(probe_nuvoton(&mock, SLOT), NuvotonProbe::Silent));

        probe_ite(&mock, SLOT);
        assert!(
            !exited(&mock, SuperIoFamily::Ite),
            "ITE exit fired on a chip that never identified as ITE"
        );
    }

    /// An ITE part is identified and closed with its own exit sequence.
    #[test]
    fn an_ite_chip_arms_its_own_exit() {
        let mock = MockBackend::full()
            .with_cr(SLOT, CR_CHIP_ID_HIGH, 0x87)
            .with_cr(SLOT, CR_CHIP_ID_LOW, 0x28);
        probe_ite(&mock, SLOT);
        assert!(exited(&mock, SuperIoFamily::Ite));
    }

    /// A contended bus yields no sample, and the caller resets the breach run so
    /// a skipped read cannot count toward confirming a collapse.
    #[test]
    fn a_contended_bus_resets_the_breach_run() {
        let mock = MockBackend::full();
        mock.bus_contended.store(true, std::sync::atomic::Ordering::Relaxed);
        let access = crate::lowlevel::LowLevelAccess::new(Box::new(mock), "scripted", Vec::new());

        let mut monitor = SuperIoMonitor {
            access,
            hwm_base: 0x0290,
            family: Family::Classic,
            cached: Vec::new(),
            last_polled: Instant::now(),
            last_good: Instant::now(),
            rail_states: [RailState::Ok; RAIL_COUNT],
            rail_proven: [true; RAIL_COUNT],
            rail_breaches: [1; RAIL_COUNT],
        };

        assert!(monitor.read_voltages().is_empty());
        assert_eq!(monitor.rail_breaches, [0; RAIL_COUNT]);
    }
}
