//! HII BIOS-settings audit.
//!
//! Exports the firmware's HII database, parses the string and forms (IFR)
//! packages, and surfaces the Setup questions — with a golden-config view of
//! the security/performance settings a build should have set. Current values
//! are read best-effort from EFI varstores; where a value can't be resolved
//! the question and its options are still reported.
//!
//! All parsing is bounds-checked and never panics: partial firmware support
//! degrades to a package tally rather than an error.

use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
use uefi::proto::hii::database::HiiDatabase;
use uefi::runtime::{self, VariableVendor};
use uefi::{CString16, Guid};

use crate::logln;

/// HII package types (UEFI spec 33.3.1.2).
const PKG_FORMS: u8 = 0x02;
const PKG_STRINGS: u8 = 0x04;
const PKG_END: u8 = 0xDF;

/// IFR opcodes (UEFI spec 33.3.8).
const IFR_FORM_SET: u8 = 0x0E;
const IFR_ONE_OF: u8 = 0x05;
const IFR_CHECKBOX: u8 = 0x06;
const IFR_NUMERIC: u8 = 0x07;
const IFR_ONE_OF_OPTION: u8 = 0x09;
const IFR_ORDERED_LIST: u8 = 0x23;
const IFR_VARSTORE: u8 = 0x24;
const IFR_VARSTORE_EFI: u8 = 0x26;
const IFR_DEFAULT: u8 = 0x5B;

/// One-of-option flags: this option is the standard default.
const OPTION_DEFAULT: u8 = 0x10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    OneOf,
    Checkbox,
    Numeric,
    OrderedList,
}

impl SettingKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneOf => "one-of",
            Self::Checkbox => "checkbox",
            Self::Numeric => "numeric",
            Self::OrderedList => "ordered",
        }
    }
}

pub struct BiosSetting {
    pub name: String,
    pub kind: SettingKind,
    /// Resolved current value, or None when the varstore can't be read.
    pub current: Option<String>,
    /// One-of option labels.
    pub options: Vec<String>,
    /// Golden-config category when the prompt matched a watched keyword.
    pub category: Option<&'static str>,
    /// `guid:name+0xoffset` for traceability.
    pub varstore: String,
}

#[derive(Default)]
pub struct HiiAudit {
    pub available: bool,
    pub raw_bytes: usize,
    pub package_lists: usize,
    pub forms_packages: usize,
    pub string_packages: usize,
    pub formsets: usize,
    pub questions_total: usize,
    /// Flagged golden-config settings (subset of all questions).
    pub settings: Vec<BiosSetting>,
    pub note: String,
}

/// Golden-config keyword table: (category, lowercase substrings to match).
const GOLDEN: &[(&str, &[&str])] = &[
    ("Secure Boot", &["secure boot"]),
    ("TPM", &["tpm", "ptt", "ftpm", "trusted platform", "security device support"]),
    ("Virtualization", &["vt-d", "vt-x", "virtualization", "svm mode", "iommu", "amd-v"]),
    ("Resizable BAR", &["resizable bar", "re-size bar", "above 4g", "sr-iov"]),
    ("Memory Profile", &["xmp", "expo", "docp", "a-xmp", "memory profile"]),
    ("Boot Mode", &["fast boot", "csm", "compatibility support", "legacy boot", "boot mode select"]),
    ("Storage", &["sata mode", "vmd controller", "nvme raid", "raid mode", "sata configuration"]),
    ("Power", &["wake on lan", "erp ready", "power loss", "restore ac power", "deep sleep"]),
    ("CPU", &["hyper-threading", "smt control", "c-state", "turbo mode", "core performance boost"]),
];

fn category_for(prompt: &str) -> Option<&'static str> {
    let p = prompt.to_ascii_lowercase();
    GOLDEN
        .iter()
        .find(|(_, kws)| kws.iter().any(|k| p.contains(k)))
        .map(|(cat, _)| *cat)
}

// --- little-endian readers over a byte slice (all bounds-checked) ---

