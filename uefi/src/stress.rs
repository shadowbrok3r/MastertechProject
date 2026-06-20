//! All-core stress engine on EFI MP Services.
//!
//! The BSP keeps running the TUI; worker kernels run on every AP via
//! `startup_all_aps` in non-blocking mode and report through per-core atomic
//! counters. Kernels are integer/memory based: `x86_64-unknown-uefi` is a
//! soft-float target (no SSE), so FP math would measure libm emulation, not
//! silicon. APs never allocate, log, or call boot services.
//!
//! Without MP Services (or on a single-core box) the same kernels run on the
//! BSP in small time slices between frames.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use core::time::Duration;

use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::pi::mp::MpServices;
use uefi::Event;
use uefi_raw::table::boot::{EventType, Tpl};

use crate::charts::History;
use crate::logln;

pub const MAX_CORES: usize = 256;
const PRESET_STAGE_SECS: f64 = 20.0;
const SAMPLE_EVERY_SECS: f64 = 0.5;
const CHART_WINDOW_SECS: f64 = 120.0;
/// Region size ladder; first allocation that succeeds wins.
const REGION_SIZES: [usize; 5] = [
    512 << 20,
    256 << 20,
    128 << 20,
    64 << 20,
    32 << 20,
];
const SIEVE_LIMIT: usize = 200_000;
const ALU_BURST: u64 = 1 << 16;

// ---------------------------------------------------------------------------
// Stages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    CpuAlu,
    Prime,
    Stream,
    Memcpy,
    Memtest,
}

pub const STAGES: [Stage; 5] = [
    Stage::CpuAlu,
    Stage::Prime,
    Stage::Stream,
    Stage::Memcpy,
    Stage::Memtest,
];

impl Stage {
    pub fn label(&self) -> &'static str {
        match self {
            Self::CpuAlu => "CPU ALU",
            Self::Prime => "Prime Sieve",
            Self::Stream => "Stream Triad",
            Self::Memcpy => "Memcpy",
            Self::Memtest => "Memtest",
        }
    }

    pub fn unit(&self) -> &'static str {
        match self {
            Self::CpuAlu => "op/s",
            Self::Prime => "primes/s",
            Self::Stream | Self::Memcpy | Self::Memtest => "B/s",
        }
    }

    /// Permissive pass floor (qc-app style); None = unscored.
    pub fn floor(&self) -> Option<f64> {
        match self {
            // Bytes/sec floors only — ALU/prime rates vary too much across
            // the fleet to gate on until calibrated.
            Self::Stream | Self::Memcpy => Some(2.0e9),
            Self::Memtest => Some(1.0e8),
            _ => None,
        }
    }

    fn idx(self) -> u8 {
        match self {
            Self::CpuAlu => 0,
            Self::Prime => 1,
            Self::Stream => 2,
            Self::Memcpy => 3,
            Self::Memtest => 4,
        }
    }

    fn from_idx(v: u8) -> Self {
        match v {
            0 => Self::CpuAlu,
            1 => Self::Prime,
            2 => Self::Stream,
            3 => Self::Memcpy,
            _ => Self::Memtest,
        }
    }
}

// ---------------------------------------------------------------------------
// AP-shared state
// ---------------------------------------------------------------------------

/// Per-core slot, cache-line sized to avoid false sharing between APs.
#[repr(align(64))]
struct CoreSlot {
    ops: AtomicU64,
    errors: AtomicU64,
    active: AtomicBool,
}

impl CoreSlot {
    const fn new() -> Self {
        Self {
            ops: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            active: AtomicBool::new(false),
        }
    }
}

/// Everything an AP may touch. Heap-pinned by the engine; APs receive a raw
/// pointer and use only atomics + raw region memory.
struct Shared {
    cancel: AtomicBool,
    stage: AtomicU8,
    next_slot: AtomicUsize,
    region_ptr: AtomicUsize,
    chunk_len: AtomicUsize,
    cores: [CoreSlot; MAX_CORES],
}

impl Shared {
    fn new() -> Box<Self> {
        Box::new(Self {
            cancel: AtomicBool::new(false),
            stage: AtomicU8::new(0),
            next_slot: AtomicUsize::new(0),
            region_ptr: AtomicUsize::new(0),
            chunk_len: AtomicUsize::new(0),
            cores: [const { CoreSlot::new() }; MAX_CORES],
        })
    }

