//! NVIDIA Aftermath / Unreal Engine GPU crash contexts.
//!
//! A `.nv-gpudmp` embeds the whole `FGenericCrashContext` XML as plain text, so
//! the D3D12 device-removed reason, the GPU driver identity, and the RHI
//! breadcrumb tree read without the Aftermath SDK. This module parses that XML
//! only; the client-side plugin does the file discovery and the byte-range
//! extraction. No regex, no new dependency, no `kdmp` use, so the same code
//! compiles for wasm32 inside `database`.

use serde::{Deserialize, Serialize};

/// `crash_sighting.dump_kind` written for these dumps.
pub const GPU_AFTERMATH_DUMP_KIND: &str = "gpu_aftermath";

const CTX_OPEN: &str = "<FGenericCrashContext";
const CTX_CLOSE: &str = "</FGenericCrashContext>";
const MAX_BREADCRUMB_DEPTH: usize = 128;
const MAX_BREADCRUMB_NODES: usize = 20_000;

pub const DXGI_ERROR_INVALID_CALL: u32 = 0x887A_0001;
pub const DXGI_ERROR_DEVICE_REMOVED: u32 = 0x887A_0005;
pub const DXGI_ERROR_DEVICE_HUNG: u32 = 0x887A_0006;
pub const DXGI_ERROR_DEVICE_RESET: u32 = 0x887A_0007;
pub const DXGI_ERROR_DRIVER_INTERNAL_ERROR: u32 = 0x887A_0020;

/// GPU-crash triage extracted from an embedded Unreal crash context. Persisted
/// verbatim as `crash_sighting.triage`; `kind` marks it as a GPU blob so a
/// reader never deserializes it as a `KernelDumpTriage`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GpuCrashDump {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub dump_name: Option<String>,
    /// UE crash folder, e.g. `UECC-Windows-<GUID>_0000`.
    #[serde(default)]
    pub crash_folder: Option<String>,
    #[serde(default)]
    pub crash_type: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    /// Signed int32 exactly as the XML carries it.
    #[serde(default)]
    pub dxgi_reason_raw: Option<i64>,
    /// Low 32 bits in `0x887a0007` form; None when zero or absent.
    #[serde(default)]
    pub dxgi_reason: Option<String>,
    #[serde(default)]
    pub dxgi_reason_name: Option<String>,
    #[serde(default)]
    pub aftermath: bool,
    #[serde(default)]
    pub rhi_name: Option<String>,
    #[serde(default)]
    pub gpu_adapter_name: Option<String>,
    #[serde(default)]
    pub gpu_vendor_id: Option<u32>,
    #[serde(default)]
    pub gpu_device_id: Option<String>,
    #[serde(default)]
    pub gpu_driver_version: Option<String>,
    #[serde(default)]
    pub gpu_driver_internal_version: Option<String>,
    #[serde(default)]
    pub gpu_driver_date: Option<String>,
    /// Kernel-mode display driver stem inferred from the vendor/adapter.
    #[serde(default)]
    pub gpu_driver_module: String,
    /// ACTIVE breadcrumb chain, outermost first. The last entry is where the
    /// GPU stopped executing.
    #[serde(default)]
    pub breadcrumb_active: Vec<String>,
    #[serde(default)]
    pub breadcrumbs_raw: Option<String>,
    #[serde(default)]
    pub is_stuck: Option<bool>,
    #[serde(default)]
    pub stuck_thread_id: Option<i64>,
    #[serde(default)]
    pub engine_version: Option<String>,
    #[serde(default)]
    pub game_name: Option<String>,
    #[serde(default)]
    pub process_name: Option<String>,
    #[serde(default)]
    pub map_name: Option<String>,
    #[serde(default)]
    pub gi_quality: Option<String>,
    #[serde(default)]
    pub use_nanite: Option<String>,
    #[serde(default)]
    pub seconds_since_start: Option<i64>,
    #[serde(default)]
    pub cpu_microcode_revision: Option<i64>,
    #[serde(default)]
    pub crash_guid: Option<String>,
    #[serde(default)]
    pub dump_time: Option<String>,
}

