//! Keeps the main window's client area inside its window rect while undecorated.

use std::sync::atomic::{AtomicBool, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    NCCALCSIZE_PARAMS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowPos, WM_NCCALCSIZE, WM_NCDESTROY,
};

const SUBCLASS_ID: usize = 0x4D54_4E43;

/// Whether the last `WM_NCCALCSIZE` had to clamp the client rect.
static CLAMPING: AtomicBool = AtomicBool::new(false);

/// Subclasses the root window so `WM_NCCALCSIZE` never yields a client rect below the window's bottom edge.
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    let Ok(handle) = cc.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    // SAFETY: `hwnd` is the live root window, owned by this (the event-loop) thread.
    unsafe {
        if !SetWindowSubclass(hwnd, Some(clamp_proc), SUBCLASS_ID, 0).as_bool() {
            return false;
        }
        // Recomputes the client rect through the subclass.
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    true
}

/// Clamps winit's undecorated-shadow client rect (moved down 1 px at full height) to the proposed window rect.
unsafe extern "system" fn clamp_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_NCCALCSIZE if wparam.0 != 0 => {
            let params = lparam.0 as *mut NCCALCSIZE_PARAMS;
            // SAFETY: for WM_NCCALCSIZE with wparam TRUE, lparam points at a valid NCCALCSIZE_PARAMS.
            unsafe {
                let window_bottom = (*params).rgrc[0].bottom;
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                let client = &mut (*params).rgrc[0];
                let overhang = client.bottom - window_bottom;
                log_transition(overhang);
                client.bottom = client.bottom.min(window_bottom);
                result
            }
        }
        WM_NCDESTROY => {
            // SAFETY: removes this subclass from the window it was installed on.
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(clamp_proc), SUBCLASS_ID);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
        // SAFETY: forwards every other message unchanged.
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

/// Logs when the client rect starts or stops overhanging the window's bottom edge.
fn log_transition(overhang: i32) {
    let clamping = overhang > 0;
    if CLAMPING.swap(clamping, Ordering::Relaxed) == clamping {
        return;
    }
    if clamping {
        log::info!("client_area: client rect overhung the window bottom by {overhang}px; clamped");
    } else {
        log::info!("client_area: client rect inside the window; no clamp");
    }
}
