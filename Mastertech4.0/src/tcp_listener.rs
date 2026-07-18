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
    ClientTransport, TcpFrame, FRAME_TAG_BINARY, FRAME_TAG_PING, FRAME_TAG_PONG, FRAME_TAG_TEXT,
    HANDSHAKE_MAGIC,
};
use tcp_protocol::is_supported_version;
use anyhow::{anyhow, Context, Result};
use displays::{deserialize_command, DESKTOP_INPUT_TAG, EGUI_INPUT_TAG};
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

// ── Terminal-mode frame broadcast ────────────────────────────────────────────
//
// When Mastertech runs in terminal mode (ratatui) the render loop encodes the
// ratatui buffer with `encode_buffer_with_timestamp` and broadcasts the bytes
// here so any admin connected via direct TCP receives the same live terminal
// rendering that the WS-relay path delivers.
//
// The bytes are the raw zstd-compressed payload produced by
// `encode_buffer_with_timestamp` — no extra tag byte is needed because the
// admin side (WebconsoleTab / receive.rs) identifies terminal frames by their
// zstd magic prefix (0x28).

static TERM_FRAME_TX: OnceLock<broadcast::Sender<Vec<u8>>> = OnceLock::new();

fn term_frame_broadcast() -> &'static broadcast::Sender<Vec<u8>> {
    TERM_FRAME_TX.get_or_init(|| {
        let (tx, _) = broadcast::channel(32);
        tx
    })
}

/// Called by `terminal_mode::websockets::TerminalApp::send_buffer` on every
/// rendered ratatui frame so TCP admin sessions receive the same bytes as the
/// WS relay path.
pub fn broadcast_term_frame(bytes: Vec<u8>) {
    let _ = term_frame_broadcast().send(bytes);
}

// ── Remote-desktop frame broadcast ─────────────────────────────────────────────
//
// The remote-desktop capture thread (`crate::remote_desktop`) pushes raster
// JPEG frames here; every connected TCP admin session forwards them to its
// socket. Mirrors the egui-frame broadcast so no changes to the capture path
// are needed. Bytes already carry the leading `DESKTOP_FRAME_TAG`.

static DESKTOP_FRAME_TX: OnceLock<broadcast::Sender<Vec<u8>>> = OnceLock::new();

fn desktop_frame_broadcast() -> &'static broadcast::Sender<Vec<u8>> {
    DESKTOP_FRAME_TX.get_or_init(|| {
        let (tx, _) = broadcast::channel(32);
        tx
    })
}

/// Called by the remote-desktop capture thread for each encoded frame.
pub fn broadcast_desktop_frame(tagged_bytes: Vec<u8>) {
    let _ = desktop_frame_broadcast().send(tagged_bytes);
}

/// Number of connected admin TCP sessions subscribed to desktop frames.
/// The capture thread uses this to auto-stop after all admins disconnect.
pub fn desktop_frame_subscriber_count() -> usize {
    desktop_frame_broadcast().receiver_count()
}

/// Preferred port. We try this first; if it's taken (e.g. another
/// Mastertech instance, or unrelated software), we fall back to an
/// OS-assigned ephemeral port and publish whatever we got to the DB.
pub const PREFERRED_PORT: u16 = 9101;

const SELF_UPDATE_CHILD_ENV: &str = "MASTERTECH_SELF_UPDATE_CHILD";