impl GpuCrashDump {
    /// True when this blob really came from a GPU crash context.
    pub fn is_gpu_crash(&self) -> bool {
        self.kind == GPU_AFTERMATH_DUMP_KIND
            || self.dxgi_reason.is_some()
            || self.aftermath
            || matches!(self.crash_type.as_deref(), Some(c) if c.eq_ignore_ascii_case("gpucrash"))
    }

    /// Driver-version marker for the signature's `module_versions` rollup.
    pub fn module_version(&self) -> Option<String> {
        match (
            self.gpu_driver_version.as_deref(),
            self.gpu_driver_internal_version.as_deref(),
        ) {
            (Some(u), Some(i)) => Some(format!("{u} ({i})")),
            (Some(u), None) => Some(u.to_string()),
            (None, Some(i)) => Some(i.to_string()),
            (None, None) => None,
        }
    }

    /// Innermost ACTIVE breadcrumb node.
    pub fn deepest_breadcrumb(&self) -> Option<String> {
        self.breadcrumb_active.last().cloned()
    }

    /// One-line account used as the sighting's `raw_excerpt`.
    pub fn summary(&self) -> String {
        format!(
            "{} ({}) adapter: {} | driver: {} / {} | breadcrumb: {} | stuck: {} [gpu-aftermath]",
            self.dxgi_reason_name.as_deref().unwrap_or("GPU crash"),
            self.dxgi_reason.as_deref().unwrap_or("-"),
            self.gpu_adapter_name.as_deref().unwrap_or("-"),
            self.gpu_driver_version.as_deref().unwrap_or("-"),
            self.gpu_driver_internal_version.as_deref().unwrap_or("-"),
            if self.breadcrumb_active.is_empty() {
                "-".to_string()
            } else {
                self.breadcrumb_active.join(" > ")
            },
            self.is_stuck.unwrap_or(false),
        )
    }
}

/// DXGI device-removed reason name for an HRESULT-shaped code.
pub fn dxgi_reason_name(code: u32) -> &'static str {
    match code {
        DXGI_ERROR_INVALID_CALL => "DXGI_ERROR_INVALID_CALL",
        0x887A_0002 => "DXGI_ERROR_NOT_FOUND",
        0x887A_0004 => "DXGI_ERROR_UNSUPPORTED",
        DXGI_ERROR_DEVICE_REMOVED => "DXGI_ERROR_DEVICE_REMOVED",
        DXGI_ERROR_DEVICE_HUNG => "DXGI_ERROR_DEVICE_HUNG",
        DXGI_ERROR_DEVICE_RESET => "DXGI_ERROR_DEVICE_RESET",
        DXGI_ERROR_DRIVER_INTERNAL_ERROR => "DXGI_ERROR_DRIVER_INTERNAL_ERROR",
        0x887A_0026 => "DXGI_ERROR_WAIT_TIMEOUT",
        _ => "DXGI_ERROR_UNKNOWN",
    }
}

/// Parse a reason written as a signed int32, an unsigned int, or `0x`-hex.
pub fn parse_dxgi_reason(raw: &str) -> Option<u32> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    s.parse::<i64>().ok().map(|v| v as u32)
}

/// Kernel-mode display driver stem for a GPU vendor id / adapter string.
pub fn gpu_kernel_module(vendor_id: Option<u32>, adapter_name: &str) -> &'static str {
    match vendor_id {
        Some(0x10DE) => return "nvlddmkm",
        Some(0x1002) | Some(0x1022) => return "amdkmdag",
        Some(0x8086) => return "igdkmd64",
        _ => {}
    }
    let a = adapter_name.to_ascii_lowercase();
    if a.contains("nvidia") || a.contains("geforce") || a.contains("quadro") || a.contains("rtx") {
        "nvlddmkm"
    } else if a.contains("radeon") || a.contains("amd") {
        "amdkmdag"
    } else if a.contains("intel") || a.contains(" arc") || a.contains("iris") || a.contains("uhd") {
        "igdkmd64"
    } else {
        "gpu"
    }
}

