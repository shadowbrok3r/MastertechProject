//! Relay-tunnel admin sessions.
//!
//! On `Cmd::OpenRelayTunnel` the client dials out to the relay's `/tunnel`
//! route over WSS and serves the direct-TCP wire protocol over the byte
//! stream, reaching admins that cannot dial the client's LAN listener.

use crate::tcp_listener::serve_admin_session;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tcp_protocol::tunnel::{connect_tunnel, derive_tunnel_url, TUNNEL_ROLE_CLIENT};
use tokio::io::{AsyncRead, ReadBuf};
use tokio::time::{Instant, Sleep};

/// Ceiling on simultaneous relay-tunnel sessions.
const MAX_TUNNEL_SESSIONS: usize = 8;
/// Deadline for the outbound tunnel WebSocket dial.
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
/// Inbound-silence deadline that tears down a half-open tunnel session.
const TUNNEL_READ_IDLE: Duration = Duration::from_secs(90);

static ACTIVE_TUNNELS: AtomicUsize = AtomicUsize::new(0);

/// Releases a reserved concurrency slot on drop.
struct TunnelGuard;

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        ACTIVE_TUNNELS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Wraps an `AsyncRead`, failing with `TimedOut` when no bytes arrive within
/// `idle` of the last successful read.
struct IdleTimeoutReader<R> {
    inner: R,
    idle: Duration,
    sleep: Pin<Box<Sleep>>,
}

impl<R> IdleTimeoutReader<R> {
    fn new(inner: R, idle: Duration) -> Self {
        Self {
            inner,
            idle,
            sleep: Box::pin(tokio::time::sleep(idle)),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for IdleTimeoutReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if buf.filled().len() > before {
                    let deadline = Instant::now() + this.idle;
                    this.sleep.as_mut().reset(deadline);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => match this.sleep.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "tunnel read idle timeout",
                ))),
                Poll::Pending => Poll::Pending,
            },
        }
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
        let read_half = IdleTimeoutReader::new(read_half, TUNNEL_READ_IDLE);
        match serve_admin_session(read_half, write_half, label.clone()).await {
            Ok(()) => log::info!("tunnel_session -> {label} closed"),
            Err(e) => log::warn!("tunnel_session -> {label} ended: {e:#}"),
        }
    });
}
