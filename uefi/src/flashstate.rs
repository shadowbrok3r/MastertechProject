//! Multi-reboot flash state.
//!
//! Clevo and Tongfang recipes run EC, then ME, then BIOS, with a reboot or a
//! power-off between steps: 44 of the 435 UEFI-lane steps on the share do not
//! return. Today the tech has to remember which step they were on across that
//! power cycle, with nothing on screen to tell them. This keeps the position in
//! an NV UEFI variable keyed to the machine's serial, and appends every attempt
//! to a log on the payload volume so a machine that dies mid-recipe still leaves
//! a record.

use serde::{Deserialize, Serialize};
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::runtime::{self, VariableVendor};
use uefi::{boot, cstr16};
use uefi_raw::table::runtime::VariableAttributes;

use crate::logln;

/// Mastertech vendor namespace for firmware-owned variables.
const VENDOR: VariableVendor = VariableVendor(uefi::guid!("6d1a4f2c-9b53-4f7a-8d21-0c3e5a7b9114"));

/// Directory on the payload volume holding per-machine flash logs.
const LOG_DIR: &str = "\\bioslove\\log";

/// Where a recipe got to, and on which machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashState {
    pub folder: String,
    /// Machine identity this state belongs to; a mismatch means a different box.
    pub serial: String,
    /// Index of the next step to run.
    pub next_step: usize,
    pub total_steps: usize,
    /// Digest of the recipe, so an index update invalidates a stale resume.
    pub recipe_sha256: String,
    /// EFI status text from the last step that returned.
    #[serde(default)]
    pub last_status: String,
    /// A child image was started and has not returned. Written before the
    /// launch and cleared after, so a hang or a power cut leaves it set: on the
    /// next boot that is the only evidence of what the machine was doing, since
    /// a blocked child stalls the polled network stack and takes the console
    /// link, frame streaming and even ICMP with it.
    #[serde(default)]
    pub running: bool,
    /// Command line of the child recorded in `running`.
    #[serde(default)]
    pub running_cmd: String,
}

impl FlashState {
    /// The recipe has no steps left to run.
    pub fn complete(&self) -> bool {
        self.next_step >= self.total_steps
    }
}

/// Read the saved position, if any.
pub fn load() -> Option<FlashState> {
    let (bytes, _) = runtime::get_variable_boxed(cstr16!("MtechFlashState"), &VENDOR).ok()?;
    match serde_json::from_slice::<FlashState>(&bytes) {
        Ok(s) => Some(s),
        Err(e) => {
            logln(format!("flashstate: stored value is unreadable ({e})"));
            None
        }
    }
}

/// Persist the position across a reboot or power-off.
pub fn save(state: &FlashState) -> Result<(), String> {
    let json = serde_json::to_vec(state).map_err(|e| format!("encode flash state: {e}"))?;
    runtime::set_variable(
        cstr16!("MtechFlashState"),
        &VENDOR,
        VariableAttributes::NON_VOLATILE
            | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS,
        &json,
    )
    .map_err(|e| format!("save flash state: {e:?}"))
}

/// Forget the saved position. Deleting a variable is a zero-length write.
pub fn clear() -> Result<(), String> {
    match runtime::set_variable(
        cstr16!("MtechFlashState"),
        &VENDOR,
        VariableAttributes::NON_VOLATILE
            | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS,
        &[],
    ) {
        Ok(()) => Ok(()),
        // Absent already is the state the caller wanted.
        Err(e) if e.status() == uefi::Status::NOT_FOUND => Ok(()),
        Err(e) => Err(format!("clear flash state: {e:?}")),
    }
}

/// One line of the on-volume flash log.
#[derive(Debug, Serialize)]
pub struct LogEntry<'a> {
    pub serial: &'a str,
    pub folder: &'a str,
    pub step: usize,
    pub kind: &'a str,
    pub exec: &'a str,
    pub args: &'a str,
    pub exec_sha256: &'a str,
    pub outcome: &'a str,
}

/// Append an attempt to `\bioslove\log\<serial>.jsonl` on `volume`.
///
/// Best effort: a read-only or full stick must never block a flash, so failures
/// are logged to the console ring and swallowed.
pub fn append_log(volume: uefi::Handle, entry: &LogEntry<'_>) {
    if let Err(e) = write_log(volume, entry) {
        logln(format!("flashstate: log not written ({e})"));
    }
}

fn write_log(volume: uefi::Handle, entry: &LogEntry<'_>) -> Result<(), String> {
    let mut line = serde_json::to_vec(entry).map_err(|e| format!("encode log entry: {e}"))?;
    line.push(b'\n');

    let mut sfs = unsafe {
        boot::open_protocol::<SimpleFileSystem>(
            boot::OpenProtocolParams {
                handle: volume,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
    }
    .map_err(|e| format!("open volume: {e:?}"))?;
    let mut root = sfs.open_volume().map_err(|e| format!("open root: {e:?}"))?;

    ensure_dir(&mut root, LOG_DIR);

    let name = format!("{LOG_DIR}\\{}.jsonl", sanitize(&entry.serial_or_unknown()));
    let path = uefi::CString16::try_from(name.as_str()).map_err(|_| "log name is not UCS-2")?;
    let handle = root
        .open(&path, FileMode::CreateReadWrite, FileAttribute::empty())
        .map_err(|e| format!("open {name}: {e:?}"))?;
    let FileType::Regular(mut f) = handle.into_type().map_err(|e| format!("{e:?}"))? else {
        return Err(format!("{name} is a directory"));
    };
    let end = f
        .get_boxed_info::<uefi::proto::media::file::FileInfo>()
        .map(|i| i.file_size())
        .unwrap_or(0);
    f.set_position(end).map_err(|e| format!("seek: {e:?}"))?;
    f.write(&line).map_err(|e| format!("write: {e:?}"))?;
    f.flush().map_err(|e| format!("flush: {e:?}"))?;
    Ok(())
}

impl LogEntry<'_> {
    fn serial_or_unknown(&self) -> String {
        if self.serial.trim().is_empty() {
            "unknown".to_string()
        } else {
            self.serial.trim().to_string()
        }
    }
}

/// Create each component of `path`. `File::open` with `Create` makes only the
/// leaf, so a missing parent fails the whole write with NOT_FOUND.
pub fn ensure_dir(root: &mut uefi::proto::media::file::Directory, path: &str) {
    let mut cur = String::new();
    for part in path.trim_matches('\\').split('\\') {
        cur.push('\\');
        cur.push_str(part);
        if let Ok(p) = uefi::CString16::try_from(cur.as_str()) {
            let _ = root.open(&p, FileMode::CreateReadWrite, FileAttribute::DIRECTORY);
        }
    }
}

/// Keep a serial safe for a FAT filename.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .take(64)
        .collect()
}
