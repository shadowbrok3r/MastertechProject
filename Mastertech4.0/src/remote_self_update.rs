//! Remote Mastertech self-update.
//!
//! The admin console sends a new `MasterTech.exe` in 512 KiB
//! [`displays::Cmd::MastertechSelfUpdateChunk`] chunks over the admin TCP/WS
//! transport.  When all chunks arrive this module:
//!
//! 1. Assembles them in memory.
//! 2. Writes the payload to a temp file alongside the running binary.
//! 3. Swaps the exe on disk via [`crate::utilities::safe_swap`], rolling back
//!    if installation fails.
//! 4. Spawns the replaced exe with the same CLI args (so `-t`, flags etc. survive).
//! 5. Returns a result string so the caller can send
//!    [`displays::Cmd::MastertechSelfUpdateResult`] back before exiting.
//!
//! Used by the terminal-mode path (`terminal_mode/websockets/mod.rs`).
//! (The GUI-side `tabs/websockets/mod.rs` was removed; what remains of
//! the egui-client transport now goes over direct TCP via
//! `tcp_listener.rs`.)

use std::collections::HashMap;

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

/// Applies the assembled binary: stage it next to the running exe, swap it
/// into place with rollback on failure, then spawn the replaced exe with the
/// original CLI args.
///
/// Returns `(success, message)` suitable for [`displays::Cmd::MastertechSelfUpdateResult`].
/// `success` means the caller may exit; on failure the running binary is untouched.
#[cfg(target_os = "windows")]
pub fn apply_and_relaunch(bytes: Vec<u8>) -> (bool, String) {
    use crate::utilities::safe_swap;

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return (false, format!("current_exe() failed: {e}")),
    };

    let parent_dir = current_exe
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    // `current_exe` may live inside a directory that was deleted or never
    // created (e.g. first-time install path).
    if let Err(e) = std::fs::create_dir_all(parent_dir) {
        return (false, format!("Failed to create binary directory {:?}: {e}", parent_dir));
    }

    let expected_len = bytes.len() as u64;
    let temp_path = parent_dir.join(safe_swap::REMOTE_STAGED_NAME);
    if let Err(e) = std::fs::write(&temp_path, &bytes) {
        return (false, format!("Failed to write temp binary ({} bytes): {e}", bytes.len()));
    }

    // Refresh the firewall rule in the old elevated process. The port-only
    // rule survives binary replacement and doesn't rely on exe-path matching.
    {
        use crate::utilities::network::try_add_firewall_rule;
        match try_add_firewall_rule(crate::tcp_listener::PREFERRED_PORT, "Mastertech Direct TCP") {
            Ok(true) => log::info!("remote_self_update -> firewall rule refreshed for port {}", crate::tcp_listener::PREFERRED_PORT),
            Ok(false) => log::warn!("remote_self_update -> firewall rule may need admin; new process will show allow-popup on first connection"),
            Err(e) => log::warn!("remote_self_update -> netsh spawn failed: {e}"),
        }
    }

    let exe = match safe_swap::apply_staged_update(&temp_path, expected_len) {
        Ok(exe) => exe,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            return (false, format!("Update not applied ({e}); still running the previous binary."));
        }
    };

    match safe_swap::relaunch(&exe, &[("MASTERTECH_SELF_UPDATE_CHILD", "1")]) {
        Ok(()) => (
            true,
            format!("Self-update applied ({expected_len} bytes). Relaunching and exiting."),
        ),
        Err(e) => (
            false,
            format!("Binary updated on disk ({expected_len} bytes) but relaunch failed: {e}. Keeping current process alive; the update applies on next start."),
        ),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn apply_and_relaunch(_bytes: Vec<u8>) -> (bool, String) {
    (false, "Remote self-update is only supported on Windows.".to_string())
}
