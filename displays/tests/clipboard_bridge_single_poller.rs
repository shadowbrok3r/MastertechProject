//! One clipboard poller per process, however many sessions are open.
//!
//! `OpenClipboard(NULL)` scopes the clipboard to the calling process, so a
//! second thread in the same process can open it while the first is still
//! inside `GetClipboardData`. Two readers then race on the same global handle
//! and `GlobalSize` trips the heap check, killing the console with
//! `STATUS_HEAP_CORRUPTION` — observed 2026-09-08 with four `admin-clipboard`
//! threads, two of them inside `GetClipboardData` at once.
//!
//! These tests never call `apply`, so the operator's clipboard is only read.

#![cfg(not(target_arch = "wasm32"))]

use displays::tabs::admin_console::client_interface::clipboard_bridge::{
    active_pollers, mirroring_sessions, ClipboardBridge,
};

/// Client interfaces open at once during the crash.
const SESSIONS: usize = 8;

#[test]
fn many_sessions_share_one_poller() {
    let bridges: Vec<ClipboardBridge> = (0..SESSIONS).map(|_| ClipboardBridge::new()).collect();
    assert_eq!(active_pollers(), 1, "one poller thread per process");

    // Opening and closing more sessions never spawns another.
    for _ in 0..SESSIONS {
        let extra = ClipboardBridge::new();
        drop(extra);
        assert_eq!(active_pollers(), 1);
    }

    drop(bridges);
    assert_eq!(active_pollers(), 1);
}

#[test]
fn mirroring_is_counted_per_session_and_released_on_drop() {
    let before = mirroring_sessions();

    let mut a = ClipboardBridge::new();
    let mut b = ClipboardBridge::new();
    assert_eq!(mirroring_sessions(), before);

    a.set_enabled(true);
    b.set_enabled(true);
    assert_eq!(mirroring_sessions(), before + 2);

    // Repeat calls are idempotent, so the count cannot drift.
    a.set_enabled(true);
    b.set_enabled(true);
    assert_eq!(mirroring_sessions(), before + 2);

    a.set_enabled(false);
    a.set_enabled(false);
    assert_eq!(mirroring_sessions(), before + 1);

    // A session dropped while still mirroring releases its count.
    drop(b);
    assert_eq!(mirroring_sessions(), before);

    drop(a);
    assert_eq!(mirroring_sessions(), before);
    assert_eq!(active_pollers(), 1);
}