/// `D3D12.0.2026.06.18-23.01.01.nv-gpudmp` -> `06/18/2026 23:01 local`.
pub fn time_from_gpu_dump_name(name: &str) -> Option<String> {
    let (head, tail) = name.split_once('-')?;
    let date: Vec<&str> = head.split('.').collect();
    let time: Vec<&str> = tail.split('.').collect();
    if date.len() < 3 || time.len() < 2 {
        return None;
    }
    let (y, mo, d) = (
        date[date.len() - 3],
        date[date.len() - 2],
        date[date.len() - 1],
    );
    let (hh, mi) = (time[0], time[1]);
    let shaped = y.len() == 4 && mo.len() == 2 && d.len() == 2 && hh.len() == 2 && mi.len() == 2;
    let digits = [y, mo, d, hh, mi]
        .iter()
        .all(|p| p.chars().all(|c| c.is_ascii_digit()));
    if shaped && digits {
        Some(format!("{mo}/{d}/{y} {hh}:{mi} local"))
    } else {
        None
    }
}

/// Parse an `FGenericCrashContext` XML block (a `CrashContext.runtime-xml`, or
/// the block a `.nv-gpudmp` embeds). None when the context is not a GPU crash.
pub fn parse_crash_context_xml(raw: &str) -> Option<GpuCrashDump> {
    let xml = match (raw.find(CTX_OPEN), raw.rfind(CTX_CLOSE)) {
        (Some(a), Some(b)) if b > a => &raw[a..b + CTX_CLOSE.len()],
        _ => raw,
    };

    let crash_type = tag(xml, "CrashType");
    let aftermath = tag_bool(xml, &["RHI.Aftermath", "GPUCrash.Aftermath"]);
    let dxgi_reason_raw = tag_i64(
        xml,
        &[
            "GPUCrash.D3DDeviceRemovedReason",
            "D3DDeviceRemovedReason",
            "GPUCrash.DeviceRemovedReason",
            "RHI.DeviceRemovedReason",
        ],
    );
    let is_gpu = dxgi_reason_raw.is_some()
        || aftermath
        || matches!(crash_type.as_deref(), Some(c) if c.eq_ignore_ascii_case("gpucrash"));
    if !is_gpu {
        return None;
    }

    let masked = dxgi_reason_raw
        .map(|v| v as u32)
        .filter(|v| *v != 0);
    let gpu_vendor_id = any_tag(xml, &["RHI.VendorId", "GPUVendorId", "VendorId"])
        .as_deref()
        .and_then(parse_vendor_id);
    let gpu_adapter_name = any_tag(xml, &["RHI.AdapterName", "GPUAdapterName", "AdapterName"]);
    let breadcrumbs_raw = any_tag(xml, &["Breadcrumbs", "RHI.Breadcrumbs"]);
    let breadcrumb_active = breadcrumbs_raw
        .as_deref()
        .map(active_breadcrumb_path)
        .unwrap_or_default();

    Some(GpuCrashDump {
        kind: GPU_AFTERMATH_DUMP_KIND.to_string(),
        dump_name: None,
        crash_folder: None,
        crash_type,
        error_message: tag(xml, "ErrorMessage"),
        dxgi_reason_raw,
        dxgi_reason: masked.map(|v| format!("{v:#010x}")),
        dxgi_reason_name: masked.map(|v| dxgi_reason_name(v).to_string()),
        aftermath,
        rhi_name: any_tag(xml, &["RHI.RHIName", "RHIName"]),
        gpu_driver_module: gpu_kernel_module(
            gpu_vendor_id,
            gpu_adapter_name.as_deref().unwrap_or_default(),
        )
        .to_string(),
        gpu_adapter_name,
        gpu_vendor_id,
        gpu_device_id: any_tag(xml, &["RHI.DeviceId", "GPUDeviceId", "DeviceId"]),
        gpu_driver_version: any_tag(
            xml,
            &[
                "RHI.UserDriverVersion",
                "RHI.AdapterUserDriverVersion",
                "UserDriverVersion",
            ],
        ),
        gpu_driver_internal_version: any_tag(
            xml,
            &[
                "RHI.InternalDriverVersion",
                "RHI.AdapterInternalDriverVersion",
                "InternalDriverVersion",
            ],
        ),
        gpu_driver_date: any_tag(xml, &["RHI.DriverDate", "RHI.AdapterDriverDate", "DriverDate"]),
        breadcrumb_active,
        breadcrumbs_raw,
        is_stuck: any_tag(xml, &["Misc.IsStuck", "IsStuck"]).map(|v| v.eq_ignore_ascii_case("true")),
        stuck_thread_id: tag_i64(xml, &["Misc.StuckThreadId", "StuckThreadId"]),
        engine_version: tag(xml, "EngineVersion"),
        game_name: tag(xml, "GameName"),
        process_name: any_tag(xml, &["ExecutableName", "Misc.ExecutableName", "ProcessName"]),
        map_name: tag(xml, "MapName"),
        gi_quality: tag(xml, "GlobalIlluminationQuality"),
        use_nanite: tag(xml, "UseNanite"),
        seconds_since_start: tag_i64(xml, &["SecondsSinceStart"]),
        cpu_microcode_revision: tag_i64(
            xml,
            &["Misc.CPUMicrocodeRevision", "CPUMicrocodeRevision"],
        ),
        crash_guid: tag(xml, "CrashGUID"),
        dump_time: None,
    })
}

