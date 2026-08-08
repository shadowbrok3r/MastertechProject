//! `EFI_FILE_PROTOCOL` shims over the console, for the standard streams a shell
//! normally hands a child image.
//!
//! Vendor flashers on the share are EDK2 **StdLib** applications — AFU's own
//! embedded strings name `Edk2Libc\StdLib\LibC\...` and `GdkCoreSourcePkg\Main.cpp`.
//! StdLib brings up a C runtime before `main`, and that setup wires `stdin`,
//! `stdout` and `stderr` from the shell's `StdIn`/`StdOut`/`StdErr` handles.
//! Handed nulls it never reaches the program: the tool hangs before printing so
//! much as a usage banner, with the panel lit and nothing on it.
//!
//! There is no real shell here, so these are the smallest possible file
//! protocols that behave like a console: `Write` forwards to `EFI_SIMPLE_TEXT_OUTPUT`,
//! `Read` pulls keystrokes from `EFI_SIMPLE_TEXT_INPUT`, and everything a
//! console cannot do answers `UNSUPPORTED` rather than hanging.

use uefi::{Status, boot};
use uefi_raw::Char16;
use uefi_raw::protocol::file_system::{
    FileAttribute, FileMode, FileProtocolRevision, FileProtocolV1,
};

/// Which console stream a shim is standing in for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    In,
    Out,
    Err,
}

/// A file protocol plus the stream it serves. `proto` is first so a `This`
/// pointer from firmware casts straight back to the owning shim.
#[repr(C)]
pub struct ConsoleFile {
    proto: FileProtocolV1,
    stream: Stream,
}

impl ConsoleFile {
    /// Protocol pointer to hand to a child, valid while `self` is alive.
    pub fn as_ptr(&self) -> *mut FileProtocolV1 {
        (&raw const self.proto).cast_mut()
    }
}

/// Recover the shim from a `This` pointer.
///
/// # Safety
/// `this` must have come from [`ConsoleFile::as_ptr`] on a live shim.
unsafe fn shim<'a>(this: *mut FileProtocolV1) -> Option<&'a ConsoleFile> {
    if this.is_null() {
        return None;
    }
    Some(unsafe { &*this.cast::<ConsoleFile>() })
}

