//! Pluggable kernel-mode access for sensors user mode cannot reach.
//!
//! Backends vend one operation per hardware-bus transaction that must be atomic
//! against other sensor tools ([`SmnAccess::read_smn`], not a PCI index/data
//! pair), so a constrained signed driver can implement the same trait a raw
//! port-IO driver does. Register decode, plausibility limits, and chip tables
//! live in [`crate::telemetry`], never in a backend.

pub mod protocol;

#[cfg(any(test, feature = "mock-backend"))]
pub mod mock;

pub mod select;

#[cfg(all(target_os = "windows", feature = "backend-winring0"))]
pub mod winring0;

use std::sync::{Arc, OnceLock, Weak};

use serde::{Deserialize, Serialize};

/// Which access provider is live. Transport identity, not sensor identity — an
/// Intel DTS read stays [`crate::telemetry::CpuDieReader::IntelDts`] whichever
/// backend carried it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendId {
    /// No kernel-mode provider; user-mode sources only.
    #[default]
    None,
    /// Mastertech's own signed driver with a fixed register allowlist.
    Mtdrv,
    /// Legacy WinRing0 (CVE-2020-14979); loads only with driver protections off.
    WinRing0,
    /// Scripted backend for tests.
    Mock,
}

impl BackendId {
    /// Operator-facing name.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Mtdrv => "Mastertech sensor driver",
            Self::WinRing0 => "WinRing0 (legacy)",
            Self::Mock => "mock",
        }
    }
}

/// What the live provider can reach. All false once the provider is lost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub msr: bool,
    /// Backend targets a logical processor itself; callers must not pin affinity.
    pub msr_per_cpu: bool,
    pub smn: bool,
    /// SuperIO config space on the 0x2E/0x4E index-data slots.
    pub lpc_config: bool,
    /// Byte access inside a hardware-monitor window the config space reported.
    pub lpc_window: bool,
}

impl Capabilities {
    /// Capabilities needed for a CPU die reading on either vendor.
    pub fn any_die(&self) -> bool {
        self.msr || self.smn
    }

    /// Capabilities needed to probe and sample a SuperIO hardware monitor.
    pub fn any_lpc(&self) -> bool {
        self.lpc_config && self.lpc_window
    }
}

/// Which sensor groups the live provider can serve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessTier {
    /// CPU die temperature and board rails both reachable.
    Full,
    /// Die temperature only.
    DieOnly,
    /// Board rails only.
    RailsOnly,
    /// No provider. ACPI zones, sysinfo, and disk temps still work.
    #[default]
    None,
}

impl AccessTier {
    fn from_caps(caps: Capabilities) -> Self {
        match (caps.any_die(), caps.any_lpc()) {
            (true, true) => Self::Full,
            (true, false) => Self::DieOnly,
            (false, true) => Self::RailsOnly,
            (false, false) => Self::None,
        }
    }
}

/// A backend that was tried and declined, with the reason an operator can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedBackend {
    pub backend: BackendId,
    pub reason: String,
}

/// Which backend carried a snapshot's die temperature and rails, and why any
/// other did not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessStatus {
    pub backend: BackendId,
    pub tier: AccessTier,
    pub backend_label: String,
    /// Why this backend is live, or why nothing is.
    pub detail: String,
    pub rejected: Vec<RejectedBackend>,
    /// Set once a provider that answered at open stopped answering.
    pub lost: Option<String>,
}

/// Model-specific-register reads.
///
/// `read_msr` executes on the calling thread's current processor, so per-core
/// reads pin affinity first. A backend that targets a processor itself declares
/// [`Capabilities::msr_per_cpu`] and overrides [`MsrAccess::read_msr_on`].
pub trait MsrAccess {
    fn read_msr(&self, msr: u32) -> Option<u64>;

    fn read_msr_on(&self, cpu: usize, msr: u32) -> Option<u64> {
        let prev = affinity::pin(cpu);
        let value = self.read_msr(msr);
        affinity::restore(prev);
        value
    }
}