/// Longest chain of nodes marked ACTIVE (`A`), outermost first. Empty on
/// malformed input; a stale deep `A` inside a finished branch never wins.
pub fn active_breadcrumb_path(tree: &str) -> Vec<String> {
    let trimmed = tree.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut scanner = Scanner {
        b: trimmed.as_bytes(),
        i: 0,
        nodes: 0,
    };
    let mut roots: Vec<Node> = Vec::new();
    loop {
        scanner.skip_ws();
        if scanner.peek().is_none() {
            break;
        }
        match parse_node(&mut scanner, 0) {
            Some(node) => roots.push(node),
            None => break,
        }
        scanner.skip_ws();
        scanner.eat(b',');
    }
    let mut best: Vec<String> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    for root in &roots {
        walk_active(root, &mut path, &mut best);
    }
    best
}

/// Breadcrumb name with a trailing printf specifier removed:
/// `RenderGraphExecute - %s` -> `RenderGraphExecute`.
fn clean_breadcrumb_name(name: &str) -> String {
    let n = name.trim();
    let cut = n
        .char_indices()
        .find(|(i, c)| *c == '%' && is_format_spec(&n[*i..]))
        .map(|(i, _)| i);
    let head = match cut {
        Some(i) => &n[..i],
        None => n,
    };
    head.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '-' | ':' | ',' | ';' | '(' | '[' | '/' | '=')
    })
    .trim()
    .to_string()
}

/// True when a `%` begins a printf conversion specifier.
fn is_format_spec(s: &str) -> bool {
    let mut it = s.chars();
    if it.next() != Some('%') {
        return false;
    }
    let mut c = it.next();
    while matches!(c, Some(ch) if "-+#0123456789.".contains(ch)) {
        c = it.next();
    }
    while matches!(c, Some('h') | Some('l') | Some('L') | Some('z')) {
        c = it.next();
    }
    matches!(c, Some(ch) if "diouxXeEfgGaAcsp%".contains(ch))
}

/// Inner text of the first `<tag>...</tag>`, unescaped and trimmed.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let rest = &xml[start..];
    let end = rest.find(&close)?;
    let v = unescape_xml(rest[..end].trim());
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn any_tag(xml: &str, names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| tag(xml, n))
}

fn tag_bool(xml: &str, names: &[&str]) -> bool {
    matches!(any_tag(xml, names).as_deref(), Some(v) if v.eq_ignore_ascii_case("true") || v == "1")
}

