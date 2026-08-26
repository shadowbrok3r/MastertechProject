//! Win32 output enumeration, per-output borderless windows, and desktop mode
//! changes for the display-path stressor.

#![cfg(all(feature = "gpu", target_os = "windows"))]

use std::marker::PhantomData;
use std::num::NonZeroIsize;
use std::sync::{Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayMonitors, EnumDisplaySettingsExW, GetMonitorInfoW,
    CDS_FULLSCREEN, CDS_TYPE, DEVMODEW, DEVMODE_FIELD_FLAGS, DISP_CHANGE_SUCCESSFUL,
    DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS,
    ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_OWNDC, CS_VREDRAW,
    HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNA, WM_CLOSE,
    WM_ERASEBKGND, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const CLASS_NAME: &str = "stress_kit_display_surface";

/// Serializes desktop mode changes; concurrent changes across adapters wedge
/// the display miniport rather than testing it.
static MODE_SET_LOCK: Mutex<()> = Mutex::new(());

/// How long a mode change waits its turn before giving up. Bounded because
/// `ChangeDisplaySettingsExW` itself can block indefinitely, and a blocking
/// `lock()` behind it made one wedged mode change wedge every later one —
/// including the restore that teardown depends on.
const MODE_LOCK_WAIT: Duration = Duration::from_secs(3);

/// Bounded acquisition of [`MODE_SET_LOCK`]. `None` means a sibling mode change
/// has held it past [`MODE_LOCK_WAIT`].
fn try_mode_lock(wait: Duration) -> Option<MutexGuard<'static, ()>> {
    let deadline = Instant::now() + wait;
    loop {
        match MODE_SET_LOCK.try_lock() {
            Ok(guard) => return Some(guard),
            // A panicking caller left it poisoned; the lock guards ordering,
            // not data, so taking it is safe.
            Err(std::sync::TryLockError::Poisoned(e)) => return Some(e.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {}
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// One attached display: its desktop rect, current mode, and the GDI device
/// name (`\\.\DISPLAY1`) that mode changes address.
#[derive(Debug, Clone)]
pub(super) struct Output {
    pub device: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
}

impl Output {
    pub(super) fn describe(&self) -> String {
        format!(
            "{} {}x{}@{}Hz{}",
            self.device,
            self.width,
            self.height,
            self.refresh_hz,
            if self.primary { " (primary)" } else { "" }
        )
    }
}

unsafe extern "system" fn monitor_enum(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> windows::core::BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<Output>) };
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo as *mut _) };
    if !ok.as_bool() {
        return true.into();
    }

    let device = from_wide(&info.szDevice);
    let rc = info.monitorInfo.rcMonitor;
    let refresh_hz = current_mode(&device).map(|m| m.dmDisplayFrequency).unwrap_or(0);
    out.push(Output {
        x: rc.left,
        y: rc.top,
        width: (rc.right - rc.left).max(1) as u32,
        height: (rc.bottom - rc.top).max(1) as u32,
        refresh_hz,
        // MONITORINFOF_PRIMARY
        primary: info.monitorInfo.dwFlags & 1 != 0,
        device,
    });
    true.into()
}

/// Every attached display, primary first.
pub(super) fn enumerate_outputs() -> Vec<Output> {
    let mut out: Vec<Output> = Vec::new();
    let ptr = &mut out as *mut Vec<Output> as isize;
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(monitor_enum), LPARAM(ptr));
    }
    out.sort_by_key(|o| (!o.primary, o.x, o.y));
    out
}

fn current_mode(device: &str) -> Option<DEVMODEW> {
    let name = wide(device);
    let mut mode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let ok = unsafe {
        EnumDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut mode,
            ENUM_DISPLAY_SETTINGS_FLAGS(0),
        )
    };
    ok.as_bool().then_some(mode)
}

