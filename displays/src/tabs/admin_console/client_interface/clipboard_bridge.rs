//! Admin-side half of remote-desktop clipboard mirroring.
//!
//! egui only surfaces the operator's clipboard on a paste event, which is too
//! late for "copy here, right-click paste over there". On desktop targets a
//! single process-wide thread owns an [`arboard::Clipboard`] and polls it while
//! any remote desktop session is live, so a local copy reaches the client
//! without the operator doing anything. That thread also performs inbound
//! applies, so text it just wrote is recorded as already-seen and never echoed
//! back to the session that sent it.
//!
//! The poller is per process, not per session. `OpenClipboard(NULL)` associates
//! the clipboard with the calling *process*, so a second thread in the same
//! process can open it while the first is still inside `GetClipboardData`;
//! two readers then race on the same global handle and `GlobalSize` fails the
//! heap check (`STATUS_HEAP_CORRUPTION`). Every session shares the one thread.
//!
//! Targets with no `arboard` backend (wasm, iOS, Android) run without the
//! thread: inbound text goes to egui's own clipboard and outbound relies on the
//! paste-event path in the desktop viewer.

use crossbeam::channel::{Receiver, Sender};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Clipboard payloads above this are dropped rather than mirrored.
pub const CLIPBOARD_MAX_BYTES: usize = 1024 * 1024;

/// Outbound queue depth per session.
const OUTBOUND_DEPTH: usize = 8;

const HAS_POLLER: bool = cfg!(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
));

// Only the poller reads these payloads; targets without one still need the
// type so the channel plumbing compiles.
#[cfg_attr(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)), allow(dead_code))]
enum BridgeMsg {
    /// Text from a client to place on this machine's clipboard, and the session
    /// it came from — that session already has it, so it is not sent back.
    Apply { text: String, from: u64 },
    /// A session turned mirroring on; re-seed from the current contents.
    Reseed,
}

/// Clipboard poller threads spawned in this process. One, once any session opens.
static POLLERS: AtomicUsize = AtomicUsize::new(0);

/// Number of clipboard poller threads this process has spawned.
///
/// One poller is the invariant the bridge exists to hold: concurrent readers in
/// one process race inside `GetClipboardData`.
pub fn active_pollers() -> usize {
    POLLERS.load(Ordering::SeqCst)
}

/// Sessions currently mirroring the operator's clipboard.
pub fn mirroring_sessions() -> usize {
    hub().mirroring.load(Ordering::SeqCst)
}

/// Process-wide clipboard owner shared by every open client interface.
struct Hub {
    to_thread: Sender<BridgeMsg>,
    /// One outbound sender per live [`ClipboardBridge`], keyed by session id.
    sinks: Mutex<Vec<(u64, Sender<String>)>>,
    /// Sessions currently mirroring; the poller runs only while this is non-zero.
    mirroring: AtomicUsize,
    next_id: AtomicU64,
}

// Only the poller broadcasts; every target still needs the rest of Hub.
#[cfg_attr(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)), allow(dead_code))]
impl Hub {
    /// Hands `text` to every session except `from`.
    fn broadcast(&self, text: &str, from: Option<u64>) {
        let sinks = self.sinks.lock().unwrap_or_else(|e| e.into_inner());
        for (id, tx) in sinks.iter() {
            if Some(*id) == from {
                continue;
            }
            if tx.try_send(text.to_string()).is_err() {
                log::debug!(target: "remote_desktop", "clipboard outbound queue full");
            }
        }
    }
}

fn hub() -> &'static Arc<Hub> {
    static HUB: OnceLock<Arc<Hub>> = OnceLock::new();
    HUB.get_or_init(|| {
        let (to_thread, from_sessions) = crossbeam::channel::unbounded::<BridgeMsg>();
        let hub = Arc::new(Hub {
            to_thread,
            sinks: Mutex::new(Vec::new()),
            mirroring: AtomicUsize::new(0),
            next_id: AtomicU64::new(0),
        });

        #[cfg(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        {
            let for_thread = hub.clone();
            match std::thread::Builder::new()
                .name("admin-clipboard".into())
                .spawn(move || clipboard_loop(from_sessions, for_thread))
            {
                Ok(_) => {
                    POLLERS.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => {
                    log::error!(target: "remote_desktop", "clipboard thread spawn failed: {e}")
                }
            }
        }

        // No poller on this target. Dropping the receiver is what makes `apply`
        // fall through to egui's clipboard and `take_outbound` yield nothing.
        #[cfg(not(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        )))]
        drop(from_sessions);

        hub
    })
}

/// One session's handle on the shared clipboard poller.
pub struct ClipboardBridge {
    id: u64,
    hub: &'static Arc<Hub>,
    outbound_rx: Receiver<String>,
    enabled: bool,
}

impl Default for ClipboardBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBridge {
    pub fn new() -> Self {
        let hub = hub();
        let id = hub.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, outbound_rx) = crossbeam::channel::bounded::<String>(OUTBOUND_DEPTH);
        hub.sinks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, tx));
        Self {
            id,
            hub,
            outbound_rx,
            enabled: false,
        }
    }

    /// Start or stop polling the operator's clipboard for this session.
    pub fn set_enabled(&mut self, on: bool) {
        if self.enabled == on {
            return;
        }
        self.enabled = on;
        if on {
            // Reseeding on the way in means text copied while mirroring was off
            // is not pushed the moment it comes back on.
            self.hub.mirroring.fetch_add(1, Ordering::SeqCst);
            let _ = self.hub.to_thread.send(BridgeMsg::Reseed);
        } else {
            self.hub.mirroring.fetch_sub(1, Ordering::SeqCst);
        }
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
        if !HAS_POLLER {
            ctx.copy_text(text);
            return;
        }
        let msg = BridgeMsg::Apply {
            text: text.clone(),
            from: self.id,
        };
        // A closed channel means the poller thread failed to spawn; egui owns
        // the clipboard in that case.
        if self.hub.to_thread.send(msg).is_err() {
            ctx.copy_text(text);
        }
    }
}

impl Drop for ClipboardBridge {
    fn drop(&mut self) {
        if self.enabled {
            self.hub.mirroring.fetch_sub(1, Ordering::SeqCst);
        }
        self.hub
            .sinks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(id, _)| *id != self.id);
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
fn clipboard_loop(rx: Receiver<BridgeMsg>, hub: Arc<Hub>) {
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
        // Parked entirely while no session mirrors, so a console with no remote
        // desktop session open never touches the clipboard.
        let msg = if hub.mirroring.load(Ordering::SeqCst) > 0 {
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
            Some(BridgeMsg::Apply { text, from }) => {
                if text != last {
                    match clipboard.set_text(text.clone()) {
                        Ok(()) => {
                            // Other sessions still get it; the sender already has it.
                            hub.broadcast(&text, Some(from));
                            last = text;
                        }
                        Err(e) => {
                            log::warn!(target: "remote_desktop", "clipboard set failed: {e}")
                        }
                    }
                }
            }
            Some(BridgeMsg::Reseed) => last = clipboard.get_text().unwrap_or_default(),
            None => {}
        }

        if hub.mirroring.load(Ordering::SeqCst) == 0 {
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
        hub.broadcast(&text, None);
    }
}
