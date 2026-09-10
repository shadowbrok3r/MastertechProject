//! PawnIO backend.
//!
//! Sandboxed bytecode modules inside an attestation-signed driver, so it loads
//! with Secure Boot and Memory Integrity on where WinRing0 does not. Register
//! reach is whatever the upstream modules allow: PawnIO refuses anything outside
//! their allowlists, so this backend cannot read an arbitrary register.

mod device;
mod devnode;
mod provision;

use std::sync::Mutex;

use device::Module;

use super::busmutex::{IsaBus, PciMutex};
use super::protocol;
use super::{
    BackendId, Capabilities, LowLevelBackend, LpcAccess, LpcSlot, MsrAccess, SmnAccess,
    SuperIoFamily,
};
use crate::telemetry::cpu_ceiling::{self, CpuVendor};

const INTEL_MSR_MODULE: &[u8] = include_bytes!("../../../drivers/pawnio/IntelMSR.bin");
const AMD_FAMILY17_MODULE: &[u8] = include_bytes!("../../../drivers/pawnio/AMDFamily17.bin");
const LPC_IO_MODULE: &[u8] = include_bytes!("../../../drivers/pawnio/LpcIO.bin");

/// `MSR_PLATFORM_INFO`. On the module's read allowlist and present on every
/// supported Intel part, so a failed read means the handle is gone rather than
/// that the register was refused.
const INTEL_PROBE_MSR: u32 = 0xCE;
/// `MSR_AMD64_PATCH_LEVEL`, the AMD module's equivalent.
const AMD_PROBE_MSR: u32 = 0x8B;

/// `ioctl_select_slot` numbers the slots rather than taking a port.
fn slot_index(slot: LpcSlot) -> i64 {
    match slot {
        LpcSlot::Port2E => 0,
        LpcSlot::Port4E => 1,
    }
}

pub struct PawnIoBackend {
    /// Vendor MSR module, which on AMD also vends SMN.
    cpu: Module,
    vendor: CpuVendor,
    /// SuperIO module; `None` when it would not load, leaving die reads intact.
    lpc: Option<Module>,
    /// Slot the LpcIO module currently points at. Re-selecting clears the
    /// module's discovered windows, so a mismatched access is refused instead.
    selected: Mutex<Option<LpcSlot>>,
    pci: PciMutex,
    isa: IsaBus,
    /// Currently admitted hardware-monitor window.
    window: Mutex<Option<(u16, u16)>>,
    version: (u16, u8, u8),
}

impl PawnIoBackend {
    /// Installs the driver when it is absent, then loads the modules this CPU
    /// needs.
    pub fn open() -> Result<Self, String> {
        let version = provision::ensure_available()?;
        let vendor = cpu_ceiling::vendor();
        let (label, blob) = match vendor {
            CpuVendor::Intel => ("IntelMSR", INTEL_MSR_MODULE),
            CpuVendor::Amd => ("AMDFamily17", AMD_FAMILY17_MODULE),
            CpuVendor::Other => {
                return Err("no PawnIO CPU module ships for this processor vendor".to_string());
            }
        };
        let cpu = Module::load(label, blob)?;
        let lpc = match Module::load("LpcIO", LPC_IO_MODULE) {
            Ok(module) => Some(module),
            Err(reason) => {
                log::debug!("stress-kit/pawnio: {reason}; board voltage rails unavailable");
                None
            }
        };
        Ok(Self {
            cpu,
            vendor,
            lpc,
            selected: Mutex::new(None),
            pci: PciMutex::open(),
            isa: IsaBus::open(),
            window: Mutex::new(None),
            version,
        })
    }

    /// One sentence naming the live driver and what it reaches.
    pub fn detail(&self) -> String {
        let (major, minor, patch) = self.version;
        let rails = if self.lpc.is_some() {
            "board voltage rails included"
        } else {
            "no SuperIO module loaded, so no board voltage rails"
        };
        format!(
            "PawnIO {major}.{minor}.{patch} is live. Its driver is Microsoft attestation-signed, \
             so Memory Integrity and the Vulnerable Driver Blocklist do not block it; register \
             reach is limited to what the signed modules allow ({rails})."
        )
    }

    fn probe_msr(&self) -> u32 {
        match self.vendor {
            CpuVendor::Amd => AMD_PROBE_MSR,
            _ => INTEL_PROBE_MSR,
        }
    }

