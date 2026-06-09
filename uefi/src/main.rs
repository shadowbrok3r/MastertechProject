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
use uefi::boot::{MemoryType, OpenProtocolAttributes, OpenProtocolParams};
use uefi::mem::memory_map::MemoryMap;
use uefi::proto::console;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::network::ip4config2::Ip4Config2;
use uefi::proto::network::snp::SimpleNetwork;
use uefi::proto::pci::root_bridge::PciRootBridgeIo;
use uefi::table::cfg::ConfigTableEntry;

/// Mastertech "OLED" palette, mapped to the 16 EFI text-console colors.
///
/// The UEFI SimpleTextOutput backend cannot render true RGB, so the color
/// scheme's indigo/lavender/magenta are approximated by the nearest ANSI
/// colors. The one value that maps exactly is the most important for the OLED
/// look: a pure-black `#000000` background == EFI `Black`.
mod palette {
    use ratatui::style::Color;
    pub const BG: Color = Color::Black; // panel_fill / window_fill #000000
    pub const TEXT: Color = Color::White; // override_text_color ~#E8E8E8
    pub const MUTED: Color = Color::Gray; // borders / section headers / footer
    pub const LABEL: Color = Color::Cyan; // field labels
    pub const ACCENT: Color = Color::LightMagenta; // purple/magenta identity
    pub const GOOD: Color = Color::LightCyan; // present / yes / link up
    pub const BAD: Color = Color::DarkGray; // absent / no
    pub const ERR: Color = Color::LightRed; // error / link down
}

/// Default upload endpoint, baked from the workspace `.env` ORCHESTRATOR_URL at
/// build time (see build.rs). Falls back to the production URL.
const DEFAULT_URL: &str = env!("ORCHESTRATOR_URL");

const TABS: [&str; 8] = [
    "Overview",
    "System",
    "Memory",
    "Firmware",
    "Network",
    "Storage",
    "Readiness",
    "Log",
];

/// In-memory ring log. Single-threaded app, but a Mutex keeps it simple and
/// also lets us back the `log` facade (so the uefi crate's own debug!/trace!
/// messages land here too).
static LOG: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn logln(s: String) {
    if let Ok(mut g) = LOG.lock() {
        g.push(s);
        let n = g.len();
        if n > 600 {
            g.drain(0..n - 500);
        }
    }
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
    // Output: use the same device handle that rendered correctly before
    // (get_handle_for_protocol). Routing output through the ConSplitter
    // (stdout_handle) breaks clear()/mode reporting on some firmware.
    let output_handle = uefi::boot::get_handle_for_protocol::<console::text::Output>()?;
    let output = uefi::boot::open_protocol_exclusive::<console::text::Output>(output_handle)?;

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

    let terminal = Terminal::new(ratatui_uefi::UefiOutputBackend::new(output))?;
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
    mem_max_bytes: u64,
    mem_slots: u16,
    mem_ecc: String,
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
                s.chassis_type = chassis_name(st.u8_at(0x05)).to_string();
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
        return out;
    };
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
            Err(_) => continue,
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