/// AMD System Management Network reads.
///
/// One call, not a PCI index/data pair: the pair is a shared-hardware sequence
/// that must be serialized against every other sensor tool, and no signed driver
/// can safely vend the raw config-space write half of it.
pub trait SmnAccess {
    fn read_smn(&self, addr: u32) -> Option<u32>;
}

/// LPC index-data slot a SuperIO answers on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LpcSlot {
    Port2E,
    Port4E,
}

impl LpcSlot {
    pub const ALL: [Self; 2] = [Self::Port2E, Self::Port4E];

    pub fn index_port(self) -> u16 {
        match self {
            Self::Port2E => 0x2E,
            Self::Port4E => 0x4E,
        }
    }

    pub fn data_port(self) -> u16 {
        self.index_port() + 1
    }
}

/// SuperIO vendor family, selecting the config-mode unlock and exit sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperIoFamily {
    Nuvoton,
    Ite,
}

/// LPC SuperIO access.
///
/// Config-space entry and exit are backend operations because the unlock byte
/// sequences are raw index-port writes a constrained driver will not vend.
/// Window access is offset-relative so a backend can bounds-check it.
pub trait LpcAccess {
    /// Serializes this sequence against other sensor tools. `false` means the
    /// bus was not taken and the caller must not touch LPC.
    fn acquire_bus(&self) -> bool;
    fn release_bus(&self);

    fn config_enter(&self, slot: LpcSlot, family: SuperIoFamily) -> Option<()>;
    fn config_exit(&self, slot: LpcSlot, family: SuperIoFamily);
    fn config_read(&self, slot: LpcSlot, reg: u8) -> Option<u8>;
    fn config_write(&self, slot: LpcSlot, reg: u8, value: u8) -> Option<()>;

    /// Admits a hardware-monitor window for byte access. `None` when the backend
    /// will not vend it.
    fn window_open(&self, base: u16, len: u16) -> Option<()>;
    fn window_close(&self, base: u16);
    /// `offset` is window-relative; the backend adds `base` and bounds-checks.
    fn window_in(&self, base: u16, offset: u8) -> Option<u8>;
    fn window_out(&self, base: u16, offset: u8, value: u8) -> Option<()>;
}

pub trait LowLevelBackend: Send + Sync {
    fn id(&self) -> BackendId;
    fn capabilities(&self) -> Capabilities;
    /// Cheap liveness probe; `Err` means the provider is gone, not that a
    /// particular sensor is missing.
    fn probe(&self) -> Result<(), String>;

    fn msr(&self) -> Option<&dyn MsrAccess> {
        None
    }
    fn smn(&self) -> Option<&dyn SmnAccess> {
        None
    }
    fn lpc(&self) -> Option<&dyn LpcAccess> {
        None
    }
}

struct Inner {
    backend: Option<Box<dyn LowLevelBackend>>,
    detail: String,
    rejected: Vec<RejectedBackend>,
    /// First proven-dead reason; one-way.
    lost: OnceLock<String>,
}

/// Handle to the live backend. Cloneable, so drop order between the monitors
/// sharing it does not matter and the provider unloads on last drop.
#[derive(Clone)]
pub struct LowLevelAccess(Arc<Inner>);

/// Non-owning reference to an open backend, so a cache can hand out clones
/// without keeping the provider loaded past its last real holder.
pub(crate) struct WeakAccess(Weak<Inner>);

impl WeakAccess {
    pub(crate) fn upgrade(&self) -> Option<LowLevelAccess> {
        self.0.upgrade().map(LowLevelAccess)
    }
}

impl LowLevelAccess {
    pub fn new(
        backend: Box<dyn LowLevelBackend>,
        detail: impl Into<String>,
        rejected: Vec<RejectedBackend>,
    ) -> Self {
        Self(Arc::new(Inner {
            backend: Some(backend),
            detail: detail.into(),
            rejected,
            lost: OnceLock::new(),
        }))
    }

