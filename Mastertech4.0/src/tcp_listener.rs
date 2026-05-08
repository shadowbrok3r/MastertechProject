//! Direct admin↔client TCP listener.
//!
//! Replaces the `websocket_server2` relay for in-shop sessions. Customer
//! machines bind a TCP listener (default port 9101, fallback OS-assigned)
//! and admins dial it directly using the IP/port published to the
//! `connected_client` row in SurrealDB.
//!
//! **Wire format** (client expects from admin):
//! 1. **Handshake**: `MTRX` magic (4 bytes) + version u8 + u32 LE len +
//!    UTF-8 connection_string.
//!    Client closes the socket if magic, version, or connection_string
//!    don't match its own.
//! 2. **Frames**: `[u32 LE total_len][u8 tag][payload bytes]` where tag is
//!    `0x01` for binary `Cmd` payloads, `0x02` for UTF-8 text. `total_len`
//!    counts the tag byte plus payload (so payload length = total_len - 1).
//!
//! Each accepted connection spawns its own [`TerminalWebsocketClient`] and
//! runs the same `handle_command` dispatcher as the WS path — no handler
//! duplication.

use crate::filesystem::get_client_hash;
use crate::terminal_mode::websockets::TerminalWebsocketClient;
use crate::transport::{
    ClientTransport, TcpFrame, FRAME_TAG_BINARY, FRAME_TAG_TEXT, HANDSHAKE_MAGIC, HANDSHAKE_VERSION,
};
use anyhow::{anyhow, Context, Result};
use displays::{deserialize_command, EGUI_INPUT_TAG};
use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::mpsc as tokio_mpsc;

// ── Egui frame broadcast ──────────────────────────────────────────────────────
//
// The client captures egui frame snapshots on every UI pass and needs to
// forward them to any admin that is connected via direct TCP (rather than
// via the WebSocket relay).  We use a tokio broadcast channel so multiple
// simultaneous admin TCP sessions all get the same frames, and old frames
// are automatically discarded when the ring buffer is full.

static EGUI_FRAME_TX: OnceLock<broadcast::Sender<Vec<u8>>> = OnceLock::new();

fn frame_broadcast() -> &'static broadcast::Sender<Vec<u8>> {
    EGUI_FRAME_TX.get_or_init(|| {
        let (tx, _) = broadcast::channel(32);
        tx
    })
}

/// Called by `first_run.rs` whenever a fresh egui frame is ready.
/// Bytes should already include the leading `EGUI_FRAME_TAG` byte so the
/// admin side can decode it the same way it handles WS relay frames.
pub fn broadcast_egui_frame(tagged_bytes: Vec<u8>) {
    // Ignore errors: no subscribers = no active TCP admin sessions.
    let _ = frame_broadcast().send(tagged_bytes);
}

/// Preferred port. We try this first; if it's taken (e.g. another
/// Mastertech instance, or unrelated software), we fall back to an
/// OS-assigned ephemeral port and publish whatever we got to the DB.
pub const PREFERRED_PORT: u16 = 9101;

/// Hard cap on a single inbound frame so a malicious or buggy peer can't
/// allocate gigabytes by sending a giant length prefix.
const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024; // 64 MiB

/// Bind a TCP listener for direct admin sessions and return the bound
/// address. Tries [`PREFERRED_PORT`] first, falls back to
/// `0` (OS-assigned) on `EADDRINUSE`.
pub async fn bind_listener() -> Result<(TcpListener, SocketAddr)> {
    let preferred = format!("0.0.0.0:{PREFERRED_PORT}");
    match TcpListener::bind(&preferred).await {
        Ok(l) => {
            let addr = l.local_addr()?;
            log::info!("tcp_listener -> bound preferred port: {addr}");
            Ok((l, addr))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            log::warn!(
                "tcp_listener -> preferred port {PREFERRED_PORT} taken ({e}); falling back to OS-assigned"
            );
            let l = TcpListener::bind("0.0.0.0:0")
                .await
                .context("bind 0.0.0.0:0 fallback")?;
            let addr = l.local_addr()?;
            log::info!("tcp_listener -> bound fallback port: {addr}");
            Ok((l, addr))
        }
        Err(e) => Err(anyhow!("bind listener: {e}")),
    }
}

