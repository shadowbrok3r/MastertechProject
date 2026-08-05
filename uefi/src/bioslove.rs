//! BIOSLove model index: the on-disk catalogue of firmware payloads, and the
//! SMBIOS matcher that picks this machine's entry out of it.
//!
//! The index is generated off-box by the `bioslove-index` host crate from
//! `\\opk-riv\winbits\Drivers\Thumb\multiboot\BiosLove` and written to
//! `\bioslove\index.json` on the payload volume.

use serde::Deserialize;
use uefi::boot;
use uefi::boot::{OpenProtocolAttributes, OpenProtocolParams};
use uefi::proto::media::fs::SimpleFileSystem;

/// Where the index is read from on every attached volume.
pub const INDEX_PATH: &str = "\\bioslove\\index.json";

/// Refuse an index larger than this; a real one is well under 2 MiB.
const INDEX_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Schema the firmware understands. A newer index is refused rather than
/// half-parsed.
pub const SUPPORTED_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Laptop,
    Desktop,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Side::Laptop => "laptop",
            Side::Desktop => "desktop",
        }
    }
}

/// How a payload is delivered. Only `Uefi` is launchable from firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Vendor `.efi` flasher, launchable via LoadImage/StartImage.
    Uefi,
    /// Real-mode DOS flasher; unreachable without CSM.
    DosOnly,
    /// Vendor's own in-setup updater (EZ Flash, M-Flash, Instant Flash).
    InBiosOnly,
    /// Needs a booted Windows.
    WindowsOnly,
    /// Spec-conformant FMP capsule; goes through `capsule.rs`.
    Capsule,
}

impl Lane {
    pub fn label(self) -> &'static str {
        match self {
            Lane::Uefi => "UEFI flasher",
            Lane::DosOnly => "DOS only",
            Lane::InBiosOnly => "in-BIOS only",
            Lane::WindowsOnly => "Windows only",
            Lane::Capsule => "UEFI capsule",
        }
    }

    /// The app can start this payload itself.
    pub fn launchable(self) -> bool {
        matches!(self, Lane::Uefi | Lane::Capsule)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Bios,
    Ec,
    Me,
    /// Version gate that branches rather than flashing.
    Gate,
    Other,
}

impl StepKind {
    pub fn label(self) -> &'static str {
        match self {
            StepKind::Bios => "BIOS",
            StepKind::Ec => "EC",
            StepKind::Me => "ME",
            StepKind::Gate => "gate",
            StepKind::Other => "other",
        }
    }
}

/// What the machine does when a step's tool finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    /// Returns control to us.
    Returns,
    /// Tool reboots the machine; resume at the next step on the way back.
    Reboot,
    /// Tool powers the machine off; resume at the next step on the way back.
    Shutdown,
    Unknown,
}

impl After {
    pub fn label(self) -> &'static str {
        match self {
            After::Returns => "returns here",
            After::Reboot => "reboots",
            After::Shutdown => "shuts down",
            After::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PayloadFile {
    pub name: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub index: u32,
    pub kind: StepKind,
    /// Vendor tool to launch, relative to the entry's folder.
    pub exec: String,
    #[serde(default)]
    pub exec_sha256: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub files: Vec<PayloadFile>,
    #[serde(default = "after_unknown")]
    pub after: After,
    /// Every file this step names resolved to a real payload at index time.
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub note: String,
}

fn after_unknown() -> After {
    After::Unknown
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Versions {
    #[serde(default)]
    pub bios: String,
    #[serde(default)]
    pub ec: String,
    #[serde(default)]
    pub me: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub folder: String,
    pub side: Side,
    /// Exact chassis tokens this folder covers, from `ver.txt` and MODELSTRING.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Wildcard chassis tokens; `?` matches any single character.
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub modelstring: String,
    #[serde(default)]
    pub versions: Versions,
    pub lane: Lane,
    /// Every step resolved to files that exist.
    #[serde(default)]
    pub reachable: bool,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Entry {
    /// Folder plus every exact alias, for display and search.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        core::iter::once(self.folder.as_str()).chain(self.aliases.iter().map(|s| s.as_str()))
    }

    /// Steps whose payloads all resolved at index time.
    pub fn resolved_steps(&self) -> usize {
        self.steps.iter().filter(|s| s.resolved).count()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    pub schema_version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub source: String,
    pub entries: Vec<Entry>,
}

impl Index {
    pub fn laptop_count(&self) -> usize {
        self.entries.iter().filter(|e| e.side == Side::Laptop).count()
    }

    pub fn desktop_count(&self) -> usize {
        self.entries.iter().filter(|e| e.side == Side::Desktop).count()
    }
}

/// How a match was reached, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Substring of a chassis token; needs operator confirmation.
    Partial,
    /// Wildcard family token matched.
    Pattern,
    /// Normalized chassis token matched exactly.
    Exact,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Exact => "exact",
            Confidence::Pattern => "family",
            Confidence::Partial => "partial",
        }
    }
}

/// The SMBIOS field a match came from, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeySource {
    SysVersion,
    SysFamily,
    SysProduct,
    BoardProduct,
}

