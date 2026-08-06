//! The index document. Field-for-field mirror of the serde types the firmware
//! reads in `uefi/src/bioslove.rs`; changing anything here breaks that parse.

use serde::{Deserialize, Serialize};

/// Schema the firmware accepts.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Laptop,
    Desktop,
}

impl Side {
    pub const ALL: [Side; 2] = [Side::Laptop, Side::Desktop];

    pub fn label(self) -> &'static str {
        match self {
            Side::Laptop => "laptop",
            Side::Desktop => "desktop",
        }
    }

    /// Directory under the share root, as the share spells it.
    pub fn dir_name(self) -> &'static str {
        match self {
            Side::Laptop => "laptop",
            Side::Desktop => "Desktop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Uefi,
    DosOnly,
    InBiosOnly,
    WindowsOnly,
    Capsule,
}

impl Lane {
    pub const ALL: [Lane; 5] = [
        Lane::Uefi,
        Lane::Capsule,
        Lane::DosOnly,
        Lane::WindowsOnly,
        Lane::InBiosOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Lane::Uefi => "uefi",
            Lane::DosOnly => "dos_only",
            Lane::InBiosOnly => "in_bios_only",
            Lane::WindowsOnly => "windows_only",
            Lane::Capsule => "capsule",
        }
    }

    /// The firmware starts this payload itself; mirrors `Lane::launchable` in
    /// `uefi/src/bioslove.rs`.
    pub fn launchable(self) -> bool {
        matches!(self, Lane::Uefi | Lane::Capsule)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Bios,
    Ec,
    Me,
    Gate,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    Returns,
    Reboot,
    Shutdown,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadFile {
    pub name: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub index: u32,
    pub kind: StepKind,
    pub exec: String,
    #[serde(default)]
    pub exec_sha256: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub files: Vec<PayloadFile>,
    pub after: After,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Versions {
    #[serde(default)]
    pub bios: String,
    #[serde(default)]
    pub ec: String,
    #[serde(default)]
    pub me: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub folder: String,
    pub side: Side,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub modelstring: String,
    #[serde(default)]
    pub versions: Versions,
    pub lane: Lane,
    #[serde(default)]
    pub reachable: bool,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Suffix every dangling-reference warning ends with.
pub const ABSENT: &str = " which is absent";

impl Entry {
    /// Warnings naming a file a script calls for that the share no longer has.
    pub fn dangling(&self) -> impl Iterator<Item = &str> {
        self.warnings
            .iter()
            .filter(|w| w.ends_with(ABSENT))
            .map(String::as_str)
    }
}

/// Where the side trees sit on the payload volume the firmware reads.
pub const DEFAULT_PAYLOAD_ROOT: &str = r"\multiboot\BiosLove";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub schema_version: u32,
    pub generated_at: String,
    /// Share path this was built from; provenance only.
    pub source: String,
    /// Volume-relative directory the firmware resolves payloads against.
    #[serde(default = "default_payload_root")]
    pub payload_root: String,
    pub entries: Vec<Entry>,
}

fn default_payload_root() -> String {
    DEFAULT_PAYLOAD_ROOT.to_string()
}
