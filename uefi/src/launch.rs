//! Starting a vendor firmware tool as a child UEFI image.
//!
//! BIOSLove's flashers are ordinary PE32+ EFI applications that its shell runs
//! from a `.nsh` script. `LoadImage`/`StartImage` runs the same binaries, so the
//! app never reimplements SPI programming, AFU command sets or EC protocols — it
//! decides *which* tool runs, with *what* arguments, against *which* verified
//! bytes.
//!
//! The tools do not read `LoadedImage->LoadOptions`: a GUID scan of all 372 on
//! the share found 360 that take argv from `EFI_SHELL_INTERFACE` (the EFI 1.10
//! protocol, which is what the shell shipped on the stick provides), 330 that
//! also accept `EFI_SHELL_PARAMETERS_PROTOCOL`, and only 3 that use the load
//! options. Both shell protocols are therefore synthesized onto the child's
//! image handle before it starts.

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

/// EFI 1.10 shell argument block, installed on the child's image handle.
///
/// A GUID scan of the 372 vendor tools on the share found only **3** that read
/// arguments from `LoadedImage->LoadOptions`; 360 look for this protocol and 330
/// for [`ShellParametersProtocol`]. Both get installed, so a tool built against
/// either shell generation sees its argv.
#[repr(C)]
struct ShellInterface {
    image_handle: *mut core::ffi::c_void,
    info: *mut core::ffi::c_void,
    argv: *const *const u16,
    argc: usize,
    redir_argv: *const *const u16,
    redir_argc: usize,
    std_in: *mut core::ffi::c_void,
    std_out: *mut core::ffi::c_void,
    std_err: *mut core::ffi::c_void,
    arg_info: *mut ShellArgInfo,
    /// EFI BOOLEAN is one byte.
    echo_on: u8,
}

#[repr(C)]
struct ShellArgInfo {
    attributes: u32,
}

const SHELL_INTERFACE_GUID: uefi::Guid = uefi::guid!("47c7b223-c42a-11d2-8e57-00a0c969723b");

/// Owns everything the child's argv points at. Dropping it uninstalls the
/// protocols, so it must outlive the image.
pub struct ShellArgs {
    image: Handle,
    // argv points into these; neither is touched after construction.
    _strings: Vec<CString16>,
    _argv: Vec<*const u16>,
    _arg_info: Vec<ShellArgInfo>,
    params: Box<uefi_raw::protocol::shell_params::ShellParametersProtocol>,
    iface: Box<ShellInterface>,
    installed_params: bool,
    installed_iface: bool,
}

impl Drop for ShellArgs {
    fn drop(&mut self) {
        if self.installed_params {
            let _ = unsafe {
                boot::uninstall_protocol_interface(
                    self.image,
                    &uefi_raw::protocol::shell_params::ShellParametersProtocol::GUID,
                    (&raw const *self.params).cast(),
                )
            };
        }
        if self.installed_iface {
            let _ = unsafe {
                boot::uninstall_protocol_interface(
                    self.image,
                    &SHELL_INTERFACE_GUID,
                    (&raw const *self.iface).cast(),
                )
            };
        }
    }
}

/// Split a command line into argv, keeping double-quoted runs whole.
fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(core::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Build both shell argument protocols and install them on `image`.
fn install_shell_args(image: Handle, command_line: &str) -> Result<ShellArgs, String> {
    let strings: Vec<CString16> = split_args(command_line)
        .iter()
        .map(|a| CString16::try_from(a.as_str()))
        .collect::<Result<_, _>>()
        .map_err(|_| format!("arguments are not valid UCS-2: {command_line}"))?;
    let argv: Vec<*const u16> = strings
        .iter()
        .map(|s| s.as_slice_with_nul().as_ptr().cast())
        .collect();
    let arg_info: Vec<ShellArgInfo> = strings
        .iter()
        .map(|_| ShellArgInfo { attributes: 0 })
        .collect();

    let mut params = Box::new(uefi_raw::protocol::shell_params::ShellParametersProtocol {
        argv: argv.as_ptr(),
        argc: argv.len(),
        std_in: core::ptr::null(),
        std_out: core::ptr::null(),
        std_err: core::ptr::null(),
    });

    // The child's own LoadedImage, which the 1.10 block exposes as `Info`.
    let info = unsafe {
        boot::open_protocol::<LoadedImage>(
            boot::OpenProtocolParams {
                handle: image,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
    }
    .map(|li| (&raw const *li).cast_mut().cast())
    .unwrap_or(core::ptr::null_mut());

    let mut iface = Box::new(ShellInterface {
        image_handle: image.as_ptr(),
        info,
        argv: argv.as_ptr(),
        argc: argv.len(),
        redir_argv: core::ptr::null(),
        redir_argc: 0,
        std_in: core::ptr::null_mut(),
        std_out: core::ptr::null_mut(),
        std_err: core::ptr::null_mut(),
        arg_info: arg_info.as_ptr().cast_mut(),
        echo_on: 1,
    });

    let installed_params = unsafe {
        boot::install_protocol_interface(
            Some(image),
            &uefi_raw::protocol::shell_params::ShellParametersProtocol::GUID,
            (&raw mut *params).cast(),
        )
    }
    .is_ok();
    let installed_iface = unsafe {
        boot::install_protocol_interface(
            Some(image),
            &SHELL_INTERFACE_GUID,
            (&raw mut *iface).cast(),
        )
    }
    .is_ok();

    if !installed_params && !installed_iface {
        return Err("could not install either shell argument protocol".into());
    }

    Ok(ShellArgs {
        image,
        _strings: strings,
        _argv: argv,
        _arg_info: arg_info,
        params,
        iface,
        installed_params,
        installed_iface,
    })
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

    // Almost every vendor tool takes argv from a shell protocol, not from the
    // load options set above.
    let args = install_shell_args(image, command_line).map_err(|e| {
        let _ = boot::unload_image(image);
        e
    })?;

    let result = boot::start_image(image);
    // Uninstall before the handle goes away; only reached if the child returned.
    drop(args);
    let _ = boot::unload_image(image);
    drop(options);

    match result {
        Ok(()) => Ok(Ran {
            status: Status::SUCCESS,
        }),
        Err(e) => Ok(Ran { status: e.status() }),
    }
}
