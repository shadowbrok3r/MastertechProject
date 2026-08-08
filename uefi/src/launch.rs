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
    // Both protocols point at these; they must outlive the child.
    _streams: crate::shellio::StdStreams,
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
fn install_shell_args(
    image: Handle,
    command_line: &str,
    legacy_iface: bool,
) -> Result<ShellArgs, String> {
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

    // EDK2 StdLib brings up its C runtime from these before `main` runs; given
    // nulls it never reaches the program, so the tool hangs with a lit panel and
    // nothing drawn. Console-backed shims are the minimum that lets it start.
    let streams = crate::shellio::StdStreams::new();

    let mut params = Box::new(uefi_raw::protocol::shell_params::ShellParametersProtocol {
        argv: argv.as_ptr(),
        argc: argv.len(),
        std_in: streams.stdin.as_ptr().cast(),
        std_out: streams.stdout.as_ptr().cast(),
        std_err: streams.stderr.as_ptr().cast(),
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
        std_in: streams.stdin.as_ptr().cast(),
        std_out: streams.stdout.as_ptr().cast(),
        std_err: streams.stderr.as_ptr().cast(),
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
    // Only for vendor tools. A child that is itself an EFI Shell must not see
    // the 1.10 block: a real shell parent installs the 2.0 protocol alone, and
    // that is the environment in which `exit` returns instead of dropping to a
    // prompt.
    let installed_iface = legacy_iface
        && unsafe {
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
        _streams: streams,
        params,
        iface,
        installed_params,
        installed_iface,
    })
}

/// Exit code the generated script returns when it landed on the wrong volume,
/// chosen to be distinguishable from anything a vendor tool returns.
const WRONG_VOLUME: usize = 77;

/// Where the shell binary is kept on the staging volume.
pub const SHELL_ON_VOLUME: &str = "\\bioslove\\shell.efi";

/// Marker written beside a step's payloads so the script can confirm the volume.
const VOLUME_MARKER: &str = "mtech.tag";

/// Outcome of running a command through a real EFI Shell.
pub struct ShellRan {
    pub status: Status,
    /// Which `fsN` the shell mapped the staging volume to.
    pub fs_index: usize,
    /// Script the shell executed, for the log.
    pub script: String,
}

/// Order to try `fsN` in: the shell numbers filesystems by walking
/// `LocateHandleBuffer(ByProtocol, SimpleFileSystem)`, which is the same call
/// this makes, so the staging volume's index in that list is almost certainly
/// its `fsN`. "Almost" is why the script verifies it and the caller retries.
fn fs_candidates(volume: Handle) -> Vec<usize> {
    let all = boot::find_handles::<uefi::proto::media::fs::SimpleFileSystem>().unwrap_or_default();
    let best = all.iter().position(|h| *h == volume);
    let mut order: Vec<usize> = best.into_iter().collect();
    order.extend((0..all.len().max(1)).filter(|i| Some(*i) != best));
    order
}

/// Script that pins the shell to the right volume, gives it a working directory,
/// and runs one command.
///
/// A freshly launched shell has **no** current directory — `cd` reports "Current
/// directory not specified" and every relative path fails. Vendor tools resolve
/// their ROM next to themselves, so establishing the directory is not optional;
/// this is the same `fsN` walk BIOSLove's own `startup.nsh` does.
fn shell_script(fs: usize, work_dir: &str, command_line: &str) -> String {
    let mut s = String::new();
    s.push_str("@echo -off\r\n");
    s.push_str(&format!(
        "if not exist fs{fs}:{work_dir}\\{VOLUME_MARKER} then\r\n  exit {WRONG_VOLUME}\r\nendif\r\n"
    ));
    s.push_str(&format!("fs{fs}:\r\n"));
    s.push_str(&format!("cd {work_dir}\r\n"));
    s.push_str(&format!("{command_line}\r\n"));
    // Surfaced on the console; the shell's own exit status is what we capture.
    s.push_str("echo MTECH_RC=%lasterror%\r\n");
    // `exit` in a script ends the script, not the shell -- this shell rejects
    // `-exit` as a flag and its line editor reads ConIn directly, so a canned
    // stdin cannot answer for the operator. The prompt is therefore expected,
    // and the tech is told exactly what to do with it.
    // Quoted: EDK2 `echo` parses a leading '-' in any word as a flag and fails
    // the line, which is why BIOSLove's own menu scripts quote their banners.
    s.push_str("echo .\r\n");
    s.push_str("echo \"*** step finished. Type  exit  to return to Mastertech ***\"\r\n");
    s.push_str("echo .\r\n");
    s.push_str("exit\r\n");
    s
}

/// Run `command_line` through a real EFI Shell staged on `volume`.
///
/// Launching a vendor flasher directly from `LoadImage` does not work: these are
/// EDK2 StdLib applications that expect a full shell, and without one AFU hangs
/// before printing even a usage banner. Handing the job to the shell the tools
/// actually ship with removes that whole class of problem.
pub fn run_via_shell(
    shell_bytes: &[u8],
    volume: Handle,
    work_dir: &str,
    command_line: &str,
) -> Result<ShellRan, String> {
    crate::bioslove::write_file_on_volume(
        volume,
        &format!("{work_dir}\\{VOLUME_MARKER}"),
        b"mtech",
    )?;

    let mut last = String::from("no filesystem candidates");
    for fs in fs_candidates(volume) {
        let script = shell_script(fs, work_dir, command_line);
        let script_path = format!("{work_dir}\\run.nsh");
        crate::bioslove::write_file_on_volume(volume, &script_path, script.as_bytes())?;

        // The shell resolves the script by absolute path; its own image path is
        // only provenance, so it is kept out of the per-model directory.
        let mut dp_buf: Vec<u8> = Vec::new();
        let cpath = CString16::try_from(SHELL_ON_VOLUME)
            .map_err(|_| "shell path is not valid UCS-2".to_string())?;
        let dp = file_device_path(volume, &cpath, &mut dp_buf).ok();
        // Flags are not an option: this shell rejects `-nostartup`/`-exit` as
        // commands in both protocol modes.
        let cmd = format!("shell.efi fs{fs}:{script_path}");

        match run(shell_bytes, &cmd, dp) {
            Ok(ran) if ran.status.0 == WRONG_VOLUME => {
                last = format!("fs{fs} is not the staging volume");
                continue;
            }
            Ok(ran) => {
                return Ok(ShellRan {
                    status: ran.status,
                    fs_index: fs,
                    script,
                });
            }
            Err(e) => return Err(e),
        }
    }
    Err(format!("could not locate the staging volume from the shell ({last})"))
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
    run_inner(bytes, command_line, file_path, true, true)
}

fn run_inner(
    bytes: &[u8],
    command_line: &str,
    file_path: Option<&DevicePath>,
    shell_args: bool,
    legacy_iface: bool,
) -> Result<Ran, String> {
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
    let args = if shell_args {
        Some(install_shell_args(image, command_line, legacy_iface).map_err(|e| {
            let _ = boot::unload_image(image);
            e
        })?)
    } else {
        None
    };

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
