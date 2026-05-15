//! Admin-side transport abstraction.
//!
//! `WebSocketClient` previously held a raw `(WsSender, WsReceiver)` from
//! `ewebsock`. To support direct admin↔client TCP without forking the
//! 16+ existing call sites, we wrap both transports behind an
//! [`AdminTransport`] that exposes the minimal `WsSender`/`WsReceiver`
//! surface the admin code actually uses:
//!
//!  - `send(WsMessage)`
//!  - `try_recv() -> Option<WsEvent>`
//!  - `close()`
//!
//! For the WebSocket variant this is a thin pass-through. For the TCP
//! variant we spawn a background dial+handshake task plus a
//! reader/writer pair; reader-side bytes are wrapped as
//! `WsEvent::Message(WsMessage::Binary(_))` so the existing receive loop
//! treats them identically.
//!
//! **Wire protocol** (mirror of `Mastertech4.0/src/transport.rs` and
//! `Mastertech4.0/src/tcp_listener.rs`):
//!  1. Handshake: `MTRX` magic (4 bytes) + version u8 + u32 LE len +
//!     UTF-8 connection_string.
//!  2. Frames: `[u32 LE total_len][u8 tag][payload bytes]` where
//!     tag = `0x01` (binary) or `0x02` (text) and total_len includes the
//!     tag byte.

use crate::Spawner;
use crossbeam::channel::{unbounded, Receiver as XReceiver, Sender as XSender, TryRecvError};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use std::time::Duration;

/// Frame tag bytes — must match `Mastertech4.0/src/transport.rs`.
const FRAME_TAG_BINARY: u8 = 0x01;
const FRAME_TAG_TEXT: u8 = 0x02;
const HANDSHAKE_MAGIC: &[u8; 4] = b"MTRX";
const HANDSHAKE_VERSION: u8 = 1;

/// Hard cap on a single frame so a wedged client can't OOM the admin
/// process. Mirrors the client's `MAX_FRAME_BYTES`.
const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;

/// Tag describing which transport an [`AdminTransport`] is using. Cheap
/// to copy; the `WebSocketClient` exposes this so UI code can show the
/// active path (e.g. a "TCP" or "Relay" badge in the admin chrome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    WebSocket,
    Tcp,
}

pub struct AdminTransport {
    inner: AdminTransportInner,
    kind: TransportKind,
}

enum AdminTransportInner {
    WebSocket {
        sender: WsSender,
        receiver: WsReceiver,
    },
    /// Direct TCP. Reader/writer/dialer tasks run in the background; we
    /// just shuttle data through these channels. Outbound frames go to
    /// `out_tx` (a writer task drains them); synthesized [`WsEvent`]s
    /// arrive on `in_rx`.
    Tcp {
        out_tx: XSender<TcpFrame>,
        in_rx: XReceiver<WsEvent>,
        /// Set to `true` when we've sent a `WsEvent::Closed` so further
        /// `send()` calls become no-ops instead of leaking unbounded-channel
        /// growth on a dead connection.
        closed: bool,
    },
}

#[derive(Debug)]
pub enum TcpFrame {
    Binary(Vec<u8>),
    Text(String),
    Shutdown,
}

impl AdminTransport {
    pub fn from_ws(sender: WsSender, receiver: WsReceiver) -> Self {
        Self {
            inner: AdminTransportInner::WebSocket { sender, receiver },
            kind: TransportKind::WebSocket,
        }
    }

    /// Spawn a TCP dial+handshake+reader+writer set. Returns
    /// immediately; the actual connect happens in the background. A
    /// `WsEvent::Opened` will arrive once the handshake completes; on
    /// failure a `WsEvent::Error` followed by `WsEvent::Closed` is
    /// emitted so the existing receive loop picks it up.
    ///
    /// `target_addr` should be `"<ip>:<port>"`. `connection_string` is
    /// what the client expects to see in the handshake's
    /// connection_string field — we send it back verbatim.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_tcp(target_addr: String, connection_string: String) -> Self {
        let (out_tx, out_rx) = unbounded::<TcpFrame>();
        let (in_tx, in_rx) = unbounded::<WsEvent>();

        let dial_addr = target_addr.clone();
        let id_for_handshake = connection_string.clone();
        let in_tx_dial = in_tx.clone();

        // Use the `displays` crate's existing PlatformSpawner abstraction
        // so we don't have to assume a specific runtime is available.
        crate::PlatformSpawner::spawn(async move {
            run_tcp_session(dial_addr, id_for_handshake, out_rx, in_tx_dial).await;
        });