    /// Handle with no provider; every accessor returns `None`.
    pub fn unavailable(detail: impl Into<String>, rejected: Vec<RejectedBackend>) -> Self {
        Self(Arc::new(Inner {
            backend: None,
            detail: detail.into(),
            rejected,
            lost: OnceLock::new(),
        }))
    }

    pub(crate) fn downgrade(&self) -> WeakAccess {
        WeakAccess(Arc::downgrade(&self.0))
    }

    /// The provider was proven gone; this handle will never read again.
    pub fn is_lost(&self) -> bool {
        self.0.lost.get().is_some()
    }

    /// Both handles name the same open provider.
    #[cfg(test)]
    pub(crate) fn same_provider(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    fn live(&self) -> Option<&dyn LowLevelBackend> {
        if self.0.lost.get().is_some() {
            return None;
        }
        self.0.backend.as_deref()
    }

    pub fn id(&self) -> BackendId {
        self.live().map_or(BackendId::None, |b| b.id())
    }

    pub fn capabilities(&self) -> Capabilities {
        self.live().map(|b| b.capabilities()).unwrap_or_default()
    }

    pub fn tier(&self) -> AccessTier {
        AccessTier::from_caps(self.capabilities())
    }

    pub fn msr(&self) -> Option<&dyn MsrAccess> {
        self.live()?.msr()
    }

    pub fn smn(&self) -> Option<&dyn SmnAccess> {
        self.live()?.smn()
    }

    pub fn lpc(&self) -> Option<&dyn LpcAccess> {
        self.live()?.lpc()
    }

    /// Probes the provider; on failure latches the reason and returns `true`.
    /// Idempotent — a latched handle stays lost.
    pub fn confirm_lost(&self, context: &str) -> bool {
        if self.0.lost.get().is_some() {
            return true;
        }
        let Some(backend) = self.0.backend.as_deref() else {
            return false;
        };
        match backend.probe() {
            Ok(()) => false,
            Err(reason) => {
                let reason = format!("{context}: {reason}");
                log::warn!("stress-kit/lowlevel: {reason}; readings end here");
                let _ = self.0.lost.set(reason);
                true
            }
        }
    }

    pub fn status(&self) -> AccessStatus {
        let id = self.id();
        AccessStatus {
            backend: id,
            tier: self.tier(),
            backend_label: id.label().to_string(),
            detail: self.0.detail.clone(),
            rejected: self.0.rejected.clone(),
            lost: self.0.lost.get().cloned(),
        }
    }
}

/// Holds an [`LpcAccess`] bus lease for one probe or sample sequence.
pub struct BusLease<'a> {
    lpc: &'a dyn LpcAccess,
}

impl<'a> BusLease<'a> {
    /// `None`, holding nothing, when the bus is contended.
    pub fn acquire(lpc: &'a dyn LpcAccess) -> Option<Self> {
        lpc.acquire_bus().then_some(Self { lpc })
    }
}

impl Drop for BusLease<'_> {
    fn drop(&mut self) {
        self.lpc.release_bus();
    }
}

/// Holds SuperIO config mode open; [`Drop`] writes the exit sequence on every
/// path including unwinds.
pub struct ConfigMode<'a> {
    lpc: &'a dyn LpcAccess,
    slot: LpcSlot,
    family: SuperIoFamily,
    /// Gates the exit write for families whose exit register is destructive on
    /// the wrong part.
    pub exit_armed: bool,
}

impl<'a> ConfigMode<'a> {
    /// Constructed before the unlock writes so a partial sequence still exits.
    pub fn enter(lpc: &'a dyn LpcAccess, slot: LpcSlot, family: SuperIoFamily) -> Self {
        let armed = matches!(family, SuperIoFamily::Nuvoton);
        let me = Self { lpc, slot, family, exit_armed: armed };
        let _ = lpc.config_enter(slot, family);
        me
    }

    pub fn read_cr(&self, reg: u8) -> Option<u8> {
        self.lpc.config_read(self.slot, reg)
    }