fn tag_i64(xml: &str, names: &[&str]) -> Option<i64> {
    any_tag(xml, names)?.trim().parse::<i64>().ok()
}

/// PCI vendor id from the XML's hex (`10DE`) or decimal form.
fn parse_vendor_id(raw: &str) -> Option<u32> {
    const KNOWN: [u32; 4] = [0x10DE, 0x1002, 0x1022, 0x8086];
    let s = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    let hex = u32::from_str_radix(s, 16).ok();
    if matches!(hex, Some(v) if KNOWN.contains(&v)) {
        return hex;
    }
    match s.parse::<u32>() {
        Ok(v) if KNOWN.contains(&v) => Some(v),
        _ => hex,
    }
}

/// Resolve the five predefined XML entities and numeric character references.
fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let semi = after.find(';').filter(|i| *i <= 10);
        let Some(semi) = semi else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let decoded = match &after[1..semi] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            entity => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse::<u32>().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One breadcrumb node: scope name, state marker, children.
#[derive(Debug, Default)]
struct Node {
    name: String,
    active: bool,
    children: Vec<Node>,
}

/// Records the longest all-ancestors-active chain reached.
fn walk_active(node: &Node, path: &mut Vec<String>, best: &mut Vec<String>) {
    if !node.active {
        return;
    }
    path.push(clean_breadcrumb_name(&node.name));
    if path.len() > best.len() {
        *best = path.clone();
    }
    for child in &node.children {
        walk_active(child, path, best);
    }
    path.pop();
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    nodes: usize,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let hit = self.peek() == Some(byte);
        if hit {
            self.i += 1;
        }
        hit
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    /// Bytes up to the first `stops` byte, cursor left on it.
    fn take_until(&mut self, stops: &[u8]) -> &'a [u8] {
        let start = self.i;
        while let Some(c) = self.peek() {
            if stops.contains(&c) {
                break;
            }
            self.i += 1;
        }
        &self.b[start..self.i]
    }
}

