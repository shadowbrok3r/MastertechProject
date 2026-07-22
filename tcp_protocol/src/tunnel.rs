//! Relay-tunnel transport: the direct-TCP wire protocol carried over a
//! WebSocket byte pipe.
//!
//! When the admin and client are on different networks, neither side can
//! dial the other directly. Both instead dial **out** to the relay's
//! `/tunnel` route over WSS (which traverses the Cloudflare tunnel), the
//! relay pairs the two sockets by one-time `session` id and forwards
//! binary frames verbatim. [`WsByteStream`] adapts the paired WebSocket
//! into `AsyncRead + AsyncWrite`, so the exact same MTRX handshake +
//! length-prefixed frame protocol (see crate docs) runs through it
//! unchanged — desktop streaming, file transfer, ping/pong liveness and
//! shape-fp exchange all behave identically to a direct TCP session.
//!
//! Mapping between WebSocket messages and the byte stream:
//! - `Binary` — carries stream bytes. Chunk boundaries are arbitrary and
//!   receivers must not assign meaning to them (the frame protocol is
//!   length-prefixed and self-delimiting).
//! - `Text` — relay control notes (`PAIRED`, `PEER_GONE`, error text).
//!   Logged, never surfaced as stream bytes.
//! - `Close` / stream end — EOF, equivalent to a TCP FIN.
//!
//! Write contract: `poll_write` only reports `Ready(Ok(n))` once the frame
//! has been flushed to the socket, so callers that never call `flush`
//! (both session writer tasks) cannot strand a tail in the sink buffer.
//! The adapter assumes one sequential writer (`write_all` loops), which is
//! how both session engines drive their write half.

use bytes::Bytes;
use futures::{Sink, SinkExt, Stream};
use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Query value for the admin side of a tunnel session.
pub const TUNNEL_ROLE_MASTER: &str = "master";
/// Query value for the agent side of a tunnel session.
pub const TUNNEL_ROLE_CLIENT: &str = "client";
/// Path of the relay's tunnel route.
pub const TUNNEL_PATH: &str = "/tunnel";

/// Derive the relay tunnel URL from a configured `/websocket` room URL.
///
/// `ws_base` is one of the baked `WS_*_URL` values (e.g.
/// `wss://sock.example.app/websocket`); any query string and trailing
/// slash are stripped, a trailing `/websocket` segment is replaced by
/// [`TUNNEL_PATH`], and the session/role pair is appended.
pub fn derive_tunnel_url(ws_base: &str, session_id: &str, role: &str) -> String {
    let no_query = ws_base.split('?').next().unwrap_or(ws_base);
    let trimmed = no_query.trim_end().trim_end_matches('/');
    let base = trimmed.strip_suffix("/websocket").unwrap_or(trimmed);
    format!("{base}{TUNNEL_PATH}?session={session_id}&role={role}")
}

/// The concrete stream type produced by [`connect_tunnel`].
pub type TunnelStream = WsByteStream<MaybeTlsStream<TcpStream>>;

/// Dial the relay tunnel at `url` (from [`derive_tunnel_url`]) and wrap it
/// as a byte stream. TLS roots come from webpki (rustls).
pub async fn connect_tunnel(url: &str) -> Result<TunnelStream, tungstenite::Error> {
    let (ws, _resp) = tokio_tungstenite::connect_async(url).await?;
    Ok(WsByteStream::new(ws))
}

/// Connect to `url`, deliver one binary message, flush, close.
///
/// Used to poke the client's room over `role=master`: the admin joins the
/// room, sends one serialized `Cmd` (which the relay forwards to the room's
/// client), then drops the connection.
pub async fn send_oneshot_ws_binary(url: &str, payload: Vec<u8>) -> Result<(), tungstenite::Error> {
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url).await?;
    ws.send(Message::Binary(payload.into())).await?;
    let _ = ws.close(None).await;
    Ok(())
}

/// `AsyncRead + AsyncWrite` over a tokio-tungstenite WebSocket.
pub struct WsByteStream<S> {
    ws: WebSocketStream<S>,
    /// Unconsumed remainder of the most recent binary message.
    read_chunk: Bytes,
    /// Length of a write accepted by the sink whose flush returned
    /// `Pending`; reported `Ready(Ok(len))` once the flush completes.
    pending_write: Option<usize>,
    read_eof: bool,
}

impl<S> WsByteStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(ws: WebSocketStream<S>) -> Self {
        Self {
            ws,
            read_chunk: Bytes::new(),
            pending_write: None,
            read_eof: false,
        }
    }
}

fn ws_err_to_io(e: tungstenite::Error) -> io::Error {
    match e {
        tungstenite::Error::Io(io_err) => io_err,
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            io::Error::new(io::ErrorKind::BrokenPipe, e)
        }
        other => io::Error::other(other),
    }
}

