#![feature(uefi_std)]

use anyhow::Result;
use core::num::NonZeroUsize;
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
};
use uefi::Identify;
use uefi::boot;
use uefi::boot::{MemoryType, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::mem::memory_map::MemoryMap;
use uefi::proto::console;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::network::ip4config2::Ip4Config2;
use uefi::proto::network::snp::SimpleNetwork;
use uefi::proto::pci::root_bridge::PciRootBridgeIo;
use uefi::table::cfg::ConfigTableEntry;

mod bioslove;
mod bootdiag;
mod capsule;
mod charts;
mod launch;
mod hii;
mod netraw;
mod order;
mod pecheck;
mod smart;
mod smolnet;
mod stream;
mod stress;
mod styling;
mod volsig;
mod wasmrt;

/// Built-in demo plugin (Mastertech ABI) for the Plugins tab self-test.
const DEMO_PLUGIN: &[u8] = include_bytes!("demo_plugin.wasm");

/// Semantic palette derived from the terminal-mode theme (see `styling`).
/// RGB values are quantized to the nearest EFI text color by ratatui-uefi.
mod palette {
    use crate::styling::{APP_BACKGROUND, CATPPUCCIN, THEME};
    use ratatui::style::Color;
    pub const BG: Color = APP_BACKGROUND; // near-black app background
    pub const TEXT: Color = THEME.text; // catppuccin text
    pub const MUTED: Color = CATPPUCCIN.overlay1; // section headers / footer
    pub const LABEL: Color = CATPPUCCIN.sapphire; // field labels
    pub const ACCENT: Color = THEME.accent; // deep pink identity
    pub const BORDER: Color = THEME.border_idle(); // idle panel borders
    pub const GOOD: Color = THEME.success; // present / yes / link up
    pub const BAD: Color = CATPPUCCIN.overlay0; // absent / no
    pub const ERR: Color = THEME.error; // error / link down
    pub const WARN: Color = CATPPUCCIN.peach; // caution / non-fatal finding
}

/// Default upload endpoint, baked from `UEFI_TARGET_URL` (process env or
/// workspace `.env`) at build time; defaults to the LAN pre-boot relay.
const DEFAULT_URL: &str = env!("UEFI_TARGET_URL");

const TABS: [&str; 15] = [
    "Overview",
    "System",
    "Memory",
    "Firmware",
    "BIOS",
    "Network",
    "Storage",
    "Stress",
    "Order",
    "Readiness",
    "Diag",
    "Boot",
    "Plugins",
    "Log",
    "Flash",
];

const TAB_FIRMWARE: usize = 3;
const TAB_BIOS: usize = 4;
const TAB_NETWORK: usize = 5;
const TAB_STRESS: usize = 7;
const TAB_ORDER: usize = 8;
const TAB_BOOT: usize = 11;
const TAB_PLUGINS: usize = 12;
const TAB_LOG: usize = 13;
const TAB_FLASH: usize = 14;

/// Milliseconds between command polls while agent mode is on.
const AGENT_POLL_MS: u64 = 5_000;

/// Milliseconds between remote-input polls while streaming and idle.
const PREBOOT_INPUT_POLL_MS: u64 = 400;

/// Milliseconds between remote-input polls while a viewer is typing.
const PREBOOT_INPUT_FAST_MS: u64 = 33;

/// Milliseconds of fast remote-input polling after each event received.
const PREBOOT_INPUT_ACTIVE_MS: u64 = 1_000;

/// Milliseconds after a handled key that the loop keeps its short stall.
const KEY_ACTIVE_MS: u64 = 2_000;

/// Milliseconds between presence heartbeats.
const PRESENCE_HEARTBEAT_MS: u64 = 45_000;

/// Milliseconds between relay viewer-flag checks.
const VIEWER_CHECK_MS: u64 = 5_000;

/// Base milliseconds between console-discovery attempts.
const DIRECT_DISCOVER_MS: u64 = 15_000;

/// Milliseconds before the first discovery attempt after presence is armed.
const DIRECT_DISCOVER_FIRST_MS: u64 = 1_000;

/// Ceiling for the doubling discovery backoff.
const DIRECT_DISCOVER_MAX_MS: u64 = 60_000;

/// UDP beacon listen budget per discovery attempt.
const DISCOVER_LISTEN_MS: u64 = 250;

/// Listen budget ceiling once the backoff has spread attempts out; one full
/// ~3s console beacon period, so a widened attempt catches a beacon.
const DISCOVER_LISTEN_MAX_MS: u64 = 3_200;

/// Listen budget for the one-shot discovery pass fired by 'c' and 'd'; spans a
/// full ~3s console beacon period so the relay is learned on the first try.
const DISCOVER_LISTEN_KICK_MS: u64 = 3_200;

/// Relay port assumed for a v1 beacon (axum_server's default listener).
const AXUM_DEFAULT_PORT: u16 = 8082;

/// Milliseconds between direct-socket service polls while the console streams.
const DIRECT_PUMP_MS: u64 = 33;

/// Milliseconds between direct-socket service polls while it is only linked.
const DIRECT_PUMP_IDLE_MS: u64 = 200;

/// Direct-link listener port on the console (mirror of displays' DIRECT_PORT).
const DIRECT_PORT: u16 = 9209;

/// Idle gap after which the direct link sends a keepalive ping. A static screen
/// emits no frames, so without this the console sees the session as stale.
const DIRECT_KEEPALIVE_MS: u64 = 10_000;

/// Budget for one TCP connect or transmit; a dropped SYN costs the loop all of it.
const TCP_CONNECT_TIMEOUT_MS: u32 = 10_000;

/// Redraw floor so live values keep ticking without a state change.
const REDRAW_MIN_MS: u64 = 200;

/// Stall ceiling while a stress run, a viewer or the operator needs the loop hot.
const BUSY_STALL_MS: u64 = 20;

/// Stall ceiling when no deadline is imminent.
const IDLE_STALL_MAX_MS: u64 = 250;

/// Stall floor, so the loop always yields to firmware timers.
const MIN_STALL_MS: u64 = 1;

/// Monotonic millisecond clock over the TSC, falling back to the milliseconds
/// the loop itself stalled when the TSC reads as unusable.
struct Clock {
    hz: u64,
    epoch: u64,
    tsc_ok: bool,
    stalled_ms: u64,
}

impl Clock {
    fn new() -> Self {
        let hz = stress::calibrate_tsc_hz();
        Self {
            hz: hz.max(1),
            epoch: stress::rdtsc(),
            tsc_ok: hz >= 1_000_000,
            stalled_ms: 0,
        }
    }

    fn now_ms(&self) -> u64 {
        if !self.tsc_ok {
            return self.stalled_ms;
        }
        let ticks = stress::rdtsc().wrapping_sub(self.epoch) as u128;
        (ticks * 1000 / self.hz as u128) as u64
    }

    fn stall(&mut self, ms: u64) {
        boot::stall(core::time::Duration::from_millis(ms));
        self.stalled_ms = self.stalled_ms.saturating_add(ms);
    }
}

/// In-memory ring log. Single-threaded app, but a Mutex keeps it simple and
/// also lets us back the `log` facade (so the uefi crate's own debug!/trace!
/// messages land here too).
static LOG: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Bumped on every appended line; the run loop redraws when it moves.
static LOG_SEQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn logln(s: String) {
    if let Ok(mut g) = LOG.lock() {
        g.push(s);
        let n = g.len();
        if n > 600 {
            g.drain(0..n - 500);
        }
    }
    LOG_SEQ.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

fn log_seq() -> u64 {
    LOG_SEQ.load(core::sync::atomic::Ordering::Relaxed)
}

fn log_snapshot() -> Vec<String> {
    LOG.lock().map(|g| g.clone()).unwrap_or_default()
}

struct BufLogger;

impl log::Log for BufLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        logln(format!("[{}] {}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

static LOGGER: BufLogger = BufLogger;

/// Mandatory setup so the `uefi` crate's global system table / image handle are
/// populated when running as a `uefi_std` binary.
fn setup_uefi_crate() {
    let system_table = std::os::uefi::env::system_table();
    let image_handle = std::os::uefi::env::image_handle();

    unsafe {
        uefi::table::set_system_table(system_table.as_ptr().cast());
        let ih = uefi::Handle::from_ptr(image_handle.as_ptr().cast()).unwrap();
        uefi::boot::set_image_handle(ih);
    }
}

fn init_ratatui_perf() {
    Layout::init_cache(NonZeroUsize::new(128).unwrap());
}

fn create_ui() -> Result<(
    Terminal<ratatui_uefi::UefiOutputBackend>,
    terminput_uefi::UefiInputReader,
)> {
    // Output: a single device handle, not the ConSplitter (stdout_handle) —
    // routing through the splitter breaks clear()/mode reporting on some
    // firmware. Prefer the handle that also carries GraphicsOutput: that is
    // the physical screen console. Handle-database order is firmware-specific
    // and the first Output handle can be a serial terminal or the StdErr
    // splitter (e.g. OVMF, AMT/BMC serial-over-LAN boards), which renders
    // nowhere visible.
    let output_handles = uefi::boot::find_handles::<console::text::Output>()?;
    let gop_backed = output_handles.iter().copied().find(|&h| {
        uefi::boot::protocols_per_handle(h)
            .map(|protos| protos.iter().any(|&&g| g == GraphicsOutput::GUID))
            .unwrap_or(false)
    });
    let output_handle = match gop_backed {
        Some(h) => h,
        None => uefi::boot::get_handle_for_protocol::<console::text::Output>()?,
    };
    let mut output = uefi::boot::open_protocol_exclusive::<console::text::Output>(output_handle)?;
    logln(format!("console: render handle={output_handle:?}"));

    // Switch to the largest text mode the console supports so the TUI fills
    // the panel (GOP consoles boot in 80x25). ratatui re-reads the backend
    // size on every draw, so nothing else needs to know.
    let current_cells = output
        .current_mode()
        .ok()
        .flatten()
        .map(|m| m.columns() * m.rows())
        .unwrap_or(0);
    let mut modes: Vec<_> = output.modes().collect();
    modes.sort_by_key(|m| core::cmp::Reverse(m.columns() * m.rows()));
    for mode in modes {
        if mode.columns() * mode.rows() <= current_cells {
            break;
        }
        // Firmware can list modes it then refuses to set; walk down the list.
        if output.set_mode(mode).is_ok() {
            logln(format!("console: mode {}x{}", mode.columns(), mode.rows()));
            break;
        }
    }

    // Input: use the firmware's aggregated ConIn (stdin_handle from the system
    // table), NOT a single device — so an external USB keyboard works alongside
    // the built-in one. Fall back to a single Input handle if that's not usable.
    let con_in = unsafe {
        let st = &*(std::os::uefi::env::system_table().as_ptr()
            as *const uefi_raw::table::system::SystemTable);
        uefi::Handle::from_ptr(st.stdin_handle)
    };
    let input = match con_in {
        Some(in_handle) => unsafe {
            uefi::boot::open_protocol::<console::text::Input>(
                OpenProtocolParams {
                    handle: in_handle,
                    agent: uefi::boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        }
        .or_else(|_| {
            let h = uefi::boot::get_handle_for_protocol::<console::text::Input>()?;
            uefi::boot::open_protocol_exclusive::<console::text::Input>(h)
        })?,
        None => {
            let h = uefi::boot::get_handle_for_protocol::<console::text::Input>()?;
            uefi::boot::open_protocol_exclusive::<console::text::Input>(h)?
        }
    };
    logln(format!("console: ConIn handle={con_in:?}"));

    let mut terminal = Terminal::new(ratatui_uefi::UefiOutputBackend::new(output))?;
    let input_reader = terminput_uefi::UefiInputReader::new(input);

    Ok((terminal, input_reader))
}

// ---------------------------------------------------------------------------
// SMBIOS / DMI parsing
// ---------------------------------------------------------------------------

/// One raw SMBIOS structure: its formatted area plus its trailing string set.
struct SmbiosStruct {
    ty: u8,
    formatted: Vec<u8>,
    strings: Vec<String>,
}

impl SmbiosStruct {
    /// SMBIOS string references are 1-based; index 0 means "no string".
    fn string(&self, idx: u8) -> String {
        if idx == 0 {
            return String::new();
        }
        self.strings
            .get(idx as usize - 1)
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    fn str_at(&self, off: usize) -> String {
        self.string(self.formatted.get(off).copied().unwrap_or(0))
    }

    fn u8_at(&self, off: usize) -> u8 {
        self.formatted.get(off).copied().unwrap_or(0)
    }

    fn u16_at(&self, off: usize) -> u16 {
        if off + 1 < self.formatted.len() {
            u16::from_le_bytes([self.formatted[off], self.formatted[off + 1]])
        } else {
            0
        }
    }

    fn u32_at(&self, off: usize) -> u32 {
        if off + 3 < self.formatted.len() {
            u32::from_le_bytes([
                self.formatted[off],
                self.formatted[off + 1],
                self.formatted[off + 2],
                self.formatted[off + 3],
            ])
        } else {
            0
        }
    }

    /// Type 1 system UUID (offset 0x08). First three fields are little-endian
    /// per SMBIOS >= 2.6.
    fn uuid(&self) -> String {
        if self.formatted.len() < 0x18 {
            return String::new();
        }
        let u = &self.formatted[0x08..0x18];
        format!(
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            u[3], u[2], u[1], u[0], u[5], u[4], u[7], u[6], u[8], u[9], u[10], u[11], u[12], u[13],
            u[14], u[15]
        )
    }
}

unsafe fn entry_point_3(p: usize) -> Option<(usize, usize)> {
    let b = unsafe { core::slice::from_raw_parts(p as *const u8, 0x18) };
    if &b[0..5] != b"_SM3_" {
        return None;
    }
    let max = u32::from_le_bytes([b[0x0C], b[0x0D], b[0x0E], b[0x0F]]) as usize;
    let addr = u64::from_le_bytes([
        b[0x10], b[0x11], b[0x12], b[0x13], b[0x14], b[0x15], b[0x16], b[0x17],
    ]) as usize;
    Some((addr, max))
}

unsafe fn entry_point_2(p: usize) -> Option<(usize, usize)> {
    let b = unsafe { core::slice::from_raw_parts(p as *const u8, 0x1F) };
    if &b[0..4] != b"_SM_" {
        return None;
    }
    let len = u16::from_le_bytes([b[0x16], b[0x17]]) as usize;
    let addr = u32::from_le_bytes([b[0x18], b[0x19], b[0x1A], b[0x1B]]) as usize;
    Some((addr, len))
}

fn read_table_bytes() -> Option<Vec<u8>> {
    let mut ep3 = 0usize;
    let mut ep2 = 0usize;
    uefi::system::with_config_table(|entries| {
        for e in entries {
            if e.guid == ConfigTableEntry::SMBIOS3_GUID {
                ep3 = e.address as usize;
            } else if e.guid == ConfigTableEntry::SMBIOS_GUID {
                ep2 = e.address as usize;
            }
        }
    });

    unsafe {
        let region = (ep3 != 0)
            .then(|| entry_point_3(ep3))
            .flatten()
            .or_else(|| (ep2 != 0).then(|| entry_point_2(ep2)).flatten());

        match region {
            Some((addr, len)) if addr != 0 && len != 0 && len < 16 * 1024 * 1024 => {
                Some(core::slice::from_raw_parts(addr as *const u8, len).to_vec())
            }
            _ => None,
        }
    }
}

fn parse_structures(table: &[u8]) -> Vec<SmbiosStruct> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 <= table.len() {
        let ty = table[i];
        let flen = table[i + 1] as usize;
        if flen < 4 || i + flen > table.len() {
            break;
        }
        let formatted = table[i..i + flen].to_vec();

        let mut j = i + flen;
        let mut strings = Vec::new();
        if j + 1 < table.len() && table[j] == 0 && table[j + 1] == 0 {
            j += 2;
        } else {
            while j < table.len() {
                let start = j;
                while j < table.len() && table[j] != 0 {
                    j += 1;
                }
                strings.push(String::from_utf8_lossy(&table[start..j]).into_owned());
                j += 1;
                if j < table.len() && table[j] == 0 {
                    j += 1;
                    break;
                }
            }
        }

        out.push(SmbiosStruct {
            ty,
            formatted,
            strings,
        });
        if ty == 127 || out.len() > 2048 {
            break;
        }
        i = j;
    }
    out
}

/// (total, in_use) expansion slots from SMBIOS type 9 (Current Usage 4 = In Use).
fn collect_slots() -> (usize, usize) {
    let Some(table) = read_table_bytes() else {
        return (0, 0);
    };
    let mut total = 0;
    let mut used = 0;
    for s in parse_structures(&table) {
        if s.ty == 9 {
            total += 1;
            if s.u8_at(0x07) == 4 {
                used += 1;
            }
        }
    }
    (total, used)
}

/// SMBIOS memory error type (type 18/33 offset 0x04).
fn mem_err_type(c: u8) -> &'static str {
    match c {
        1 => "Other",
        2 => "Unknown",
        3 => "OK",
        4 => "Bad read",
        5 => "Parity",
        6 => "Single-bit",
        7 => "Double-bit",
        8 => "Multi-bit",
        9 => "Nibble",
        0x0A => "Checksum",
        0x0B => "CRC",
        0x0C => "Corrected single-bit",
        0x0D => "Corrected",
        0x0E => "Uncorrectable",
        _ => "?",
    }
}

/// One logged memory error from SMBIOS type 18 (32-bit) / 33 (64-bit).
struct MemError {
    kind: &'static str,
    addr: u64,
}

/// Logged memory errors (type 18/33), excluding OK/Unknown no-error entries.
fn collect_mem_errors() -> Vec<MemError> {
    let Some(table) = read_table_bytes() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in parse_structures(&table) {
        let t = match s.ty {
            18 | 33 => s.u8_at(0x04),
            _ => continue,
        };
        if t == 0x02 || t == 0x03 {
            continue;
        }
        let addr = if s.ty == 18 {
            s.u32_at(0x0B) as u64
        } else {
            (s.u32_at(0x0B) as u64) | ((s.u32_at(0x0F) as u64) << 32)
        };
        out.push(MemError { kind: mem_err_type(t), addr });
    }
    out
}

fn mem_type_name(code: u8) -> &'static str {
    match code {
        0x03 => "DRAM",
        0x12 => "SDRAM",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x20 => "HBM",
        0x21 => "HBM2",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        0x24 => "HBM3",
        _ => "RAM",
    }
}

struct Dimm {
    locator: String,
    bank: String,
    size: String,
    speed: u16,
    cfg_speed: u16,
    mtype: String,
    mfr: String,
    part: String,
    serial: String,
}

#[derive(Default)]
struct Smbios {
    present: bool,
    sys_mfr: String,
    sys_product: String,
    sys_version: String,
    sys_serial: String,
    sys_uuid: String,
    sys_sku: String,
    sys_family: String,
    board_mfr: String,
    board_product: String,
    board_version: String,
    board_serial: String,
    board_asset: String,
    cpu_socket: String,
    cpu_version: String,
    cpu_cores: u16,
    cpu_threads: u16,
    dimms: Vec<Dimm>,
    bios_vendor: String,
    bios_version: String,
    bios_date: String,
    chassis_type: String,
    /// Type 3 chassis type code with the lock bit masked off.
    chassis_code: u8,
    chassis_serial: String,
    chassis_asset: String,
    oem_strings: Vec<String>,
    batteries: Vec<Battery>,
    mem_max_bytes: u64,
    mem_slots: u16,
    mem_ecc: String,
}

impl Smbios {
    /// Chassis types that run on a battery (Type 3 codes).
    fn is_portable(&self) -> bool {
        matches!(self.chassis_code, 8..=11 | 14 | 30..=32)
    }
}

/// SMBIOS Type 22 portable battery. Static at boot: proves a bay exists and
/// gives design capacity, but never the current charge.
#[derive(Default)]
struct Battery {
    location: String,
    manufacturer: String,
    device_name: String,
    design_voltage_mv: u16,
    design_capacity_mwh: u32,
}

/// SMBIOS Type 3 chassis type code -> name (subset).
fn chassis_name(code: u8) -> &'static str {
    match code & 0x7F {
        0x03 => "Desktop",
        0x04 => "Low Profile Desktop",
        0x06 => "Mini Tower",
        0x07 => "Tower",
        0x08 => "Portable",
        0x09 => "Laptop",
        0x0A => "Notebook",
        0x0B => "Hand Held",
        0x0E => "Sub Notebook",
        0x11 => "Rack Mount Chassis",
        0x1E => "Tablet",
        0x1F => "Convertible",
        0x20 => "Detachable",
        _ => "Other",
    }
}

/// SMBIOS Type 16 memory-array error-correction code -> name.
fn ecc_name(code: u8) -> &'static str {
    match code {
        0x03 => "None",
        0x04 => "Parity",
        0x05 => "Single-bit ECC",
        0x06 => "Multi-bit ECC",
        0x07 => "CRC",
        _ => "Unknown",
    }
}

/// Serial placeholders OEMs leave when the real value is blank (mirrors the
/// qc-app list). Sub-4-char values are treated as placeholders too.
fn is_placeholder_serial(s: &str) -> bool {
    let t = s.trim();
    if t.len() < 4 {
        return true;
    }
    const PLACEHOLDERS: &[&str] = &[
        "to be filled by o.e.m.",
        "default string",
        "system serial number",
        "base board serial number",
        "chassis serial number",
        "none",
        "n/a",
        "not applicable",
        "not specified",
        "00000000",
        "123456789",
        // TongFang/Clevo barebone factory default.
        "1558",
        "...",
    ];
    let l = t.to_ascii_lowercase();
    PLACEHOLDERS.iter().any(|p| l == *p)
}

/// Firmware-readable order/identity serial. Chassis serial (burned on new
/// builds) wins; the ACPI MSDM OA3 key is the always-present fallback matching
/// PrestaShop `order_serial.serial_number` (same path as `request_prestashop`).
/// SMBIOS system/board/asset serials rank below both.
fn effective_serial(info: &SysInfo) -> String {
    let d = &info.dmi;
    if !is_placeholder_serial(&d.chassis_serial) {
        return d.chassis_serial.trim().to_string();
    }
    let key = info.msdm_key.trim();
    if !key.is_empty() {
        return key.to_string();
    }
    for cand in [&d.sys_serial, &d.board_serial, &d.chassis_asset] {
        if !is_placeholder_serial(cand) {
            return cand.trim().to_string();
        }
    }
    String::new()
}

fn collect_smbios() -> Smbios {
    let mut s = Smbios::default();
    let Some(bytes) = read_table_bytes() else {
        return s;
    };
    let structs = parse_structures(&bytes);
    if structs.is_empty() {
        return s;
    }
    s.present = true;

    for st in &structs {
        match st.ty {
            1 => {
                s.sys_mfr = st.str_at(0x04);
                s.sys_product = st.str_at(0x05);
                s.sys_version = st.str_at(0x06);
                s.sys_serial = st.str_at(0x07);
                s.sys_uuid = st.uuid();
                s.sys_sku = st.str_at(0x19);
                s.sys_family = st.str_at(0x1A);
            }
            2 => {
                s.board_mfr = st.str_at(0x04);
                s.board_product = st.str_at(0x05);
                s.board_version = st.str_at(0x06);
                s.board_serial = st.str_at(0x07);
                s.board_asset = st.str_at(0x08);
            }
            4 => {
                s.cpu_socket = st.str_at(0x04);
                s.cpu_version = st.str_at(0x10);
                let c8 = st.u8_at(0x23);
                s.cpu_cores = if c8 == 0xFF { st.u16_at(0x2A) } else { c8 as u16 };
                let t8 = st.u8_at(0x25);
                s.cpu_threads = if t8 == 0xFF { st.u16_at(0x2C) } else { t8 as u16 };
            }
            17 => {
                let raw = st.u16_at(0x0C);
                if raw == 0 {
                    continue;
                }
                let size = if raw == 0x7FFF {
                    human_bytes(st.u32_at(0x1C) as u64 * 1024 * 1024)
                } else if raw == 0xFFFF {
                    "unknown".to_string()
                } else if raw & 0x8000 != 0 {
                    human_bytes((raw & 0x7FFF) as u64 * 1024)
                } else {
                    human_bytes(raw as u64 * 1024 * 1024)
                };
                s.dimms.push(Dimm {
                    locator: st.str_at(0x10),
                    bank: st.str_at(0x11),
                    size,
                    speed: st.u16_at(0x15),
                    cfg_speed: st.u16_at(0x20),
                    mtype: mem_type_name(st.u8_at(0x12)).to_string(),
                    mfr: st.str_at(0x17),
                    part: st.str_at(0x1A),
                    serial: st.str_at(0x18),
                });
            }
            0 => {
                s.bios_vendor = st.str_at(0x04);
                s.bios_version = st.str_at(0x05);
                s.bios_date = st.str_at(0x08);
            }
            3 => {
                // Bit 7 of the type byte is the chassis-lock flag.
                s.chassis_code = st.u8_at(0x05) & 0x7F;
                s.chassis_type = chassis_name(st.u8_at(0x05)).to_string();
                s.chassis_serial = st.str_at(0x07);
                s.chassis_asset = st.str_at(0x08);
            }
            11 => {
                s.oem_strings = st.strings.clone();
            }
            22 => {
                // DesignCapacity is mWh scaled by the multiplier when the SBDS
                // fields are used (design capacity 0 means "see SBDS").
                let raw_cap = st.u16_at(0x0F) as u32;
                let mult = st.u8_at(0x15) as u32;
                s.batteries.push(Battery {
                    location: st.str_at(0x04),
                    manufacturer: st.str_at(0x05),
                    device_name: st.str_at(0x08),
                    design_voltage_mv: st.u16_at(0x0A),
                    design_capacity_mwh: raw_cap * if mult == 0 { 1 } else { mult },
                });
            }
            16 => {
                // Max capacity: 0x07 is KB as u32; if 0x80000000 use the
                // extended u64 (bytes) at 0x0F.
                let cap_kb = st.u32_at(0x07);
                s.mem_max_bytes = if cap_kb == 0x8000_0000 {
                    let lo = st.u32_at(0x0F) as u64;
                    let hi = st.u32_at(0x13) as u64;
                    lo | (hi << 32) // extended capacity is already in bytes
                } else {
                    cap_kb as u64 * 1024
                };
                s.mem_ecc = ecc_name(st.u8_at(0x06)).to_string();
                s.mem_slots = st.u16_at(0x0D);
            }
            _ => {}
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Network (SimpleNetwork) enumeration
// ---------------------------------------------------------------------------

struct Nic {
    mac: String,
    media_present: bool,
    media_supported: bool,
    if_type: u8,
    mtu: u32,
    state: String,
}

fn fmt_mac(m: &uefi::proto::network::EfiMacAddr) -> String {
    // NB: don't use MacAddress::into_ethernet_addr() — in uefi-raw 0.14 it
    // copies octets()[6..] (26 bytes) into a [u8; 6] and panics. Slice the
    // first 6 octets ourselves.
    let o = m.octets();
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        o[0], o[1], o[2], o[3], o[4], o[5]
    )
}

fn collect_nics() -> Vec<Nic> {
    let mut out = Vec::new();
    let Ok(handles) = uefi::boot::find_handles::<SimpleNetwork>() else {
        logln("net: no SNP handles (no NIC driver connected)".into());
        return out;
    };
    let found = handles.len();
    let mut unopenable = 0usize;
    for handle in handles {
        let params = OpenProtocolParams {
            handle,
            agent: uefi::boot::image_handle(),
            controller: None,
        };
        // Non-exclusive read so we don't wrestle the protocol away from the
        // firmware's network stack.
        let snp = match unsafe {
            uefi::boot::open_protocol::<SimpleNetwork>(params, OpenProtocolAttributes::GetProtocol)
        } {
            Ok(s) => s,
            Err(_) => {
                unopenable += 1;
                continue;
            }
        };
        let m = snp.mode();
        let mac = fmt_mac(&m.current_address);
        // One physical NIC can surface as several SNP handles with the same
        // MAC; keep only the first of each address.
        if out.iter().any(|n: &Nic| n.mac == mac) {
            continue;
        }
        out.push(Nic {
            mac,
            media_present: bool::from(m.media_present),
            media_supported: bool::from(m.media_present_supported),
            if_type: m.if_type,
            mtu: m.max_packet_size,
            state: format!("{:?}", m.state),
        });
    }
    if out.is_empty() && found > 0 {
        logln(format!(
            "net: {found} SNP handle(s) present but {unopenable} could not be opened"
        ));
    }
    out
}

/// A network controller found by walking the PCI bus directly. This works even
/// when no UEFI network driver is bound (so SNP is absent), which is the whole
/// point: it proves the NIC hardware exists and identifies it.
struct PciNet {
    loc: String,
    vendor: u16,
    device: u16,
    kind: &'static str,
}

fn pci_vendor_name(v: u16) -> &'static str {
    match v {
        0x1002 => "AMD/ATI",
        0x8086 => "Intel",
        0x10EC => "Realtek",
        0x14E4 => "Broadcom",
        0x1969 => "Qualcomm Atheros",
        0x168C => "Atheros",
        0x17CB => "Qualcomm",
        0x1D6A => "Aquantia",
        0x15B3 => "Mellanox",
        0x14C3 => "MediaTek",
        0x10DE => "NVIDIA",
        0x1106 => "VIA",
        0x1AF4 => "Red Hat/Virtio",
        _ => "vendor",
    }
}

fn collect_pci_net() -> Vec<PciNet> {
    let mut out = Vec::new();
    let Ok(handles) = uefi::boot::find_handles::<PciRootBridgeIo>() else {
        return out;
    };
    for h in handles {
        let params = OpenProtocolParams {
            handle: h,
            agent: uefi::boot::image_handle(),
            controller: None,
        };
        let mut root = match unsafe {
            uefi::boot::open_protocol::<PciRootBridgeIo>(params, OpenProtocolAttributes::GetProtocol)
        } {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(tree) = root.enumerate() else {
            continue;
        };
        for addr in tree.iter() {
            let mut a = *addr;
            a.reg = 0; // vendor/device ID dword
            let Ok(id) = root.pci().read_one::<u32>(a) else {
                continue;
            };
            let vendor = (id & 0xFFFF) as u16;
            let device = (id >> 16) as u16;
            if vendor == 0xFFFF || vendor == 0 {
                continue;
            }
            a.reg = 0x08; // revision / class-code dword
            let Ok(cls) = root.pci().read_one::<u32>(a) else {
                continue;
            };
            let base = ((cls >> 24) & 0xFF) as u8;
            let sub = ((cls >> 16) & 0xFF) as u8;
            if base != 0x02 {
                continue; // not a network controller
            }
            out.push(PciNet {
                loc: format!("{:02x}:{:02x}.{}", a.bus, a.dev, a.fun),
                vendor,
                device,
                kind: match sub {
                    0x00 => "Ethernet",
                    0x80 => "Wireless/Other",
                    _ => "Network",
                },
            });
        }
    }
    out
}

/// A PCI display controller (GPU). The friendly marketing name is an OS-driver
/// string we can't see pre-OS, so we report vendor + device id.
struct Gpu {
    vendor: u16,
    device: u16,
    integrated: bool,
}

fn collect_gpus() -> Vec<Gpu> {
    let mut out = Vec::new();
    let Ok(handles) = uefi::boot::find_handles::<PciRootBridgeIo>() else {
        return out;
    };
    for h in handles {
        let params = OpenProtocolParams {
            handle: h,
            agent: uefi::boot::image_handle(),
            controller: None,
        };
        let mut root = match unsafe {
            uefi::boot::open_protocol::<PciRootBridgeIo>(params, OpenProtocolAttributes::GetProtocol)
        } {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(tree) = root.enumerate() else {
            continue;
        };
        for addr in tree.iter() {
            let mut a = *addr;
            a.reg = 0;
            let Ok(id) = root.pci().read_one::<u32>(a) else {
                continue;
            };
            let vendor = (id & 0xFFFF) as u16;
            let device = (id >> 16) as u16;
            if vendor == 0xFFFF || vendor == 0 {
                continue;
            }
            a.reg = 0x08;
            let Ok(cls) = root.pci().read_one::<u32>(a) else {
                continue;
            };
            let base = ((cls >> 24) & 0xFF) as u8;
            if base != 0x03 {
                continue; // not a display controller
            }
            out.push(Gpu {
                vendor,
                device,
                // bus 0 is the root complex -> integrated graphics.
                integrated: a.bus == 0,
            });
        }
    }
    out
}

/// Negotiated vs capable PCIe link for downstream GPU/storage/network devices.
/// A GPU at x4/Gen3 instead of x16/Gen5 (or a reseated-but-loose NVMe) shows here.
struct PcieLink {
    loc: String,
    class_label: &'static str,
    max_width: u8,
    max_speed: u8,
    cur_width: u8,
    cur_speed: u8,
    /// Device Status error-detected bits [3:0]: corr/non-fatal/fatal/unsupported.
    dev_err: u8,
    /// AER Correctable Error Status bitmask (0 = AER absent or clean).
    aer_corr: u32,
    /// AER Uncorrectable Error Status bitmask (0 = AER absent or clean).
    aer_uncorr: u32,
}

fn collect_pcie_links() -> Vec<PcieLink> {
    let mut out = Vec::new();
    let Ok(handles) = uefi::boot::find_handles::<PciRootBridgeIo>() else {
        return out;
    };
    for h in handles {
        let params = OpenProtocolParams {
            handle: h,
            agent: uefi::boot::image_handle(),
            controller: None,
        };
        let mut root = match unsafe {
            uefi::boot::open_protocol::<PciRootBridgeIo>(params, OpenProtocolAttributes::GetProtocol)
        } {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(tree) = root.enumerate() else {
            continue;
        };
        for addr in tree.iter() {
            macro_rules! rd {
                ($off:expr) => {{
                    let off = $off as u64;
                    let mut a = *addr;
                    if off < 0x100 {
                        a.reg = off as u8;
                        a.ext_reg = 0;
                    } else {
                        a.reg = 0;
                        a.ext_reg = off as u32;
                    }
                    root.pci().read_one::<u32>(a).ok()
                }};
            }
            let Some(id) = rd!(0x00) else { continue };
            if (id & 0xFFFF) == 0xFFFF || (id & 0xFFFF) == 0 {
                continue;
            }
            let Some(cls) = rd!(0x08) else { continue };
            let base = ((cls >> 24) & 0xFF) as u8;
            let sub = ((cls >> 16) & 0xFF) as u8;
            let class_label = match base {
                0x03 => "GPU",
                0x02 => "Network",
                0x01 if sub == 0x08 => "NVMe",
                0x01 => "Storage",
                _ => continue,
            };
            let Some(cmd_status) = rd!(0x04) else { continue };
            if (cmd_status >> 16) & 0x10 == 0 {
                continue; // no capability list
            }
            let Some(cap_ptr) = rd!(0x34) else { continue };
            let mut ptr = (cap_ptr & 0xFF) as u64;
            let mut pcie_cap = None;
            let mut guard = 0;
            while (0x40..0x100).contains(&ptr) && guard < 48 {
                guard += 1;
                let Some(cap) = rd!(ptr & 0xFC) else { break };
                if (cap & 0xFF) as u8 == 0x10 {
                    pcie_cap = Some(ptr);
                    break;
                }
                let next = ((cap >> 8) & 0xFF) as u64;
                if next == 0 || next == ptr {
                    break;
                }
                ptr = next;
            }
            let Some(cap) = pcie_cap else { continue };
            let Some(linkcap) = rd!(cap + 0x0C) else { continue };
            let Some(linksts) = rd!(cap + 0x10) else { continue };
            // Device Status error-detected bits [3:0] (upper half of cap+0x08 dword).
            let dev_err = rd!(cap + 0x08).map(|d| ((d >> 16) & 0xF) as u8).unwrap_or(0);
            // Walk extended caps (>= 0x100) for AER (ext cap id 0x0001).
            let (mut aer_corr, mut aer_uncorr) = (0u32, 0u32);
            let mut eptr = 0x100u64;
            let mut eguard = 0;
            while (0x100..0x1000).contains(&eptr) && eguard < 64 {
                eguard += 1;
                let Some(hdr) = rd!(eptr) else { break };
                if hdr == 0 || hdr == 0xFFFF_FFFF {
                    break;
                }
                if (hdr & 0xFFFF) == 0x0001 {
                    aer_uncorr = rd!(eptr + 0x04).unwrap_or(0);
                    aer_corr = rd!(eptr + 0x10).unwrap_or(0);
                    break;
                }
                let next = ((hdr >> 20) & 0xFFF) as u64;
                if next == 0 || next == eptr {
                    break;
                }
                eptr = next;
            }
            out.push(PcieLink {
                loc: format!("{:02x}:{:02x}.{}", addr.bus, addr.dev, addr.fun),
                class_label,
                max_speed: (linkcap & 0xF) as u8,
                max_width: ((linkcap >> 4) & 0x3F) as u8,
                cur_speed: ((linksts >> 16) & 0xF) as u8,
                cur_width: ((linksts >> 20) & 0x3F) as u8,
                dev_err,
                aer_corr,
                aer_uncorr,
            });
        }
    }
    out
}

/// Bind the MNP→IP4 stack onto already-bound NICs, and fall back to a full
/// `connect -r` when no SNP handle exists at all.
///
/// The narrow pass only reaches controllers that already expose SNP, so it
/// cannot bind a NIC that BDS never connected (one that isn't in the boot
/// order, or was skipped by Fast Boot). The wide pass is the UEFI shell's
/// `connect -r`; it is kept for that case only, because connecting every
/// controller can stall on an unrelated flaky device. Neither can conjure a
/// driver the firmware doesn't have (network stack disabled in setup).
fn connect_network_stack() {
    let snp_handles = uefi::boot::find_handles::<SimpleNetwork>().unwrap_or_default();
    if !snp_handles.is_empty() {
        for h in snp_handles {
            let _ = uefi::boot::connect_controller(h, &[], None, true);
        }
        return;
    }

    let Ok(all) = uefi::boot::locate_handle_buffer(uefi::boot::SearchType::AllHandles) else {
        logln("net: no SNP handles and no handle buffer to sweep".into());
        return;
    };
    logln(format!(
        "net: no SNP handle bound - sweeping {} controllers (connect -r)",
        all.len()
    ));
    let mut connected = 0usize;
    for h in all.iter() {
        if uefi::boot::connect_controller(*h, &[], None, true).is_ok() {
            connected += 1;
        }
    }
    let now = uefi::boot::find_handles::<SimpleNetwork>().map(|h| h.len()).unwrap_or(0);
    logln(format!(
        "net: connect -r bound {connected} controller(s); SNP handles now {now}"
    ));
}

/// Count handles exposing a protocol GUID (0 when none / NOT_FOUND).
fn count_proto(guid: uefi::Guid) -> usize {
    uefi::boot::locate_handle_buffer(uefi::boot::SearchType::ByProtocol(&guid))
        .map(|b| b.len())
        .unwrap_or(0)
}

/// Per-layer UEFI network stack handle counts, to pinpoint where a missing
/// IPv4 stack breaks: SNP up but IP4-Config2 absent ⇒ Ip4Dxe didn't bind
/// (IPv4 PXE/HTTP support disabled, or the firmware is IPv6-only).
fn net_stack_summary() -> String {
    const MNP_SB: uefi::Guid = uefi::guid!("f36ff770-a7e1-42cf-9ed2-56f0f271f44c");
    const IP4_SB: uefi::Guid = uefi::guid!("c51711e7-b4bf-404a-bfb8-0a048ef1ffe4");
    const IP4_CFG2: uefi::Guid = uefi::guid!("5b446ed1-e30b-4faa-871a-3654eca36080");
    const IP6_CFG: uefi::Guid = uefi::guid!("937fe521-95ae-4d1a-8929-48bcd90ad31a");
    let snp = uefi::boot::find_handles::<SimpleNetwork>().map(|h| h.len()).unwrap_or(0);
    format!(
        "SNP={} MNP-SB={} IP4-SB={} IP4-Config2={} IP6-Config={}",
        snp,
        count_proto(MNP_SB),
        count_proto(IP4_SB),
        count_proto(IP4_CFG2),
        count_proto(IP6_CFG),
    )
}

/// A DHCP/static IPv4 lease on one interface.
struct IfaceIp {
    ip: String,
    mask: String,
}

fn ip_str(o: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

/// Run DHCP (via IP4 Config2 `ifup`) on every IPv4-capable interface and return
/// the resulting addresses. NOTE: `ifup` blocks up to 30s per interface that
/// fails to get a lease (e.g. an unassociated Wi-Fi NIC), so this is an
/// explicit user action, not part of the passive scan.
fn run_dhcp() -> (Vec<IfaceIp>, Option<netraw::RawNet>, String) {
    let mut out = Vec::new();
    // Bind the MNP→IP4 stack onto the NICs only (skips unrelated controllers that
    // could hang). This is what produces Ip4Config2 / the IP4 service binding.
    connect_network_stack();

    // Primary: Ip4Config2 ifup on each interface.
    if let Ok(handles) = uefi::boot::find_handles::<Ip4Config2>() {
        logln(format!("dhcp: Ip4Config2 handles={}", handles.len()));
        for (i, h) in handles.into_iter().enumerate() {
            if let Ok(mut cfg) = Ip4Config2::new(h) {
                logln(format!("dhcp: if{i} ifup..."));
                if let Err(e) = cfg.ifup() {
                    logln(format!("dhcp: if{i} ifup ERR {e:?}"));
                }
                if let Ok(info) = cfg.get_interface_info() {
                    let ip = ip_str(info.station_addr.0);
                    logln(format!("dhcp: if{i} addr={ip}"));
                    out.push(IfaceIp { ip, mask: ip_str(info.subnet_mask.0) });
                }
            }
        }
        if out.iter().any(|i| i.ip != "0.0.0.0") {
            return (out, None, "DHCP: lease acquired".into());
        }
    }

    // Fallback: PXE Base Code DHCP — exposed when IPv4 PXE is enabled, and works
    // even when Ip4Config2 is absent (e.g. the firmware only brings the IP4 stack
    // up under PXE).
    {
        use uefi::proto::network::pxe::BaseCode;
        if let Ok(handles) = uefi::boot::find_handles::<BaseCode>() {
            for h in handles {
                let Ok(mut bc) = (unsafe {
                    uefi::boot::open_protocol::<BaseCode>(
                        OpenProtocolParams {
                            handle: h,
                            agent: uefi::boot::image_handle(),
                            controller: None,
                        },
                        OpenProtocolAttributes::GetProtocol,
                    )
                }) else {
                    continue;
                };
                let _ = bc.start(false);
                if bc.dhcp(false).is_ok() {
                    if let core::net::IpAddr::V4(v4) = bc.mode().station_ip() {
                        if !v4.is_unspecified() {
                            out.push(IfaceIp { ip: ip_str(v4.octets()), mask: String::new() });
                        }
                    }
                }
            }
        }
        if out.iter().any(|i| i.ip != "0.0.0.0") {
            logln(format!("dhcp: PXE base code lease {}", out[0].ip));
            return (out, None, "DHCP: lease via PXE base code".into());
        }
    }

    // Final fallback: raw DHCP over SimpleNetwork (no firmware IPv4 stack needed).
    match netraw::dhcp() {
        Ok(rn) => {
            out.push(IfaceIp { ip: netraw::ip_str(rn.ip), mask: netraw::ip_str(rn.mask) });
            let status = format!(
                "DHCP: lease via raw SNP {} (gw {}) - press 'p' to UDP-upload",
                netraw::ip_str(rn.ip),
                netraw::ip_str(rn.gateway)
            );
            (out, Some(rn), status)
        }
        Err(e) => (
            out,
            None,
            format!("no IPv4 lease — {} (Ip4Config2 + PXE + raw SNP: {e})", net_stack_summary()),
        ),
    }
}

/// Quote a string as a JSON literal. Firmware strings carry NULs and other
/// control bytes, which a strict parser rejects unless escaped.
fn jq(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// Build the hardware fingerprint as a JSON document for upload.
fn fingerprint_json(info: &SysInfo) -> String {
    let d = &info.dmi;
    let mut dimms = String::new();
    for (i, m) in d.dimms.iter().enumerate() {
        if i > 0 {
            dimms.push(',');
        }
        dimms.push_str(&format!(
            "{{\"slot\":{},\"bank\":{},\"size\":{},\"speed\":{},\"cfg_speed\":{},\"type\":{},\"mfr\":{},\"part\":{},\"serial\":{}}}",
            jq(&m.locator),
            jq(&m.bank),
            jq(&m.size),
            m.speed,
            m.cfg_speed,
            jq(&m.mtype),
            jq(&m.mfr),
            jq(&m.part),
            jq(&m.serial)
        ));
    }
    let mut macs = String::new();
    for (i, n) in info.nics.iter().enumerate() {
        if i > 0 {
            macs.push(',');
        }
        macs.push_str(&jq(&n.mac));
    }
    let mut disks = String::new();
    // Each SMART record pairs with at most one block device, matched by capacity.
    let mut sata_used = vec![false; info.sata.len()];
    for (i, dk) in info.disks.iter().enumerate() {
        if i > 0 {
            disks.push(',');
        }
        let paired = info.sata.iter().enumerate().find_map(|(j, s)| {
            (s.capacity != 0 && s.capacity == dk.capacity && sata_used.get(j) == Some(&false))
                .then_some(j)
        });
        let smart = match paired.and_then(|j| {
            if let Some(f) = sata_used.get_mut(j) {
                *f = true;
            }
            info.sata.get(j)
        }) {
            Some(s) => format!(",\"sata_smart\":{}", smart::drive_json(s)),
            None => String::new(),
        };
        disks.push_str(&format!(
            "{{\"capacity_bytes\":{},\"removable\":{},\"bus\":{},\"drive_type\":{}{}}}",
            dk.capacity,
            dk.removable,
            jq(dk.bus),
            jq(drive_type(dk.bus)),
            smart
        ));
    }
    let mut gpus = String::new();
    for (i, g) in info.gpus.iter().enumerate() {
        if i > 0 {
            gpus.push(',');
        }
        gpus.push_str(&format!(
            "{{\"vendor\":{},\"vendor_id\":{},\"device_id\":{},\"integrated\":{}}}",
            jq(pci_vendor_name(g.vendor)),
            g.vendor,
            g.device,
            g.integrated
        ));
    }
    let mut nvme = String::new();
    for (i, d) in info.nvme.iter().enumerate() {
        if i > 0 {
            nvme.push(',');
        }
        let smart = match &d.smart {
            Some(s) => format!(
                ",\"smart\":{{\"critical_warning\":{},\"temp_c\":{},\"percentage_used\":{},\"available_spare\":{},\"power_on_hours\":{},\"data_units_written\":{},\"media_errors\":{}}}",
                s.critical_warning,
                s.temp_c,
                s.percentage_used,
                s.available_spare,
                s.power_on_hours,
                s.data_units_written,
                s.media_errors
            ),
            None => String::new(),
        };
        nvme.push_str(&format!(
            "{{\"model\":{},\"serial\":{},\"firmware\":{}{}}}",
            jq(&d.model),
            jq(&d.serial),
            jq(&d.firmware),
            smart
        ));
    }
    let feats = info
        .cpu_feats
        .iter()
        .map(|f| jq(f))
        .collect::<Vec<_>>()
        .join(",");
    let tpm = match &info.tpm {
        Some(t) => jq(t),
        None => "null".to_string(),
    };
    let sb_capable = info.secure_boot.is_some();
    let sb_enabled = info.secure_boot.unwrap_or(false);
    let win11 = win11_readiness(info).ready;

    let mut out = format!(
        concat!(
            "{{\"agent\":\"uefi-app\",",
            "\"system\":{{\"manufacturer\":{},\"product\":{},\"version\":{},\"serial\":{},",
            "\"uuid\":{},\"sku\":{},\"family\":{}}},",
            "\"baseboard\":{{\"manufacturer\":{},\"product\":{},\"version\":{},\"serial\":{},\"asset_tag\":{}}},",
            "\"cpu\":{{\"model\":{},\"socket\":{},\"cores\":{},\"threads\":{},\"features\":[{}]}},",
            "\"gpu\":[{}],",
            "\"memory\":{{\"total_bytes\":{},\"usable_bytes\":{},\"max_bytes\":{},\"slots\":{},\"ecc\":{},\"dimms\":[{}]}},",
            "\"storage\":[{}],\"nvme\":[{}],\"sata\":{},",
            "\"firmware\":{{\"uefi_vendor\":{},\"uefi_spec\":{},\"bios_vendor\":{},\"bios_version\":{},\"bios_date\":{},\"chassis\":{},\"rtc\":{}}},",
            "\"security\":{{\"secure_boot_capable\":{},\"secure_boot_enabled\":{},\"tpm\":{},\"msdm_present\":{},\"msdm_key\":{}}},",
            "\"win11_ready\":{},",
            "\"macs\":[{}]}}"
        ),
        jq(&d.sys_mfr),
        jq(&d.sys_product),
        jq(&d.sys_version),
        jq(&effective_serial(info)),
        jq(&d.sys_uuid),
        jq(&d.sys_sku),
        jq(&d.sys_family),
        jq(&d.board_mfr),
        jq(&d.board_product),
        jq(&d.board_version),
        jq(&d.board_serial),
        jq(&d.board_asset),
        jq(&d.cpu_version),
        jq(&d.cpu_socket),
        d.cpu_cores,
        d.cpu_threads,
        feats,
        gpus,
        info.mem_total,
        info.mem_usable,
        d.mem_max_bytes,
        d.mem_slots,
        jq(&d.mem_ecc),
        dimms,
        disks,
        nvme,
        smart::sata_json_string(&info.sata),
        jq(&info.fw_vendor),
        jq(&info.uefi_revision),
        jq(&d.bios_vendor),
        jq(&d.bios_version),
        jq(&d.bios_date),
        jq(&d.chassis_type),
        jq(&info.rtc),
        sb_capable,
        sb_enabled,
        tpm,
        info.msdm,
        jq(&info.msdm_key),
        win11,
        macs
    );
    out.truncate(out.len() - 1);
    out.push_str(&format!(
        ",\"firmware_vars\":{{\"boot_entries\":{},\"pk_enrolled\":{},\"kek_enrolled\":{},\"setup_mode\":{},\"microcode\":{},\"pci_slots_total\":{},\"pci_slots_used\":{}}}",
        info.boot_entries,
        info.pk_enrolled,
        info.kek_enrolled,
        match info.setup_mode {
            Some(b) => b.to_string(),
            None => "null".to_string(),
        },
        match info.microcode {
            Some(m) => format!("\"0x{m:08X}\""),
            None => "null".to_string(),
        },
        info.pci_slots_total,
        info.pci_slots_used,
    ));
    out.push_str(",\"pcie\":[");
    for (i, l) in info.pcie.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"loc\":{},\"class\":{},\"max_width\":{},\"max_gen\":{},\"cur_width\":{},\"cur_gen\":{},\"dev_err\":{},\"aer_corr\":\"0x{:08x}\",\"aer_uncorr\":\"0x{:08x}\"}}",
            jq(&l.loc),
            jq(l.class_label),
            l.max_width,
            l.max_speed,
            l.cur_width,
            l.cur_speed,
            l.dev_err,
            l.aer_corr,
            l.aer_uncorr
        ));
    }
    out.push(']');

    let mca = info
        .mca
        .iter()
        .enumerate()
        .map(|(i, m)| {
            format!(
                "{}{{\"bank\":{},\"status\":\"0x{:016x}\",\"addr\":\"0x{:016x}\"}}",
                if i > 0 { "," } else { "" },
                m.bank,
                m.status,
                m.addr
            )
        })
        .collect::<String>();
    let mem_errors = info
        .mem_errors
        .iter()
        .enumerate()
        .map(|(i, e)| {
            format!(
                "{}{{\"kind\":{},\"addr\":\"0x{:x}\"}}",
                if i > 0 { "," } else { "" },
                jq(e.kind),
                e.addr
            )
        })
        .collect::<String>();
    let spd = info
        .spd
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let pmic = match d.pmic_addr {
                Some(pa) => format!(
                    ",\"pmic\":{{\"addr\":\"0x{pa:02x}\",\"r04\":\"0x{:02x}\",\"r08\":\"0x{:02x}\",\"fault\":{}}}",
                    d.pmic_r04, d.pmic_r08, d.pmic_fault
                ),
                None => String::new(),
            };
            let temp = match d.temp_c {
                Some(t) => format!("{t:.1}"),
                None => "null".to_string(),
            };
            format!(
                "{}{{\"addr\":\"0x{:02x}\",\"type\":{},\"temp_c\":{}{}}}",
                if i > 0 { "," } else { "" },
                d.addr,
                jq(d.type_name),
                temp,
                pmic
            )
        })
        .collect::<String>();
    let mch_window = info
        .mchbar
        .window
        .iter()
        .map(|w| format!("\"0x{w:08x}\""))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str(&format!(
        ",\"diagnostics\":{{\"rtc_suspect\":{},\"bert\":{{\"present\":{},\"error\":{},\"severity\":{},\"entries\":{}}},\"fpdt\":{{\"present\":{},\"fw_boot_ms\":{},\"os_loader_ms\":{}}},\"cmos\":{{\"present\":{},\"rtc_power_lost\":{},\"checksum_bad\":{},\"diag\":\"0x{:02x}\",\"dump\":{}}},\"spd\":[{}],\"mchbar\":{{\"vendor\":{},\"present\":{},\"enabled\":{},\"base\":\"0x{:x}\",\"window\":[{}]}},\"mca\":[{}],\"mem_errors\":[{}]}}",
        info.rtc_suspect,
        info.bert.present,
        info.bert.error_present,
        jq(&info.bert.severity),
        info.bert.entry_count,
        info.fpdt.present,
        info.fpdt.fw_boot_ms,
        info.fpdt.os_loader_ms,
        info.cmos.present,
        info.cmos.rtc_power_lost,
        info.cmos.checksum_bad,
        info.cmos.diag,
        jq(&info.cmos.dump),
        spd,
        jq(info.mchbar.vendor),
        info.mchbar.present,
        info.mchbar.enabled,
        info.mchbar.base,
        mch_window,
        mca,
        mem_errors,
    ));
    out.push_str(",\"bios_settings\":");
    out.push_str(&hii::audit_json(&info.hii).to_string());
    out.push_str(",\"firmware_update\":");
    out.push_str(&capsule::esrt_json(&info.esrt, &info.power).to_string());
    out.push_str(",\"boot_diagnostics\":");
    out.push_str(&bootdiag::diag_json(&info.bootdiag).to_string());
    out.push_str(",\"volume_signatures\":");
    out.push_str(&volsig::volumes_json(&info.volumes).to_string());
    // Raw serial sources behind the effective `system.serial`, for fidelity.
    out.push_str(",\"identity\":");
    out.push_str(
        &serde_json::json!({
            "effective_serial": effective_serial(info),
            "msdm_key": info.msdm_key,
            "sys_serial": d.sys_serial,
            "board_serial": d.board_serial,
            "chassis_serial": d.chassis_serial,
            "chassis_asset": d.chassis_asset,
            "oem_strings": d.oem_strings,
        })
        .to_string(),
    );
    out.push('}');
    out
}

/// Fingerprint JSON with the stress summary spliced in when a run exists.
fn fingerprint_with_stress(info: &SysInfo, stress: Option<serde_json::Value>) -> String {
    let mut j = fingerprint_json(info);
    if let Some(s) = stress {
        j.truncate(j.len() - 1);
        j.push_str(",\"stress\":");
        j.push_str(&s.to_string());
        j.push('}');
    }
    j
}

/// Parsed upload target.
struct UploadUrl {
    /// Full normalized URL incl scheme/host/port/path (for EFI HTTP).
    full: String,
    scheme: &'static str,
    /// host:port (for the raw-TCP4 path).
    host_port: String,
    path: String,
    /// True when the firmware HTTP stack (DNS + TLS) is required: https, or an
    /// http host that's a name rather than a literal IPv4 (needs DNS).
    needs_efi_http: bool,
    /// True for `tcp://` — push a framed fingerprint to the axum_server QC
    /// listener (the "connected client" path; plain TCP, no TLS).
    is_qc_tcp: bool,
}

/// Parse a typed target into an [`UploadUrl`]. Accepts `https://host[:port][/path]`,
/// `http://host[:port][/path]`, or a bare `host[:port]` (assumed http). Default
/// port: 443 for https, 8082 (the axum_server port) for http. Default path:
/// `/api/v1/qc/fingerprint`.
fn parse_upload_url(target: &str) -> UploadUrl {
    let (scheme, rest) = if let Some(r) = target.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = target.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = target.strip_prefix("tcp://") {
        ("tcp", r)
    } else {
        ("http", target)
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let default_port = match scheme {
        "https" => 443,
        "tcp" => 9201, // axum_server QC listener
        _ => 8082,
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(default_port)),
        None => (hostport.to_string(), default_port),
    };
    let path = if path.is_empty() {
        "/api/v1/qc/fingerprint".to_string()
    } else {
        path.to_string()
    };
    let is_ipv4 = host.parse::<core::net::Ipv4Addr>().is_ok();
    UploadUrl {
        full: format!("{scheme}://{host}:{port}{path}"),
        scheme,
        host_port: format!("{host}:{port}"),
        path,
        needs_efi_http: scheme == "https" || (scheme == "http" && !is_ipv4),
        is_qc_tcp: scheme == "tcp",
    }
}

/// Consecutive relay failures that open the breaker.
const RELAY_TRIP_AFTER: u32 = 3;

/// First suppression window; doubles per re-trip up to [`RELAY_MUTE_MAX_MS`].
const RELAY_MUTE_BASE_MS: u64 = 5_000;

/// Ceiling on the suppression window.
const RELAY_MUTE_MAX_MS: u64 = 60_000;

/// Breaker that suppresses relay HTTP while the target keeps failing. A failed
/// connect costs the run loop a full [`TCP_CONNECT_TIMEOUT_MS`] pump, so an
/// unreachable relay is muted rather than retried on every poll.
mod relay_gate {
    use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

    static NOW_MS: AtomicU64 = AtomicU64::new(0);
    static MUTE_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
    static FAILS: AtomicU32 = AtomicU32::new(0);
    static TRIPS: AtomicU32 = AtomicU32::new(0);

    /// Publish the run loop's clock; the breaker times its backoff from it.
    pub fn tick(now_ms: u64) {
        NOW_MS.store(now_ms, Relaxed);
    }

    /// True while relay requests are suppressed.
    pub fn muted() -> bool {
        NOW_MS.load(Relaxed) < MUTE_UNTIL_MS.load(Relaxed)
    }

    /// Milliseconds until the breaker closes.
    pub fn remaining_ms() -> u64 {
        MUTE_UNTIL_MS.load(Relaxed).saturating_sub(NOW_MS.load(Relaxed))
    }

    /// Clear the breaker on a target change or an operator retry.
    pub fn reset() {
        FAILS.store(0, Relaxed);
        TRIPS.store(0, Relaxed);
        MUTE_UNTIL_MS.store(0, Relaxed);
    }

    /// Record a request outcome; a failure streak opens the breaker. A connect
    /// timeout trips it alone, having already spent a full connect budget.
    pub fn note(ok: bool, err: Option<&str>) {
        if ok {
            reset();
            return;
        }
        let timed_out = err.is_some_and(|e| e.contains("TIMEOUT"));
        let streak = FAILS.fetch_add(1, Relaxed) + 1;
        if !timed_out && streak < super::RELAY_TRIP_AFTER {
            return;
        }
        FAILS.store(0, Relaxed);
        let shift = TRIPS.fetch_add(1, Relaxed).min(3);
        let window = (super::RELAY_MUTE_BASE_MS << shift).min(super::RELAY_MUTE_MAX_MS);
        MUTE_UNTIL_MS.store(NOW_MS.load(Relaxed).saturating_add(window), Relaxed);
        super::logln(format!(
            "relay: unreachable - muting relay HTTP for {}s",
            window / 1000
        ));
    }
}

/// GET `path` from the upload target's host, picking the transport like the
/// fingerprint upload does. `tcp://` (QC frame) targets are redirected to the
/// axum HTTP port on the same host. Returns (status code, body).
fn http_get_json(target: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
    if relay_gate::muted() {
        return Err(format!("relay muted {}ms", relay_gate::remaining_ms()));
    }
    let u = parse_upload_url(target);
    let res = if u.is_qc_tcp {
        let host = u
            .host_port
            .rsplit_once(':')
            .map(|(h, _)| h.to_string())
            .unwrap_or_else(|| u.host_port.clone());
        net_tcp::get(&format!("{host}:8082"), path)
    } else if u.needs_efi_http {
        http_efi::get(&format!("{}://{}{}", u.scheme, u.host_port, path))
    } else {
        net_tcp::get(&u.host_port, path)
    };
    // A served status code still proves the relay is reachable.
    relay_gate::note(res.is_ok(), res.as_ref().err().map(|e| e.as_str()));
    res
}

/// POST `body` to `path` on the upload target's host, picking the transport like
/// [`http_get_json`]. `tcp://` targets redirect to the axum HTTP port.
fn http_post_json(target: &str, path: &str, body: &[u8]) -> Result<String, String> {
    if relay_gate::muted() {
        return Err(format!("relay muted {}ms", relay_gate::remaining_ms()));
    }
    let u = parse_upload_url(target);
    let res = if u.is_qc_tcp {
        let host = u
            .host_port
            .rsplit_once(':')
            .map(|(h, _)| h.to_string())
            .unwrap_or_else(|| u.host_port.clone());
        net_tcp::post(&format!("{host}:8082"), path, body)
    } else if u.needs_efi_http {
        http_efi::post(&format!("{}://{}{}", u.scheme, u.host_port, path), body)
    } else {
        net_tcp::post(&u.host_port, path, body)
    };
    relay_gate::note(res.is_ok(), res.as_ref().err().map(|e| e.as_str()));
    res
}

/// GET an absolute or target-relative URL, picking the transport like
/// [`http_get_json`]. Paths resolve against `target`; absolute `http://<IPv4>`
/// URLs take the raw-TCP4 path, anything needing DNS/TLS falls to EFI HTTP.
fn http_get_url(target: &str, url: &str) -> Result<(u16, Vec<u8>), String> {
    if url.starts_with('/') {
        return http_get_json(target, url);
    }
    let u = parse_upload_url(url);
    if u.needs_efi_http {
        http_efi::get_capped(&u.full, 8 << 20)
    } else {
        net_tcp::get(&u.host_port, &u.path)
    }
}

/// Body ceiling for a firmware capsule.
const CAPSULE_MAX_BYTES: usize = 64 << 20;

/// Download a firmware capsule, picking the transport like [`http_get_url`].
/// Target-relative paths and absolute `http://<IPv4>` URLs go over raw TCP4;
/// only genuine DNS/TLS URLs need the EFI HTTP driver, which most of the fleet
/// does not have.
fn download_capsule(target: &str, url: &str) -> Result<Vec<u8>, String> {
    let (code, body) = if url.starts_with('/') {
        let u = parse_upload_url(target);
        if u.needs_efi_http {
            http_efi::get_capped(&format!("{}{}", u.full.trim_end_matches('/'), url), CAPSULE_MAX_BYTES)?
        } else {
            net_tcp::get_capped(&u.host_port, url, CAPSULE_MAX_BYTES)?
        }
    } else {
        let u = parse_upload_url(url);
        if u.needs_efi_http {
            http_efi::get_capped(&u.full, CAPSULE_MAX_BYTES)?
        } else {
            net_tcp::get_capped(&u.host_port, &u.path, CAPSULE_MAX_BYTES)?
        }
    };
    if code != 200 {
        return Err(format!("HTTP {code} fetching capsule"));
    }
    if body.is_empty() {
        return Err("empty capsule body".into());
    }
    Ok(body)
}

/// Read a capsule off an attached FAT volume (the boot USB). Needs no network,
/// so this is the delivery path that always works. `path` is backslash-separated
/// and relative to a volume root, e.g. `bios\\update.cap`.
fn load_capsule_from_disk(path: &str) -> Result<Vec<u8>, String> {
    use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};

    let cpath = uefi::CString16::try_from(path.replace('/', "\\").as_str())
        .map_err(|_| format!("path is not valid UCS-2: {path}"))?;
    let handles = boot::find_handles::<SimpleFileSystem>()
        .map_err(|e| format!("no filesystems ({e:?})"))?;

    let mut last = format!("{path} not found on any volume");
    for h in handles {
        let mut sfs = match unsafe {
            boot::open_protocol::<SimpleFileSystem>(
                OpenProtocolParams {
                    handle: h,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            Ok(s) => s,
            Err(_) => continue,
        };
        let Ok(mut root) = sfs.open_volume() else {
            continue;
        };
        let Ok(handle) = root.open(&cpath, FileMode::Read, FileAttribute::empty()) else {
            continue;
        };
        let Ok(FileType::Regular(mut f)) = handle.into_type() else {
            last = format!("{path} is a directory");
            continue;
        };
        let size = match f.get_boxed_info::<uefi::proto::media::file::FileInfo>() {
            Ok(i) => i.file_size() as usize,
            Err(e) => {
                last = format!("stat {path}: {e:?}");
                continue;
            }
        };
        if size == 0 || size > CAPSULE_MAX_BYTES {
            last = format!("{path} is {size} bytes (max {CAPSULE_MAX_BYTES})");
            continue;
        }
        let mut buf = vec![0u8; size];
        match f.read(&mut buf) {
            Ok(n) if n == size => {
                logln(format!("capsule: read {n} bytes from {path}"));
                return Ok(buf);
            }
            Ok(n) => last = format!("short read: {n} of {size} bytes"),
            Err(e) => last = format!("read {path}: {e:?}"),
        }
    }
    Err(last)
}

/// Load the capsule named by `app.capsule_src` (a volume path, or a URL when it
/// has a scheme), describe it, and run the pre-flight gates without flashing.
fn load_and_inspect_capsule(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    let src = app.capsule_src.trim().to_string();
    if src.is_empty() {
        app.status = "set a capsule path or URL first (press 'e')".into();
        return Ok(());
    }
    app.capsule_bytes.clear();
    app.capsule_note.clear();
    app.capsule_blocked = true;
    app.fw_armed = false;
    app.status = format!("loading capsule from {src} ...");
    terminal.draw(|frame| render(frame, app))?;

    let loaded = if src.contains("://") {
        let target = app.target.clone();
        download_capsule(&target, &src)
    } else {
        load_capsule_from_disk(&src)
    };
    let bytes = match loaded {
        Ok(b) => b,
        Err(e) => {
            app.capsule_note.push(format!("load failed: {e}"));
            app.status = format!("capsule load failed: {e}");
            return Ok(());
        }
    };

    match capsule::inspect(&bytes) {
        Ok(lines) => app.capsule_note.extend(lines),
        Err(e) => {
            app.capsule_note.push(format!("malformed: {e}"));
            app.status = format!("capsule malformed: {e}");
            return Ok(());
        }
    }

    // A local flash is authorized by physical presence plus the two-press
    // confirm, so no expected version is demanded; power is still enforced.
    match capsule::preflight(&app.info.esrt, &app.info.power, None, true) {
        Ok(warns) => {
            for w in warns {
                app.capsule_note.push(format!("ok      {w}"));
            }
            app.capsule_blocked = false;
            app.capsule_bytes = bytes;
            app.status = "capsule loaded - press F twice to flash".into();
        }
        Err(e) => {
            app.capsule_note.push(format!("BLOCKED {e}"));
            app.status = format!("pre-flight refused: {e}");
        }
    }
    Ok(())
}

/// Flash the loaded capsule. Returns only on failure; success resets the box.
fn do_bios_flash(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    if app.stress.is_active() {
        app.status = "stop the stress test before flashing ('s')".into();
        return Ok(());
    }
    if let Err(e) = capsule::preflight(&app.info.esrt, &app.info.power, None, true) {
        app.status = format!("flash refused: {e}");
        return Ok(());
    }
    app.status = format!(
        "FLASHING {} bytes - DO NOT POWER OFF",
        app.capsule_bytes.len()
    );
    terminal.draw(|frame| render(frame, app))?;

    let bytes = core::mem::take(&mut app.capsule_bytes);
    match capsule::apply_capsule(&bytes, &app.info.esrt) {
        Ok(msg) => {
            app.status = format!("capsule applied: {msg} - {RESCAN_NOTICE}");
            terminal.draw(|frame| render(frame, app))?;
            app.set_info(SysInfo::collect());
            app.status = format!("capsule applied: {msg}");
        }
        Err(e) => {
            app.status = format!("BIOS update failed: {e}");
            app.capsule_note.push(format!("FAILED  {e}"));
            app.capsule_bytes = bytes;
        }
    }
    Ok(())
}

/// HTTP(S) POST via the EFI HTTP protocol — used for `https://` (TLS) and for
/// hostnames (DNS), both of which the raw-TCP4 path can't do. Plain http:// is
/// blocked by firmware policy, but https:// is allowed.
mod http_efi {
    use crate::logln;
    use uefi::boot;
    use uefi::proto::network::http::{HttpBinding, HttpHelper};
    use uefi_raw::protocol::network::http::HttpMethod;

    pub fn post(url: &str, body: &[u8]) -> Result<String, String> {
        logln(format!("http(efi): POST {url} ({}B)", body.len()));
        let handles = boot::find_handles::<HttpBinding>().map_err(|e| {
            logln(format!("http(efi): no HTTP service ({e:?})"));
            format!("no HTTPS/DNS in firmware; set target to http://<LAN-IP>:8082 ('e') [{e:?}]")
        })?;
        let h = *handles.first().ok_or("no HTTP-capable interface")?;
        let mut http = HttpHelper::new(h).map_err(|e| {
            logln(format!("http(efi): new ERR {e:?}"));
            format!("http open: {e:?}")
        })?;
        http.configure().map_err(|e| {
            logln(format!("http(efi): configure ERR {e:?}"));
            format!("configure: {e:?}")
        })?;
        let mut b = body.to_vec();
        http.request(HttpMethod::POST, url, Some(&mut b)).map_err(|e| {
            logln(format!("http(efi): request ERR {e:?} (TLS/DNS/cert?)"));
            format!("request: {e:?}")
        })?;
        let resp = http.response_first(true).map_err(|e| {
            logln(format!("http(efi): response ERR {e:?}"));
            format!("response: {e:?}")
        })?;
        logln(format!(
            "http(efi): {:?} ({}B)",
            resp.status,
            resp.body.len()
        ));
        Ok(format!("HTTP {:?} ({}B)", resp.status, resp.body.len()))
    }

    /// GET with a 4 MiB body cap (order/command payloads).
    pub fn get(url: &str) -> Result<(u16, Vec<u8>), String> {
        get_capped(url, 4 << 20)
    }

    /// GET returning (status code, full body). Reads until Content-Length is
    /// satisfied; without one, drains until the firmware reports the
    /// connection finished. `max_cap` bounds the body (capsules need a larger
    /// ceiling than the default order payloads).
    pub fn get_capped(url: &str, max_cap: usize) -> Result<(u16, Vec<u8>), String> {
        logln(format!("http(efi): GET {url}"));
        let handles = boot::find_handles::<HttpBinding>()
            .map_err(|e| format!("no HTTPS/DNS in firmware; set target to http://<LAN-IP>:8082 ('e') [{e:?}]"))?;
        let h = *handles.first().ok_or("no HTTP-capable interface")?;
        let mut http = HttpHelper::new(h).map_err(|e| format!("http open: {e:?}"))?;
        http.configure().map_err(|e| format!("configure: {e:?}"))?;
        http.request(HttpMethod::GET, url, None)
            .map_err(|e| format!("request: {e:?} (TLS/DNS/cert?)"))?;
        let first = http
            .response_first(true)
            .map_err(|e| format!("response: {e:?}"))?;
        let code = http_code(first.status);
        let content_len = first
            .headers
            .iter()
            .find(|(k, _)| k == "content-length")
            .and_then(|(_, v)| v.trim().parse::<usize>().ok());
        let mut body = first.body;
        let cap = content_len.unwrap_or(max_cap).min(max_cap);
        while body.len() < cap {
            match http.response_more(&mut body) {
                Ok(chunk) if chunk.is_empty() => break,
                Ok(_) => {}
                // Connection finished (or anything else): stop draining.
                Err(_) => break,
            }
        }
        logln(format!("http(efi): GET done {code} ({}B)", body.len()));
        Ok((code, body))
    }

    /// EFI status-code enum → numeric HTTP code (subset the app reacts to).
    fn http_code(s: uefi_raw::protocol::network::http::HttpStatusCode) -> u16 {
        use uefi_raw::protocol::network::http::HttpStatusCode as C;
        match s {
            C::STATUS_200_OK => 200,
            C::STATUS_201_CREATED => 201,
            C::STATUS_204_NO_CONTENT => 204,
            C::STATUS_400_BAD_REQUEST => 400,
            C::STATUS_401_UNAUTHORIZED => 401,
            C::STATUS_403_FORBIDDEN => 403,
            C::STATUS_404_NOT_FOUND => 404,
            C::STATUS_500_INTERNAL_SERVER_ERROR => 500,
            C::STATUS_502_BAD_GATEWAY => 502,
            C::STATUS_503_SERVICE_UNAVAILABLE => 503,
            other => 600 + other.0 as u16,
        }
    }
}

/// HTTP POST over raw EFI TCP4 (bypasses the EFI HTTP protocol, which firmware
/// blocks for plain http:// when PcdAllowHttpConnections=FALSE). Builds the
/// request bytes by hand and drives the TCP4 service binding directly.
mod net_tcp {
    use crate::logln;
    use core::net::Ipv4Addr;
    use core::time::Duration;
    use uefi::Handle;
    use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
    use uefi::proto::unsafe_protocol;
    use uefi_raw::protocol::driver::ServiceBindingProtocol;
    use uefi_raw::protocol::network::tcp4::{
        Tcp4AccessPoint, Tcp4CompletionToken, Tcp4ConfigData, Tcp4ConnectionToken,
        Tcp4FragmentData, Tcp4IoToken, Tcp4Packet, Tcp4Protocol,
    };
    use uefi_raw::table::boot::{EventType, Tpl};
    use uefi_raw::{Boolean, Ipv4Address, Status};

    #[unsafe_protocol(Tcp4Protocol::SERVICE_BINDING_GUID)]
    struct Tcp4Sb(ServiceBindingProtocol);

    #[unsafe_protocol(Tcp4Protocol::GUID)]
    struct Tcp4(Tcp4Protocol);

    // TCP4 protocol statuses absent from uefi-raw's core Status list.
    const ERROR_BIT: usize = 1 << (usize::BITS - 1);
    const CONNECTION_FIN: Status = Status(ERROR_BIT | 104);
    const CONNECTION_RESET: Status = Status(ERROR_BIT | 105);

    // The EFI transmit/receive data structs end in a flexible array of
    // fragments; these mirror them with exactly one fragment.
    #[repr(C)]
    struct TxData1 {
        push: Boolean,
        urgent: Boolean,
        data_length: u32,
        fragment_count: u32,
        fragment_table: [Tcp4FragmentData; 1],
    }
    #[repr(C)]
    struct RxData1 {
        urgent: Boolean,
        data_length: u32,
        fragment_count: u32,
        fragment_table: [Tcp4FragmentData; 1],
    }

    fn parse_target(t: &str) -> Option<(Ipv4Address, u16)> {
        let (host, port) = match t.split_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok()?),
            None => (t, 8080u16),
        };
        let ip: Ipv4Addr = host.parse().ok()?;
        Some((Ipv4Address(ip.octets()), port))
    }

    fn build_request(path: &str, host: &str, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(format!("POST {path} HTTP/1.1\r\n").as_bytes());
        v.extend_from_slice(format!("Host: {host}\r\n").as_bytes());
        v.extend_from_slice(b"Content-Type: application/json\r\n");
        v.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
        v.extend_from_slice(b"Connection: close\r\n\r\n");
        v.extend_from_slice(body);
        v
    }

    /// Drive the network stack until a completion token's status leaves
    /// NOT_READY, or we hit the millisecond budget.
    unsafe fn pump(tcp: *mut Tcp4Protocol, status: *const Status, budget_ms: u32) -> Status {
        let mut waited = 0;
        loop {
            let s = unsafe { core::ptr::read_volatile(status) };
            if s != Status::NOT_READY {
                return s;
            }
            let _ = unsafe { ((*tcp).poll)(tcp) };
            boot::stall(Duration::from_millis(1));
            waited += 1;
            if waited >= budget_ms {
                return Status::TIMEOUT;
            }
        }
    }

    /// `target` is "host:port" (already normalized by the caller).
    /// Build a `tcp_protocol` text frame: `[u32 LE total_len][0x02][body]`.
    fn build_qc_frame(body: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(5 + body.len());
        v.extend_from_slice(&((1 + body.len()) as u32).to_le_bytes());
        v.push(0x02); // FRAME_TAG_TEXT
        v.extend_from_slice(body);
        v
    }

    /// True when the firmware exposes no TCP4 service binding (Ip4Dxe/Tcp4 absent).
    fn tcp4_absent() -> bool {
        boot::find_handles::<Tcp4Sb>().map(|h| h.is_empty()).unwrap_or(true)
    }

    /// HTTP POST over raw TCP4, or smoltcp-over-SNP when no TCP4 stack exists.
    pub fn post(target: &str, path: &str, body: &[u8]) -> Result<String, String> {
        if tcp4_absent() {
            return crate::smolnet::post(target, path, body);
        }
        run(target, path, body, false)
    }

    /// Send the fingerprint as a single length-prefixed frame to the
    /// axum_server QC listener (Mastertech "connected client" path).
    pub fn send_qc(target: &str, body: &[u8]) -> Result<String, String> {
        run(target, "", body, true)
    }

    fn build_get(path: &str, host: &str) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(format!("GET {path} HTTP/1.1\r\n").as_bytes());
        v.extend_from_slice(format!("Host: {host}\r\n").as_bytes());
        v.extend_from_slice(b"Accept: application/json\r\n");
        v.extend_from_slice(b"Connection: close\r\n\r\n");
        v
    }

    /// HTTP GET over raw TCP4 returning (status code, body). Reads until
    /// Content-Length is satisfied or the peer closes (Connection: close).
    pub fn get(target: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
        get_capped(target, path, DEFAULT_BODY_CAP)
    }

    /// Body ceiling for ordinary JSON responses.
    pub const DEFAULT_BODY_CAP: usize = 4 << 20;

    /// GET with an explicit body ceiling. Exceeding it, or a body shorter than
    /// the advertised Content-Length, is an error — never a truncated success.
    pub fn get_capped(target: &str, path: &str, max_cap: usize) -> Result<(u16, Vec<u8>), String> {
        if tcp4_absent() {
            // smolnet has a fixed whole-request deadline and ignores
            // Content-Length, so it cannot be trusted with a large body.
            if max_cap > DEFAULT_BODY_CAP {
                return Err(
                    "firmware IPv4 stack absent; the smoltcp fallback cannot carry a body this large"
                        .into(),
                );
            }
            return crate::smolnet::get(target, path);
        }
        let (rip, rport) =
            parse_target(target).ok_or_else(|| "bad target (use a.b.c.d or a.b.c.d:port)".to_string())?;
        logln(format!("tcp: GET {target}{path}"));
        let handles = boot::find_handles::<Tcp4Sb>()
            .map_err(|e| format!("no TCP4 service ({e:?})"))?;
        let mut last = "no TCP4 interface".to_string();
        for (idx, sbh) in handles.into_iter().enumerate() {
            let mut sb = match unsafe {
                boot::open_protocol::<Tcp4Sb>(
                    OpenProtocolParams {
                        handle: sbh,
                        agent: boot::image_handle(),
                        controller: None,
                    },
                    OpenProtocolAttributes::GetProtocol,
                )
            } {
                Ok(s) => s,
                Err(e) => {
                    last = format!("open sb: {e:?}");
                    continue;
                }
            };
            let mut child: uefi_raw::Handle = core::ptr::null_mut();
            let st = unsafe { (sb.0.create_child)(&mut sb.0, &mut child) };
            if st != Status::SUCCESS {
                last = format!("create_child: {st:?}");
                continue;
            }
            let Some(child_handle) = (unsafe { Handle::from_ptr(child) }) else {
                last = "null child handle".into();
                continue;
            };
            let result = get_child(child_handle, idx, rip, rport, path, target, max_cap);
            let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
            match result {
                Ok(out) => return Ok(out),
                Err(e) => {
                    logln(format!("tcp: GET if{idx} failed: {e}"));
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn get_child(
        child: Handle,
        idx: usize,
        rip: Ipv4Address,
        rport: u16,
        path: &str,
        host: &str,
        max_cap: usize,
    ) -> Result<(u16, Vec<u8>), String> {
        let mut tcp = unsafe {
            boot::open_protocol::<Tcp4>(
                OpenProtocolParams {
                    handle: child,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        }
        .map_err(|e| format!("open tcp4: {e:?}"))?;
        let tcp_ptr: *mut Tcp4Protocol = &mut tcp.0;

        let cfg = Tcp4ConfigData {
            type_of_service: 0,
            time_to_live: 64,
            access_point: Tcp4AccessPoint {
                use_default_address: Boolean::from(true),
                station_address: Ipv4Address([0, 0, 0, 0]),
                subnet_mask: Ipv4Address([0, 0, 0, 0]),
                station_port: 0,
                remote_address: rip,
                remote_port: rport,
                active_flag: Boolean::from(true),
            },
            control_option: core::ptr::null_mut(),
        };
        let st = unsafe { ((*tcp_ptr).configure)(tcp_ptr, &cfg) };
        if st != Status::SUCCESS {
            return Err(format!("configure: {st:?} (DHCP first?)"));
        }

        let event = unsafe { boot::create_event(EventType::empty(), Tpl::CALLBACK, None, None) }
            .map_err(|e| format!("create_event: {e:?}"))?;
        let ev = event.as_ptr();

        let mut ct = Tcp4ConnectionToken {
            completion_token: Tcp4CompletionToken {
                event: ev,
                status: Status::NOT_READY,
            },
        };
        let st = unsafe { ((*tcp_ptr).connect)(tcp_ptr, &mut ct) };
        if st != Status::SUCCESS {
            return Err(format!("connect call: {st:?}"));
        }
        let st = unsafe { pump(tcp_ptr, &ct.completion_token.status, super::TCP_CONNECT_TIMEOUT_MS) };
        if st != Status::SUCCESS {
            return Err(format!("connect: {st:?}"));
        }

        // Transmit the GET request.
        let req = build_get(path, host);
        let mut tx = TxData1 {
            push: Boolean::from(true),
            urgent: Boolean::from(false),
            data_length: req.len() as u32,
            fragment_count: 1,
            fragment_table: [Tcp4FragmentData {
                fragment_length: req.len() as u32,
                fragment_buf: req.as_ptr() as *mut u8,
            }],
        };
        let mut txtok = Tcp4IoToken {
            completion_token: Tcp4CompletionToken {
                event: ev,
                status: Status::NOT_READY,
            },
            packet: Tcp4Packet {
                tx_data: (&mut tx as *mut TxData1).cast(),
            },
        };
        let st = unsafe { ((*tcp_ptr).transmit)(tcp_ptr, &mut txtok) };
        if st != Status::SUCCESS {
            return Err(format!("transmit call: {st:?}"));
        }
        let st = unsafe { pump(tcp_ptr, &txtok.completion_token.status, super::TCP_CONNECT_TIMEOUT_MS) };
        if st != Status::SUCCESS {
            return Err(format!("transmit: {st:?}"));
        }

        // Drain the response until Content-Length is satisfied or FIN.
        let mut full: Vec<u8> = Vec::new();
        let mut rxbuf = vec![0u8; 64 * 1024];
        let mut header_end: Option<usize> = None;
        let mut content_len: Option<usize> = None;
        loop {
            let mut rx = RxData1 {
                urgent: Boolean::from(false),
                data_length: rxbuf.len() as u32,
                fragment_count: 1,
                fragment_table: [Tcp4FragmentData {
                    fragment_length: rxbuf.len() as u32,
                    fragment_buf: rxbuf.as_mut_ptr(),
                }],
            };
            let mut rxtok = Tcp4IoToken {
                completion_token: Tcp4CompletionToken {
                    event: ev,
                    status: Status::NOT_READY,
                },
                packet: Tcp4Packet {
                    rx_data: (&mut rx as *mut RxData1).cast(),
                },
            };
            let call = unsafe { ((*tcp_ptr).receive)(tcp_ptr, &mut rxtok) };
            if call == CONNECTION_FIN || call == CONNECTION_RESET {
                break;
            }
            if call != Status::SUCCESS {
                if full.is_empty() {
                    return Err(format!("recv call: {call:?}"));
                }
                break;
            }
            let st = unsafe { pump(tcp_ptr, &rxtok.completion_token.status, 15_000) };
            if st == CONNECTION_FIN || st == CONNECTION_RESET {
                break;
            }
            if st != Status::SUCCESS {
                if full.is_empty() {
                    return Err(format!("recv: {st:?}"));
                }
                break;
            }
            let got = (rx.data_length as usize).min(rxbuf.len());
            if got == 0 {
                break;
            }
            full.extend_from_slice(&rxbuf[..got]);
            if header_end.is_none() {
                if let Some(pos) = full.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = Some(pos + 4);
                    let head = String::from_utf8_lossy(&full[..pos]).to_lowercase();
                    content_len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok());
                }
            }
            if let (Some(he), Some(cl)) = (header_end, content_len) {
                if full.len() >= he + cl {
                    break;
                }
            }
            if full.len() > max_cap {
                let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };
                return Err(format!("response body exceeded {max_cap} bytes"));
            }
        }
        let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };

        let he = header_end.ok_or("no HTTP header in response")?;
        let status_line = full[..he]
            .split(|&b| b == b'\r')
            .next()
            .map(|l| String::from_utf8_lossy(l).to_string())
            .unwrap_or_default();
        let code: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let mut body = full.split_off(he);
        if let Some(cl) = content_len {
            if body.len() < cl {
                return Err(format!("short body: {} of {cl} bytes", body.len()));
            }
            body.truncate(cl);
        }
        logln(format!("tcp: GET if{idx} {code} ({}B)", body.len()));
        Ok((code, body))
    }

    fn run(target: &str, path: &str, body: &[u8], framed: bool) -> Result<String, String> {
        let (rip, rport) =
            parse_target(target).ok_or_else(|| "bad target (use a.b.c.d or a.b.c.d:port)".to_string())?;
        let host = target.to_string();
        logln(format!(
            "tcp: target {}.{}.{}.{}:{} ({})",
            rip.0[0],
            rip.0[1],
            rip.0[2],
            rip.0[3],
            rport,
            if framed { "qc-frame" } else { "http" }
        ));

        let handles = boot::find_handles::<Tcp4Sb>().map_err(|e| {
            logln(format!("tcp: find Tcp4Sb ERR {e:?}"));
            format!("no TCP4 service ({e:?})")
        })?;
        logln(format!("tcp: Tcp4Sb handles={}", handles.len()));

        let mut last = "no TCP4 interface".to_string();
        for (idx, sbh) in handles.into_iter().enumerate() {
            match try_one(sbh, idx, rip, rport, path, &host, body, framed) {
                Ok(s) => return Ok(s),
                Err(e) => {
                    logln(format!("tcp: if{idx} failed: {e}"));
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn try_one(
        sbh: Handle,
        idx: usize,
        rip: Ipv4Address,
        rport: u16,
        path: &str,
        host: &str,
        body: &[u8],
        framed: bool,
    ) -> Result<String, String> {
        let mut sb = unsafe {
            boot::open_protocol::<Tcp4Sb>(
                OpenProtocolParams {
                    handle: sbh,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        }
        .map_err(|e| format!("open sb: {e:?}"))?;

        let mut child: uefi_raw::Handle = core::ptr::null_mut();
        let st = unsafe { (sb.0.create_child)(&mut sb.0, &mut child) };
        if st != Status::SUCCESS {
            return Err(format!("create_child: {st:?}"));
        }
        let child_handle = unsafe { Handle::from_ptr(child) }.ok_or("null child handle")?;
        logln(format!("tcp: if{idx} child created"));

        let result = try_child(child_handle, idx, rip, rport, path, host, body, framed);

        // Always tear the child down.
        let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
        result
    }

    fn try_child(
        child: Handle,
        idx: usize,
        rip: Ipv4Address,
        rport: u16,
        path: &str,
        host: &str,
        body: &[u8],
        framed: bool,
    ) -> Result<String, String> {
        let mut tcp = unsafe {
            boot::open_protocol::<Tcp4>(
                OpenProtocolParams {
                    handle: child,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        }
        .map_err(|e| format!("open tcp4: {e:?}"))?;
        let tcp_ptr: *mut Tcp4Protocol = &mut tcp.0;

        let cfg = Tcp4ConfigData {
            type_of_service: 0,
            time_to_live: 64,
            access_point: Tcp4AccessPoint {
                use_default_address: Boolean::from(true),
                station_address: Ipv4Address([0, 0, 0, 0]),
                subnet_mask: Ipv4Address([0, 0, 0, 0]),
                station_port: 0,
                remote_address: rip,
                remote_port: rport,
                active_flag: Boolean::from(true),
            },
            control_option: core::ptr::null_mut(),
        };
        let st = unsafe { ((*tcp_ptr).configure)(tcp_ptr, &cfg) };
        if st != Status::SUCCESS {
            return Err(format!("configure: {st:?} (DHCP first?)"));
        }
        logln(format!("tcp: if{idx} configured"));

        // One reusable completion event (we poll status, never wait on it).
        let event = unsafe { boot::create_event(EventType::empty(), Tpl::CALLBACK, None, None) }
            .map_err(|e| format!("create_event: {e:?}"))?;
        let ev = event.as_ptr();

        // Connect.
        let mut ct = Tcp4ConnectionToken {
            completion_token: Tcp4CompletionToken {
                event: ev,
                status: Status::NOT_READY,
            },
        };
        let st = unsafe { ((*tcp_ptr).connect)(tcp_ptr, &mut ct) };
        if st != Status::SUCCESS {
            return Err(format!("connect call: {st:?}"));
        }
        let st = unsafe { pump(tcp_ptr, &ct.completion_token.status, super::TCP_CONNECT_TIMEOUT_MS) };
        if st != Status::SUCCESS {
            return Err(format!("connect: {st:?}"));
        }
        logln(format!("tcp: if{idx} connected"));

        // Transmit the request (framed QC payload or a hand-built HTTP request).
        let req = if framed {
            build_qc_frame(body)
        } else {
            build_request(path, host, body)
        };
        let mut tx = TxData1 {
            push: Boolean::from(true),
            urgent: Boolean::from(false),
            data_length: req.len() as u32,
            fragment_count: 1,
            fragment_table: [Tcp4FragmentData {
                fragment_length: req.len() as u32,
                fragment_buf: req.as_ptr() as *mut u8,
            }],
        };
        let mut txtok = Tcp4IoToken {
            completion_token: Tcp4CompletionToken {
                event: ev,
                status: Status::NOT_READY,
            },
            packet: Tcp4Packet {
                tx_data: (&mut tx as *mut TxData1).cast(),
            },
        };
        let st = unsafe { ((*tcp_ptr).transmit)(tcp_ptr, &mut txtok) };
        if st != Status::SUCCESS {
            return Err(format!("transmit call: {st:?}"));
        }
        let st = unsafe { pump(tcp_ptr, &txtok.completion_token.status, super::TCP_CONNECT_TIMEOUT_MS) };
        if st != Status::SUCCESS {
            return Err(format!("transmit: {st:?}"));
        }
        logln(format!("tcp: if{idx} sent {} bytes", req.len()));

        // Best-effort single read of the response.
        let mut rxbuf = vec![0u8; 1024];
        let mut rx = RxData1 {
            urgent: Boolean::from(false),
            data_length: rxbuf.len() as u32,
            fragment_count: 1,
            fragment_table: [Tcp4FragmentData {
                fragment_length: rxbuf.len() as u32,
                fragment_buf: rxbuf.as_mut_ptr(),
            }],
        };
        let mut rxtok = Tcp4IoToken {
            completion_token: Tcp4CompletionToken {
                event: ev,
                status: Status::NOT_READY,
            },
            packet: Tcp4Packet {
                rx_data: (&mut rx as *mut RxData1).cast(),
            },
        };
        let resp_line = match unsafe { ((*tcp_ptr).receive)(tcp_ptr, &mut rxtok) } {
            Status::SUCCESS => {
                let st = unsafe { pump(tcp_ptr, &rxtok.completion_token.status, 5_000) };
                if st == Status::SUCCESS {
                    let got = (rx.data_length as usize).min(rxbuf.len());
                    let data = &rxbuf[..got];
                    if framed {
                        // Ack frame: [u32 len][u8 tag][json] — show the JSON body.
                        if data.len() > 5 {
                            String::from_utf8_lossy(&data[5..]).trim().to_string()
                        } else {
                            format!("(short ack {} bytes)", data.len())
                        }
                    } else {
                        String::from_utf8_lossy(data)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string()
                    }
                } else {
                    format!("(no response: {st:?})")
                }
            }
            s => format!("(recv: {s:?})"),
        };
        logln(format!("tcp: if{idx} resp: {resp_line}"));

        // Politely close.
        let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };

        Ok(format!("sent {}B via if{idx}; resp: {resp_line}", req.len()))
    }

    /// A persistent TCP4 connection to a console's direct-link listener. Unlike
    /// [`run`], the socket stays open across many main-loop iterations so the
    /// firmware can stream frames and receive input/plugin frames live.
    pub struct DirectLink {
        sb: boot::ScopedProtocol<Tcp4Sb>,
        child: uefi_raw::Handle,
        tcp: boot::ScopedProtocol<Tcp4>,
        event: uefi::Event,
        rx: Vec<u8>,
    }

    impl DirectLink {
        /// Dial `target` ("a.b.c.d:port"), open a child, configure + connect.
        pub fn connect(target: &str) -> Result<Self, String> {
            if tcp4_absent() {
                return Err("no TCP4 stack".into());
            }
            let (rip, rport) =
                parse_target(target).ok_or_else(|| "bad console addr".to_string())?;
            let sbh = *boot::find_handles::<Tcp4Sb>()
                .map_err(|e| format!("find Tcp4Sb: {e:?}"))?
                .first()
                .ok_or("no TCP4 interface")?;
            let mut sb = unsafe {
                boot::open_protocol::<Tcp4Sb>(
                    OpenProtocolParams { handle: sbh, agent: boot::image_handle(), controller: None },
                    OpenProtocolAttributes::GetProtocol,
                )
            }
            .map_err(|e| format!("open sb: {e:?}"))?;

            let mut child: uefi_raw::Handle = core::ptr::null_mut();
            let st = unsafe { (sb.0.create_child)(&mut sb.0, &mut child) };
            if st != Status::SUCCESS {
                return Err(format!("create_child: {st:?}"));
            }
            let child_handle = match unsafe { Handle::from_ptr(child) } {
                Some(h) => h,
                None => {
                    let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                    return Err("null child handle".into());
                }
            };

            let mut tcp = match unsafe {
                boot::open_protocol::<Tcp4>(
                    OpenProtocolParams {
                        handle: child_handle,
                        agent: boot::image_handle(),
                        controller: None,
                    },
                    OpenProtocolAttributes::GetProtocol,
                )
            } {
                Ok(t) => t,
                Err(e) => {
                    let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                    return Err(format!("open tcp4: {e:?}"));
                }
            };
            let tcp_ptr: *mut Tcp4Protocol = &mut tcp.0;

            let cfg = Tcp4ConfigData {
                type_of_service: 0,
                time_to_live: 64,
                access_point: Tcp4AccessPoint {
                    use_default_address: Boolean::from(true),
                    station_address: Ipv4Address([0, 0, 0, 0]),
                    subnet_mask: Ipv4Address([0, 0, 0, 0]),
                    station_port: 0,
                    remote_address: rip,
                    remote_port: rport,
                    active_flag: Boolean::from(true),
                },
                control_option: core::ptr::null_mut(),
            };
            let st = unsafe { ((*tcp_ptr).configure)(tcp_ptr, &cfg) };
            if st != Status::SUCCESS {
                let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                return Err(format!("configure: {st:?}"));
            }
            let event = match unsafe { boot::create_event(EventType::empty(), Tpl::CALLBACK, None, None) } {
                Ok(e) => e,
                Err(e) => {
                    let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };
                    let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                    return Err(format!("create_event: {e:?}"));
                }
            };
            let mut ct = Tcp4ConnectionToken {
                completion_token: Tcp4CompletionToken { event: event.as_ptr(), status: Status::NOT_READY },
            };
            let st = unsafe { ((*tcp_ptr).connect)(tcp_ptr, &mut ct) };
            if st == Status::SUCCESS {
                let st = unsafe { pump(tcp_ptr, &ct.completion_token.status, 8_000) };
                if st != Status::SUCCESS {
                    let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };
                    let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                    return Err(format!("connect: {st:?}"));
                }
            } else {
                let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };
                let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                return Err(format!("connect call: {st:?}"));
            }
            logln(format!("direct: connected to {target}"));
            Ok(Self { sb, child, tcp, event, rx: Vec::new() })
        }

        /// Send one `[u32 LE total_len][tag][body]` frame.
        pub fn send(&mut self, tag: u8, body: &[u8]) -> Result<(), String> {
            let mut buf = Vec::with_capacity(5 + body.len());
            buf.extend_from_slice(&((1 + body.len()) as u32).to_le_bytes());
            buf.push(tag);
            buf.extend_from_slice(body);
            let tcp_ptr: *mut Tcp4Protocol = &mut self.tcp.0;
            let mut tx = TxData1 {
                push: Boolean::from(true),
                urgent: Boolean::from(false),
                data_length: buf.len() as u32,
                fragment_count: 1,
                fragment_table: [Tcp4FragmentData {
                    fragment_length: buf.len() as u32,
                    fragment_buf: buf.as_ptr() as *mut u8,
                }],
            };
            let mut tok = Tcp4IoToken {
                completion_token: Tcp4CompletionToken { event: self.event.as_ptr(), status: Status::NOT_READY },
                packet: Tcp4Packet { tx_data: (&mut tx as *mut TxData1).cast() },
            };
            let st = unsafe { ((*tcp_ptr).transmit)(tcp_ptr, &mut tok) };
            if st != Status::SUCCESS {
                return Err(format!("transmit call: {st:?}"));
            }
            let st = unsafe { pump(tcp_ptr, &tok.completion_token.status, 8_000) };
            if st != Status::SUCCESS {
                return Err(format!("transmit: {st:?}"));
            }
            Ok(())
        }

        /// Non-blocking-ish receive: read whatever is ready within `budget_ms`,
        /// buffer it, and return one complete frame `(tag, body)` if available.
        pub fn poll_frame(&mut self, budget_ms: u32) -> Result<Option<(u8, Vec<u8>)>, String> {
            if let Some(f) = self.take_frame() {
                return Ok(Some(f));
            }
            let tcp_ptr: *mut Tcp4Protocol = &mut self.tcp.0;
            let mut rxbuf = vec![0u8; 8 * 1024];
            let mut rx = RxData1 {
                urgent: Boolean::from(false),
                data_length: rxbuf.len() as u32,
                fragment_count: 1,
                fragment_table: [Tcp4FragmentData {
                    fragment_length: rxbuf.len() as u32,
                    fragment_buf: rxbuf.as_mut_ptr(),
                }],
            };
            let mut tok = Tcp4IoToken {
                completion_token: Tcp4CompletionToken { event: self.event.as_ptr(), status: Status::NOT_READY },
                packet: Tcp4Packet { rx_data: (&mut rx as *mut RxData1).cast() },
            };
            let call = unsafe { ((*tcp_ptr).receive)(tcp_ptr, &mut tok) };
            if call == CONNECTION_FIN || call == CONNECTION_RESET {
                return Err(format!("recv closed: {call:?}"));
            }
            if call != Status::SUCCESS {
                // No data ready — not an error.
                return Ok(None);
            }
            let st = unsafe { pump(tcp_ptr, &tok.completion_token.status, budget_ms) };
            if st == CONNECTION_FIN || st == CONNECTION_RESET {
                return Err(format!("recv closed: {st:?}"));
            }
            if st == Status::TIMEOUT {
                // Cancel the outstanding receive so the token/buffer is free.
                let _ = unsafe { ((*tcp_ptr).cancel)(tcp_ptr, core::ptr::null_mut()) };
                return Ok(None);
            }
            if st != Status::SUCCESS {
                return Ok(None);
            }
            let got = (rx.data_length as usize).min(rxbuf.len());
            if got > 0 {
                self.rx.extend_from_slice(&rxbuf[..got]);
            }
            Ok(self.take_frame())
        }

        /// Extract one complete framed message from the receive buffer.
        fn take_frame(&mut self) -> Option<(u8, Vec<u8>)> {
            if self.rx.len() < 4 {
                return None;
            }
            let total = u32::from_le_bytes([self.rx[0], self.rx[1], self.rx[2], self.rx[3]]) as usize;
            if total == 0 || total > (16 << 20) {
                self.rx.clear();
                return None;
            }
            if self.rx.len() < 4 + total {
                return None;
            }
            let tag = self.rx[4];
            let body = self.rx[5..4 + total].to_vec();
            self.rx.drain(0..4 + total);
            Some((tag, body))
        }
    }

    impl Drop for DirectLink {
        fn drop(&mut self) {
            let tcp_ptr: *mut Tcp4Protocol = &mut self.tcp.0;
            let _ = unsafe { ((*tcp_ptr).configure)(tcp_ptr, core::ptr::null()) };
            let _ = unsafe { (self.sb.0.destroy_child)(&mut self.sb.0, self.child) };
        }
    }
}

// ---------------------------------------------------------------------------
// Storage, security (Secure Boot / TPM), CPU features
// ---------------------------------------------------------------------------

struct Disk {
    capacity: u64,
    removable: bool,
    bus: &'static str,
}

/// Classify a block device's bus from its device path's messaging node.
fn disk_bus(handle: uefi::Handle) -> &'static str {
    use uefi::proto::device_path::{DevicePath, DeviceType};
    let dp = match unsafe {
        uefi::boot::open_protocol::<DevicePath>(
            OpenProtocolParams {
                handle,
                agent: uefi::boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    } {
        Ok(d) => d,
        Err(_) => return "unknown",
    };
    let mut bus = "unknown";
    for node in dp.node_iter() {
        if node.device_type() == DeviceType::MESSAGING {
            bus = match node.sub_type().0 {
                23 | 34 => "NVMe",
                18 => "SATA",
                5 | 15 | 16 => "USB",
                2 | 22 => "SCSI/SAS",
                1 => "ATAPI",
                25 => "UFS",
                26 => "SD",
                29 => "eMMC",
                _ => bus,
            };
        }
    }
    bus
}

/// Map a bus to the schema's SSD/HDD-ish drive_type. NVMe/eMMC/UFS are always
/// solid-state; SATA can't be told apart without ATA IDENTIFY, so report the
/// bus.
fn drive_type(bus: &str) -> &'static str {
    match bus {
        "NVMe" => "SSD (NVMe)",
        "eMMC" => "eMMC",
        "UFS" => "UFS",
        "USB" => "USB",
        "SATA" => "SATA",
        _ => "unknown",
    }
}

fn collect_storage() -> Vec<Disk> {
    use uefi::proto::media::block::BlockIO;
    let mut out: Vec<Disk> = Vec::new();
    let Ok(handles) = uefi::boot::find_handles::<BlockIO>() else {
        return out;
    };
    for handle in handles {
        let bio = match unsafe {
            uefi::boot::open_protocol::<BlockIO>(
                OpenProtocolParams {
                    handle,
                    agent: uefi::boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            Ok(b) => b,
            Err(_) => continue,
        };
        let m = bio.media();
        // Whole disks only (skip partition handles) that have media present.
        if m.is_logical_partition() || !m.is_media_present() {
            continue;
        }
        let cap = (m.last_block() + 1) * m.block_size() as u64;
        if cap == 0 {
            continue;
        }
        out.push(Disk {
            capacity: cap,
            removable: m.is_removable_media(),
            bus: disk_bus(handle),
        });
    }
    // The same physical disk can expose multiple BlockIO handles; dedupe.
    out.sort_by(|a, b| b.capacity.cmp(&a.capacity));
    out.dedup_by(|a, b| a.capacity == b.capacity && a.removable == b.removable && a.bus == b.bus);
    out
}

struct NvmeDrive {
    model: String,
    serial: String,
    firmware: String,
    smart: Option<NvmeSmart>,
}

/// NVMe SMART / Health Information (Get Log Page 0x02).
struct NvmeSmart {
    critical_warning: u8,
    temp_c: i32,
    percentage_used: u8,
    available_spare: u8,
    power_on_hours: u64,
    data_units_written: u64,
    media_errors: u64,
}

fn ascii_field(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim().to_string()
}

/// Little-endian u64 at `off`, or 0 when out of range.
fn le64(buf: &[u8], off: usize) -> u64 {
    if off + 8 <= buf.len() {
        u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
    } else {
        0
    }
}

/// Query each NVMe controller's IDENTIFY CONTROLLER data for model/serial/fw.
fn collect_nvme() -> Vec<NvmeDrive> {
    use uefi::proto::nvme::pass_thru::NvmePassThru;
    use uefi::proto::nvme::{NvmeQueueType, NvmeRequestBuilder};
    let mut out = Vec::new();
    let handles = match uefi::boot::find_handles::<NvmePassThru>() {
        Ok(h) => h,
        Err(e) => {
            logln(format!("nvme: find NvmePassThru ERR {e:?}"));
            return out;
        }
    };
    logln(format!("nvme: NvmePassThru handles={}", handles.len()));
    for handle in handles {
        let pt = match unsafe {
            uefi::boot::open_protocol::<NvmePassThru>(
                OpenProtocolParams {
                    handle,
                    agent: uefi::boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            Ok(p) => p,
            Err(_) => continue,
        };
        let align = pt.io_align();
        // Identify Controller: admin opcode 0x06, CNS=1 in CDW10, 4 KiB result.
        let id_req = match NvmeRequestBuilder::new(align, 0x06, NvmeQueueType::ADMIN)
            .with_cdw10(1)
            .with_transfer_buffer(4096)
        {
            Ok(b) => b.build(),
            Err(_) => continue,
        };
        let mut ns = pt.controller();
        let (mut model, mut serial, mut firmware) = (String::new(), String::new(), String::new());
        match ns.execute_command(id_req) {
            Ok(resp) => {
                if let Some(buf) = resp.transfer_buffer() {
                    if buf.len() >= 72 {
                        serial = ascii_field(&buf[4..24]);
                        model = ascii_field(&buf[24..64]);
                        firmware = ascii_field(&buf[64..72]);
                    }
                }
            }
            Err(e) => {
                logln(format!("nvme: IDENTIFY ERR {e:?}"));
                continue;
            }
        }
        // SMART / Health log: Get Log Page (opcode 0x02), LID 0x02, 512 bytes
        // (NUMDL = 127 dwords, 0-based, in CDW10[27:16]).
        let smart = (|| {
            let cdw10 = (127u32 << 16) | 0x02;
            let req = NvmeRequestBuilder::new(align, 0x02, NvmeQueueType::ADMIN)
                .with_cdw10(cdw10)
                .with_transfer_buffer(512)
                .ok()?
                .build();
            let resp = ns.execute_command(req).ok()?;
            let buf = resp.transfer_buffer()?;
            if buf.len() < 168 {
                return None;
            }
            Some(NvmeSmart {
                critical_warning: buf[0],
                temp_c: u16::from_le_bytes([buf[1], buf[2]]) as i32 - 273,
                available_spare: buf[3],
                percentage_used: buf[5],
                data_units_written: le64(buf, 48),
                power_on_hours: le64(buf, 128),
                media_errors: le64(buf, 160),
            })
        })();
        out.push(NvmeDrive { model, serial, firmware, smart });
    }
    out
}

/// Returns Some(enabled) if the SecureBoot variable exists (i.e. the platform
/// is Secure Boot capable); the bool is whether it's currently enabled.
fn secure_boot() -> Option<bool> {
    let mut buf = [0u8; 1];
    match uefi::runtime::get_variable(
        uefi::cstr16!("SecureBoot"),
        &uefi::runtime::VariableVendor::GLOBAL_VARIABLE,
        &mut buf,
    ) {
        Ok((data, _)) => data.first().map(|&b| b == 1),
        Err(_) => None,
    }
}

/// Count of UEFI boot entries (`BootOrder` is a u16 array).
fn boot_entry_count() -> usize {
    let mut buf = [0u8; 512];
    match uefi::runtime::get_variable(
        uefi::cstr16!("BootOrder"),
        &uefi::runtime::VariableVendor::GLOBAL_VARIABLE,
        &mut buf,
    ) {
        Ok((data, _)) => data.len() / 2,
        Err(_) => 0,
    }
}

/// True when a global UEFI variable exists (present even if larger than the probe).
fn global_var_present(name: &uefi::CStr16) -> bool {
    let mut buf = [0u8; 4096];
    match uefi::runtime::get_variable(name, &uefi::runtime::VariableVendor::GLOBAL_VARIABLE, &mut buf) {
        Ok(_) => true,
        Err(e) => e.status() == uefi::Status::BUFFER_TOO_SMALL,
    }
}

/// `SetupMode` global variable: Some(true) = Setup Mode (no platform key enrolled).
fn setup_mode() -> Option<bool> {
    let mut buf = [0u8; 1];
    match uefi::runtime::get_variable(
        uefi::cstr16!("SetupMode"),
        &uefi::runtime::VariableVendor::GLOBAL_VARIABLE,
        &mut buf,
    ) {
        Ok((data, _)) => data.first().map(|&b| b == 1),
        Err(_) => None,
    }
}

/// Returns Some(description) if a TPM 2.0 is present.
fn tpm_version() -> Option<String> {
    use uefi::proto::tcg::v2::Tcg;
    let handles = uefi::boot::find_handles::<Tcg>().ok()?;
    let handle = *handles.first()?;
    let mut tcg = unsafe {
        uefi::boot::open_protocol::<Tcg>(
            OpenProtocolParams {
                handle,
                agent: uefi::boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()?;
    let cap = tcg.get_capability().ok()?;
    Some(format!("2.0 (mfr 0x{:08x})", cap.manufacturer_id))
}

fn cpu_features() -> Vec<&'static str> {
    let mut f = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{__cpuid, __cpuid_count};
        let c1 = __cpuid(1);
        if c1.ecx & (1 << 25) != 0 {
            f.push("AES");
        }
        if c1.ecx & (1 << 28) != 0 {
            f.push("AVX");
        }
        if c1.ecx & (1 << 5) != 0 {
            f.push("VT-x");
        }
        if c1.ecx & (1 << 31) != 0 {
            f.push("hypervisor");
        }
        let c7 = __cpuid_count(7, 0);
        if c7.ebx & (1 << 5) != 0 {
            f.push("AVX2");
        }
        let ca = __cpuid(0x8000_0001);
        if ca.ecx & (1 << 2) != 0 {
            f.push("AMD-V");
        }
    }
    f
}

/// Detect the ACPI `MSDM` table — an embedded OEM Windows digital license.
/// Returns (present, oem_key). Presence is a strong proxy for "this machine
/// shipped with an OEM Windows license"; true activation needs the OS.
fn collect_msdm() -> (bool, String) {
    let mut acpi2 = 0usize;
    let mut acpi1 = 0usize;
    uefi::system::with_config_table(|entries| {
        for e in entries {
            if e.guid == ConfigTableEntry::ACPI2_GUID {
                acpi2 = e.address as usize;
            } else if e.guid == ConfigTableEntry::ACPI_GUID {
                acpi1 = e.address as usize;
            }
        }
    });

    let rd_u32 = |p: usize| -> u32 {
        unsafe { u32::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap()) }
    };
    let rd_u64 = |p: usize| -> u64 {
        unsafe { u64::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 8).try_into().unwrap()) }
    };
    let sig4 = |p: usize| -> [u8; 4] {
        unsafe { core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap() }
    };

    // Resolve the system descriptor table: prefer ACPI 2.0 XSDT (8-byte
    // pointers), else ACPI 1.0 RSDT (4-byte pointers).
    let (table_base, count, ptr_size) = if acpi2 != 0 && &sig4(acpi2) == b"RSD " {
        let xsdt = rd_u64(acpi2 + 24) as usize;
        if xsdt == 0 {
            return (false, String::new());
        }
        let len = rd_u32(xsdt + 4) as usize;
        (xsdt + 36, len.saturating_sub(36) / 8, 8usize)
    } else if acpi1 != 0 {
        let rsdt = rd_u32(acpi1 + 16) as usize;
        if rsdt == 0 {
            return (false, String::new());
        }
        let len = rd_u32(rsdt + 4) as usize;
        (rsdt + 36, len.saturating_sub(36) / 4, 4usize)
    } else {
        return (false, String::new());
    };

    if count > 1024 {
        return (false, String::new());
    }
    for i in 0..count {
        let ep = table_base + i * ptr_size;
        let table = if ptr_size == 8 {
            rd_u64(ep) as usize
        } else {
            rd_u32(ep) as usize
        };
        if table == 0 {
            continue;
        }
        if &sig4(table) == b"MSDM" {
            let tlen = rd_u32(table + 4) as usize;
            // SLS structure: 29-char OEM key at offset 0x38.
            let key = if tlen >= 0x38 + 29 {
                let kb = unsafe { core::slice::from_raw_parts((table + 0x38) as *const u8, 29) };
                String::from_utf8_lossy(kb).trim().to_string()
            } else {
                String::new()
            };
            return (true, key);
        }
    }
    (false, String::new())
}

/// Physical address of the ACPI table with signature `want`, if present.
fn find_acpi_table(want: &[u8; 4]) -> Option<usize> {
    let mut acpi2 = 0usize;
    let mut acpi1 = 0usize;
    uefi::system::with_config_table(|entries| {
        for e in entries {
            if e.guid == ConfigTableEntry::ACPI2_GUID {
                acpi2 = e.address as usize;
            } else if e.guid == ConfigTableEntry::ACPI_GUID {
                acpi1 = e.address as usize;
            }
        }
    });
    unsafe {
        let rd_u32 = |p: usize| -> u32 {
            u32::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap())
        };
        let rd_u64 = |p: usize| -> u64 {
            u64::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 8).try_into().unwrap())
        };
        let sig4 = |p: usize| -> [u8; 4] {
            core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap()
        };
        let (base, count, ps) = if acpi2 != 0 && &sig4(acpi2) == b"RSD " {
            let xsdt = rd_u64(acpi2 + 24) as usize;
            if xsdt == 0 {
                return None;
            }
            let len = rd_u32(xsdt + 4) as usize;
            (xsdt + 36, len.saturating_sub(36) / 8, 8usize)
        } else if acpi1 != 0 {
            let rsdt = rd_u32(acpi1 + 16) as usize;
            if rsdt == 0 {
                return None;
            }
            let len = rd_u32(rsdt + 4) as usize;
            (rsdt + 36, len.saturating_sub(36) / 4, 4usize)
        } else {
            return None;
        };
        if count > 1024 {
            return None;
        }
        for i in 0..count {
            let ep = base + i * ps;
            let t = if ps == 8 { rd_u64(ep) as usize } else { rd_u32(ep) as usize };
            if t != 0 && &sig4(t) == want {
                return Some(t);
            }
        }
        None
    }
}

/// ACPI Boot Error Record Table — the firmware's record of the last fatal
/// hardware error, persisted across reset.
#[derive(Default)]
struct BertInfo {
    present: bool,
    error_present: bool,
    severity: String,
    entry_count: u32,
}

fn collect_bert() -> BertInfo {
    let Some(bert) = find_acpi_table(b"BERT") else {
        return BertInfo::default();
    };
    unsafe {
        let rd_u32 = |p: usize| -> u32 {
            u32::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap())
        };
        let rd_u64 = |p: usize| -> u64 {
            u64::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 8).try_into().unwrap())
        };
        let region_len = rd_u32(bert + 36);
        let region = rd_u64(bert + 40) as usize;
        if region == 0 || region_len < 20 {
            return BertInfo { present: true, ..Default::default() };
        }
        let block_status = rd_u32(region);
        let severity = rd_u32(region + 16);
        BertInfo {
            present: true,
            error_present: block_status & 0xF != 0,
            severity: match severity {
                0 => "recoverable",
                1 => "fatal",
                2 => "corrected",
                3 => "none",
                _ => "unknown",
            }
            .to_string(),
            entry_count: (block_status >> 4) & 0x3FF,
        }
    }
}

/// ACPI Firmware Performance Data Table — firmware boot-phase timings. A boot
/// that ran a full memory retrain shows a longer `fw_boot_ms` than a cached one.
#[derive(Default)]
struct FpdtInfo {
    present: bool,
    /// Reset → ExitBootServices-exit duration, ms.
    fw_boot_ms: u64,
    /// Reset → OS-loader StartImage duration, ms.
    os_loader_ms: u64,
}

fn collect_fpdt() -> FpdtInfo {
    let Some(fpdt) = find_acpi_table(b"FPDT") else {
        return FpdtInfo::default();
    };
    unsafe {
        let rd_u8 = |p: usize| -> u8 { *(p as *const u8) };
        let rd_u16 = |p: usize| -> u16 {
            u16::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 2).try_into().unwrap())
        };
        let rd_u32 = |p: usize| -> u32 {
            u32::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap())
        };
        let rd_u64 = |p: usize| -> u64 {
            u64::from_le_bytes(core::slice::from_raw_parts(p as *const u8, 8).try_into().unwrap())
        };
        let sig4 = |p: usize| -> [u8; 4] {
            core::slice::from_raw_parts(p as *const u8, 4).try_into().unwrap()
        };

        // Basic Boot Performance Pointer record (type 0) → FBPT physical address.
        let tlen = rd_u32(fpdt + 4) as usize;
        let mut off = fpdt + 36;
        let mut fbpt = 0usize;
        let mut guard = 0;
        while off + 4 <= fpdt + tlen && guard < 64 {
            guard += 1;
            let rtype = rd_u16(off);
            let rlen = rd_u8(off + 2) as usize;
            if rlen < 4 {
                break;
            }
            if rtype == 0x0000 && rlen >= 16 {
                fbpt = rd_u64(off + 8) as usize;
                break;
            }
            off += rlen;
        }
        if fbpt == 0 || &sig4(fbpt) != b"FBPT" {
            return FpdtInfo { present: true, ..Default::default() };
        }

        // Basic Boot Performance Data record (type 2): ns timestamps from reset.
        let flen = rd_u32(fbpt + 4) as usize;
        let mut foff = fbpt + 8;
        let mut guard = 0;
        while foff + 4 <= fbpt + flen && guard < 64 {
            guard += 1;
            let rtype = rd_u16(foff);
            let rlen = rd_u8(foff + 2) as usize;
            if rlen < 4 {
                break;
            }
            if rtype == 0x0002 && rlen >= 48 {
                let reset_end = rd_u64(foff + 8);
                let os_loader_start = rd_u64(foff + 24);
                let exit_bs_exit = rd_u64(foff + 40);
                return FpdtInfo {
                    present: true,
                    fw_boot_ms: exit_bs_exit.saturating_sub(reset_end) / 1_000_000,
                    os_loader_ms: os_loader_start.saturating_sub(reset_end) / 1_000_000,
                };
            }
            foff += rlen;
        }
        FpdtInfo { present: true, ..Default::default() }
    }
}

