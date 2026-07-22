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

#[cfg(not(target_arch = "wasm32"))]
use crate::Spawner;
use crossbeam::channel::{Receiver as XReceiver, Sender as XSender, TryRecvError};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
#[cfg(not(target_arch = "wasm32"))]
use web_time::Duration;
// Wire-protocol constants live in the shared `tcp_protocol` crate so this
// file and `Mastertech4.0/src/{transport,tcp_listener}.rs` cannot drift.
pub use tcp_protocol::{
    FRAME_TAG_BINARY, FRAME_TAG_PING, FRAME_TAG_PONG, FRAME_TAG_SHAPE_FP, FRAME_TAG_TEXT,
    HANDSHAKE_MAGIC, HANDSHAKE_VERSION_CURRENT as HANDSHAKE_VERSION, MAX_FRAME_BYTES,
};

/// Tag describing which transport an [`AdminTransport`] is using. Cheap
/// to copy; the `WebSocketClient` exposes this so UI code can show the
/// active path (e.g. a "TCP" or "Relay" badge in the admin chrome).
/// `Relay` is a TCP-variant session running the MTRX protocol over a relay
/// tunnel; it shares TCP's in-band ping/pong liveness semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    WebSocket,
    Tcp,
    Relay,
}

pub struct AdminTransport {
    inner: AdminTransportInner,
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
        /// Shared with `run_session` so `close()` can break the retry
        /// loop even when no writer task is running yet (i.e. we're stuck
        /// dialing an unreachable host).  Without this, sending
        /// `TcpFrame::Shutdown` only ends a session that has a live
        /// writer — a dial-retry loop has no writer, so the Shutdown
        /// frame sits in the queue forever and the admin keeps trying to
        /// reconnect to a client the operator already disconnected.
        ///
        /// The retry loop polls this atomic at 200 ms granularity (see
        /// `shutdown_aware_sleep`) so a `close()` is honored within
        /// ~200 ms even if we're mid-sleep between dial attempts.
        shutdown: Arc<AtomicBool>,
        /// Wall-clock ms until which relaxed ping/read deadlines apply
        /// while waiting for a client to relaunch after self-update.
        relaunch_grace_until_ms: Arc<AtomicU64>,
        /// Set true when this session runs over the relay tunnel — either
        /// started that way (`from_tunnel`) or fell back after repeated TCP
        /// dial failures. Shared with the session task so `kind()` reflects
        /// the live path.
        tunnel_active: Arc<AtomicBool>,
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
        }
    }

    /// Spawn a session that starts by dialing direct TCP and permanently
    /// falls back to the relay tunnel after repeated dial failures. Returns
    /// immediately; the connect happens in the background. A `WsEvent::Opened`
    /// arrives once the handshake completes; failures surface as
    /// `WsEvent::Error`/`WsEvent::Closed` on the existing receive loop.
    ///
    /// `target_addr` should be `"<ip>:<port>"`. `connection_string` is what
    /// the client expects in the handshake's connection_string field — we
    /// send it back verbatim.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_tcp(target_addr: String, connection_string: String) -> Self {
        Self::spawn_session(Some(target_addr), connection_string)
    }

    /// Spawn a session that runs over the relay tunnel from the start, never
    /// attempting direct TCP. Used when the client advertises no reachable
    /// TCP coords (or a probe already proved TCP unreachable).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_tunnel(connection_string: String) -> Self {
        Self::spawn_session(None, connection_string)
    }

    /// Shared background-session spawn for [`from_tcp`]/[`from_tunnel`].
    /// `initial_target` `Some` starts in TCP mode; `None` starts in tunnel
    /// mode.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_session(initial_target: Option<String>, connection_string: String) -> Self {
        use crossbeam::channel::unbounded;

        let (out_tx, out_rx) = unbounded::<TcpFrame>();
        let (in_tx, in_rx) = unbounded::<WsEvent>();

        // Shared shutdown signal — `close()` flips it from the UI thread;
        // `run_session` polls it at every retry sleep and before every dial
        // attempt, so manual disconnect stops the retry loop even mid-dial.
        let shutdown = Arc::new(AtomicBool::new(false));
        let relaunch_grace_until_ms = Arc::new(AtomicU64::new(0));
        let tunnel_active = Arc::new(AtomicBool::new(initial_target.is_none()));

        let shutdown_for_session = shutdown.clone();
        let relaunch_grace_for_session = relaunch_grace_until_ms.clone();
        let tunnel_active_for_session = tunnel_active.clone();

        // Use the `displays` crate's existing PlatformSpawner abstraction
        // so we don't have to assume a specific runtime is available.
        crate::PlatformSpawner::spawn(async move {
            run_session(
                initial_target,
                connection_string,
                out_rx,
                in_tx,
                shutdown_for_session,
                relaunch_grace_for_session,
                tunnel_active_for_session,
            )
            .await;
        });

        Self {
            inner: AdminTransportInner::Tcp {
                out_tx,
                in_rx,
                closed: false,
                shutdown,
                relaunch_grace_until_ms,
                tunnel_active,
            },
        }
    }

    /// Extend ping/connect deadlines while a remote client relaunches after
    /// self-update. `grace_secs` is added to the current wall clock.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn signal_relaunch_pending(&mut self, grace_secs: u64) {
        if let AdminTransportInner::Tcp {
            relaunch_grace_until_ms,
            ..
        } = &mut self.inner
        {
            let until = now_millis().saturating_add(grace_secs.saturating_mul(1000));
            relaunch_grace_until_ms.store(until, Ordering::Relaxed);
            log::info!(
                "admin_transport -> relaunch grace active for {grace_secs}s \
                 (until epoch_ms={until})"
            );
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn signal_relaunch_pending(&mut self, _grace_secs: u64) {}

    pub fn kind(&self) -> TransportKind {
        match &self.inner {
            AdminTransportInner::WebSocket { .. } => TransportKind::WebSocket,
            AdminTransportInner::Tcp { tunnel_active, .. } => {
                if tunnel_active.load(Ordering::Relaxed) {
                    TransportKind::Relay
                } else {
                    TransportKind::Tcp
                }
            }
        }
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
            AdminTransportInner::Tcp {
                out_tx,
                closed,
                shutdown,
                ..
            } => {
                // Set the shared shutdown atomic FIRST so the dial-retry
                // loop in `run_session` sees it on its next sleep
                // poll (≤200ms).  Without this, a Disconnect issued
                // while the session is in the retry-dial state (peer
                // unreachable) would do nothing — the writer task
                // doesn't exist yet, so `TcpFrame::Shutdown` would sit
                // in the queue forever.
                shutdown.store(true, Ordering::Relaxed);
                // Still send Shutdown so an *active* writer task tears
                // down cleanly (graceful FIN) instead of the retry loop
                // observing the atomic.
                let _ = out_tx.send(TcpFrame::Shutdown);
                *closed = true;
            }
        }
    }
}