/// Ask the firmware to (re)bind drivers to every controller, recursively. This
/// is the equivalent of the UEFI shell's `connect -r`, and can make SNP appear
/// for a NIC that has a UEFI driver which BDS simply hadn't connected (e.g. a
/// NIC that isn't in the boot order). It cannot conjure a driver that the
/// firmware doesn't have (i.e. when the network stack is disabled).
fn connect_all_controllers() {
    if let Ok(handles) = uefi::boot::locate_handle_buffer(uefi::boot::SearchType::AllHandles) {
        for &h in handles.iter() {
            let _ = uefi::boot::connect_controller(h, None, None, true);
        }
    }
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
fn run_dhcp() -> (Vec<IfaceIp>, String) {
    let mut out = Vec::new();
    let handles = match uefi::boot::find_handles::<Ip4Config2>() {
        Ok(h) => h,
        Err(e) => {
            logln(format!("dhcp: find Ip4Config2 ERR {e:?}"));
            return (out, format!("no IP4 stack: {e:?}"));
        }
    };
    logln(format!("dhcp: Ip4Config2 handles={}", handles.len()));
    if handles.is_empty() {
        return (out, "no DHCP-capable interfaces".into());
    }
    let mut last_err = String::new();
    for (i, h) in handles.into_iter().enumerate() {
        if let Ok(mut cfg) = Ip4Config2::new(h) {
            logln(format!("dhcp: if{i} ifup..."));
            if let Err(e) = cfg.ifup() {
                last_err = format!("{e:?}");
                logln(format!("dhcp: if{i} ifup ERR {e:?}"));
            }
            if let Ok(info) = cfg.get_interface_info() {
                let ip = ip_str(info.station_addr.0);
                logln(format!("dhcp: if{i} addr={ip}"));
                out.push(IfaceIp {
                    ip,
                    mask: ip_str(info.subnet_mask.0),
                });
            }
        }
    }
    let got = out.iter().any(|i| i.ip != "0.0.0.0");
    let status = if got {
        "DHCP: lease acquired".into()
    } else if last_err.is_empty() {
        "DHCP: no address".into()
    } else {
        format!("DHCP: {last_err}")
    };
    (out, status)
}

fn jq(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
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
    for (i, dk) in info.disks.iter().enumerate() {
        if i > 0 {
            disks.push(',');
        }
        disks.push_str(&format!(
            "{{\"capacity_bytes\":{},\"removable\":{},\"bus\":{},\"drive_type\":{}}}",
            dk.capacity,
            dk.removable,
            jq(dk.bus),
            jq(drive_type(dk.bus))
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
        nvme.push_str(&format!(
            "{{\"model\":{},\"serial\":{},\"firmware\":{}}}",
            jq(&d.model),
            jq(&d.serial),
            jq(&d.firmware)
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

    format!(
        concat!(
            "{{\"system\":{{\"manufacturer\":{},\"product\":{},\"version\":{},\"serial\":{},",
            "\"uuid\":{},\"sku\":{},\"family\":{}}},",
            "\"baseboard\":{{\"manufacturer\":{},\"product\":{},\"version\":{},\"serial\":{},\"asset_tag\":{}}},",
            "\"cpu\":{{\"model\":{},\"socket\":{},\"cores\":{},\"threads\":{},\"features\":[{}]}},",
            "\"gpu\":[{}],",
            "\"memory\":{{\"total_bytes\":{},\"usable_bytes\":{},\"max_bytes\":{},\"slots\":{},\"ecc\":{},\"dimms\":[{}]}},",
            "\"storage\":[{}],\"nvme\":[{}],",
            "\"firmware\":{{\"uefi_vendor\":{},\"uefi_spec\":{},\"bios_vendor\":{},\"bios_version\":{},\"bios_date\":{},\"chassis\":{},\"rtc\":{}}},",
            "\"security\":{{\"secure_boot_capable\":{},\"secure_boot_enabled\":{},\"tpm\":{},\"msdm_present\":{},\"msdm_key\":{}}},",
            "\"win11_ready\":{},",
            "\"macs\":[{}]}}"
        ),
        jq(&d.sys_mfr),
        jq(&d.sys_product),
        jq(&d.sys_version),
        jq(&d.sys_serial),
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
    )
}

/// Parsed upload target.
struct UploadUrl {
    /// Full normalized URL incl scheme/host/port/path (for EFI HTTP).
    full: String,
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
        host_port: format!("{host}:{port}"),
        path,
        needs_efi_http: scheme == "https" || (scheme == "http" && !is_ipv4),
        is_qc_tcp: scheme == "tcp",
    }
}

/// HTTP(S) POST via the EFI HTTP protocol — used for `https://` (TLS) and for
/// hostnames (DNS), both of which the raw-TCP4 path can't do. Plain http:// is
/// blocked by firmware policy, but https:// is allowed.
mod http_efi {
    use crate::logln;
    use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
    use uefi::proto::network::http::{HttpBinding, HttpHelper};
    use uefi_raw::protocol::network::http::HttpMethod;

    pub fn post(url: &str, body: &[u8]) -> Result<String, String> {
        logln(format!("http(efi): POST {url} ({}B)", body.len()));
        let handles = boot::find_handles::<HttpBinding>().map_err(|e| {
            logln(format!("http(efi): no HTTP service ({e:?})"));
            format!("no EFI HTTP service ({e:?})")
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

    /// HTTP POST to `http://target/path` over raw TCP4.
    pub fn post(target: &str, path: &str, body: &[u8]) -> Result<String, String> {
        run(target, path, body, false)
    }

    /// Send the fingerprint as a single length-prefixed frame to the
    /// axum_server QC listener (Mastertech "connected client" path).
    pub fn send_qc(target: &str, body: &[u8]) -> Result<String, String> {
        run(target, "", body, true)
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
        let st = unsafe { pump(tcp_ptr, &ct.completion_token.status, 10_000) };
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
        let st = unsafe { pump(tcp_ptr, &txtok.completion_token.status, 10_000) };
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
}

fn ascii_field(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim().to_string()
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
        // Identify Controller: admin opcode 0x06, CNS=1 in CDW10, 4 KiB result.
        let builder =
            NvmeRequestBuilder::new(pt.io_align(), 0x06, NvmeQueueType::ADMIN).with_cdw10(1);
        let req = match builder.with_transfer_buffer(4096) {
            Ok(b) => b.build(),
            Err(_) => continue,
        };
        let mut ns = pt.controller();
        let resp = match ns.execute_command(req) {
            Ok(r) => r,
            Err(e) => {
                logln(format!("nvme: IDENTIFY ERR {e:?}"));
                continue;
            }
        };
        if let Some(buf) = resp.transfer_buffer() {
            if buf.len() >= 72 {
                out.push(NvmeDrive {
                    serial: ascii_field(&buf[4..24]),
                    model: ascii_field(&buf[24..64]),
                    firmware: ascii_field(&buf[64..72]),
                });
            }
        }
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
    unsafe {
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
    gpus: Vec<Gpu>,
    secure_boot: Option<bool>,
    tpm: Option<String>,
    cpu_feats: Vec<&'static str>,
    msdm: bool,
    msdm_key: String,
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
        } else {
            info.rtc = "unavailable".into();
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

        info.gop = (|| {
            let h = uefi::boot::get_handle_for_protocol::<GraphicsOutput>().ok()?;
            let gop = uefi::boot::open_protocol_exclusive::<GraphicsOutput>(h).ok()?;
            let (w, ht) = gop.current_mode_info().resolution();
            Some(format!("{w} x {ht}"))
        })()
        .unwrap_or_else(|| "none".into());

        info.dmi = collect_smbios();
        info.nics = collect_nics();
        info.pci_net = collect_pci_net();
        info.disks = collect_storage();
        info.nvme = collect_nvme();
        info.gpus = collect_gpus();
        info.secure_boot = secure_boot();
        info.tpm = tpm_version();
        info.cpu_feats = cpu_features();
        let (msdm, key) = collect_msdm();
        info.msdm = msdm;
        info.msdm_key = key;
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
        .border_style(Style::default().fg(palette::MUTED).bg(palette::BG))
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
                "{:<12}{:<11}{:<11}{:<8}{:<18}{:<20}{}",
                "Slot", "Size", "Speed", "Type", "Manufacturer", "Part #", "Serial"
            ),
            Style::default().fg(palette::LABEL),
        )));
        for m in &info.dmi.dimms {
            let speed = if m.speed == 0 {
                "?".to_string()
            } else {
                format!("{} MT/s", m.speed)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<12}", m.locator), Style::default().fg(palette::ACCENT)),
                Span::styled(
                    format!("{:<11}{:<11}{:<8}{:<18}{:<20}{}", m.size, speed, m.mtype, m.mfr, m.part, m.serial),
                    Style::default().fg(palette::TEXT),
                ),
            ]));
        }
    }
    frame.render_widget(para(lines, "Memory"), area);
}

fn page_firmware(frame: &mut Frame, area: Rect, info: &SysInfo) {
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
    frame.render_widget(para(lines, "Firmware & Tables"), area);
}

fn page_network(frame: &mut Frame, area: Rect, app: &App) {
    let info = &app.info;
    let mut lines = Vec::new();

    // UEFI interfaces (only present when a network driver is bound).
    lines.push(header("UEFI interfaces (SimpleNetwork)"));
    if info.nics.is_empty() {
        lines.push(Line::from(Span::styled(
            "none - no UEFI network driver is bound",
            Style::default().fg(palette::ERR),
        )));
    } else {
        for (i, n) in info.nics.iter().enumerate() {
            let link = if !n.media_supported {
                Span::styled("link n/a", Style::default().fg(palette::MUTED))
            } else if n.media_present {
                Span::styled("link up", Style::default().fg(palette::GOOD))
            } else {
                Span::styled("link down", Style::default().fg(palette::ERR))
            };
            lines.push(Line::from(vec![
                Span::styled(format!("NIC {i}  "), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("{}  ", n.mac), Style::default().fg(palette::TEXT)),
                link,
                Span::styled(
                    format!("  type {}  MTU {}  {}", n.if_type, n.mtu, n.state),
                    Style::default().fg(palette::MUTED),
                ),
            ]));
        }
    }

    // Raw PCI hardware (works with no driver — proves the NIC is physically present).
    lines.push(Line::from(""));
    lines.push(header("Network hardware (PCI bus)"));
    if info.pci_net.is_empty() {
        lines.push(Line::from(Span::styled(
            "no PCI network controllers found",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for p in &info.pci_net {
            lines.push(Line::from(vec![
                Span::styled(format!("{}  ", p.loc), Style::default().fg(palette::ACCENT)),
                Span::styled(
                    format!("{} ", pci_vendor_name(p.vendor)),
                    Style::default().fg(palette::TEXT),
                ),
                Span::styled(
                    format!("[{:04x}:{:04x}]  ", p.vendor, p.device),
                    Style::default().fg(palette::MUTED),
                ),
                Span::styled(p.kind, Style::default().fg(palette::LABEL)),
            ]));
        }
    }

    // IP leases (after DHCP).
    lines.push(Line::from(""));
    lines.push(header("IPv4 (press 'd' for DHCP)"));
    if app.ifaces.is_empty() {
        lines.push(Line::from(Span::styled(
            "no lease yet",
            Style::default().fg(palette::MUTED),
        )));
    } else {
        for (i, f) in app.ifaces.iter().enumerate() {
            let up = f.ip != "0.0.0.0";
            lines.push(Line::from(vec![
                Span::styled(format!("if {i}  "), Style::default().fg(palette::ACCENT)),
                Span::styled(
                    format!("{:<16}", f.ip),
                    Style::default().fg(if up { palette::GOOD } else { palette::BAD }),
                ),
                Span::styled(format!("mask {}", f.mask), Style::default().fg(palette::MUTED)),
            ]));
        }
    }

    // Upload target + status.
    lines.push(Line::from(""));
    lines.push(header("Upload target"));
    if app.editing {
        lines.push(Line::from(Span::styled(
            "[ EDITING - type host or host:port, then press ENTER to save ]",
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        )));
    }
    let shown = if app.target.is_empty() {
        "<host:port>".to_string()
    } else {
        app.target.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("target: ", Style::default().fg(palette::MUTED)),
        Span::styled(shown, Style::default().fg(palette::TEXT)),
        Span::styled(
            if app.editing { "_" } else { "" },
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));
    if !app.target.is_empty() {
        let u = parse_upload_url(&app.target);
        lines.push(Line::from(vec![
            Span::styled("will POST to: ", Style::default().fg(palette::MUTED)),
            Span::styled(u.full, Style::default().fg(palette::LABEL)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("transport:    ", Style::default().fg(palette::MUTED)),
            Span::styled(
                if u.is_qc_tcp {
                    "QC TCP (framed, connected_client)"
                } else if u.needs_efi_http {
                    "EFI HTTP (DNS+TLS via firmware)"
                } else {
                    "raw TCP4 (plain http, IP only)"
                },
                Style::default().fg(palette::LABEL),
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        if app.editing {
            "ENTER save"
        } else {
            "'e' edit target    'p' send POST"
        },
        Style::default().fg(palette::MUTED),
    )));
    if !app.status.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("status: ", Style::default().fg(palette::MUTED)),
            Span::styled(app.status.clone(), Style::default().fg(palette::GOOD)),
        ]));
    }

    // Guidance.
    if info.nics.is_empty() && !info.pci_net.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "NIC present but no UEFI driver - enable the UEFI Network Stack in",
            Style::default().fg(palette::ERR),
        )));
        lines.push(Line::from(Span::styled(
            "BIOS setup, then press 'c' to connect drivers and rescan.",
            Style::default().fg(palette::ERR),
        )));
    }
    frame.render_widget(para(lines, "Network"), area);
}

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
        }
    }
    frame.render_widget(para(lines, "Storage"), area);
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
    frame.render_widget(para(lines, "Readiness"), area);
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
        .block(panel("MASTERTECH UEFI"))
        .style(Style::default().fg(palette::MUTED).bg(palette::BG))
        .highlight_style(
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled("  ", Style::default().fg(palette::MUTED)));
    frame.render_widget(tabs, root[0]);

    match app.tab {
        0 => page_overview(frame, root[1], &app.info),
        1 => page_system(frame, root[1], &app.info),
        2 => page_memory(frame, root[1], &app.info),
        3 => page_firmware(frame, root[1], &app.info),
        4 => page_network(frame, root[1], app),
        5 => page_storage(frame, root[1], &app.info),
        6 => page_readiness(frame, root[1], &app.info),
        _ => page_log(frame, root[1]),
    }

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Tab / -> ", Style::default().fg(palette::ACCENT)),
        Span::styled("next   ", Style::default().fg(palette::MUTED)),
        Span::styled("<- ", Style::default().fg(palette::ACCENT)),
        Span::styled("prev   ", Style::default().fg(palette::MUTED)),
        Span::styled("1-5 ", Style::default().fg(palette::ACCENT)),
        Span::styled("jump   ", Style::default().fg(palette::MUTED)),
        Span::styled("r ", Style::default().fg(palette::ACCENT)),
        Span::styled("refresh   ", Style::default().fg(palette::MUTED)),
        Span::styled("c ", Style::default().fg(palette::ACCENT)),
        Span::styled("connect   ", Style::default().fg(palette::MUTED)),
        Span::styled("d ", Style::default().fg(palette::ACCENT)),
        Span::styled("dhcp   ", Style::default().fg(palette::MUTED)),
        Span::styled("e/p ", Style::default().fg(palette::ACCENT)),
        Span::styled("target/post   ", Style::default().fg(palette::MUTED)),
        Span::styled("q ", Style::default().fg(palette::ACCENT)),
        Span::styled("quit", Style::default().fg(palette::MUTED)),
    ]))
    .centered()
    .style(base_style())
    .block(panel(""));
    frame.render_widget(footer, root[2]);
}