unsafe extern "efiapi" fn open(
    _this: *mut FileProtocolV1,
    _new: *mut *mut FileProtocolV1,
    _name: *const Char16,
    _mode: FileMode,
    _attr: FileAttribute,
) -> Status {
    // A console stream has no directory to open a name against.
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn close(_this: *mut FileProtocolV1) -> Status {
    // The streams outlive every child; closing one must not tear it down.
    Status::SUCCESS
}

unsafe extern "efiapi" fn delete(_this: *mut FileProtocolV1) -> Status {
    Status::WARN_DELETE_FAILURE
}

/// Fill `buffer` with keystrokes as UCS-2. Returns 0 bytes when nothing is
/// pending, which is how a non-blocking console read reports "no input".
unsafe extern "efiapi" fn read(
    this: *mut FileProtocolV1,
    buffer_size: *mut usize,
    buffer: *mut core::ffi::c_void,
) -> Status {
    let Some(s) = (unsafe { shim(this) }) else {
        return Status::INVALID_PARAMETER;
    };
    if buffer_size.is_null() {
        return Status::INVALID_PARAMETER;
    }
    if s.stream != Stream::In {
        // Reading an output stream yields end-of-file, not an error: StdLib
        // probes streams and must not take a failure as fatal.
        unsafe { *buffer_size = 0 };
        return Status::SUCCESS;
    }
    let cap = unsafe { *buffer_size } / core::mem::size_of::<u16>();
    if cap == 0 || buffer.is_null() {
        unsafe { *buffer_size = 0 };
        return Status::SUCCESS;
    }
    let mut written = 0usize;
    uefi::system::with_stdin(|stdin| {
        while written < cap {
            match stdin.read_key() {
                Ok(Some(uefi::proto::console::text::Key::Printable(c))) => {
                    let ch: char = c.into();
                    let mut buf = [0u16; 2];
                    for unit in ch.encode_utf16(&mut buf).iter() {
                        if written < cap {
                            unsafe { *(buffer.cast::<u16>().add(written)) = *unit };
                            written += 1;
                        }
                    }
                }
                // Special keys have no UCS-2 form; stop rather than spin.
                _ => break,
            }
        }
    });
    unsafe { *buffer_size = written * core::mem::size_of::<u16>() };
    Status::SUCCESS
}

/// Forward `buffer` (UCS-2, length in bytes) to the console.
unsafe extern "efiapi" fn write(
    this: *mut FileProtocolV1,
    buffer_size: *mut usize,
    buffer: *const core::ffi::c_void,
) -> Status {
    let Some(s) = (unsafe { shim(this) }) else {
        return Status::INVALID_PARAMETER;
    };
    if buffer_size.is_null() {
        return Status::INVALID_PARAMETER;
    }
    let units = unsafe { *buffer_size } / core::mem::size_of::<u16>();
    if s.stream == Stream::In {
        unsafe { *buffer_size = 0 };
        return Status::UNSUPPORTED;
    }
    if units == 0 || buffer.is_null() {
        return Status::SUCCESS;
    }
    let src = unsafe { core::slice::from_raw_parts(buffer.cast::<u16>(), units) };
    // output_string needs a NUL terminator, and the caller's slice has none.
    // Chunked so an unbounded write cannot demand one huge allocation.
    const CHUNK: usize = 256;
    let mut owned: Vec<u16> = Vec::with_capacity(CHUNK + 1);
    uefi::system::with_stdout(|out| {
        for part in src.chunks(CHUNK) {
            owned.clear();
            for &u in part {
                // A bare LF leaves the cursor mid-line on EFI consoles.
                if u == b'\n' as u16 {
                    owned.push(b'\r' as u16);
                }
                if u != 0 {
                    owned.push(u);
                }
            }
            owned.push(0);
            let s = unsafe { uefi::CStr16::from_ptr(owned.as_ptr().cast()) };
            let _ = out.output_string(s);
        }
    });
    Status::SUCCESS
}

unsafe extern "efiapi" fn get_position(_this: *const FileProtocolV1, pos: *mut u64) -> Status {
    // Streams are not seekable; 0 keeps callers that only log it happy.
    if !pos.is_null() {
        unsafe { *pos = 0 };
    }
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_position(_this: *mut FileProtocolV1, _pos: u64) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn get_info(
    _this: *mut FileProtocolV1,
    _kind: *const uefi_raw::Guid,
    size: *mut usize,
    _buffer: *mut core::ffi::c_void,
) -> Status {
    if !size.is_null() {
        unsafe { *size = 0 };
    }
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_info(
    _this: *mut FileProtocolV1,
    _kind: *const uefi_raw::Guid,
    _size: usize,
    _buffer: *const core::ffi::c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn flush(_this: *mut FileProtocolV1) -> Status {
    Status::SUCCESS
}

/// Build a console-backed stream. Boxed by the caller so the pointer handed to
/// the child stays put.
pub fn console_stream(stream: Stream) -> Box<ConsoleFile> {
    Box::new(ConsoleFile {
        proto: FileProtocolV1 {
            // Revision 1: no OpenEx/ReadEx/WriteEx/FlushEx, so a caller that
            // honours the revision never reaches past this struct.
            revision: FileProtocolRevision::REVISION_1,
            open,
            close,
            delete,
            read,
            write,
            get_position,
            set_position,
            get_info,
            set_info,
            flush,
        },
        stream,
    })
}

/// The three streams a shell hands a child image.
pub struct StdStreams {
    pub stdin: Box<ConsoleFile>,
    pub stdout: Box<ConsoleFile>,
    pub stderr: Box<ConsoleFile>,
}

impl StdStreams {
    pub fn new() -> Self {
        Self {
            stdin: console_stream(Stream::In),
            stdout: console_stream(Stream::Out),
            stderr: console_stream(Stream::Err),
        }
    }
}

/// Keeps `boot` in use when the module is compiled without other references.
#[allow(dead_code)]
fn _assert_boot_linked() {
    let _ = boot::image_handle;
}
