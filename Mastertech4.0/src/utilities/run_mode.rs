//! Process-wide record of whether the terminal UI is running.

use std::sync::atomic::{AtomicBool, Ordering};

static TERMINAL: AtomicBool = AtomicBool::new(false);

/// Marks this process as running terminal mode.
pub fn set_terminal() {
    TERMINAL.store(true, Ordering::Relaxed);
}

/// True once terminal mode has started.
#[cfg(unix)]
pub fn is_terminal() -> bool {
    TERMINAL.load(Ordering::Relaxed)
}