    fn chunk(&self, slot: usize) -> (*mut u8, usize) {
        let len = self.chunk_len.load(Ordering::Acquire);
        let base = self.region_ptr.load(Ordering::Acquire);
        ((base + slot * len) as *mut u8, len)
    }
}

/// AP entry point: self-assign a slot, run the active stage's kernel until
/// cancelled. Must not allocate, log, or call boot services.
extern "efiapi" fn ap_entry(arg: *mut c_void) {
    let sh = unsafe { &*(arg as *const Shared) };
    let slot = sh.next_slot.fetch_add(1, Ordering::AcqRel);
    if slot >= MAX_CORES {
        return;
    }
    sh.cores[slot].active.store(true, Ordering::Release);
    run_kernel(
        Stage::from_idx(sh.stage.load(Ordering::Acquire)),
        sh,
        slot,
        None,
    );
    sh.cores[slot].active.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Kernels (shared by APs and the BSP fallback path)
// ---------------------------------------------------------------------------

#[inline(always)]
fn rdtsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    0
}

#[inline(always)]
fn expired(deadline: Option<u64>) -> bool {
    deadline.is_some_and(|d| rdtsc() >= d)
}

/// Run one stage kernel on `slot`. `deadline` (TSC) bounds BSP time slices;
/// APs pass None and run until `sh.cancel`.
fn run_kernel(stage: Stage, sh: &Shared, slot: usize, deadline: Option<u64>) {
    match stage {
        Stage::CpuAlu => alu_kernel(sh, slot, deadline),
        Stage::Prime => prime_kernel(sh, slot, deadline),
        Stage::Stream => stream_kernel(sh, slot, deadline),
        Stage::Memcpy => memcpy_kernel(sh, slot, deadline),
        Stage::Memtest => memtest_kernel(sh, slot, deadline),
    }
}

