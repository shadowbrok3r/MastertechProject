//! In-app capture of every log record routed to the TUI logger, so the
//! Logs tab can copy the full backlog to the system clipboard.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// Maximum number of captured log lines kept in memory.
const CAPTURE_DEPTH: usize = 10_000;

static CAPTURED: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Clipboard instance kept alive for the process lifetime. On X11 the
/// clipboard contents are served by the owning process, so dropping the
/// handle right after `set_text` would lose the copied data.
static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();

/// Record one log line into the capture ring. Called from the logger
/// format hook, so it must never panic or block on contention.
pub fn capture_record(record: &log::Record) {
    let line = format!(
        "{} [{:5}] {}: {}",
        chrono::Local::now().format("%F %H:%M:%S%.3f"),
        record.level(),
        record.target(),
        record.args()
    );
    if let Ok(mut buf) = CAPTURED.lock() {
        if buf.len() >= CAPTURE_DEPTH {
            buf.pop_front();
        }
        buf.push_back(line);
    }
}

/// All captured log lines, oldest first, joined with newlines.
pub fn all_captured_logs() -> String {
    CAPTURED
        .lock()
        .map(|buf| buf.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// The last `max_lines` captured lines (all of them when `None`), oldest
/// first, alongside the ring's full depth.
pub fn tail_captured_logs(max_lines: Option<usize>) -> (String, usize) {
    let Ok(buf) = CAPTURED.lock() else {
        return (String::new(), 0);
    };
    let total = buf.len();
    let skip = match max_lines {
        Some(n) => total.saturating_sub(n),
        None => 0,
    };
    let text = buf.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n");
    (text, total)
}

/// Copy the entire captured backlog to the system clipboard.
/// Returns the number of lines copied.
pub fn copy_all_to_clipboard() -> anyhow::Result<usize> {
    let text = all_captured_logs();
    let lines = if text.is_empty() { 0 } else { text.lines().count() };
    let slot = CLIPBOARD.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|e| anyhow::anyhow!("clipboard mutex poisoned: {e}"))?;
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new()?);
    }
    if let Some(clipboard) = guard.as_mut() {
        clipboard.set_text(text)?;
    }
    Ok(lines)
}