/// True when this process was spawned by a remote self-update relaunch.
pub fn is_self_update_child() -> bool {
    std::env::var(SELF_UPDATE_CHILD_ENV)
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Waits until the preferred TCP port can be bound (old process released it).
pub async fn wait_for_preferred_port_available() {
    use tokio::net::TcpListener;

    const MAX_WAIT: Duration = Duration::from_secs(60);
    const STEP: Duration = Duration::from_millis(250);
    let deadline = tokio::time::Instant::now() + MAX_WAIT;
    while tokio::time::Instant::now() < deadline {
        match TcpListener::bind(format!("0.0.0.0:{PREFERRED_PORT}")).await {
            Ok(l) => {
                drop(l);
                log::info!(
                    "tcp_listener -> preferred port {PREFERRED_PORT} free after self-update wait"
                );
                return;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tokio::time::sleep(STEP).await;
            }
            Err(e) => {
                log::warn!("tcp_listener -> bind probe error while waiting for port: {e}");
                tokio::time::sleep(STEP).await;
            }
        }
    }
    log::warn!(
        "tcp_listener -> preferred port {PREFERRED_PORT} still busy after {MAX_WAIT:?}"
    );
}

/// Hard cap on a single inbound frame so a malicious or buggy peer can't
/// allocate gigabytes by sending a giant length prefix.
const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024; // 64 MiB
/// Bounded depth of the file-chunk writer channel (× 4 MiB chunk = in-flight
/// cap). Small enough to bound RAM, deep enough to keep the socket busy.
const FILE_CHANNEL_DEPTH: usize = 4;

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
                    // Debug only: a reachability probe fires this every 30s
                    // per admin and would otherwise spam INFO.  The
                    // post-handshake info log inside handle_session marks
                    // real admin connections.
                    log::debug!("tcp_listener -> inbound TCP from {peer}");
                    tokio::spawn(async move {
                        if let Err(e) = handle_session(stream, peer).await {
                            log::warn!("tcp_listener -> session {peer} ended: {e:#}");
                        } else {
                            log::debug!("tcp_listener -> session {peer} closed cleanly");
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
    // SO_KEEPALIVE + TCP_NODELAY.  Keepalive is the safety net for a peer
    // that vanishes silently (NAT timeout, cable yank) when no app-level
    // pings are flowing yet; combined with the master's 15 s ping cadence
    // both sides notice a dead peer within ~30 s.
    if let Err(e) = tcp_protocol::apply_tcp_options(&stream) {
        log::warn!("tcp_listener -> apply_tcp_options failed for {peer}: {e}");
    }

    let (mut read_half, write_half) = stream.into_split();

    // 1) Handshake
    let expected_id = get_client_hash().connection_string;
    let outcome = perform_handshake(&mut read_half, &expected_id, peer)
        .await
        .with_context(|| format!("handshake with {peer}"))?;
    match outcome {
        HandshakeOutcome::Authenticated => {
            log::info!(
                "tcp_listener -> admin session established with {peer} (id={expected_id})"
            );
        }
        HandshakeOutcome::Probe => {
            // Reachability probe (see displays/src/ui_data/reachability.rs):
            // peer connected, sent no bytes, and closed.  Don't spawn the
            // full session machinery — just return cleanly so the accept
            // loop logs a quiet "closed cleanly" instead of a WARN.
            log::debug!("tcp_listener -> reachability probe from {peer}");
            return Ok(());
        }
    }

    // 2) Spawn writer task. Control frames use an unbounded channel; bulk
    //    file chunks use a small BOUNDED channel so a large download paces to
    //    the socket instead of queueing gigabytes in RAM.
    let (write_tx, write_rx) = unbounded_channel::<TcpFrame>();
    let (file_tx, file_rx) = tokio::sync::mpsc::channel::<TcpFrame>(FILE_CHANNEL_DEPTH);
    let writer_handle = tokio::spawn(writer_task(write_half, write_rx, file_rx, peer));

    // 3) Per-session command dispatcher
    let mut client = TerminalWebsocketClient::new();
    let mut transport = ClientTransport::Tcp { ctrl: write_tx.clone(), file: file_tx.clone() };

    let result = run_session_loop(read_half, &mut client, &mut transport, &write_tx).await;

    // 4) Tear down
    drop(transport); // close writer channels so writer_task exits
    drop(write_tx);
    drop(file_tx);
    let _ = writer_handle.await;
    result
}

/// Outcome of [`perform_handshake`].
///
/// We distinguish a real authenticated handshake from a bare TCP
/// connect-and-close so the accept loop can stay quiet for the
/// reachability prober (see `displays/src/ui_data/reachability.rs`),
/// which deliberately opens a socket without sending the handshake.
enum HandshakeOutcome {
    Authenticated,
    Probe,
}

/// Read and validate the handshake preamble from the admin side.
async fn perform_handshake(
    read_half: &mut tokio::net::tcp::OwnedReadHalf,
    expected_id: &str,
    peer: SocketAddr,
) -> Result<HandshakeOutcome> {
    // Magic
    let mut magic = [0u8; 4];
    match tokio::time::timeout(Duration::from_secs(5), read_half.read_exact(&mut magic)).await {
        Ok(Ok(_)) => {}
        // Peer closed before sending any bytes — this is the reachability
        // prober (see displays/src/ui_data/reachability.rs), not a
        // malformed admin.  On Linux the drop produces a clean FIN
        // (`UnexpectedEof`); on Windows the drop frequently RSTs
        // (`ConnectionReset`, os error 10054) or shows up as a generic
        // `ConnectionAborted`.  Treat all three as a clean probe.
        Ok(Err(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
            ) =>
        {
            return Ok(HandshakeOutcome::Probe);
        }
        Ok(Err(e)) => return Err(anyhow!("handshake read magic: {e}")),
        Err(_) => return Err(anyhow!("handshake timeout reading magic from {peer}")),
    }
    if &magic != HANDSHAKE_MAGIC {
        return Err(anyhow!(
            "bad handshake magic from {peer}: got {magic:?}, expected MTRX"
        ));
    }

    // Version.  Accept any version in the supported window so a v1 agent
    // can talk to a v2 master (and vice versa) — the version *equality*
    // check this used to do produced "peer closed connection" loops the
    // moment the master bumped its handshake byte to V2.  See
    // tcp_protocol::is_supported_version.
    let version = tokio::time::timeout(Duration::from_secs(5), read_half.read_u8())
        .await
        .map_err(|_| anyhow!("handshake timeout reading version from {peer}"))?
        .map_err(|e| anyhow!("handshake read version: {e}"))?;
    if !is_supported_version(version) {
        return Err(anyhow!(
            "unsupported handshake version from {peer}: got {version}, \
             agent accepts {}..={}",
            tcp_protocol::HANDSHAKE_VERSION_V1,
            tcp_protocol::HANDSHAKE_VERSION_CURRENT,
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
    Ok(HandshakeOutcome::Authenticated)
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
    // Subscribe to terminal-mode (ratatui) buffer frames for the same reason.
    let mut term_frame_rx = term_frame_broadcast().subscribe();
    // Subscribe to raster desktop frames (full remote-desktop control).
    let mut desktop_frame_rx = desktop_frame_broadcast().subscribe();

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
                        match bytes.first().copied() {
                            Some(EGUI_INPUT_TAG) => {
                                // Egui input event from admin — route to the frame capture plugin.
                                if let Ok((ev, _)) = bincode::serde::decode_from_slice::<
                                    displays::plugins::EguiInputEvent,
                                    _,
                                >(&bytes[1..], bincode::config::standard())
                                {
                                    let _ = displays::plugins::egui_input_sender().try_send(ev);
                                }
                            }
                            Some(DESKTOP_INPUT_TAG) => {
                                // Full-desktop input event from admin — inject via enigo.
                                if let Ok((ev, _)) = bincode::serde::decode_from_slice::<
                                    displays::remote_desktop::DesktopInputEvent,
                                    _,
                                >(&bytes[1..], bincode::config::standard())
                                {
                                    let _ = crate::remote_desktop::desktop_input_sender().try_send(ev);
                                }
                            }
                            _ => {
                                let cmd = deserialize_command(&bytes);
                                client.handle_command(cmd, transport).await;
                            }
                        }
                    }
                    Ok(InboundFrame::Text(text)) => {
                        // Route plain-text commands to the persistent PowerShell
                        // session.  The admin shell panel sends commands as
                        // text frames (non-interactive mode); we handle them
                        // the same way the WebSocket relay path does.
                        client.handle_text_command(text).await;
                    }
                    Ok(InboundFrame::Ping(payload)) => {
                        // Master keepalive: echo the payload back as a Pong so
                        // it can measure round-trip and confirm liveness.  We
                        // don't enforce a payload length here — the master
                        // already validates with `decode_ping_payload`.
                        let _ = write_tx.send(TcpFrame::Pong(payload));
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
            // Outbound: forward ratatui terminal buffer frames to the admin
            // when Mastertech is running in terminal mode on the client.
            term_result = term_frame_rx.recv() => {
                match term_result {
                    Ok(bytes) => {
                        let _ = write_tx.send(TcpFrame::Binary(bytes));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        term_frame_rx = term_frame_broadcast().subscribe();
                    }
                    Err(broadcast::error::RecvError::Closed) => {}
                }
            }
            // Outbound: forward raster desktop frames to the admin.
            desktop_result = desktop_frame_rx.recv() => {
                match desktop_result {
                    Ok(tagged_bytes) => {
                        let _ = write_tx.send(TcpFrame::Binary(tagged_bytes));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        desktop_frame_rx = desktop_frame_broadcast().subscribe();
                    }
                    Err(broadcast::error::RecvError::Closed) => {}
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
    /// Master-side keepalive ping (v2+ protocol). Payload is the 16-byte
    /// `[seq][epoch_ms]` blob from `tcp_protocol::encode_ping_payload`;
    /// the session loop echoes it back as a `Pong`.
    Ping(Vec<u8>),
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
        FRAME_TAG_PING => Ok(InboundFrame::Ping(payload)),
        // Per tcp_protocol docs: unknown tags MUST be ignored, not fatal —
        // this is what lets a v1 agent survive talking to a v3 master.
        // We currently don't expect Pong inbound (agent doesn't send Ping),
        // so it falls through here too.
        other => {
            log::warn!(
                "tcp_listener -> ignoring unknown frame tag: 0x{other:02x} \
                 (payload {} bytes)",
                payload.len()
            );
            // Re-read by recursing once.  Bounded because the next frame
            // either parses cleanly, hits EOF, or fails on the length
            // prefix — none of which recurse further.
            Box::pin(read_frame(read_half)).await
        }
    }
}

async fn writer_task(
    mut write_half: tokio::net::tcp::OwnedWriteHalf,
    mut rx: UnboundedReceiver<TcpFrame>,
    mut file_rx: tokio::sync::mpsc::Receiver<TcpFrame>,
    peer: SocketAddr,
) {
    loop {
        // Control frames (pongs, egui/desktop frames, command output) are
        // biased ahead of bulk file chunks so liveness pongs are never
        // starved by a large download; the bounded `file_rx` gives the
        // download natural backpressure.
        let frame = tokio::select! {
            biased;
            Some(frame) = rx.recv() => frame,
            Some(frame) = file_rx.recv() => frame,
            else => break,
        };
        let (tag, payload) = match frame {
            TcpFrame::Binary(b) => (FRAME_TAG_BINARY, b),
            TcpFrame::Text(t) => (FRAME_TAG_TEXT, t.into_bytes()),
            TcpFrame::Pong(b) => (FRAME_TAG_PONG, b),
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

/// Create-or-update this machine's `connected_client` row with the identity
/// fields the typed read path requires (`client_hash`, `computer`,
/// `connection_string`, `connected`, `assigned_user`). Refuses to write when
/// `client_hash` or `computer` is missing. A row missing `client_hash` fails
/// SurrealValue deserialization and terminates the store-wide admin LIVE
/// query for every client, so every create path funnels through here.
pub async fn upsert_self_identity(connected: bool) {
    use database::db;

    let identity = get_client_hash();
    if identity.client_hash.is_empty() {
        log::error!("upsert_self_identity -> refusing to write connected_client without client_hash");
        return;
    }
    let Some(computer) = identity.computer.clone() else {
        log::error!("upsert_self_identity -> refusing to write connected_client without computer");
        return;
    };

    let res = db()
        .query(
            "UPSERT $id SET client_hash = $client_hash, \
             connection_string = $cs, computer = $computer, \
             connected = $connected, assigned_user = $auth.id, \
             last_update = time::now()",
        )
        .bind(("id", identity.id.clone()))
        .bind(("client_hash", identity.client_hash.clone()))
        .bind(("cs", identity.connection_string.clone()))
        .bind(("computer", computer))
        .bind(("connected", connected))
        .await;
    if let Err(e) = res {
        log::warn!("upsert_self_identity -> upsert failed: {e:?}");
    }
}

/// Bind the direct-TCP admin listener, add a firewall rule, and publish the
/// address to this client's `connected_client` row so admins can dial
/// directly without going through the WS relay.
///
/// Called from both the Egui run mode (`first_run.rs`) and the terminal run
/// mode (`terminal_mode/mod.rs`) so either entry point benefits from
/// direct-TCP admin connections.
pub async fn spawn_direct_tcp_listener(client_uuid: database::schema::RecordId) {
    use crate::utilities::network::{detect_local_ipv4, try_add_firewall_rule};
    use database::db;

    if is_self_update_child() {
        wait_for_preferred_port_available().await;
    }

    let local_ip = match detect_local_ipv4() {
        Some(ip) => ip,
        None => {
            log::warn!(
                "spawn_direct_tcp_listener -> no routable IPv4 detected; \
                 skipping direct-TCP listener (relay path still active)"
            );
            return;
        }
    };

    let (listener, addr) = match bind_listener().await {
        Ok(pair) => pair,
        Err(e) => {
            log::warn!(
                "spawn_direct_tcp_listener -> bind failed: {e:?} \
                 (relay path still active)"
            );
            return;
        }
    };

    // Best-effort Windows firewall rule. If it fails, the OS firewall
    // popup still appears on the first inbound connection and the user
    // can click "Allow" once. We never block on this.
    #[cfg(target_os = "windows")]
    match try_add_firewall_rule(addr.port(), "Mastertech Direct TCP") {
        Ok(true) => log::info!(
            "spawn_direct_tcp_listener -> firewall rule added for port {}",
            addr.port()
        ),
        Ok(false) => log::info!(
            "spawn_direct_tcp_listener -> firewall rule not added (likely needs admin); \
             relying on Windows allow-access popup on first bind"
        ),
        Err(e) => log::warn!("spawn_direct_tcp_listener -> netsh spawn failed: {e}"),
    }
    #[cfg(not(target_os = "windows"))]
    let _ = try_add_firewall_rule;

    log::info!(
        "spawn_direct_tcp_listener -> listening on {} (advertise as {}:{})",
        addr,
        local_ip,
        addr.port()
    );

    // Publish IP+port to the client's row so admins can dial directly.
    // Also re-publish `connection_string` so it can never drift from what
    // `get_client_hash()` computes at runtime — otherwise a stale row would
    // cause every direct-TCP handshake to fail with an ID mismatch.
    // Retries with exponential back-off in case the DB row hasn't been
    // created yet (first-run race between the WS sender and this task).
    let publish_uuid = client_uuid.clone();
    let port = addr.port();
    let identity = get_client_hash();
    let connection_string = identity.connection_string;
    let client_hash = identity.client_hash;
    let computer = identity.computer;
    tokio::spawn(async move {
        let ip_string = local_ip.to_string();
        for attempt in 0..5u32 {
            // Create-or-update: sole writer of this client's row in GUI mode,
            // so it sets every required identity field, not just TCP coords.
            let res = db()
                .query(
                    "UPSERT $client SET local_ip = $ip, tcp_port = $port, \
                     connection_string = $cs, client_hash = $client_hash, \
                     computer = $computer, \
                     connected = true, assigned_user = $auth.id, \
                     last_update = time::now()",
                )
                .bind(("client", publish_uuid.clone()))
                .bind(("ip", ip_string.clone()))
                .bind(("port", port))
                .bind(("cs", connection_string.clone()))
                .bind(("client_hash", client_hash.clone()))
                .bind(("computer", computer.clone()))
                .await;
            match res {
                Ok(_) => {
                    log::info!(
                        "spawn_direct_tcp_listener -> published {ip_string}:{port} \
                         (cs={connection_string}) to {:?}",
                        publish_uuid
                    );
                    return;
                }
                Err(e) => {
                    log::warn!(
                        "spawn_direct_tcp_listener -> publish attempt {} failed: {e:?}",
                        attempt + 1
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(2_u64.pow(attempt))).await;
                }
            }
        }
        log::error!(
            "spawn_direct_tcp_listener -> failed to publish IP/port after 5 attempts; \
             admins will fall back to relay"
        );
    });

    // Periodic heartbeat: refresh `last_update` and re-assert
    // `connected = true` every 15 minutes so the axum_server's 30-min
    // stale sweep (heartbeat_sweep.rs) doesn't flip this row to
    // `connected = false` on long-lived egui sessions. The terminal-mode
    // path heartbeats through its own websocket loop; this is the
    // egui-mode equivalent. 15 min sits comfortably under the 30-min
    // threshold without flooding the DB.
    let heartbeat_uuid = client_uuid.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = displays::wait_for_shutdown() => {
                    log::info!("tcp_listener heartbeat -> shutdown signaled; stopping");
                    return;
                }
                _ = tokio::time::sleep(Duration::from_secs(15 * 60)) => {
                    let res = db()
                        .query("UPDATE $client SET connected = true, last_update = time::now()")
                        .bind(("client", heartbeat_uuid.clone()))
                        .await;
                    match res {
                        Ok(_) => log::debug!(
                            "tcp_listener heartbeat -> refreshed last_update"
                        ),
                        Err(e) => log::warn!(
                            "tcp_listener heartbeat -> refresh failed: {e:?}"
                        ),
                    }
                }
            }
        }
    });

    accept_loop(listener).await;
}