// ---------------------------------------------------------------------------
// Root-bridge I/O (CMOS via .Io, SPD/PMIC via i801 SMBus, MCHBAR via .Mem)
// ---------------------------------------------------------------------------

fn rb_open() -> Option<ScopedProtocol<PciRootBridgeIo>> {
    for h in uefi::boot::find_handles::<PciRootBridgeIo>().ok()? {
        if let Ok(r) = unsafe {
            uefi::boot::open_protocol::<PciRootBridgeIo>(
                OpenProtocolParams { handle: h, agent: uefi::boot::image_handle(), controller: None },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            return Some(r);
        }
    }
    None
}

fn cfg_rd32(root: &mut ScopedProtocol<PciRootBridgeIo>, bus: u8, dev: u8, fun: u8, reg: u8) -> Option<u32> {
    let a = uefi::proto::pci::PciIoAddress::new(bus, dev, fun).with_register(reg);
    root.pci().read_one::<u32>(a).ok()
}

fn cfg_wr16(root: &mut ScopedProtocol<PciRootBridgeIo>, bus: u8, dev: u8, fun: u8, reg: u8, v: u16) {
    let a = uefi::proto::pci::PciIoAddress::new(bus, dev, fun).with_register(reg);
    let _ = root.pci().write_one::<u16>(a, v);
}

fn cfg_wr8(root: &mut ScopedProtocol<PciRootBridgeIo>, bus: u8, dev: u8, fun: u8, reg: u8, v: u8) {
    let a = uefi::proto::pci::PciIoAddress::new(bus, dev, fun).with_register(reg);
    let _ = root.pci().write_one::<u8>(a, v);
}

fn io_r(root: &mut ScopedProtocol<PciRootBridgeIo>, port: u16) -> Option<u8> {
    root.io().read_one::<u8>(port as u32).ok()
}

fn io_w(root: &mut ScopedProtocol<PciRootBridgeIo>, port: u16, v: u8) {
    let _ = root.io().write_one::<u8>(port as u32, v);
}

fn mem_r32(root: &mut ScopedProtocol<PciRootBridgeIo>, addr: u64) -> Option<u32> {
    root.memory().read_one::<u32>(addr).ok()
}

/// CMOS/RTC config-status region read over the legacy 0x70/0x71 ports.
#[derive(Default)]
struct CmosInfo {
    present: bool,
    rtc_power_lost: bool,
    checksum_bad: bool,
    diag: u8,
    dump: String,
}

fn cmos_read(root: &mut ScopedProtocol<PciRootBridgeIo>, idx: u8) -> Option<u8> {
    io_w(root, 0x70, idx & 0x7F);
    io_r(root, 0x71)
}

fn collect_cmos() -> CmosInfo {
    let Some(mut root) = rb_open() else {
        return CmosInfo::default();
    };
    let Some(diag) = cmos_read(&mut root, 0x0E) else {
        return CmosInfo::default();
    };
    let mut dump = String::new();
    for idx in 0x0Eu8..0x40 {
        dump.push_str(&format!("{:02x}", cmos_read(&mut root, idx).unwrap_or(0)));
    }
    CmosInfo {
        present: true,
        rtc_power_lost: diag & 0x80 != 0,
        checksum_bad: diag & 0x40 != 0,
        diag,
        dump,
    }
}

/// Run one i801 transaction (cnt = protocol | START); returns final status on
/// INTR, None on error/timeout. Clears status before start, leaves INUSE held.
fn smb_run(
    root: &mut ScopedProtocol<PciRootBridgeIo>,
    base: u16,
    add: u8,
    cmd: u8,
    dat0: Option<u8>,
    cnt: u8,
) -> Option<u8> {
    for _ in 0..40 {
        if io_r(root, base)? & 0x01 == 0 {
            break;
        }
        uefi::boot::stall(core::time::Duration::from_micros(250));
    }
    io_w(root, base, 0x9E);
    io_w(root, base + 4, add);
    io_w(root, base + 3, cmd);
    if let Some(d) = dat0 {
        io_w(root, base + 5, d);
    }
    io_w(root, base + 2, cnt);
    for _ in 0..400 {
        let s = io_r(root, base)?;
        if s & 0x1C != 0 {
            io_w(root, base + 2, 0x02);
            io_w(root, base, 0x9E);
            return None;
        }
        if s & 0x02 != 0 {
            return Some(s);
        }
        uefi::boot::stall(core::time::Duration::from_micros(250));
    }
    io_w(root, base + 2, 0x02);
    io_w(root, base, 0x9E);
    None
}

fn smb_read_byte(root: &mut ScopedProtocol<PciRootBridgeIo>, base: u16, addr7: u8, reg: u8) -> Option<u8> {
    smb_run(root, base, (addr7 << 1) | 1, reg, None, 0x48)?;
    let v = io_r(root, base + 5);
    io_w(root, base, 0x9E);
    v
}

/// SMBus Read-Word, byte-swapped to JEDEC MSB:LSB (matches i2c_smbus_read_word_swapped).
fn smb_read_word(root: &mut ScopedProtocol<PciRootBridgeIo>, base: u16, addr7: u8, reg: u8) -> Option<u16> {
    smb_run(root, base, (addr7 << 1) | 1, reg, None, 0x4C)?;
    let lo = io_r(root, base + 5)?;
    let hi = io_r(root, base + 6)?;
    io_w(root, base, 0x9E);
    Some(((lo as u16) << 8) | hi as u16)
}

/// SMBus Read-Word in wire order (LSB first) — what SBS Smart Battery returns.
fn smb_read_word_le(root: &mut ScopedProtocol<PciRootBridgeIo>, base: u16, addr7: u8, reg: u8) -> Option<u16> {
    smb_run(root, base, (addr7 << 1) | 1, reg, None, 0x4C)?;
    let lo = io_r(root, base + 5)?;
    let hi = io_r(root, base + 6)?;
    io_w(root, base, 0x9E);
    Some(((hi as u16) << 8) | lo as u16)
}

/// SBS Smart Battery SMBus address.
const SBS_ADDR: u8 = 0x0B;

/// Power source for the capsule pre-flight: chassis type and SMBIOS type 22 give
/// portability and battery presence, the SBS Smart Battery gives live charge.
fn collect_power(d: &Smbios) -> capsule::PowerState {
    let mut p = capsule::PowerState {
        portable: d.is_portable(),
        battery_present: !d.batteries.is_empty(),
        charge_pct: None,
        discharging: None,
        source: if d.present { "smbios" } else { "none" },
    };
    if !p.portable {
        return p;
    }
    let Some(mut root) = rb_open() else {
        return p;
    };
    let Some(base) = smbus_base(&mut root) else {
        return p;
    };
    // Acquire INUSE (single read; the act of reading sets the semaphore).
    let _ = io_r(&mut root, base);

    if let Some(status) = smb_read_word_le(&mut root, base, SBS_ADDR, 0x16) {
        p.battery_present = true;
        p.discharging = Some(status & 0x0040 != 0);
        p.source = "sbs";
    }
    if let Some(soc) = smb_read_word_le(&mut root, base, SBS_ADDR, 0x0D) {
        if soc <= 100 {
            p.charge_pct = Some(soc as u8);
            p.source = "sbs";
        }
    }
    // Negative current means the pack is draining, i.e. no AC.
    if let Some(cur) = smb_read_word_le(&mut root, base, SBS_ADDR, 0x0A) {
        let draining = (cur as i16) < 0;
        p.discharging = Some(p.discharging.unwrap_or(false) || draining);
        p.source = "sbs";
    }
    logln(format!(
        "power: portable={} battery={} charge={:?} discharging={:?} src={}",
        p.portable, p.battery_present, p.charge_pct, p.discharging, p.source
    ));
    p
}

/// Per-DIMM SPD/temperature/PMIC over SMBus (Intel i801 only).
struct SpdDimm {
    addr: u8,
    type_name: &'static str,
    temp_c: Option<f64>,
    pmic_addr: Option<u8>,
    pmic_r04: u8,
    pmic_r08: u8,
    pmic_fault: bool,
}

/// Locate the Intel SMBus controller at 00:1f.x, enable I/O + host, return SMB_BASE.
fn smbus_base(root: &mut ScopedProtocol<PciRootBridgeIo>) -> Option<u16> {
    for fun in 0u8..8 {
        let id = cfg_rd32(root, 0, 0x1f, fun, 0x00)?;
        if id & 0xFFFF != 0x8086 {
            continue;
        }
        let Some(cls) = cfg_rd32(root, 0, 0x1f, fun, 0x08) else { continue };
        if cls >> 8 != 0x0C0500 {
            continue;
        }
        if let Some(cmd) = cfg_rd32(root, 0, 0x1f, fun, 0x04) {
            cfg_wr16(root, 0, 0x1f, fun, 0x04, (cmd as u16) | 0x0001);
        }
        if let Some(hcfg) = cfg_rd32(root, 0, 0x1f, fun, 0x40) {
            cfg_wr8(root, 0, 0x1f, fun, 0x40, (hcfg as u8 | 0x01) & !0x04);
        }
        let bar = cfg_rd32(root, 0, 0x1f, fun, 0x20)?;
        if bar & 1 == 0 {
            continue;
        }
        return Some((bar & 0xFFFE) as u16);
    }
    None
}

fn collect_spd() -> Vec<SpdDimm> {
    let mut out = Vec::new();
    let Some(mut root) = rb_open() else {
        return out;
    };
    let Some(base) = smbus_base(&mut root) else {
        return out;
    };
    // Acquire INUSE (single read; the act of reading sets the semaphore).
    let _ = io_r(&mut root, base);
    for slot in 0u8..8 {
        let addr = 0x50 + slot;
        if smb_read_word(&mut root, base, addr, 0x00) == Some(0x5118) {
            let mut temp = None;
            if let Some(cap) = smb_read_byte(&mut root, base, addr, 0x05) {
                if cap & 0x02 != 0 {
                    if let Some(raw) = smb_read_word(&mut root, base, addr, 0x31) {
                        let v = (raw >> 2) & 0x7FF;
                        let v = if v & 0x400 != 0 { v as i32 - 0x800 } else { v as i32 };
                        temp = Some(v as f64 * 0.25);
                    }
                }
            }
            let paddr = 0x48 + slot;
            let (mut pmic_addr, mut r04, mut r08, mut fault) = (None, 0u8, 0u8, false);
            if smb_read_byte(&mut root, base, paddr, 0x3B).is_some() {
                r04 = smb_read_byte(&mut root, base, paddr, 0x04).unwrap_or(0);
                r08 = smb_read_byte(&mut root, base, paddr, 0x08).unwrap_or(0);
                pmic_addr = Some(paddr);
                fault = r04 & 0x70 != 0 || r08 & 0x6D != 0;
            }
            out.push(SpdDimm {
                addr,
                type_name: "DDR5",
                temp_c: temp,
                pmic_addr,
                pmic_r04: r04,
                pmic_r08: r08,
                pmic_fault: fault,
            });
            continue;
        }
        let Some(t) = smb_read_byte(&mut root, base, addr, 0x02) else { continue };
        let type_name = match t {
            0x0C => "DDR4",
            0x0B => "DDR3",
            0x10 => "LPDDR4",
            _ => "RAM",
        };
        let mut temp = None;
        if let Some(raw) = smb_read_word(&mut root, base, 0x18 + slot, 0x05) {
            let mag = (raw & 0x1FFF) as i32;
            let v = if mag & 0x1000 != 0 { mag - 0x2000 } else { mag };
            temp = Some(v as f64 * 0.0625);
        }
        out.push(SpdDimm {
            addr,
            type_name,
            temp_c: temp,
            pmic_addr: None,
            pmic_r04: 0,
            pmic_r08: 0,
            pmic_fault: false,
        });
    }
    // Release INUSE (bit6) and clear status.
    io_w(&mut root, base, 0xDE);
    out
}

/// Memory-controller base discovery + a bounded raw timing-window snapshot.
#[derive(Default)]
struct MchbarInfo {
    vendor: &'static str,
    present: bool,
    enabled: bool,
    base: u64,
    window: Vec<u32>,
}

fn collect_mchbar() -> MchbarInfo {
    let Some(mut root) = rb_open() else {
        return MchbarInfo::default();
    };
    let Some(id) = cfg_rd32(&mut root, 0, 0, 0, 0x00) else {
        return MchbarInfo::default();
    };
    if id & 0xFFFF == 0x8086 {
        let lo = cfg_rd32(&mut root, 0, 0, 0, 0x48).unwrap_or(0);
        let hi = cfg_rd32(&mut root, 0, 0, 0, 0x4C).unwrap_or(0);
        let enabled = lo & 1 != 0;
        let base = ((hi as u64) << 32) | (lo & 0xFFFF_FFFE) as u64;
        let mut window = Vec::new();
        if enabled && base != 0 {
            for i in 0..16u64 {
                window.push(mem_r32(&mut root, base + 0x4000 + i * 4).unwrap_or(0));
            }
        }
        return MchbarInfo { vendor: "intel", present: true, enabled, base, window };
    }
    if cfg_rd32(&mut root, 0, 0x18, 0, 0x00).is_some_and(|df| df & 0xFFFF == 0x1022) {
        return MchbarInfo { vendor: "amd", present: true, ..Default::default() };
    }
    MchbarInfo::default()
}

struct Win11 {
    ready: bool,
    checks: Vec<(&'static str, bool)>,
}

fn win11_readiness(info: &SysInfo) -> Win11 {
    const GIB: u64 = 1024 * 1024 * 1024;
    let checks = vec![
        ("TPM 2.0 present", info.tpm.is_some()),
        ("Secure Boot capable", info.secure_boot.is_some()),
        ("RAM >= 4 GB", info.mem_total >= 4 * GIB),
        (
            "Storage >= 64 GB",
            info.disks.iter().any(|d| !d.removable && d.capacity >= 64 * GIB),
        ),
        (
            "CPU >= 2 cores",
            info.dmi.cpu_cores >= 2 || info.dmi.cpu_threads >= 2,
        ),
    ];
    let ready = checks.iter().all(|(_, p)| *p);
    Win11 { ready, checks }
}

// ---------------------------------------------------------------------------
// Firmware / memory / display info
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SysInfo {
    fw_vendor: String,
    fw_revision: u32,
    uefi_revision: String,
    rtc: String,
    mem_total: u64,
    mem_usable: u64,
    mem_regions: usize,
    largest_conv: u64,
    config_tables: usize,
    acpi1: bool,
    acpi2: bool,
    smbios: bool,
    smbios3: bool,
    gop: String,
    dmi: Smbios,
    nics: Vec<Nic>,
    pci_net: Vec<PciNet>,
    disks: Vec<Disk>,
    nvme: Vec<NvmeDrive>,
    sata: Vec<smart::SataDrive>,
    volumes: Vec<volsig::VolumeSig>,
    gpus: Vec<Gpu>,
    secure_boot: Option<bool>,
    tpm: Option<String>,
    cpu_feats: Vec<&'static str>,
    msdm: bool,
    msdm_key: String,
    boot_entries: usize,
    pk_enrolled: bool,
    kek_enrolled: bool,
    setup_mode: Option<bool>,
    microcode: Option<u32>,
    pci_slots_total: usize,
    pci_slots_used: usize,
    pcie: Vec<PcieLink>,
    rtc_suspect: bool,
    bert: BertInfo,
    fpdt: FpdtInfo,
    cmos: CmosInfo,
    spd: Vec<SpdDimm>,
    mchbar: MchbarInfo,
    mca: Vec<stress::McaBank>,
    mem_errors: Vec<MemError>,
    hii: hii::HiiAudit,
    esrt: capsule::EsrtInfo,
    power: capsule::PowerState,
    bootdiag: bootdiag::BootDiag,
}

fn is_ram(ty: MemoryType) -> bool {
    matches!(
        ty,
        MemoryType::CONVENTIONAL
            | MemoryType::LOADER_CODE
            | MemoryType::LOADER_DATA
            | MemoryType::BOOT_SERVICES_CODE
            | MemoryType::BOOT_SERVICES_DATA
            | MemoryType::RUNTIME_SERVICES_CODE
            | MemoryType::RUNTIME_SERVICES_DATA
            | MemoryType::ACPI_RECLAIM
            | MemoryType::ACPI_NON_VOLATILE
            | MemoryType::PERSISTENT_MEMORY
    )
}

fn human_bytes(b: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;
    if b >= GIB {
        format!("{:.2} GiB", b as f64 / GIB as f64)
    } else {
        format!("{:.1} MiB", b as f64 / MIB as f64)
    }
}

impl SysInfo {
    fn collect() -> Self {
        let mut info = SysInfo::default();

        info.fw_vendor = format!("{}", uefi::system::firmware_vendor());
        info.fw_revision = uefi::system::firmware_revision();
        info.uefi_revision = format!("{}", uefi::system::uefi_revision());

        if let Ok(t) = uefi::runtime::get_time() {
            info.rtc = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                t.year(),
                t.month(),
                t.day(),
                t.hour(),
                t.minute(),
                t.second()
            );
            info.rtc_suspect = t.year() < 2024;
        } else {
            info.rtc = "unavailable".into();
            info.rtc_suspect = true;
        }

        if let Ok(mm) = uefi::boot::memory_map(MemoryType::LOADER_DATA) {
            for d in mm.entries() {
                let bytes = d.page_count * 4096;
                info.mem_regions += 1;
                if is_ram(d.ty) {
                    info.mem_total += bytes;
                }
                if d.ty == MemoryType::CONVENTIONAL {
                    info.mem_usable += bytes;
                    if bytes > info.largest_conv {
                        info.largest_conv = bytes;
                    }
                }
            }
        }

        uefi::system::with_config_table(|entries| {
            info.config_tables = entries.len();
            for e in entries {
                match e.guid {
                    ConfigTableEntry::ACPI2_GUID => info.acpi2 = true,
                    ConfigTableEntry::ACPI_GUID => info.acpi1 = true,
                    ConfigTableEntry::SMBIOS_GUID => info.smbios = true,
                    ConfigTableEntry::SMBIOS3_GUID => info.smbios3 = true,
                    _ => {}
                }
            }
        });

        // GetProtocol, never exclusive: an exclusive GOP open forces the
        // firmware to disconnect GraphicsConsoleDxe — the producer of the
        // text console this app renders to.
        info.gop = (|| {
            let h = uefi::boot::get_handle_for_protocol::<GraphicsOutput>().ok()?;
            let gop = unsafe {
                uefi::boot::open_protocol::<GraphicsOutput>(
                    OpenProtocolParams {
                        handle: h,
                        agent: uefi::boot::image_handle(),
                        controller: None,
                    },
                    OpenProtocolAttributes::GetProtocol,
                )
            }
            .ok()?;
            let (w, ht) = gop.current_mode_info().resolution();
            Some(format!("{w} x {ht}"))
        })()
        .unwrap_or_else(|| "none".into());

        info.dmi = collect_smbios();
        info.nics = collect_nics();
        info.pci_net = collect_pci_net();
        info.disks = collect_storage();
        info.nvme = collect_nvme();
        info.sata = smart::collect_sata();
        info.volumes = volsig::collect_volumes();
        info.gpus = collect_gpus();
        info.secure_boot = secure_boot();
        info.tpm = tpm_version();
        info.cpu_feats = cpu_features();
        let (msdm, key) = collect_msdm();
        info.msdm = msdm;
        info.msdm_key = key;
        info.boot_entries = boot_entry_count();
        info.pk_enrolled = global_var_present(uefi::cstr16!("PK"));
        info.kek_enrolled = global_var_present(uefi::cstr16!("KEK"));
        info.setup_mode = setup_mode();
        info.microcode = stress::cpu_microcode();
        let (slots_total, slots_used) = collect_slots();
        info.pci_slots_total = slots_total;
        info.pci_slots_used = slots_used;
        info.pcie = collect_pcie_links();
        info.bert = collect_bert();
        info.fpdt = collect_fpdt();
        info.cmos = collect_cmos();
        info.spd = collect_spd();
        info.mchbar = collect_mchbar();
        info.mca = stress::cpu_mca();
        info.mem_errors = collect_mem_errors();
        info.hii = hii::collect();
        info.esrt = capsule::collect();
        info.power = collect_power(&info.dmi);
        info.bootdiag = bootdiag::collect(&info);
        info
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn base_style() -> Style {
    Style::default().fg(palette::TEXT).bg(palette::BG)
}

fn panel(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette::BORDER).bg(palette::BG))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(palette::BG))
}