        Self {
            inner: AdminTransportInner::Tcp {
                out_tx,
                in_rx,
                closed: false,
            },
            kind: TransportKind::Tcp,
        }
    }

    pub fn kind(&self) -> TransportKind {
        self.kind
    }

    /// Mirrors [`ewebsock::WsSender::send`].
    pub fn send(&mut self, msg: WsMessage) {
        match &mut self.inner {
            AdminTransportInner::WebSocket { sender, .. } => sender.send(msg),
            AdminTransportInner::Tcp { out_tx, closed, .. } => {
                if *closed {
                    return;
                }
                match msg {
                    WsMessage::Binary(b) => {
                        let _ = out_tx.send(TcpFrame::Binary(b.into()));
                    }
                    WsMessage::Text(t) => {
                        let _ = out_tx.send(TcpFrame::Text(t.into()));
                    }
                    // Ping/Pong/Close are WS-framing concepts; on TCP we
                    // rely on the socket itself for liveness/teardown.
                    _ => {}
                }
            }
        }
    }

    /// Mirrors [`ewebsock::WsReceiver::try_recv`].
    pub fn try_recv(&mut self) -> Option<WsEvent> {
        match &mut self.inner {
            AdminTransportInner::WebSocket { receiver, .. } => receiver.try_recv(),
            AdminTransportInner::Tcp { in_rx, closed, .. } => match in_rx.try_recv() {
                Ok(ev) => {
                    if matches!(&ev, WsEvent::Closed) {
                        *closed = true;
                    }
                    Some(ev)
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    if *closed {
                        None
                    } else {
                        *closed = true;
                        Some(WsEvent::Closed)
                    }
                }
            },
        }
    }

    /// Mirrors [`ewebsock::WsSender::close`].
    pub fn close(&mut self) {
        match &mut self.inner {
            AdminTransportInner::WebSocket { sender, .. } => sender.close(),
            AdminTransportInner::Tcp { out_tx, closed, .. } => {
                let _ = out_tx.send(TcpFrame::Shutdown);
                *closed = true;
            }
        }
    }
}