/// `node := '{' '{' name '}' ',' state [ ',' '{' node (',' node)* '}' ] '}'`
fn parse_node(s: &mut Scanner, depth: usize) -> Option<Node> {
    if depth > MAX_BREADCRUMB_DEPTH || s.nodes >= MAX_BREADCRUMB_NODES {
        return None;
    }
    s.nodes += 1;
    s.skip_ws();
    if !s.eat(b'{') || !s.eat(b'{') {
        return None;
    }
    let name = String::from_utf8_lossy(s.take_until(b"}")).trim().to_string();
    if !s.eat(b'}') {
        return None;
    }

    let mut state = String::new();
    let mut at_children = false;
    if s.eat(b',') {
        s.skip_ws();
        if s.peek() == Some(b'{') {
            at_children = true;
        } else {
            state = String::from_utf8_lossy(s.take_until(b",}")).trim().to_string();
        }
    }

    let mut children = Vec::new();
    if at_children || s.eat(b',') {
        s.skip_ws();
        if !s.eat(b'{') {
            return None;
        }
        loop {
            s.skip_ws();
            match s.peek() {
                Some(b'{') => children.push(parse_node(s, depth + 1)?),
                Some(b'}') => break,
                // Truncation markers and unknown items.
                Some(_) => {
                    s.take_until(b",}");
                }
                None => return None,
            }
            s.skip_ws();
            if !s.eat(b',') {
                break;
            }
        }
        s.skip_ws();
        if !s.eat(b'}') {
            return None;
        }
    }
    s.skip_ws();
    if !s.eat(b'}') {
        return None;
    }
    Some(Node {
        name,
        active: state.eq_ignore_ascii_case("a"),
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = "{{Frame 533486},A,{{{SceneRender - ViewFamilies},A,{{{RenderGraphExecute - %s},A,{{{Scene},A,{{{VirtualTextureUpdate},A}}}}}}},...,{{EndDrawingViewport},A}}}";

    fn sample_xml() -> String {
        format!(
            "<?xml version=\"1.0\"?><FGenericCrashContext><RuntimeProperties>\
             <CrashType>GPUCrash</CrashType>\
             <ErrorMessage>Aftermath crash dump</ErrorMessage>\
             <ExecutableName>FortniteClient-Win64-Shipping</ExecutableName>\
             <EngineVersion>5.8.0-+++Fortnite+Release-41.00</EngineVersion>\
             <Misc.IsStuck>true</Misc.IsStuck>\
             <Misc.StuckThreadId>34696</Misc.StuckThreadId>\
             <Misc.CPUMicrocodeRevision>188760099</Misc.CPUMicrocodeRevision>\
             <GPUCrash.D3DDeviceRemovedReason>-2005270521</GPUCrash.D3DDeviceRemovedReason>\
             <RHI.RHIName>D3D12</RHI.RHIName>\
             <RHI.AdapterName>NVIDIA GeForce RTX 5090</RHI.AdapterName>\
             <RHI.UserDriverVersion>610.47</RHI.UserDriverVersion>\
             <RHI.InternalDriverVersion>32.0.16.1047</RHI.InternalDriverVersion>\
             <RHI.DriverDate>5-19-2026</RHI.DriverDate>\
             <RHI.DeviceId>2B85</RHI.DeviceId><RHI.VendorId>10DE</RHI.VendorId>\
             <RHI.Aftermath>true</RHI.Aftermath>\
             <SecondsSinceStart>8999</SecondsSinceStart>\
             <Breadcrumbs>{TREE}</Breadcrumbs>\
             </RuntimeProperties><GameData>\
             <MapName>Apollo_Terrain</MapName>\
             <GlobalIlluminationQuality>Lumen Epic</GlobalIlluminationQuality>\
             <UseNanite>Enabled</UseNanite>\
             </GameData></FGenericCrashContext>"
        )
    }

    #[test]
    fn masks_signed_dxgi_reason() {
        assert_eq!(parse_dxgi_reason("-2005270521"), Some(DXGI_ERROR_DEVICE_RESET));
        assert_eq!(parse_dxgi_reason("0x887A0007"), Some(DXGI_ERROR_DEVICE_RESET));
        assert_eq!(parse_dxgi_reason("not hex"), None);
        assert_eq!(dxgi_reason_name(DXGI_ERROR_DEVICE_HUNG), "DXGI_ERROR_DEVICE_HUNG");
        assert_eq!(dxgi_reason_name(0x1234), "DXGI_ERROR_UNKNOWN");
    }

    #[test]
    fn deepest_active_breadcrumb_names_the_gpu_stop() {
        let path = active_breadcrumb_path(TREE);
        assert_eq!(path.len(), 5);
        assert_eq!(path.first().map(String::as_str), Some("Frame 533486"));
        assert_eq!(path.get(2).map(String::as_str), Some("RenderGraphExecute"));
        assert_eq!(path.last().map(String::as_str), Some("VirtualTextureUpdate"));
        assert!(!path.contains(&"EndDrawingViewport".to_string()));
    }

    #[test]
    fn stale_deep_branch_never_outranks_the_live_chain() {
        let tree = "{{Root},A,{{{Finished},F,{{{DeepStale},A,{{{Deeper},A}}}}},{{Live},A}}}";
        assert_eq!(active_breadcrumb_path(tree), vec!["Root", "Live"]);
    }

    #[test]
    fn malformed_breadcrumbs_are_empty() {
        for bad in ["", "   ", "not a tree", "{{Unclosed},A", "{{{{{{"] {
            assert!(active_breadcrumb_path(bad).is_empty(), "{bad}");
        }
        assert!(active_breadcrumb_path(&"{".repeat(5000)).is_empty());
        assert!(active_breadcrumb_path(&"{{a},A,{".repeat(500)).is_empty());
        assert!(active_breadcrumb_path("{{Done},F,{{{Also},F}}}").is_empty());
        assert_eq!(active_breadcrumb_path("{{Solo},A}"), vec!["Solo"]);
        assert_eq!(active_breadcrumb_path("{{Root},A,{...,{{Kid},A},...}}"), vec!["Root", "Kid"]);
    }

    #[test]
    fn parses_the_crash_context() {
        let g = parse_crash_context_xml(&sample_xml()).expect("gpu crash");
        assert_eq!(g.kind, GPU_AFTERMATH_DUMP_KIND);
        assert_eq!(g.dxgi_reason.as_deref(), Some("0x887a0007"));
        assert_eq!(g.dxgi_reason_name.as_deref(), Some("DXGI_ERROR_DEVICE_RESET"));
        assert_eq!(g.dxgi_reason_raw, Some(-2005270521));
        assert_eq!(g.gpu_driver_module, "nvlddmkm");
        assert_eq!(g.gpu_vendor_id, Some(0x10DE));
        assert_eq!(g.gpu_device_id.as_deref(), Some("2B85"));
        assert_eq!(g.gpu_driver_version.as_deref(), Some("610.47"));
        assert_eq!(g.module_version().as_deref(), Some("610.47 (32.0.16.1047)"));
        assert_eq!(g.process_name.as_deref(), Some("FortniteClient-Win64-Shipping"));
        assert_eq!(g.is_stuck, Some(true));
        assert_eq!(g.stuck_thread_id, Some(34696));
        assert_eq!(g.cpu_microcode_revision, Some(188760099));
        assert_eq!(g.seconds_since_start, Some(8999));
        assert_eq!(g.map_name.as_deref(), Some("Apollo_Terrain"));
        assert_eq!(g.gi_quality.as_deref(), Some("Lumen Epic"));
        assert_eq!(g.use_nanite.as_deref(), Some("Enabled"));
        assert_eq!(g.deepest_breadcrumb().as_deref(), Some("VirtualTextureUpdate"));
        assert!(g.is_gpu_crash());
        assert!(g.summary().contains("DXGI_ERROR_DEVICE_RESET"));
    }

    #[test]
    fn rejects_a_non_gpu_context() {
        let xml = sample_xml()
            .replace("<CrashType>GPUCrash</CrashType>", "<CrashType>Crash</CrashType>")
            .replace("<RHI.Aftermath>true</RHI.Aftermath>", "")
            .replace(
                "<GPUCrash.D3DDeviceRemovedReason>-2005270521</GPUCrash.D3DDeviceRemovedReason>",
                "",
            );
        assert!(parse_crash_context_xml(&xml).is_none());
        assert!(parse_crash_context_xml("no context here").is_none());
    }

    #[test]
    fn vendor_and_adapter_pick_the_driver_module() {
        assert_eq!(gpu_kernel_module(Some(0x10DE), ""), "nvlddmkm");
        assert_eq!(gpu_kernel_module(Some(0x1002), ""), "amdkmdag");
        assert_eq!(gpu_kernel_module(Some(0x8086), ""), "igdkmd64");
        assert_eq!(gpu_kernel_module(None, "AMD Radeon RX 9070 XT"), "amdkmdag");
        assert_eq!(gpu_kernel_module(None, "Intel Arc B580"), "igdkmd64");
        assert_eq!(gpu_kernel_module(None, ""), "gpu");
    }

    #[test]
    fn dump_name_carries_the_crash_time() {
        assert_eq!(
            time_from_gpu_dump_name("D3D12.0.2026.06.18-23.01.01.nv-gpudmp").as_deref(),
            Some("06/18/2026 23:01 local")
        );
        assert_eq!(time_from_gpu_dump_name("UEMinidump.dmp"), None);
    }

    #[test]
    fn unescapes_xml_entities() {
        assert_eq!(unescape_xml("A &amp; B &lt;t&gt;"), "A & B <t>");
        assert_eq!(unescape_xml("A&#66;"), "AB");
        assert_eq!(unescape_xml("bare &amp amp"), "bare &amp amp");
    }
}
