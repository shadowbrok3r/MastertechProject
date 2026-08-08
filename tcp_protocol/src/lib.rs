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
//! - `0x0B` — Shape fingerprint. Payload: `[u8 kind][u64 LE fp][UTF-8 version]`.
//!
//! Unknown tags MUST be ignored (logged at warn) rather than tearing down
//! the session — this is what lets a v2 master talk to a v1 agent without
//! the agent dying when it sees a Ping frame it doesn't understand.

#[cfg(all(
    feature = "tunnel",
    not(target_arch = "wasm32"),
    not(target_os = "uefi")
))]
pub mod tunnel;

pub const FRAME_TAG_BINARY: u8 = 0x01;
pub const FRAME_TAG_TEXT: u8 = 0x02;
/// Ping (master → agent). Agent echoes back with [`FRAME_TAG_PONG`].
pub const FRAME_TAG_PING: u8 = 0x03;
/// Pong (agent → master, in response to [`FRAME_TAG_PING`]).
pub const FRAME_TAG_PONG: u8 = 0x04;
/// Shape fingerprint of `Cmd`, exchanged once after the handshake so each
/// side can detect a peer built from drifted source. Log-only on the agent,
/// surfaces a self-update hint on the admin. Payload built by
/// [`encode_shape_fp`].
pub const FRAME_TAG_SHAPE_FP: u8 = 0x0B;

/// `kind` byte marking a [`FRAME_TAG_SHAPE_FP`] frame as sent by the admin.
pub const SHAPE_FP_KIND_ADMIN: u8 = 0x01;
/// `kind` byte marking a [`FRAME_TAG_SHAPE_FP`] frame as sent by the agent.
pub const SHAPE_FP_KIND_AGENT: u8 = 0x02;

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

/// Build a shape-fingerprint payload: `[u8 kind][u64 LE fp][UTF-8 version]`.
/// The version has no length prefix — it runs to the end of the frame.
#[inline]
pub fn encode_shape_fp(kind: u8, fp: u64, version: &str) -> Vec<u8> {
    let vb = version.as_bytes();
    let mut out = Vec::with_capacity(9 + vb.len());
    out.push(kind);
    out.extend_from_slice(&fp.to_le_bytes());
    out.extend_from_slice(vb);
    out
}

/// Inverse of [`encode_shape_fp`]. Returns `None` when the payload is too
/// short to hold the kind + fingerprint or the version bytes aren't UTF-8,
/// so callers can ignore a malformed frame rather than tear the session down.
#[inline]
pub fn decode_shape_fp(bytes: &[u8]) -> Option<(u8, u64, String)> {
    if bytes.len() < 9 {
        return None;
    }
    let kind = bytes[0];
    let fp = u64::from_le_bytes(bytes[1..9].try_into().ok()?);
    let version = core::str::from_utf8(&bytes[9..]).ok()?.to_string();
    Some((kind, fp, version))
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
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "uefi")))]
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

/// Pre-boot terminal streaming: the UEFI firmware app renders a ratatui
/// terminal and streams it to the admin console using the same viewer widget
/// as the OS client. Firmware can't produce the OS path's zstd+egui
/// `BufferMessage` (no zstd C-FFI, no egui), so it ships these plain-serde
/// mirror types under dedicated frame tags instead. Encoded with bincode
/// (the same codec displays already uses), uncompressed.
pub mod preboot {
    use serde::{Deserialize, Serialize};