    /// `IN AL, DX`, admitted only for the selected slot's own ports and the
    /// windows `ioctl_find_bars` discovered.
    fn pio_in(&self, port: u16) -> Option<u8> {
        let [value] = self
            .lpc
            .as_ref()?
            .execute::<1, 1>("ioctl_pio_inb", [port as i64])?;
        Some(value as u8)
    }

    fn pio_out(&self, port: u16, value: u8) -> Option<()> {
        self.lpc
            .as_ref()?
            .execute::<2, 0>("ioctl_pio_outb", [port as i64, value as i64])?;
        Some(())
    }

    /// `None` unless the module still points at `slot`.
    fn on_selected(&self, slot: LpcSlot) -> Option<()> {
        let selected = *self.selected.lock().ok()?;
        if selected == Some(slot) {
            return Some(());
        }
        log::debug!(
            "stress-kit/pawnio: LpcIO points at {selected:?}, not slot 0x{:02X}; access refused",
            slot.index_port()
        );
        None
    }

    /// Writes one config-mode exit byte, retrying once before warning.
    fn exit_write(&self, port: u16, value: u8) {
        if self.pio_out(port, value).is_some() {
            return;
        }
        if self.pio_out(port, value).is_some() {
            log::debug!(
                "stress-kit/pawnio: config-mode exit 0x{value:02X} to 0x{port:04X} needed a retry"
            );
            return;
        }
        log::warn!(
            "stress-kit/pawnio: config-mode exit 0x{value:02X} to 0x{port:04X} failed twice; \
             SuperIO may be left unlocked"
        );
    }

    /// Absolute port for a window-relative offset; `None` unless the window is
    /// the admitted one and the offset is inside it.
    fn window_port(&self, base: u16, offset: u8) -> Option<u16> {
        let (open_base, len) = (*self.window.lock().ok()?)?;
        (open_base == base && (offset as u16) < len).then_some(base + offset as u16)
    }
}

impl MsrAccess for PawnIoBackend {
    fn read_msr(&self, msr: u32) -> Option<u64> {
        let [value] = self.cpu.execute::<1, 1>("ioctl_read_msr", [msr as i64])?;
        Some(value as u64)
    }
}

impl SmnAccess for PawnIoBackend {
    /// The module performs the config-space index/data pair itself; the shared
    /// PCI mutex still stops peer tools interleaving their own pair with it.
    fn read_smn(&self, addr: u32) -> Option<u32> {
        let _guard = self.pci.lock();
        let [value] = self.cpu.execute::<1, 1>("ioctl_read_smn", [addr as i64])?;
        Some(value as u32)
    }
}

impl LpcAccess for PawnIoBackend {
    fn acquire_bus(&self) -> bool {
        self.isa.acquire()
    }

    fn release_bus(&self) {
        self.isa.release()
    }

    fn config_enter(&self, slot: LpcSlot, family: SuperIoFamily) -> Option<()> {
        let lpc = self.lpc.as_ref()?;
        lpc.execute::<1, 0>("ioctl_select_slot", [slot_index(slot)])?;
        *self.selected.lock().ok()? = Some(slot);
        *self.window.lock().ok()? = None;

        let index = slot.index_port();
        match family {
            SuperIoFamily::Nuvoton => {
                self.pio_out(index, protocol::NUVOTON_ENTER)?;
                self.pio_out(index, protocol::NUVOTON_ENTER)?;
            }
            SuperIoFamily::Ite => {
                let last = match slot {
                    LpcSlot::Port4E => protocol::ITE_ENTER_LAST_4E,
                    LpcSlot::Port2E => protocol::ITE_ENTER_LAST_2E,
                };
                for byte in protocol::ITE_ENTER_PREFIX.into_iter().chain([last]) {
                    self.pio_out(index, byte)?;
                }
            }
        }

        // Walks the logical devices to build the module's allowlist of monitor
        // windows, which is what admits every later window read. It needs the
        // chip already unlocked, so it cannot be deferred to `window_open`.
        if lpc.execute::<0, 0>("ioctl_find_bars", []).is_none() {
            log::debug!("stress-kit/pawnio: LpcIO found no monitor window on slot 0x{index:02X}");
        }
        Some(())
    }