/// Background driver for a single admin↔client session. Runs in one of two
/// modes and retries indefinitely until `AdminTransport::close()` flips
/// `shutdown`.
///
/// - TCP mode (`initial_target` = `Some`): dials the advertised address,
///   refreshing it from the DB each round. After two consecutive dial
///   failures the session permanently switches to Tunnel mode.
/// - Tunnel mode (`initial_target` = `None`, or entered by fallback): each
///   attempt mints a fresh session id, dials the relay `/tunnel` route as
///   master, then pushes an `OpenRelayTunnel` control message so the client
///   dials the same tunnel.
///
/// `shutdown` is honored at every retry boundary so a UI-initiated disconnect
/// breaks out within ≤200 ms even mid-dial. `tunnel_active` is shared with the
/// transport handle so `kind()` reports the live path.
#[cfg(not(target_arch = "wasm32"))]
async fn run_session(
    initial_target: Option<String>,
    connection_string: String,
    out_rx: XReceiver<TcpFrame>,
    in_tx: XSender<WsEvent>,
    shutdown: Arc<AtomicBool>,
    relaunch_grace_until_ms: Arc<AtomicU64>,
    tunnel_active: Arc<AtomicBool>,
) {
    use std::sync::Mutex;
    use tokio::net::TcpStream;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    const CONNECT_TIMEOUT_RELAUNCH: Duration = Duration::from_secs(10);
    const RETRY_INTERVAL: Duration = Duration::from_secs(3);
    const TUNNEL_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
    /// Per-frame read idle timeout. With pings at 15 s, a healthy session
    /// always sees a pong inside this window. If 45 s elapses with nothing
    /// inbound, treat the connection as dead and let the outer loop retry.
    const READ_IDLE: Duration = Duration::from_secs(45);
    const READ_IDLE_RELAUNCH: Duration = Duration::from_secs(120);
    /// Consecutive TCP dial failures before permanently switching to tunnel.
    const TCP_FAILURE_FALLBACK: u32 = 2;

    let master_base = if cfg!(debug_assertions) {
        database::WS_MASTER_URL_LOCAL
    } else {
        database::WS_MASTER_URL
    };

    // Wrap the receiver so the writer task can borrow it each connection
    // without taking ownership, allowing reconnect on drop+redial.
    let out_rx = Arc::new(Mutex::new(out_rx));
    let mut tunnel_mode = initial_target.is_none();
    let mut target_addr = initial_target.unwrap_or_default();
    // One toast per failure streak; reset after a successful dial.
    let mut connect_failure_toasted = false;
    let mut consecutive_failures: u32 = 0;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = in_tx.send(WsEvent::Closed);
            return;
        }

        let relaunch_grace_active = in_relaunch_grace(&relaunch_grace_until_ms);
        let read_idle = if relaunch_grace_active {
            READ_IDLE_RELAUNCH
        } else {
            READ_IDLE
        };

        if !tunnel_mode {
            // ---- TCP mode: acquire read/write halves ----
            if let Some(fresh) = fetch_tcp_target(&connection_string).await {
                if fresh != target_addr {
                    log::info!("admin_transport -> refreshed dial target {target_addr} -> {fresh}");
                    target_addr = fresh;
                    connect_failure_toasted = false;
                }
            }
            let connect_timeout = if relaunch_grace_active {
                CONNECT_TIMEOUT_RELAUNCH
            } else {
                CONNECT_TIMEOUT
            };
            log::info!(
                "admin_transport -> dialing {target_addr} (relaunch_grace={relaunch_grace_active})"
            );

            let dial_result = match tokio::time::timeout(connect_timeout, TcpStream::connect(&target_addr)).await {
                Ok(Ok(s)) => Ok(s),
                Ok(Err(e)) => Err(format!("TCP connect to {target_addr} failed: {e}")),
                Err(_) => Err(format!("TCP connect to {target_addr} timed out")),
            };
            let stream = match dial_result {
                Ok(s) => s,
                Err(msg) => {
                    consecutive_failures += 1;
                    log::warn!("admin_transport -> {msg} (failure {consecutive_failures})");
                    let _ = in_tx.send(WsEvent::Error(format!("{msg} (retrying…)")));
                    if !connect_failure_toasted {
                        connect_failure_toasted = true;
                        let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(msg.clone()));
                    }
                    if consecutive_failures >= TCP_FAILURE_FALLBACK {
                        tunnel_mode = true;
                        tunnel_active.store(true, Ordering::Relaxed);
                        consecutive_failures = 0;
                        connect_failure_toasted = false;
                        let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(
                            format!("TCP unreachable — switching to relay tunnel for {connection_string}"),
                        ));
                        continue;
                    }
                    if shutdown_aware_sleep(&shutdown, RETRY_INTERVAL).await {
                        let _ = in_tx.send(WsEvent::Closed);
                        return;
                    }
                    continue;
                }
            };
            if let Err(e) = tcp_protocol::apply_tcp_options(&stream) {
                log::warn!("admin_transport -> apply_tcp_options failed: {e}");
            }
            connect_failure_toasted = false;
            consecutive_failures = 0;
            let (read_half, write_half) = stream.into_split();
            run_connection_phase(
                read_half,
                write_half,
                &connection_string,
                &out_rx,
                &in_tx,
                &shutdown,
                &relaunch_grace_until_ms,
                read_idle,
            )
            .await;
        } else {
            // ---- Tunnel mode: acquire read/write halves ----
            use tcp_protocol::tunnel::{connect_tunnel, derive_tunnel_url, send_oneshot_ws_binary, TUNNEL_ROLE_MASTER};

            let session_id = uuid::Uuid::new_v4().to_string();
            let master_url = derive_tunnel_url(master_base, &session_id, TUNNEL_ROLE_MASTER);
            log::info!("admin_transport -> tunnel dial {master_url} for {connection_string}");

            let tunnel = match tokio::time::timeout(TUNNEL_CONNECT_TIMEOUT, connect_tunnel(&master_url)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    log::warn!("admin_transport -> tunnel connect failed: {e}");
                    let _ = in_tx.send(WsEvent::Error(format!("relay tunnel connect failed: {e} (retrying…)")));
                    if shutdown_aware_sleep(&shutdown, RETRY_INTERVAL).await {
                        let _ = in_tx.send(WsEvent::Closed);
                        return;
                    }
                    continue;
                }
                Err(_) => {
                    log::warn!("admin_transport -> tunnel connect timed out");
                    let _ = in_tx.send(WsEvent::Error("relay tunnel connect timed out (retrying…)".to_string()));
                    if shutdown_aware_sleep(&shutdown, RETRY_INTERVAL).await {
                        let _ = in_tx.send(WsEvent::Closed);
                        return;
                    }
                    continue;
                }
            };

            // Tell the client (via the always-on room control route) to dial
            // the same tunnel as `role=client`.
            let control_url = database::websocket_url_with_room(master_base, &connection_string, "control");
            let ctrl = super::serialize_command(&crate::Cmd::OpenRelayTunnel { session_id });
            if let Err(e) = send_oneshot_ws_binary(&control_url, ctrl).await {
                log::warn!("admin_transport -> tunnel control send failed: {e}");
                let _ = in_tx.send(WsEvent::Error(format!("relay control send failed: {e} (retrying…)")));
                if shutdown_aware_sleep(&shutdown, RETRY_INTERVAL).await {
                    let _ = in_tx.send(WsEvent::Closed);
                    return;
                }
                continue;
            }

            let (read_half, write_half) = tokio::io::split(tunnel);
            run_connection_phase(
                read_half,
                write_half,
                &connection_string,
                &out_rx,
                &in_tx,
                &shutdown,
                &relaunch_grace_until_ms,
                read_idle,
            )
            .await;
        }

        if shutdown.load(Ordering::Relaxed) {
            let _ = in_tx.send(WsEvent::Closed);
            return;
        }

        log::info!("admin_transport -> session ended; reconnecting in {RETRY_INTERVAL:?}");
        let _ = in_tx.send(WsEvent::Error("peer disconnected (reconnecting…)".to_string()));
        if shutdown_aware_sleep(&shutdown, RETRY_INTERVAL).await {
            let _ = in_tx.send(WsEvent::Closed);
            return;
        }
    }
}

