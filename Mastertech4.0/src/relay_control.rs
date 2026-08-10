//! Always-on relay room connection for egui run mode.
//!
//! Terminal mode gets this from `start_websocket_sender`; egui mode had no
//! relay presence at all, so admins on another network could not reach it —
//! the relay had no `role=client` socket to forward `Cmd::OpenRelayTunnel` to
//! and every tunnel attempt expired unpaired. This holds that socket open and
//! dials a tunnel session on request; real sessions still run over direct TCP
//! or the tunnel, so no other `Cmd` is serviced here.

use displays::Cmd;
use ewebsock::{WsEvent, WsMessage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Bound at most once per process.
static STARTED: AtomicBool = AtomicBool::new(false);
/// Set to retire the channel (e.g. before handing the room to another agent
/// in this process). The loop exits at its next boundary and stays off.
static STOPPED: AtomicBool = AtomicBool::new(false);
/// Set once the room channel has opened at least once.
static OPENED_ONCE: AtomicBool = AtomicBool::new(false);
/// Last reported churn cause and when it warned; cleared by the next successful open.
static CHURN_REPORTED: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);
/// How long an unchanged churn cause stays demoted to debug. Without a ceiling a
/// relay that is down for the whole session warns once and then logs nothing,
/// which reads as a healthy channel.
const CHURN_REWARN_AFTER: Duration = Duration::from_secs(300);

/// Warn when the churn cause differs from the last reported one or the last
/// warning aged past [`CHURN_REWARN_AFTER`]; debug for repeats in between.
fn churn_level(cause: &str) -> log::Level {
    let mut last = CHURN_REPORTED.lock().unwrap_or_else(|e| e.into_inner());
    match last.as_ref() {
        Some((prev, at)) if prev == cause && at.elapsed() < CHURN_REWARN_AFTER => log::Level::Debug,
        _ => {
            *last = Some((cause.to_string(), std::time::Instant::now()));
            log::Level::Warn
        }
    }
}

/// Re-arms churn reporting so the next failure warns again.
fn clear_churn() {
    *CHURN_REPORTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Reconnect backoff bounds; the channel retries for the process lifetime.
const MIN_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Keepalive cadence and the inbound silence window that forces a redial.
/// Deliberately below the relay's own pong deadline (`WS_PONG_TIMEOUT_SECS`,
/// default 35 s): whoever gives up first should be the side that can redial,
/// so a stalled socket is re-registered instead of orphaned relay-side.
const PING_INTERVAL: Duration = Duration::from_secs(10);
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(25);
/// Window for the handshake to produce `WsEvent::Opened`. Separate from
/// [`LIVENESS_TIMEOUT`], which only describes an already-open socket: the relay
/// is fronted by Cloudflare, so a cold upgrade can outlast the silence window
/// and must not be scored as a stalled connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(45);
/// Hard socket lifetime. A relay that drops our room slot without closing the
/// socket leaves us auto-ponging a connection it no longer routes, which no
/// silence check can observe; recycling caps that blind window.
const MAX_SOCKET_AGE: Duration = Duration::from_secs(600);
/// Event-drain poll interval.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Spawn the relay room connection. Idempotent; safe to call every frame.
pub fn spawn_relay_control_channel() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        run().await;
    });
}

/// Retire the room channel so another agent in this process can own the room.
pub fn stop_relay_control_channel() {
    STOPPED.store(true, Ordering::SeqCst);
}

/// True once the channel has been retired or the process is shutting down.
fn should_stop() -> bool {
    STOPPED.load(Ordering::SeqCst) || displays::is_shutting_down()
}

/// Why one room socket ended.
enum SocketEnd {
    /// Retired at [`MAX_SOCKET_AGE`]; redial immediately to keep the gap short.
    Recycled,
    /// Died or never opened; `opened` gates the backoff reset.
    Ended { opened: bool },
}

