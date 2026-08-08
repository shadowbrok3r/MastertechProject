//! SuperIO (LPC) board-voltage reader.
//!
//! Probes the 0x2E/0x4E LPC index-data slots once for a Nuvoton NCT67xx
//! hardware-monitor block, requires the 0x5CA3 vendor id to confirm the window
//! before it is used, then samples the voltage bank on a throttle. Rail scaling
//! uses the *conventional* Nuvoton resistor dividers — the real ratios are
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

/// One voltage channel with the nominal divider applied.
struct Rail {
    label: &'static str,
    index: u8,
    factor: f32,
    /// Values outside this are physically impossible and are discarded.
    reportable: Band,
    /// Values outside this are reported and warned about, not discarded.
    nominal: Band,
}

const RAIL_COUNT: usize = super::RAIL_LABELS.len();

/// Channel-to-rail map and nominal dividers for the NCT67xx 0x48x layout;
/// index assignment and divider are both board-specific in reality.
// 3.14 here is a 3.3V rail's lower bound, not an approximation of PI.
#[allow(clippy::approx_constant)]
const NUVOTON_RAILS: [Rail; RAIL_COUNT] = [
    Rail {
        label: super::RAIL_LABELS[0],
        index: 0,
        factor: 1.0,
        reportable: Band { min: 0.05, max: 2.04 },
        nominal: Band { min: 0.50, max: 1.80 },
    },
    Rail {
        label: super::RAIL_LABELS[1],
        index: 1,
        factor: 5.0,
        reportable: Band { min: 2.00, max: 8.00 },
        nominal: Band { min: 4.75, max: 5.25 },
    },
    // Chip supply, not the board's +3.3V PSU rail; labelled so nothing reads it as one.
    Rail {
        label: super::RAIL_LABELS[2],
        index: 3,
        factor: 2.0,
        reportable: Band { min: 1.50, max: 4.08 },
        nominal: Band { min: 3.14, max: 3.47 },
    },
    Rail {
        label: super::RAIL_LABELS[3],
        index: 4,
        factor: 12.0,
        reportable: Band { min: 4.00, max: 20.00 },
        nominal: Band { min: 11.40, max: 12.60 },
    },
    Rail {
        label: super::RAIL_LABELS[4],
        index: 8,
        factor: 2.0,
        reportable: Band { min: 1.00, max: 4.08 },
        nominal: Band { min: 2.50, max: 3.60 },
    },
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
    let mut i = 0;
    while i < RAIL_COUNT {
        assert!(
            (NUVOTON_RAILS[i].index as usize) < VOLTAGE_CHANNELS,
            "rail channel out of bank range"
        );
        assert!(
            NUVOTON_RAILS[i].reportable.min <= NUVOTON_RAILS[i].nominal.min,
            "nominal band must sit inside the reportable band"
        );
        assert!(
            NUVOTON_RAILS[i].nominal.max <= NUVOTON_RAILS[i].reportable.max,
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

/// One sampled voltage bank; `None` per channel means the read did not answer.
type Bank = [Option<u8>; VOLTAGE_CHANNELS];

pub struct SuperIoMonitor {
    access: LowLevelAccess,
    hwm_base: u16,
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
        let (chip, hwm_base) = detected?;

        let mut me = Self {
            access,
            hwm_base,
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
        match self.sample_bank() {
            Some(bank) => self.grade(&bank),
            None => {
                self.rail_breaches = [0; RAIL_COUNT];
                Vec::new()
            }
        }
    }

    /// Reads every voltage channel; `None` when the bus is held by a peer, the
    /// window will not open, or the fixed AVCC/3VCC channels show the HWM block
    /// isn't answering.
    fn sample_bank(&self) -> Option<Bank> {
        let lpc = self.access.lpc()?;
        let _bus = BusLease::acquire(lpc)?;
        let window = HwmWindow::open(lpc, self.hwm_base, HWM_WINDOW_LEN)?;

        // 0xFF is the floating-bus read and scales inside some rails' bands, so it
        // stays no-data; 0x00 is kept so a collapsed rail can still be graded.
        let mut bank: Bank = [None; VOLTAGE_CHANNELS];
        for (channel, slot) in bank.iter_mut().enumerate() {
            *slot = hwm_read(&window, VOLTAGE_BANK, VOLTAGE_REG_BASE + channel as u8)
                .filter(|&r| r != 0xFF);
        }

        let live = GATE_INDEXES.iter().all(|&i| {
            bank[i as usize].is_some_and(|r| {
                let volts = r as f32 * LSB_VOLTS * GATE_FACTOR;
                (GATE_MIN..=GATE_MAX).contains(&volts)
            })
        });
        live.then_some(bank)
    }

    /// Scales each mapped channel and grades it, logging state transitions.
    fn grade(&mut self, bank: &Bank) -> Vec<VoltageReading> {
        let mut out = Vec::with_capacity(RAIL_COUNT);
        for (slot, rail) in NUVOTON_RAILS.iter().enumerate() {
            let volts = bank[rail.index as usize].map(|r| r as f32 * LSB_VOLTS * rail.factor);
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
                log_rail_state(rail, state, volts);
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

/// Logs a rail entering a new plausibility state.
fn log_rail_state(rail: &Rail, state: RailState, volts: Option<f32>) {
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
            "stress-kit/superio: {} channel {} reads {:.3} V, below reportable {:.2} V and never \
             nominal; treated as unwired, not a collapse",
            rail.label,
            rail.index,
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
    /// Supported NCT67xx whose HWM window answered with the Nuvoton vendor id.
    Found(&'static str, u16),
    /// A chip answered the Nuvoton unlock, or a read/validation step failed.
    Answered,
    /// No chip id came back.
    Silent,
}

fn probe_slot(lpc: &dyn LpcAccess, slot: LpcSlot) -> Option<(&'static str, u16)> {
    match probe_nuvoton(lpc, slot) {
        NuvotonProbe::Found(chip, base) => Some((chip, base)),
        NuvotonProbe::Answered => None,
        NuvotonProbe::Silent => {
            probe_ite(lpc, slot);
            None
        }
    }
}

fn probe_nuvoton(lpc: &dyn LpcAccess, slot: LpcSlot) -> NuvotonProbe {
    let (chip, base) = {
        let mode = ConfigMode::enter(lpc, slot, SuperIoFamily::Nuvoton);
        let (Some(id_high), Some(id_low)) =
            (mode.read_cr(CR_CHIP_ID_HIGH), mode.read_cr(CR_CHIP_ID_LOW))
        else {
            return NuvotonProbe::Answered;
        };
        if id_high == 0x00 || id_high == 0xFF {
            return NuvotonProbe::Silent;
        }
        let Some(chip) = nuvoton_chip(id_high, id_low) else {
            // Logged at info: the chip id is the one fact that turns "voltages
            // unavailable" into an actionable gap, and it is otherwise lost.
            log::info!(
                "stress-kit/superio: slot 0x{:02X} chip id 0x{id_high:02X}{id_low:02X} ({}) has \
                 no reader; board voltages unavailable",
                slot.index_port(),
                known_unsupported_chip(id_high, id_low).unwrap_or("unrecognized")
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
        (chip, base)
    };

    match read_vendor_id(lpc, base) {
        Some(NUVOTON_VENDOR_ID) => NuvotonProbe::Found(chip, base),
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

fn nuvoton_chip(id_high: u8, id_low: u8) -> Option<&'static str> {
    NUVOTON_CLASSIC
        .iter()
        .find(|(h, l, _)| *h == id_high && *l == id_low)
        .map(|(_, _, name)| *name)
}

/// Names a Nuvoton part that is identified but has no reader here, so the log
/// says which chip is missing rather than only its id. These use the NCT6687
/// EC-space layout, not the 0x48x bank/register map [`NUVOTON_CLASSIC`] reads.
fn known_unsupported_chip(id_high: u8, id_low: u8) -> Option<&'static str> {
    match (id_high, id_low) {
        (0xD5, 0x92) => Some("NCT6687D"),
        (0xD4, 0x92) => Some("NCT6687D-M"),
        (0xC7, 0x32) => Some("NCT6683D"),
        _ => None,
    }
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

    const V12: &Rail = &NUVOTON_RAILS[3];

    const SLOT: LpcSlot = LpcSlot::Port2E;

    fn exited(mock: &MockBackend, family: SuperIoFamily) -> bool {
        mock.ops().contains(&LpcOp::ConfigExit(SLOT, family))
    }

    #[test]
    fn v12_slot_matches_its_label() {
        assert_eq!(V12.label, "+12V");
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