impl KeySource {
    pub fn label(self) -> &'static str {
        match self {
            KeySource::BoardProduct => "baseboard product",
            KeySource::SysProduct => "system product",
            KeySource::SysFamily => "system family",
            KeySource::SysVersion => "system version",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Match {
    /// Position in `Index::entries`.
    pub entry: usize,
    pub confidence: Confidence,
    pub source: KeySource,
    /// SMBIOS value that matched.
    pub key: String,
    /// Index-side token it matched.
    pub token: String,
}

impl Match {
    /// One-line provenance for the UI.
    pub fn evidence(&self) -> String {
        format!(
            "{} \"{}\" -> {} ({})",
            self.source.label(),
            self.key,
            self.token,
            self.confidence.label()
        )
    }
}

/// Uppercase alphanumerics only. SMBIOS reports `MS-16H5` where the folder is
/// `MS16H5`, and vendors vary spacing.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Normalized `pattern` against normalized `value`, `?` matching any one char.
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern.len() != value.len() {
        return false;
    }
    pattern
        .bytes()
        .zip(value.bytes())
        .all(|(p, v)| p == b'?' || p == v)
}

/// Wildcard tokens keep `?`; everything else is stripped like a plain token.
fn normalize_pattern(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '?')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Candidate SMBIOS values, strongest key first.
fn match_keys(d: &crate::Smbios) -> Vec<(KeySource, &str)> {
    let mut keys = vec![
        (KeySource::BoardProduct, d.board_product.as_str()),
        (KeySource::SysProduct, d.sys_product.as_str()),
        (KeySource::SysFamily, d.sys_family.as_str()),
        (KeySource::SysVersion, d.sys_version.as_str()),
    ];
    keys.retain(|(_, v)| normalize(v).len() >= 3);
    keys
}

/// Rank index entries against this machine's SMBIOS identity.
///
/// Entries on the other side of the laptop/desktop split are excluded outright:
/// chassis type is the one signal that is never ambiguous.
pub fn match_machine(index: &Index, d: &crate::Smbios) -> Vec<Match> {
    let side = if d.is_portable() {
        Side::Laptop
    } else {
        Side::Desktop
    };
    let keys = match_keys(d);
    let mut out: Vec<Match> = Vec::new();

    for (i, e) in index.entries.iter().enumerate() {
        if e.side != side {
            continue;
        }
        let mut best: Option<Match> = None;
        for (src, raw) in &keys {
            let key = normalize(raw);
            if key.is_empty() {
                continue;
            }
            let mut hit: Option<(Confidence, String)> = None;

            for name in e.names() {
                let token = normalize(name);
                if token.is_empty() {
                    continue;
                }
                if token == key {
                    hit = Some((Confidence::Exact, name.to_string()));
                    break;
                }
                // A chassis token embedded in a longer marketing string.
                if hit.is_none() && token.len() >= 5 && (key.contains(&token) || token.contains(&key))
                {
                    hit = Some((Confidence::Partial, name.to_string()));
                }
            }
            if !matches!(hit, Some((Confidence::Exact, _))) {
                for p in &e.patterns {
                    let pat = normalize_pattern(p);
                    if !pat.is_empty() && pattern_matches(&pat, &key) {
                        hit = Some((Confidence::Pattern, p.clone()));
                        break;
                    }
                }
            }

            if let Some((confidence, token)) = hit {
                let cand = Match {
                    entry: i,
                    confidence,
                    source: *src,
                    key: raw.trim().to_string(),
                    token,
                };
                let better = best
                    .as_ref()
                    .is_none_or(|b| (cand.confidence, cand.source) > (b.confidence, b.source));
                if better {
                    best = Some(cand);
                }
            }
        }
        if let Some(m) = best {
            out.push(m);
        }
    }

    // Strongest confidence, then strongest SMBIOS field, then reachable first.
    out.sort_by(|a, b| {
        (b.confidence, b.source)
            .cmp(&(a.confidence, a.source))
            .then_with(|| {
                index.entries[b.entry]
                    .reachable
                    .cmp(&index.entries[a.entry].reachable)
            })
            .then_with(|| index.entries[a.entry].folder.cmp(&index.entries[b.entry].folder))
    });
    out
}

/// Entry positions whose folder, aliases or modelstring contain `needle`.
/// An empty needle returns everything on `side`, folder-sorted.
pub fn search(index: &Index, side: Side, needle: &str) -> Vec<usize> {
    let n = normalize(needle);
    let mut hits: Vec<usize> = index
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.side == side)
        .filter(|(_, e)| {
            n.is_empty()
                || e.names().any(|name| normalize(name).contains(&n))
                || normalize(&e.modelstring).contains(&n)
        })
        .map(|(i, _)| i)
        .collect();
    hits.sort_by(|a, b| index.entries[*a].folder.cmp(&index.entries[*b].folder));
    hits
}