/// Register-only integer mix: xorshift*, popcount, rotate. ~6 ALU ops/iter.
fn alu_kernel(sh: &Shared, slot: usize, deadline: Option<u64>) {
    let me = &sh.cores[slot];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ (slot as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
    let mut acc: u64 = 0;
    while !sh.cancel.load(Ordering::Relaxed) {
        for _ in 0..ALU_BURST {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
            acc = acc.wrapping_add(x.count_ones() as u64).rotate_left(7) ^ x;
        }
        core::hint::black_box(acc);
        me.ops.fetch_add(ALU_BURST, Ordering::Relaxed);
        if expired(deadline) {
            return;
        }
    }
}

/// Sieve of Eratosthenes over the core's chunk; one byte per candidate.
fn prime_kernel(sh: &Shared, slot: usize, deadline: Option<u64>) {
    let (ptr, len) = sh.chunk(slot);
    if ptr.is_null() || len < 4096 {
        return;
    }
    let limit = SIEVE_LIMIT.min(len - 1);
    let me = &sh.cores[slot];
    while !sh.cancel.load(Ordering::Relaxed) {
        unsafe {
            core::ptr::write_bytes(ptr, 1, limit + 1);
            let mut i = 2usize;
            while i * i <= limit {
                if *ptr.add(i) != 0 {
                    let mut j = i * i;
                    while j <= limit {
                        *ptr.add(j) = 0;
                        j += i;
                    }
                }
                i += 1;
            }
            let mut found = 0u64;
            for n in 2..=limit {
                found += *ptr.add(n) as u64;
            }
            me.ops.fetch_add(found, Ordering::Relaxed);
        }
        if expired(deadline) {
            return;
        }
    }
}

/// McCalpin-style triad on u64 thirds of the chunk: c[i] = a[i] + 3*b[i].
fn stream_kernel(sh: &Shared, slot: usize, deadline: Option<u64>) {
    let (ptr, len) = sh.chunk(slot);
    if ptr.is_null() || len < 8192 {
        return;
    }
    let me = &sh.cores[slot];
    let base = ((ptr as usize + 7) & !7) as *mut u64;
    let n = (len - 8) / 8 / 3;
    let (a, b, c) = (base, unsafe { base.add(n) }, unsafe { base.add(2 * n) });
    unsafe {
        for i in 0..n {
            a.add(i).write(i as u64);
            b.add(i).write((i as u64) << 1);
        }
    }
    while !sh.cancel.load(Ordering::Relaxed) {
        unsafe {
            for i in 0..n {
                let v = a.add(i).read_volatile() + 3 * b.add(i).read_volatile();
                c.add(i).write_volatile(v);
            }
        }
        me.ops.fetch_add((n * 24) as u64, Ordering::Relaxed);
        if expired(deadline) {
            return;
        }
    }
}

/// Block copies between chunk halves, alternating direction each pass.
fn memcpy_kernel(sh: &Shared, slot: usize, deadline: Option<u64>) {
    let (ptr, len) = sh.chunk(slot);
    if ptr.is_null() || len < 8192 {
        return;
    }
    let me = &sh.cores[slot];
    let half = len / 2;
    let mut flip = false;
    while !sh.cancel.load(Ordering::Relaxed) {
        unsafe {
            let (src, dst) = if flip {
                (ptr.add(half), ptr)
            } else {
                (ptr, ptr.add(half))
            };
            core::ptr::copy_nonoverlapping(src, dst, half);
        }
        flip = !flip;
        // Read + write traffic.
        me.ops.fetch_add((half * 2) as u64, Ordering::Relaxed);
        if expired(deadline) {
            return;
        }
    }
}

/// First failing memtest cell address (0 = none); CAS-set once by any core.
static FIRST_FAIL_ADDR: AtomicU64 = AtomicU64::new(0);

const MEMTEST_PATTERNS: [u64; 8] = [
    0x0000_0000_0000_0000,
    0xFFFF_FFFF_FFFF_FFFF,
    0xAAAA_AAAA_AAAA_AAAA,
    0x5555_5555_5555_5555,
    0x3333_3333_3333_3333,
    0xCCCC_CCCC_CCCC_CCCC,
    0x0F0F_0F0F_0F0F_0F0F,
    0xF0F0_F0F0_F0F0_F0F0,
];

/// Pattern write/verify over the core's chunk. Values mix the pattern with
/// the cell address so aliased/stuck address lines are caught too.
fn memtest_kernel(sh: &Shared, slot: usize, deadline: Option<u64>) {
    let (ptr, len) = sh.chunk(slot);
    if ptr.is_null() || len < 8192 {
        return;
    }
    let me = &sh.cores[slot];
    let base = ((ptr as usize + 7) & !7) as *mut u64;
    let n = (len - 8) / 8;
    let mut pass = 0usize;
    while !sh.cancel.load(Ordering::Relaxed) {
        let pattern = MEMTEST_PATTERNS[pass % MEMTEST_PATTERNS.len()];
        unsafe {
            for i in 0..n {
                let p = base.add(i);
                p.write_volatile(pattern ^ (p as u64));
                // Cancel/deadline checks per 64 KiB to stay responsive.
                if i & 0x1FFF == 0
                    && (sh.cancel.load(Ordering::Relaxed) || expired(deadline))
                {
                    return;
                }
            }
            let mut bad = 0u64;
            for i in 0..n {
                let p = base.add(i);
                if p.read_volatile() != pattern ^ (p as u64) {
                    bad += 1;
                    let _ = FIRST_FAIL_ADDR.compare_exchange(
                        0,
                        p as u64,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                if i & 0x1FFF == 0
                    && (sh.cancel.load(Ordering::Relaxed) || expired(deadline))
                {
                    if bad > 0 {
                        me.errors.fetch_add(bad, Ordering::Relaxed);
                    }
                    return;
                }
            }
            if bad > 0 {
                me.errors.fetch_add(bad, Ordering::Relaxed);
            }
        }
        me.ops.fetch_add((n * 16) as u64, Ordering::Relaxed);
        pass += 1;
        if expired(deadline) {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// CPU package temperature (Intel DTS via MSR; ring 0 in UEFI)
// ---------------------------------------------------------------------------

pub struct TempSensor {
    tjmax: i64,
}

#[cfg(target_arch = "x86_64")]
unsafe fn rdmsr(msr: u32) -> u64 {
    let (hi, lo): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((hi as u64) << 32) | lo as u64
}

#[cfg(target_arch = "x86_64")]
unsafe fn wrmsr(msr: u32, val: u64) {
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") val as u32,
            in("edx") (val >> 32) as u32,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// CPU microcode revision. Intel: write 0 to IA32_BIOS_SIGN_ID, CPUID, read EDX.
/// AMD: low dword of MSR 0x8B. None on non-x86 or an unrecognized vendor.
pub fn cpu_microcode() -> Option<u32> {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::__cpuid;
        let v = __cpuid(0);
        let intel = v.ebx == 0x756e_6547 && v.edx == 0x4965_6e69 && v.ecx == 0x6c65_746e;
        let amd = v.ebx == 0x6874_7541 && v.edx == 0x6974_6e65 && v.ecx == 0x444d_4163;
        if intel {
            wrmsr(0x8B, 0);
            let _ = __cpuid(1);
            return Some((rdmsr(0x8B) >> 32) as u32);
        }
        if amd {
            return Some((rdmsr(0x8B) & 0xFFFF_FFFF) as u32);
        }
        None
    }
    #[cfg(not(target_arch = "x86_64"))]
    None
}

/// One machine-check bank with a logged error.
pub struct McaBank {
    pub bank: u8,
    pub status: u64,
    pub addr: u64,
}

/// Machine-check banks with a logged error (IA32_MCi_STATUS bit 63). Firmware
/// may clear these during POST. Empty on non-x86 / unrecognized vendor.
pub fn cpu_mca() -> Vec<McaBank> {
    let mut out = Vec::new();
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::__cpuid;
        let v = __cpuid(0);
        let intel = v.ebx == 0x756e_6547 && v.edx == 0x4965_6e69 && v.ecx == 0x6c65_746e;
        let amd = v.ebx == 0x6874_7541 && v.edx == 0x6974_6e65 && v.ecx == 0x444d_4163;
        if !intel && !amd {
            return out;
        }
        let count = (rdmsr(0x179) & 0xFF) as u8; // IA32_MCG_CAP.Count
        for i in 0..count.min(64) {
            let status = rdmsr(0x401 + (i as u32) * 4); // IA32_MCi_STATUS
            if status & (1 << 63) != 0 {
                let addr = if status & (1 << 58) != 0 {
                    rdmsr(0x402 + (i as u32) * 4) // IA32_MCi_ADDR
                } else {
                    0
                };
                out.push(McaBank { bank: i, status, addr });
            }
        }
    }
    out
}

impl TempSensor {
    /// GenuineIntel with a digital thermal sensor (CPUID.06H:EAX[0]) only;
    /// AMD needs family-specific SMN access and reports None for now.
    pub fn detect() -> Option<Self> {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use core::arch::x86_64::__cpuid;
            let v = __cpuid(0);
            let intel =
                v.ebx == 0x756e_6547 && v.edx == 0x4965_6e69 && v.ecx == 0x6c65_746e;
            if !intel || v.eax < 6 {
                return None;
            }
            if __cpuid(6).eax & 1 == 0 {
                return None;
            }
            // MSR_TEMPERATURE_TARGET[23:16] = TjMax; sanity-clamp to 100.
            let tj = (rdmsr(0x1A2) >> 16) & 0xFF;
            let tjmax = if (40..=125).contains(&tj) { tj as i64 } else { 100 };
            return Some(Self { tjmax });
        }
        #[cfg(not(target_arch = "x86_64"))]
        None
    }

    /// Package temp in °C from IA32_THERM_STATUS (valid bit gated).
    pub fn read(&self) -> Option<f64> {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let v = rdmsr(0x19C);
            if v & (1 << 31) != 0 {
                return Some((self.tjmax - ((v >> 16) & 0x7F) as i64) as f64);
            }
            None
        }
        #[cfg(not(target_arch = "x86_64"))]
        None
    }
}

// ---------------------------------------------------------------------------
// BSP-side engine
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StageResult {
    pub stage: Stage,
    pub secs: f64,
    pub avg_rate: f64,
    pub peak_rate: f64,
    pub errors: u64,
    /// None = unscored.
    pub pass: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Single,
    Preset,
}

struct Running {
    stage: Stage,
    started: u64,
    last_sample: u64,
    last_ops: u64,
    peak_rate: f64,
    done_event: Option<Event>,
}

pub struct StressEngine {
    shared: Box<Shared>,
    region: Vec<u8>,
    mp: Option<ScopedProtocol<MpServices>>,
    workers: usize,
    tsc_hz: u64,
    epoch: u64,
    temp: Option<TempSensor>,
    quiesced: bool,

    running: Option<Running>,
    pub mode: Mode,
    pub preset_idx: usize,
    pub selected: usize,
    pub results: Vec<StageResult>,
    pub rate_hist: History,
    pub temp_hist: History,
    pub temp_now: Option<f64>,
    pub temp_max: Option<f64>,
    pub status: String,
}

impl StressEngine {
    pub fn new() -> Self {
        let mp = open_mp();
        let workers = mp
            .as_ref()
            .and_then(|m| m.get_number_of_processors().ok())
            .map(|c| c.enabled.saturating_sub(1)) // APs only; BSP renders
            .unwrap_or(0);
        let tsc_hz = calibrate_tsc_hz();
        let temp = TempSensor::detect();
        logln(format!(
            "stress: mp={} ap_workers={} tsc={}MHz temp={}",
            mp.is_some(),
            workers,
            tsc_hz / 1_000_000,
            temp.is_some()
        ));
        Self {
            shared: Shared::new(),
            region: Vec::new(),
            mp,
            workers,
            tsc_hz: tsc_hz.max(1),
            epoch: rdtsc(),
            temp,
            quiesced: true,
            running: None,
            mode: Mode::Idle,
            preset_idx: 0,
            selected: 0,
            results: Vec::new(),
            rate_hist: History::new(CHART_WINDOW_SECS),
            temp_hist: History::new(CHART_WINDOW_SECS),
            temp_now: None,
            temp_max: None,
            status: String::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.running.is_some()
    }

    pub fn current_stage(&self) -> Option<Stage> {
        self.running.as_ref().map(|r| r.stage)
    }

    pub fn workers_label(&self) -> String {
        if self.workers > 0 {
            format!("{} AP cores + BSP ui", self.workers)
        } else {
            "BSP only (no MP services)".into()
        }
    }

    fn now_secs(&self) -> f64 {
        (rdtsc().wrapping_sub(self.epoch)) as f64 / self.tsc_hz as f64
    }

    pub fn elapsed_in_stage(&self) -> f64 {
        self.running
            .as_ref()
            .map(|r| (rdtsc().wrapping_sub(r.started)) as f64 / self.tsc_hz as f64)
            .unwrap_or(0.0)
    }

    /// Lazily allocate the scratch region all chunk-using kernels share.
    fn ensure_region(&mut self) -> bool {
        if !self.region.is_empty() {
            return true;
        }
        if !self.quiesced {
            return false;
        }
        for size in REGION_SIZES {
            let mut v: Vec<u8> = Vec::new();
            if v.try_reserve_exact(size).is_ok() {
                v.resize(size, 0);
                logln(format!("stress: region {} MiB", size >> 20));
                self.region = v;
                return true;
            }
        }
        self.status = "region alloc failed".into();
        false
    }

    pub fn start_single(&mut self, stage: Stage) {
        if self.is_active() {
            return;
        }
        self.mode = Mode::Single;
        self.results.retain(|r| r.stage != stage);
        self.start_stage(stage);
    }

    pub fn start_preset(&mut self) {
        if self.is_active() {
            return;
        }
        self.mode = Mode::Preset;
        self.preset_idx = 0;
        self.results.clear();
        self.start_stage(STAGES[0]);
    }

    fn start_stage(&mut self, stage: Stage) {
        // A zombie AP from a failed quiesce would resume the moment `cancel`
        // resets below; refuse to start anything until reboot in that case.
        if !self.quiesced && self.workers > 0 {
            self.status = "APs did not quiesce; reboot before new runs".into();
            self.mode = Mode::Idle;
            return;
        }
        if stage != Stage::CpuAlu && !self.ensure_region() {
            self.mode = Mode::Idle;
            return;
        }

        // Reset shared state for this dispatch.
        let sh = &*self.shared;
        sh.cancel.store(false, Ordering::Release);
        sh.stage.store(stage.idx(), Ordering::Release);
        sh.next_slot.store(0, Ordering::Release);
        for c in &sh.cores {
            c.ops.store(0, Ordering::Relaxed);
            c.errors.store(0, Ordering::Relaxed);
        }
        FIRST_FAIL_ADDR.store(0, Ordering::Relaxed);
        let workers = self.workers.clamp(0, MAX_CORES);
        if stage == Stage::CpuAlu {
            sh.region_ptr.store(0, Ordering::Release);
            sh.chunk_len.store(0, Ordering::Release);
        } else {
            // BSP fallback uses slot 0; chunk per worker, or whole region solo.
            let parts = workers.max(1);
            sh.region_ptr
                .store(self.region.as_ptr() as usize, Ordering::Release);
            sh.chunk_len
                .store(self.region.len() / parts, Ordering::Release);
        }

        let mut done_event = None;
        if self.workers > 0 {
            match self.dispatch_aps() {
                Ok(ev) => done_event = Some(ev),
                Err(e) => logln(format!("stress: dispatch failed: {e}")),
            }
        }
        let dispatched = done_event.is_some();
        self.quiesced = !dispatched;

        self.rate_hist.clear();
        let t = rdtsc();
        self.running = Some(Running {
            stage,
            started: t,
            last_sample: t,
            last_ops: 0,
            peak_rate: 0.0,
            done_event,
        });
        self.status = format!(
            "{} running on {}",
            stage.label(),
            if dispatched { "all APs" } else { "BSP (sliced)" }
        );
    }

    /// Non-blocking all-AP dispatch; the returned event signals completion.
    fn dispatch_aps(&mut self) -> Result<Event, String> {
        let mp = self.mp.as_ref().ok_or("no MP services")?;
        let ev = unsafe { boot::create_event(EventType::empty(), Tpl::CALLBACK, None, None) }
            .map_err(|e| format!("create_event: {e:?}"))?;
        let ev_for_call = unsafe { ev.unsafe_clone() };
        let arg = &*self.shared as *const Shared as *mut c_void;
        mp.startup_all_aps(false, ap_entry, arg, Some(ev_for_call), None)
            .map_err(|e| format!("startup_all_aps: {e:?}"))?;
        Ok(ev)
    }

    /// Stop the active stage; in preset mode the next tick starts the next
    /// stage. Returns once APs quiesced (bounded wait).
    pub fn stop(&mut self) {
        self.finish_stage(true);
        self.mode = Mode::Idle;
        self.status = "stopped".into();
    }

    fn finish_stage(&mut self, record: bool) {
        let Some(run) = self.running.take() else {
            return;
        };
        let sh = &*self.shared;
        sh.cancel.store(true, Ordering::Release);

        // Quiesce: completion event, then per-core active flags, ~3 s budget.
        let deadline = rdtsc().wrapping_add(self.tsc_hz.saturating_mul(3));
        let mut clean = self.workers == 0;
        while rdtsc() < deadline {
            let evdone = run
                .done_event
                .as_ref()
                .map(|e| boot::check_event(e).unwrap_or(false))
                .unwrap_or(false);
            let idle = !sh.cores.iter().any(|c| c.active.load(Ordering::Acquire));
            if evdone || idle {
                clean = true;
                break;
            }
            boot::stall(Duration::from_millis(2));
        }
        self.quiesced = clean;
        if !clean {
            // Leak the region rather than freeing memory APs may still touch.
            logln("stress: APs did not quiesce; leaking region".into());
            let leaked = core::mem::take(&mut self.region);
            core::mem::forget(leaked);
        }

        if record {
            let secs =
                (rdtsc().wrapping_sub(run.started)) as f64 / self.tsc_hz as f64;
            let (ops, errors) = self.totals();
            let avg = if secs > 0.05 { ops as f64 / secs } else { 0.0 };
            let pass = match run.stage {
                Stage::Memtest => Some(errors == 0 && ops > 0),
                s => s.floor().map(|f| avg >= f),
            };
            self.results.push(StageResult {
                stage: run.stage,
                secs,
                avg_rate: avg,
                peak_rate: run.peak_rate,
                errors,
                pass,
            });
        }
    }

    fn totals(&self) -> (u64, u64) {
        let sh = &*self.shared;
        let mut ops = 0u64;
        let mut errs = 0u64;
        for c in &sh.cores {
            ops = ops.wrapping_add(c.ops.load(Ordering::Relaxed));
            errs = errs.wrapping_add(c.errors.load(Ordering::Relaxed));
        }
        (ops, errs)
    }

    /// Per-frame driver: BSP fallback slice, sampling, preset sequencing.
    pub fn tick(&mut self) {
        if self.running.is_none() {
            return;
        }

        // Solo mode: run the kernel for a ~25 ms slice on the BSP.
        if self.workers == 0 {
            let stage = self.running.as_ref().map(|r| r.stage).unwrap();
            let deadline = rdtsc().wrapping_add(self.tsc_hz / 40);
            run_kernel(stage, &self.shared, 0, Some(deadline));
        }

        let now = rdtsc();
        let hz = self.tsc_hz as f64;
        let sample_due = {
            let r = self.running.as_ref().unwrap();
            (now.wrapping_sub(r.last_sample)) as f64 / hz >= SAMPLE_EVERY_SECS
        };
        if sample_due {
            let (ops, _) = self.totals();
            let t = self.now_secs();
            let r = self.running.as_mut().unwrap();
            let dt = (now.wrapping_sub(r.last_sample)) as f64 / hz;
            let rate = (ops.saturating_sub(r.last_ops)) as f64 / dt;
            r.last_sample = now;
            r.last_ops = ops;
            if rate > r.peak_rate {
                r.peak_rate = rate;
            }
            self.rate_hist.push(t, rate);

            if let Some(sensor) = &self.temp {
                if let Some(c) = sensor.read() {
                    self.temp_now = Some(c);
                    self.temp_max = Some(self.temp_max.map_or(c, |m: f64| m.max(c)));
                    self.temp_hist.push(t, c);
                }
            }
        }

        // Preset sequencing.
        if self.mode == Mode::Preset && self.elapsed_in_stage() >= PRESET_STAGE_SECS {
            self.finish_stage(true);
            self.preset_idx += 1;
            if let Some(stage) = STAGES.get(self.preset_idx).copied() {
                self.start_stage(stage);
            } else {
                self.mode = Mode::Idle;
                let passed = self
                    .results
                    .iter()
                    .all(|r| r.pass.unwrap_or(true));
                self.status = if passed {
                    "benchmark complete - PASS".into()
                } else {
                    "benchmark complete - FAIL".into()
                };
            }
        } else if self.mode == Mode::Single {
            // Single mode keeps a rolling result row up to date.
        }
    }

    /// Live rate for the footer/status line.
    pub fn live_rate(&self) -> Option<f64> {
        self.rate_hist.latest()
    }

    pub fn now_chart_secs(&self) -> f64 {
        self.now_secs()
    }

    pub fn memtest_errors(&self) -> u64 {
        self.totals().1
    }

    /// First failing memtest cell address this/last run, if any.
    pub fn memtest_fail_addr(&self) -> Option<u64> {
        match FIRST_FAIL_ADDR.load(Ordering::Relaxed) {
            0 => None,
            a => Some(a),
        }
    }

    /// `"stress"` payload appended to the fingerprint upload.
    pub fn summary_json(&self) -> Option<serde_json::Value> {
        if self.results.is_empty() {
            return None;
        }
        let stages: Vec<serde_json::Value> = self
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "stage": r.stage.label(),
                    "secs": (r.secs * 10.0).round() / 10.0,
                    "avg_rate": r.avg_rate,
                    "peak_rate": r.peak_rate,
                    "unit": r.stage.unit(),
                    "errors": r.errors,
                    "result": match r.pass {
                        Some(true) => "pass",
                        Some(false) => "fail",
                        None => "unscored",
                    },
                })
            })
            .collect();
        Some(serde_json::json!({
            "source": "uefi",
            "workers": self.workers,
            "region_mib": self.region.len() >> 20,
            "cpu_max_c": self.temp_max,
            "stages": stages,
        }))
    }
}

impl Drop for StressEngine {
    fn drop(&mut self) {
        if self.is_active() {
            self.finish_stage(false);
        }
    }
}

fn open_mp() -> Option<ScopedProtocol<MpServices>> {
    let handle = boot::get_handle_for_protocol::<MpServices>().ok()?;
    unsafe {
        boot::open_protocol::<MpServices>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()
}

/// TSC frequency from a 50 ms firmware stall; rates only need ~1% accuracy.
fn calibrate_tsc_hz() -> u64 {
    let t0 = rdtsc();
    boot::stall(Duration::from_millis(50));
    let t1 = rdtsc();
    t1.wrapping_sub(t0).saturating_mul(20)
}