/// Modes this display advertises at `width`x`height`, refresh rates ascending
/// and deduplicated. The current rate is included.
pub(super) fn refresh_modes_at(device: &str, width: u32, height: u32) -> Vec<u32> {
    let name = wide(device);
    let mut rates: Vec<u32> = Vec::new();
    for index in 0.. {
        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let ok = unsafe {
            EnumDisplaySettingsExW(
                PCWSTR(name.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(index),
                &mut mode,
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
        };
        if !ok.as_bool() {
            break;
        }
        if mode.dmPelsWidth == width && mode.dmPelsHeight == height && mode.dmDisplayFrequency > 1 {
            rates.push(mode.dmDisplayFrequency);
        }
    }
    rates.sort_unstable();
    rates.dedup();
    rates
}

/// Distinct resolutions this display advertises, largest first, restricted to
/// modes at or above 800x600 so a cycle cannot land the bench on a mode a tech
/// cannot read.
pub(super) fn resolutions(device: &str) -> Vec<(u32, u32)> {
    let name = wide(device);
    let mut modes: Vec<(u32, u32)> = Vec::new();
    for index in 0.. {
        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let ok = unsafe {
            EnumDisplaySettingsExW(
                PCWSTR(name.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(index),
                &mut mode,
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
        };
        if !ok.as_bool() {
            break;
        }
        if mode.dmPelsWidth >= 800 && mode.dmPelsHeight >= 600 {
            modes.push((mode.dmPelsWidth, mode.dmPelsHeight));
        }
    }
    modes.sort_unstable_by_key(|&(w, h)| std::cmp::Reverse(w as u64 * h as u64));
    modes.dedup();
    modes
}

/// Applies a temporary mode change. `CDS_FULLSCREEN` marks it app-owned so
/// Windows restores the desktop mode if this process dies mid-run.
pub(super) fn apply_mode(device: &str, width: u32, height: u32, hz: u32) -> Result<(), String> {
    // Skipped rather than queued: concurrent mode changes wedge the miniport,
    // so a caller that cannot get the turn must not proceed.
    let Some(_guard) = try_mode_lock(MODE_LOCK_WAIT) else {
        return Err(format!(
            "{device}: skipped a mode change, another display has held the mode-set turn for {}s",
            MODE_LOCK_WAIT.as_secs()
        ));
    };
    let name = wide(device);
    let Some(mut mode) = current_mode(device) else {
        return Err(format!("{device}: current mode unreadable"));
    };
    mode.dmPelsWidth = width;
    mode.dmPelsHeight = height;
    mode.dmDisplayFrequency = hz;
    mode.dmFields = DEVMODE_FIELD_FLAGS(
        DM_PELSWIDTH.0 | DM_PELSHEIGHT.0 | DM_DISPLAYFREQUENCY.0,
    );

    let result = unsafe {
        ChangeDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            Some(&mode),
            None,
            CDS_FULLSCREEN,
            None,
        )
    };
    if result == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(format!("{device}: mode set to {width}x{height}@{hz} returned {}", result.0))
    }
}

/// Drops the app-owned mode and returns the display to its registry setting.
/// Proceeds without the turn if it cannot be had: leaving a display on a
/// stressor-chosen mode is worse than overlapping one restore with a wedged
/// change that is never going to finish.
pub(super) fn restore_mode(device: &str) {
    let guard = try_mode_lock(MODE_LOCK_WAIT);
    if guard.is_none() {
        log::warn!(
            "[stress-kit/gpu_display] {device}: restoring the mode without the mode-set turn; \
             a sibling change has held it for {}s",
            MODE_LOCK_WAIT.as_secs()
        );
    }
    let name = wide(device);
    let result =
        unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), None, None, CDS_TYPE(0), None) };
    if result != DISP_CHANGE_SUCCESSFUL {
        log::warn!(
            "[stress-kit/gpu_display] {device}: mode restore returned {}",
            result.0
        );
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // The stressor owns teardown; a stray close must not drop the surface.
        WM_CLOSE => LRESULT(0),
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

static REGISTER_CLASS: Once = Once::new();

fn register_class() -> Result<HINSTANCE, String> {
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
    let instance = HINSTANCE(module.0);
    REGISTER_CLASS.call_once(|| {
        let class = wide(CLASS_NAME);
        let desc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&desc) } == 0 {
            log::error!(
                "[stress-kit/gpu_display] RegisterClassExW failed: {}",
                std::io::Error::last_os_error()
            );
        }
    });
    Ok(instance)
}

/// A borderless always-on-top window covering one output. Thread-affine: Win32
/// delivers its messages only to the thread that created it, so it must be
/// created, pumped, and dropped on one thread.
pub(super) struct OutputWindow {
    hwnd: HWND,
    instance: HINSTANCE,
    _not_send: PhantomData<*const ()>,
}

impl OutputWindow {
    pub(super) fn new(output: &Output) -> Result<Self, String> {
        let instance = register_class()?;
        let class = wide(CLASS_NAME);
        let title = wide(&format!("stress-kit display path — {}", output.device));
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP | WS_VISIBLE,
                output.x,
                output.y,
                output.width as i32,
                output.height as i32,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW failed for {}: {e}", output.device))?;

        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
        Ok(Self { hwnd, instance, _not_send: PhantomData })
    }

    pub(super) fn raw_handle(&self) -> Result<wgpu::rwh::RawWindowHandle, String> {
        let hwnd = NonZeroIsize::new(self.hwnd.0 as isize)
            .ok_or_else(|| "window handle is null".to_string())?;
        let mut handle = wgpu::rwh::Win32WindowHandle::new(hwnd);
        handle.hinstance = NonZeroIsize::new(self.instance.0 as isize);
        Ok(wgpu::rwh::RawWindowHandle::Win32(handle))
    }

    /// Drains this thread's message queue; a window that stops pumping is
    /// marked unresponsive by the compositor and stops receiving flips.
    pub(super) fn pump(&self) {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub(super) fn move_to(&self, x: i32, y: i32, width: u32, height: u32) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width.max(1) as i32,
                height.max(1) as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
}

impl Drop for OutputWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_describes_every_attached_output() {
        let outputs = enumerate_outputs();
        // A session with no desktop has none; the stressor calls that unsupported.
        if outputs.is_empty() {
            eprintln!("no attached outputs in this session; nothing to check");
            return;
        }
        assert!(
            outputs.iter().filter(|o| o.primary).count() <= 1,
            "more than one primary: {outputs:?}"
        );
        for output in &outputs {
            assert!(output.device.starts_with(r"\\.\"), "odd device name: {output:?}");
            assert!(output.width > 0 && output.height > 0, "empty rect: {output:?}");
            assert!(
                !refresh_modes_at(&output.device, output.width, output.height).is_empty(),
                "{} advertises no mode at its own size",
                output.device
            );
            eprintln!("{}", output.describe());
        }
    }

    /// Covers the screen; run it deliberately with
    /// `cargo test -p stress-kit -- --ignored windows_open`.
    #[test]
    #[ignore = "creates fullscreen windows on every attached output"]
    fn windows_open_on_every_output() {
        for output in enumerate_outputs() {
            let window = OutputWindow::new(&output)
                .unwrap_or_else(|e| panic!("no window on {}: {e}", output.device));
            window.raw_handle().expect("no raw handle");
            for _ in 0..10 {
                window.pump();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}
