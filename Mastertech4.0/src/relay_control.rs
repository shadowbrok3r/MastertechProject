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
use std::time::Duration;

/// Bound at most once per process.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Reconnect backoff bounds; the channel retries for the process lifetime.
const MIN_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Keepalive cadence and the inbound silence window that forces a redial.
const PING_INTERVAL: Duration = Duration::from_secs(10);
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(45);
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
    log::info!("relay_control -> room channel for {connection_string} via {url}");

    let mut backoff = MIN_BACKOFF;
    loop {
        if displays::is_shutting_down() {
            return;
        }
        match ewebsock::connect(url.clone(), ewebsock::Options::default()) {
            Ok((sender, receiver)) => {
                if serve_room_socket(sender, receiver).await {
                    backoff = MIN_BACKOFF;
                }
            }
            Err(e) => log::warn!("relay_control -> connect failed: {e}; retrying in {backoff:?}"),
        }
        if displays::is_shutting_down() {
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

/// Drains one room socket until it dies. Returns `true` when the socket had
/// opened, so the caller resets its backoff.
async fn serve_room_socket(
    mut sender: ewebsock::WsSender,
    receiver: ewebsock::WsReceiver,
) -> bool {
    let mut opened = false;
    let mut last_event = tokio::time::Instant::now();
    let mut last_ping = tokio::time::Instant::now();

    loop {
        if displays::is_shutting_down() {
            sender.close();
            return opened;
        }

        while let Some(event) = receiver.try_recv() {
            last_event = tokio::time::Instant::now();
            match event {
                WsEvent::Opened => {
                    opened = true;
                    log::info!("relay_control -> room channel open");
                    // The relay writes connected = false when a client socket
                    // drops; re-assert it so the row matches reality.
                    crate::tcp_listener::upsert_self_identity(true).await;
                }
                WsEvent::Closed => {
                    log::info!("relay_control -> room channel closed; will reconnect");
                    return opened;
                }
                WsEvent::Error(e) => {
                    log::warn!("relay_control -> room channel error: {e}");
                    return opened;
                }
                WsEvent::Message(WsMessage::Binary(bin)) => handle_binary(&bin),
                WsEvent::Message(_) => {}
            }
        }

        if last_ping.elapsed() >= PING_INTERVAL {
            sender.send(WsMessage::Ping(Vec::new()));
            last_ping = tokio::time::Instant::now();
        }
        if last_event.elapsed() >= LIVENESS_TIMEOUT {
            log::warn!(
                "relay_control -> no room traffic for {LIVENESS_TIMEOUT:?}; reconnecting"
            );
            sender.close();
            return opened;
        }

        tokio::select! {
            biased;
            _ = displays::wait_for_shutdown() => {
                sender.close();
                return opened;
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
