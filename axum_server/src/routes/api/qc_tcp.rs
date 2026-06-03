//! Plain-TCP QC fingerprint listener.
//!
//! A pre-OS UEFI agent can only do raw TCP4 (no TLS, no DNS, no HTTP client) —
//! so instead of POSTing over HTTPS it dials this listener on the LAN and pushes
//! one length-prefixed JSON frame. We reuse the `tcp_protocol` framing
//! (`[u32 LE total_len][u8 tag][payload]`, tag 0x02 = UTF-8) on the data plane;
//! the MTRX handshake is skipped (that's for the admin-dials-client direction).
//!
//! The frame is handed to [`super::qc_fleet::store_fingerprint`], the same code
//! path as the HTTP route, so the box lands in `qc_fingerprint` + `computer` and
//! shows up as a `connected_client` of kind `qc_agent`.

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEFAULT_PORT: u16 = 9201;
const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;
const FRAME_TAG_TEXT: u8 = 0x02;

/// Bind and serve the QC TCP listener forever. Spawn this from `main`.
pub async fn serve() {
    let port: u16 = std::env::var("QC_TCP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = format!("0.0.0.0:{port}");

    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("qc_tcp: bind {addr} failed: {e} (QC-over-TCP disabled)");
            return;
        }
    };
    tracing::info!("qc_tcp: listening on {addr}");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, peer).await {
                        tracing::warn!("qc_tcp: session from {peer} ended: {e}");
                    }
                });
            }
            Err(e) => tracing::warn!("qc_tcp: accept error: {e}"),
        }
    }
}

async fn handle(mut stream: TcpStream, peer: SocketAddr) -> std::io::Result<()> {
    // Read one frame: [u32 LE total_len][u8 tag][payload], total_len = 1 + body.
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let total = u32::from_le_bytes(len_buf);
    if total == 0 || total > MAX_FRAME_BYTES {
        return Err(std::io::Error::other("bad frame length"));
    }
    let mut frame = vec![0u8; total as usize];
    stream.read_exact(&mut frame).await?;
    let _tag = frame[0]; // tag; we accept any, body is JSON
    let body = &frame[1..];

    let resp = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => super::qc_fleet::store_fingerprint(v, Some(peer.ip().to_string())).await,
        Err(e) => serde_json::json!({ "status": "error", "error": format!("bad json: {e}") }),
    };

    // Reply with one framed JSON ack.
    let out_body = serde_json::to_vec(&resp).unwrap_or_default();
    let mut out = Vec::with_capacity(5 + out_body.len());
    out.extend_from_slice(&((1 + out_body.len()) as u32).to_le_bytes());
    out.push(FRAME_TAG_TEXT);
    out.extend_from_slice(&out_body);
    stream.write_all(&out).await?;
    stream.flush().await?;
    Ok(())
}
