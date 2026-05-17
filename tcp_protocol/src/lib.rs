//! Shared wire-protocol constants for the MasterTech admin↔client direct
//! TCP path.
//!
//! Two crates speak this protocol: the agent listener in
//! `Mastertech4.0/src/tcp_listener.rs` and the master dialer in
//! `displays/src/tabs/admin_console/client_interface/admin_transport.rs`.
//! Before this crate existed both sides hard-coded the same magic, version
//! and frame-tag bytes; this crate is the single source of truth so the
//! two cannot drift.
//!
//! # Wire format
//!
//! ## Handshake (admin → client, immediately after TCP connect)
//!
//! ```text
//! [MTRX (4 bytes magic)][version u8][u32 LE id_len][UTF-8 connection_string]
//! ```
//!
//! ## Frames (bidirectional, post-handshake)
//!
//! ```text
//! [u32 LE total_len][u8 tag][payload bytes]
//! ```
//!
//! `total_len = 1 + payload_len` (it counts the tag byte). Receivers MUST
//! reject `total_len == 0` or `total_len > MAX_FRAME_BYTES`.
//!
//! ## Frame tags
//!
//! - `0x01` — binary `Cmd` payload (bincode-serialized).
//! - `0x02` — UTF-8 text command.
//! - `0x03` — Ping (v2+). Payload: `[u64 LE seq][u64 LE epoch_ms]` (16 bytes).
//! - `0x04` — Pong (v2+). Echoes the ping payload verbatim.
//!
//! Unknown tags MUST be ignored (logged at warn) rather than tearing down
//! the session — this is what lets a v2 master talk to a v1 agent without
//! the agent dying when it sees a Ping frame it doesn't understand.

pub const FRAME_TAG_BINARY: u8 = 0x01;
pub const FRAME_TAG_TEXT: u8 = 0x02;
/// Ping (master → agent). Agent echoes back with [`FRAME_TAG_PONG`].
pub const FRAME_TAG_PING: u8 = 0x03;
/// Pong (agent → master, in response to [`FRAME_TAG_PING`]).
pub const FRAME_TAG_PONG: u8 = 0x04;

/// Magic preamble that opens the handshake. Cheap rejection of port-scan
/// probes before any deserialization.
pub const HANDSHAKE_MAGIC: &[u8; 4] = b"MTRX";

/// Original wire protocol. Binary + Text frames only.
pub const HANDSHAKE_VERSION_V1: u8 = 1;
/// Adds Ping/Pong frame tags. Unknown-tag tolerance means a v2 master can
/// talk to a v1 agent (the agent ignores pings); a v1 master can talk to a
/// v2 agent (master never sends pings, agent never receives any).
pub const HANDSHAKE_VERSION_V2: u8 = 2;
/// What new builds send. Receivers should accept any version in
/// `HANDSHAKE_VERSION_V1..=HANDSHAKE_VERSION_CURRENT`.
pub const HANDSHAKE_VERSION_CURRENT: u8 = HANDSHAKE_VERSION_V2;

/// Hard cap on a single inbound frame so a malicious or buggy peer can't
/// allocate gigabytes by sending a giant length prefix.
pub const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024; // 64 MiB

/// Length of the Ping/Pong payload: `u64 LE seq + u64 LE epoch_ms`.
pub const PING_FRAME_LEN: usize = 16;

/// Returns `true` if `version` is a wire version this build can speak.
/// Used by the agent's handshake to widen its acceptance window.
#[inline]
pub const fn is_supported_version(version: u8) -> bool {
    version >= HANDSHAKE_VERSION_V1 && version <= HANDSHAKE_VERSION_CURRENT
}

/// Build a 16-byte ping payload from a sequence number and timestamp.
#[inline]
pub fn encode_ping_payload(seq: u64, epoch_ms: u64) -> [u8; PING_FRAME_LEN] {
    let mut out = [0u8; PING_FRAME_LEN];
    out[0..8].copy_from_slice(&seq.to_le_bytes());
    out[8..16].copy_from_slice(&epoch_ms.to_le_bytes());
    out
}

/// Inverse of [`encode_ping_payload`]. Returns `None` on a malformed
/// payload so callers can ignore it rather than tear the session down.
#[inline]
pub fn decode_ping_payload(bytes: &[u8]) -> Option<(u64, u64)> {
    if bytes.len() != PING_FRAME_LEN {
        return None;
    }
    let seq = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let ts = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    Some((seq, ts))
}

/// Apply `SO_KEEPALIVE` + `TCP_NODELAY` to a freshly-accepted or
/// freshly-dialed TCP stream.
///
/// Both sides of the direct admin↔client path call this on every stream.
/// Without keepalive the OS will never detect a peer that has silently
/// vanished (NAT timeout, hard-crash, cable yank); the next write would
/// hang or fail eventually but reads can sit forever. With these settings:
///
/// - 30 s idle before the first probe
/// - 10 s between subsequent probes
/// - 3 probe retries before the OS declares the socket dead
///
/// So a half-open socket is detected within ~60 s without any application
/// traffic. Combined with the 15 s app-level ping (master side), real
/// failures surface in ≤30 s.
///
/// `with_retries` is a no-op on Windows pre-1703; time + interval still
/// apply, which is enough to detect failure in roughly the same window.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply_tcp_options(stream: &tokio::net::TcpStream) -> std::io::Result<()> {
    use socket2::{SockRef, TcpKeepalive};
    let sock = SockRef::from(stream);
    let mut ka = TcpKeepalive::new()
        .with_time(std::time::Duration::from_secs(30))
        .with_interval(std::time::Duration::from_secs(10));
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        ka = ka.with_retries(3);
    }
    sock.set_tcp_keepalive(&ka)?;
    stream.set_nodelay(true)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_payload_roundtrip() {
        let bytes = encode_ping_payload(42, 0x1234_5678_9abc_def0);
        let (seq, ts) = decode_ping_payload(&bytes).unwrap();
        assert_eq!(seq, 42);
        assert_eq!(ts, 0x1234_5678_9abc_def0);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert!(decode_ping_payload(&[]).is_none());
        assert!(decode_ping_payload(&[0u8; 15]).is_none());
        assert!(decode_ping_payload(&[0u8; 17]).is_none());
    }

    #[test]
    fn version_window() {
        assert!(is_supported_version(HANDSHAKE_VERSION_V1));
        assert!(is_supported_version(HANDSHAKE_VERSION_V2));
        assert!(!is_supported_version(0));
        assert!(!is_supported_version(HANDSHAKE_VERSION_CURRENT + 1));
    }
}
