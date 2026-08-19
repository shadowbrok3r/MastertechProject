//! TUI-safe panic reporting: keeps panic text off a live alternate screen.
//!
//! The default panic hook writes to stderr. With ratatui's alternate screen on
//! stdout, that text lands on top of the rendered frame and stays there, so a
//! panic on any tokio worker shreds the UI of an app that is otherwise still
//! running. [`install_hook`] routes the report through `log` and drops the
//! stderr write while a [`TerminalGuard`] is alive.
//!
//! Install before any hook that chains to its predecessor, or that hook's own
//! work is skipped while the TUI is active.

use std::io;
use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use ratatui::crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// True while a [`TerminalGuard`] holds the alternate screen.
pub fn tui_active() -> bool {
    TUI_ACTIVE.load(Ordering::Acquire)
}

/// Installs the logging panic hook. Idempotent.
pub fn install_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!(target: "panic", "{}", describe(info));
            if tui_active() {
                return;
            }
            previous(info);
        }));
    });
}

/// Formats a panic as `panic at file:line:col: message`, appending a backtrace
/// when `RUST_BACKTRACE` requests one.
fn describe(info: &PanicHookInfo<'_>) -> String {
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "unknown location".to_string());
    let mut out = format!("panic at {location}: {}", payload_str(info.payload()));
    let backtrace = std::backtrace::Backtrace::capture();
    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        out.push('\n');
        out.push_str(&backtrace.to_string());
    }
    out
}

/// Sets the active flag without a real TTY, for tests only.
#[cfg(test)]
fn set_active_for_test(active: bool) {
    TUI_ACTIVE.store(active, Ordering::Release);
}

/// Extracts the `&str` or `String` form of a panic payload.
fn payload_str(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Owns raw mode, the alternate screen and mouse capture, restoring all three
/// on drop so an unwinding panic cannot leave the terminal unusable.
pub struct TerminalGuard {
    _private: (),
}

impl TerminalGuard {
    /// Enables raw mode, enters the alternate screen and marks the TUI active.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(e);
        }
        TUI_ACTIVE.store(true, Ordering::Release);
        Ok(Self { _private: () })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Cleared first so a panic inside this teardown still reaches stderr.
        TUI_ACTIVE.store(false, Ordering::Release);
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, Show);
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[test]
    fn payload_str_downcasts_str_and_string() {
        let borrowed: &str = "boom";
        assert_eq!(payload_str(&borrowed), "boom");
        let owned: String = "kaboom".to_string();
        assert_eq!(payload_str(&owned), "kaboom");
        let other: i32 = 7;
        assert_eq!(payload_str(&other), "non-string panic payload");
    }

    /// Owns `TUI_ACTIVE` and the process panic hook, so it covers the default
    /// state too rather than racing a second test over the same globals.
    #[test]
    fn chained_hook_runs_only_while_the_tui_is_inactive() {
        static CHAINED: AtomicUsize = AtomicUsize::new(0);

        assert!(!tui_active(), "no guard has been entered");

        std::panic::set_hook(Box::new(|_| {
            CHAINED.fetch_add(1, Ordering::SeqCst);
        }));
        install_hook();

        let _ = std::panic::catch_unwind(|| panic!("reaches the chained hook"));
        assert_eq!(CHAINED.load(Ordering::SeqCst), 1);

        set_active_for_test(true);
        let _ = std::panic::catch_unwind(|| panic!("must not reach the chained hook"));
        assert_eq!(
            CHAINED.load(Ordering::SeqCst),
            1,
            "stderr-writing hook ran while the alternate screen was live"
        );

        set_active_for_test(false);
        let _ = std::panic::catch_unwind(|| panic!("reaches the chained hook again"));
        assert_eq!(CHAINED.load(Ordering::SeqCst), 2);
    }
}