/// Accept loop. Spawns a per-connection task that handles handshake,
/// frame I/O, and Cmd dispatch.
///
/// Exits cleanly when [`displays::wait_for_shutdown`] resolves so the
/// `#[tokio::main]` runtime drop after `eframe::run_native` returns doesn't
/// hang on a perpetually-pending `accept().await` (which on Windows can keep
/// the launching terminal alive past egui window close).
pub async fn accept_loop(listener: TcpListener) {
    loop {
        tokio::select! {
            biased;
            _ = displays::wait_for_shutdown() => {
                log::info!("tcp_listener -> shutdown signaled; stopping accept loop");
                return;
            }
            res = listener.accept() => match res {
                Ok((stream, peer)) => {
                    log::info!("tcp_listener -> admin connected from {peer}");
                    tokio::spawn(async move {
                        if let Err(e) = handle_session(stream, peer).await {
                            log::warn!("tcp_listener -> session {peer} ended: {e}");
                        } else {
                            log::info!("tcp_listener -> session {peer} closed cleanly");
                        }
                    });
                }
                Err(e) => {
                    log::error!("tcp_listener -> accept error: {e}; sleeping 1s");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

/// Per-connection driver.
///
/// 1. Reads + validates the handshake.
/// 2. Spawns the writer task that drains a `TcpFrame` channel and writes
///    length-prefixed frames to the socket.
/// 3. Constructs a per-session [`TerminalWebsocketClient`] and a
///    [`ClientTransport::Tcp`] pointing at the writer channel.
/// 4. `tokio::select!`s between inbound frames and the client's outbound
///    side channels (`bin_rx`, `command_rx`, `sysinfo_rx`) so live
///    streams (e.g. sysinfo) flow back to the admin without the admin
///    having to drive the loop with another request.
async fn handle_session(stream: TcpStream, peer: SocketAddr) -> Result<()> {
    // Disable Nagle so small Cmd ack frames don't stall under load.
    stream.set_nodelay(true).ok();

    let (mut read_half, write_half) = stream.into_split();

    // 1) Handshake
    let expected_id = get_client_hash().connection_string;
    perform_handshake(&mut read_half, &expected_id, peer)
        .await
        .with_context(|| format!("handshake with {peer}"))?;
    log::info!("tcp_listener -> handshake OK for {peer} (id={expected_id})");

    // 2) Spawn writer task
    let (write_tx, write_rx) = unbounded_channel::<TcpFrame>();
    let writer_handle = tokio::spawn(writer_task(write_half, write_rx, peer));

    // 3) Per-session command dispatcher
    let mut client = TerminalWebsocketClient::new();
    let mut transport = ClientTransport::Tcp(write_tx.clone());

    let result = run_session_loop(read_half, &mut client, &mut transport, &write_tx).await;

    // 4) Tear down
    drop(transport); // close writer channel so writer_task exits
    drop(write_tx);
    let _ = writer_handle.await;
    result
}

/// Read and validate the handshake preamble from the admin side.
async fn perform_handshake(
    read_half: &mut tokio::net::tcp::OwnedReadHalf,
    expected_id: &str,
    peer: SocketAddr,
) -> Result<()> {
    // Magic
    let mut magic = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), read_half.read_exact(&mut magic))
        .await
        .map_err(|_| anyhow!("handshake timeout reading magic from {peer}"))?
        .map_err(|e| anyhow!("handshake read magic: {e}"))?;
    if &magic != HANDSHAKE_MAGIC {
        return Err(anyhow!(
            "bad handshake magic from {peer}: got {magic:?}, expected MTRX"
        ));
    }

    // Version
    let version = tokio::time::timeout(Duration::from_secs(5), read_half.read_u8())
        .await
        .map_err(|_| anyhow!("handshake timeout reading version from {peer}"))?
        .map_err(|e| anyhow!("handshake read version: {e}"))?;
    if version != HANDSHAKE_VERSION {
        return Err(anyhow!(
            "unsupported handshake version from {peer}: got {version}, expected {HANDSHAKE_VERSION}"
        ));
    }

    // Connection string length + bytes
    let id_len = tokio::time::timeout(Duration::from_secs(5), read_half.read_u32_le())
        .await
        .map_err(|_| anyhow!("handshake timeout reading id len from {peer}"))?
        .map_err(|e| anyhow!("handshake read id len: {e}"))?;
    if id_len > 1024 {
        return Err(anyhow!("handshake id_len too large from {peer}: {id_len}"));
    }
    let mut id_bytes = vec![0u8; id_len as usize];
    tokio::time::timeout(Duration::from_secs(5), read_half.read_exact(&mut id_bytes))
        .await
        .map_err(|_| anyhow!("handshake timeout reading id bytes from {peer}"))?
        .map_err(|e| anyhow!("handshake read id bytes: {e}"))?;
    let claimed_id = std::str::from_utf8(&id_bytes)
        .map_err(|e| anyhow!("handshake id not utf-8 from {peer}: {e}"))?;
    if claimed_id != expected_id {
        return Err(anyhow!(
            "handshake id mismatch from {peer}: got {claimed_id:?}, expected {expected_id:?}"
        ));
    }
    Ok(())
}

/// Inbound-frame + outbound-channel multiplexer for a single session.
///
/// **Why a dedicated reader task:** `read_frame` performs multiple
/// `read_exact` calls per logical frame. If `tokio::select!` cancels that
/// future mid-read (because an egui frame or `command_rx` branch fired
/// first), the next poll starts a **fresh** `read_u32_le` while the socket
/// may still be mid-frame — the length prefix is then mis-decoded as
/// hundreds of megabytes and the session dies with `frame too large`.
/// A single task owns the read half and forwards **complete** frames on a
/// channel so reads are never cancelled part-way.
async fn run_session_loop(
    read_half: tokio::net::tcp::OwnedReadHalf,
    client: &mut TerminalWebsocketClient,
    transport: &mut ClientTransport,
    write_tx: &UnboundedSender<TcpFrame>,
) -> Result<()> {
    // Subscribe to egui frames so this TCP session can forward them to the
    // admin without any changes to the frame-capture path in first_run.rs.
    let mut frame_rx = frame_broadcast().subscribe();

    let (in_tx, mut in_rx) = tokio_mpsc::unbounded_channel::<Result<InboundFrame, anyhow::Error>>();
    let reader_handle = tokio::spawn(reader_task(read_half, in_tx));

    let result = loop {
        tokio::select! {
            biased;

            // Process-wide shutdown: stop draining channels and let the writer
            // task close the socket.  Without this, a connected admin session
            // can keep the runtime busy long enough that runtime drop hangs on
            // exit.
            _ = displays::wait_for_shutdown() => {
                log::info!("tcp_listener -> shutdown signaled; ending session loop");
                break Ok(());
            }

            // Inbound: complete frames only (never partial — see module comment).
            msg = in_rx.recv() => {
                let Some(res) = msg else {
                    log::info!("tcp_listener -> inbound channel closed");
                    break Ok(());
                };
                match res {
                    Ok(InboundFrame::Binary(bytes)) => {
                        if bytes.first().copied() == Some(EGUI_INPUT_TAG) {
                            // Egui input event from admin — route to the frame capture plugin.
                            if let Ok((ev, _)) = bincode::serde::decode_from_slice::<
                                displays::plugins::EguiInputEvent,
                                _,
                            >(&bytes[1..], bincode::config::standard())
                            {
                                let _ = displays::plugins::egui_input_sender().try_send(ev);
                            }
                        } else {
                            let cmd = deserialize_command(&bytes);
                            client.handle_command(cmd, transport).await;
                        }
                    }
                    Ok(InboundFrame::Text(_text)) => {
                        // Phase 1: text frames are unused (control strings
                        // like MASTER_CONNECTED were a relay-server thing).
                    }
                    Ok(InboundFrame::Eof) => {
                        log::info!("tcp_listener -> peer closed connection cleanly");
                        break Ok(());
                    }
                    Err(e) => break Err(e),
                }
            }
            // Outbound: drain the side-channels the dispatcher uses for
            // streaming responses (process stdout, sysinfo loop, etc.) so
            // they reach the admin without needing another inbound poke.
            Some(bin) = client.command_rx.recv() => {
                let _ = write_tx.send(TcpFrame::Binary(bin));
            }
            Some(sysinfo) = client.sysinfo_rx.recv() => {
                let _ = write_tx.send(TcpFrame::Binary(sysinfo));
            }
            Some(bin) = client.bin_rx.recv() => {
                let _ = write_tx.send(TcpFrame::Binary(bin));
            }
            // Outbound: forward egui frame snapshots to the admin.
            // Lag errors (receiver fell behind) are fine — just grab the
            // latest frame and keep going.
            frame_result = frame_rx.recv() => {
                match frame_result {
                    Ok(tagged_bytes) => {
                        let _ = write_tx.send(TcpFrame::Binary(tagged_bytes));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Resubscribe to skip past missed frames.
                        frame_rx = frame_broadcast().subscribe();
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Sender dropped (shouldn't happen but handle cleanly).
                    }
                }
            }
        }
    };

    reader_handle.abort();
    result
}

/// Serializes all inbound length-prefixed frames onto `in_tx`. Never
/// returns until EOF or a fatal read / protocol error.
async fn reader_task(
    mut read_half: tokio::net::tcp::OwnedReadHalf,
    in_tx: tokio_mpsc::UnboundedSender<Result<InboundFrame, anyhow::Error>>,
) {
    loop {
        let frame = read_frame(&mut read_half).await;
        let stop = frame.is_err() || matches!(&frame, Ok(InboundFrame::Eof));
        if in_tx.send(frame).is_err() {
            break;
        }
        if stop {
            break;
        }
    }
}

enum InboundFrame {
    Binary(Vec<u8>),
    Text(String),
    Eof,
}

async fn read_frame(read_half: &mut tokio::net::tcp::OwnedReadHalf) -> Result<InboundFrame> {
    // total_len = tag byte (1) + payload length
    let total_len = match read_half.read_u32_le().await {
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(InboundFrame::Eof),
        Err(e) => return Err(anyhow!("read frame len: {e}")),
    };
    if total_len == 0 {
        return Err(anyhow!("zero-length frame"));
    }
    if total_len > MAX_FRAME_BYTES {
        return Err(anyhow!("frame too large: {total_len} bytes"));
    }
    let tag = read_half
        .read_u8()
        .await
        .map_err(|e| anyhow!("read frame tag: {e}"))?;
    let payload_len = (total_len - 1) as usize;
    let mut payload = vec![0u8; payload_len];
    read_half
        .read_exact(&mut payload)
        .await
        .map_err(|e| anyhow!("read frame payload ({payload_len} bytes): {e}"))?;
    match tag {
        FRAME_TAG_BINARY => Ok(InboundFrame::Binary(payload)),
        FRAME_TAG_TEXT => {
            let s = String::from_utf8(payload)
                .map_err(|e| anyhow!("text frame not utf-8: {e}"))?;
            Ok(InboundFrame::Text(s))
        }
        other => Err(anyhow!("unknown frame tag: 0x{other:02x}")),
    }
}

async fn writer_task(
    mut write_half: tokio::net::tcp::OwnedWriteHalf,
    mut rx: UnboundedReceiver<TcpFrame>,
    peer: SocketAddr,
) {
    while let Some(frame) = rx.recv().await {
        let (tag, payload) = match frame {
            TcpFrame::Binary(b) => (FRAME_TAG_BINARY, b),
            TcpFrame::Text(t) => (FRAME_TAG_TEXT, t.into_bytes()),
        };
        let total_len = (payload.len() as u64).saturating_add(1);
        if total_len > MAX_FRAME_BYTES as u64 {
            log::warn!(
                "tcp_listener -> dropping outbound frame to {peer}: too large ({total_len} bytes)"
            );
            continue;
        }
        let total_len_u32 = total_len as u32;
        if let Err(e) = write_half.write_all(&total_len_u32.to_le_bytes()).await {
            log::info!("tcp_listener -> write len to {peer} failed: {e}");
            return;
        }
        if let Err(e) = write_half.write_all(&[tag]).await {
            log::info!("tcp_listener -> write tag to {peer} failed: {e}");
            return;
        }
        if let Err(e) = write_half.write_all(&payload).await {
            log::info!("tcp_listener -> write payload to {peer} failed: {e}");
            return;
        }
    }
    let _ = write_half.shutdown().await;
}