async fn run() {
    let identity = match tokio::task::spawn_blocking(crate::filesystem::get_client_hash).await {
        Ok(id) => id,
        Err(e) => {
            log::error!("relay_control -> failed to compute client identity: {e}");
            return;
        }
    };
    let connection_string = identity.connection_string.clone();
    if connection_string.trim().is_empty() {
        log::error!("relay_control -> empty connection_string; not connecting");
        return;
    }

    let base = if cfg!(debug_assertions) {
        database::WS_CLIENT_URL_LOCAL
    } else {
        database::WS_CLIENT_URL
    };
    let url = database::websocket_url_with_room(base, &connection_string, "client");
    // Logged once per process: without a live room socket the relay has no
    // `role=client` peer to forward OpenRelayTunnel to, so every admin tunnel
    // attempt expires unpaired.
    log::info!("relay_control -> room channel for {connection_string} via {url}");

    let mut backoff = MIN_BACKOFF;
    loop {
        if should_stop() {
            return;
        }
        match ewebsock::connect(url.clone(), ewebsock::Options::default()) {
            Ok((sender, receiver)) => match serve_room_socket(sender, receiver).await {
                SocketEnd::Recycled => {
                    backoff = MIN_BACKOFF;
                    continue;
                }
                // A socket that opened proves the relay is reachable, so the
                // next failure starts its backoff from the floor.
                SocketEnd::Ended { opened: true } => backoff = MIN_BACKOFF,
                SocketEnd::Ended { opened: false } => {}
            },
            Err(e) => {
                let cause = format!("connect failed: {e}");
                log::log!(
                    churn_level(&cause),
                    "relay_control -> {cause}; retrying in {backoff:?}"
                );
            }
        }
        if should_stop() {
            return;
        }
        tokio::select! {
            biased;
            _ = displays::wait_for_shutdown() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Drains one room socket until it dies or is recycled.
async fn serve_room_socket(
    mut sender: ewebsock::WsSender,
    receiver: ewebsock::WsReceiver,
) -> SocketEnd {
    let mut opened = false;
    let started = tokio::time::Instant::now();
    let mut last_event = tokio::time::Instant::now();
    let mut last_ping = tokio::time::Instant::now();

    loop {
        if should_stop() {
            sender.close();
            return SocketEnd::Ended { opened };
        }

        while let Some(event) = receiver.try_recv() {
            last_event = tokio::time::Instant::now();
            match event {
                WsEvent::Opened => {
                    opened = true;
                    clear_churn();
                    if OPENED_ONCE.swap(true, Ordering::SeqCst) {
                        log::debug!("relay_control -> room channel open");
                    } else {
                        log::info!("relay_control -> room channel open");
                    }
                }
                WsEvent::Closed => {
                    log::debug!("relay_control -> room channel closed; will reconnect");
                    return SocketEnd::Ended { opened };
                }
                WsEvent::Error(e) => {
                    let cause = format!("room channel error: {e}");
                    log::log!(churn_level(&cause), "relay_control -> {cause}");
                    return SocketEnd::Ended { opened };
                }
                WsEvent::Message(WsMessage::Binary(bin)) => handle_binary(&bin),
                WsEvent::Message(_) => {}
            }
        }

        if opened && last_ping.elapsed() >= PING_INTERVAL {
            sender.send(WsMessage::Ping(Vec::new()));
            last_ping = tokio::time::Instant::now();
        }
        // An unopened socket is timed against the handshake window; the silence
        // window only describes a socket that reached `Opened` and went quiet.
        let (deadline, cause) = if opened {
            (LIVENESS_TIMEOUT, format!("no room traffic for {LIVENESS_TIMEOUT:?}"))
        } else {
            (
                CONNECT_TIMEOUT,
                format!("room channel never opened within {CONNECT_TIMEOUT:?}"),
            )
        };
        if last_event.elapsed() >= deadline {
            log::log!(churn_level(&cause), "relay_control -> {cause}; reconnecting");
            sender.close();
            return SocketEnd::Ended { opened };
        }
        if opened && started.elapsed() >= MAX_SOCKET_AGE {
            log::debug!("relay_control -> recycling room socket after {MAX_SOCKET_AGE:?}");
            sender.close();
            return SocketEnd::Recycled;
        }

        tokio::select! {
            biased;
            _ = displays::wait_for_shutdown() => {
                sender.close();
                return SocketEnd::Ended { opened };
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
    }
}

/// Services `OpenRelayTunnel` and ignores every other room-delivered `Cmd`.
fn handle_binary(bin: &[u8]) {
    match displays::try_deserialize_command(bin) {
        Some(Cmd::OpenRelayTunnel { session_id }) => {
            log::info!(
                "relay_control -> OpenRelayTunnel for session {}",
                &session_id[..session_id.len().min(8)]
            );
            crate::tunnel_session::spawn_tunnel_session(session_id);
        }
        Some(_) => {
            log::debug!("relay_control -> ignoring room Cmd; sessions run over TCP/tunnel")
        }
        None => log::debug!("relay_control -> undecodable room frame ({} bytes)", bin.len()),
    }
}