    /// A ratatui buffer frame streamed from firmware, row-major `cols*rows`.
    pub const FRAME_TAG_PREBOOT_FRAME: u8 = 0x05;
    /// An input event from the admin viewer back to firmware.
    pub const FRAME_TAG_PREBOOT_INPUT: u8 = 0x06;
    /// Opens a persistent pre-boot session; body is the UTF-8 session id
    /// (the machine serial). Sent once, before any frame, so the QC TCP
    /// listener keeps the socket open and registers a relay session rather
    /// than treating the connection as a one-shot fingerprint push.
    ///
    /// Persistent session flow (firmware dials the QC listener):
    /// 1. `[len][0x07][serial]` — hello
    /// 2. `[len][0x05][frame]` … streamed on change
    /// 3. server writes `[len][0x06][event]` back as the viewer sends input
    pub const FRAME_TAG_PREBOOT_HELLO: u8 = 0x07;
    /// Console → firmware: run a WASM plugin. Body is a bincode [`PbPluginRun`].
    /// Only meaningful on the direct socket (the firmware dials a console
    /// listener); lets the console push a registry plugin without HTTP.
    pub const FRAME_TAG_PREBOOT_PLUGIN_RUN: u8 = 0x08;
    /// Firmware → console: the outcome of a plugin run. Body is a bincode
    /// [`PbPluginResult`].
    pub const FRAME_TAG_PREBOOT_PLUGIN_RESULT: u8 = 0x09;
    /// Console → firmware: start/stop streaming frames. Body is a bincode
    /// [`PbStreamCtl`]. Lets a viewer opening/closing gate the frame flow over
    /// the direct socket the same way the relay's viewer flag does.
    pub const FRAME_TAG_PREBOOT_STREAM_CTL: u8 = 0x0A;
    /// Console → firmware: read a named slice of firmware state. Body is a
    /// bincode [`PbQuery`]. 0x0B is the top-level shape-fingerprint tag, so the
    /// pre-boot query pair starts at 0x0C.
    pub const FRAME_TAG_PREBOOT_QUERY: u8 = 0x0C;
    /// Firmware → console: the answer. Body is a bincode [`PbQueryResult`].
    pub const FRAME_TAG_PREBOOT_QUERY_RESULT: u8 = 0x0D;

    /// Mirrors `ratatui::style::Color` so a cell's color survives the wire
    /// without pulling ratatui into firmware's wire crate.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet), repr(u8))]
    pub enum PbColor {
        Reset,
        Black,
        Red,
        Green,
        Yellow,
        Blue,
        Magenta,
        Cyan,
        Gray,
        DarkGray,
        LightRed,
        LightGreen,
        LightYellow,
        LightBlue,
        LightMagenta,
        LightCyan,
        White,
        Indexed(u8),
        Rgb(u8, u8, u8),
    }

