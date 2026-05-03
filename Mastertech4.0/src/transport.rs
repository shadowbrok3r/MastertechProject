//! Transport-agnostic sender used by the client's Cmd handler.
//!
//! `ClientTransport::send(WsMessage)` keeps the exact same call-site
//! signature as `ewebsock::WsSender::send(WsMessage)` — that's the trick
//! that lets the existing 2000-line `handle_command` work unchanged over
//! either transport. Only the parameter TYPE changes from
//! `&mut WsSender` to `&mut ClientTransport`.
//!
//! - `WebSocket` variant: forwards directly to ewebsock.
//! - `Tcp` variant: hands frames to a writer task via an unbounded channel;
//!   the writer task length-prefixes (4-byte LE u32) and writes to the
//!   TcpStream write half. Ping/Pong/Close `WsMessage` variants are
//!   dropped on the TCP path — the transport layer handles connection
//!   liveness via TCP itself.

use ewebsock::{WsMessage, WsSender};
use tokio::sync::mpsc::UnboundedSender;

/// Frame queued for the TCP writer task. Distinguishes binary `Cmd`
/// payloads (the common case) from text control strings, which the
/// writer encodes with a leading byte tag so the reader can dispatch.
pub enum TcpFrame {
    Binary(Vec<u8>),
    Text(String),
}

/// Frame tag bytes used on the wire. Reader and writer must agree.
pub const FRAME_TAG_BINARY: u8 = 0x01;
pub const FRAME_TAG_TEXT: u8 = 0x02;

/// Magic preamble sent by the admin (master) right after the TCP connect
/// completes. Lets the client cheaply reject random port-scan probes
/// before any deserialization happens.
pub const HANDSHAKE_MAGIC: &[u8; 4] = b"MTRX";

/// Wire protocol version. Bump when the framing changes.
pub const HANDSHAKE_VERSION: u8 = 1;

pub enum ClientTransport {
    /// The relay path. Hands `WsMessage` straight to ewebsock.
    WebSocket(WsSender),
    /// The direct TCP path. Frames are pushed into the writer task's
    /// channel; that task takes care of length-prefixing and socket I/O.
    Tcp(UnboundedSender<TcpFrame>),
}

impl ClientTransport {
    /// Mirrors [`ewebsock::WsSender::send`] so existing call sites compile
    /// unchanged when their `sender` parameter is retyped from
    /// `&mut WsSender` to `&mut ClientTransport`.
    pub fn send(&mut self, msg: WsMessage) {
        match self {
            ClientTransport::WebSocket(ws) => ws.send(msg),
            ClientTransport::Tcp(tx) => match msg {
                WsMessage::Binary(b) => {
                    let _ = tx.send(TcpFrame::Binary(b.into()));
                }
                WsMessage::Text(t) => {
                    let _ = tx.send(TcpFrame::Text(t.into()));
                }
                // Ping/Pong/Close are handled by the WebSocket framing
                // layer when present and don't have a meaning on direct
                // TCP; quietly drop them rather than fabricating messages.
                _ => {}
            },
        }
    }
}