fn yn(b: bool) -> Span<'static> {
    if b {
        Span::styled("yes", Style::default().fg(palette::GOOD))
    } else {
        Span::styled("no", Style::default().fg(palette::BAD))
    }
}

fn kv(k: &str, v: impl Into<String>) -> Line<'static> {
    let v = v.into();
    let v = if v.is_empty() { "-".to_string() } else { v };
    Line::from(vec![
        Span::styled(format!("{k:<14}"), Style::default().fg(palette::LABEL)),
        Span::styled(v, Style::default().fg(palette::TEXT)),
    ])
}

fn header(s: &str) -> Line<'static> {
    Line::from(Span::styled(
        s.to_string(),
        Style::default().fg(palette::MUTED).add_modifier(Modifier::BOLD),
    ))
}

/// Truncate to `w` chars (trailing `~` when clipped) so table columns don't bleed.
fn fit(s: &str, w: usize) -> String {
    if s.chars().count() > w {
        s.chars().take(w.saturating_sub(1)).collect::<String>() + "~"
    } else {
        s.to_string()
    }
}

/// Placeholder for an empty field inside a composed value.
fn or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

/// First 12 hex chars of a digest, enough to read off a screen and compare.
fn sha_prefix(s: &str) -> String {
    if s.is_empty() {
        "no digest".to_string()
    } else {
        format!("sha256:{}", &s[..s.len().min(12)])
    }
}

