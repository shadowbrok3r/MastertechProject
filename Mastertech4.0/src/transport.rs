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
use tokio::sync::mpsc::{Sender, UnboundedSender};

/// Frame queued for the TCP writer task. Distinguishes binary `Cmd`
/// payloads (the common case) from text control strings, which the
/// writer encodes with a leading byte tag so the reader can dispatch.
pub enum TcpFrame {
    Binary(Vec<u8>),
    Text(String),
    /// Pong response echoed back to a master-sent Ping.  Carries the
    /// original 16-byte ping payload verbatim so the master can measure
    /// round-trip and dedup by sequence number.
    Pong(Vec<u8>),
}

// Wire-protocol constants are owned by the `tcp_protocol` crate so the
// agent (this binary) and the master (`displays`) cannot drift.  Re-export
// them under the original names so existing call sites compile unchanged.
pub use tcp_protocol::{
    FRAME_TAG_BINARY, FRAME_TAG_PING, FRAME_TAG_PONG, FRAME_TAG_TEXT, HANDSHAKE_MAGIC,
    HANDSHAKE_VERSION_CURRENT as HANDSHAKE_VERSION,
};

pub enum ClientTransport {
    /// The relay path. Hands `WsMessage` straight to ewebsock.
    WebSocket(WsSender),
    /// The direct TCP path. Control frames go to the writer task's unbounded
    /// channel; bulk file chunks go to a separate BOUNDED channel so a large
    /// download applies backpressure (the streaming task blocks when the
    /// socket falls behind) instead of buffering the whole file in RAM.
    Tcp {
        ctrl: UnboundedSender<TcpFrame>,
        file: Sender<TcpFrame>,
    },
}

impl ClientTransport {
    /// Mirrors [`ewebsock::WsSender::send`] so existing call sites compile
    /// unchanged when their `sender` parameter is retyped from
    /// `&mut WsSender` to `&mut ClientTransport`.
    pub fn send(&mut self, msg: WsMessage) {
        match self {
            ClientTransport::WebSocket(ws) => ws.send(msg),
            ClientTransport::Tcp { ctrl, .. } => match msg {
                WsMessage::Binary(b) => {
                    let _ = ctrl.send(TcpFrame::Binary(b.into()));
                }
                WsMessage::Text(t) => {
                    let _ = ctrl.send(TcpFrame::Text(t.into()));
                }
                // Ping/Pong/Close are handled by the WebSocket framing
                // layer when present and don't have a meaning on direct
                // TCP; quietly drop them rather than fabricating messages.
                _ => {}
            },
        }
    }

    /// Clone of the bounded file-chunk sender for streaming large downloads
    /// off the session loop with backpressure. `None` on the relay path.
    pub fn file_sender(&self) -> Option<Sender<TcpFrame>> {
        match self {
            ClientTransport::Tcp { file, .. } => Some(file.clone()),
            ClientTransport::WebSocket(_) => None,
        }
    }
}
