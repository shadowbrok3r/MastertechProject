//! Bench harness for `launch.rs`, run under QEMU/OVMF.
//!
//! Boots as `\EFI\BOOT\BOOTX64.EFI`, reads `\launchtest.txt` for a tool path and
//! arguments, and drives the same load/verify/start path the Flash tab uses. It
//! exists to answer whether a real vendor flasher sees the argv the app
//! synthesizes, without a laptop and without writing firmware.
//!
//! `\launchtest.txt` is two lines: the volume path of the tool, then its
//! arguments (an empty second line runs it with none, which is the control).

#![feature(uefi_std)]
// Pulls in whole modules for a few functions; the rest is legitimately unused.
#![allow(dead_code)]

use uefi::boot;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;

#[path = "../capsule.rs"]
mod capsule;
#[path = "../flashstate.rs"]
mod flashstate;
#[path = "../launch.rs"]
mod launch;
#[path = "../pecheck.rs"]
mod pecheck;

/// `capsule` logs through the crate root; here that is just the console.
fn logln(s: String) {
    println!("{s}");
}

const CONFIG: &str = "\\launchtest.txt";

/// Hand the `uefi` crate the table and handle that `uefi_std` owns.
fn adopt_uefi_env() {
    unsafe {
        uefi::table::set_system_table(std::os::uefi::env::system_table().as_ptr().cast());
        let ih = uefi::Handle::from_ptr(std::os::uefi::env::image_handle().as_ptr().cast()).unwrap();
        uefi::boot::set_image_handle(ih);
    }
}

fn main() {
    adopt_uefi_env();
    println!("=== launchtest: vendor-tool argv harness ===");

    let Some((cfg, volume)) = read_any_volume(CONFIG) else {
        println!("FAIL could not read {CONFIG}");
        finish();
        return;
    };
    let text = String::from_utf8_lossy(&cfg);
    let mut lines = text.lines();
    let tool = lines.next().unwrap_or("").trim().to_string();
    let args = lines.next().unwrap_or("").trim().to_string();
    if tool.is_empty() {
        println!("FAIL {CONFIG} has no tool path on line 1");
        finish();
        return;
    }
    println!("tool : {tool}");
    println!("args : {}", if args.is_empty() { "<none - control run>" } else { &args });

    flashstate_selftest(volume);

    let Some(bytes) = read_on_volume(volume, &tool) else {
        println!("FAIL could not read {tool}");
        finish();
        return;
    };

    let dry = launch::dry_run(&bytes);
    println!(
        "pe   : {} ({} bytes) sha256:{}",
        dry.verdict,
        dry.bytes,
        &dry.sha256[..16]
    );
    if !dry.ok {
        println!("FAIL {}", dry.detail);
        finish();
        return;
    }

    let mut dp_buf: Vec<u8> = Vec::new();
    let cpath = uefi::CString16::try_from(tool.as_str()).ok();
    let dp = cpath
        .as_ref()
        .and_then(|c| launch::file_device_path(volume, c, &mut dp_buf).ok());
    println!("dpath: {}", if dp.is_some() { "built" } else { "NOT BUILT" });

    let command_line = if args.is_empty() {
        base_name(&tool).to_string()
    } else {
        format!("{} {}", base_name(&tool), args)
    };
    println!("cmd  : {command_line}");
    println!("--- child output follows ---");

    match launch::run(&bytes, &command_line, dp) {
        Ok(ran) => {
            println!("--- child returned ---");
            println!("RESULT returned status={:?}", ran.status);
        }
        Err(e) => println!("RESULT launch refused: {e}"),
    }
    finish();
}

fn base_name(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

/// Round-trip the multi-reboot state through a real NV UEFI variable, and
/// append one line to the on-volume flash log.
fn flashstate_selftest(volume: uefi::Handle) {
    println!("--- flashstate self-test ---");
    // Two-phase: a state found on entry was written by an earlier boot, which is
    // the only way to show the variable survives a power cycle.
    match flashstate::load() {
        Some(r) => {
            println!(
                "  found : SURVIVED REBOOT - {} step {}/{} last={}",
                r.folder,
                r.next_step,
                r.total_steps,
                if r.last_status.is_empty() { "-" } else { &r.last_status }
            );
            match flashstate::clear() {
                Ok(()) => println!("  clear : ok"),
                Err(e) => println!("  clear : FAIL {e}"),
            }
            println!("  after : {:?}", flashstate::load().map(|s| s.folder));
        }
        None => {
            let st = flashstate::FlashState {
                folder: "QEMUTEST".to_string(),
                serial: "SN-TEST-1".to_string(),
                next_step: 1,
                total_steps: 3,
                recipe_sha256: "deadbeef".to_string(),
                last_status: "WARN_DELETE_FAILURE".to_string(),
            };
            match flashstate::save(&st) {
                Ok(()) => println!("  saved : nothing stored, wrote one - reboot to check"),
                Err(e) => println!("  save  : FAIL {e}"),
            }
        }
    }
    flashstate::append_log(
        volume,
        &flashstate::LogEntry {
            serial: "SN-TEST-1",
            folder: "QEMUTEST",
            step: 1,
            kind: "ec",
            exec: "selftest.efi",
            args: "-a",
            exec_sha256: "abc123",
            outcome: "selftest",
        },
    );
    println!("  log   : append attempted");
}

/// Give the console time to flush over serial, then power the VM off.
fn finish() {
    println!("=== launchtest done ===");
    boot::stall(core::time::Duration::from_secs(3));
    uefi::runtime::reset(uefi::runtime::ResetType::SHUTDOWN, uefi::Status::SUCCESS, None);
}

fn open_root(handle: uefi::Handle) -> Option<Directory> {
    let mut sfs = unsafe {
        boot::open_protocol::<SimpleFileSystem>(
            boot::OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()?;
    sfs.open_volume().ok()
}

/// Read `path` off whichever volume has it, returning the bytes and that volume.
fn read_any_volume(path: &str) -> Option<(Vec<u8>, uefi::Handle)> {
    for h in boot::find_handles::<SimpleFileSystem>().ok()? {
        if let Some(b) = read_on_volume(h, path) {
            return Some((b, h));
        }
    }
    None
}

fn read_on_volume(volume: uefi::Handle, path: &str) -> Option<Vec<u8>> {
    let mut root = open_root(volume)?;
    let cpath = uefi::CString16::try_from(path).ok()?;
    let handle = root
        .open(&cpath, FileMode::Read, FileAttribute::empty())
        .ok()?;
    let FileType::Regular(mut f) = handle.into_type().ok()? else {
        return None;
    };
    let size = f
        .get_boxed_info::<uefi::proto::media::file::FileInfo>()
        .ok()?
        .file_size() as usize;
    let mut buf = vec![0u8; size];
    match f.read(&mut buf) {
        Ok(n) if n == size => Some(buf),
        _ => None,
    }
}