fn rd_u16(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn rd_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn rd_guid(b: &[u8], off: usize) -> Option<Guid> {
    let s = b.get(off..off + 16)?;
    let mut a = [0u8; 16];
    a.copy_from_slice(s);
    Some(Guid::from_bytes(a))
}

/// Read a UCS-2 (CHAR16) NUL-terminated string starting at `off`; returns the
/// decoded string and the byte offset just past the terminator.
fn read_ucs2(b: &[u8], off: usize) -> (String, usize) {
    let mut s = String::new();
    let mut p = off;
    while let Some(cu) = rd_u16(b, p) {
        p += 2;
        if cu == 0 {
            break;
        }
        s.push(char::from_u32(cu as u32).unwrap_or('\u{FFFD}'));
    }
    (s, p)
}

/// One IFR varstore mapping: id -> variable (guid, name) + declared size.
struct VarStore {
    id: u16,
    guid: Guid,
    name: String,
    /// Cached variable contents, read once on first use.
    data: Option<Vec<u8>>,
    read_tried: bool,
}

impl VarStore {
    /// Read `width` bytes at `offset` from the backing variable (cached).
    fn value(&mut self, offset: usize, width: usize) -> Option<u64> {
        if !self.read_tried {
            self.read_tried = true;
            self.data = read_variable(&self.name, self.guid);
        }
        let d = self.data.as_ref()?;
        let bytes = d.get(offset..offset + width)?;
        let mut v = 0u64;
        for (i, &byte) in bytes.iter().enumerate() {
            v |= (byte as u64) << (8 * i);
        }
        Some(v)
    }
}

fn read_variable(name: &str, guid: Guid) -> Option<Vec<u8>> {
    let cname = CString16::try_from(name).ok()?;
    let vendor = VariableVendor(guid);
    match runtime::get_variable_boxed(&cname, &vendor) {
        Ok((data, _)) => Some(data.into_vec()),
        Err(_) => None,
    }
}

/// EFI_IFR_TYPE_VALUE width in bytes for a numeric flags size field (bits 0-1).
fn width_for_flags(flags: u8) -> usize {
    match flags & 0x03 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 8,
    }
}

/// Parse one string package's blocks into a StringId -> text map. Bails safely
/// on any block type it can't length-decode (SCSU etc.) to avoid desyncing.
fn parse_strings(pkg: &[u8], map: &mut Vec<(u16, String)>) {
    // EFI_HII_STRING_PACKAGE_HDR: StringInfoOffset at +8 (from package start).
    let Some(info_off) = rd_u32(pkg, 8).map(|v| v as usize) else {
        return;
    };
    let mut p = info_off;
    let mut sid: u16 = 1; // StringId 0 is the empty string.
    loop {
        let Some(&block) = pkg.get(p) else { break };
        match block {
            0x00 => break, // SIBT_END
            0x14 => {
                // SIBT_STRING_UCS2: type(1) + CHAR16[] NUL-term.
                let (s, next) = read_ucs2(pkg, p + 1);
                map.push((sid, s));
                sid = sid.wrapping_add(1);
                p = next;
            }
            0x15 => {
                // SIBT_STRING_UCS2_FONT: type(1) + fontId(1) + CHAR16[].
                let (s, next) = read_ucs2(pkg, p + 2);
                map.push((sid, s));
                sid = sid.wrapping_add(1);
                p = next;
            }
            0x16 | 0x17 => {
                // SIBT_STRINGS_UCS2[_FONT]: type + [fontId] + u16 count + strings.
                let hdr = if block == 0x17 { 2 } else { 1 };
                let Some(count) = rd_u16(pkg, p + hdr) else { break };
                let mut next = p + hdr + 2;
                for _ in 0..count {
                    let (s, np) = read_ucs2(pkg, next);
                    map.push((sid, s));
                    sid = sid.wrapping_add(1);
                    next = np;
                }
                p = next;
            }
            0x20 => {
                // SIBT_DUPLICATE: type + EFI_STRING_ID.
                sid = sid.wrapping_add(1);
                p += 3;
            }
            0x21 => {
                // SIBT_SKIP2: type + u16 count.
                let Some(c) = rd_u16(pkg, p + 1) else { break };
                sid = sid.wrapping_add(c);
                p += 3;
            }
            0x22 => {
                // SIBT_SKIP1: type + u8 count.
                let c = *pkg.get(p + 1).unwrap_or(&0);
                sid = sid.wrapping_add(c as u16);
                p += 2;
            }
            0x30 => {
                // SIBT_EXT1: type + blockType2 + u8 length.
                let l = *pkg.get(p + 2).unwrap_or(&0) as usize;
                if l == 0 {
                    break;
                }
                p += l;
            }
            0x31 => {
                // SIBT_EXT2: type + blockType2 + u16 length.
                let Some(l) = rd_u16(pkg, p + 2).map(|v| v as usize) else { break };
                if l == 0 {
                    break;
                }
                p += l;
            }
            0x32 => {
                // SIBT_EXT4: type + blockType2 + u32 length.
                let Some(l) = rd_u32(pkg, p + 2).map(|v| v as usize) else { break };
                if l == 0 {
                    break;
                }
                p += l;
            }
            _ => break, // SCSU / unknown: can't length-decode, stop safely.
        }
    }
}

