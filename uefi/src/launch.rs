//! Starting a vendor firmware tool as a child UEFI image.
//!
//! BIOSLove's flashers are ordinary PE32+ EFI applications that the UEFI Shell
//! runs from a `.nsh` script. `LoadImage`/`StartImage` runs the same binaries
//! without a shell, so the app never reimplements SPI programming, AFU command
//! sets or EC protocols — it decides *which* tool runs, with *what* arguments,
//! against *which* verified bytes.

use uefi::boot;
use uefi::boot::LoadImageSource;
use uefi::proto::console::text::OutputMode;
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{DevicePathBuilder, media::FilePath};
use uefi::proto::loaded_image::LoadedImage;
use uefi::{CStr16, CString16, Handle, Status};

use crate::pecheck;

/// Structural verdict on a tool binary, taken without loading it.
pub struct DryRun {
    pub ok: bool,
    pub verdict: &'static str,
    pub detail: String,
    pub sha256: String,
    pub bytes: usize,
}

/// Parse a candidate tool and hash it. Loads nothing and starts nothing.
pub fn dry_run(bytes: &[u8]) -> DryRun {
    let v = pecheck::validate(bytes, bytes.len() as u64);
    DryRun {
        ok: pecheck::is_valid(&v),
        verdict: pecheck::verdict_label(&v),
        detail: pecheck::verdict_detail(&v),
        sha256: crate::capsule::sha256_hex(bytes),
        bytes: bytes.len(),
    }
}

/// Device path of `file` on the volume behind `volume`, built into `buf`.
///
/// A child loaded from a bare buffer has no device handle, so a vendor flasher
/// cannot find the ROM sitting next to it. Handing `LoadImage` this path sets
/// the child's `LoadedImage->DeviceHandle` and `FilePath`, which is how the
/// tool resolves its own payload.
pub fn file_device_path<'a>(
    volume: Handle,
    file: &CStr16,
    buf: &'a mut Vec<u8>,
) -> Result<&'a DevicePath, String> {
    let volume_path = boot::open_protocol_exclusive::<DevicePath>(volume)
        .map_err(|e| format!("volume has no device path ({:?})", e.status()))?;

    let mut b = DevicePathBuilder::with_vec(buf);
    for node in volume_path.node_iter() {
        b = b
            .push(&node)
            .map_err(|e| format!("device path node rejected: {e:?}"))?;
    }
    b.push(&FilePath { path_name: file })
        .map_err(|e| format!("file path node rejected: {e:?}"))?
        .finalize()
        .map_err(|e| format!("device path build failed: {e:?}"))
}

/// Console text mode captured before handing the screen to a child image.
pub struct ConsoleGuard {
    mode: Option<OutputMode>,
}

/// Record the current text mode so it can be put back after a child returns.
pub fn save_console() -> ConsoleGuard {
    let mode = uefi::system::with_stdout(|out| out.current_mode().ok().flatten());
    ConsoleGuard { mode }
}

impl ConsoleGuard {
    /// Restore the saved mode and blank the screen the child wrote over.
    pub fn restore(self) {
        uefi::system::with_stdout(|out| {
            if let Some(m) = self.mode {
                let _ = out.set_mode(m);
            }
            let _ = out.clear();
        });
    }
}

/// A child image ran and handed control back with this status. A tool that
/// resets or powers off the machine never produces one.
pub struct Ran {
    pub status: Status,
}

/// Load `bytes` as an EFI application, hand it `command_line`, and start it.
///
/// `file_path` is the on-volume path the bytes came from; it gives the child a
/// device handle so it can open sibling files. Verified bytes are still what
/// runs — the path is provenance, not the source.
///
/// Blocks until the child returns; many flashers never do, resetting or powering
/// off the machine instead. The caller owns the console: save it first and
/// restore it after, because the child writes directly to ConOut.
pub fn run(bytes: &[u8], command_line: &str, file_path: Option<&DevicePath>) -> Result<Ran, String> {
    let image = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromBuffer {
            buffer: bytes,
            file_path,
        },
    )
    .map_err(|e| match e.status() {
        Status::SECURITY_VIOLATION | Status::ACCESS_DENIED => {
            "refused by platform security policy - Secure Boot rejects unsigned vendor tools".into()
        }
        Status::UNSUPPORTED | Status::INVALID_PARAMETER => {
            format!("not a loadable EFI application ({:?})", e.status())
        }
        s => format!("LoadImage failed: {s:?}"),
    })?;

    // UEFI passes the whole command line, argv[0] included, as UCS-2.
    let options = CString16::try_from(command_line)
        .map_err(|_| format!("arguments are not valid UCS-2: {command_line}"))?;
    let units = options.as_slice_with_nul();
    let size = (units.len() * core::mem::size_of::<uefi::Char16>()) as u32;

    {
        let mut li = unsafe {
            boot::open_protocol::<LoadedImage>(
                boot::OpenProtocolParams {
                    handle: image,
                    agent: boot::image_handle(),
                    controller: None,
                },
                boot::OpenProtocolAttributes::GetProtocol,
            )
        }
        .map_err(|e| {
            let _ = boot::unload_image(image);
            format!("child image has no LoadedImage protocol ({:?})", e.status())
        })?;
        // SAFETY: `options` outlives `start_image` below, and the protocol only
        // records the pointer; the child reads it while it runs.
        unsafe { li.set_load_options(units.as_ptr().cast(), size) };
    }

    let result = boot::start_image(image);
    // Only meaningful if the child returned; a resetting tool never gets here.
    let _ = boot::unload_image(image);
    drop(options);

    match result {
        Ok(()) => Ok(Ran {
            status: Status::SUCCESS,
        }),
        Err(e) => Ok(Ran { status: e.status() }),
    }
}