    /// One terminal cell: its grapheme, colors, and ratatui modifier bits.
    #[derive(Serialize, Deserialize, Clone, Debug)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PreBootCell {
        pub symbol: String,
        pub fg: PbColor,
        pub bg: PbColor,
        pub mods: u16,
    }

    /// A full terminal frame.
    #[derive(Serialize, Deserialize, Clone, Debug)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PreBootFrame {
        pub frame: u64,
        pub cols: u16,
        pub rows: u16,
        pub cells: Vec<PreBootCell>,
    }

    /// Lossy key code — the subset firmware's `terminput` loop can consume.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet), repr(u8))]
    pub enum PbKeyCode {
        Char(char),
        Enter,
        Esc,
        Backspace,
        Tab,
        Up,
        Down,
        Left,
        Right,
        Home,
        End,
        PageUp,
        PageDown,
        Delete,
        Insert,
        F(u8),
    }

    #[derive(Serialize, Deserialize, Clone, Copy, Debug)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PreBootKey {
        pub code: PbKeyCode,
        pub ctrl: bool,
        pub alt: bool,
        pub shift: bool,
    }

    /// An event from the viewer to the firmware app.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet), repr(u8))]
    pub enum PreBootEvent {
        Key(PreBootKey),
        MouseClick { x: u16, y: u16 },
        MouseScroll { x: u16, y: u16, up: bool },
    }

    /// Console → firmware plugin invocation. `source` is either a registry
    /// plugin id (firmware fetches it) or an absolute/relative URL; empty runs
    /// the embedded demo. `tool` empty means "first advertised tool".
    #[derive(Serialize, Deserialize, Clone, Debug, Default)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PbPluginRun {
        pub source: String,
        pub tool: String,
        pub args: String,
    }

    /// Firmware → console plugin outcome.
    #[derive(Serialize, Deserialize, Clone, Debug, Default)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PbPluginResult {
        pub ok: bool,
        pub id: String,
        pub name: String,
        pub version: String,
        pub tools: String,
        pub tool: String,
        pub result: String,
        pub log: Vec<String>,
        pub stdout: String,
        pub error: String,
    }

    /// Console → firmware stream gate.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PbStreamCtl {
        pub stream: bool,
    }

    /// Console → firmware state read. `topic` selects the document; `arg` and
    /// `limit` are topic-specific filters. Answered with a [`PbQueryResult`].
    #[derive(Serialize, Deserialize, Clone, Debug, Default)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PbQuery {
        pub topic: String,
        pub arg: String,
        pub limit: u32,
    }

    /// Firmware → console answer. `json` is a self-describing document so a new
    /// topic needs no wire change; `truncated` says a cap dropped part of it.
    #[derive(Serialize, Deserialize, Clone, Debug, Default)]
    #[cfg_attr(feature = "fingerprint", derive(facet::Facet))]
    pub struct PbQueryResult {
        pub topic: String,
        pub ok: bool,
        pub json: String,
        pub error: String,
        pub truncated: bool,
    }

    pub fn encode_plugin_run(p: &PbPluginRun) -> Vec<u8> {
        bincode::serde::encode_to_vec(p, bincode::config::standard()).unwrap_or_default()
    }
    pub fn decode_plugin_run(b: &[u8]) -> Option<PbPluginRun> {
        bincode::serde::decode_from_slice(b, bincode::config::standard()).ok().map(|(v, _)| v)
    }
    pub fn encode_plugin_result(p: &PbPluginResult) -> Vec<u8> {
        bincode::serde::encode_to_vec(p, bincode::config::standard()).unwrap_or_default()
    }
    pub fn decode_plugin_result(b: &[u8]) -> Option<PbPluginResult> {
        bincode::serde::decode_from_slice(b, bincode::config::standard()).ok().map(|(v, _)| v)
    }
    pub fn encode_stream_ctl(p: &PbStreamCtl) -> Vec<u8> {
        bincode::serde::encode_to_vec(p, bincode::config::standard()).unwrap_or_default()
    }
    pub fn decode_stream_ctl(b: &[u8]) -> Option<PbStreamCtl> {
        bincode::serde::decode_from_slice(b, bincode::config::standard()).ok().map(|(v, _)| v)
    }
    pub fn encode_query(p: &PbQuery) -> Vec<u8> {
        bincode::serde::encode_to_vec(p, bincode::config::standard()).unwrap_or_default()
    }
    pub fn decode_query(b: &[u8]) -> Option<PbQuery> {
        bincode::serde::decode_from_slice(b, bincode::config::standard()).ok().map(|(v, _)| v)
    }
    pub fn encode_query_result(p: &PbQueryResult) -> Vec<u8> {
        bincode::serde::encode_to_vec(p, bincode::config::standard()).unwrap_or_default()
    }
    pub fn decode_query_result(b: &[u8]) -> Option<PbQueryResult> {
        bincode::serde::decode_from_slice(b, bincode::config::standard()).ok().map(|(v, _)| v)
    }

    /// UDP port a console broadcasts direct-link discovery beacons on, and the
    /// firmware listens on. LAN-local (255.255.255.255) — never routed.
    pub const DISCOVERY_PORT: u16 = 9210;

    /// Discovery-beacon magic + versions. v1 payload is `MTCB\x01<addr UTF-8>`;
    /// v2 payload is `MTCB\x02<addr UTF-8>\x00<relay base url UTF-8>`, where addr
    /// is the console's direct-link `ip:port` and the relay is the Mastertech
    /// relay base url (e.g. `http://192.168.22.139:8082`). NUL separates the two
    /// because it cannot appear in an `IPv4:port`. Deliberately not bincode so
    /// the firmware can parse it straight from a raw UDP payload.
    const BEACON_MAGIC: &[u8; 4] = b"MTCB";
    const BEACON_VERSION: u8 = 1;
    pub const BEACON_VERSION_2: u8 = 2;

    /// Longest relay url accepted from a v2 beacon.
    pub const MAX_RELAY_URL_BYTES: usize = 256;

    /// Endpoints advertised by a discovery beacon; `relay` is None for v1.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Beacon {
        pub addr: String,
        pub relay: Option<String>,
    }

    /// Build a v1 discovery beacon advertising `addr` (the console's `ip:port`).
    pub fn encode_beacon(addr: &str) -> Vec<u8> {
        let mut v = Vec::with_capacity(5 + addr.len());
        v.extend_from_slice(BEACON_MAGIC);
        v.push(BEACON_VERSION);
        v.extend_from_slice(addr.as_bytes());
        v
    }

    /// Build a v2 discovery beacon advertising `addr` and the relay base url.
    pub fn encode_beacon_v2(addr: &str, relay: &str) -> Vec<u8> {
        let mut v = Vec::with_capacity(6 + addr.len() + relay.len());
        v.extend_from_slice(BEACON_MAGIC);
        v.push(BEACON_VERSION_2);
        v.extend_from_slice(addr.as_bytes());
        v.push(0);
        v.extend_from_slice(relay.as_bytes());
        v
    }

    /// True for an `http://`/`https://` url of at most [`MAX_RELAY_URL_BYTES`]
    /// bytes whose authority is non-empty and free of control characters,
    /// whitespace and userinfo.
    pub fn is_valid_relay_url(url: &str) -> bool {
        if url.len() > MAX_RELAY_URL_BYTES || url.chars().any(char::is_control) {
            return false;
        }
        let Some(rest) =
            url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))
        else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        !authority.is_empty()
            && !authority.contains('@')
            && !authority.chars().any(|c| c.is_ascii_whitespace())
    }

    /// Accept an `IPv4:port` with a non-zero port, so a stray or malformed
    /// datagram on the port can't become a dial target.
    fn parse_beacon_addr(bytes: &[u8]) -> Option<String> {
        let addr = core::str::from_utf8(bytes).ok()?.trim();
        let sa = addr.parse::<core::net::SocketAddrV4>().ok()?;
        if sa.port() == 0 {
            return None;
        }
        Some(addr.to_string())
    }

    /// Parse a v1 discovery beacon payload, returning the advertised `ip:port`.
    /// Rejects anything without the magic/version and anything that isn't a
    /// real `IPv4:port` with a non-zero port.
    pub fn parse_beacon(payload: &[u8]) -> Option<String> {
        if payload.len() < 6 || &payload[0..4] != BEACON_MAGIC || payload[4] != BEACON_VERSION {
            return None;
        }
        parse_beacon_addr(&payload[5..])
    }

    /// Parse a v1 or v2 discovery beacon payload; v1 yields `relay: None`. Address
    /// checks match [`parse_beacon`], and a v2 relay url must satisfy
    /// [`is_valid_relay_url`] so a malformed datagram can't become a huge or
    /// hostile target string.
    pub fn parse_beacon_v2(payload: &[u8]) -> Option<Beacon> {
        if payload.len() < 6 || &payload[0..4] != BEACON_MAGIC {
            return None;
        }
        match payload[4] {
            BEACON_VERSION => parse_beacon(payload).map(|addr| Beacon { addr, relay: None }),
            BEACON_VERSION_2 => {
                let body = &payload[5..];
                let sep = body.iter().position(|b| *b == 0)?;
                let addr = parse_beacon_addr(&body[..sep])?;
                let relay = core::str::from_utf8(body.get(sep + 1..)?).ok()?.trim();
                if !is_valid_relay_url(relay) {
                    return None;
                }
                Some(Beacon { addr, relay: Some(relay.to_string()) })
            }
            _ => None,
        }
    }

    pub fn encode_frame(f: &PreBootFrame) -> Vec<u8> {
        bincode::serde::encode_to_vec(f, bincode::config::standard()).unwrap_or_default()
    }

    pub fn decode_frame(b: &[u8]) -> Option<PreBootFrame> {
        bincode::serde::decode_from_slice(b, bincode::config::standard())
            .ok()
            .map(|(v, _)| v)
    }

    pub fn encode_event(e: &PreBootEvent) -> Vec<u8> {
        bincode::serde::encode_to_vec(e, bincode::config::standard()).unwrap_or_default()
    }

    pub fn decode_event(b: &[u8]) -> Option<PreBootEvent> {
        bincode::serde::decode_from_slice(b, bincode::config::standard())
            .ok()
            .map(|(v, _)| v)
    }

    /// Pack queued event bodies as `[u32 LE count][(u32 LE len)(body)]*` for the
    /// HTTP-relayed input channel (server drains its queue into one response).
    pub fn encode_event_batch(bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(bodies.len() as u32).to_le_bytes());
        for b in bodies {
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }
        out
    }

    /// Inverse of [`encode_event_batch`]; malformed input yields what parsed so far.
    pub fn split_event_batch(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if bytes.len() < 4 {
            return out;
        }
        let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let mut p = 4;
        for _ in 0..count {
            if p + 4 > bytes.len() {
                break;
            }
            let len = u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize;
            p += 4;
            if p + len > bytes.len() {
                break;
            }
            out.push(bytes[p..p + len].to_vec());
            p += len;
        }
        out
    }
}