struct App {
    info: SysInfo,
    tab: usize,
    target: String,
    editing: bool,
    ifaces: Vec<IfaceIp>,
    status: String,
}

impl App {
    fn next(&mut self) {
        self.tab = (self.tab + 1) % TABS.len();
    }
    fn prev(&mut self) {
        self.tab = (self.tab + TABS.len() - 1) % TABS.len();
    }
}

fn run() -> Result<()> {
    init_ratatui_perf();
    let (mut terminal, mut input_reader) = create_ui()?;
    let mut app = App {
        info: SysInfo::collect(),
        tab: 0,
        target: DEFAULT_URL.to_string(),
        editing: false,
        ifaces: Vec::new(),
        status: String::new(),
    };

    terminal.clear()?;

    // Auto-acquire DHCP on boot if a NIC with link (or unknown link state) is
    // present. Skips interfaces reporting link-down so a disconnected Wi-Fi NIC
    // doesn't stall boot on the 30s ifup timeout.
    if app
        .info
        .nics
        .iter()
        .any(|n| !n.media_supported || n.media_present)
    {
        app.status = "boot: acquiring DHCP...".into();
        terminal.draw(|frame| render(frame, &app))?;
        let (ifaces, status) = run_dhcp();
        app.ifaces = ifaces;
        app.status = format!("boot DHCP: {status}");
        logln(format!("boot DHCP: {status}"));
    }

    loop {
        terminal.draw(|frame| render(frame, &app))?;

        let Some(terminput::Event::Key(key)) = input_reader.read_event()? else {
            continue;
        };
        logln(format!("key={:?} editing={} tab={}", key.code, app.editing, app.tab));

        // While editing the upload target, all printable keys feed the field.
        if app.editing {
            match key.code {
                terminput::KeyCode::Enter | terminput::KeyCode::Esc => app.editing = false,
                terminput::KeyCode::Backspace => {
                    app.target.pop();
                }
                terminput::KeyCode::Char(c) if c == '\u{8}' || c == '\u{7f}' => {
                    app.target.pop();
                }
                terminput::KeyCode::Char(c) if !c.is_control() => app.target.push(c),
                _ => {}
            }
            continue;
        }

        match key.code {
            terminput::KeyCode::Char('q') | terminput::KeyCode::Esc => break,
            terminput::KeyCode::Char('r') => app.info = SysInfo::collect(),
            terminput::KeyCode::Char('c') => {
                connect_all_controllers();
                app.info = SysInfo::collect();
                app.status = "connect: rescanned controllers".into();
            }
            terminput::KeyCode::Char('d') => {
                app.status = "DHCP: working (up to 30s)...".into();
                terminal.draw(|frame| render(frame, &app))?;
                let (ifaces, status) = run_dhcp();
                app.ifaces = ifaces;
                app.status = status;
            }
            terminput::KeyCode::Char('e') => app.editing = true,
            terminput::KeyCode::Char('p') => {
                logln(format!("POST key: target='{}'", app.target));
                if app.target.is_empty() {
                    app.status = "set a target first (press 'e')".into();
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
                    let json = fingerprint_json(&app.info);
                    let result = if u.is_qc_tcp {
                        net_tcp::send_qc(&u.host_port, json.as_bytes())
                    } else if u.needs_efi_http {
                        http_efi::post(&u.full, json.as_bytes())
                    } else {
                        net_tcp::post(&u.host_port, &u.path, json.as_bytes())
                    };
                    app.status = match result {
                        Ok(s) => format!("OK: {s}"),
                        Err(e) => format!("upload failed: {e}"),
                    };
                }
            }
            terminput::KeyCode::Right
            | terminput::KeyCode::Tab
            | terminput::KeyCode::Char('\t')
            | terminput::KeyCode::Char('l') => app.next(),
            terminput::KeyCode::Left | terminput::KeyCode::Char('h') => app.prev(),
            terminput::KeyCode::Char(c @ '1'..='8') => {
                app.tab = (c as usize - '1' as usize).min(TABS.len() - 1);
            }
            _ => {}
        }
    }

    Ok(())
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
