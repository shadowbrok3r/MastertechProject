//! Win32 window introspection for remote screen control.
//!
//! Answers "what am I about to click or type into" without paying for a
//! screenshot, and lets the admin assert a target before input is injected.
//!
//! **DPI:** the process manifest declares no DPI awareness, so `GetWindowRect`
//! would hand back coordinates Windows has virtualized for a 96-DPI process —
//! on a 150% display those are 2/3 of the physical pixels a capture records.
//! Every query here temporarily makes its own thread per-monitor aware so rects
//! come back in the same physical pixels as [`crate::remote_desktop`] captures.
//! The override is per-thread and restored immediately, so the egui UI thread's
//! own awareness is untouched.

use displays::remote_desktop::{FocusInfo, WindowInfo};

/// Which desktop currently receives input, and whether this session shares it.
pub struct InputDesktop {
    /// `Default`, `Screen-saver`, `Winlogon`, …; `None` when access was denied.
    pub name: Option<String>,
    /// The input desktop is this session's, so injection and capture reach it.
    pub reachable: bool,
    /// Winlogon, or an input desktop this process may not even open.
    pub secure: bool,
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{E_ACCESSDENIED, HANDLE, HWND, LPARAM, RECT};
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetThreadDesktop, GetUserObjectInformationW, OpenInputDesktop,
        DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, HDESK, UOI_NAME,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowRect,
        GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        IsZoomed, SetForegroundWindow, ShowWindow, GUITHREADINFO, SW_RESTORE,
    };

    /// Restores the previous per-thread DPI context on drop, so an early return
    /// cannot leak an override onto a pooled thread.
    struct DpiScope(DPI_AWARENESS_CONTEXT);

    impl DpiScope {
        fn per_monitor() -> Self {
            let prev = unsafe {
                SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            };
            Self(prev)
        }
    }

    impl Drop for DpiScope {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe { SetThreadDpiAwarenessContext(self.0) };
            }
        }
    }

    fn wide_to_string(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn desktop_name(hdesk: HDESK) -> Option<String> {
        let mut buf = [0u16; 128];
        unsafe {
            GetUserObjectInformationW(
                HANDLE(hdesk.0),
                UOI_NAME,
                Some(buf.as_mut_ptr().cast()),
                std::mem::size_of_val(&buf) as u32,
                None,
            )
        }
        .ok()?;
        Some(wide_to_string(&buf))
    }

    pub fn input_desktop() -> InputDesktop {
        let opened =
            unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) };
        let hdesk = match opened {
            Ok(h) => h,
            Err(e) => {
                return InputDesktop {
                    name: None,
                    reachable: false,
                    // Only the secure desktop denies a read handle.
                    secure: e.code() == E_ACCESSDENIED,
                };
            }
        };
        let name = desktop_name(hdesk);
        let _ = unsafe { CloseDesktop(hdesk) };

        // GetThreadDesktop's handle is owned by the system; do not close it.
        let own = unsafe { GetThreadDesktop(GetCurrentThreadId()) }
            .ok()
            .and_then(desktop_name);

        InputDesktop {
            reachable: matches!((&name, &own), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b)),
            secure: name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("winlogon")),
            name,
        }
    }

    fn window_title(hwnd: HWND) -> String {
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
        wide_to_string(&buf[..n.max(0) as usize])
    }

    fn window_class(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        wide_to_string(&buf[..n.max(0) as usize])
    }

    fn process_name(pid: u32) -> String {
        // sysinfo is already a dependency and avoids another OpenProcess dance.
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.process(Pid::from_u32(pid))
            .map(|p| p.name().to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn describe(hwnd: HWND, foreground: HWND) -> Option<WindowInfo> {
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return None;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        Some(WindowInfo {
            hwnd: hwnd.0 as u64,
            title: window_title(hwnd),
            class_name: window_class(hwnd),
            pid,
            process_name: process_name(pid),
            x: rect.left,
            y: rect.top,
            width: (rect.right - rect.left).max(0) as u32,
            height: (rect.bottom - rect.top).max(0) as u32,
            is_foreground: hwnd == foreground,
            is_minimized: unsafe { IsIconic(hwnd) }.as_bool(),
            is_maximized: unsafe { IsZoomed(hwnd) }.as_bool(),
        })
    }

    pub fn focus() -> FocusInfo {
        let _dpi = DpiScope::per_monitor();
        let desk = input_desktop();
        let fg = unsafe { GetForegroundWindow() };

        // A null foreground window means no window on this desktop holds focus,
        // which any desktop other than this session's also produces.
        if fg.0.is_null() {
            return FocusInfo {
                foreground: None,
                focused_control_class: None,
                caret: None,
                input_desktop: desk.name,
                input_reachable: desk.reachable,
                secure_desktop_suspected: desk.secure,
                dpi_context: "per-monitor-v2 (thread override)".into(),
            };
        }

        let mut gui = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let tid = unsafe { GetWindowThreadProcessId(fg, None) };
        let have_gui = unsafe { GetGUIThreadInfo(tid, &mut gui) }.is_ok();

        let focused_control_class = (have_gui && !gui.hwndFocus.0.is_null())
            .then(|| window_class(gui.hwndFocus));
        let caret = (have_gui && !gui.hwndCaret.0.is_null()).then(|| {
            let r = gui.rcCaret;
            (
                r.left,
                r.top,
                (r.right - r.left).max(0) as u32,
                (r.bottom - r.top).max(0) as u32,
            )
        });

        FocusInfo {
            foreground: describe(fg, fg),
            focused_control_class,
            caret,
            input_desktop: desk.name,
            input_reachable: desk.reachable,
            secure_desktop_suspected: desk.secure,
            dpi_context: "per-monitor-v2 (thread override)".into(),
        }
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            out.push(hwnd);
        }
        // Keep enumerating.
        BOOL(1)
    }

    pub fn list() -> Vec<WindowInfo> {
        let _dpi = DpiScope::per_monitor();
        let fg = unsafe { GetForegroundWindow() };
        let mut handles: Vec<HWND> = Vec::new();
        let _ = unsafe {
            EnumWindows(
                Some(collect),
                LPARAM(&mut handles as *mut Vec<HWND> as isize),
            )
        };
        handles
            .into_iter()
            .filter_map(|h| describe(h, fg))
            // Untitled windows are shell plumbing, not anything to drive.
            .filter(|w| !w.title.trim().is_empty() && w.width > 0 && w.height > 0)
            .collect()
    }

    pub fn activate(hwnd: u64, title_contains: Option<&str>) -> Result<WindowInfo, String> {
        let _dpi = DpiScope::per_monitor();
        let target = if hwnd != 0 {
            HWND(hwnd as *mut std::ffi::c_void)
        } else {
            let needle = title_contains
                .ok_or("pass hwnd or title_contains")?
                .to_lowercase();
            if needle.trim().is_empty() {
                return Err("title_contains is empty".into());
            }
            let matches: Vec<WindowInfo> = list()
                .into_iter()
                .filter(|w| w.title.to_lowercase().contains(&needle))
                .collect();
            match matches.len() {
                0 => return Err(format!("no visible window whose title contains {needle:?}")),
                1 => HWND(matches[0].hwnd as *mut std::ffi::c_void),
                _ => {
                    let titles: Vec<&str> = matches.iter().map(|w| w.title.as_str()).take(6).collect();
                    return Err(format!(
                        "{} windows match {needle:?} ({}); pass an hwnd from desktop_list_windows",
                        matches.len(),
                        titles.join(" | ")
                    ));
                }
            }
        };

        // A minimized window cannot take focus until it is restored.
        if unsafe { IsIconic(target) }.as_bool() {
            let _ = unsafe { ShowWindow(target, SW_RESTORE) };
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        let _ = unsafe { SetForegroundWindow(target) };
        std::thread::sleep(std::time::Duration::from_millis(150));

        let fg = unsafe { GetForegroundWindow() };
        let info = describe(target, fg).ok_or("window vanished during activation")?;
        if !info.is_foreground {
            // Windows refuses foreground changes from a process that does not
            // own the current foreground; say so instead of pretending.
            return Err(format!(
                "activation did not take: {:?} is still behind. Windows blocks foreground \
                 changes in some states; try clicking its taskbar button instead.",
                info.title
            ));
        }
        Ok(info)
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::*;
    pub fn input_desktop() -> InputDesktop {
        InputDesktop {
            name: None,
            reachable: true,
            secure: false,
        }
    }
    pub fn focus() -> FocusInfo {
        FocusInfo {
            foreground: None,
            focused_control_class: None,
            caret: None,
            input_desktop: None,
            input_reachable: true,
            secure_desktop_suspected: false,
            dpi_context: "unsupported on this platform".into(),
        }
    }
    pub fn list() -> Vec<WindowInfo> {
        Vec::new()
    }
    pub fn activate(_hwnd: u64, _title: Option<&str>) -> Result<WindowInfo, String> {
        Err("window activation is Windows-only".into())
    }
}

pub use imp::{activate, focus, input_desktop, list};

fn describe_owner(name: Option<&str>, secure: bool) -> String {
    match (name, secure) {
        (Some(name), true) => format!("the secure desktop ({name}) owns input"),
        (Some(name), false) => format!("the {name} desktop owns input"),
        (None, _) => "the input desktop is not readable by this session".into(),
    }
}

/// `Some(description)` when a desktop other than this session's owns input, so
/// captures and injected input cannot reach the screen.
pub fn unreachable_input_desktop() -> Option<String> {
    let desk = input_desktop();
    (!desk.reachable).then(|| describe_owner(desk.name.as_deref(), desk.secure))
}

/// `Ok(())` when the foreground window's title contains `needle`.
///
/// Checked on the client immediately before input is injected, so a window
/// change cannot slip between the caller's check and the keystroke.
pub fn assert_foreground_title(needle: &str) -> Result<(), String> {
    let info = focus();
    if !info.input_reachable {
        return Err(format!(
            "{}; no injection can reach it",
            describe_owner(info.input_desktop.as_deref(), info.secure_desktop_suspected)
        ));
    }
    let actual = info
        .foreground
        .as_ref()
        .map(|w| w.title.clone())
        .unwrap_or_default();
    if actual.to_lowercase().contains(&needle.to_lowercase()) {
        return Ok(());
    }
    Err(format!(
        "refusing input: expected a foreground window containing {needle:?} but it is {actual:?}. \
         Take a screenshot and re-target — the layout moved."
    ))
}
