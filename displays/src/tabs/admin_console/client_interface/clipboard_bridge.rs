//! Admin-side half of remote-desktop clipboard mirroring.
//!
//! egui only surfaces the operator's clipboard on a paste event, which is too
//! late for "copy here, right-click paste over there". On desktop targets a
//! dedicated thread owns an [`arboard::Clipboard`] and polls it while a remote
//! desktop session is live, so a local copy reaches the client without the
//! operator doing anything. That thread also performs inbound applies, so text
//! it just wrote is recorded as already-seen and never echoed back.
//!
//! Targets with no `arboard` backend (wasm, iOS, Android) run without the
//! thread: inbound text goes to egui's own clipboard and outbound relies on the
//! paste-event path in the desktop viewer.

use crossbeam::channel::{Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Clipboard payloads above this are dropped rather than mirrored.
pub const CLIPBOARD_MAX_BYTES: usize = 1024 * 1024;

// Only the poller reads these payloads; targets without one still need the
// type so the channel plumbing compiles.
#[cfg_attr(
    not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )),
    allow(dead_code)
)]
enum BridgeMsg {
    /// Text from the client to place on this machine's clipboard.
    Apply(String),
    /// Mirroring was turned on; re-seed from the current contents.
    Reseed,
}

pub struct ClipboardBridge {
    enabled: Arc<AtomicBool>,
    to_thread: Sender<BridgeMsg>,
    outbound_rx: Receiver<String>,
}

impl Default for ClipboardBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBridge {
    pub fn new() -> Self {
        let (to_thread, from_ui) = crossbeam::channel::unbounded::<BridgeMsg>();
        let (outbound_tx, outbound_rx) = crossbeam::channel::bounded::<String>(8);
        let enabled = Arc::new(AtomicBool::new(false));

        #[cfg(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        {
            let flag = enabled.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("admin-clipboard".into())
                .spawn(move || clipboard_loop(from_ui, outbound_tx, flag))
            {
                log::error!(target: "remote_desktop", "clipboard thread spawn failed: {e}");
            }
        }

        // No poller on this target. Dropping the far ends is what makes `apply`
        // fall through to egui's clipboard and `take_outbound` yield nothing.
        #[cfg(not(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        )))]
        drop((from_ui, outbound_tx));

        Self {
            enabled,
            to_thread,
            outbound_rx,
        }
    }

    /// Start or stop polling the operator's clipboard.
    pub fn set_enabled(&mut self, on: bool) {
        if self.enabled.swap(on, Ordering::SeqCst) == on {
            return;
        }
        // Reseeding on the way in means text copied while mirroring was off is
        // not pushed the moment it comes back on.
        let _ = self.to_thread.send(BridgeMsg::Reseed);
    }

    /// Newest local clipboard change since the last call, if any.
    pub fn take_outbound(&mut self) -> Option<String> {
        let mut newest = None;
        while let Ok(text) = self.outbound_rx.try_recv() {
            newest = Some(text);
        }
        newest
    }

    /// Place the client's clipboard text on this machine's clipboard.
    pub fn apply(&mut self, ctx: &eframe::egui::Context, text: String) {
        if text.len() > CLIPBOARD_MAX_BYTES {
            log::warn!(
                target: "remote_desktop",
                "dropping {} byte inbound clipboard (cap {CLIPBOARD_MAX_BYTES})",
                text.len()
            );
            return;
        }
        // A closed channel means no poller on this target; egui owns the
        // clipboard there instead.
        if self.to_thread.send(BridgeMsg::Apply(text.clone())).is_err() {
            ctx.copy_text(text);
        }
    }
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn clipboard_loop(rx: Receiver<BridgeMsg>, out: Sender<String>, enabled: Arc<AtomicBool>) {
    use std::time::Duration;

    /// How often the admin re-reads its own clipboard while mirroring is on.
    const POLL: Duration = Duration::from_millis(300);

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!(target: "remote_desktop", "clipboard init failed: {e}");
            return;
        }
    };
    let mut last = clipboard.get_text().unwrap_or_default();

    loop {
        // Parked entirely while mirroring is off, so a console with no remote
        // desktop session open never touches the clipboard.
        let msg = if enabled.load(Ordering::SeqCst) {
            match rx.recv_timeout(POLL) {
                Ok(m) => Some(m),
                Err(crossbeam::channel::RecvTimeoutError::Timeout) => None,
                Err(crossbeam::channel::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        };

        match msg {
            Some(BridgeMsg::Apply(text)) => {
                if text != last {
                    match clipboard.set_text(text.clone()) {
                        Ok(()) => last = text,
                        Err(e) => {
                            log::warn!(target: "remote_desktop", "clipboard set failed: {e}")
                        }
                    }
                }
            }
            Some(BridgeMsg::Reseed) => last = clipboard.get_text().unwrap_or_default(),
            None => {}
        }

        if !enabled.load(Ordering::SeqCst) {
            continue;
        }
        // A non-text clipboard (image, file drop) reads as an error; leaving
        // `last` alone keeps the next text copy a change.
        let Ok(text) = clipboard.get_text() else {
            continue;
        };
        if text == last {
            continue;
        }
        last = text.clone();
        if text.len() > CLIPBOARD_MAX_BYTES {
            log::warn!(
                target: "remote_desktop",
                "not mirroring {} byte clipboard (cap {CLIPBOARD_MAX_BYTES})",
                text.len()
            );
            continue;
        }
        if out.try_send(text).is_err() {
            log::debug!(target: "remote_desktop", "clipboard outbound queue full");
        }
    }
}