/// Per-connection engine shared by both transport modes: handshake, shape-fp
/// exchange, ping ticker, writer task, and reader loop over an already-split
/// byte pipe. Returns when the connection ends (peer closed, read/write error,
/// ping timeout, or shutdown); the caller decides whether to retry.
#[cfg(not(target_arch = "wasm32"))]
async fn run_connection_phase<R, W>(
    mut read_half: R,
    mut write_half: W,
    connection_string: &str,
    out_rx: &Arc<std::sync::Mutex<XReceiver<TcpFrame>>>,
    in_tx: &XSender<WsEvent>,
    shutdown: &Arc<AtomicBool>,
    relaunch_grace_until_ms: &Arc<AtomicU64>,
    read_idle: Duration,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// How often the master probes the agent's liveness.
    const PING_INTERVAL: Duration = Duration::from_secs(15);
    /// Deadline from "ping sent" to "pong received" before we declare the
    /// session dead. 30 s = 2× the ping interval, so a single dropped
    /// packet doesn't yank a healthy session.
    const PONG_DEADLINE_MS: u64 = 30_000;
    const PONG_DEADLINE_RELAUNCH_MS: u64 = 120_000;

    // ---- Handshake (admin sends) ----
    let id_bytes = connection_string.as_bytes();
    let mut hs = Vec::with_capacity(4 + 1 + 4 + id_bytes.len());
    hs.extend_from_slice(HANDSHAKE_MAGIC);
    hs.push(HANDSHAKE_VERSION);
    hs.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
    hs.extend_from_slice(id_bytes);

    if let Err(e) = write_half.write_all(&hs).await {
        log::warn!("admin_transport -> handshake write failed: {e}");
        let _ = in_tx.send(WsEvent::Error(format!("handshake write failed: {e} (retrying…)")));
        return;
    }

    log::info!("admin_transport -> connected + handshake sent for {connection_string}");
    let _ = in_tx.send(WsEvent::Opened);

    // Send our Cmd shape fingerprint before the writer task takes the write half.
    let fp_payload = tcp_protocol::encode_shape_fp(
        tcp_protocol::SHAPE_FP_KIND_ADMIN,
        *crate::shape_fp::CMD_SHAPE_FP,
        crate::shape_fp::BUILD_VERSION,
    );
    if let Err(e) = write_frame(&mut write_half, FRAME_TAG_SHAPE_FP, &fp_payload).await {
        log::warn!("admin_transport -> shape-fp write failed: {e}");
        let _ = in_tx.send(WsEvent::Error(format!("shape-fp write failed: {e} (retrying…)")));
        return;
    }

    // Liveness state shared between the reader (stamps on every Pong)
    // and the ping ticker (checks deadline). Initialized to `now` so
    // the first 30 s after a fresh dial don't immediately trip.
    let last_pong_at = Arc::new(AtomicU64::new(now_millis()));

    // ---- Ping ticker → writer channel ----
    //
    // A *separate* mpsc keeps pings from being starved by a slow
    // command stream filling `out_rx`. Capacity 4 is plenty: a healthy
    // writer drains it instantly; if we ever queue 4 unsent pings the
    // socket is already dead and the deadline check is about to fire.
    let (ping_tx, mut ping_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    {
        let last_pong_at = last_pong_at.clone();
        let in_tx_pinger = in_tx.clone();
        let relaunch_grace = relaunch_grace_until_ms.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(PING_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Skip the immediate tick — first ping fires PING_INTERVAL
            // after handshake so we don't race the writer-task spawn.
            tick.tick().await;
            let mut seq: u64 = 0;
            loop {
                tick.tick().await;
                seq += 1;
                let payload = tcp_protocol::encode_ping_payload(seq, now_millis());
                if ping_tx.send(payload.to_vec()).await.is_err() {
                    // Writer is gone — session is tearing down.
                    break;
                }
                // After the second ping, enforce the pong deadline.
                // Skipping the first one means a fresh session always
                // gets at least one round-trip before we'd time it out.
                if seq >= 2 {
                    let deadline_ms = if in_relaunch_grace(&relaunch_grace) {
                        PONG_DEADLINE_RELAUNCH_MS
                    } else {
                        PONG_DEADLINE_MS
                    };
                    let last = last_pong_at.load(Ordering::Relaxed);
                    if now_millis().saturating_sub(last) > deadline_ms {
                        log::warn!(
                            "admin_transport -> pong deadline exceeded \
                             (last pong {}ms ago, limit {deadline_ms}ms); declaring dead",
                            now_millis().saturating_sub(last)
                        );
                        let _ = in_tx_pinger.send(WsEvent::Error(
                            format!("ping timeout (no pong in {deadline_ms}ms, retrying…)"),
                        ));
                        break;
                    }
                }
            }
        });
    }

    // ---- Spawn writer task ----
    let shutdown_writer = shutdown.clone();
    let in_tx_writer = in_tx.clone();
    let out_rx_writer = out_rx.clone();
    let writer_handle = tokio::spawn(async move {
        // We can't await `out_rx.recv()` directly (crossbeam = blocking).
        // Persist a single in-flight `spawn_blocking` JoinHandle across
        // select! iterations so we don't churn worker threads — and so a
        // ping branch firing doesn't drop a partially-consumed user
        // frame on the floor. (Dropping the &mut JoinHandle does NOT
        // drop the JoinHandle itself; it stays in `pending` and resumes
        // next iteration.)
        let mut pending: Option<
            tokio::task::JoinHandle<Result<TcpFrame, crossbeam::channel::RecvError>>,
        > = None;
        loop {
            if pending.is_none() {
                let rx = out_rx_writer.clone();
                pending = Some(tokio::task::spawn_blocking(move || {
                    rx.lock().unwrap_or_else(|e| e.into_inner()).recv()
                }));
            }
            let pending_ref = pending.as_mut().expect("just set above");

            tokio::select! {
                // Bias pings so a saturated user-frame stream still
                // gets keepalive traffic out the door.
                biased;

                ping = ping_rx.recv() => {
                    let Some(payload) = ping else {
                        // Pinger task exited — either pong timeout or
                        // handshake-time bail. Drop the session so the
                        // outer loop redials.
                        log::info!("admin_transport -> ping channel closed; ending writer");
                        break;
                    };
                    if let Err(e) = write_frame(&mut write_half, FRAME_TAG_PING, &payload).await {
                        log::info!("admin_transport -> ping write error: {e}");
                        let _ = in_tx_writer.send(WsEvent::Error(format!("write ping: {e}")));
                        break;
                    }
                }

                user_result = pending_ref => {
                    pending = None;
                    let frame = match user_result {
                        Ok(Ok(f)) => f,
                        Ok(Err(_)) => {
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
                            if let Err(e) = write_frame(&mut write_half, FRAME_TAG_TEXT, s.as_bytes()).await {
                                log::info!("admin_transport -> writer error: {e}");
                                let _ = in_tx_writer.send(WsEvent::Error(format!("write: {e}")));
                                break;
                            }
                        }
                    }
                }
            }
        }
        let _ = write_half.shutdown().await;
    });

    // ---- Reader loop (this task) ----
    loop {
        // 45 s idle timeout: in a healthy session a pong arrives every
        // 15 s, so this only trips when the connection is truly silent
        // (peer crashed without a FIN, NAT path fell over without RST,
        // relay dropped an unpaired tunnel, etc.).
        let total_len = match tokio::time::timeout(read_idle, read_half.read_u32_le()).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    log::info!("admin_transport -> peer closed connection; will retry");
                } else {
                    log::warn!("admin_transport -> reader error: {e}; will retry");
                    let _ = in_tx.send(WsEvent::Error(format!("read: {e} (retrying…)")));
                }
                break;
            }
            Err(_) => {
                log::warn!(
                    "admin_transport -> read idle {read_idle:?} with no traffic; declaring dead"
                );
                let _ = in_tx.send(WsEvent::Error(
                    "read idle timeout (retrying…)".to_string(),
                ));
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
                // Any inbound data proves the agent is alive — stamp the
                // liveness clock so a long bulk transfer (which delays
                // pong echoes) can't trip the pong deadline on its own.
                last_pong_at.store(now_millis(), Ordering::Relaxed);
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
            FRAME_TAG_PONG => {
                // Agent's reply to our ping. Stamp `last_pong_at` —
                // do NOT forward to `in_tx`; pongs are not application
                // data. We don't currently use the sequence number,
                // but `decode_ping_payload` validates the 16-byte
                // shape so a malformed pong is dropped instead of
                // resetting our deadline counter.
                if tcp_protocol::decode_ping_payload(&payload).is_some() {
                    last_pong_at.store(now_millis(), Ordering::Relaxed);
                } else {
                    log::warn!(
                        "admin_transport -> malformed pong payload ({} bytes); ignoring",
                        payload.len()
                    );
                }
            }
            FRAME_TAG_PING => {
                // In v2 the master is the ping initiator; the agent
                // never sends pings. Log and drop — this is a future
                // role-reversal hook, not a current path.
                log::debug!("admin_transport -> unexpected ping from agent; ignoring");
                let _ = payload;
            }
            FRAME_TAG_SHAPE_FP => {
                let local = *crate::shape_fp::CMD_SHAPE_FP;
                match tcp_protocol::decode_shape_fp(&payload) {
                    Some((_, peer_fp, peer_ver)) if peer_fp != local => {
                        let local_ver = crate::shape_fp::BUILD_VERSION;
                        let _ = in_tx.send(WsEvent::Message(WsMessage::Text(
                            format!(
                                "__SHAPE_FP_MISMATCH__|{peer_fp:#018x}|{peer_ver}|{local:#018x}|{local_ver}"
                            )
                            .into(),
                        )));
                    }
                    Some((_, _, peer_ver)) => {
                        log::debug!("admin_transport -> Cmd shape ok (agent ver={peer_ver})");
                    }
                    None => log::warn!("admin_transport -> malformed shape-fp frame"),
                }
            }
            other => {
                log::warn!("admin_transport -> unknown frame tag: 0x{other:02x}");
            }
        }
    }

    writer_handle.abort();
}