impl<S> AsyncRead for WsByteStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if !this.read_chunk.is_empty() {
                let n = buf.remaining().min(this.read_chunk.len());
                buf.put_slice(&this.read_chunk.split_to(n));
                return Poll::Ready(Ok(()));
            }
            if this.read_eof {
                // Zero bytes filled = EOF, same as a closed TCP socket.
                return Poll::Ready(Ok(()));
            }
            match ready!(Pin::new(&mut this.ws).poll_next(cx)) {
                Some(Ok(Message::Binary(b))) => {
                    if !b.is_empty() {
                        this.read_chunk = b;
                    }
                }
                Some(Ok(Message::Text(t))) => {
                    log::debug!("tunnel -> relay note: {t}");
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                Some(Ok(Message::Close(_))) | None => {
                    this.read_eof = true;
                }
                Some(Err(e)) => return Poll::Ready(Err(ws_err_to_io(e))),
            }
        }
    }
}

impl<S> AsyncWrite for WsByteStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        // A previous write was accepted by the sink but its flush returned
        // Pending. Per the AsyncWrite contract the caller retries with the
        // same buffer, so finishing the flush completes *that* write.
        if let Some(len) = this.pending_write {
            match Pin::new(&mut this.ws).poll_flush(cx) {
                Poll::Ready(Ok(())) => {
                    this.pending_write = None;
                    return Poll::Ready(Ok(len));
                }
                Poll::Ready(Err(e)) => {
                    this.pending_write = None;
                    return Poll::Ready(Err(ws_err_to_io(e)));
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        ready!(Pin::new(&mut this.ws).poll_ready(cx)).map_err(ws_err_to_io)?;
        Pin::new(&mut this.ws)
            .start_send(Message::Binary(Bytes::copy_from_slice(buf)))
            .map_err(ws_err_to_io)?;

        // Drive the flush before reporting success so a writer that never
        // calls flush (both session writer tasks) can't strand the frame in
        // the sink buffer.
        match Pin::new(&mut this.ws).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(ws_err_to_io(e))),
            Poll::Pending => {
                this.pending_write = Some(buf.len());
                Poll::Pending
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.ws).poll_flush(cx).map_err(ws_err_to_io)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.ws).poll_close(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            // Close on an already-closed socket is a successful shutdown.
            Poll::Ready(Err(
                tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed,
            )) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(ws_err_to_io(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::protocol::Role;

    #[test]
    fn tunnel_url_from_room_url() {
        assert_eq!(
            derive_tunnel_url("wss://sock.example.app/websocket", "abc", "master"),
            "wss://sock.example.app/tunnel?session=abc&role=master"
        );
        assert_eq!(
            derive_tunnel_url("wss://sock.example.app/websocket?x=1", "abc", "client"),
            "wss://sock.example.app/tunnel?session=abc&role=client"
        );
        assert_eq!(
            derive_tunnel_url("ws://127.0.0.1:8081/websocket/", "s", "client"),
            "ws://127.0.0.1:8081/tunnel?session=s&role=client"
        );
        // Base without the /websocket suffix still lands on /tunnel.
        assert_eq!(
            derive_tunnel_url("ws://127.0.0.1:8081", "s", "master"),
            "ws://127.0.0.1:8081/tunnel?session=s&role=master"
        );
    }

    async fn ws_pair() -> (
        WsByteStream<tokio::io::DuplexStream>,
        WsByteStream<tokio::io::DuplexStream>,
    ) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let client = WebSocketStream::from_raw_socket(a, Role::Client, None).await;
        let server = WebSocketStream::from_raw_socket(b, Role::Server, None).await;
        (WsByteStream::new(client), WsByteStream::new(server))
    }

    #[tokio::test]
    async fn round_trip_bytes() {
        let (mut a, mut b) = ws_pair().await;

        // Frame written as three separate write_all calls (len, tag,
        // payload) must arrive as one contiguous byte stream.
        let payload = vec![0xABu8; 100_000];
        let total_len = (payload.len() as u32) + 1;
        let writer = tokio::spawn(async move {
            a.write_all(&total_len.to_le_bytes()).await.unwrap();
            a.write_all(&[0x01]).await.unwrap();
            a.write_all(&payload).await.unwrap();
            a
        });

        let got_len = b.read_u32_le().await.unwrap();
        assert_eq!(got_len, total_len);
        let tag = b.read_u8().await.unwrap();
        assert_eq!(tag, 0x01);
        let mut got = vec![0u8; (got_len - 1) as usize];
        b.read_exact(&mut got).await.unwrap();
        assert!(got.iter().all(|&x| x == 0xAB));

        let mut a = writer.await.unwrap();

        // Reply in the other direction.
        b.write_all(b"pong").await.unwrap();
        let mut reply = [0u8; 4];
        a.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pong");
    }

    #[tokio::test]
    async fn peer_shutdown_reads_as_eof() {
        let (mut a, mut b) = ws_pair().await;
        a.write_all(b"hi").await.unwrap();
        a.shutdown().await.unwrap();

        let mut buf = [0u8; 2];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hi");

        let n = b.read(&mut buf).await.unwrap();
        assert_eq!(n, 0);
    }
}
