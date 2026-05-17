//! Worker ↔ admin wire protocol. Self-contained on purpose so the
//! worker binary doesn't pull `displays`.
//!
//! Encoding: every frame starts with [`BUILDER_WIRE_TAG`] (1 byte),
//! followed by `bincode::serde::encode_to_vec(&BuilderWire,
//! bincode::config::standard())`. The leading tag lets the admin's
//! receive path distinguish builder traffic from the existing `Cmd`
//! stream the same way `EGUI_INPUT_TAG` does for remote-egui input.
//! websocket_server2 is a transparent byte pipe; multi-megabyte WASM
//! payloads ride inside `CompileResult` and tungstenite handles
//! fragmentation.

use serde::{Deserialize, Serialize};

/// One-byte tag prepended to every `BuilderWire` frame on the wire.
/// Pairs with `displays::EGUI_INPUT_TAG` (0xEE) and
/// `displays::EGUI_FRAME_TAG` so the admin can multiplex multiple
/// payload shapes on a single WebSocket without a length-prefixed
/// envelope. Chosen so it never collides with a bincode-encoded
/// `Cmd` discriminant (those start at 0x00 and stay under ~0x80).
pub const BUILDER_WIRE_TAG: u8 = 0xBB;

/// Cargo profile passed to the worker. String-typed on the wire so
/// custom workspace profiles (`release-fast`, etc.) work without
/// updating both sides in lockstep.
pub type CompileProfile = String;

/// Rustc target triple. `wasm32-wasip1` is the only one Mastertech
/// loads today; the field is open so we can add e.g. `wasm32-unknown-unknown`
/// later.
pub type CompileTarget = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuilderWire {
    /// Worker → admin. Sent once, immediately after the WS handshake
    /// completes. Lets the admin/MCP side filter usable workers by
    /// target without round-tripping a no-op compile job.
    Hello {
        hostname: String,
        target_triples: Vec<CompileTarget>,
        capabilities: Vec<String>,
        worker_version: String,
    },

    /// Admin → worker. Request a `cargo build` of a single-file
    /// plugin. `cargo_toml` and `lib_rs` are the literal file
    /// contents the worker will materialize on disk.
    CompileRequest {
        job_id: String,
        plugin_id: String,
        cargo_toml: String,
        lib_rs: String,
        target: CompileTarget,
        profile: CompileProfile,
    },

    /// Worker → admin. Optional progress signal while a long compile
    /// is running. Jobs only terminate on `CompileResult`.
    CompileProgress {
        job_id: String,
        stage: String,
        message: String,
    },

    /// Worker → admin. Terminal result for a `CompileRequest`. On
    /// success `wasm_bytes` is `Some` and the bytes are ready for
    /// the admin's `ArtifactStore`. On failure `stderr` holds the
    /// cargo output the agent needs to fix the source.
    CompileResult {
        job_id: String,
        success: bool,
        wasm_bytes: Option<Vec<u8>>,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    },
}

impl BuilderWire {
    /// Bincode-only encoding (no tag). Useful for tests and for
    /// callers that frame at a higher layer.
    pub fn encode(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        let (msg, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())?;
        Ok(msg)
    }

    /// `[BUILDER_WIRE_TAG] ++ bincode(self)`. This is what travels on
    /// the wire between admin and worker; both ends use this pair.
    pub fn encode_tagged(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        let body = self.encode()?;
        let mut out = Vec::with_capacity(1 + body.len());
        out.push(BUILDER_WIRE_TAG);
        out.extend(body);
        Ok(out)
    }

    /// Decode a frame that begins with [`BUILDER_WIRE_TAG`]. Returns
    /// `Ok(None)` if the tag doesn't match (so callers can fall
    /// through to other handlers); returns `Err` only on bincode
    /// decode failure of an otherwise-tagged frame.
    pub fn decode_tagged(bytes: &[u8]) -> Result<Option<Self>, bincode::error::DecodeError> {
        match bytes.first() {
            Some(&BUILDER_WIRE_TAG) => Self::decode(&bytes[1..]).map(Some),
            _ => Ok(None),
        }
    }
}