fn lookup<'a>(map: &'a [(u16, String)], id: u16) -> Option<&'a str> {
    if id == 0 {
        return None;
    }
    map.iter().find(|(k, _)| *k == id).map(|(_, s)| s.as_str())
}

/// A question captured mid-walk, before its options/value are resolved.
struct Pending {
    kind: SettingKind,
    prompt_id: u16,
    varstore_id: u16,
    var_offset: u16,
    width: usize,
    options: Vec<(String, u64, bool)>, // (label, value, is_default)
}

/// Walk one forms package's IFR opcodes, appending finished questions.
fn parse_forms(
    pkg: &[u8],
    strings: &[(u16, String)],
    varstores: &mut Vec<VarStore>,
    audit: &mut HiiAudit,
) {
    // EFI_HII_PACKAGE_HEADER is 4 bytes; IFR opcodes follow.
    let mut p = 4usize;
    let mut pending: Option<Pending> = None;

    // Flush a finished question into the audit, resolving value + category.
    let flush = |pend: Pending, varstores: &mut Vec<VarStore>, audit: &mut HiiAudit| {
        audit.questions_total += 1;
        let Some(prompt) = lookup(strings, pend.prompt_id) else {
            return;
        };
        let category = category_for(prompt);
        if category.is_none() {
            return; // Only keep golden-config settings.
        }
        let mut options: Vec<String> = pend.options.iter().map(|(l, _, _)| l.clone()).collect();
        if options.is_empty() && pend.kind == SettingKind::Checkbox {
            options = vec!["Disabled".into(), "Enabled".into()];
        }
        let vs = varstores.iter_mut().find(|v| v.id == pend.varstore_id);
        let current = match (pend.kind, vs) {
            (SettingKind::Checkbox, Some(vs)) => vs
                .value(pend.var_offset as usize, 1)
                .map(|v| if v != 0 { "Enabled".into() } else { "Disabled".into() }),
            (SettingKind::OneOf, Some(vs)) => vs.value(pend.var_offset as usize, pend.width).map(|v| {
                pend.options
                    .iter()
                    .find(|(_, ov, _)| *ov == v)
                    .map(|(l, _, _)| l.clone())
                    .unwrap_or_else(|| format!("0x{v:x}"))
            }),
            (SettingKind::Numeric, Some(vs)) => {
                vs.value(pend.var_offset as usize, pend.width).map(|v| v.to_string())
            }
            _ => None,
        };
        let vs_desc = varstores
            .iter()
            .find(|v| v.id == pend.varstore_id)
            .map(|v| format!("{}+0x{:x}", v.name, pend.var_offset))
            .unwrap_or_else(|| format!("vs{}+0x{:x}", pend.varstore_id, pend.var_offset));
        audit.settings.push(BiosSetting {
            name: prompt.to_string(),
            kind: pend.kind,
            current,
            options,
            category,
            varstore: vs_desc,
        });
    };

    while p + 2 <= pkg.len() {
        let opcode = pkg[p];
        let len = (pkg[p + 1] & 0x7F) as usize;
        if len < 2 || p + len > pkg.len() {
            break; // Malformed length: stop safely.
        }
        let body = &pkg[p..p + len];
        match opcode {
            IFR_FORM_SET => audit.formsets += 1,
            IFR_VARSTORE => {
                // header(2) + Guid(16) + VarStoreId(2) + Size(2) + Name[](ASCII).
                if let (Some(guid), Some(id)) = (rd_guid(body, 2), rd_u16(body, 18)) {
                    let name = ascii_z(&body[22.min(body.len())..]);
                    varstores.push(VarStore { id, guid, name, data: None, read_tried: false });
                }
            }
            IFR_VARSTORE_EFI => {
                // header(2) + VarStoreId(2) + Guid(16) + Attributes(4) + Size(2) + Name[].
                if let (Some(id), Some(guid)) = (rd_u16(body, 2), rd_guid(body, 4)) {
                    let name = ascii_z(&body[26.min(body.len())..]);
                    if !name.is_empty() {
                        varstores.push(VarStore { id, guid, name, data: None, read_tried: false });
                    }
                }
            }
            IFR_ONE_OF | IFR_NUMERIC => {
                if let Some(prev) = pending.take() {
                    flush(prev, varstores, audit);
                }
                // QUESTION_HEADER: Prompt@2 Help@4 QId@6 VarStoreId@8 VarInfo@10, Flags@12.
                let prompt_id = rd_u16(body, 2).unwrap_or(0);
                let varstore_id = rd_u16(body, 8).unwrap_or(0);
                let var_offset = rd_u16(body, 10).unwrap_or(0);
                let flags = *body.get(12).unwrap_or(&0);
                pending = Some(Pending {
                    kind: if opcode == IFR_ONE_OF { SettingKind::OneOf } else { SettingKind::Numeric },
                    prompt_id,
                    varstore_id,
                    var_offset,
                    width: width_for_flags(flags),
                    options: Vec::new(),
                });
            }
            IFR_CHECKBOX => {
                if let Some(prev) = pending.take() {
                    flush(prev, varstores, audit);
                }
                pending = Some(Pending {
                    kind: SettingKind::Checkbox,
                    prompt_id: rd_u16(body, 2).unwrap_or(0),
                    varstore_id: rd_u16(body, 8).unwrap_or(0),
                    var_offset: rd_u16(body, 10).unwrap_or(0),
                    width: 1,
                    options: Vec::new(),
                });
            }
            IFR_ORDERED_LIST => {
                if let Some(prev) = pending.take() {
                    flush(prev, varstores, audit);
                }
                pending = Some(Pending {
                    kind: SettingKind::OrderedList,
                    prompt_id: rd_u16(body, 2).unwrap_or(0),
                    varstore_id: rd_u16(body, 8).unwrap_or(0),
                    var_offset: rd_u16(body, 10).unwrap_or(0),
                    width: 1,
                    options: Vec::new(),
                });
            }
            IFR_ONE_OF_OPTION => {
                // header(2) + Option(2) + Flags(1) + Type(1) + Value.
                if let Some(pend) = pending.as_mut() {
                    let opt_id = rd_u16(body, 2).unwrap_or(0);
                    let flags = *body.get(4).unwrap_or(&0);
                    let ty = *body.get(5).unwrap_or(&0);
                    let value = read_type_value(body, 6, ty);
                    if let Some(label) = lookup(strings, opt_id) {
                        pend.options.push((label.to_string(), value, flags & OPTION_DEFAULT != 0));
                    }
                }
            }
            IFR_DEFAULT => {
                // header(2) + DefaultId(2) + Type(1) + Value; attaches to pending.
                if let Some(pend) = pending.as_mut() {
                    let ty = *body.get(4).unwrap_or(&0);
                    let value = read_type_value(body, 5, ty);
                    // Mark the matching option as default if present.
                    if let Some(o) = pend.options.iter_mut().find(|(_, v, _)| *v == value) {
                        o.2 = true;
                    }
                }
            }
            _ => {}
        }
        p += len;
    }
    if let Some(prev) = pending.take() {
        flush(prev, varstores, audit);
    }
}