/// Sleeps up to `dur`, polling `shutdown` at 200 ms granularity.
/// Returns `true` if shutdown was observed (caller should send
/// `WsEvent::Closed` and exit); `false` if the full duration elapsed.
///
/// We don't use `tokio::select!` with a `Notify` here because the
/// notify would need to be plumbed all the way through the
/// `AdminTransport` -> `run_session` boundary; polling a shared
/// atomic is simpler and 200 ms feels instant to a UI operator.
#[cfg(not(target_arch = "wasm32"))]
async fn shutdown_aware_sleep(shutdown: &Arc<AtomicBool>, dur: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + dur;
    while tokio::time::Instant::now() < deadline {
        if shutdown.load(Ordering::Relaxed) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let step = remaining.min(Duration::from_millis(200));
        if step.is_zero() {
            break;
        }
        tokio::time::sleep(step).await;
    }
    shutdown.load(Ordering::Relaxed)
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_tcp_target(connection_string: &str) -> Option<String> {
    use database::db;

    let mut response = db()
        .query(
            "SELECT local_ip, tcp_port FROM connected_client \
             WHERE connection_string = $cs LIMIT 1",
        )
        .bind(("cs", connection_string.to_string()))
        .await
        .ok()?;
    let rows: Vec<serde_json::Value> = response.take(0).ok()?;
    let row = rows.into_iter().next()?;
    let ip = row.get("local_ip")?.as_str()?.to_string();
    if ip.is_empty() {
        return None;
    }
    let port = row.get("tcp_port")?.as_u64()? as u16;
    Some(format!("{ip}:{port}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn in_relaunch_grace(relaunch_grace_until_ms: &AtomicU64) -> bool {
    now_millis() < relaunch_grace_until_ms.load(Ordering::Relaxed)
}

/// Wall-clock epoch milliseconds. Wraps `SystemTime` so callers don't have
/// to spell out the unwrap chain. Returns 0 on the (impossible) clock-rewind
/// case rather than panicking; a stuck `last_pong_at` is preferable to
/// crashing the transport.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}


/// Build the whole frame (`[u32 LE total_len][u8 tag][payload]`) into one
/// buffer and write it with a single `write_all` — over the tunnel each
/// `write_all` becomes one WebSocket message, so one logical frame stays one
/// message.
#[cfg(not(target_arch = "wasm32"))]
async fn write_frame<W>(write_half: &mut W, tag: u8, payload: &[u8]) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let total_len: u32 = (payload.len() as u32).saturating_add(1);
    let mut buf = Vec::with_capacity(4 + 1 + payload.len());
    buf.extend_from_slice(&total_len.to_le_bytes());
    buf.push(tag);
    buf.extend_from_slice(payload);
    write_half.write_all(&buf).await
}

/// High-level transport event for non-egui consumers (the Dioxus mobile app)
/// that drive an [`AdminTransport`] directly without the full
/// [`WebSocketClient`](super::WebSocketClient). Binary frames that decode as a
/// [`Cmd`](crate::Cmd) are delivered as [`SessionEvent::Cmd`]; everything else
/// passes through raw.
#[derive(Debug)]
pub enum SessionEvent {
    Opened,
    Closed,
    Error(String),
    Cmd(crate::Cmd),
    Text(String),
    Binary(Vec<u8>),
}

impl AdminTransport {
    /// Dial a client the way the admin console's `open_session` does: on
    /// native, direct TCP (with automatic relay-tunnel fallback) when
    /// `local_ip`+`tcp_port` are advertised, else straight to the relay
    /// tunnel. On wasm, the browser relay room is the only path.
    pub fn dial(client: &database::schema::ConnectedClient) -> Option<AdminTransport> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let (Some(ip), Some(port)) = (client.local_ip.as_deref(), client.tcp_port) {
                if !ip.is_empty() {
                    return Some(AdminTransport::from_tcp(
                        format!("{ip}:{port}"),
                        client.connection_string.clone(),
                    ));
                }
            }
            Some(AdminTransport::from_tunnel(client.connection_string.clone()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            AdminTransport::dial_relay(&client.connection_string)
        }
    }

    /// Dial the legacy WebSocket relay room for `connection_string` as
    /// `master`. Native sessions no longer use this path (they use direct TCP
    /// or the relay tunnel); it remains the wasm/browser transport.
    pub fn dial_relay(connection_string: &str) -> Option<AdminTransport> {
        let url = database::websocket_url_with_room(
            if cfg!(debug_assertions) {
                database::WS_MASTER_URL_LOCAL
            } else {
                database::WS_MASTER_URL
            },
            connection_string,
            "master",
        );
        match ewebsock::connect(&url, Default::default()) {
            Ok((sender, receiver)) => Some(AdminTransport::from_ws(sender, receiver)),
            Err(e) => {
                log::error!("AdminTransport::dial_relay -> {url:?}: {e}");
                None
            }
        }
    }

    /// Serialize and send a [`Cmd`](crate::Cmd) over this transport.
    pub fn send_cmd(&mut self, cmd: &crate::Cmd) {
        self.send(WsMessage::Binary(super::serialize_command(cmd)));
    }

    /// Poll one high-level [`SessionEvent`], decoding binary frames as `Cmd`
    /// when they parse exactly. Returns `None` when no event is queued.
    pub fn poll_event(&mut self) -> Option<SessionEvent> {
        match self.try_recv()? {
            WsEvent::Opened => Some(SessionEvent::Opened),
            WsEvent::Closed => Some(SessionEvent::Closed),
            WsEvent::Error(e) => Some(SessionEvent::Error(e)),
            WsEvent::Message(WsMessage::Binary(b)) => {
                let b: Vec<u8> = b.into();
                if super::is_zstd_frame(&b) {
                    return Some(SessionEvent::Binary(b));
                }
                match super::deserialize_command(&b) {
                    Some(cmd) => Some(SessionEvent::Cmd(cmd)),
                    None => Some(SessionEvent::Binary(b)),
                }
            }
            WsEvent::Message(WsMessage::Text(t)) => Some(SessionEvent::Text(t.into())),
            WsEvent::Message(_) => None,
        }
    }
}