fn para(lines: Vec<Line<'static>>, title: &str) -> Paragraph<'static> {
    Paragraph::new(lines)
        .style(base_style())
        .block(panel(title))
        .wrap(Wrap { trim: false })
}

fn page_overview(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    let d = &info.dmi;
    let mut sys = Vec::new();
    if d.present {
        sys.push(kv("Manufacturer", d.sys_mfr.clone()));
        sys.push(kv("Product", d.sys_product.clone()));
        sys.push(kv("Serial", d.sys_serial.clone()));
        sys.push(kv("Board", d.board_product.clone()));
    } else {
        sys.push(Line::from(Span::styled(
            "SMBIOS not available",
            Style::default().fg(palette::ERR),
        )));
    }
    frame.render_widget(para(sys, "System"), cols[0]);

    let mut cpu = Vec::new();
    if d.present {
        cpu.push(kv("CPU", d.cpu_version.clone()));
        cpu.push(kv("Cores/Thr", format!("{} / {}", d.cpu_cores, d.cpu_threads)));
    }
    cpu.push(kv("FW vendor", info.fw_vendor.clone()));
    cpu.push(kv("UEFI spec", info.uefi_revision.clone()));
    cpu.push(kv("RTC", info.rtc.clone()));
    frame.render_widget(para(cpu, "CPU & Firmware"), cols[1]);

    let mem = vec![
        kv("Total RAM", human_bytes(info.mem_total)),
        kv("Usable", human_bytes(info.mem_usable)),
        kv("DIMMs", format!("{}", d.dimms.len())),
        kv("Display", info.gop.clone()),
        kv("NICs", format!("{}", info.nics.len())),
    ];
    frame.render_widget(para(mem, "Memory & Display"), cols[2]);
}

fn page_system(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let d = &info.dmi;
    let mut lines = Vec::new();
    if d.present {
        lines.push(header("System"));
        lines.push(kv("Manufacturer", d.sys_mfr.clone()));
        lines.push(kv("Product", d.sys_product.clone()));
        lines.push(kv("Version", d.sys_version.clone()));
        lines.push(kv("Serial", d.sys_serial.clone()));
        lines.push(kv("UUID", d.sys_uuid.clone()));
        lines.push(kv("SKU", d.sys_sku.clone()));
        lines.push(kv("Family", d.sys_family.clone()));
        lines.push(Line::from(""));
        lines.push(header("Baseboard"));
        lines.push(kv("Manufacturer", d.board_mfr.clone()));
        lines.push(kv("Product", d.board_product.clone()));
        lines.push(kv("Version", d.board_version.clone()));
        lines.push(kv("Serial", d.board_serial.clone()));
        lines.push(kv("Asset tag", d.board_asset.clone()));
        lines.push(Line::from(""));
        lines.push(header("Processor"));
        lines.push(kv("Model", d.cpu_version.clone()));
        lines.push(kv("Socket", d.cpu_socket.clone()));
        lines.push(kv("Cores", format!("{}", d.cpu_cores)));
        lines.push(kv("Threads", format!("{}", d.cpu_threads)));
        lines.push(Line::from(""));
        lines.push(header("Graphics (PCI)"));
        if info.gpus.is_empty() {
            lines.push(Line::from(Span::styled(
                "no display controller found",
                Style::default().fg(palette::MUTED),
            )));
        } else {
            for g in &info.gpus {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", pci_vendor_name(g.vendor)),
                        Style::default().fg(palette::TEXT),
                    ),
                    Span::styled(
                        format!("[{:04x}:{:04x}]  ", g.vendor, g.device),
                        Style::default().fg(palette::MUTED),
                    ),
                    Span::styled(
                        if g.integrated { "integrated" } else { "discrete" },
                        Style::default().fg(palette::LABEL),
                    ),
                ]));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            "SMBIOS not available on this firmware",
            Style::default().fg(palette::ERR),
        )));
    }
    frame.render_widget(para(lines, "System / Baseboard / CPU"), area);
}

fn page_memory(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let mut lines = vec![
        kv("Total RAM", human_bytes(info.mem_total)),
        kv("Usable", human_bytes(info.mem_usable)),
        kv("Largest free", human_bytes(info.largest_conv)),
        kv("Map regions", format!("{}", info.mem_regions)),
        Line::from(""),
        header("Memory devices"),
    ];
    if info.dmi.dimms.is_empty() {
        lines.push(Line::from(Span::styled(
            "no populated memory devices reported",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "{:<18}{:<11}{:<11}{:<7}{:<7}{:<16}{:<18}{}",
                "Slot", "Size", "Speed", "Temp", "Type", "Manufacturer", "Part #", "Serial"
            ),
            Style::default().fg(palette::LABEL),
        )));
        let temps_aligned = info.spd.len() == info.dmi.dimms.len();
        for (i, m) in info.dmi.dimms.iter().enumerate() {
            let speed = if m.speed == 0 {
                "?".to_string()
            } else {
                format!("{} MT/s", m.speed)
            };
            let temp = if temps_aligned { info.spd[i].temp_c } else { None };
            let (temp_str, temp_col) = match temp {
                Some(t) if t >= 85.0 => (format!("{t:.0}C"), palette::ERR),
                Some(t) => (format!("{t:.0}C"), palette::GOOD),
                None => ("-".to_string(), palette::MUTED),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<18}", fit(&m.locator, 17)), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("{:<11}{:<11}", fit(&m.size, 10), speed), Style::default().fg(palette::TEXT)),
                Span::styled(format!("{temp_str:<7}"), Style::default().fg(temp_col)),
                Span::styled(
                    format!("{:<7}{:<16}{:<18}{}", fit(&m.mtype, 6), fit(&m.mfr, 15), fit(&m.part, 17), m.serial),
                    Style::default().fg(palette::TEXT),
                ),
            ]));
        }
    }
    frame.render_widget(para(lines, "Memory"), area);
}

fn page_firmware(frame: &mut Frame, area: Rect, app: &App) {
    let info = &app.info;
    let d = &info.dmi;
    let mut lines = vec![
        header("Firmware"),
        kv("UEFI vendor", info.fw_vendor.clone()),
        kv("UEFI spec", info.uefi_revision.clone()),
        kv("BIOS vendor", d.bios_vendor.clone()),
        kv("BIOS version", d.bios_version.clone()),
        kv("BIOS date", d.bios_date.clone()),
        kv("RTC clock", info.rtc.clone()),
        Line::from(""),
        header("Chassis & memory array"),
        kv("Chassis", d.chassis_type.clone()),
        kv("Max RAM", human_bytes(d.mem_max_bytes)),
        kv("DIMM slots", format!("{}", d.mem_slots)),
        kv("ECC", d.mem_ecc.clone()),
        kv("Display", info.gop.clone()),
        Line::from(""),
        header("Tables present"),
    ];
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "ACPI 2.0"), Style::default().fg(palette::LABEL)),
        yn(info.acpi2),
        Span::raw("    "),
        Span::styled(format!("{:<10}", "ACPI 1.0"), Style::default().fg(palette::LABEL)),
        yn(info.acpi1),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "SMBIOS 3.x"), Style::default().fg(palette::LABEL)),
        yn(info.smbios3),
        Span::raw("    "),
        Span::styled(format!("{:<10}", "SMBIOS"), Style::default().fg(palette::LABEL)),
        yn(info.smbios),
    ]));

    lines.push(Line::from(""));
    lines.push(header("Secure Boot & firmware variables"));
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "PK enrolled"), Style::default().fg(palette::LABEL)),
        yn(info.pk_enrolled),
        Span::raw("    "),
        Span::styled(format!("{:<10}", "KEK"), Style::default().fg(palette::LABEL)),
        yn(info.kek_enrolled),
    ]));
    lines.push(kv(
        "Setup mode",
        match info.setup_mode {
            Some(true) => "yes (no PK)".to_string(),
            Some(false) => "no (user mode)".to_string(),
            None => "unknown".to_string(),
        },
    ));
    lines.push(kv("Boot entries", format!("{}", info.boot_entries)));
    lines.push(kv(
        "Microcode",
        match info.microcode {
            Some(m) => format!("0x{m:08X}"),
            None => "n/a".to_string(),
        },
    ));
    lines.push(kv(
        "PCI slots",
        format!("{} total, {} in use", info.pci_slots_total, info.pci_slots_used),
    ));

    lines.push(Line::from(""));
    lines.push(header("Firmware update (ESRT / capsule)"));
    let esrt = &info.esrt;
    if esrt.present {
        if let Some(e) = esrt.system_entry() {
            lines.push(kv("BIOS fw version", format!("0x{:08x}", e.fw_version)));
            lines.push(kv("Lowest supported", format!("0x{:08x}", e.lowest_supported)));
            let sc = if e.last_attempt_status == 0 { palette::GOOD } else { palette::ERR };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<14}", "Last attempt"), Style::default().fg(palette::LABEL)),
                Span::styled(
                    format!("v0x{:08x} - {}", e.last_attempt_version, e.last_status_name()),
                    Style::default().fg(sc),
                ),
            ]));
        }
        lines.push(kv("ESRT resources", format!("{}", esrt.fw_resource_count)));
    } else {
        lines.push(kv("ESRT", "not present".to_string()));
    }
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "Capsule-on-disk"), Style::default().fg(palette::LABEL)),
        yn(esrt.capsule_on_disk),
    ]));

    // Power gate — a flash on a portable off AC is refused outright.
    let p = &info.power;
    let power_desc = match (p.portable, p.charge_pct, p.discharging) {
        (false, _, _) => format!("AC-only chassis ({})", p.source),
        (true, Some(c), Some(true)) => format!("portable, ON BATTERY {c}%"),
        (true, Some(c), _) => format!("portable, AC, {c}%"),
        (true, None, Some(true)) => "portable, ON BATTERY, charge unknown".to_string(),
        (true, None, _) => format!("portable, charge unknown ({})", p.source),
    };
    let power_ok = capsule::power_verdict(p);
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "Power"), Style::default().fg(palette::LABEL)),
        Span::styled(
            power_desc,
            Style::default().fg(if power_ok.is_ok() { palette::GOOD } else { palette::ERR }),
        ),
    ]));
    for b in &d.batteries {
        lines.push(kv(
            "Battery",
            format!(
                "{} {} @ {} ({} mV, {} mWh design)",
                b.manufacturer, b.device_name, b.location, b.design_voltage_mv, b.design_capacity_mwh
            ),
        ));
    }

    lines.push(Line::from(""));
    lines.push(header("Capsule flash"));
    let caret = if app.editing == EditField::Capsule { "_" } else { "" };
    lines.push(kv("Source", format!("{}{caret}", app.capsule_src)));
    if app.capsule_note.is_empty() {
        lines.push(kv("Loaded", "nothing - 'e' path, 'f' load".to_string()));
    } else {
        for n in &app.capsule_note {
            let c = if n.starts_with("BLOCKED") || n.starts_with("FAILED") || n.starts_with("load failed") || n.starts_with("malformed") {
                palette::ERR
            } else if n.starts_with("ok") {
                palette::GOOD
            } else {
                palette::TEXT
            };
            lines.push(Line::from(Span::styled(format!("  {n}"), Style::default().fg(c))));
        }
    }
    if app.fw_armed {
        lines.push(Line::from(Span::styled(
            "  ARMED - press F again to FLASH. Irreversible, no rollback.",
            Style::default().fg(palette::ERR),
        )));
    } else if !app.capsule_bytes.is_empty() {
        lines.push(Line::from(Span::styled(
            "  ready - press F to arm",
            Style::default().fg(palette::ACCENT),
        )));
    }

    lines.push(Line::from(""));
    lines.push(header("Hardware error logs (intermittent-fault triage)"));
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "RTC battery"), Style::default().fg(palette::LABEL)),
        if info.rtc_suspect {
            Span::styled("SUSPECT - implausible clock (check coin cell)", Style::default().fg(palette::ERR))
        } else {
            Span::styled("ok", Style::default().fg(palette::GOOD))
        },
    ]));
    if info.bert.present {
        let c = if info.bert.error_present { palette::ERR } else { palette::GOOD };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "ACPI BERT"), Style::default().fg(palette::LABEL)),
            Span::styled(
                if info.bert.error_present {
                    format!("{} ({} entries)", info.bert.severity, info.bert.entry_count)
                } else {
                    "no boot error recorded".to_string()
                },
                Style::default().fg(c),
            ),
        ]));
    } else {
        lines.push(kv("ACPI BERT", "not present".to_string()));
    }
    if info.mca.is_empty() {
        lines.push(kv("MCA banks", "no logged machine checks".to_string()));
    } else {
        for m in &info.mca {
            lines.push(Line::from(vec![
                Span::styled(format!("MCA bank {:<2} ", m.bank), Style::default().fg(palette::ERR)),
                Span::styled(
                    format!("status 0x{:016x}  addr 0x{:x}", m.status, m.addr),
                    Style::default().fg(palette::TEXT),
                ),
            ]));
        }
    }
    for e in &info.mem_errors {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "Mem error"), Style::default().fg(palette::ERR)),
            Span::styled(format!("{} @ 0x{:x}", e.kind, e.addr), Style::default().fg(palette::TEXT)),
        ]));
    }
    frame.render_widget(para(lines, "Firmware & Tables"), area);
}