    pub fn write_cr(&self, reg: u8, value: u8) -> Option<()> {
        self.lpc.config_write(self.slot, reg, value)
    }
}

impl Drop for ConfigMode<'_> {
    fn drop(&mut self) {
        if self.exit_armed {
            self.lpc.config_exit(self.slot, self.family);
        }
    }
}

/// An admitted hardware-monitor window; [`Drop`] releases it.
pub struct HwmWindow<'a> {
    lpc: &'a dyn LpcAccess,
    base: u16,
}

impl<'a> HwmWindow<'a> {
    pub fn open(lpc: &'a dyn LpcAccess, base: u16, len: u16) -> Option<Self> {
        lpc.window_open(base, len)?;
        Some(Self { lpc, base })
    }

    pub fn base(&self) -> u16 {
        self.base
    }

    pub fn read(&self, offset: u8) -> Option<u8> {
        self.lpc.window_in(self.base, offset)
    }

    pub fn write(&self, offset: u8, value: u8) -> Option<()> {
        self.lpc.window_out(self.base, offset, value)
    }
}

impl Drop for HwmWindow<'_> {
    fn drop(&mut self) {
        self.lpc.window_close(self.base);
    }
}

/// Thread-affinity pinning for per-core MSR reads.
pub mod affinity {
    /// Pins the current thread to `cpu`; returns the previous mask, or `None`
    /// when pinning is unsupported or failed.
    #[cfg(target_os = "windows")]
    pub fn pin(cpu: usize) -> Option<usize> {
        use winapi::um::{processthreadsapi, winbase};
        let prev = unsafe {
            winbase::SetThreadAffinityMask(processthreadsapi::GetCurrentThread(), 1usize << cpu)
        };
        (prev != 0).then_some(prev)
    }

    /// Restores a mask from [`pin`]. A zero mask is a failed pin, never restored.
    #[cfg(target_os = "windows")]
    pub fn restore(prev: Option<usize>) {
        use winapi::um::{processthreadsapi, winbase};
        if let Some(prev) = prev.filter(|p| *p != 0) {
            unsafe { winbase::SetThreadAffinityMask(processthreadsapi::GetCurrentThread(), prev) };
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn pin(_cpu: usize) -> Option<usize> {
        None
    }

    #[cfg(not(target_os = "windows"))]
    pub fn restore(_prev: Option<usize>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_follows_capabilities() {
        let die = Capabilities { msr: true, ..Default::default() };
        let rails = Capabilities { lpc_config: true, lpc_window: true, ..Default::default() };
        let both = Capabilities { msr: true, lpc_config: true, lpc_window: true, ..Default::default() };
        assert_eq!(AccessTier::from_caps(die), AccessTier::DieOnly);
        assert_eq!(AccessTier::from_caps(rails), AccessTier::RailsOnly);
        assert_eq!(AccessTier::from_caps(both), AccessTier::Full);
        assert_eq!(AccessTier::from_caps(Capabilities::default()), AccessTier::None);
    }

    /// A half-configured LPC backend cannot serve rails.
    #[test]
    fn lpc_needs_both_config_and_window() {
        let config_only = Capabilities { lpc_config: true, ..Default::default() };
        assert!(!config_only.any_lpc());
        assert_eq!(AccessTier::from_caps(config_only), AccessTier::None);
    }

    #[test]
    fn smn_alone_still_reads_a_die() {
        let amd = Capabilities { smn: true, ..Default::default() };
        assert!(amd.any_die());
        assert_eq!(AccessTier::from_caps(amd), AccessTier::DieOnly);
    }

    #[test]
    fn unavailable_handle_reports_nothing() {
        let access = LowLevelAccess::unavailable("no backend compiled in", Vec::new());
        assert_eq!(access.id(), BackendId::None);
        assert_eq!(access.tier(), AccessTier::None);
        assert!(access.msr().is_none() && access.smn().is_none() && access.lpc().is_none());
        // Nothing to lose, so it never latches.
        assert!(!access.confirm_lost("probe"));
        assert_eq!(access.status().lost, None);
    }
}