/// Background driver for a single TCP session. Dials `target_addr`,
/// performs the handshake, then runs concurrent read/write loops.
///
/// On connect failure the task waits `RETRY_INTERVAL` and tries again —
/// indefinitely — so that a temporarily unreachable client (firewall popup
/// not yet acknowledged, machine rebooting, etc.) reconnects automatically
/// once the path opens up.  Only stops retrying when the outbound channel
/// is dropped (i.e. `AdminTransport::close()` was called).
#[cfg(not(target_arch = "wasm32"))]
async fn run_tcp_session(
    target_addr: String,
    connection_string: String,
    out_rx: XReceiver<TcpFrame>,
    in_tx: XSender<WsEvent>,
) {
    use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    const RETRY_INTERVAL: Duration = Duration::from_secs(3);

    // Wrap the receiver so the writer task can borrow it each session
    // without taking ownership, allowing reconnect on drop+redial.
    let out_rx = Arc::new(Mutex::new(out_rx));

    // Set to true when the admin explicitly closes the transport (Shutdown
    // frame or out_tx dropped); breaks the retry loop.
    let shutdown = Arc::new(AtomicBool::new(false));

    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = in_tx.send(WsEvent::Closed);
            return;
        }

        log::info!("admin_transport -> dialing {target_addr}");

        // ---- Dial ----
        let stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&target_addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                log::warn!("admin_transport -> connect to {target_addr} failed: {e}; retrying in {RETRY_INTERVAL:?}");
                let _ = in_tx.send(WsEvent::Error(format!("TCP connect failed: {e} (retrying…)")));
                // Surface as a toast so the operator sees the failure
                // even when their attention is on a different tab.
                // Dedup at the consumer side will collapse the 3-second
                // retry storm into a single visible toast.
                let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(
                    format!("Admin TCP connect to {target_addr} failed: {e}"),
                ));
                tokio::time::sleep(RETRY_INTERVAL).await;
                continue;
            }
            Err(_) => {
                log::warn!("admin_transport -> connect to {target_addr} timed out; retrying in {RETRY_INTERVAL:?}");
                let _ = in_tx.send(WsEvent::Error(format!("TCP connect timed out (retrying…)")));
                let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(
                    format!("Admin TCP connect to {target_addr} timed out (retrying…)"),
                ));
                tokio::time::sleep(RETRY_INTERVAL).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);

        let (mut read_half, mut write_half) = stream.into_split();

        // ---- Handshake (admin sends) ----
        let id_bytes = connection_string.as_bytes();
        let mut hs = Vec::with_capacity(4 + 1 + 4 + id_bytes.len());
        hs.extend_from_slice(HANDSHAKE_MAGIC);
        hs.push(HANDSHAKE_VERSION);
        hs.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
        hs.extend_from_slice(id_bytes);

        if let Err(e) = write_half.write_all(&hs).await {
            log::warn!("admin_transport -> handshake write failed: {e}; retrying in {RETRY_INTERVAL:?}");
            let _ = in_tx.send(WsEvent::Error(format!("handshake write failed: {e} (retrying…)")));
            tokio::time::sleep(RETRY_INTERVAL).await;
            continue;
        }

        log::info!("admin_transport -> connected + handshake sent to {target_addr}");
        let _ = in_tx.send(WsEvent::Opened);

        // ---- Spawn writer task ----
        let shutdown_writer = shutdown.clone();
        let in_tx_writer = in_tx.clone();
        let out_rx_writer = out_rx.clone();
        let writer_handle = tokio::spawn(async move {
            loop {
                let frame = match tokio::task::spawn_blocking({
                    let rx = out_rx_writer.clone();
                    move || rx.lock().unwrap_or_else(|e| e.into_inner()).recv()
                })
                .await
                {
                    Ok(Ok(f)) => f,
                    Ok(Err(_)) => {
                        // out_tx dropped — treat as intentional shutdown
                        shutdown_writer.store(true, Ordering::Relaxed);
                        break;
                    }
                    Err(e) => {
                        log::warn!("admin_transport -> writer recv join error: {e}");
                        break;
                    }
                };

                match frame {
                    TcpFrame::Shutdown => {
                        shutdown_writer.store(true, Ordering::Relaxed);
                        break;
                    }
                    TcpFrame::Binary(payload) => {
                        if let Err(e) = write_frame(&mut write_half, FRAME_TAG_BINARY, &payload).await {
                            log::info!("admin_transport -> writer error: {e}");
                            let _ = in_tx_writer.send(WsEvent::Error(format!("write: {e}")));
                            break;
                        }
                    }
                    TcpFrame::Text(s) => {
                        if let Err(e) = write_frame(&mut write_half, FRAME_TAG_TEXT, s.as_bytes()).await
                        {
                            log::info!("admin_transport -> writer error: {e}");
                            let _ = in_tx_writer.send(WsEvent::Error(format!("write: {e}")));
                            break;
                        }
                    }
                }
            }
            let _ = write_half.shutdown().await;
        });

        // ---- Reader loop (this task) ----
        loop {
            let total_len = match read_half.read_u32_le().await {
                Ok(n) => n,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        log::info!("admin_transport -> peer closed connection; will retry");
                    } else {
                        log::warn!("admin_transport -> reader error: {e}; will retry");
                        let _ = in_tx.send(WsEvent::Error(format!("read: {e} (retrying…)")));
                    }
                    break;
                }
            };
            if total_len == 0 || total_len > MAX_FRAME_BYTES {
                log::warn!("admin_transport -> bogus frame len {total_len}");
                let _ = in_tx.send(WsEvent::Error(format!("bad frame len: {total_len} (retrying…)")));
                break;
            }
            let tag = match read_half.read_u8().await {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("admin_transport -> reader tag error: {e}");
                    break;
                }
            };
            let payload_len = (total_len - 1) as usize;
            let mut payload = vec![0u8; payload_len];
            if let Err(e) = read_half.read_exact(&mut payload).await {
                log::warn!("admin_transport -> reader payload error: {e}");
                break;
            }
            match tag {
                FRAME_TAG_BINARY => {
                    let _ = in_tx.send(WsEvent::Message(WsMessage::Binary(payload.into())));
                }
                FRAME_TAG_TEXT => {
                    match String::from_utf8(payload) {
                        Ok(s) => {
                            let _ = in_tx.send(WsEvent::Message(WsMessage::Text(s.into())));
                        }
                        Err(e) => {
                            log::warn!("admin_transport -> text frame not utf-8: {e}");
                        }
                    }
                }
                other => {
                    log::warn!("admin_transport -> unknown frame tag: 0x{other:02x}");
                }
            }
        };

        writer_handle.abort();

        if shutdown.load(Ordering::Relaxed) {
            let _ = in_tx.send(WsEvent::Closed);
            return;
        }

        // Session ended (peer closed / read error) — wait and redial
        log::info!("admin_transport -> session ended; reconnecting in {RETRY_INTERVAL:?}");
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn write_frame(
    write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    tag: u8,
    payload: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let total_len: u32 = (payload.len() as u32).saturating_add(1);
    write_half.write_all(&total_len.to_le_bytes()).await?;
    write_half.write_all(&[tag]).await?;
    write_half.write_all(payload).await?;
    Ok(())
}