/// BIOS Setup audit from the HII database: golden-config settings + coverage.
fn page_bios(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let a = &info.hii;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(40), Constraint::Min(0)])
        .split(area);

    let mut left = vec![
        header("HII database"),
        Line::from(vec![
            Span::styled(format!("{:<14}", "Exported"), Style::default().fg(palette::LABEL)),
            yn(a.available),
        ]),
        kv("Raw size", human_bytes(a.raw_bytes as u64)),
        kv("Package lists", format!("{}", a.package_lists)),
        kv("Forms packages", format!("{}", a.forms_packages)),
        kv("Form sets", format!("{}", a.formsets)),
        kv("Setup questions", format!("{}", a.questions_total)),
        kv("Golden flagged", format!("{}", a.settings.len())),
    ];
    if !a.note.is_empty() {
        left.push(Line::from(""));
        left.push(Line::from(Span::styled(
            fit(&a.note, 36),
            Style::default().fg(palette::MUTED),
        )));
    }
    left.push(Line::from(""));
    left.push(header("Reads via"));
    left.push(Line::from(Span::styled(
        "EFI_HII_DATABASE_PROTOCOL",
        Style::default().fg(palette::MUTED),
    )));
    left.push(Line::from(Span::styled(
        "values best-effort (EFI varstore)",
        Style::default().fg(palette::MUTED),
    )));
    frame.render_widget(para(left, "BIOS Audit"), cols[0]);

    let mut lines = vec![header("Golden-config settings")];
    if a.settings.is_empty() {
        lines.push(Line::from(Span::styled(
            "no watched settings resolved",
            Style::default().fg(palette::MUTED),
        )));
    }
    for s in &a.settings {
        let cat = s.category.unwrap_or("");
        let cur = s.current.as_deref().unwrap_or("(unread)");
        let cur_color = match s.current.as_deref() {
            Some(v) if v.eq_ignore_ascii_case("enabled") => palette::GOOD,
            Some(v) if v.eq_ignore_ascii_case("disabled") => palette::ERR,
            Some(_) => palette::TEXT,
            None => palette::MUTED,
        };
        lines.push(Line::from(vec![
            Span::styled(format!("[{cat}] "), Style::default().fg(palette::LABEL)),
            Span::styled(fit(&s.name, 34), Style::default().fg(palette::TEXT)),
            Span::styled(" = ", Style::default().fg(palette::MUTED)),
            Span::styled(cur.to_string(), Style::default().fg(cur_color)),
        ]));
        if !s.options.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("      {} / {}", s.kind.label(), s.options.join(" | ")),
                Style::default().fg(palette::MUTED),
            )));
        }
    }
    frame.render_widget(para(lines, "Setup Settings"), cols[1]);
}

/// Boot Doctor: why won't Windows boot — the boot chain, checked pre-OS.
fn page_boot(frame: &mut Frame, area: Rect, app: &App) {
    use bootdiag::Severity;
    let info = &app.info;
    let d = &info.bootdiag;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(40), Constraint::Min(0)])
        .split(area);

    let esp_line = |label: &str, ok: bool| {
        Line::from(vec![
            Span::styled(format!("{label:<16}"), Style::default().fg(palette::LABEL)),
            if ok {
                Span::styled("yes", Style::default().fg(palette::GOOD))
            } else {
                Span::styled("NO", Style::default().fg(palette::ERR))
            },
        ])
    };
    let mut left = vec![
        header("Boot chain"),
        esp_line("ESP readable", d.esp_found),
        esp_line("bootmgfw.efi", d.bootmgfw_present),
        esp_line("BCD store", d.bcd_present),
        esp_line("fallback boot", d.fallback_present),
    ];
    if d.bootmgfw_present {
        left.push(kv("bootmgfw size", human_bytes(d.bootmgfw_size)));
    }
    if let Some(pe) = &d.bootmgfw_pe {
        let ok = pecheck::is_valid(pe);
        left.push(Line::from(vec![
            Span::styled("PE structure    ", Style::default().fg(palette::LABEL)),
            Span::styled(
                pecheck::verdict_label(pe),
                Style::default().fg(if ok { palette::GOOD } else { palette::ERR }),
            ),
        ]));
    }
    left.push(Line::from(""));
    left.push(header("Windows boot entry"));
    match d.windows_entry {
        Some(n) => {
            left.push(kv("Entry", format!("Boot{n:04X}")));
            left.push(esp_line("in BootOrder", d.windows_in_boot_order));
            left.push(esp_line("active", d.windows_entry_active));
        }
        None => left.push(Line::from(Span::styled(
            "none found",
            Style::default().fg(palette::ERR),
        ))),
    }
    left.push(Line::from(""));
    left.push(header("GPT partitions"));
    left.push(kv("ESP", format!("{}", d.part_esp)));
    left.push(kv("MSR", format!("{}", d.part_msr)));
    left.push(kv("Windows data", format!("{}", d.part_win_data)));
    left.push(kv("Recovery", format!("{}", d.part_recovery)));
    frame.render_widget(para(left, "Boot Doctor"), cols[0]);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(cols[1]);

    let roll = bootdiag::overall(d);
    let (roll_tag, roll_color) = match roll {
        Severity::Ok => ("OK", palette::GOOD),
        Severity::Warn => ("WARN", palette::WARN),
        Severity::Fail => ("FAIL", palette::ERR),
    };
    let mut vlines = vec![
        header("Verdict"),
        Line::from(vec![
            Span::styled("overall  ", Style::default().fg(palette::LABEL)),
            Span::styled(
                roll_tag,
                Style::default().fg(roll_color).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    // Display order only; the JSON array keeps push order.
    let mut ranked: Vec<&(Severity, String)> = d.verdicts.iter().collect();
    ranked.sort_by(|a, b| b.0.rank().cmp(&a.0.rank()));
    for (sev, msg) in ranked {
        let (tag, color) = match sev {
            Severity::Ok => ("OK  ", palette::GOOD),
            Severity::Warn => ("WARN", palette::WARN),
            Severity::Fail => ("FAIL", palette::ERR),
        };
        vlines.push(Line::from(Span::styled(
            format!("[{tag}]"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        for chunk in wrap_text(msg, 72) {
            vlines.push(Line::from(Span::styled(
                format!("  {chunk}"),
                Style::default().fg(palette::TEXT),
            )));
        }
    }
    frame.render_widget(para(vlines, "Diagnosis"), rows[0]);

    let mut elines = vec![header("Boot entries (firmware order)")];
    if d.boot_entries.is_empty() {
        elines.push(Line::from(Span::styled(
            "no boot entries parsed",
            Style::default().fg(palette::MUTED),
        )));
    }
    for (i, e) in d.boot_entries.iter().enumerate() {
        let color = if e.is_windows { palette::GOOD } else { palette::TEXT };
        let sel = i == app.boot_sel.min(d.boot_entries.len().saturating_sub(1));
        elines.push(Line::from(vec![
            Span::styled(
                if sel { "> " } else { "  " },
                Style::default().fg(palette::ACCENT),
            ),
            Span::styled(format!("Boot{:04X} ", e.num), Style::default().fg(palette::LABEL)),
            Span::styled(fit(&e.description, 38), Style::default().fg(color)),
            Span::styled(
                if e.active { "" } else { " [off]" },
                Style::default().fg(palette::ERR),
            ),
            Span::styled(
                if e.in_boot_order { "" } else { " [not in BootOrder]" },
                Style::default().fg(palette::WARN),
            ),
        ]));
    }
    frame.render_widget(para(elines, "Entries"), rows[1]);
}

/// This machine's side of the laptop/desktop split, from SMBIOS chassis type.
fn machine_side(app: &App) -> bioslove::Side {
    if app.info.dmi.is_portable() {
        bioslove::Side::Laptop
    } else {
        bioslove::Side::Desktop
    }
}

/// Rows the Flash tab lists: auto-detected matches while the search box is
/// empty, search hits across both sides once anything is typed.
fn flash_rows(app: &App) -> Vec<(usize, Option<bioslove::Confidence>)> {
    let Some(index) = &app.flash_index else {
        return Vec::new();
    };
    let needle = app.flash_filter.trim();
    if needle.is_empty() {
        if !app.flash_matches.is_empty() {
            return app
                .flash_matches
                .iter()
                .map(|m| (m.entry, Some(m.confidence)))
                .collect();
        }
        return bioslove::search(index, Some(machine_side(app)), "")
            .into_iter()
            .map(|i| (i, None))
            .collect();
    }
    bioslove::search(index, None, needle)
        .into_iter()
        .map(|i| (i, None))
        .collect()
}

/// Re-run the SMBIOS match against the loaded index.
fn redetect_bioslove(app: &mut App) {
    let Some(index) = &app.flash_index else {
        return;
    };
    app.flash_matches = bioslove::match_machine(index, &app.info.dmi);
    app.flash_sel = 0;
    app.dirty = true;
}

/// Read the BIOSLove index off an attached volume and auto-detect this machine.
fn load_bioslove_index(app: &mut App) {
    match bioslove::load_from_volume() {
        Ok(index) => {
            logln(format!(
                "bioslove: index {} entries ({} laptop, {} desktop) generated {}",
                index.entries.len(),
                index.laptop_count(),
                index.desktop_count(),
                index.generated_at
            ));
            app.flash_err.clear();
            app.flash_index = Some(index);
            redetect_bioslove(app);
            let n = app.flash_matches.len();
            app.status = match app.flash_matches.first() {
                Some(m) => {
                    let e = &app.flash_index.as_ref().unwrap().entries[m.entry];
                    format!("detected {} ({}) - {n} candidate(s)", e.folder, m.confidence.label())
                }
                None => "index loaded - no SMBIOS match, press 'e' to search".to_string(),
            };
        }
        Err(e) => {
            logln(format!("bioslove: {e}"));
            app.flash_err = e;
            app.flash_index = None;
            app.flash_matches.clear();
            app.status = "no BIOSLove index found - see the Flash tab".into();
        }
    }
}

/// Drop a prepared step; the selection or the index moved under it.
fn clear_flash_prep(app: &mut App) {
    app.flash_ready = None;
    app.flash_note.clear();
    app.flash_blocked = false;
    app.flash_armed = false;
}

/// Trailing path component of a backslash-separated volume path.
fn base_name(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

/// Read the step under the cursor, verify every byte it will touch against the
/// index, and run the pre-flight gates. Loads no image and starts nothing.
fn prepare_flash_step(app: &mut App) {
    clear_flash_prep(app);
    let rows = flash_rows(app);
    let Some((ei, _)) = rows.get(app.flash_sel).copied() else {
        app.status = "no model selected".into();
        return;
    };

    let mut note: Vec<String> = Vec::new();
    let mut blocked = false;
    let mut ready = None;

    {
        let Some(index) = app.flash_index.as_ref() else {
            return;
        };
        let entry = &index.entries[ei];

        if !entry.lane.launchable() {
            note.push(format!("BLOCKED {} is {}", entry.folder, entry.lane.label()));
            note.push(match entry.lane {
                bioslove::Lane::InBiosOnly => {
                    "  flash from the board's own updater (EZ Flash / M-Flash / Instant Flash)".into()
                }
                bioslove::Lane::DosOnly => "  real-mode DOS flasher; needs the legacy BIOSLove boot".into(),
                bioslove::Lane::WindowsOnly => "  vendor tool runs under Windows, not firmware".into(),
                _ => "  no firmware-launchable tool in this folder".into(),
            });
            app.flash_note = note;
            app.flash_blocked = true;
            app.dirty = true;
            return;
        }

        let Some(step) = entry.steps.get(app.flash_step) else {
            note.push("BLOCKED no such step in this recipe".to_string());
            app.flash_note = note;
            app.flash_blocked = true;
            app.dirty = true;
            return;
        };

        note.push(format!(
            "step {} of {} - {} via {}",
            step.index,
            entry.steps.len(),
            step.kind.label(),
            base_name(&step.exec)
        ));

        if !step.resolved {
            note.push("BLOCKED index says this step's payloads are missing".to_string());
            blocked = true;
        }

        match capsule::power_verdict(&app.info.power) {
            Ok(m) => note.push(format!("ok  power: {m}")),
            Err(e) => {
                note.push(format!("BLOCKED power: {e}"));
                blocked = true;
            }
        }

        // Auto-detect confirms the board; an operator-picked row does not.
        match app.flash_matches.iter().find(|m| m.entry == ei) {
            Some(m) if m.confidence == bioslove::Confidence::Exact => {
                note.push(format!("ok  board: {}", m.evidence()))
            }
            Some(m) => note.push(format!("WARN board: {} - confirm by hand", m.evidence())),
            None => note.push("WARN board: operator-selected, SMBIOS did not match".to_string()),
        }

        if !blocked {
            let exec_path = index.file_path(entry, &step.exec);
            match bioslove::read_file_any_volume(&exec_path, bioslove::PAYLOAD_MAX_BYTES) {
                Err(e) => {
                    note.push(format!("BLOCKED tool: {e}"));
                    blocked = true;
                }
                Ok((bytes, volume)) => {
                    let dry = launch::dry_run(&bytes);
                    if !step.exec_sha256.is_empty() && dry.sha256 != step.exec_sha256 {
                        note.push(format!(
                            "BLOCKED tool digest {} does not match the index",
                            sha_prefix(&dry.sha256)
                        ));
                        blocked = true;
                    } else if step.exec_sha256.is_empty() {
                        note.push("WARN tool has no digest in the index".to_string());
                    } else {
                        note.push(format!("ok  tool digest {}", sha_prefix(&dry.sha256)));
                    }
                    if dry.ok {
                        note.push(format!("ok  tool is a valid EFI application ({} B)", dry.bytes));
                    } else {
                        note.push(format!("BLOCKED tool {}: {}", dry.verdict, dry.detail));
                        blocked = true;
                    }

                    for f in &step.files {
                        let path = index.file_path(entry, &f.name);
                        match bioslove::read_file_on_volume(
                            volume,
                            &path,
                            bioslove::PAYLOAD_MAX_BYTES,
                        ) {
                            Err(e) => {
                                note.push(format!("BLOCKED payload {}: {e}", f.name));
                                blocked = true;
                            }
                            Ok(p) => {
                                let got = capsule::sha256_hex(&p);
                                if f.sha256.is_empty() {
                                    note.push(format!("WARN payload {} has no digest", f.name));
                                } else if got != f.sha256 {
                                    note.push(format!(
                                        "BLOCKED payload {} digest {} does not match the index",
                                        f.name,
                                        sha_prefix(&got)
                                    ));
                                    blocked = true;
                                } else {
                                    note.push(format!(
                                        "ok  payload {} {} B {}",
                                        f.name,
                                        p.len(),
                                        sha_prefix(&got)
                                    ));
                                }
                            }
                        }
                    }

                    if !blocked {
                        let cmd = format!("{} {}", base_name(&step.exec), step.args);
                        note.push(format!("ready to run: {}", cmd.trim()));
                        note.push(match step.after {
                            bioslove::After::Returns => {
                                "expect: tool returns here when it finishes".to_string()
                            }
                            bioslove::After::Reboot | bioslove::After::Shutdown => format!(
                                "expect: machine {} - boot back here and pick step {}",
                                step.after.label(),
                                step.index + 1
                            ),
                            bioslove::After::Unknown => {
                                "expect: unknown - the script does not say".to_string()
                            }
                        });
                        ready = Some(PreparedStep {
                            entry: ei,
                            step: app.flash_step,
                            volume,
                            exec_path,
                            command_line: cmd.trim().to_string(),
                            bytes,
                        });
                    }
                }
            }
        }
    }

    app.status = if blocked {
        "pre-flight refused this step; see the Flash tab".into()
    } else {
        "verified - press F twice to run".into()
    };
    app.flash_note = note;
    app.flash_blocked = blocked;
    app.flash_ready = ready;
    app.dirty = true;
}

/// Hand the console to the prepared vendor tool and start it.
fn do_flash_step(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    let Some(p) = app.flash_ready.take() else {
        app.status = "nothing prepared - press ENTER first".into();
        return Ok(());
    };
    // The row set can move under a prepared step (a changed search term), so the
    // selection must still be the entry that was verified.
    let selected = flash_rows(app).get(app.flash_sel).map(|(ei, _)| *ei);
    if selected != Some(p.entry) {
        clear_flash_prep(app);
        app.status = "selection moved since verification - press ENTER again".into();
        return Ok(());
    }

    app.status = format!("RUNNING {} - DO NOT POWER OFF", p.command_line);
    terminal.draw(|frame| render(frame, app))?;
    logln(format!(
        "bioslove: starting {} from {}",
        p.command_line, p.exec_path
    ));

    let guard = launch::save_console();
    let mut dp_buf: Vec<u8> = Vec::new();
    let cpath = uefi::CString16::try_from(p.exec_path.as_str()).ok();
    let dp = cpath
        .as_ref()
        .and_then(|c| launch::file_device_path(p.volume, c, &mut dp_buf).ok());
    if dp.is_none() {
        logln("bioslove: no device path for the child; it may not find its payload".to_string());
    }
    let outcome = launch::run(&p.bytes, &p.command_line, dp);
    guard.restore();
    terminal.clear()?;
    app.dirty = true;

    match outcome {
        Err(e) => {
            logln(format!("bioslove: launch failed: {e}"));
            app.flash_note.push(format!("FAILED {e}"));
            app.status = format!("launch failed: {e}");
        }
        Ok(ran) => {
            logln(format!("bioslove: {} returned {:?}", p.command_line, ran.status));
            app.flash_note
                .push(format!("returned {:?} from {}", ran.status, p.command_line));
            if ran.status == uefi::Status::SUCCESS {
                // The recipe advances only on a clean return.
                app.flash_step = p.step.saturating_add(1);
                app.status = format!("step {} done - {:?}", p.step + 1, ran.status);
            } else {
                app.status = format!("step {} returned {:?}", p.step + 1, ran.status);
            }
        }
    }
    Ok(())
}

/// BIOSLove model index: what this machine is, and what firmware is on hand.
fn page_flash(frame: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(0)])
        .split(area);

    let d = &app.info.dmi;
    let mut left = vec![
        header("This machine"),
        kv("Baseboard", d.board_product.clone()),
        kv("System", d.sys_product.clone()),
        kv("Family", d.sys_family.clone()),
        kv("Chassis", format!("{} ({})", d.chassis_type, machine_side(app).label())),
        kv("BIOS", format!("{} {}", d.bios_version, d.bios_date)),
        Line::from(""),
        header("Index"),
    ];
    match &app.flash_index {
        Some(index) => {
            left.push(kv(
                "Entries",
                format!("{} laptop / {} desktop", index.laptop_count(), index.desktop_count()),
            ));
            left.push(kv("Built", index.generated_at.clone()));
            left.push(kv("Source", index.source.clone()));
        }
        None => {
            left.push(Line::from(Span::styled(
                if app.flash_err.is_empty() {
                    "not loaded - press 'f'".to_string()
                } else {
                    app.flash_err.clone()
                },
                Style::default().fg(if app.flash_err.is_empty() {
                    palette::MUTED
                } else {
                    palette::ERR
                }),
            )));
        }
    }
    left.push(Line::from(""));
    left.push(header("Search"));
    left.push(kv(
        "Model",
        format!(
            "{}{}",
            app.flash_filter,
            if app.editing == EditField::Flash { "_" } else { "" }
        ),
    ));
    left.push(Line::from(Span::styled(
        if app.flash_filter.trim().is_empty() {
            "showing auto-detected matches"
        } else {
            "showing search hits (both sides)"
        },
        Style::default().fg(palette::MUTED),
    )));
    frame.render_widget(para(left, "Flash"), cols[0]);

    let rows = flash_rows(app);
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Min(0)])
        .split(cols[1]);

    let mut list = Vec::new();
    if app.flash_index.is_none() {
        list.push(Line::from(Span::styled(
            format!("press 'f' to read {}", bioslove::INDEX_PATH),
            Style::default().fg(palette::MUTED),
        )));
    } else if rows.is_empty() {
        list.push(Line::from(Span::styled(
            "no match - press 'e' and type a model or chassis code",
            Style::default().fg(palette::WARN),
        )));
    }
    let index = app.flash_index.as_ref();
    // Keep the highlighted row on screen without a scrollbar widget.
    let height = split[0].height.saturating_sub(2) as usize;
    let first = app.flash_sel.saturating_sub(height.saturating_sub(1));
    for (n, (ei, conf)) in rows.iter().enumerate().skip(first).take(height) {
        let Some(e) = index.and_then(|i| i.entries.get(*ei)) else {
            continue;
        };
        let sel = n == app.flash_sel;
        let mark = if sel { ">" } else { " " };
        let badge = match conf {
            Some(c) => format!("[{}]", c.label()),
            None => String::new(),
        };
        let ver = if e.versions.bios.is_empty() {
            String::new()
        } else {
            format!("BIOS {}", e.versions.bios)
        };
        let style = if sel {
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)
        } else if !e.reachable {
            Style::default().fg(palette::BAD)
        } else {
            Style::default().fg(palette::TEXT)
        };
        list.push(Line::from(vec![
            Span::styled(format!("{mark} {:<16}", e.folder), style),
            Span::styled(
                format!("{:<9}", e.side.label()),
                Style::default().fg(palette::MUTED),
            ),
            Span::styled(
                format!("{:<14}", e.lane.label()),
                Style::default().fg(if e.lane.launchable() {
                    palette::GOOD
                } else {
                    palette::MUTED
                }),
            ),
            Span::styled(format!("{ver:<16}"), Style::default().fg(palette::TEXT)),
            Span::styled(badge, Style::default().fg(palette::ACCENT)),
        ]));
    }
    let title = format!("Models ({} shown)", rows.len());
    frame.render_widget(para(list, &title), split[0]);

    let mut detail = Vec::new();
    match rows.get(app.flash_sel).and_then(|(ei, _)| index.map(|i| (&i.entries[*ei], *ei))) {
        None => detail.push(Line::from(Span::styled(
            "no entry selected",
            Style::default().fg(palette::MUTED),
        ))),
        Some((e, ei)) => {
            detail.push(header(&format!("{} ({})", e.folder, e.side.label())));
            if let Some(m) = app.flash_matches.iter().find(|m| m.entry == ei) {
                detail.push(kv("Matched", m.evidence()));
            }
            if !e.modelstring.is_empty() {
                detail.push(kv("Covers", e.modelstring.clone()));
            }
            if !e.aliases.is_empty() {
                detail.push(kv("Chassis", e.aliases.join(", ")));
            }
            detail.push(kv(
                "Versions",
                format!(
                    "BIOS {} / EC {} / ME {}",
                    or_dash(&e.versions.bios),
                    or_dash(&e.versions.ec),
                    or_dash(&e.versions.me)
                ),
            ));
            detail.push(Line::from(vec![
                Span::styled(format!("{:<14}", "Delivery"), Style::default().fg(palette::LABEL)),
                Span::styled(
                    e.lane.label().to_string(),
                    Style::default().fg(if e.lane.launchable() {
                        palette::GOOD
                    } else {
                        palette::WARN
                    }),
                ),
            ]));
            detail.push(Line::from(vec![
                Span::styled(format!("{:<14}", "Payloads"), Style::default().fg(palette::LABEL)),
                if e.reachable {
                    Span::styled("all resolved", Style::default().fg(palette::GOOD))
                } else {
                    Span::styled(
                        format!("{} of {} steps resolved", e.resolved_steps(), e.steps.len()),
                        Style::default().fg(palette::ERR),
                    )
                },
            ]));
            if !e.steps.is_empty() {
                detail.push(Line::from(""));
                detail.push(header("Sequence"));
            }
            for (n, s) in e.steps.iter().enumerate() {
                let at = n == app.flash_step;
                let c = if !s.resolved {
                    palette::ERR
                } else if at {
                    palette::ACCENT
                } else {
                    palette::TEXT
                };
                detail.push(Line::from(Span::styled(
                    format!(
                        "{} {}. {:<5} {} {}  [{}]",
                        if at { ">" } else { " " },
                        s.index,
                        s.kind.label(),
                        s.exec,
                        s.args,
                        s.after.label()
                    ),
                    Style::default().fg(c),
                )));
                if !s.exec_sha256.is_empty() {
                    detail.push(Line::from(Span::styled(
                        format!("     tool  {}", sha_prefix(&s.exec_sha256)),
                        Style::default().fg(palette::MUTED),
                    )));
                }
                for f in &s.files {
                    detail.push(Line::from(Span::styled(
                        format!(
                            "     {:<24} {:>9} B  {}",
                            f.name,
                            f.size,
                            sha_prefix(&f.sha256)
                        ),
                        Style::default().fg(if s.resolved { palette::MUTED } else { palette::ERR }),
                    )));
                }
                if !s.note.is_empty() {
                    detail.push(Line::from(Span::styled(
                        format!("     {}", s.note),
                        Style::default().fg(palette::WARN),
                    )));
                }
            }
            for w in &e.warnings {
                detail.push(Line::from(Span::styled(
                    format!("  ! {w}"),
                    Style::default().fg(palette::WARN),
                )));
            }
            detail.push(Line::from(""));
            detail.push(header("Pre-flight"));
            if app.flash_note.is_empty() {
                detail.push(Line::from(Span::styled(
                    "  ENTER verifies the selected step ([ ] move the cursor)",
                    Style::default().fg(palette::MUTED),
                )));
            }
            for n in &app.flash_note {
                let c = if n.starts_with("BLOCKED") || n.starts_with("FAILED") {
                    palette::ERR
                } else if n.starts_with("WARN") {
                    palette::WARN
                } else if n.starts_with("ok") || n.starts_with("ready") {
                    palette::GOOD
                } else {
                    palette::TEXT
                };
                detail.push(Line::from(Span::styled(
                    format!("  {n}"),
                    Style::default().fg(c),
                )));
            }
            if app.flash_armed {
                detail.push(Line::from(Span::styled(
                    "  ARMED - press F again to run the vendor tool. It writes firmware.",
                    Style::default().fg(palette::ERR),
                )));
            } else if app.flash_ready.is_some() {
                detail.push(Line::from(Span::styled(
                    "  verified - press F to arm",
                    Style::default().fg(palette::ACCENT),
                )));
            }
        }
    }
    frame.render_widget(para(detail, "Selected"), split[1]);
}

/// WASM diagnostic plugins run in-firmware via the wasmi interpreter.
fn page_plugins(frame: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(40), Constraint::Min(0)])
        .split(area);

    let left = vec![
        header("WASM runtime"),
        kv("Interpreter", "wasmi (no JIT)".to_string()),
        kv("ABI", "wasm32-wasip1 + env".to_string()),
        kv("Embedded", format!("{} B demo", DEMO_PLUGIN.len())),
        Line::from(""),
        header("Registry"),
        kv(
            "Plugin id",
            format!(
                "{}{}",
                app.plugin_id_in,
                if app.editing == EditField::Plugin { "_" } else { "" }
            ),
        ),
        kv(
            "Fetched",
            if app.plugin_wasm.is_empty() {
                "-".to_string()
            } else {
                format!("{} B", app.plugin_wasm.len())
            },
        ),
        Line::from(""),
        header("Run"),
        Line::from(vec![
            Span::styled("e ", Style::default().fg(palette::ACCENT)),
            Span::styled("edit plugin id", Style::default().fg(palette::MUTED)),
        ]),
        Line::from(vec![
            Span::styled("f ", Style::default().fg(palette::ACCENT)),
            Span::styled("fetch from target", Style::default().fg(palette::MUTED)),
        ]),
        Line::from(vec![
            Span::styled("ENTER ", Style::default().fg(palette::ACCENT)),
            Span::styled("run fetched (or demo)", Style::default().fg(palette::MUTED)),
        ]),
        Line::from(Span::styled(
            "remote: agent op run_plugin",
            Style::default().fg(palette::MUTED),
        )),
        Line::from(Span::styled(
            "{url, tool, args}",
            Style::default().fg(palette::MUTED),
        )),
    ];
    frame.render_widget(para(left, "Plugins"), cols[0]);

    let mut lines = vec![header("Last plugin run")];
    if app.plugin_out.is_empty() {
        lines.push(Line::from(Span::styled(
            "no run yet - press ENTER",
            Style::default().fg(palette::MUTED),
        )));
    }
    for l in &app.plugin_out {
        lines.push(Line::from(Span::styled(l.clone(), Style::default().fg(palette::TEXT))));
    }
    frame.render_widget(para(lines, "Output"), cols[1]);
}

/// Firmware capability bits + pre-serialized diagnostic JSON for the `host_fw_*`
/// read ABI, so plugins query cached data without live re-collection or borrows.
fn fw_data(info: &SysInfo) -> wasmrt::FwData {
    use wasmrt::caps;
    let mut fw = wasmrt::FwData::default();
    #[cfg(target_arch = "x86_64")]
    {
        fw.caps |= caps::MSR;
    }
    fw.caps |= caps::VARIABLES;
    if !info.nvme.is_empty() {
        fw.caps |= caps::NVME;
    }
    if let Some(mut root) = rb_open() {
        fw.caps |= caps::PCI;
        if smbus_base(&mut root).is_some() {
            fw.caps |= caps::SMBUS;
        }
    }
    fw.push(
        "bert",
        serde_json::json!({
            "present": info.bert.present,
            "error": info.bert.error_present,
            "severity": info.bert.severity,
            "entries": info.bert.entry_count,
        })
        .to_string(),
    );
    fw.push(
        "fpdt",
        serde_json::json!({
            "present": info.fpdt.present,
            "fw_boot_ms": info.fpdt.fw_boot_ms,
            "os_loader_ms": info.fpdt.os_loader_ms,
        })
        .to_string(),
    );
    fw.push(
        "msdm",
        serde_json::json!({ "present": info.msdm, "key": info.msdm_key }).to_string(),
    );
    fw.push(
        "smbios",
        serde_json::json!({
            "sys_mfr": info.dmi.sys_mfr,
            "sys_product": info.dmi.sys_product,
            "sys_serial": info.dmi.sys_serial,
            "board_product": info.dmi.board_product,
            "bios_vendor": info.dmi.bios_vendor,
            "bios_version": info.dmi.bios_version,
            "bios_date": info.dmi.bios_date,
            "cpu": info.dmi.cpu_version,
        })
        .to_string(),
    );
    fw.push("esrt", capsule::esrt_json(&info.esrt, &info.power).to_string());
    fw.push("bootdiag", bootdiag::diag_json(&info.bootdiag).to_string());
    fw.push("sata", smart::sata_json_string(&info.sata));
    fw.push(
        "volume_signatures",
        volsig::volumes_json(&info.volumes).to_string(),
    );
    fw.push("bios_settings", hii::audit_json(&info.hii).to_string());
    fw
}

/// GET queued remote-viewer input from the relay and inject it into the loop.
/// Returns true if any events were injected (drives the adaptive poll rate).
fn poll_preboot_input(app: &mut App) -> bool {
    // A linked console delivers input as FRAME_TAG_PREBOOT_INPUT; this would duplicate it.
    if app.direct.is_some() || app.target.is_empty() {
        return false;
    }
    let serial = order::encode_path_segment(&effective_serial(&app.info));
    let path = format!("/api/v1/qc/preboot/{serial}/input");
    let mut got = false;
    if let Ok((200, body)) = http_get_json(&app.target, &path) {
        for eb in tcp_protocol::preboot::split_event_batch(&body) {
            if let Some(ev) = tcp_protocol::preboot::decode_event(&eb) {
                if let Some(t) = stream::event_to_terminput(&ev) {
                    app.injected.push(t);
                    got = true;
                }
            }
        }
    }
    got
}

/// Register once (full fingerprint), then heartbeat, so a networked box appears
/// and stays fresh in the admin console without operator action.
fn presence_beat(app: &mut App) {
    // Retry registration each interval until the fingerprint upload lands, then
    // just heartbeat. The heartbeat itself upserts the connected_client row, so
    // the box shows in the roster even if the full fingerprint hasn't yet.
    if !app.present_registered {
        match upload_fingerprint(app) {
            Ok(_) => {
                app.present_registered = true;
                app.status = relay_line(app);
            }
            Err(e) => app.status = format!("presence: register failed: {e}"),
        }
    }
    send_presence_heartbeat(app);
}

/// One-line relay summary: the target, where it came from, and whether this
/// machine has registered with it.
fn relay_line(app: &App) -> String {
    if app.target.is_empty() {
        return "relay: none - listening for a console beacon ('e' sets one by hand)".to_string();
    }
    let presence = if !app.present {
        "presence off"
    } else if app.present_registered {
        "presence registered"
    } else {
        "presence pending"
    };
    format!("relay: {} [{}] - {presence}", app.target, app.target_src.label())
}

/// Relay url assumed for a v1 beacon: the console that sent it is the
/// Mastertech host, on axum's default port.
fn derived_relay(addr: &str) -> Option<String> {
    let ip = netraw::parse_ipv4(addr)?;
    Some(format!("http://{}:{AXUM_DEFAULT_PORT}", netraw::ip_str(ip)))
}

/// Take a discovered relay url as the target unless a stronger source already
/// set it. Re-registers presence against the new relay.
fn adopt_relay(app: &mut App, url: &str, src: TargetSource) {
    if !tcp_protocol::preboot::is_valid_relay_url(url) {
        return;
    }
    if !app.target.is_empty() && app.target_src > src {
        return;
    }
    // Same url from a stronger source: record the provenance, change nothing else.
    if app.target == url {
        app.target_src = src;
        return;
    }
    let prev = core::mem::replace(&mut app.target, url.to_string());
    let prev_src = core::mem::replace(&mut app.target_src, src);
    relay_gate::reset();
    app.present_registered = false;
    app.present_next_ms = 0;
    logln(format!(
        "relay: adopted {url} ({}) - was '{prev}' ({})",
        src.label(),
        prev_src.label()
    ));
    app.status = relay_line(app);
}

/// This machine's IPv4 subnet mask, from the raw SNP lease or the UEFI stack.
fn local_mask(app: &App) -> Option<[u8; 4]> {
    if let Some(rn) = app.raw_net {
        if rn.mask != [0; 4] {
            return Some(rn.mask);
        }
    }
    app.ifaces.iter().filter_map(|i| netraw::parse_ipv4(&i.mask)).find(|m| *m != [0; 4])
}

/// True when `relay`'s host is the beaconing console's own IPv4, or an IPv4 on
/// the same subnet as it.
fn relay_on_beacon_subnet(app: &App, relay: &str, addr: &str) -> bool {
    let host = relay
        .strip_prefix("http://")
        .or_else(|| relay.strip_prefix("https://"))
        .unwrap_or(relay);
    let host = host.split(['/', '?', '#']).next().unwrap_or(host);
    let (Some(r), Some(c)) = (netraw::parse_ipv4(host), netraw::parse_ipv4(addr)) else {
        return false;
    };
    if r == c {
        return true;
    }
    match local_mask(app) {
        Some(m) => (0..4).all(|i| (r[i] & m[i]) == (c[i] & m[i])),
        None => false,
    }
}

/// Hold an off-subnet advertised relay for operator confirmation instead of
/// adopting it: uploads to it carry serial, SMBIOS and OA3 key material.
fn offer_relay(app: &mut App, url: &str) {
    if app.target == url || app.pending_relay.as_deref() == Some(url) {
        return;
    }
    app.pending_relay = Some(url.to_string());
    logln(format!("relay: off-subnet relay {url} offered - 'y' accepts"));
    app.status = format!("beacon offers off-subnet relay {url} - 'y' accepts");
}

/// Settle the relay target from a beacon: the url a v2 beacon advertises when
/// it names the beacon's own subnet, else the axum default on the beacon's own
/// IPv4. An off-subnet advertised url waits for the operator.
fn adopt_beacon_relay(app: &mut App, b: &tcp_protocol::preboot::Beacon) {
    if let Some(url) = b.relay.as_deref() {
        if relay_on_beacon_subnet(app, url, &b.addr) {
            adopt_relay(app, url, TargetSource::Advertised);
            return;
        }
        if let Some(fallback) = derived_relay(&b.addr) {
            adopt_relay(app, &fallback, TargetSource::Derived);
        }
        offer_relay(app, url);
        return;
    }
    if let Some(url) = derived_relay(&b.addr) {
        adopt_relay(app, &url, TargetSource::Derived);
    }
}

/// Listen for a console beacon and take the relay it names, without dialing —
/// works before a DHCP lease exists. True when a beacon was heard.
fn listen_for_relay(app: &mut App, listen_ms: u64) -> bool {
    match netraw::discover_console(tcp_protocol::preboot::DISCOVERY_PORT, listen_ms) {
        Some(b) => {
            adopt_beacon_relay(app, &b);
            true
        }
        None => false,
    }
}

/// Find a console direct-link endpoint: a LAN UDP beacon first (relay-free),
/// else the relay's console registry over HTTP. A beacon also settles the relay
/// target — advertised by a v2 beacon, or derived from a v1 beacon's IPv4.
/// Returns `ip[:port]` and whether a beacon was heard.
fn discover_console_addr(app: &mut App, listen_ms: u64) -> (Option<String>, bool) {
    if let Some(b) = netraw::discover_console(tcp_protocol::preboot::DISCOVERY_PORT, listen_ms) {
        adopt_beacon_relay(app, &b);
        return (Some(b.addr), true);
    }
    (relay_console_addr(app), false)
}

/// Ask the relay's console registry for a console endpoint over HTTP.
fn relay_console_addr(app: &App) -> Option<String> {
    if app.target.is_empty() {
        return None;
    }
    let (code, body) = http_get_json(&app.target, "/api/v1/qc/preboot/console").ok()?;
    if code != 200 {
        return None;
    }
    let v = serde_json::from_slice::<serde_json::Value>(&body).ok()?;
    v.get("consoles")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("addr"))
        .and_then(|a| a.as_str())
        .map(|s| s.to_string())
}