/// Decode an EFI_IFR_TYPE_VALUE at `off` for the given type code.
fn read_type_value(b: &[u8], off: usize, ty: u8) -> u64 {
    match ty {
        0x00 | 0x04 => *b.get(off).unwrap_or(&0) as u64, // UINT8 / BOOLEAN
        0x01 | 0x07 | 0x0A => rd_u16(b, off).unwrap_or(0) as u64, // UINT16 / STRING / ACTION
        0x02 => rd_u32(b, off).unwrap_or(0) as u64,
        0x03 => {
            let lo = rd_u32(b, off).unwrap_or(0) as u64;
            let hi = rd_u32(b, off + 4).unwrap_or(0) as u64;
            (hi << 32) | lo
        }
        _ => 0,
    }
}

/// ASCII NUL-terminated name from a byte slice.
fn ascii_z(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Run the audit: export the HII database and parse every package list.
pub fn collect() -> HiiAudit {
    let mut audit = HiiAudit::default();

    let handle = match boot::get_handle_for_protocol::<HiiDatabase>() {
        Ok(h) => h,
        Err(e) => {
            audit.note = format!("HII database protocol absent ({e:?})");
            return audit;
        }
    };
    let db = match unsafe {
        boot::open_protocol::<HiiDatabase>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    } {
        Ok(db) => db,
        Err(e) => {
            audit.note = format!("open HII database failed ({e:?})");
            return audit;
        }
    };
    let raw = match db.export_all_raw() {
        Ok(b) => b,
        Err(e) => {
            audit.note = format!("export package lists failed ({e:?})");
            return audit;
        }
    };
    let raw: &[u8] = &raw;
    audit.available = true;
    audit.raw_bytes = raw.len();

    // Walk package lists: [GUID(16)][package_length u32][packages...].
    let mut lp = 0usize;
    while lp + 20 <= raw.len() {
        let list_len = match rd_u32(raw, lp + 16) {
            Some(l) if l as usize >= 20 => l as usize,
            _ => break,
        };
        let list_end = (lp + list_len).min(raw.len());
        audit.package_lists += 1;

        // Two passes: strings first (forms reference their StringIds), then forms.
        let mut strings: Vec<(u16, String)> = Vec::new();
        let mut forms_spans: Vec<(usize, usize)> = Vec::new();
        let mut pp = lp + 20;
        while pp + 4 <= list_end {
            let lat = rd_u32(raw, pp).unwrap_or(0);
            let plen = (lat & 0x00FF_FFFF) as usize;
            let ptype = (lat >> 24) as u8;
            if plen < 4 || pp + plen > list_end {
                break;
            }
            match ptype {
                PKG_STRINGS => {
                    audit.string_packages += 1;
                    parse_strings(&raw[pp..pp + plen], &mut strings);
                }
                PKG_FORMS => {
                    audit.forms_packages += 1;
                    forms_spans.push((pp, pp + plen));
                }
                PKG_END => break,
                _ => {}
            }
            pp += plen;
        }

        let mut varstores: Vec<VarStore> = Vec::new();
        for (s, e) in forms_spans {
            parse_forms(&raw[s..e], &strings, &mut varstores, &mut audit);
        }

        lp = list_end;
    }

    logln(format!(
        "hii: {} bytes, {} lists, {} forms pkgs, {} formsets, {} questions, {} flagged",
        audit.raw_bytes,
        audit.package_lists,
        audit.forms_packages,
        audit.formsets,
        audit.questions_total,
        audit.settings.len()
    ));
    if audit.settings.is_empty() && audit.note.is_empty() {
        audit.note = "no golden-config settings resolved (firmware may hide Setup IFR)".into();
    }
    audit
}

/// `bios_settings` object for the fingerprint upload.
pub fn audit_json(a: &HiiAudit) -> serde_json::Value {
    let settings: Vec<serde_json::Value> = a
        .settings
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "kind": s.kind.label(),
                "category": s.category,
                "current": s.current,
                "options": s.options,
                "varstore": s.varstore,
            })
        })
        .collect();
    serde_json::json!({
        "available": a.available,
        "raw_bytes": a.raw_bytes,
        "package_lists": a.package_lists,
        "forms_packages": a.forms_packages,
        "formsets": a.formsets,
        "questions_total": a.questions_total,
        "note": a.note,
        "settings": settings,
    })
}
