//! Plain-UDP QC fingerprint listener.
//!
//! A pre-OS UEFI agent on firmware that won't instantiate its UEFI IPv4 stack
//! (no Tcp4/Http) can still send raw UDP over SimpleNetwork. It chunks the
//! fingerprint into datagrams `[b"MTUF"][u32 msg_id][u16 seq][u16 total][data]`;
//! this reassembles them and hands the JSON to
//! [`super::qc_fleet::store_fingerprint`] — the same path as the HTTP/TCP routes.

use std::collections::HashMap;
use std::net::IpAddr;

use tokio::net::UdpSocket;

const DEFAULT_PORT: u16 = 9202;
const HEADER: usize = 12;
const MAGIC: &[u8; 4] = b"MTUF";
const MAX_MSG_BYTES: usize = 4 * 1024 * 1024;
const MAX_PENDING: usize = 256;

struct Reassembly {
    total: u16,
    chunks: HashMap<u16, Vec<u8>>,
    bytes: usize,
}

/// Bind and serve the QC UDP listener forever. Spawn this from `main`.
pub async fn serve() {
    let port: u16 = std::env::var("QC_UDP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = format!("0.0.0.0:{port}");
    let sock = match UdpSocket::bind(&addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("qc_udp: bind {addr} failed: {e} (QC-over-UDP disabled)");
            return;
        }
    };
    tracing::info!("qc_udp: listening on {addr}");

    let mut pending: HashMap<(IpAddr, u32), Reassembly> = HashMap::new();
    let mut buf = vec![0u8; 2048];
    loop {
        let (n, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("qc_udp: recv error: {e}");
                continue;
            }
        };
        if n < HEADER || &buf[0..4] != MAGIC {
            continue;
        }
        let msg_id = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let seq = u16::from_be_bytes([buf[8], buf[9]]);
        let total = u16::from_be_bytes([buf[10], buf[11]]);
        if total == 0 {
            continue;
        }
        let data = buf[HEADER..n].to_vec();

        if pending.len() > MAX_PENDING {
            pending.clear();
        }
        let key = (peer.ip(), msg_id);
        let entry = pending
            .entry(key)
            .or_insert_with(|| Reassembly { total, chunks: HashMap::new(), bytes: 0 });
        entry.bytes += data.len();
        entry.chunks.insert(seq, data);
        let over = entry.bytes > MAX_MSG_BYTES;
        let complete = entry.chunks.len() as u16 >= entry.total;
        if over {
            pending.remove(&key);
            continue;
        }
        if !complete {
            continue;
        }

        let entry = pending.remove(&key).unwrap();
        let mut full = Vec::with_capacity(entry.bytes);
        for s in 0..entry.total {
            if let Some(c) = entry.chunks.get(&s) {
                full.extend_from_slice(c);
            }
        }
        match serde_json::from_slice::<serde_json::Value>(&full) {
            Ok(v) => {
                let resp = super::qc_fleet::store_fingerprint(v, Some(peer.ip().to_string())).await;
                tracing::info!(
                    "qc_udp: fingerprint from {peer}: {}",
                    resp.get("status").and_then(|s| s.as_str()).unwrap_or("?")
                );
            }
            Err(e) => tracing::warn!("qc_udp: bad json from {peer}: {e}"),
        }
    }
}