/// Discover a console (UDP beacon or relay) and dial it. On success, sends
/// HELLO(serial) so the console registers a session, then keeps the socket in
/// `app.direct`. Any failure leaves the relay path untouched. Returns whether a
/// beacon was heard.
fn direct_discover(app: &mut App, listen_ms: u64) -> bool {
    if app.direct.is_some() {
        return false;
    }
    // Prefer a LAN UDP beacon (no relay round-trip); fall back to asking the
    // relay for a console endpoint over HTTP.
    let (addr, heard) = discover_console_addr(app, listen_ms);
    let Some(mut addr) = addr else { return heard };
    if !addr.contains(':') {
        addr = format!("{addr}:{DIRECT_PORT}");
    }
    let serial = effective_serial(&app.info);
    if serial.is_empty() {
        return heard;
    }
    match net_tcp::DirectLink::connect(&addr) {
        Ok(mut link) => {
            match link.send(tcp_protocol::preboot::FRAME_TAG_PREBOOT_HELLO, serial.as_bytes()) {
                Ok(()) => {
                    app.direct = Some(link);
                    app.direct_stream = false;
                    app.stream_throttle = stream::Throttle::new();
                    app.status = format!("direct link -> {addr}");
                    logln(format!("direct: linked to console {addr} as {serial}"));
                }
                Err(e) => logln(format!("direct: HELLO failed: {e}")),
            }
        }
        Err(e) => logln(format!("direct: dial {addr} failed: {e}")),
    }
    heard
}

/// One immediate discovery pass for the moment the network comes up ('c'/'d'),
/// so the relay is learned now instead of at the next backoff deadline. Re-arms
/// the backoff at its base interval, never tighter. Returns whether a beacon
/// was heard.
fn kick_discovery(app: &mut App, clock: &Clock) -> bool {
    if app.direct.is_some() {
        return false;
    }
    let heard = direct_discover(app, DISCOVER_LISTEN_KICK_MS);
    let now = clock.now_ms();
    app.discover_backoff_ms = DIRECT_DISCOVER_MS;
    if app.direct.is_some() {
        app.direct_pump_next_ms = now;
    } else {
        app.discover_next_ms = now.saturating_add(DIRECT_DISCOVER_MS);
    }
    heard
}

/// Short names of the `connect_all` steps, in run order.
const CONNECT_STEPS: [&str; 5] = ["drivers", "lease", "console", "presence", "agent"];

/// This machine's IPv4, from the firmware stack lease or the raw SNP lease.
fn local_ip(app: &App) -> Option<String> {
    if let Some(f) = app.ifaces.iter().find(|f| f.ip != "0.0.0.0") {
        return Some(f.ip.clone());
    }
    app.raw_net
        .filter(|rn| rn.ip != [0; 4])
        .map(|rn| netraw::ip_str(rn.ip))
}

/// This machine's IPv4 subnet mask as a string.
fn local_mask_str(app: &App) -> Option<String> {
    if let Some(f) = app.ifaces.iter().find(|f| f.ip != "0.0.0.0") {
        return Some(f.mask.clone());
    }
    app.raw_net
        .filter(|rn| rn.mask != [0; 4])
        .map(|rn| netraw::ip_str(rn.mask))
}

/// Binds drivers, leases, finds the console, arms presence and enables
/// auto-poll in one pass. Draws before each step, records its pass/fail, and
/// skips steps that are already satisfied.
fn connect_all(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
    clock: &Clock,
) -> Result<()> {
    app.connect_steps.clear();
    // The progress line and the step table only render on the Network page.
    app.tab = TAB_NETWORK;

    app.status = "connect 1/5: binding NIC drivers...".into();
    terminal.draw(|frame| render(frame, app))?;
    connect_network_stack();
    app.info.nics = collect_nics();
    app.info.pci_net = collect_pci_net();
    app.invalidate_fw();
    let nics = app.info.nics.len();
    app.connect_steps.push(if nics == 0 {
        (false, "no UEFI NIC driver".to_string())
    } else {
        (true, format!("{nics} bound"))
    });

    if local_ip(app).is_some() {
        app.connect_steps.push((true, "already held".to_string()));
    } else {
        app.status = "connect 2/5: DHCP - up to 30s per interface...".into();
        terminal.draw(|frame| render(frame, app))?;
        let (ifaces, raw, note) = run_dhcp();
        app.ifaces = ifaces;
        app.raw_net = raw;
        app.connect_steps.push(match local_ip(app) {
            Some(ip) => (true, ip),
            None => (false, fit(&note, 21)),
        });
    }
    let leased = local_ip(app).is_some();

    if app.direct.is_some() {
        app.connect_steps.push((true, "already linked".to_string()));
    } else {
        app.status = "connect 3/5: listening for a console beacon...".into();
        terminal.draw(|frame| render(frame, app))?;
        let heard = if leased {
            kick_discovery(app, clock)
        } else {
            listen_for_relay(app, DISCOVER_LISTEN_KICK_MS)
        };
        app.connect_steps.push(if app.direct.is_some() {
            (true, "direct link".to_string())
        } else if heard {
            (true, "beacon heard".to_string())
        } else if app.target_src == TargetSource::Operator {
            (true, "relay preset".to_string())
        } else {
            (false, "no beacon heard".to_string())
        });
    }

    // Presence and the agent are skipped without a lease or a direct link.
    let networked = leased || app.direct.is_some();
    let now = clock.now_ms();
    if !networked {
        app.connect_steps.push((false, "no network".to_string()));
        app.connect_steps.push((false, "no network".to_string()));
    } else {
        if app.present {
            app.reset_discovery(now);
            app.connect_steps.push((true, "already on".to_string()));
        } else {
            app.arm_presence(now, false);
            app.connect_steps.push((true, "armed".to_string()));
        }
        let was_agent = app.agent;
        app.agent = true;
        app.agent_next_ms = clock.now_ms();
        app.connect_steps
            .push((true, if was_agent { "already on".into() } else { "auto-poll on".into() }));
    }

    let ok = app.connect_steps.iter().filter(|(ok, _)| *ok).count();
    app.status = format!("connect: {ok}/5 ok - {}", relay_line(app));
    app.dirty = true;
    Ok(())
}

/// Drain any frames the console sent over the direct socket: input events (into
/// the injected queue), stream control, and plugin-run requests. Drops the link
/// on a transport error so discovery can re-establish it. Returns true when the
/// link produced anything, including a drop.
fn direct_pump(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<bool> {
    use tcp_protocol::preboot;
    let mut handled = false;
    for _ in 0..8 {
        let polled = match app.direct.as_mut() {
            Some(link) => link.poll_frame(4),
            None => return Ok(handled),
        };
        match polled {
            Ok(Some((tag, body))) => {
                handled = true;
                match tag {
                    t if t == preboot::FRAME_TAG_PREBOOT_INPUT => {
                        if let Some(ev) = preboot::decode_event(&body) {
                            if let Some(te) = stream::event_to_terminput(&ev) {
                                app.injected.push(te);
                            }
                        }
                    }
                    t if t == preboot::FRAME_TAG_PREBOOT_STREAM_CTL => {
                        if let Some(ctl) = preboot::decode_stream_ctl(&body) {
                            app.direct_stream = ctl.stream;
                            app.stream_throttle = stream::Throttle::new();
                            logln(format!("direct: stream_ctl -> {}", ctl.stream));
                        }
                    }
                    t if t == tcp_protocol::FRAME_TAG_PING => {
                        if let Some(link) = app.direct.as_mut() {
                            if let Err(e) = link.send(tcp_protocol::FRAME_TAG_PONG, &[]) {
                                logln(format!("direct: pong failed: {e}"));
                                app.direct = None;
                                app.direct_stream = false;
                                return Ok(true);
                            }
                        }
                    }
                    t if t == tcp_protocol::FRAME_TAG_PONG => {}
                    t if t == preboot::FRAME_TAG_PREBOOT_PLUGIN_RUN => {
                        if let Some(req) = preboot::decode_plugin_run(&body) {
                            let res = run_plugin_direct(app, &req, terminal);
                            if let Some(link) = app.direct.as_mut() {
                                let out = preboot::encode_plugin_result(&res);
                                if let Err(e) =
                                    link.send(preboot::FRAME_TAG_PREBOOT_PLUGIN_RESULT, &out)
                                {
                                    logln(format!("direct: plugin result send failed: {e}"));
                                    app.direct = None;
                                    app.direct_stream = false;
                                    return Ok(true);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(e) => {
                logln(format!("direct: link dropped: {e}"));
                app.direct = None;
                app.direct_stream = false;
                handled = true;
                break;
            }
        }
    }
    Ok(handled)
}

/// Run a plugin for a direct-link request: registry id or URL fetched over the
/// relay target, else the embedded demo. Returns a wire result either way.
fn run_plugin_direct(
    app: &mut App,
    req: &tcp_protocol::preboot::PbPluginRun,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> tcp_protocol::preboot::PbPluginResult {
    use tcp_protocol::preboot::PbPluginResult;
    let host = {
        let s = effective_serial(&app.info);
        if s.is_empty() { "uefi".to_string() } else { s }
    };
    let src = req.source.trim();
    let bytes = if src.is_empty() {
        DEMO_PLUGIN.to_vec()
    } else if src.starts_with("http://") || src.starts_with("https://") || src.starts_with('/') {
        match http_get_url(&app.target, src) {
            Ok((200, b)) => b,
            Ok((code, _)) => {
                return PbPluginResult { error: format!("fetch HTTP {code}"), ..Default::default() };
            }
            Err(e) => return PbPluginResult { error: format!("fetch: {e}"), ..Default::default() },
        }
    } else {
        let path = format!("/api/v1/plugins/{}/wasm", order::encode_path_segment(src));
        match http_get_json(&app.target, &path) {
            Ok((200, b)) => b,
            Ok((code, _)) => {
                return PbPluginResult { error: format!("fetch HTTP {code}"), ..Default::default() };
            }
            Err(e) => return PbPluginResult { error: format!("fetch: {e}"), ..Default::default() },
        }
    };
    app.status = format!("direct: running plugin ({} B)", bytes.len());
    let _ = terminal.draw(|frame| render(frame, app));
    let req_tool = req.tool.trim().to_string();
    let args = if req.args.trim().is_empty() { "{}" } else { req.args.trim() };
    let call = if req_tool.is_empty() {
        wasmrt::Call::FirstTool(args)
    } else {
        wasmrt::Call::Tool(&req_tool, args)
    };
    let fw = app.fw();
    match wasmrt::run(&bytes, &host, call, fw) {
        Ok(run) => {
            app.status = format!("direct: plugin {} ok", run.id);
            PbPluginResult {
                ok: true,
                id: run.id,
                name: run.name,
                version: run.version,
                tools: run.tools,
                tool: run.tool,
                result: run.result,
                log: run.log,
                stdout: run.stdout,
                error: String::new(),
            }
        }
        Err(e) => PbPluginResult { tool: req_tool, error: e, ..Default::default() },
    }
}

/// Tiny POST that bumps the connected_client:qc_<serial> row's last_update.
fn send_presence_heartbeat(app: &mut App) {
    if app.target.is_empty() {
        return;
    }
    let serial = order::encode_path_segment(&effective_serial(&app.info));
    if serial.is_empty() {
        return;
    }
    let path = format!("/api/v1/qc/preboot/{serial}/alive");
    if let Err(e) = http_post_json(&app.target, &path, b"{}") {
        app.status = format!("presence: {e}");
    }
}

/// Poll the relay's viewer flag: start streaming when an admin viewer is
/// waiting, stop after two consecutive misses. A relay error counts as a miss
/// so an auto-stream can't outlive a dead relay; manual 'v' state is honored
/// until the viewer that saw it leaves.
fn viewer_check(app: &mut App) {
    // A linked console drives streaming over FRAME_TAG_PREBOOT_STREAM_CTL instead.
    if app.direct.is_some() || app.target.is_empty() {
        return;
    }
    let serial = order::encode_path_segment(&effective_serial(&app.info));
    if serial.is_empty() {
        return;
    }
    let path = format!("/api/v1/qc/preboot/{serial}/viewer");
    let waiting = match http_get_json(&app.target, &path) {
        Ok((200, body)) => {
            core::str::from_utf8(&body).map(|s| s.contains("true")).unwrap_or(false)
        }
        _ => false,
    };
    if waiting {
        app.viewer_miss = 0;
        if !app.streaming && !app.stream_manual_off {
            app.streaming = true;
            app.stream_auto = true;
            app.stream_frame = 0;
            app.stream_throttle = stream::Throttle::new();
            app.status = "admin viewer connected - streaming TUI".into();
            logln("stream: auto-started (viewer waiting)".into());
        }
    } else {
        // Viewer gone: lift a manual stop so the next viewer can auto-start.
        app.stream_manual_off = false;
        if app.streaming && app.stream_auto {
            app.viewer_miss = app.viewer_miss.saturating_add(1);
            if app.viewer_miss >= 2 {
                app.streaming = false;
                app.stream_auto = false;
                app.viewer_miss = 0;
                app.status = "admin viewer left - streaming off".into();
                logln("stream: auto-stopped (no viewer)".into());
            }
        }
    }
}

/// POST the in-memory log ring to the relay for `curl`-friendly retrieval at
/// GET /api/v1/qc/preboot/<serial>/logs.
fn upload_logs(app: &mut App) {
    if app.target.is_empty() {
        app.status = "logs: set a target first ('e')".into();
        return;
    }
    let serial = order::encode_path_segment(&effective_serial(&app.info));
    if serial.is_empty() {
        app.status = "logs: no usable serial".into();
        return;
    }
    let body = log_snapshot().join("\n");
    let path = format!("/api/v1/qc/preboot/{serial}/logs");
    app.status = match http_post_json(&app.target, &path, body.as_bytes()) {
        Ok(_) => format!("logs: uploaded {}B - curl .../qc/preboot/{serial}/logs", body.len()),
        Err(e) => format!("logs: upload failed: {e}"),
    };
}

/// GET `/api/v1/plugins/{id}/wasm` from the upload target and stash the bytes;
/// introspection output (id/version/tools) lands in `plugin_out`.
fn fetch_registry_plugin(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    if app.target.is_empty() {
        app.status = "set a target first ('e' elsewhere)".into();
        return Ok(());
    }
    let id = app.plugin_id_in.trim().to_string();
    if id.is_empty() {
        app.status = "set a plugin id first ('e')".into();
        return Ok(());
    }
    app.status = format!("fetching {id} ...");
    terminal.draw(|frame| render(frame, app))?;
    let path = format!("/api/v1/plugins/{}/wasm", order::encode_path_segment(&id));
    match http_get_json(&app.target, &path) {
        Ok((200, bytes)) => {
            if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
                app.status = format!("fetch: not wasm ({} B)", bytes.len());
                return Ok(());
            }
            app.set_plugin_wasm(bytes);
            // Introspect without invoking: id/name/version/tools into the pane.
            let host = {
                let s = effective_serial(&app.info);
                if s.is_empty() { "uefi".to_string() } else { s }
            };
            let fw = app.fw();
            match wasmrt::run(&app.plugin_wasm, &host, wasmrt::Call::Load, fw) {
                Ok(run) => {
                    app.plugin_out.clear();
                    app.plugin_out.push(format!("fetched: {} B", app.plugin_wasm.len()));
                    app.plugin_out.push(format!("id: {}", run.id));
                    app.plugin_out.push(format!("name: {} v{}", run.name, run.version));
                    for chunk in wrap_text(&format!("tools: {}", run.tools), 60) {
                        app.plugin_out.push(chunk);
                    }
                    app.plugin_tools = run.tools;
                    app.status = format!("{id} fetched - ENTER runs its first tool");
                }
                Err(e) => {
                    app.plugin_out.clear();
                    app.plugin_out.push(format!("fetched {} B, load failed: {e}", app.plugin_wasm.len()));
                    app.status = format!("{id}: instantiate failed: {e}");
                }
            }
        }
        Ok((code, body)) => {
            let snippet = String::from_utf8_lossy(&body[..body.len().min(120)]).into_owned();
            app.plugin_out.clear();
            app.plugin_out.push(format!("fetch '{id}' -> HTTP {code}"));
            if code == 404 {
                app.plugin_out.push("route/plugin not found.".into());
                app.plugin_out.push("redeploy axum (the".into());
                app.plugin_out.push("/api/v1/plugins/{id}/wasm".into());
                app.plugin_out.push("route is new) + check the id.".into());
            }
            for chunk in wrap_text(&snippet, 56) {
                app.plugin_out.push(chunk);
            }
            app.status = format!("fetch '{id}': HTTP {code} (see Output)");
        }
        Err(e) => {
            app.plugin_out.clear();
            app.plugin_out.push(format!("fetch '{id}' failed:"));
            for chunk in wrap_text(&e, 56) {
                app.plugin_out.push(chunk);
            }
            app.status = format!("fetch '{id}' failed (see Output)");
        }
    }
    Ok(())
}

/// Run the fetched registry plugin's first advertised tool with empty args.
fn run_fetched_plugin(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    let bytes = app.plugin_wasm.clone();
    let tool = wasmrt::first_tool_name(&app.plugin_tools);
    app.status = match &tool {
        Some(t) => format!("running {t} ..."),
        None => "running plugin ...".to_string(),
    };
    terminal.draw(|frame| render(frame, app))?;
    if let Some(tools) = run_wasm_plugin(app, &bytes, tool.as_deref(), "{}") {
        app.plugin_tools = tools;
    }
    Ok(())
}

/// Load and invoke a plugin (its first advertised tool when `tool` is `None`),
/// formatting the run into `app.plugin_out`. Returns its tool descriptors.
fn run_wasm_plugin(app: &mut App, bytes: &[u8], tool: Option<&str>, args: &str) -> Option<String> {
    let host = {
        let s = effective_serial(&app.info);
        if s.is_empty() { "uefi".to_string() } else { s }
    };
    let fw = app.fw();
    let call = match tool {
        Some(t) => wasmrt::Call::Tool(t, args),
        None => wasmrt::Call::FirstTool(args),
    };
    app.plugin_out.clear();
    match wasmrt::run(bytes, &host, call, fw) {
        Ok(run) => {
            app.plugin_out.push(format!("id: {}", run.id));
            app.plugin_out.push(format!("name: {} v{}", run.name, run.version));
            for chunk in wrap_text(&format!("tools: {}", run.tools), 60) {
                app.plugin_out.push(chunk);
            }
            app.plugin_out.push(format!("tool: {}", run.tool));
            for chunk in wrap_text(&format!("result: {}", run.result), 60) {
                app.plugin_out.push(chunk);
            }
            if !run.stdout.is_empty() {
                for chunk in wrap_text(&format!("stdout: {}", run.stdout), 60) {
                    app.plugin_out.push(chunk);
                }
            }
            for l in run.log.iter().take(6) {
                app.plugin_out.push(format!("log: {l}"));
            }
            app.status = format!("plugin {} ran ok", run.id);
            Some(run.tools)
        }
        Err(e) => {
            app.plugin_out.push(format!("error: {e}"));
            app.status = format!("plugin failed: {e}");
            None
        }
    }
}

/// Wrap a string into <=width chunks for the fixed-width output pane.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    s.as_bytes()
        .chunks(width.max(1))
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect()
}

/// Label/value row sized for the Network tab's left column.
fn nrow(k: &str, v: &str, w: usize, c: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<9}"), Style::default().fg(palette::LABEL)),
        Span::styled(fit(v, w.saturating_sub(9)), Style::default().fg(c)),
    ])
}

/// Two key hints on one row of the Network tab's right column.
fn nkeys(k1: &str, d1: &str, k2: &str, d2: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k1} "), Style::default().fg(palette::ACCENT)),
        Span::styled(format!("{d1:<14}"), Style::default().fg(palette::MUTED)),
        Span::styled(format!("{k2} "), Style::default().fg(palette::ACCENT)),
        Span::styled(d2.to_string(), Style::default().fg(palette::MUTED)),
    ])
}

/// What each `connect_all` step will do, shown before the first run.
const CONNECT_PLAN: [&str; 5] = [
    "bind NIC drivers",
    "get a DHCP lease",
    "find the console",
    "appear in console",
    "poll for commands",
];

fn page_network(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(4)])
        .split(area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(0)])
        .split(rows[0]);
    let lw = cols[0].width.saturating_sub(2) as usize;
    let rw = cols[1].width.saturating_sub(2) as usize;
    let body_h = rows[0].height.saturating_sub(2) as usize;

    let info = &app.info;
    let mut l: Vec<Line> = Vec::new();

    l.push(header("UEFI NIC drivers"));
    if info.nics.is_empty() {
        l.push(Line::from(Span::styled(
            fit("none bound", lw),
            Style::default().fg(palette::ERR),
        )));
        if !info.pci_net.is_empty() {
            // The NIC is on the bus, so the driver is either unconnected or absent.
            l.push(Line::from(Span::styled(
                fit("NIC on PCI but no driver connected - press 'c'", lw),
                Style::default().fg(palette::ERR),
            )));
            l.push(Line::from(Span::styled(
                fit("still none? enable Network Stack in BIOS", lw),
                Style::default().fg(palette::WARN),
            )));
        }
    } else {
        let detail = info.nics.len() == 1;
        for (i, n) in info.nics.iter().take(2).enumerate() {
            let (link, c) = if !n.media_supported {
                ("link n/a", palette::MUTED)
            } else if n.media_present {
                ("link up", palette::GOOD)
            } else {
                ("link down", palette::ERR)
            };
            l.push(Line::from(vec![
                Span::styled(format!("NIC {i} "), Style::default().fg(palette::ACCENT)),
                Span::styled(
                    format!("{:<18}", fit(&n.mac, 17)),
                    Style::default().fg(palette::TEXT),
                ),
                Span::styled(link, Style::default().fg(c)),
            ]));
            if detail {
                l.push(Line::from(Span::styled(
                    fit(
                        &format!("      type {}  MTU {}  {}", n.if_type, n.mtu, n.state),
                        lw,
                    ),
                    Style::default().fg(palette::MUTED),
                )));
            }
        }
    }

    l.push(header("Network hardware (PCI)"));
    if info.pci_net.is_empty() {
        l.push(Line::from(Span::styled(
            "none found",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for p in info.pci_net.iter().take(2) {
            l.push(Line::from(vec![
                Span::styled(
                    format!("{:<8}", fit(&p.loc, 8)),
                    Style::default().fg(palette::ACCENT),
                ),
                Span::styled(
                    fit(
                        &format!(
                            "{} [{:04x}:{:04x}] {}",
                            pci_vendor_name(p.vendor),
                            p.vendor,
                            p.device,
                            p.kind
                        ),
                        lw.saturating_sub(8),
                    ),
                    Style::default().fg(palette::TEXT),
                ),
            ]));
        }
    }

    l.push(header("IPv4"));
    match local_ip(app) {
        Some(ip) => {
            let cidr = match local_mask_str(app).as_deref().and_then(netraw::parse_ipv4) {
                Some(m) => format!("{ip}/{}", m.iter().map(|b| b.count_ones()).sum::<u32>()),
                None => ip,
            };
            l.push(nrow("address", &cidr, lw, palette::GOOD));
        }
        None => l.push(nrow("address", "no lease", lw, palette::MUTED)),
    }

    l.push(header("Relay (upload target)"));
    l.push(nrow(
        "presence",
        if !app.present {
            "off - 'n' arms it"
        } else if app.present_registered {
            "registered with the relay"
        } else {
            "armed, not registered yet"
        },
        lw,
        if app.present_registered {
            palette::GOOD
        } else if app.present {
            palette::WARN
        } else {
            palette::MUTED
        },
    ));
    if app.editing == EditField::Target {
        l.push(Line::from(Span::styled(
            fit("[EDITING - type host:port, ENTER saves]", lw),
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        )));
    }
    if let Some(url) = app.pending_relay.as_deref() {
        l.push(Line::from(Span::styled(
            fit(&format!("OFFER {url} - 'y' accepts"), lw),
            Style::default().fg(palette::WARN),
        )));
    }
    if app.target.is_empty() {
        l.push(nrow("target", "none - 'e' sets one", lw, palette::ERR));
    } else {
        let u = parse_upload_url(&app.target);
        l.push(nrow("target", &u.full, lw, palette::TEXT));
        l.push(nrow(
            "source",
            app.target_src.label(),
            lw,
            if app.target_src == TargetSource::Default {
                palette::WARN
            } else {
                palette::GOOD
            },
        ));
        l.push(nrow(
            "sends by",
            if u.is_qc_tcp {
                "QC TCP (framed)"
            } else if u.needs_efi_http {
                "EFI HTTP (DNS+TLS)"
            } else {
                "raw TCP4 (plain http)"
            },
            lw,
            palette::LABEL,
        ));
    }
    l.truncate(body_h);

    let mut r: Vec<Line> = Vec::new();
    for (i, name) in CONNECT_STEPS.iter().enumerate() {
        match app.connect_steps.get(i) {
            Some((ok, detail)) => r.push(Line::from(vec![
                Span::styled(
                    format!("{} {:<9}", i + 1, name),
                    Style::default().fg(palette::TEXT),
                ),
                Span::styled(
                    if *ok { "ok " } else { "-- " },
                    Style::default().fg(if *ok { palette::GOOD } else { palette::ERR }),
                ),
                Span::styled(
                    fit(detail, rw.saturating_sub(14)),
                    Style::default().fg(palette::MUTED),
                ),
            ])),
            None => r.push(Line::from(vec![
                Span::styled(
                    format!("{} {:<9}", i + 1, name),
                    Style::default().fg(palette::TEXT),
                ),
                Span::styled(
                    fit(CONNECT_PLAN.get(i).copied().unwrap_or(""), rw.saturating_sub(11)),
                    Style::default().fg(palette::MUTED),
                ),
            ])),
        }
    }
    r.push(header("Single steps"));
    r.push(nkeys("c", "bind drivers", "d", "lease+presence"));
    r.push(nkeys("n", "presence", "e", "relay url"));
    r.push(nkeys("a", "poll once", "A", "auto-poll"));
    r.push(nkeys("p", "upload now", "y", "accept offer"));
    r.push(nkeys("v", "relay stream", "u", "send logs"));
    r.truncate(body_h);

    frame.render_widget(
        Paragraph::new(l).style(base_style()).block(panel("Connection")),
        cols[0],
    );
    frame.render_widget(
        Paragraph::new(r)
            .style(base_style())
            .block(panel("Connect - Shift+C runs 1-5")),
        cols[1],
    );

    let title = format!("Status - {}", fit(&mode_line(app), 55));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            app.status.clone(),
            Style::default().fg(palette::GOOD),
        )))
        .style(base_style())
        .wrap(Wrap { trim: false })
        .block(panel(&title)),
        rows[1],
    );
}

/// Failing attribute rows rendered per drive before the pane runs out of lines.
const SATA_ATTR_ROWS: usize = 2;
/// Columns left for the status text on a drive line at an 80-column console.
const SATA_STATUS_COLS: usize = 24;

