//! Relay-tunnel admin sessions.
//!
//! On `Cmd::OpenRelayTunnel` the client dials out to the relay's `/tunnel`
//! route over WSS and serves the direct-TCP wire protocol over the byte
//! stream, reaching admins that cannot dial the client's LAN listener.

use crate::tcp_listener::serve_admin_session;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tcp_protocol::tunnel::{connect_tunnel, derive_tunnel_url, TUNNEL_ROLE_CLIENT};

/// Ceiling on simultaneous relay-tunnel sessions.
const MAX_TUNNEL_SESSIONS: usize = 8;
/// Deadline for the outbound tunnel WebSocket dial.
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);

static ACTIVE_TUNNELS: AtomicUsize = AtomicUsize::new(0);

/// Releases a reserved concurrency slot on drop.
struct TunnelGuard;

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        ACTIVE_TUNNELS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// True when `session_id` is non-empty, ≤128 chars, and only `[A-Za-z0-9-]`.
fn session_id_valid(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Dials the relay tunnel for `session_id` and serves an admin session over
/// it. Spawns and returns immediately; failures log and drop with no retry.
pub fn spawn_tunnel_session(session_id: String) {
    if !session_id_valid(&session_id) {
        log::warn!("tunnel_session -> rejecting invalid session id");
        return;
    }

    let prev = ACTIVE_TUNNELS.fetch_add(1, Ordering::SeqCst);
    if prev >= MAX_TUNNEL_SESSIONS {
        ACTIVE_TUNNELS.fetch_sub(1, Ordering::SeqCst);
        log::warn!("tunnel_session -> at capacity ({MAX_TUNNEL_SESSIONS}); dropping session");
        return;
    }
    let guard = TunnelGuard;

    tokio::spawn(async move {
        let _guard = guard;
        let label = format!("tunnel:{}", &session_id[..session_id.len().min(8)]);

        let base = if cfg!(debug_assertions) {
            database::WS_CLIENT_URL_LOCAL
        } else {
            database::WS_CLIENT_URL
        };
        let url = derive_tunnel_url(base, &session_id, TUNNEL_ROLE_CLIENT);

        let stream = match tokio::time::timeout(DIAL_TIMEOUT, connect_tunnel(&url)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                log::warn!("tunnel_session -> {label} dial failed: {e}");
                return;
            }
            Err(_) => {
                log::warn!("tunnel_session -> {label} dial timed out after {DIAL_TIMEOUT:?}");
                return;
            }
        };

        log::info!("tunnel_session -> {label} connected; serving admin session");
        let (read_half, write_half) = tokio::io::split(stream);
        match serve_admin_session(read_half, write_half, label.clone()).await {
            Ok(()) => log::info!("tunnel_session -> {label} closed"),
            Err(e) => log::warn!("tunnel_session -> {label} ended: {e:#}"),
        }
    });
}
