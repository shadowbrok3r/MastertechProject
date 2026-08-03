//! Bus-protocol constants shared by backends and the readers above them.
//!
//! Chip identification, register maps, and rail scaling are chip knowledge and
//! live with the readers in [`crate::telemetry`]; only the sequences a backend
//! must emit itself are here.

/// Nuvoton/Winbond config-mode unlock byte, written twice to the index port.
pub const NUVOTON_ENTER: u8 = 0x87;
/// Nuvoton/Winbond config-mode exit byte.
pub const NUVOTON_EXIT: u8 = 0xAA;

/// ITE config-mode unlock prefix; the fourth byte depends on the slot.
pub const ITE_ENTER_PREFIX: [u8; 3] = [0x87, 0x01, 0x55];
/// Final ITE unlock byte for the 0x4E slot; the 0x2E slot uses 0x55.
pub const ITE_ENTER_LAST_4E: u8 = 0xAA;
pub const ITE_ENTER_LAST_2E: u8 = 0x55;
/// ITE config-mode exit is a register write, not a bare index-port byte. This
/// register is a software reset on Nuvoton parts, so it is only ever written
/// after an ITE chip id has answered.
pub const ITE_EXIT_REG: u8 = 0x02;
pub const ITE_EXIT_VALUE: u8 = 0x02;

/// Hardware-monitor window: index register at `base + 5`, data at `base + 6`.
pub const HWM_ADDR_OFFSET: u8 = 0x05;
pub const HWM_DATA_OFFSET: u8 = 0x06;
/// Bytes a hardware-monitor window spans.
pub const HWM_WINDOW_LEN: u16 = 8;

/// AMD SMN index/data register pair in D0:F0 config space.
pub const AMD_SMN_BUS_DEVICE_FN: u32 = 0;
pub const AMD_SMN_INDEX_REG: u32 = 0x60;
pub const AMD_SMN_DATA_REG: u32 = 0x64;

/// Mutex other sensor tools take around PCI config index/data sequences.
pub const PCI_MUTEX_NAME: &str = r"Global\Access_PCI";
/// Both mutex names peers use for the SuperIO/ISA bus; taken together so either
/// convention interlocks with us.
pub const ISA_MUTEX_NAMES: [&str; 2] =
    [r"Global\Access_ISABUS.HTP.Method", r"Global\Access_ISABUS"];

/// Bounded wait for a shared bus mutex.
pub const MUTEX_WAIT_MS: u32 = 200;

/// Fixed-function ISA IO ranges (inclusive) a monitor window must not overlap.
pub const RESERVED_IO_RANGES: &[(u16, u16)] = &[
    (0x0200, 0x0207), // game port
    (0x0220, 0x022F), // legacy audio
    (0x0278, 0x027F), // LPT2
    (0x02E8, 0x02EF), // COM4
    (0x02F8, 0x02FF), // COM2
    (0x0300, 0x031F), // legacy NIC / MPU-401
    (0x0330, 0x033F), // legacy SCSI / MIDI
    (0x0370, 0x0377), // secondary FDC / secondary ATA control
    (0x0378, 0x037F), // LPT1
    (0x0388, 0x038F), // FM synth
    (0x03B0, 0x03DF), // VGA / MDA / LPT3
    (0x03E8, 0x03EF), // COM3
    (0x03F0, 0x03F7), // primary FDC / primary ATA control
    (0x03F8, 0x03FF), // COM1
    (0x0678, 0x067F), // LPT2 ECP
    (0x0778, 0x077F), // LPT1 ECP
];

/// Accepted hardware-monitor window bounds. The floor stays above the ATA
/// command blocks.
pub const HWM_BASE_MIN: u16 = 0x0200;
pub const HWM_BASE_MAX: u16 = 0x0FF8;

/// True when `[base, base + len)` overlaps a fixed-function ISA range.
pub fn overlaps_reserved(base: u16, len: u16) -> bool {
    let last = base.saturating_add(len.saturating_sub(1));
    RESERVED_IO_RANGES
        .iter()
        .any(|&(lo, hi)| base <= hi && lo <= last)
}

/// True when a window is 8-byte aligned, inside the accepted range, and clear of
/// every fixed-function ISA range.
pub fn window_admissible(base: u16, len: u16) -> bool {
    base & 0x0007 == 0
        && (HWM_BASE_MIN..=HWM_BASE_MAX).contains(&base)
        && !overlaps_reserved(base, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_ranges_are_refused() {
        assert!(!window_admissible(0x0200, HWM_WINDOW_LEN)); // game port
        assert!(!window_admissible(0x03F0, HWM_WINDOW_LEN)); // primary FDC/ATA
        assert!(!window_admissible(0x03F8, HWM_WINDOW_LEN)); // COM1
    }

    #[test]
    fn misaligned_and_out_of_range_windows_are_refused() {
        assert!(!window_admissible(0x0293, HWM_WINDOW_LEN));
        assert!(!window_admissible(0x0000, HWM_WINDOW_LEN));
        assert!(!window_admissible(0x1000, HWM_WINDOW_LEN));
    }

    #[test]
    fn ordinary_monitor_windows_are_admitted() {
        assert!(window_admissible(0x0290, HWM_WINDOW_LEN));
        assert!(window_admissible(0x04E0, HWM_WINDOW_LEN));
        assert!(window_admissible(0x0A00, HWM_WINDOW_LEN));
        assert!(window_admissible(0x0FF8, HWM_WINDOW_LEN));
    }
}