#[cfg(feature = "fingerprint")]
pub mod shape_fp;

// Pinned structural fingerprints of the preboot wire mirrors. Bump a pin
// deliberately when the mirrored ratatui/terminput shape changes on the wire.
#[cfg(all(test, feature = "fingerprint"))]
mod preboot_shape_fp {
    use super::preboot::*;
    use crate::shape_fp::shape_fingerprint;

    // Mirrors ratatui::style::Color.
    #[test]
    fn pb_color_pin() {
        assert_eq!(shape_fingerprint::<PbColor>(), 0x0a68_c6d9_fd03_bf4b);
    }

    // Mirrors the terminput/crossterm KeyCode subset.
    #[test]
    fn pb_key_code_pin() {
        assert_eq!(shape_fingerprint::<PbKeyCode>(), 0x780a_8389_35f5_29ab);
    }

    // Mirrors the terminput/crossterm input Event.
    #[test]
    fn pre_boot_event_pin() {
        assert_eq!(shape_fingerprint::<PreBootEvent>(), 0x358c_711f_5281_4666);
    }

    // Mirrors ratatui::buffer::Cell.
    #[test]
    fn pre_boot_cell_pin() {
        assert_eq!(shape_fingerprint::<PreBootCell>(), 0x90f6_bd5e_85d2_437c);
    }