fn page_storage(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let mut lines = vec![header("Block devices (whole disks)")];
    if info.disks.is_empty() {
        lines.push(Line::from(Span::styled(
            "no block devices reported",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for (i, d) in info.disks.iter().enumerate() {
            let kind = if d.removable { "removable" } else { "fixed" };
            lines.push(Line::from(vec![
                Span::styled(format!("disk {i}  "), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("{:<12}", human_bytes(d.capacity)), Style::default().fg(palette::TEXT)),
                Span::styled(format!("{:<11}", drive_type(d.bus)), Style::default().fg(palette::GOOD)),
                Span::styled(format!("{:<6}", d.bus), Style::default().fg(palette::LABEL)),
                Span::styled(kind, Style::default().fg(palette::MUTED)),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(header("NVMe controllers (IDENTIFY)"));
    if info.nvme.is_empty() {
        lines.push(Line::from(Span::styled(
            "none detected",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for d in &info.nvme {
            lines.push(Line::from(vec![
                Span::styled(format!("{:<24}", d.model), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("SN {:<22}", d.serial), Style::default().fg(palette::TEXT)),
                Span::styled(format!("fw {}", d.firmware), Style::default().fg(palette::MUTED)),
            ]));
            if let Some(s) = &d.smart {
                let warn_color = if s.critical_warning != 0 { palette::ERR } else { palette::GOOD };
                lines.push(Line::from(vec![
                    Span::styled("  SMART  ", Style::default().fg(palette::LABEL)),
                    Span::styled(format!("{}C  ", s.temp_c), Style::default().fg(palette::TEXT)),
                    Span::styled(format!("used {}%  ", s.percentage_used), Style::default().fg(palette::TEXT)),
                    Span::styled(format!("spare {}%  ", s.available_spare), Style::default().fg(palette::TEXT)),
                    Span::styled(format!("POH {}h  ", s.power_on_hours), Style::default().fg(palette::MUTED)),
                    Span::styled(
                        format!("media_err {}  ", s.media_errors),
                        Style::default().fg(if s.media_errors > 0 { palette::ERR } else { palette::MUTED }),
                    ),
                    Span::styled(format!("warn 0x{:02x}", s.critical_warning), Style::default().fg(warn_color)),
                ]));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(header("SATA / ATA (SMART)"));
    if info.sata.is_empty() {
        lines.push(Line::from(Span::styled(
            "no ATA pass-thru controller",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for d in &info.sata {
            if d.status == smart::SmartStatus::NoHandle {
                lines.push(Line::from(Span::styled(
                    "UNVERIFIED no ATA pass-thru handle (RAID/RST or USB bridge)",
                    Style::default().fg(palette::WARN),
                )));
                continue;
            }
            let (tag, color) = if d.is_failing() {
                ("FAILING   ", palette::ERR)
            } else if d.is_verified() {
                ("healthy   ", palette::GOOD)
            } else {
                ("UNVERIFIED", palette::WARN)
            };
            let status_text = d
                .unverified_reason()
                .unwrap_or_else(|| d.status.label());
            let name = if d.model.is_empty() {
                "unknown drive".to_string()
            } else {
                d.model.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{tag} "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{:<22}", fit(&name, 22)), Style::default().fg(palette::ACCENT)),
                Span::styled(
                    format!("{:<11}", if d.capacity == 0 { "-".into() } else { human_bytes(d.capacity) }),
                    Style::default().fg(palette::TEXT),
                ),
                Span::styled(
                    format!("port {:<5}", if d.port == 0xFFFF { "-".to_string() } else { d.port.to_string() }),
                    Style::default().fg(palette::LABEL),
                ),
                Span::styled(
                    fit(&status_text, SATA_STATUS_COLS),
                    Style::default().fg(palette::MUTED),
                ),
            ]));
            if let Some(q) = &d.quirk {
                lines.push(Line::from(Span::styled(
                    format!("  note  {q}"),
                    Style::default().fg(palette::MUTED),
                )));
            }
            // Only failing attributes; passing pre-fail rows would clip the pane.
            let flagged: Vec<&smart::SmartAttr> = d
                .attrs
                .iter()
                .filter(|a| a.judgment == smart::AttrJudgment::Failing)
                .collect();
            for a in flagged.iter().take(SATA_ATTR_ROWS) {
                let (jtag, jcolor) = if smart::is_aliased(d, a.id) {
                    ("aliased raw", palette::MUTED)
                } else {
                    ("FAILING", palette::ERR)
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:>3} {:<28}", a.id, fit(&a.name, 28)),
                        Style::default().fg(palette::TEXT),
                    ),
                    Span::styled(
                        format!(
                            "val {:<5}worst {:<5}thr {:<5}",
                            a.value,
                            a.worst,
                            a.threshold.map_or("-".to_string(), |t| t.to_string())
                        ),
                        Style::default().fg(palette::TEXT),
                    ),
                    Span::styled(jtag, Style::default().fg(jcolor)),
                ]));
            }
            let hidden = flagged.len().saturating_sub(SATA_ATTR_ROWS);
            if hidden > 0 {
                lines.push(Line::from(Span::styled(
                    format!("  +{hidden} more failing attribute(s) in the fingerprint upload"),
                    Style::default().fg(palette::MUTED),
                )));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(header("PCIe links (negotiated vs max)"));
    if info.pcie.is_empty() {
        lines.push(Line::from(Span::styled(
            "no PCIe GPU/storage/network devices reported",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for l in &info.pcie {
            let degraded = l.max_width > 0 && (l.cur_width < l.max_width || l.cur_speed < l.max_speed);
            let errored = l.dev_err != 0 || l.aer_corr != 0 || l.aer_uncorr != 0;
            let color = if degraded || errored { palette::ERR } else { palette::GOOD };
            let mut spans = vec![
                Span::styled(format!("{:<9}", l.loc), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("{:<9}", l.class_label), Style::default().fg(palette::LABEL)),
                Span::styled(format!("x{} Gen{}", l.cur_width, l.cur_speed), Style::default().fg(color)),
                Span::styled(
                    format!("   (max x{} Gen{})", l.max_width, l.max_speed),
                    Style::default().fg(palette::MUTED),
                ),
            ];
            if errored {
                spans.push(Span::styled(
                    format!("  AER c=0x{:x} u=0x{:x} dev=0x{:x}", l.aer_corr, l.aer_uncorr, l.dev_err),
                    Style::default().fg(palette::ERR),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    frame.render_widget(para(lines, "Storage"), area);
}

fn page_stress(frame: &mut Frame, area: Rect, app: &App) {
    let eng = &app.stress;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(0)])
        .split(area);

    // Left: stage list, engine state, results.
    let mut lines = vec![header("Stages")];
    for (i, st) in stress::STAGES.iter().enumerate() {
        let sel = i == eng.selected;
        let running = eng.current_stage() == Some(*st);
        let cursor = Span::styled(
            if sel { "> " } else { "  " },
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        );
        let name_style = if sel {
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette::TEXT)
        };
        let state = if running {
            Span::styled(
                format!(
                    "RUN {:>9} {}",
                    charts::fmt_mag(eng.live_rate().unwrap_or(0.0)),
                    st.unit()
                ),
                Style::default().fg(palette::GOOD),
            )
        } else if let Some(r) = eng.results.iter().rev().find(|r| r.stage == *st) {
            let (tag, color) = match r.pass {
                Some(true) => ("PASS", palette::GOOD),
                Some(false) => ("FAIL", palette::ERR),
                None => ("done", palette::MUTED),
            };
            Span::styled(
                format!("{tag} {:>9} {}", charts::fmt_mag(r.avg_rate), st.unit()),
                Style::default().fg(color),
            )
        } else {
            Span::styled("-", Style::default().fg(palette::BAD))
        };
        lines.push(Line::from(vec![
            cursor,
            Span::styled(format!("{:<13}", st.label()), name_style),
            state,
        ]));
    }

    lines.push(Line::from(""));
    lines.push(header("Engine"));
    lines.push(kv("Workers", eng.workers_label()));
    lines.push(kv(
        "Mode",
        match eng.mode {
            stress::Mode::Idle => "idle".to_string(),
            stress::Mode::Single => format!(
                "single - {} {:.0}s",
                eng.current_stage().map(|s| s.label()).unwrap_or("-"),
                eng.elapsed_in_stage()
            ),
            stress::Mode::Preset => format!(
                "benchmark {}/{} - {} {:.0}s",
                eng.preset_idx + 1,
                stress::STAGES.len(),
                eng.current_stage().map(|s| s.label()).unwrap_or("-"),
                eng.elapsed_in_stage()
            ),
        },
    ));
    lines.push(kv(
        "CPU temp",
        match (eng.temp_now, eng.temp_max) {
            (Some(c), Some(m)) => format!("{c:.0} C (max {m:.0} C)"),
            _ => "n/a (Intel DTS only)".into(),
        },
    ));
    let errs = eng.memtest_errors();
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "Mem errors"), Style::default().fg(palette::LABEL)),
        Span::styled(
            format!("{errs}"),
            Style::default().fg(if errs == 0 { palette::GOOD } else { palette::ERR }),
        ),
    ]));
    let memtest_active = eng.current_stage() == Some(stress::Stage::Memtest);
    let memtest_ran = eng.results.iter().any(|r| r.stage == stress::Stage::Memtest);
    if memtest_active {
        lines.push(kv("Algorithm", stress::memtest_algo_label()));
    }
    if memtest_active || memtest_ran {
        let cov = stress::memtest_coverage();
        lines.push(kv(
            "Coverage",
            format!("{}/{} algos", cov.len(), stress::MEMTEST_ALGOS.len()),
        ));
    }
    if let Some(addr) = eng.memtest_fail_addr() {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "1st fail addr"), Style::default().fg(palette::LABEL)),
            Span::styled(format!("0x{addr:x}"), Style::default().fg(palette::ERR)),
        ]));
        for e in stress::memtest_error_log().iter().take(4) {
            lines.push(Line::from(vec![
                Span::styled("  bits ", Style::default().fg(palette::MUTED)),
                Span::styled(
                    format!("0x{:016x}", e.expected ^ e.actual),
                    Style::default().fg(palette::ERR),
                ),
                Span::styled(format!("  @ 0x{:x}", e.addr), Style::default().fg(palette::MUTED)),
            ]));
        }
    }
    if !eng.status.is_empty() {
        lines.push(kv("Status", eng.status.clone()));
    }

    // Results upload target ('p' posts fingerprint + stress summary here).
    lines.push(Line::from(""));
    lines.push(header("Results upload ('e' edit, 'p' post)"));
    let editing_target = app.editing == EditField::Target;
    if editing_target {
        lines.push(Line::from(Span::styled(
            "[ EDITING - ENTER to save ]",
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(vec![
        Span::styled("target: ", Style::default().fg(palette::MUTED)),
        Span::styled(
            if app.target.is_empty() {
                "<host:port>".to_string()
            } else {
                app.target.clone()
            },
            Style::default().fg(palette::TEXT),
        ),
        Span::styled(
            if editing_target { "_" } else { "" },
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));
    if !app.target.is_empty() {
        let u = parse_upload_url(&app.target);
        lines.push(Line::from(vec![
            Span::styled("via:    ", Style::default().fg(palette::MUTED)),
            Span::styled(
                if u.is_qc_tcp {
                    "QC TCP frame (axum 9201)"
                } else if u.needs_efi_http {
                    "EFI HTTP (DNS+TLS)"
                } else {
                    "raw TCP4 http (LAN)"
                },
                Style::default().fg(palette::LABEL),
            ),
        ]));
    }
    if !app.status.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("last:   ", Style::default().fg(palette::MUTED)),
            Span::styled(app.status.clone(), Style::default().fg(palette::GOOD)),
        ]));
    }
    frame.render_widget(para(lines, "Stress"), cols[0]);

    // Right: live charts (throughput + temperature).
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(cols[1]);
    let chart_stage = eng
        .current_stage()
        .or_else(|| eng.results.last().map(|r| r.stage));
    let (title, unit, floor) = match chart_stage {
        Some(s) => (
            format!("{} throughput", s.label()),
            s.unit(),
            s.floor(),
        ),
        None => ("throughput (run a stage)".to_string(), "", None),
    };
    frame.render_widget(
        charts::line_chart(
            title,
            unit,
            styling::THEME.accent,
            &eng.rate_hist,
            eng.now_chart_secs(),
            floor,
        ),
        rows[0],
    );
    if eng.temp_hist.is_empty() {
        frame.render_widget(
            para(
                vec![Line::from(Span::styled(
                    "no CPU thermal sensor (Intel DTS only for now)",
                    Style::default().fg(palette::MUTED),
                ))],
                "CPU Package Temp",
            ),
            rows[1],
        );
    } else {
        frame.render_widget(
            charts::line_chart(
                "CPU Package Temp".to_string(),
                "C",
                styling::CATPPUCCIN.peach,
                &eng.temp_hist,
                eng.now_chart_secs(),
                Some(95.0),
            ),
            rows[1],
        );
    }
}

fn page_order(frame: &mut Frame, area: Rect, app: &App) {
    use order::{GateKind, LookupState};
    let mut lines = Vec::new();

    if app.editing == EditField::Serial {
        lines.push(Line::from(Span::styled(
            "[ EDITING - type serial, then press ENTER to save ]",
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(vec![
        Span::styled("serial: ", Style::default().fg(palette::MUTED)),
        Span::styled(
            if app.order.serial.is_empty() {
                "<none - press 'e'>".to_string()
            } else {
                app.order.serial.clone()
            },
            Style::default().fg(palette::TEXT),
        ),
        Span::styled(
            if app.editing == EditField::Serial { "_" } else { "" },
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("server: ", Style::default().fg(palette::MUTED)),
        Span::styled(app.target.clone(), Style::default().fg(palette::LABEL)),
    ]));
    lines.push(Line::from(""));

    match &app.order.state {
        LookupState::Idle => {
            lines.push(Line::from(Span::styled(
                "ENTER to look up the order for this serial",
                Style::default().fg(palette::MUTED),
            )));
        }
        LookupState::Busy => {
            lines.push(Line::from(Span::styled(
                "looking up order ...",
                Style::default().fg(palette::ACCENT),
            )));
        }
        LookupState::Failed(e) => {
            lines.push(Line::from(Span::styled(
                format!("lookup failed: {e}"),
                Style::default().fg(palette::ERR),
            )));
        }
        LookupState::Done(resp) if !resp.found => {
            lines.push(Line::from(Span::styled(
                format!(
                    "NOT FOUND{}",
                    resp.error
                        .as_deref()
                        .map(|e| format!(" - {e}"))
                        .unwrap_or_default()
                ),
                Style::default().fg(palette::ERR),
            )));
            if !resp.tried.is_empty() {
                lines.push(kv("Tried", resp.tried.join(", ")));
            }
        }
        LookupState::Done(resp) => {
            let o = &resp.order;
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", resp.backend.to_uppercase()),
                    Style::default()
                        .fg(palette::BG)
                        .bg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  matched by {}", resp.matched_by),
                    Style::default().fg(palette::MUTED),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(kv("Order", format!("{} (id {})", o.reference, o.id)));
            lines.push(kv("Customer", o.customer.clone()));
            lines.push(kv("Kind", o.kind.clone()));
            lines.push(kv(
                "Status",
                format!("{} (#{})", o.status.name, o.status.legacy_id),
            ));
            lines.push(kv("Total", o.total.clone()));
            if let Some(bs) = &o.build_serial {
                lines.push(kv("Build SN", bs.clone()));
            }
            if let Some(ev) = &o.everest_doc {
                lines.push(kv("Everest", ev.clone()));
            }
            if let Some(g) = &resp.gate {
                let (color, label) = match g.kind() {
                    GateKind::Good => (palette::GOOD, "QC OK"),
                    GateKind::Refuse => (palette::ERR, "QC BLOCKED"),
                    GateKind::Neutral => (palette::MUTED, "QC read-only"),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{:<14}", "Gate"), Style::default().fg(palette::LABEL)),
                    Span::styled(format!("{label} - {}", g.message), Style::default().fg(color)),
                ]));
            }
            if let Some(od) = &resp.odoo {
                lines.push(kv(
                    "Odoo lot",
                    format!(
                        "{} (#{}) {}",
                        od.name,
                        od.lot_id,
                        od.product_name.clone().unwrap_or_default()
                    ),
                ));
            }
            if let Some(sv) = &o.service {
                lines.push(Line::from(""));
                lines.push(header("Service intake"));
                lines.push(kv("Device", format!("{} {} {}", sv.mfg, sv.device, sv.model)));
                lines.push(kv("Device SN", sv.serial.clone()));
                if !sv.notes.is_empty() {
                    lines.push(kv("Notes", sv.notes.clone()));
                }
            }
            if let Some(spec) = &resp.spec {
                lines.push(Line::from(""));
                lines.push(header("Ordered build spec"));
                if !spec.model.is_empty() {
                    lines.push(kv("Model", spec.model.clone()));
                }
                lines.push(kv("CPU", spec.cpu.clone()));
                lines.push(kv("GPU", spec.gpu.clone()));
                lines.push(kv("RAM", spec.ram.clone()));
                if let Some(mb) = &spec.motherboard {
                    lines.push(kv("Board", mb.clone()));
                }
                if let Some(os) = &spec.os {
                    lines.push(kv("OS", os.clone()));
                }
                for d in &spec.drives {
                    lines.push(kv("Drive", format!("{} ({})", d.name, d.kind)));
                }
                for x in &spec.extra {
                    lines.push(kv(&x.slot, x.name.clone()));
                }
            }
            if !o.items.is_empty() {
                lines.push(Line::from(""));
                lines.push(header("Items"));
                for it in o.items.iter().take(12) {
                    let serials = it
                        .serials
                        .iter()
                        .filter(|s| !s.trim().is_empty())
                        .cloned()
                        .collect::<Vec<_>>();
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{:>3} x ", it.qty),
                            Style::default().fg(palette::LABEL),
                        ),
                        Span::styled(it.name.clone(), Style::default().fg(palette::TEXT)),
                        Span::styled(
                            if serials.is_empty() {
                                String::new()
                            } else {
                                format!("  SN {}", serials.join(", "))
                            },
                            Style::default().fg(palette::GOOD),
                        ),
                    ]));
                }
                if o.items.len() > 12 {
                    lines.push(Line::from(Span::styled(
                        format!("... and {} more items", o.items.len() - 12),
                        Style::default().fg(palette::MUTED),
                    )));
                }
            }
        }
    }
    frame.render_widget(para(lines, "Order Lookup"), area);
}

fn page_readiness(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let w = win11_readiness(info);
    let (verdict, vstyle) = if w.ready {
        ("READY", Style::default().fg(palette::GOOD).add_modifier(Modifier::BOLD))
    } else {
        ("NOT READY", Style::default().fg(palette::ERR).add_modifier(Modifier::BOLD))
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Windows 11: ", Style::default().fg(palette::TEXT)),
            Span::styled(verdict, vstyle),
        ]),
        Line::from(""),
    ];
    for (label, pass) in &w.checks {
        lines.push(Line::from(vec![
            Span::styled(if *pass { "[+] " } else { "[x] " },
                Style::default().fg(if *pass { palette::GOOD } else { palette::ERR })),
            Span::styled(label.to_string(), Style::default().fg(palette::TEXT)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(header("Security & CPU"));
    let sb = match info.secure_boot {
        Some(true) => "enabled",
        Some(false) => "disabled (capable)",
        None => "not supported",
    };
    lines.push(kv("Secure Boot", sb));
    lines.push(kv("TPM", info.tpm.clone().unwrap_or_else(|| "none".into())));
    lines.push(kv("CPU", info.dmi.cpu_version.clone()));
    lines.push(kv("CPU feats", if info.cpu_feats.is_empty() { "-".to_string() } else { info.cpu_feats.join(", ") }));
    lines.push(Line::from(""));
    lines.push(header("Windows license (ACPI MSDM)"));
    if info.msdm {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "OEM license"), Style::default().fg(palette::LABEL)),
            Span::styled("embedded (MSDM present)", Style::default().fg(palette::GOOD)),
        ]));
        if !info.msdm_key.is_empty() {
            lines.push(kv("OEM key", info.msdm_key.clone()));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", "OEM license"), Style::default().fg(palette::LABEL)),
            Span::styled("none (no MSDM table)", Style::default().fg(palette::MUTED)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(header("Order identity (lookup serial)"));
    let eff = effective_serial(info);
    lines.push(Line::from(vec![
        Span::styled(format!("{:<14}", "Lookup serial"), Style::default().fg(palette::LABEL)),
        Span::styled(
            if eff.is_empty() { "(none)".to_string() } else { eff.clone() },
            Style::default()
                .fg(if eff.is_empty() { palette::ERR } else { palette::GOOD })
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let show = |v: &str| if v.trim().is_empty() { "-".to_string() } else { v.trim().to_string() };
    lines.push(kv("Chassis serial", show(&info.dmi.chassis_serial)));
    lines.push(kv("MSDM OA3 key", show(&info.msdm_key)));
    lines.push(kv("System serial", show(&info.dmi.sys_serial)));
    if info.dmi.oem_strings.is_empty() {
        lines.push(kv("OEM strings", "-".to_string()));
    } else {
        for (i, s) in info.dmi.oem_strings.iter().enumerate() {
            lines.push(kv(&format!("OEM string {}", i + 1), show(s)));
        }
    }
    frame.render_widget(para(lines, "Readiness"), area);
}

const DEV_ERR_BITS: [(u32, &str); 4] =
    [(0, "CorrErr"), (1, "NonFatalErr"), (2, "FatalErr"), (3, "UnsuppReq")];
const AER_CORR_BITS: [(u32, &str); 8] = [
    (0, "RxErr"),
    (6, "BadTLP"),
    (7, "BadDLLP"),
    (8, "ReplayRollover"),
    (12, "ReplayTimeout"),
    (13, "AdvisoryNonFatal"),
    (14, "CorrInternal"),
    (15, "HdrLogOverflow"),
];
const AER_UNCORR_BITS: [(u32, &str); 13] = [
    (4, "DataLinkProto"),
    (5, "SurpriseDown"),
    (12, "PoisonedTLP"),
    (13, "FlowCtlProto"),
    (14, "CompTimeout"),
    (15, "CompAbort"),
    (16, "UnexpCompletion"),
    (17, "RxOverflow"),
    (18, "MalformedTLP"),
    (19, "ECRC"),
    (20, "UnsuppReq"),
    (21, "ACSViolation"),
    (22, "UncorrInternal"),
];

fn aer_names(val: u32, defs: &[(u32, &'static str)]) -> String {
    defs.iter()
        .filter(|(b, _)| val & (1u32 << *b) != 0)
        .map(|(_, n)| *n)
        .collect::<Vec<&str>>()
        .join(", ")
}

fn page_diag(frame: &mut Frame, area: Rect, info: &SysInfo) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let mut left = Vec::new();
    left.push(header("Boot timing (ACPI FPDT)"));
    if info.fpdt.present {
        left.push(kv("Firmware", format!("{} ms", info.fpdt.fw_boot_ms)));
        left.push(kv("To OS loader", format!("{} ms", info.fpdt.os_loader_ms)));
        left.push(Line::from(Span::styled(
            "  (a boot that retrains RAM runs longer)",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        left.push(kv("FPDT", "absent"));
    }
    left.push(Line::from(""));
    left.push(header("RTC / CMOS"));
    left.push(Line::from(vec![
        Span::styled(format!("{:<14}", "RTC suspect"), Style::default().fg(palette::LABEL)),
        yn(info.rtc_suspect),
    ]));
    if info.cmos.present {
        left.push(Line::from(vec![
            Span::styled(format!("{:<14}", "Battery lost"), Style::default().fg(palette::LABEL)),
            yn(info.cmos.rtc_power_lost),
        ]));
        left.push(Line::from(vec![
            Span::styled(format!("{:<14}", "CMOS checksum"), Style::default().fg(palette::LABEL)),
            if info.cmos.checksum_bad {
                Span::styled("bad", Style::default().fg(palette::ERR))
            } else {
                Span::styled("ok", Style::default().fg(palette::GOOD))
            },
        ]));
        left.push(kv("CMOS diag", format!("0x{:02x}", info.cmos.diag)));
    } else {
        left.push(kv("CMOS", "unreadable"));
    }
    left.push(Line::from(""));
    left.push(header("ACPI BERT (last fatal error)"));
    if info.bert.present {
        if info.bert.error_present {
            left.push(Line::from(Span::styled(
                format!("{} ({} entries)", info.bert.severity, info.bert.entry_count),
                Style::default().fg(palette::ERR),
            )));
        } else {
            left.push(Line::from(Span::styled(
                "present, no error logged",
                Style::default().fg(palette::GOOD),
            )));
        }
    } else {
        left.push(Line::from(Span::styled(
            "absent (no persisted record)",
            Style::default().fg(palette::MUTED),
        )));
    }
    left.push(Line::from(""));
    left.push(header("Machine-check banks (MCA)"));
    if info.mca.is_empty() {
        left.push(Line::from(Span::styled("none logged", Style::default().fg(palette::GOOD))));
    } else {
        for b in &info.mca {
            left.push(Line::from(Span::styled(
                format!("bank {}: status 0x{:016x} addr 0x{:016x}", b.bank, b.status, b.addr),
                Style::default().fg(palette::ERR),
            )));
        }
    }
    left.push(Line::from(""));
    left.push(header("SMBIOS memory errors"));
    if info.mem_errors.is_empty() {
        left.push(Line::from(Span::styled("none", Style::default().fg(palette::GOOD))));
    } else {
        for e in &info.mem_errors {
            left.push(Line::from(Span::styled(
                format!("{} @ 0x{:x}", e.kind, e.addr),
                Style::default().fg(palette::ERR),
            )));
        }
    }
    frame.render_widget(para(left, "Diagnostics - boot & errors"), cols[0]);

    let mut right = Vec::new();
    right.push(header("DIMM SPD / temperature (SMBus)"));
    if info.spd.is_empty() {
        right.push(Line::from(Span::styled(
            "no SMBus SPD (non-Intel host or none read)",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for d in &info.spd {
            let mut spans = vec![
                Span::styled(format!("0x{:02x} ", d.addr), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("{:<6}", d.type_name), Style::default().fg(palette::LABEL)),
            ];
            match d.temp_c {
                Some(t) => {
                    let c = if t >= 85.0 { palette::ERR } else { palette::GOOD };
                    spans.push(Span::styled(format!("{t:.1} C"), Style::default().fg(c)));
                }
                None => spans.push(Span::styled("temp n/a", Style::default().fg(palette::MUTED))),
            }
            if let Some(pa) = d.pmic_addr {
                if d.pmic_fault {
                    spans.push(Span::styled(
                        format!("  PMIC 0x{pa:02x} FAULT (r04=0x{:02x} r08=0x{:02x})", d.pmic_r04, d.pmic_r08),
                        Style::default().fg(palette::ERR),
                    ));
                } else {
                    spans.push(Span::styled("  PMIC ok", Style::default().fg(palette::GOOD)));
                }
            }
            right.push(Line::from(spans));
        }
    }
    right.push(Line::from(""));
    right.push(header("Memory controller (MCHBAR)"));
    match info.mchbar.vendor {
        "intel" if info.mchbar.enabled => {
            right.push(kv("MCHBAR", format!("0x{:x}", info.mchbar.base)));
            let hex: Vec<String> = info.mchbar.window.iter().map(|w| format!("{w:08x}")).collect();
            right.push(Line::from(Span::styled(hex.join(" "), Style::default().fg(palette::TEXT))));
            right.push(Line::from(Span::styled(
                "  (window changing across boots = RAM retrained)",
                Style::default().fg(palette::MUTED),
            )));
        }
        "intel" => right.push(kv("MCHBAR", "present, disabled")),
        "amd" => right.push(Line::from(Span::styled(
            "AMD UMC (base-detect only)",
            Style::default().fg(palette::MUTED),
        ))),
        _ => right.push(Line::from(Span::styled("not available", Style::default().fg(palette::MUTED)))),
    }
    right.push(Line::from(""));
    right.push(header("PCIe link errors (AER / Device Status)"));
    let mut any_aer = false;
    for l in &info.pcie {
        if l.dev_err == 0 && l.aer_corr == 0 && l.aer_uncorr == 0 {
            continue;
        }
        any_aer = true;
        let concerning = l.aer_uncorr != 0 || l.aer_corr & 0x11C1 != 0;
        let col = if concerning { palette::ERR } else { palette::MUTED };
        right.push(Line::from(Span::styled(
            format!("{} {}", l.class_label, l.loc),
            Style::default().fg(palette::ACCENT),
        )));
        let dev = aer_names(l.dev_err as u32, &DEV_ERR_BITS);
        if !dev.is_empty() {
            right.push(Line::from(Span::styled(format!("  dev: {dev}"), Style::default().fg(col))));
        }
        let cor = aer_names(l.aer_corr, &AER_CORR_BITS);
        if !cor.is_empty() {
            right.push(Line::from(Span::styled(format!("  corr: {cor}"), Style::default().fg(col))));
        }
        let unc = aer_names(l.aer_uncorr, &AER_UNCORR_BITS);
        if !unc.is_empty() {
            right.push(Line::from(Span::styled(format!("  uncorr: {unc}"), Style::default().fg(palette::ERR))));
        }
    }
    if !any_aer {
        right.push(Line::from(Span::styled("no PCIe errors logged", Style::default().fg(palette::GOOD))));
    }
    frame.render_widget(para(right, "Diagnostics - memory & links"), cols[1]);
}

fn page_log(frame: &mut Frame, area: Rect) {
    let logs = log_snapshot();
    // Show only the tail that fits inside the panel borders.
    let rows = area.height.saturating_sub(2) as usize;
    let start = logs.len().saturating_sub(rows.max(1));
    let lines: Vec<Line> = logs[start..]
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(palette::TEXT))))
        .collect();
    let title = format!("Log ({} lines, showing tail)", logs.len());
    frame.render_widget(
        Paragraph::new(lines)
            .style(base_style())
            .block(panel(&title)),
        area,
    );
}

/// Host part of the relay target, without scheme or path.
fn relay_host(app: &App) -> String {
    let hp = parse_upload_url(&app.target).host_port;
    hp.split(':').next().unwrap_or(&hp).to_string()
}

/// Three-letter provenance tag for the relay target.
fn target_src_tag(src: TargetSource) -> &'static str {
    match src {
        TargetSource::Default => "def",
        TargetSource::Derived => "der",
        TargetSource::Advertised => "adv",
        TargetSource::Operator => "ops",
    }
}

/// One-line connection state for the tab panel's bottom border.
fn conn_status_line(app: &App, area_w: usize) -> Line<'static> {
    let w = area_w.saturating_sub(2);
    let mut segs: Vec<(String, Style)> = Vec::with_capacity(6);

    let (net, net_c) = match local_ip(app) {
        Some(ip) => (fit(&ip, 15), palette::GOOD),
        None => ("down".to_string(), palette::ERR),
    };
    segs.push((format!("NET {net}"), Style::default().fg(net_c)));

    let (rly, rly_c) = if app.target.is_empty() {
        ("none".to_string(), palette::ERR)
    } else {
        let c = if app.target_src == TargetSource::Default {
            palette::WARN
        } else {
            palette::GOOD
        };
        (
            format!("{}/{}", fit(&relay_host(app), 15), target_src_tag(app.target_src)),
            c,
        )
    };
    segs.push((format!("RLY {rly}"), Style::default().fg(rly_c)));

    let (prs, prs_c) = if !app.present {
        ("off", palette::MUTED)
    } else if app.present_registered {
        ("reg", palette::GOOD)
    } else {
        ("arm", palette::WARN)
    };
    segs.push((format!("PRS {prs}"), Style::default().fg(prs_c)));

    let (agt, agt_c) = if app.agent {
        ("auto", palette::GOOD)
    } else if app.agent_ran {
        ("once", palette::MUTED)
    } else {
        ("off", palette::MUTED)
    };
    segs.push((format!("AGT {agt}"), Style::default().fg(agt_c)));

    let (str_v, str_c) = if app.direct_stream {
        ("dir", palette::GOOD)
    } else if app.streaming {
        ("rly", palette::GOOD)
    } else {
        ("off", palette::MUTED)
    };
    segs.push((format!("STR {str_v}"), Style::default().fg(str_c)));

    if app.pending_relay.is_some() {
        segs.push(("OFFER:y".to_string(), Style::default().fg(palette::WARN)));
    }

    // One-space separators when two-space would overflow the border run.
    let total = segs.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
    let gaps = segs.len().saturating_sub(1);
    let sep = if total + gaps * 2 > w { " " } else { "  " };
    let mut spans = Vec::with_capacity(segs.len() * 2);
    let mut used = 0usize;
    for (i, (text, style)) in segs.into_iter().enumerate() {
        let lead = if i > 0 { sep.len() } else { 0 };
        let room = w.saturating_sub(used + lead);
        if room == 0 {
            break;
        }
        if lead > 0 {
            spans.push(Span::styled(sep.to_string(), Style::default().fg(palette::MUTED)));
        }
        let text = fit(&text, room);
        used += lead + text.chars().count();
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

fn render(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let tabs = Tabs::new(TABS.to_vec())
        .select(app.tab)
        .block(
            panel("MASTERTECH UEFI")
                .title_bottom(conn_status_line(app, root[0].width as usize)),
        )
        .style(Style::default().fg(palette::MUTED).bg(palette::BG))
        .highlight_style(
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        )
        .divider(Span::styled("  ", Style::default().fg(palette::MUTED)));
    frame.render_widget(tabs, root[0]);

    match app.tab {
        0 => page_overview(frame, root[1], &app.info),
        1 => page_system(frame, root[1], &app.info),
        2 => page_memory(frame, root[1], &app.info),
        TAB_FIRMWARE => page_firmware(frame, root[1], app),
        TAB_BIOS => page_bios(frame, root[1], &app.info),
        TAB_NETWORK => page_network(frame, root[1], app),
        6 => page_storage(frame, root[1], &app.info),
        TAB_STRESS => page_stress(frame, root[1], app),
        TAB_ORDER => page_order(frame, root[1], app),
        9 => page_readiness(frame, root[1], &app.info),
        10 => page_diag(frame, root[1], &app.info),
        TAB_BOOT => page_boot(frame, root[1], app),
        TAB_PLUGINS => page_plugins(frame, root[1], app),
        TAB_FLASH => page_flash(frame, root[1], app),
        _ => page_log(frame, root[1]),
    }

    let hints: &[(&str, &str)] = match app.tab {
        TAB_STRESS => &[
            ("Up/Dn", "stage"),
            ("ENTER", "run/stop"),
            ("b", "benchmark"),
            ("s", "stop"),
            ("e", "target"),
            ("p", "upload"),
            ("q", "quit"),
        ],
        TAB_ORDER => &[
            ("ENTER", "lookup"),
            ("e", "serial"),
            ("Tab", "next"),
            ("q", "quit"),
        ],
        TAB_PLUGINS => &[
            ("e", "plugin id"),
            ("f", "fetch"),
            ("ENTER", "run"),
            ("Tab/->", "next"),
            ("q", "quit"),
        ],
        TAB_FIRMWARE => &[
            ("e", "capsule path"),
            ("f", "load"),
            ("F F", "FLASH BIOS"),
            ("U U", "BIOS setup"),
            ("r", "refresh"),
            ("Tab", "next"),
            ("q", "quit"),
        ],
        TAB_BOOT => &[
            ("Up/Dn", "select entry"),
            ("B B", "boot selected"),
            ("r", "rescan"),
            ("Tab", "next"),
            ("q", "quit"),
        ],
        TAB_FLASH => &[
            ("e", "search model"),
            ("Up/Dn", "select"),
            ("[ ]", "step"),
            ("ENTER", "verify"),
            ("F F", "RUN step"),
            ("f", "reload index"),
            ("q", "quit"),
        ],
        _ => &[
            ("Tab", "next"),
            ("Shift+C", "connect"),
            ("r", "rescan"),
            ("e", "relay"),
            (
                "n",
                if app.present {
                    "presence off"
                } else {
                    "presence on"
                },
            ),
            ("A", "agent"),
            ("q", "quit"),
        ],
    };
    // Two-space separators, no trailing pad.
    let mut spans = Vec::with_capacity(hints.len() * 3);
    for (i, (k, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(palette::MUTED)));
        }
        spans.push(Span::styled(
            format!("{k} "),
            Style::default().fg(palette::ACCENT),
        ));
        spans.push(Span::styled(
            (*label).to_string(),
            Style::default().fg(palette::MUTED),
        ));
    }
    let status = fit(&app.status, root[2].width.saturating_sub(4) as usize);
    let footer = Paragraph::new(Line::from(spans))
        .centered()
        .style(base_style())
        .block(panel("").title_bottom(Line::from(Span::styled(
            format!(" {status} "),
            Style::default().fg(palette::GOOD),
        ))));
    frame.render_widget(footer, root[2]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditField {
    None,
    Target,
    Serial,
    Plugin,
    Capsule,
    /// Flash tab model search box.
    Flash,
}

/// Where the current relay target came from. Ordered weakest to strongest: a
/// discovery pass only replaces the target when its source ranks at least as
/// high, so nothing can overwrite what the operator typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TargetSource {
    Default,
    Derived,
    Advertised,
    Operator,
}

impl TargetSource {
    fn label(self) -> &'static str {
        match self {
            TargetSource::Default => "compile-time default",
            TargetSource::Derived => "derived from beacon",
            TargetSource::Advertised => "advertised by console",
            TargetSource::Operator => "operator-set",
        }
    }
}

struct App {
    info: SysInfo,
    tab: usize,
    target: String,
    /// Provenance of `target`; gates relay auto-adoption.
    target_src: TargetSource,
    /// Advertised relay outside the beacon's subnet, awaiting a 'y' from the
    /// operator before it becomes the target.
    pending_relay: Option<String>,
    editing: EditField,
    ifaces: Vec<IfaceIp>,
    status: String,
    stress: stress::StressEngine,
    order: order::OrderPanel,
    /// The screen shows state that changed since the last frame.
    dirty: bool,
    /// Autonomous fleet-command polling enabled.
    agent: bool,
    /// Clock time of the next command poll.
    agent_next_ms: u64,
    /// A fleet-command poll has run at least once.
    agent_ran: bool,
    /// Pass/fail and detail for each step of the last `connect_all` run.
    connect_steps: Vec<(bool, String)>,
    /// Raw SimpleNetwork lease (set when DHCP succeeded only via the SNP path).
    raw_net: Option<netraw::RawNet>,
    /// Rendered output from the last WASM plugin run.
    plugin_out: Vec<String>,
    /// Registry plugin id to fetch from the upload target.
    plugin_id_in: String,
    /// Last fetched registry plugin (wasm bytes); Enter runs it over the demo.
    plugin_wasm: Vec<u8>,
    /// `mcp_tools` blob of `plugin_wasm`, read at fetch time; empty when unknown.
    plugin_tools: String,
    /// `fw_data(&info)` memo; `None` until built, cleared whenever `info` changes.
    fw_cache: Option<wasmrt::FwData>,
    /// Pre-boot TUI streaming to the admin console is active.
    streaming: bool,
    /// Monotonic streamed-frame counter.
    stream_frame: u64,
    /// Clock time of the next remote-input poll.
    stream_input_next_ms: u64,
    /// Remote input keeps polling fast until this clock time.
    stream_input_active_ms: u64,
    /// A key was handled recently; keep the loop hot until this clock time.
    key_active_ms: u64,
    /// Dirty-frame suppressor for the stream.
    stream_throttle: stream::Throttle,
    /// Remote viewer events queued for injection into the input loop.
    injected: Vec<terminput::Event>,
    /// Auto-presence: once the network is up, register + heartbeat so the box
    /// shows in the admin console without operator action.
    present: bool,
    present_registered: bool,
    /// Clock time of the next heartbeat.
    present_next_ms: u64,
    /// Clock time of the next relay viewer-flag check.
    viewer_next_ms: u64,
    /// Streaming was started by the relay's viewer flag, not the 'v' key.
    stream_auto: bool,
    /// Consecutive viewer-flag checks with no viewer (auto-stop hysteresis).
    viewer_miss: u8,
    /// 'v' stopped streaming; suppresses auto-start until the viewer leaves.
    stream_manual_off: bool,
    /// Persistent direct socket to a console listener (bypasses the relay for
    /// streaming); None until discovery dials one.
    direct: Option<net_tcp::DirectLink>,
    /// Console asked us (over the direct socket) to stream frames.
    direct_stream: bool,
    /// Clock time of the next direct-socket service poll.
    direct_pump_next_ms: u64,
    /// Clock time of the next console-discovery attempt while unlinked.
    discover_next_ms: u64,
    /// Gap between discovery attempts; doubles on every miss.
    discover_backoff_ms: u64,
    /// Capsule source: a volume-relative path, or a URL when it has a scheme.
    capsule_src: String,
    /// Loaded capsule awaiting a flash.
    capsule_bytes: Vec<u8>,
    /// Inspection + pre-flight report for the loaded capsule.
    capsule_note: Vec<String>,
    /// Pre-flight refused the loaded capsule; blocks arming.
    capsule_blocked: bool,
    /// Second 'F' will flash. Cleared by any other key.
    fw_armed: bool,
    /// Two-press guard for the reboot-into-setup handoff.
    fw_setup_armed: bool,
    /// Clock of the last byte written to the direct link, for the keepalive.
    direct_tx_ms: u64,
    /// Highlighted Boot#### row on the Boot tab.
    boot_sel: usize,
    /// Two-press guard for booting the highlighted entry.
    boot_armed: bool,
    /// BIOSLove payload catalogue; None until an index is found on a volume.
    flash_index: Option<bioslove::Index>,
    /// Why the last index load failed; empty once one loads.
    flash_err: String,
    /// SMBIOS auto-detect result, strongest first.
    flash_matches: Vec<bioslove::Match>,
    /// Model search box; empty falls back to the auto-detect list.
    flash_filter: String,
    /// Highlighted row on the Flash tab.
    flash_sel: usize,
    /// Step cursor within the highlighted entry's recipe.
    flash_step: usize,
    /// Pre-flight report for the step under the cursor.
    flash_note: Vec<String>,
    /// A pre-flight gate refused this step; blocks arming.
    flash_blocked: bool,
    /// Verified tool bytes and command line awaiting a launch.
    flash_ready: Option<PreparedStep>,
    /// Second 'F' launches the prepared step. Cleared by any other key.
    flash_armed: bool,
}

/// A recipe step that passed every gate and is ready to run.
struct PreparedStep {
    entry: usize,
    step: usize,
    /// Volume the tool and its payloads were read from.
    volume: uefi::Handle,
    /// Volume path of the tool, for the child's device path.
    exec_path: String,
    /// Full command line, argv[0] included.
    command_line: String,
    bytes: Vec<u8>,
}

impl App {
    /// Firmware data for a plugin run, built once per `info` generation.
    fn fw(&mut self) -> wasmrt::FwData {
        match &self.fw_cache {
            Some(fw) => fw.clone(),
            None => {
                let fw = fw_data(&self.info);
                logln("wasm: fw_data rebuilt".to_string());
                self.fw_cache = Some(fw.clone());
                fw
            }
        }
    }

    /// Drop the `fw_data` memo derived from `info`.
    fn invalidate_fw(&mut self) {
        self.fw_cache = None;
    }

    /// Replace `info` and drop the data derived from it.
    fn set_info(&mut self, info: SysInfo) {
        self.info = info;
        self.invalidate_fw();
    }

    /// Replace the fetched plugin bytes and drop its cached tool list.
    fn set_plugin_wasm(&mut self, bytes: Vec<u8>) {
        self.plugin_wasm = bytes;
        self.plugin_tools.clear();
    }

    fn next(&mut self) {
        self.tab = (self.tab + 1) % TABS.len();
    }
    fn prev(&mut self) {
        self.tab = (self.tab + TABS.len() - 1) % TABS.len();
    }

    /// Turn presence on and schedule its heartbeat, viewer check and discovery.
    /// An already-registered caller waits a full interval for its first beat.
    fn arm_presence(&mut self, now: u64, registered: bool) {
        self.present = true;
        self.present_registered = registered;
        self.present_next_ms = if registered {
            now.saturating_add(PRESENCE_HEARTBEAT_MS)
        } else {
            now
        };
        self.viewer_next_ms = now.saturating_add(VIEWER_CHECK_MS);
        self.reset_discovery(now);
    }

    /// Turn presence off and drop the direct link and any auto-started stream.
    fn clear_presence(&mut self) {
        self.present = false;
        self.present_registered = false;
        self.direct = None;
        self.direct_stream = false;
        if self.stream_auto {
            self.streaming = false;
            self.stream_auto = false;
        }
        self.stream_throttle = stream::Throttle::new();
    }

    /// Restart console discovery at the base interval.
    fn reset_discovery(&mut self, now: u64) {
        self.discover_backoff_ms = DIRECT_DISCOVER_MS;
        self.discover_next_ms = now.saturating_add(DIRECT_DISCOVER_FIRST_MS);
    }

    /// True when the loop must poll instead of blocking on the keyboard.
    fn polling(&self) -> bool {
        self.stress.is_active()
            || self.agent
            || self.streaming
            || self.present
            || self.direct.is_some()
    }
}

/// One-line description of what the run loop is currently doing.
fn mode_line(app: &App) -> String {
    if !app.polling() {
        return "idle - Shift+C connects".to_string();
    }
    let mut on: Vec<&str> = Vec::new();
    on.push(if app.present {
        "presence on"
    } else {
        "presence off"
    });
    if app.direct.is_some() {
        on.push(if app.direct_stream {
            "direct link streaming"
        } else {
            "direct link"
        });
    }
    if app.streaming {
        on.push("relay stream");
    }
    if app.agent {
        on.push("agent");
    }
    if app.stress.is_active() {
        on.push("stress run");
    }
    format!("polling - {}", on.join(", "))
}

/// Shown before every `SysInfo::collect()`; the SMART and volume probes block.
const RESCAN_NOTICE: &str = "rescanning hardware (SMART + volume probe, up to 60s)...";

/// Run every background job whose deadline has passed: fleet-command polls,
/// presence heartbeats and viewer checks, direct-link service, and console
/// discovery. Each deadline is re-armed from the clock after the job returns,
/// so a slow network op can never queue a burst of catch-up work.
fn service_background(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
    clock: &Clock,
) -> Result<()> {
    let now = clock.now_ms();
    if app.agent && now >= app.agent_next_ms {
        agent_poll(app, terminal)?;
        app.agent_next_ms = clock.now_ms().saturating_add(AGENT_POLL_MS);
        app.dirty = true;
    }
    if app.present && now >= app.viewer_next_ms {
        viewer_check(app);
        app.viewer_next_ms = clock.now_ms().saturating_add(VIEWER_CHECK_MS);
        app.dirty = true;
    }
    if app.present && now >= app.present_next_ms {
        presence_beat(app);
        app.present_next_ms = clock.now_ms().saturating_add(PRESENCE_HEARTBEAT_MS);
        app.dirty = true;
    }
    // Service the direct socket (input/stream-ctl/plugin frames), or back off
    // through discovery attempts until a console listener answers.
    if app.direct.is_some() {
        if now >= app.direct_pump_next_ms {
            if direct_pump(app, terminal)? {
                app.dirty = true;
            }
            let interval = if app.direct_stream {
                DIRECT_PUMP_MS
            } else {
                DIRECT_PUMP_IDLE_MS
            };
            app.direct_pump_next_ms = clock.now_ms().saturating_add(interval);
        }
    } else if app.present && now >= app.discover_next_ms {
        // The console beacons every ~3s, so widen the listen window as the
        // backoff spreads attempts out, holding the listen duty cycle at ~10%.
        let listen = (app.discover_backoff_ms / 10)
            .clamp(DISCOVER_LISTEN_MS, DISCOVER_LISTEN_MAX_MS);
        direct_discover(app, listen);
        let after = clock.now_ms();
        if app.direct.is_some() {
            app.discover_backoff_ms = DIRECT_DISCOVER_MS;
            app.direct_pump_next_ms = after;
        } else {
            app.discover_next_ms = after.saturating_add(app.discover_backoff_ms);
            app.discover_backoff_ms =
                app.discover_backoff_ms.saturating_mul(2).min(DIRECT_DISCOVER_MAX_MS);
        }
        app.dirty = true;
    }
    Ok(())
}

/// Milliseconds to stall: the gap to the nearest deadline, capped by how hot
/// the loop currently needs to run and floored so it always yields.
fn next_stall_ms(app: &App, now: u64, last_draw_ms: u64, streaming: bool) -> u64 {
    let mut next = last_draw_ms.saturating_add(REDRAW_MIN_MS);
    if app.agent {
        next = next.min(app.agent_next_ms);
    }
    if app.present {
        next = next.min(app.present_next_ms).min(app.viewer_next_ms);
        if app.direct.is_none() {
            next = next.min(app.discover_next_ms);
        }
    }
    if app.direct.is_some() {
        next = next.min(app.direct_pump_next_ms);
    }
    if streaming {
        next = next.min(app.stream_input_next_ms);
    }
    let hot = app.stress.is_active()
        || now < app.stream_input_active_ms
        || now < app.key_active_ms;
    let ceiling = if hot { BUSY_STALL_MS } else { IDLE_STALL_MAX_MS };
    next.saturating_sub(now).clamp(MIN_STALL_MS, ceiling)
}

fn run() -> Result<()> {
    init_ratatui_perf();
    let (mut terminal, mut input_reader) = create_ui()?;
    logln(RESCAN_NOTICE.to_string());
    let info = SysInfo::collect();
    let default_serial = effective_serial(&info);
    let mut app = App {
        info,
        tab: 0,
        target: DEFAULT_URL.to_string(),
        target_src: TargetSource::Default,
        pending_relay: None,
        editing: EditField::None,
        ifaces: Vec::new(),
        status: String::new(),
        stress: stress::StressEngine::new(),
        order: order::OrderPanel::new(default_serial),
        dirty: true,
        agent: false,
        agent_next_ms: 0,
        agent_ran: false,
        connect_steps: Vec::new(),
        raw_net: None,
        plugin_out: Vec::new(),
        plugin_id_in: "com.mastertech.uefi-diag".to_string(),
        plugin_wasm: Vec::new(),
        plugin_tools: String::new(),
        fw_cache: None,
        streaming: false,
        stream_frame: 0,
        stream_input_next_ms: 0,
        stream_input_active_ms: 0,
        key_active_ms: 0,
        stream_throttle: stream::Throttle::new(),
        injected: Vec::new(),
        present: false,
        present_registered: false,
        present_next_ms: 0,
        viewer_next_ms: 0,
        stream_auto: false,
        viewer_miss: 0,
        stream_manual_off: false,
        direct: None,
        direct_stream: false,
        direct_pump_next_ms: 0,
        discover_next_ms: 0,
        discover_backoff_ms: DIRECT_DISCOVER_MS,
        capsule_src: "bios\\update.cap".to_string(),
        capsule_bytes: Vec::new(),
        capsule_note: Vec::new(),
        capsule_blocked: false,
        fw_armed: false,
        fw_setup_armed: false,
        direct_tx_ms: 0,
        boot_sel: 0,
        boot_armed: false,
        flash_index: None,
        flash_err: String::new(),
        flash_matches: Vec::new(),
        flash_filter: String::new(),
        flash_sel: 0,
        flash_step: 0,
        flash_note: Vec::new(),
        flash_blocked: false,
        flash_ready: None,
        flash_armed: false,
    };

    terminal.clear()?;

    // No network at boot: connecting is explicit so a flaky NIC or a hanging
    // driver bind can never stall or freeze startup.
    app.status =
        "ready - press Shift+C to connect (drivers, DHCP, console, presence, agent)".into();

    let mut clock = Clock::new();
    let mut last_draw_ms = 0u64;
    let mut last_log_seq = log_seq();
    let mut was_linked = false;

    loop {
        app.stress.tick();
        let now = clock.now_ms();
        relay_gate::tick(now);
        // A grown log ring changes the Log tab.
        if log_seq() != last_log_seq {
            last_log_seq = log_seq();
            app.dirty = true;
        }
        // A dropped link re-arms discovery at the base interval.
        if was_linked && app.direct.is_none() {
            app.reset_discovery(now);
        }
        was_linked = app.direct.is_some();

        // Direct socket (preferred when a console has linked + asked to stream)
        // or the HTTP relay (when 'v'/viewer-flag streaming is on).
        let want_direct = app.direct.is_some() && app.direct_stream;
        let want_relay = app.streaming && !app.target.is_empty();
        let streaming = want_direct || want_relay;

        // Redraw on a state change, else at the floor cadence so live values move.
        // While streaming the rendered buffer is captured into a wire frame; the
        // CompletedFrame's borrow ends when the block returns.
        if app.dirty || now.saturating_sub(last_draw_ms) >= REDRAW_MIN_MS {
            app.dirty = false;
            last_draw_ms = now;
            let captured = {
                let completed = terminal.draw(|frame| render(frame, &app))?;
                if streaming {
                    Some(stream::buffer_to_frame(completed.buffer, app.stream_frame))
                } else {
                    None
                }
            };
            if let Some(mut pf) = captured {
                // Send the frame body only when the screen changed.
                if let Some(body) = app.stream_throttle.body_if_dirty(&mut pf) {
                    if want_direct {
                        if let Some(link) = app.direct.as_mut() {
                            app.direct_tx_ms = now;
                            if let Err(e) =
                                link.send(tcp_protocol::preboot::FRAME_TAG_PREBOOT_FRAME, &body)
                            {
                                logln(format!("direct: frame send failed: {e}"));
                                app.direct = None;
                                app.direct_stream = false;
                                // Undelivered: re-arm so the next draw re-sends.
                                app.stream_throttle = stream::Throttle::new();
                            }
                        }
                    } else {
                        let serial = order::encode_path_segment(&effective_serial(&app.info));
                        let path = format!("/api/v1/qc/preboot/{serial}/frame");
                        if let Err(e) = http_post_json(&app.target, &path, &body) {
                            logln(format!("stream: frame POST failed: {e}"));
                            // Undelivered: re-arm so the next draw re-sends.
                            app.stream_throttle = stream::Throttle::new();
                        }
                    }
                }
                app.stream_frame = app.stream_frame.wrapping_add(1);
            }
        }

        // Keepalive: a static screen sends no frames, so without this the
        // console's session goes stale and a live link is reaped.
        if app.direct.is_some() && now.saturating_sub(app.direct_tx_ms) >= DIRECT_KEEPALIVE_MS {
            let sent = app
                .direct
                .as_mut()
                .map(|l| l.send(tcp_protocol::FRAME_TAG_PING, &[]));
            match sent {
                Some(Ok(())) => app.direct_tx_ms = now,
                Some(Err(e)) => {
                    logln(format!("direct: keepalive failed, dropping link: {e}"));
                    app.direct = None;
                    app.direct_stream = false;
                    app.dirty = true;
                }
                None => {}
            }
        }

        // Poll remote input a round-trip apart for ~1s after any event, then at
        // the slow interval.
        if streaming && now >= app.stream_input_next_ms {
            let interval = if now < app.stream_input_active_ms {
                PREBOOT_INPUT_FAST_MS
            } else {
                PREBOOT_INPUT_POLL_MS
            };
            let got = poll_preboot_input(&mut app);
            let after = clock.now_ms();
            app.stream_input_next_ms = after.saturating_add(interval);
            if got {
                app.stream_input_active_ms = after.saturating_add(PREBOOT_INPUT_ACTIVE_MS);
            }
        }

        // Blocking input when idle; poll + deadline-driven background work while
        // a stress run, agent polling, presence or streaming needs the loop to
        // keep turning. A queued remote-viewer event is consumed as if it were
        // typed locally.
        let event = if !app.injected.is_empty() {
            // FIFO: keystrokes must replay in the order they were typed.
            Some(app.injected.remove(0))
        } else if app.polling() {
            let ev = input_reader.poll_event()?;
            if ev.is_none() {
                service_background(&mut app, &mut terminal, &clock)?;
                let now = clock.now_ms();
                let stall = next_stall_ms(&app, now, last_draw_ms, streaming);
                clock.stall(stall);
                continue;
            }
            ev
        } else {
            input_reader.read_event()?
        };
        let Some(terminput::Event::Key(key)) = event else {
            continue;
        };
        // Any key can move the screen, and typing keeps the loop hot.
        app.dirty = true;
        app.key_active_ms = clock.now_ms().saturating_add(KEY_ACTIVE_MS);
        logln(format!(
            "key={:?} editing={:?} tab={}",
            key.code, app.editing, app.tab
        ));

        // While editing a field, all printable keys feed it.
        if app.editing != EditField::None {
            let before = (app.editing == EditField::Target).then(|| app.target.clone());
            let field = match app.editing {
                EditField::Serial => &mut app.order.serial,
                EditField::Plugin => &mut app.plugin_id_in,
                EditField::Capsule => &mut app.capsule_src,
                EditField::Flash => &mut app.flash_filter,
                _ => &mut app.target,
            };
            match key.code {
                terminput::KeyCode::Enter | terminput::KeyCode::Esc => {
                    app.editing = EditField::None
                }
                terminput::KeyCode::Backspace => {
                    field.pop();
                }
                terminput::KeyCode::Char(c) if c == '\u{8}' || c == '\u{7f}' => {
                    field.pop();
                }
                terminput::KeyCode::Char(c) if !c.is_control() => field.push(c),
                _ => {}
            }
            // A changed search term re-slices the row set under the cursor.
            if app.editing == EditField::Flash || key.code == terminput::KeyCode::Enter {
                app.flash_sel = 0;
                app.flash_step = 0;
                clear_flash_prep(&mut app);
                app.dirty = true;
            }
            // A hand-edited target outranks discovery and is never overwritten.
            if before.is_some_and(|b| b != app.target) {
                app.target_src = TargetSource::Operator;
                relay_gate::reset();
                app.status = relay_line(&app);
            }
            continue;
        }

        // Arming is single-shot: anything other than a second 'F' disarms.
        if app.fw_setup_armed && key.code != terminput::KeyCode::Char('U') {
            app.fw_setup_armed = false;
        }
        if app.boot_armed && key.code != terminput::KeyCode::Char('B') {
            app.boot_armed = false;
        }
        if app.fw_armed && key.code != terminput::KeyCode::Char('F') {
            app.fw_armed = false;
            app.status = "flash disarmed".into();
        }
        if app.flash_armed && key.code != terminput::KeyCode::Char('F') {
            app.flash_armed = false;
            app.status = "flash disarmed".into();
        }

        match key.code {
            terminput::KeyCode::Char('q') | terminput::KeyCode::Esc => {
                if app.stress.is_active() {
                    app.stress.stop();
                }
                break;
            }
            terminput::KeyCode::Char('r') => {
                app.status = RESCAN_NOTICE.into();
                terminal.draw(|frame| render(frame, &app))?;
                app.set_info(SysInfo::collect());
                redetect_bioslove(&mut app);
                app.status = "rescanned".into();
            }
            terminput::KeyCode::Char('c') => {
                app.status = RESCAN_NOTICE.into();
                terminal.draw(|frame| render(frame, &app))?;
                connect_network_stack();
                app.set_info(SysInfo::collect());
                app.reset_discovery(clock.now_ms());
                // Beacon-only: no lease yet, so learn the relay without dialing.
                app.status = "connect: listening for a console beacon...".into();
                terminal.draw(|frame| render(frame, &app))?;
                let heard = listen_for_relay(&mut app, DISCOVER_LISTEN_KICK_MS);
                let beacon = if heard { "beacon heard" } else { "no beacon" };
                app.status =
                    format!("connect: bound network stack, {beacon} - {}", relay_line(&app));
            }
            terminput::KeyCode::Char('C') => {
                connect_all(&mut app, &mut terminal, &clock)?;
            }
            terminput::KeyCode::Char('d') => {
                app.status = "DHCP: working (up to 30s)...".into();
                terminal.draw(|frame| render(frame, &app))?;
                let (ifaces, raw, status) = run_dhcp();
                let networked = !ifaces.is_empty() || raw.is_some();
                app.ifaces = ifaces;
                app.raw_net = raw;
                app.status = status;
                // Network is up by operator choice — auto-appear in the console.
                let now = clock.now_ms();
                if networked && !app.present {
                    app.arm_presence(now, false);
                } else {
                    app.reset_discovery(now);
                }
                if networked {
                    let lease = core::mem::replace(
                        &mut app.status,
                        "DHCP done - listening for a console beacon...".to_string(),
                    );
                    terminal.draw(|frame| render(frame, &app))?;
                    kick_discovery(&mut app, &clock);
                    app.status = format!("{lease} - {}", relay_line(&app));
                }
            }
            terminput::KeyCode::Char('n') => {
                let now = clock.now_ms();
                if app.present {
                    app.clear_presence();
                    app.status =
                        "net presence OFF - blocking input ('n' re-arms heartbeat)".into();
                } else {
                    app.arm_presence(now, false);
                    app.status = "net presence ON - heartbeat + console discovery".into();
                }
            }
            terminput::KeyCode::Char('e') => {
                app.editing = if app.tab == TAB_ORDER {
                    EditField::Serial
                } else if app.tab == TAB_PLUGINS {
                    EditField::Plugin
                } else if app.tab == TAB_FIRMWARE {
                    EditField::Capsule
                } else if app.tab == TAB_FLASH {
                    EditField::Flash
                } else {
                    EditField::Target
                };
            }
            terminput::KeyCode::Char('p') => {
                logln(format!("POST key: target='{}'", app.target));
                if app.target.is_empty() {
                    app.status = "set a target first (press 'e')".into();
                } else if let Some(rn) = app.raw_net {
                    // Raw SNP lease active (firmware IPv4 stack unavailable): UDP upload.
                    let host = parse_upload_url(&app.target).host_port;
                    match netraw::parse_ipv4(&host) {
                        Some(ip) => {
                            app.status = format!("POST: raw UDP -> {}:{} ...", netraw::ip_str(ip), netraw::UDP_PORT);
                            terminal.draw(|frame| render(frame, &app))?;
                            let json = fingerprint_with_stress(&app.info, app.stress.summary_json());
                            app.status = match rn.send_udp(ip, netraw::UDP_PORT, json.as_bytes()) {
                                Ok(n) => format!("OK: sent {n} UDP chunk(s) to {}:{}", netraw::ip_str(ip), netraw::UDP_PORT),
                                Err(e) => format!("raw upload failed: {e}"),
                            };
                        }
                        None => app.status = "raw upload needs an IPv4 target (set host to a.b.c.d)".into(),
                    }
                } else {
                    let u = parse_upload_url(&app.target);
                    let transport = if u.is_qc_tcp {
                        "QC TCP"
                    } else if u.needs_efi_http {
                        "EFI HTTP"
                    } else {
                        "TCP4 HTTP"
                    };
                    app.status = format!("POST: {transport} -> {} ...", u.full);
                    terminal.draw(|frame| render(frame, &app))?;
                    let json =
                        fingerprint_with_stress(&app.info, app.stress.summary_json());
                    let result = if u.is_qc_tcp {
                        net_tcp::send_qc(&u.host_port, json.as_bytes())
                    } else if u.needs_efi_http {
                        http_efi::post(&u.full, json.as_bytes())
                    } else {
                        net_tcp::post(&u.host_port, &u.path, json.as_bytes())
                    };
                    app.status = match result {
                        Ok(s) => {
                            // A successful upload proves the target is
                            // reachable - arm presence without needing 'd'.
                            let now = clock.now_ms();
                            if !app.present {
                                app.arm_presence(now, true);
                            } else {
                                app.reset_discovery(now);
                            }
                            format!("OK: {s}")
                        }
                        Err(e) => format!("upload failed: {e}"),
                    };
                }
            }
            terminput::KeyCode::Char('y') => {
                match app.pending_relay.take() {
                    Some(url) => {
                        adopt_relay(&mut app, &url, TargetSource::Operator);
                        app.status = relay_line(&app);
                    }
                    None => app.status = "no relay offer to accept".into(),
                }
            }
            terminput::KeyCode::Char('a') => {
                agent_poll(&mut app, &mut terminal)?;
            }
            terminput::KeyCode::Char('A') => {
                app.agent = !app.agent;
                app.agent_next_ms = clock.now_ms();
                app.status = if app.agent {
                    "agent mode ON - auto-polling commands".into()
                } else {
                    "agent mode off".into()
                };
            }
            terminput::KeyCode::Char('v') => {
                app.streaming = !app.streaming;
                app.stream_auto = false;
                // A manual stop holds until the current viewer disconnects.
                app.stream_manual_off = !app.streaming;
                app.stream_frame = 0;
                app.stream_throttle = stream::Throttle::new();
                app.status = if app.streaming {
                    format!("relay streaming TUI -> {}", app.target)
                } else if app.direct.is_some() {
                    "relay streaming off - the direct link still carries frames".into()
                } else {
                    "relay streaming off (auto-resume when this viewer leaves)".into()
                };
            }
            terminput::KeyCode::Char('u') => {
                upload_logs(&mut app);
            }
            // Stress tab controls.
            terminput::KeyCode::Up if app.tab == TAB_STRESS => {
                app.stress.selected = app.stress.selected.saturating_sub(1);
            }
            terminput::KeyCode::Down if app.tab == TAB_STRESS => {
                app.stress.selected =
                    (app.stress.selected + 1).min(stress::STAGES.len() - 1);
            }
            terminput::KeyCode::Enter if app.tab == TAB_STRESS => {
                if app.stress.is_active() {
                    app.stress.stop();
                } else {
                    app.stress.start_single(stress::STAGES[app.stress.selected]);
                }
            }
            terminput::KeyCode::Char('b') if app.tab == TAB_STRESS => {
                app.stress.start_preset();
            }
            // Plugins tab: run the fetched registry plugin, else the demo.
            terminput::KeyCode::Enter if app.tab == TAB_PLUGINS => {
                if app.plugin_wasm.is_empty() {
                    app.status =
                        format!("no fetched plugin - running demo ('f' fetches {})", app.plugin_id_in);
                    terminal.draw(|frame| render(frame, &app))?;
                    run_wasm_plugin(&mut app, DEMO_PLUGIN, Some("selftest"), "{}");
                } else {
                    run_fetched_plugin(&mut app, &mut terminal)?;
                }
            }
            terminput::KeyCode::Char('f') if app.tab == TAB_PLUGINS => {
                fetch_registry_plugin(&mut app, &mut terminal)?;
            }
            // Firmware tab: load + inspect a capsule, then a two-press flash.
            terminput::KeyCode::Char('f') if app.tab == TAB_FIRMWARE => {
                load_and_inspect_capsule(&mut app, &mut terminal)?;
            }
            // Flash tab: (re)load the BIOSLove index off a volume and re-detect.
            terminput::KeyCode::Char('f') if app.tab == TAB_FLASH => {
                app.status = format!("reading {}...", bioslove::INDEX_PATH);
                terminal.draw(|frame| render(frame, &app))?;
                load_bioslove_index(&mut app);
            }
            terminput::KeyCode::Up if app.tab == TAB_FLASH => {
                app.flash_sel = app.flash_sel.saturating_sub(1);
                app.flash_step = 0;
                clear_flash_prep(&mut app);
                app.dirty = true;
            }
            terminput::KeyCode::Down if app.tab == TAB_FLASH => {
                let last = flash_rows(&app).len().saturating_sub(1);
                app.flash_sel = (app.flash_sel + 1).min(last);
                app.flash_step = 0;
                clear_flash_prep(&mut app);
                app.dirty = true;
            }
            // Step cursor within the selected recipe.
            terminput::KeyCode::Char('[') if app.tab == TAB_FLASH => {
                app.flash_step = app.flash_step.saturating_sub(1);
                clear_flash_prep(&mut app);
                app.dirty = true;
            }
            terminput::KeyCode::Char(']') if app.tab == TAB_FLASH => {
                let rows = flash_rows(&app);
                let last = rows
                    .get(app.flash_sel)
                    .and_then(|(ei, _)| app.flash_index.as_ref().map(|i| i.entries[*ei].steps.len()))
                    .unwrap_or(0)
                    .saturating_sub(1);
                app.flash_step = (app.flash_step + 1).min(last);
                clear_flash_prep(&mut app);
                app.dirty = true;
            }
            // Verify the step under the cursor without loading or starting it.
            terminput::KeyCode::Enter if app.tab == TAB_FLASH => {
                app.status = "verifying...".into();
                terminal.draw(|frame| render(frame, &app))?;
                prepare_flash_step(&mut app);
            }
            terminput::KeyCode::Char('F') if app.tab == TAB_FLASH => {
                if app.flash_ready.is_none() {
                    app.status = "prepare a step first (ENTER)".into();
                } else if app.flash_blocked {
                    app.status = "pre-flight refused this step".into();
                } else if !app.flash_armed {
                    app.flash_armed = true;
                    app.status = "ARMED - press F again to run the vendor tool".into();
                } else {
                    app.flash_armed = false;
                    do_flash_step(&mut app, &mut terminal)?;
                }
            }
            terminput::KeyCode::Char('F') if app.tab == TAB_FIRMWARE => {
                if app.capsule_bytes.is_empty() {
                    app.status = "load a capsule first ('e' path, 'f' load)".into();
                } else if app.capsule_blocked {
                    app.status = "pre-flight refused this capsule; see the Firmware tab".into();
                } else if !app.fw_armed {
                    app.fw_armed = true;
                    app.status = "ARMED - press F again to FLASH (irreversible)".into();
                } else {
                    app.fw_armed = false;
                    do_bios_flash(&mut app, &mut terminal)?;
                }
            }
            // Boot tab: pick an entry, then two presses to boot it once.
            terminput::KeyCode::Up if app.tab == TAB_BOOT => {
                app.boot_sel = app.boot_sel.saturating_sub(1);
                app.boot_armed = false;
                app.dirty = true;
            }
            terminput::KeyCode::Down if app.tab == TAB_BOOT => {
                let last = app.info.bootdiag.boot_entries.len().saturating_sub(1);
                app.boot_sel = (app.boot_sel + 1).min(last);
                app.boot_armed = false;
                app.dirty = true;
            }
            terminput::KeyCode::Char('B') if app.tab == TAB_BOOT => {
                let entries = &app.info.bootdiag.boot_entries;
                let pick = entries
                    .get(app.boot_sel)
                    .map(|e| (e.num, e.description.clone()))
                    .or_else(|| {
                        bootdiag::default_boot_target(entries).map(|n| (n, "default".to_string()))
                    });
                match pick {
                    None => app.status = "no boot entries to boot".into(),
                    Some((num, desc)) if !app.boot_armed => {
                        app.boot_armed = true;
                        app.status =
                            format!("press B again to reboot into Boot{num:04X} ({desc})");
                    }
                    Some((num, desc)) => {
                        app.boot_armed = false;
                        match bootdiag::set_boot_next(num) {
                            Ok(()) => {
                                app.status = format!("booting Boot{num:04X} ({desc})...");
                                terminal.draw(|frame| render(frame, &app))?;
                                uefi::runtime::reset(
                                    uefi::runtime::ResetType::WARM,
                                    uefi::Status::SUCCESS,
                                    None,
                                );
                            }
                            Err(e) => app.status = format!("BootNext failed: {e}"),
                        }
                    }
                }
            }
            // Hand off to the board's own updater: arm setup entry, then reset.
            terminput::KeyCode::Char('U') if app.tab == TAB_FIRMWARE => {
                if !app.fw_setup_armed {
                    app.fw_setup_armed = true;
                    app.status = "press U again to reboot into BIOS setup".into();
                } else {
                    app.fw_setup_armed = false;
                    match capsule::request_boot_to_fw_ui(&app.info.esrt) {
                        Ok(()) => {
                            app.status = "rebooting into BIOS setup...".into();
                            terminal.draw(|frame| render(frame, &app))?;
                            uefi::runtime::reset(
                                uefi::runtime::ResetType::WARM,
                                uefi::Status::SUCCESS,
                                None,
                            );
                        }
                        Err(e) => app.status = format!("setup entry refused: {e}"),
                    }
                }
            }
            terminput::KeyCode::Char('s') if app.tab == TAB_STRESS => {
                if app.stress.is_active() {
                    app.stress.stop();
                }
            }
            // Order tab controls.
            terminput::KeyCode::Enter | terminput::KeyCode::Char('o')
                if app.tab == TAB_ORDER =>
            {
                do_order_lookup(&mut app, &mut terminal)?;
            }
            terminput::KeyCode::Right
            | terminput::KeyCode::Tab
            | terminput::KeyCode::Char('\t')
            | terminput::KeyCode::Char('l') => app.next(),
            terminput::KeyCode::Left | terminput::KeyCode::Char('h') => app.prev(),
            terminput::KeyCode::Char(c @ '1'..='9') => {
                app.tab = (c as usize - '1' as usize).min(TABS.len() - 1);
            }
            terminput::KeyCode::Char('0') => app.tab = TAB_LOG,
            _ => {}
        }
    }

    Ok(())
}

/// Blocking serial → order fetch against the upload target's host. Draws the
/// busy state first so the screen reflects the in-flight request.
fn do_order_lookup(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    let serial = app.order.serial.trim().to_string();
    if serial.is_empty() {
        app.status = "set a serial first (press 'e')".into();
        return Ok(());
    }
    if app.target.is_empty() {
        app.status = "set a target first (Network tab, 'e')".into();
        return Ok(());
    }
    app.order.state = order::LookupState::Busy;
    terminal.draw(|frame| render(frame, app))?;
    let path = format!(
        "/api/v1/qc/order-by-serial/{}",
        order::encode_path_segment(&serial)
    );
    logln(format!("order: GET {path}"));
    app.order.state = match http_get_json(&app.target, &path) {
        Ok((code, body)) => match order::parse_response(&body) {
            Ok(resp) => order::LookupState::Done(Box::new(resp)),
            Err(e) => order::LookupState::Failed(format!("HTTP {code}: {e}")),
        },
        Err(e) => order::LookupState::Failed(e),
    };
    Ok(())
}

/// One register + poll + execute + ack cycle against the orchestrator command
/// queue, keyed by this machine's SMBIOS serial (mirrors qc-app's fleet client).
fn agent_poll(
    app: &mut App,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
) -> Result<()> {
    let serial = effective_serial(&app.info);
    if serial.is_empty() {
        app.status = "agent: no usable serial".into();
        return Ok(());
    }
    if app.target.is_empty() {
        app.status = "agent: set a target first ('e')".into();
        return Ok(());
    }
    app.agent_ran = true;
    let mid = order::encode_path_segment(&serial);
    app.status = "agent: registering + polling...".into();
    terminal.draw(|frame| render(frame, app))?;

    let reg = format!(
        "{{\"machine_id\":{},\"agent_version\":{}}}",
        jq(&serial),
        jq(env!("CARGO_PKG_VERSION"))
    );
    let _ = http_post_json(&app.target, "/api/v1/qc/register", reg.as_bytes());

    let cmds = match http_get_json(&app.target, &format!("/api/v1/qc/agents/{mid}/commands")) {
        Ok((200, body)) => parse_commands(&body),
        Ok((code, _)) => {
            app.status = format!("agent: poll HTTP {code}");
            return Ok(());
        }
        Err(e) => {
            app.status = format!("agent: poll failed: {e}");
            return Ok(());
        }
    };
    if cmds.is_empty() {
        app.status = "agent: no pending commands".into();
        return Ok(());
    }

    let mut ran = 0usize;
    for cmd in &cmds {
        // A destructive op acks itself before its point of no return; acking
        // again here would be harmless but the outcome says when it is needed.
        if execute_command(app, cmd, terminal, &mid)? == CmdOutcome::NeedsAck {
            ack_command(&app.target, &mid, &cmd.id);
        }
        ran += 1;
    }
    app.status = format!("agent: executed {ran} command(s)");
    Ok(())
}

/// Whether the caller still owes the server an ack for a command.
#[derive(PartialEq, Eq)]
enum CmdOutcome {
    NeedsAck,
    Acked,
}

fn ack_command(target: &str, mid: &str, id: &str) {
    let ack = format!("{{\"command_id\":{}}}", jq(id));
    let _ = http_post_json(target, &format!("/api/v1/qc/agents/{mid}/ack"), ack.as_bytes());
}

/// One pending fleet command. `kind` is `"send_report"` or `{"custom":{"payload":…}}`.
#[derive(serde::Deserialize)]
struct AgentCommand {
    #[serde(default)]
    id: String,
    #[serde(default)]
    kind: serde_json::Value,
}

fn parse_commands(body: &[u8]) -> Vec<AgentCommand> {
    serde_json::from_slice(body).unwrap_or_default()
}

/// Execute one fleet command: `send_report` re-uploads the fingerprint; custom
/// ops drive a fingerprint refresh or a stress run.
fn execute_command(
    app: &mut App,
    cmd: &AgentCommand,
    terminal: &mut Terminal<ratatui_uefi::UefiOutputBackend>,
    mid: &str,
) -> Result<CmdOutcome> {
    if cmd.kind.as_str() == Some("send_report") {
        let _ = upload_fingerprint(app);
        return Ok(CmdOutcome::NeedsAck);
    }
    match cmd.kind.pointer("/custom/payload/op").and_then(|v| v.as_str()) {
        Some("fingerprint") | Some("send_report") => {
            let _ = upload_fingerprint(app);
        }
        Some("run_stress_preset") => {
            app.stress.start_preset();
            terminal.draw(|frame| render(frame, app))?;
        }
        Some("run_stress_stage") => {
            if let Some(idx) = cmd.kind.pointer("/custom/payload/stage").and_then(|v| v.as_u64()) {
                if let Some(stage) = stress::STAGES.get(idx as usize) {
                    app.stress.start_single(*stage);
                }
            }
        }
        // Flash a BIOS capsule. Requires url + confirm:true, and an
        // expected_version unless force:true. On success it resets and never
        // returns, so the ack is posted before the point of no return.
        Some("bios_update") => {
            let url = cmd.kind.pointer("/custom/payload/url").and_then(|v| v.as_str());
            let confirm = cmd
                .kind
                .pointer("/custom/payload/confirm")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let force = cmd
                .kind
                .pointer("/custom/payload/force")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let expected = cmd
                .kind
                .pointer("/custom/payload/expected_version")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);

            let Some(url) = url.filter(|_| confirm) else {
                app.status = "bios_update ignored (needs url + confirm:true)".into();
                return Ok(CmdOutcome::NeedsAck);
            };
            if app.stress.is_active() {
                app.status = "bios_update refused: stress test running".into();
                logln("bios_update: refused, stress active".into());
                return Ok(CmdOutcome::NeedsAck);
            }
            match capsule::preflight(&app.info.esrt, &app.info.power, expected, force) {
                Ok(warns) => {
                    for w in warns {
                        logln(format!("bios_update: {w}"));
                    }
                }
                Err(e) => {
                    app.status = format!("bios_update rejected: {e}");
                    logln(format!("bios_update: {e}"));
                    return Ok(CmdOutcome::NeedsAck);
                }
            }

            app.status = format!("bios_update: fetching {url} ...");
            terminal.draw(|frame| render(frame, app))?;
            let target = app.target.clone();
            let bytes = match download_capsule(&target, url) {
                Ok(b) => b,
                Err(e) => {
                    app.status = format!("capsule download failed: {e}");
                    return Ok(CmdOutcome::NeedsAck);
                }
            };
            if let Err(e) = capsule::inspect(&bytes) {
                app.status = format!("capsule malformed: {e}");
                return Ok(CmdOutcome::NeedsAck);
            }
            // The orchestrator publishes a digest with every capsule; the LAN
            // transport is plaintext, so check it before the point of no return.
            match cmd.kind.pointer("/custom/payload/sha256").and_then(|v| v.as_str()) {
                Some(want) => {
                    if let Err(e) = capsule::verify_sha256(&bytes, want) {
                        app.status = format!("capsule integrity check failed: {e}");
                        logln(format!("bios_update: {e}"));
                        return Ok(CmdOutcome::NeedsAck);
                    }
                    logln("bios_update: sha256 verified".into());
                }
                None if force => logln("bios_update: no sha256 supplied (forced)".into()),
                None => {
                    app.status = "bios_update rejected: payload has no sha256".into();
                    return Ok(CmdOutcome::NeedsAck);
                }
            }

            // Ack now: apply_capsule resets on success and would otherwise leave
            // the command Pending forever, re-flashing on every later poll.
            app.status = format!("bios_update: applying {} bytes", bytes.len());
            terminal.draw(|frame| render(frame, app))?;
            ack_command(&target, mid, &cmd.id);

            if let Err(e) = capsule::apply_capsule(&bytes, &app.info.esrt) {
                app.status = format!("BIOS update failed: {e}");
                logln(format!("bios_update: {e}"));
            }
            return Ok(CmdOutcome::Acked);
        }
        // Fetch a WASM plugin by URL and invoke a tool on it in firmware.
        Some("run_plugin") => {
            let url = cmd.kind.pointer("/custom/payload/url").and_then(|v| v.as_str());
            let tool = cmd
                .kind
                .pointer("/custom/payload/tool")
                .and_then(|v| v.as_str())
                .unwrap_or("selftest");
            let args = cmd
                .kind
                .pointer("/custom/payload/args")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());
            match url {
                Some(url) => {
                    let target = app.target.clone();
                    match http_get_url(&target, url) {
                        Ok((200, bytes)) => {
                            run_wasm_plugin(app, &bytes, Some(tool), &args);
                        }
                        Ok((code, _)) => app.status = format!("run_plugin: HTTP {code}"),
                        Err(e) => app.status = format!("run_plugin fetch failed: {e}"),
                    }
                }
                None => app.status = "run_plugin needs a url".into(),
            }
        }
        // Start/stop pre-boot TUI streaming; optional target override.
        Some("stream_start") => {
            if let Some(t) = cmd.kind.pointer("/custom/payload/target").and_then(|v| v.as_str()) {
                app.target = t.to_string();
                app.target_src = TargetSource::Operator;
                relay_gate::reset();
            }
            app.streaming = true;
            app.stream_frame = 0;
            app.stream_throttle = stream::Throttle::new();
            app.status = format!("streaming to {}", app.target);
        }
        Some("stream_stop") => {
            app.streaming = false;
            app.status = "streaming stopped".into();
        }
        // Inject a remote keystroke into the input loop (viewer control).
        Some("preboot_key") => {
            use tcp_protocol::preboot::{PbKeyCode, PreBootEvent, PreBootKey};
            let p = &cmd.kind;
            let code = if let Some(ch) = p
                .pointer("/custom/payload/char")
                .and_then(|v| v.as_str())
                .and_then(|s| s.chars().next())
            {
                Some(PbKeyCode::Char(ch))
            } else {
                match p.pointer("/custom/payload/named").and_then(|v| v.as_str()) {
                    Some("Enter") => Some(PbKeyCode::Enter),
                    Some("Esc") => Some(PbKeyCode::Esc),
                    Some("Backspace") => Some(PbKeyCode::Backspace),
                    Some("Tab") => Some(PbKeyCode::Tab),
                    Some("Up") => Some(PbKeyCode::Up),
                    Some("Down") => Some(PbKeyCode::Down),
                    Some("Left") => Some(PbKeyCode::Left),
                    Some("Right") => Some(PbKeyCode::Right),
                    _ => None,
                }
            };
            if let Some(code) = code {
                let key = PreBootKey {
                    code,
                    ctrl: p.pointer("/custom/payload/ctrl").and_then(|v| v.as_bool()).unwrap_or(false),
                    alt: p.pointer("/custom/payload/alt").and_then(|v| v.as_bool()).unwrap_or(false),
                    shift: p.pointer("/custom/payload/shift").and_then(|v| v.as_bool()).unwrap_or(false),
                };
                if let Some(ev) = stream::event_to_terminput(&PreBootEvent::Key(key)) {
                    app.injected.push(ev);
                    app.status = "remote key injected".into();
                }
            }
        }
        _ => {}
    }
    Ok(CmdOutcome::NeedsAck)
}

/// Send the current fingerprint (with stress summary) to the upload target,
/// picking the transport like the `p` action.
fn upload_fingerprint(app: &App) -> Result<String, String> {
    let u = parse_upload_url(&app.target);
    let json = fingerprint_with_stress(&app.info, app.stress.summary_json());
    if u.is_qc_tcp {
        net_tcp::send_qc(&u.host_port, json.as_bytes())
    } else if u.needs_efi_http {
        http_efi::post(&u.full, json.as_bytes())
    } else {
        net_tcp::post(&u.host_port, &u.path, json.as_bytes())
    }
}

fn pause() {
    use std::io::Read;
    println!("\n[ press any key to exit ]");
    let _ = std::io::stdin().read(&mut [0u8; 1]);
}

fn setup_panic_handler() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(location) = info.location() {
            logln(format!("PANIC at {}:{}", location.file(), location.line()));
            println!("Panic at {}:{}", location.file(), location.line());
        } else {
            println!("Panic occurred but no location information available.");
        }
        pause();
    }));
}

fn main() {
    setup_panic_handler();
    setup_uefi_crate();

    // Route the `log` facade into our in-memory buffer so the uefi crate's own
    // debug!/trace! output (HTTP/IP4 internals) shows up on the Log tab.
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
    logln("app start".into());

    match run() {
        Ok(()) => {}
        Err(e) => {
            logln(format!("run() error: {e:?}"));
            println!("!!! error: {e:?}");
            pause();
        }
    }
}