    fn config_exit(&self, slot: LpcSlot, family: SuperIoFamily) {
        match family {
            SuperIoFamily::Nuvoton => self.exit_write(slot.index_port(), protocol::NUVOTON_EXIT),
            // ITE exits by register write, which the module emits as one
            // index/data pair.
            SuperIoFamily::Ite => {
                if self
                    .config_write(slot, protocol::ITE_EXIT_REG, protocol::ITE_EXIT_VALUE)
                    .is_none()
                {
                    log::warn!(
                        "stress-kit/pawnio: ITE config-mode exit failed; SuperIO may be left \
                         unlocked"
                    );
                }
            }
        }
    }

    fn config_read(&self, slot: LpcSlot, reg: u8) -> Option<u8> {
        self.on_selected(slot)?;
        let [value] = self
            .lpc
            .as_ref()?
            .execute::<1, 1>("ioctl_superio_inb", [reg as i64])?;
        Some(value as u8)
    }

    fn config_write(&self, slot: LpcSlot, reg: u8, value: u8) -> Option<()> {
        self.on_selected(slot)?;
        self.lpc
            .as_ref()?
            .execute::<2, 0>("ioctl_superio_outb", [reg as i64, value as i64])?;
        Some(())
    }

    /// Admitted twice: against our own reserved-range rules, then against the
    /// module's discovered windows by reading the window's own index register,
    /// which has no side effect.
    fn window_open(&self, base: u16, len: u16) -> Option<()> {
        if !protocol::window_admissible(base, len) {
            log::debug!(
                "stress-kit/pawnio: refusing monitor window 0x{base:04X}+{len}; misaligned, out \
                 of range, or aliasing a legacy ISA device"
            );
            return None;
        }
        *self.window.lock().ok()? = Some((base, len));
        if self.pio_in(base + protocol::HWM_ADDR_OFFSET as u16).is_none() {
            log::debug!(
                "stress-kit/pawnio: LpcIO did not admit monitor window 0x{base:04X}; it is not \
                 among the windows discovered on the selected slot"
            );
            *self.window.lock().ok()? = None;
            return None;
        }
        Some(())
    }

    fn window_close(&self, base: u16) {
        if let Ok(mut window) = self.window.lock()
            && window.map(|(b, _)| b) == Some(base)
        {
            *window = None;
        }
    }

    fn window_in(&self, base: u16, offset: u8) -> Option<u8> {
        self.pio_in(self.window_port(base, offset)?)
    }

    fn window_out(&self, base: u16, offset: u8, value: u8) -> Option<()> {
        self.pio_out(self.window_port(base, offset)?, value)
    }
}

impl LowLevelBackend for PawnIoBackend {
    fn id(&self) -> BackendId {
        BackendId::PawnIo
    }

    fn capabilities(&self) -> Capabilities {
        let lpc = self.lpc.is_some();
        Capabilities {
            msr: true,
            msr_per_cpu: false,
            smn: self.vendor == CpuVendor::Amd,
            lpc_config: lpc,
            lpc_window: lpc,
            package_temp: false,
        }
    }

    fn probe(&self) -> Result<(), String> {
        if self.read_msr(self.probe_msr()).is_some() {
            return Ok(());
        }
        Err(
            "PawnIO stopped answering; the driver was uninstalled, stopped, or its device was \
             torn down"
                .to_string(),
        )
    }

    fn msr(&self) -> Option<&dyn MsrAccess> {
        Some(self)
    }

    fn smn(&self) -> Option<&dyn SmnAccess> {
        (self.vendor == CpuVendor::Amd).then_some(self as &dyn SmnAccess)
    }

    fn lpc(&self) -> Option<&dyn LpcAccess> {
        self.lpc.is_some().then_some(self as &dyn LpcAccess)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slot numbering is the module's, not the port address.
    #[test]
    fn slots_map_to_the_modules_indexes() {
        assert_eq!(slot_index(LpcSlot::Port2E), 0);
        assert_eq!(slot_index(LpcSlot::Port4E), 1);
    }

    /// The modules ship as signed blobs. A truncated one is rejected by the
    /// driver at load with no diagnostic beyond a Win32 error.
    #[test]
    fn the_embedded_modules_carry_a_signature_header() {
        for (label, blob) in [
            ("IntelMSR", INTEL_MSR_MODULE),
            ("AMDFamily17", AMD_FAMILY17_MODULE),
            ("LpcIO", LPC_IO_MODULE),
        ] {
            let sig_len = u32::from_le_bytes(blob[..4].try_into().unwrap()) as usize;
            assert!(
                sig_len > 0 && 4 + sig_len < blob.len(),
                "{label} is not a signed PawnIO module"
            );
        }
    }
}