    // Mirrors a ratatui::buffer::Buffer frame of cells.
    #[test]
    fn pre_boot_frame_pin() {
        assert_eq!(shape_fingerprint::<PreBootFrame>(), 0xc22a_bddf_43bd_a574);
    }

    // Mirrors crossterm::event::KeyEvent (code + modifier flags).
    #[test]
    fn pre_boot_key_pin() {
        assert_eq!(shape_fingerprint::<PreBootKey>(), 0xbf87_b4d4_9df4_beab);
    }

    // Firmware plugin-run wire; no ratatui counterpart.
    #[test]
    fn pb_plugin_run_pin() {
        assert_eq!(shape_fingerprint::<PbPluginRun>(), 0x7224_27b5_2772_56c7);
    }

    // Firmware plugin-result wire; no ratatui counterpart.
    #[test]
    fn pb_plugin_result_pin() {
        assert_eq!(shape_fingerprint::<PbPluginResult>(), 0xb2ca_f75a_d3e2_f0bc);
    }

    // Firmware state-query wire; no ratatui counterpart.
    #[test]
    fn pb_query_pin() {
        assert_eq!(shape_fingerprint::<PbQuery>(), 0xb377_b7fa_ccfe_06c0);
    }

    // Firmware state-query answer; no ratatui counterpart.
    #[test]
    fn pb_query_result_pin() {
        assert_eq!(shape_fingerprint::<PbQueryResult>(), 0xa522_4ac5_c450_a1f8);
    }

