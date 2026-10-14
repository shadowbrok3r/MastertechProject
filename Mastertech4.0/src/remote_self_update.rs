//! Remote Mastertech self-update.
//!
//! The admin console sends a new MasterTech binary in 512 KiB
//! [`displays::Cmd::MastertechSelfUpdateChunk`] chunks over the admin TCP/WS
//! transport.  When all chunks arrive this module:
//!
//! 1. Assembles them in memory.
//! 2. Writes the payload to a temp file alongside the running binary.
//! 3. Swaps the exe on disk via [`crate::utilities::safe_swap`], rolling back
//!    if installation fails.
//! 4. Relaunches the new exe with the same CLI args, or execs it in place in Unix terminal mode.
//! 5. Returns a result string so the caller can send
//!    [`displays::Cmd::MastertechSelfUpdateResult`] back before exiting.
//!
//! Used by the terminal-mode path (`terminal_mode/websockets/mod.rs`).
//! (The GUI-side `tabs/websockets/mod.rs` was removed; what remains of
//! the egui-client transport now goes over direct TCP via
//! `tcp_listener.rs`.)

use std::collections::HashMap;
use std::path::PathBuf;

/// Accumulator for in-flight self-update chunks.  A single instance is
/// stored on each `TerminalWebsocketClient`.
#[derive(Default)]
pub struct SelfUpdateBuffer {
    chunks: HashMap<u32, Vec<u8>>,
    total_chunks: u32,
}

impl SelfUpdateBuffer {
    /// Push one received chunk.  Returns `Some(assembled_bytes)` once all
    /// chunks are present; `None` if more are still expected.
    pub fn push(&mut self, chunk_index: u32, total_chunks: u32, data: Vec<u8>) -> Option<Vec<u8>> {
        if chunk_index == 0 || total_chunks != self.total_chunks {
            self.chunks.clear();
        }
        self.total_chunks = total_chunks;
        self.chunks.insert(chunk_index, data);
        if self.chunks.len() as u32 == self.total_chunks {
            let mut indices: Vec<u32> = self.chunks.keys().copied().collect();
            indices.sort_unstable();
            let bytes: Vec<u8> = indices
                .into_iter()
                .flat_map(|i| self.chunks.remove(&i).unwrap_or_default())
                .collect();
            Some(bytes)
        } else {
            None
        }
    }
}

/// Outcome of [`apply_and_relaunch`], reported as [`displays::Cmd::MastertechSelfUpdateResult`].
pub struct Applied {
    /// The new binary is installed and running, or about to run.
    pub success: bool,
    pub message: String,
    /// Updated binary to exec in place once the result is sent, instead of exiting.
    pub exec_after_reply: Option<PathBuf>,
}

impl Applied {
    pub fn failed(message: String) -> Self {
        Self {
            success: false,
            message,
            exec_after_reply: None,
        }
    }

    fn done(message: String) -> Self {
        Self {
            success: true,
            message,
            exec_after_reply: None,
        }
    }
}

/// Stages and swaps in `bytes`, then relaunches or returns the exe to exec after the reply.
pub fn apply_and_relaunch(bytes: Vec<u8>) -> Applied {
    use crate::utilities::safe_swap;

    let current_exe = match safe_swap::running_exe() {
        Ok(p) => p,
        Err(e) => return Applied::failed(format!("current_exe() failed: {e}")),
    };

    let parent_dir = current_exe
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    // `current_exe` may live inside a directory that was deleted or never
    // created (e.g. first-time install path).
    if let Err(e) = std::fs::create_dir_all(parent_dir) {
        return Applied::failed(format!("Failed to create binary directory {:?}: {e}", parent_dir));
    }

    let expected_len = bytes.len() as u64;
    let temp_path = parent_dir.join(safe_swap::REMOTE_STAGED_NAME);
    if let Err(e) = std::fs::write(&temp_path, &bytes) {
        return Applied::failed(format!("Failed to write temp binary ({} bytes): {e}", bytes.len()));
    }

    // Refresh the firewall rule in the old elevated process. The port-only
    // rule survives binary replacement and doesn't rely on exe-path matching.
    #[cfg(target_os = "windows")]
    {
        use crate::utilities::network::try_add_firewall_rule;
        match try_add_firewall_rule(crate::tcp_listener::PREFERRED_PORT, "Mastertech Direct TCP") {
            Ok(true) => log::info!("remote_self_update -> firewall rule refreshed for port {}", crate::tcp_listener::PREFERRED_PORT),
            Ok(false) => log::warn!("remote_self_update -> firewall rule may need admin; new process will show allow-popup on first connection"),
            Err(e) => log::warn!("remote_self_update -> netsh spawn failed: {e}"),
        }
    }

    #[cfg(unix)]
    if let Err(e) = safe_swap::prepare_staged(&temp_path, None) {
        let _ = std::fs::remove_file(&temp_path);
        return Applied::failed(format!("Update not applied ({e:#}); still running the previous binary."));
    }

    let exe = match safe_swap::apply_staged_update(&temp_path, expected_len) {
        Ok(exe) => exe,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            return Applied::failed(format!("Update not applied ({e}); still running the previous binary."));
        }
    };

    #[cfg(unix)]
    if crate::utilities::run_mode::is_terminal() {
        return Applied {
            success: true,
            message: format!("Self-update applied ({expected_len} bytes). Restarting in place."),
            exec_after_reply: Some(exe),
        };
    }

    match safe_swap::relaunch(&exe, &[("MASTERTECH_SELF_UPDATE_CHILD", "1")]) {
        Ok(()) => Applied::done(format!(
            "Self-update applied ({expected_len} bytes). Relaunching and exiting."
        )),
        Err(e) => Applied::failed(format!(
            "Binary updated on disk ({expected_len} bytes) but relaunch failed: {e}. Keeping current process alive; the update applies on next start."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_push_discards_chunks_from_an_abandoned_one() {
        let mut buffer = SelfUpdateBuffer::default();
        assert_eq!(buffer.push(0, 3, b"old0".to_vec()), None);
        assert_eq!(buffer.push(1, 3, b"old1".to_vec()), None);
        assert_eq!(buffer.push(0, 2, b"new0".to_vec()), None);
        assert_eq!(buffer.push(1, 2, b"new1".to_vec()), Some(b"new0new1".to_vec()));

        assert_eq!(buffer.push(0, 2, b"a0".to_vec()), None);
        assert_eq!(buffer.push(0, 2, b"b0".to_vec()), None);
        assert_eq!(buffer.push(1, 2, b"b1".to_vec()), Some(b"b0b1".to_vec()));
    }
}
