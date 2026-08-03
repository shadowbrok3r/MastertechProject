//! Scripted backend for testing the readers without hardware.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{
    BackendId, Capabilities, LowLevelBackend, LpcAccess, LpcSlot, MsrAccess, SmnAccess,
    SuperIoFamily,
};

/// One recorded LPC operation, so a test can assert the exact unlock,
/// bank-select, and exit sequences a reader emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LpcOp {
    AcquireBus,
    ReleaseBus,
    ConfigEnter(LpcSlot, SuperIoFamily),
    ConfigExit(LpcSlot, SuperIoFamily),
    ConfigRead(LpcSlot, u8),
    ConfigWrite(LpcSlot, u8, u8),
    WindowOpen(u16, u16),
    WindowClose(u16),
    WindowIn(u16, u8),
    WindowOut(u16, u8, u8),
}

#[derive(Default)]
pub struct MockBackend {
    pub caps: Capabilities,
    /// `(logical cpu, msr) -> value`; distinct per cpu so a reader that ignores
    /// affinity is caught rather than returning a plausible constant.
    pub msrs: Mutex<HashMap<(usize, u32), u64>>,
    pub smn: Mutex<HashMap<u32, u32>>,
    /// `(index port, config register) -> value`.
    pub cr: Mutex<HashMap<(u16, u8), u8>>,
    /// `(window base, offset) -> value`, resolved after bank select.
    pub hwm: Mutex<HashMap<(u16, u8), u8>>,
    pub trace: Mutex<Vec<LpcOp>>,
    /// Every accessor fails once this many calls have been served.
    pub fail_after: AtomicUsize,
    calls: AtomicUsize,
    /// `acquire_bus` refuses, simulating a peer holding the ISA mutexes.
    pub bus_contended: AtomicBool,
    window: Mutex<Option<(u16, u16)>>,
}

impl MockBackend {
    /// Backend with every capability and no scripted data.
    pub fn full() -> Self {
        Self {
            caps: Capabilities {
                msr: true,
                msr_per_cpu: true,
                smn: true,
                lpc_config: true,
                lpc_window: true,
            },
            fail_after: AtomicUsize::new(usize::MAX),
            ..Default::default()
        }
    }

    pub fn with_msr(self, cpu: usize, msr: u32, value: u64) -> Self {
        self.msrs.lock().unwrap().insert((cpu, msr), value);
        self
    }

    /// Same value on every logical processor up to `cpus`.
    pub fn with_msr_all(self, cpus: usize, msr: u32, value: u64) -> Self {
        {
            let mut m = self.msrs.lock().unwrap();
            for cpu in 0..cpus {
                m.insert((cpu, msr), value);
            }
        }
        self
    }

    pub fn with_smn(self, addr: u32, value: u32) -> Self {
        self.smn.lock().unwrap().insert(addr, value);
        self
    }

    pub fn with_cr(self, slot: LpcSlot, reg: u8, value: u8) -> Self {
        self.cr.lock().unwrap().insert((slot.index_port(), reg), value);
        self
    }

    pub fn with_hwm(self, base: u16, offset: u8, value: u8) -> Self {
        self.hwm.lock().unwrap().insert((base, offset), value);
        self
    }

    pub fn ops(&self) -> Vec<LpcOp> {
        self.trace.lock().unwrap().clone()
    }

    fn record(&self, op: LpcOp) {
        self.trace.lock().unwrap().push(op);
    }

    /// `false` once the scripted failure point is reached.
    fn alive(&self) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed) < self.fail_after.load(Ordering::Relaxed)
    }
}

impl MsrAccess for MockBackend {
    fn read_msr(&self, msr: u32) -> Option<u64> {
        self.read_msr_on(0, msr)
    }

    fn read_msr_on(&self, cpu: usize, msr: u32) -> Option<u64> {
        self.alive()
            .then(|| self.msrs.lock().ok()?.get(&(cpu, msr)).copied())
            .flatten()
    }
}

impl SmnAccess for MockBackend {
    fn read_smn(&self, addr: u32) -> Option<u32> {
        self.alive()
            .then(|| self.smn.lock().ok()?.get(&addr).copied())
            .flatten()
    }
}

impl LpcAccess for MockBackend {
    fn acquire_bus(&self) -> bool {
        if self.bus_contended.load(Ordering::Relaxed) {
            return false;
        }
        self.record(LpcOp::AcquireBus);
        true
    }

    fn release_bus(&self) {
        self.record(LpcOp::ReleaseBus);
    }

    fn config_enter(&self, slot: LpcSlot, family: SuperIoFamily) -> Option<()> {
        self.record(LpcOp::ConfigEnter(slot, family));
        self.alive().then_some(())
    }

    fn config_exit(&self, slot: LpcSlot, family: SuperIoFamily) {
        self.record(LpcOp::ConfigExit(slot, family));
    }

    fn config_read(&self, slot: LpcSlot, reg: u8) -> Option<u8> {
        self.record(LpcOp::ConfigRead(slot, reg));
        self.alive()
            .then(|| self.cr.lock().ok()?.get(&(slot.index_port(), reg)).copied())
            .flatten()
    }

    fn config_write(&self, slot: LpcSlot, reg: u8, value: u8) -> Option<()> {
        self.record(LpcOp::ConfigWrite(slot, reg, value));
        self.cr.lock().ok()?.insert((slot.index_port(), reg), value);
        self.alive().then_some(())
    }

    fn window_open(&self, base: u16, len: u16) -> Option<()> {
        self.record(LpcOp::WindowOpen(base, len));
        if !super::protocol::window_admissible(base, len) {
            return None;
        }
        *self.window.lock().ok()? = Some((base, len));
        Some(())
    }

    fn window_close(&self, base: u16) {
        self.record(LpcOp::WindowClose(base));
        if let Ok(mut w) = self.window.lock() {
            if w.map(|(b, _)| b) == Some(base) {
                *w = None;
            }
        }
    }

    fn window_in(&self, base: u16, offset: u8) -> Option<u8> {
        self.record(LpcOp::WindowIn(base, offset));
        if !self.in_window(base, offset) {
            return None;
        }
        self.alive()
            .then(|| self.hwm.lock().ok()?.get(&(base, offset)).copied())
            .flatten()
    }

    fn window_out(&self, base: u16, offset: u8, value: u8) -> Option<()> {
        self.record(LpcOp::WindowOut(base, offset, value));
        if !self.in_window(base, offset) {
            return None;
        }
        self.hwm.lock().ok()?.insert((base, offset), value);
        self.alive().then_some(())
    }
}

impl MockBackend {
    fn in_window(&self, base: u16, offset: u8) -> bool {
        self.window
            .lock()
            .ok()
            .and_then(|w| *w)
            .is_some_and(|(b, len)| b == base && (offset as u16) < len)
    }
}

impl LowLevelBackend for MockBackend {
    fn id(&self) -> BackendId {
        BackendId::Mock
    }

    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn probe(&self) -> Result<(), String> {
        if self.calls.load(Ordering::Relaxed) < self.fail_after.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err("mock backend exhausted".into())
        }
    }

    fn msr(&self) -> Option<&dyn MsrAccess> {
        self.caps.msr.then_some(self as &dyn MsrAccess)
    }

    fn smn(&self) -> Option<&dyn SmnAccess> {
        self.caps.smn.then_some(self as &dyn SmnAccess)
    }

    fn lpc(&self) -> Option<&dyn LpcAccess> {
        self.caps.lpc_config.then_some(self as &dyn LpcAccess)
    }
}