    // Firmware stream-gate wire; no ratatui counterpart.
    #[test]
    fn pb_stream_ctl_pin() {
        assert_eq!(shape_fingerprint::<PbStreamCtl>(), 0xfce4_0c16_a7f4_104c);
    }
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
    fn query_roundtrip() {
        use super::preboot::*;
        let q = PbQuery { topic: "logs".into(), arg: "flash:".into(), limit: 500 };
        let back = decode_query(&encode_query(&q)).unwrap();
        assert_eq!((back.topic.as_str(), back.arg.as_str(), back.limit), ("logs", "flash:", 500));

        let r = PbQueryResult {
            topic: "flash".into(),
            ok: true,
            json: r#"{"preflight":{"blocked":true}}"#.into(),
            error: String::new(),
            truncated: true,
        };
        let back = decode_query_result(&encode_query_result(&r)).unwrap();
        assert!(back.ok && back.truncated);
        assert_eq!(back.json, r#"{"preflight":{"blocked":true}}"#);
    }

    #[test]
    fn query_tags_are_distinct() {
        use super::preboot::*;
        let tags = [
            FRAME_TAG_PREBOOT_FRAME,
            FRAME_TAG_PREBOOT_INPUT,
            FRAME_TAG_PREBOOT_HELLO,
            FRAME_TAG_PREBOOT_PLUGIN_RUN,
            FRAME_TAG_PREBOOT_PLUGIN_RESULT,
            FRAME_TAG_PREBOOT_STREAM_CTL,
            FRAME_TAG_PREBOOT_QUERY,
            FRAME_TAG_PREBOOT_QUERY_RESULT,
            FRAME_TAG_BINARY,
            FRAME_TAG_TEXT,
            FRAME_TAG_PING,
            FRAME_TAG_PONG,
            FRAME_TAG_SHAPE_FP,
        ];
        let unique: std::collections::BTreeSet<u8> = tags.iter().copied().collect();
        assert_eq!(unique.len(), tags.len(), "a frame tag is used twice: {tags:?}");
    }

    #[test]
    fn beacon_roundtrip_and_reject() {
        use super::preboot::{encode_beacon, parse_beacon};
        assert_eq!(parse_beacon(&encode_beacon("192.168.22.139:9209")).as_deref(), Some("192.168.22.139:9209"));
        assert_eq!(parse_beacon(b"random udp junk"), None);
        assert_eq!(parse_beacon(b"MTCB\x01nohost"), None); // no :port
        assert_eq!(parse_beacon(b"MTCB\x02192.168.1.1:80"), None); // wrong version
        // Strict IPv4:port — reject non-numeric/empty octets, multi-colon, port 0.
        assert_eq!(parse_beacon(b"MTCB\x01a.b.c.d:80"), None);
        assert_eq!(parse_beacon(b"MTCB\x01...:80"), None);
        assert_eq!(parse_beacon(b"MTCB\x011.2.3.4:5:9209"), None);
        assert_eq!(parse_beacon(b"MTCB\x011.2.3.4:0"), None);
        assert_eq!(parse_beacon(b"MTCB\x01999.1.1.1:80"), None);
    }

    #[test]
    fn beacon_v2_roundtrip_and_reject() {
        use super::preboot::{Beacon, MAX_RELAY_URL_BYTES, encode_beacon, encode_beacon_v2, parse_beacon_v2};
        let v2 = encode_beacon_v2("192.168.22.139:9209", "http://192.168.22.139:8082");
        assert_eq!(
            parse_beacon_v2(&v2),
            Some(Beacon {
                addr: "192.168.22.139:9209".to_string(),
                relay: Some("http://192.168.22.139:8082".to_string()),
            })
        );
        // v1 payloads still parse, with no relay.
        assert_eq!(
            parse_beacon_v2(&encode_beacon("192.168.22.139:9209")),
            Some(Beacon { addr: "192.168.22.139:9209".to_string(), relay: None })
        );
        // A v2 relay url must carry an http(s) scheme and be non-empty.
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "192.168.1.5:8082")), None);
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "ftp://1.2.3.4:8082")), None);
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "")), None);
        // A scheme with no authority, or an authority with whitespace, is rejected.
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "http://")), None);
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "https:///api")), None);
        assert_eq!(
            parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "http:// space host")),
            None
        );
        assert_eq!(
            parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "http://evil@1.2.3.4:8082")),
            None
        );
        // Oversized and control-character relay urls are rejected.
        let long = format!("http://{}", "a".repeat(MAX_RELAY_URL_BYTES));
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", &long)), None);
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:9209", "http://1.2.3.4\u{7}:8082")), None);
        // v2 without the NUL separator, and with a bad addr, are rejected.
        assert_eq!(parse_beacon_v2(b"MTCB\x021.2.3.4:9209http://1.2.3.4:8082"), None);
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("1.2.3.4:0", "http://1.2.3.4:8082")), None);
        assert_eq!(parse_beacon_v2(&encode_beacon_v2("nohost", "http://1.2.3.4:8082")), None);
        assert_eq!(parse_beacon_v2(b"random udp junk"), None);
        assert_eq!(parse_beacon_v2(b"MTCB\x031.2.3.4:9209"), None);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert!(decode_ping_payload(&[]).is_none());
        assert!(decode_ping_payload(&[0u8; 15]).is_none());
        assert!(decode_ping_payload(&[0u8; 17]).is_none());
    }

    #[test]
    fn shape_fp_roundtrip_and_reject() {
        let bytes = encode_shape_fp(SHAPE_FP_KIND_AGENT, 0x43d1_b51a_200b_ddd6, "0.1.0");
        let (kind, fp, ver) = decode_shape_fp(&bytes).unwrap();
        assert_eq!(kind, SHAPE_FP_KIND_AGENT);
        assert_eq!(fp, 0x43d1_b51a_200b_ddd6);
        assert_eq!(ver, "0.1.0");
        // Empty version is valid (frame remainder may be empty).
        assert_eq!(decode_shape_fp(&encode_shape_fp(1, 7, "")), Some((1, 7, String::new())));
        assert!(decode_shape_fp(&[]).is_none());
        assert!(decode_shape_fp(&[0u8; 8]).is_none());
    }

    #[test]
    fn version_window() {
        assert!(is_supported_version(HANDSHAKE_VERSION_V1));
        assert!(is_supported_version(HANDSHAKE_VERSION_V2));
        assert!(!is_supported_version(0));
        assert!(!is_supported_version(HANDSHAKE_VERSION_CURRENT + 1));
    }
}
