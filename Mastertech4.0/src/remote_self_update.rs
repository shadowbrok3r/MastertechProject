//! Remote Mastertech self-update.
//!
//! The admin console sends a new `MasterTech.exe` in 512 KiB
//! [`displays::Cmd::MastertechSelfUpdateChunk`] chunks over the admin TCP/WS
//! transport.  When all chunks arrive this module:
//!
//! 1. Assembles them in memory.
//! 2. Writes the payload to a temp file alongside the running binary.
//! 3. Calls [`self_replace::self_replace`] to atomically swap the exe on disk.
//! 4. Spawns the new process with the same CLI args (so `-t`, flags etc. survive).
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

/// Applies the assembled binary: write to a temp path, call `self_replace`,
/// spawn a fresh process with the original CLI args, and schedule exit.
///
/// Returns `(success, message)` suitable for [`displays::Cmd::MastertechSelfUpdateResult`].
#[cfg(target_os = "windows")]
pub fn apply_and_relaunch(bytes: Vec<u8>) -> (bool, String) {
    use std::os::windows::process::CommandExt;

    // Resolve the running executable's path.
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return (false, format!("current_exe() failed: {e}")),
    };

    // Write the new binary next to the current one as a temp file.
    let parent_dir = current_exe
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    // Ensure the directory exists — `current_exe` may live inside a directory
    // that was deleted or never created (e.g. first-time install path).
    if let Err(e) = std::fs::create_dir_all(parent_dir) {
        return (false, format!("Failed to create binary directory {:?}: {e}", parent_dir));
    }

    let temp_path = parent_dir.join("MasterTech_update_pending.exe");

    if let Err(e) = std::fs::write(&temp_path, &bytes) {
        return (false, format!("Failed to write temp binary ({} bytes): {e}", bytes.len()));
    }

    // Spawn the new binary with the same args before replacing, so Windows
    // does not lock the file we are about to overwrite.
    //
    // Refresh the firewall rule now (in the old elevated process) so it's
    // ready before the new process starts. The port-only rule survives binary
    // replacement and doesn't rely on exe-path matching.
    #[cfg(target_os = "windows")]
    {
        use crate::utilities::network::try_add_firewall_rule;
        match try_add_firewall_rule(crate::tcp_listener::PREFERRED_PORT, "Mastertech Direct TCP") {
            Ok(true) => log::info!("remote_self_update -> firewall rule refreshed for port {}", crate::tcp_listener::PREFERRED_PORT),
            Ok(false) => log::warn!("remote_self_update -> firewall rule may need admin; new process will show allow-popup on first connection"),
            Err(e) => log::warn!("remote_self_update -> netsh spawn failed: {e}"),
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let spawn_result = std::process::Command::new(&temp_path)
        .args(&args)
        .creation_flags(DETACHED_PROCESS)
        .spawn();

    if let Err(e) = spawn_result {
        // Clean up on spawn failure so we do not leave an orphan file.
        let _ = std::fs::remove_file(&temp_path);
        return (false, format!("Failed to spawn updated binary: {e}"));
    }

    // Atomically replace the running exe on disk.
    match self_replace::self_replace(&temp_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&temp_path);
            (
                true,
                format!(
                    "Self-update applied ({} bytes). Relaunching and exiting.",
                    bytes.len()
                ),
            )
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            (false, format!("self_replace failed: {e}"))
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn apply_and_relaunch(_bytes: Vec<u8>) -> (bool, String) {
    (false, "Remote self-update is only supported on Windows.".to_string())
}
