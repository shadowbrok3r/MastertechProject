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

    /// Directory under the payload root, as the share spells it.
    pub fn dir_name(self) -> &'static str {
        match self {
            Side::Laptop => "laptop",
            Side::Desktop => "Desktop",
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

/// Where the side trees sit on the payload volume when the index does not say.
fn default_payload_root() -> String {
    "\\multiboot\\BiosLove".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    pub schema_version: u32,
    #[serde(default)]
    pub generated_at: String,
    /// Share path the index was built from; provenance only.
    #[serde(default)]
    pub source: String,
    /// Volume-relative directory holding the `laptop` and `Desktop` trees.
    #[serde(default = "default_payload_root")]
    pub payload_root: String,
    pub entries: Vec<Entry>,
}

/// Directory network-fetched payloads are staged into, one subdirectory per
/// model. A vendor tool resolves its ROM relative to its own device path, so
/// everything one step needs has to land in a single directory.
pub const CACHE_ROOT: &str = "\\bioslove\\cache";

/// Staging directory for `entry`'s payloads.
pub fn cache_dir(entry: &Entry) -> String {
    format!("{CACHE_ROOT}\\{}", entry.folder)
}

/// Trailing component of a backslash-separated path.
pub fn base_name(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

impl Index {
    /// Volume path of a file inside an entry's folder. `name` may carry a
    /// backslash-separated subdirectory, as the generator emits it.
    pub fn file_path(&self, entry: &Entry, name: &str) -> String {
        format!(
            "{}\\{}\\{}\\{}",
            self.payload_root.trim_end_matches('\\'),
            entry.side.dir_name(),
            entry.folder,
            name
        )
    }

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
    /// The token was the folder's own name, not one of its aliases.
    pub own_name: bool,
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

/// The SMBIOS values the matcher needs, so this module does not depend on the
/// app's system-info shape.
pub struct MachineKeys<'a> {
    /// Chassis runs on a battery, i.e. the `laptop` side of the share.
    pub portable: bool,
    pub board_product: &'a str,
    pub sys_product: &'a str,
    pub sys_family: &'a str,
    pub sys_version: &'a str,
}

/// Candidate SMBIOS values, strongest key first.
fn match_keys<'a>(d: &MachineKeys<'a>) -> Vec<(KeySource, &'a str)> {
    let mut keys = vec![
        (KeySource::BoardProduct, d.board_product),
        (KeySource::SysProduct, d.sys_product),
        (KeySource::SysFamily, d.sys_family),
        (KeySource::SysVersion, d.sys_version),
    ];
    keys.retain(|(_, v)| normalize(v).len() >= 3);
    keys
}

/// Rank index entries against this machine's SMBIOS identity.
///
/// Entries on the other side of the laptop/desktop split are excluded outright:
/// chassis type is the one signal that is never ambiguous.
pub fn match_machine(index: &Index, d: &MachineKeys<'_>) -> Vec<Match> {
    let side = if d.portable {
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
                // A chassis token embedded in a longer string. OEMs prefix the
                // family with a platform code — SMBIOS says PF5LUXG, the folder
                // is LUXG — so 4-char tokens have to qualify.
                if hit.is_none() && token.len() >= 4 && (key.contains(&token) || token.contains(&key))
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
                let own_name = normalize(&token) == normalize(&e.folder);
                let cand = Match {
                    entry: i,
                    confidence,
                    source: *src,
                    key: raw.trim().to_string(),
                    token,
                    own_name,
                };
                let better = best.as_ref().is_none_or(|b| {
                    (cand.confidence, cand.source, cand.own_name)
                        > (b.confidence, b.source, b.own_name)
                });
                if better {
                    best = Some(cand);
                }
            }
        }
        if let Some(m) = best {
            out.push(m);
        }
    }

    // Strongest confidence, then strongest SMBIOS field, then the folder named
    // for the token over one that only lists it as an alias, then reachable.
    out.sort_by(|a, b| {
        (b.confidence, b.source, b.own_name)
            .cmp(&(a.confidence, a.source, a.own_name))
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
/// `side` of `None` searches both. An empty needle returns everything.
pub fn search(index: &Index, side: Option<Side>, needle: &str) -> Vec<usize> {
    let n = normalize(needle);
    let mut hits: Vec<usize> = index
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| side.is_none_or(|s| e.side == s))
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

/// Read `INDEX_PATH` off the first attached volume that has it, returning that
/// volume so payloads and the cache land beside the index.
pub fn load_from_volume() -> Result<(Index, uefi::Handle), String> {
    let (bytes, volume) = read_file_any_volume(INDEX_PATH, INDEX_MAX_BYTES)?;
    Ok((parse(&bytes)?, volume))
}

/// Probe file used to prove a volume accepts writes.
const WRITE_PROBE: &str = "\\bioslove\\.writable";

/// Which volume staging landed on, so the UI can say whose disk gets written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingVolume {
    /// The volume the index came from, or the one the app booted from.
    Own,
    /// Some other attached filesystem — on an ISO boot this is typically the
    /// machine's own ESP, so staged payloads land on the customer's disk.
    Foreign,
}

/// Free bytes on a volume; `None` when the filesystem will not report it.
pub fn free_space(volume: uefi::Handle) -> Option<u64> {
    use uefi::proto::media::file::{File, FileSystemInfo};
    let mut sfs = unsafe {
        boot::open_protocol::<SimpleFileSystem>(
            OpenProtocolParams {
                handle: volume,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()?;
    let mut root = sfs.open_volume().ok()?;
    root.get_boxed_info::<FileSystemInfo>().ok().map(|i| i.free_space())
}

/// First attached volume that accepts a write **and** has room for `need_bytes`.
///
/// The index's own volume is not usable as staging on its own: a box booted off
/// a Ventoy ISO reads it from a read-only filesystem, and a network-fetched
/// index has no volume at all. Writability is proven by writing, since nothing
/// in `EFI_FILE_PROTOCOL` reports it up front. Capacity has to be checked too —
/// an ISO boot leaves only small EFI system partitions writable, and a 16 MiB
/// ROM fails partway through the copy with `VOLUME_FULL` otherwise.
pub fn writable_volume(
    prefer: &[uefi::Handle],
    need_bytes: u64,
) -> Option<(uefi::Handle, StagingVolume)> {
    let fits = |h: uefi::Handle| -> bool {
        if write_file_on_volume(h, WRITE_PROBE, b"1").is_err() {
            return false;
        }
        // A filesystem that won't report free space still gets a try; the write
        // is the final authority either way.
        free_space(h).is_none_or(|f| f >= need_bytes)
    };
    for h in prefer {
        if fits(*h) {
            return Some((*h, StagingVolume::Own));
        }
    }
    let all = boot::find_handles::<SimpleFileSystem>().ok()?;
    all.into_iter()
        .filter(|h| !prefer.contains(h))
        .find(|h| fits(*h))
        .map(|h| (h, StagingVolume::Foreign))
}

/// Volume this image was loaded from, the natural scratch space on a boot stick.
pub fn boot_volume() -> Option<uefi::Handle> {
    use uefi::proto::loaded_image::LoadedImage;
    let li = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle()).ok()?;
    li.device()
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

/// Largest firmware payload that will be read for verification.
pub const PAYLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Read a volume-relative path off any attached filesystem, returning the bytes
/// and the volume handle they came from.
pub fn read_file_any_volume(path: &str, max: usize) -> Result<(Vec<u8>, uefi::Handle), String> {
    let handles =
        boot::find_handles::<SimpleFileSystem>().map_err(|e| format!("no filesystems ({e:?})"))?;

    let mut last = format!("{path} not found on any volume");
    for h in handles {
        match read_file_on_volume(h, path, max) {
            Ok(bytes) => return Ok((bytes, h)),
            Err(e) => {
                // Keep the most specific failure, not "not on this volume".
                if !e.ends_with("not on this volume") {
                    last = e;
                }
            }
        }
    }
    Err(last)
}

/// Write `bytes` to a volume-relative path, creating parent directories and
/// replacing any existing file.
pub fn write_file_on_volume(
    volume: uefi::Handle,
    path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};

    let path = path.replace('/', "\\");
    let cpath = uefi::CString16::try_from(path.as_str())
        .map_err(|_| format!("path is not valid UCS-2: {path}"))?;
    let mut sfs = unsafe {
        boot::open_protocol::<SimpleFileSystem>(
            OpenProtocolParams {
                handle: volume,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .map_err(|e| format!("open volume: {e:?}"))?;
    let mut root = sfs.open_volume().map_err(|e| format!("open root: {e:?}"))?;

    if let Some((dir, _)) = path.rsplit_once('\\') {
        crate::flashstate::ensure_dir(&mut root, dir);
    }
    // Delete any existing file rather than overwrite it: EFI_FILE_PROTOCOL has no
    // truncate, so a shorter payload would leave the old tail behind.
    if let Ok(h) = root.open(&cpath, FileMode::ReadWrite, FileAttribute::empty()) {
        if let Ok(FileType::Regular(old)) = h.into_type() {
            let _ = old.delete();
        }
    }
    let handle = root
        .open(&cpath, FileMode::CreateReadWrite, FileAttribute::empty())
        .map_err(|e| format!("create {path}: {e:?}"))?;
    let FileType::Regular(mut f) = handle.into_type().map_err(|e| format!("{e:?}"))? else {
        return Err(format!("{path} is a directory"));
    };
    f.write(bytes).map_err(|e| format!("write {path}: {e:?}"))?;
    f.flush().map_err(|e| format!("flush {path}: {e:?}"))?;
    Ok(())
}

/// Read a volume-relative path off one known filesystem.
pub fn read_file_on_volume(
    volume: uefi::Handle,
    path: &str,
    max: usize,
) -> Result<Vec<u8>, String> {
    use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode, FileType};

    let cpath = uefi::CString16::try_from(path.replace('/', "\\").as_str())
        .map_err(|_| format!("path is not valid UCS-2: {path}"))?;
    let mut sfs = unsafe {
        boot::open_protocol::<SimpleFileSystem>(
            OpenProtocolParams {
                handle: volume,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .map_err(|_| "not on this volume".to_string())?;
    let mut root = sfs
        .open_volume()
        .map_err(|_| "not on this volume".to_string())?;
    let handle = root
        .open(&cpath, FileMode::Read, FileAttribute::empty())
        .map_err(|_| "not on this volume".to_string())?;
    let FileType::Regular(mut f) = handle
        .into_type()
        .map_err(|e| format!("open {path}: {e:?}"))?
    else {
        return Err(format!("{path} is a directory"));
    };
    let size = f
        .get_boxed_info::<FileInfo>()
        .map(|i| i.file_size() as usize)
        .map_err(|e| format!("stat {path}: {e:?}"))?;
    if size == 0 {
        return Err(format!("{path} is empty"));
    }
    if size > max {
        return Err(format!("{path} is {size} bytes (max {max})"));
    }
    let mut buf = vec![0u8; size];
    match f.read(&mut buf) {
        Ok(n) if n == size => Ok(buf),
        Ok(n) => Err(format!("short read: {n} of {size} bytes")),
        Err(e) => Err(format!("read {path}: {e:?}")),
    }
}

// Tests for `normalize` and `pattern_matches` live in the host-side
// `bioslove-index` crate: this crate only builds for x86_64-unknown-uefi, whose
// test binary cannot execute on a host.