/// Read `INDEX_PATH` off the first attached volume that has it.
pub fn load_from_volume() -> Result<Index, String> {
    let bytes = read_file_any_volume(INDEX_PATH)?;
    parse(&bytes)
}

/// Parse and version-gate an index document.
pub fn parse(bytes: &[u8]) -> Result<Index, String> {
    let index: Index =
        serde_json::from_slice(bytes).map_err(|e| format!("index is not valid JSON: {e}"))?;
    if index.schema_version != SUPPORTED_SCHEMA {
        return Err(format!(
            "index schema {} is not supported (this build reads {SUPPORTED_SCHEMA})",
            index.schema_version
        ));
    }
    Ok(index)
}

/// Read a backslash-separated volume-relative path off any attached filesystem.
pub fn read_file_any_volume(path: &str) -> Result<Vec<u8>, String> {
    use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode, FileType};

    let cpath = uefi::CString16::try_from(path.replace('/', "\\").as_str())
        .map_err(|_| format!("path is not valid UCS-2: {path}"))?;
    let handles =
        boot::find_handles::<SimpleFileSystem>().map_err(|e| format!("no filesystems ({e:?})"))?;

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
        let size = match f.get_boxed_info::<FileInfo>() {
            Ok(i) => i.file_size() as usize,
            Err(e) => {
                last = format!("stat {path}: {e:?}");
                continue;
            }
        };
        if size == 0 || size > INDEX_MAX_BYTES {
            last = format!("{path} is {size} bytes (max {INDEX_MAX_BYTES})");
            continue;
        }
        let mut buf = vec![0u8; size];
        match f.read(&mut buf) {
            Ok(n) if n == size => return Ok(buf),
            Ok(n) => last = format!("short read: {n} of {size} bytes"),
            Err(e) => last = format!("read {path}: {e:?}"),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_vendor_punctuation() {
        assert_eq!(normalize("MS-16H5"), "MS16H5");
        assert_eq!(normalize(" nh58dcq "), "NH58DCQ");
    }

    #[test]
    fn patterns_match_family_tokens() {
        assert!(pattern_matches("GM?IX7?", "GM6IX7N"));
        assert!(!pattern_matches("GM?IX7?", "GM6IX9N"));
        assert!(!pattern_matches("GM?IX7?", "GM6IX7NX"));
    }
}
