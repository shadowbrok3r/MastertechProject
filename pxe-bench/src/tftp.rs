//! Read-only TFTP server (RFC 1350 + blksize/tsize options, RFC 2347/2348/2349).
//!
//! Serves iPXE binaries out of `tftp_root`. Each RRQ gets its own ephemeral
//! socket per the spec; writes are rejected.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::net::UdpSocket;

const OP_RRQ: u16 = 1;
const OP_WRQ: u16 = 2;
const OP_DATA: u16 = 3;
const OP_ACK: u16 = 4;
const OP_ERROR: u16 = 5;
const OP_OACK: u16 = 6;

const DEFAULT_BLKSIZE: usize = 512;
const MAX_RETRIES: u32 = 5;
const ACK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
struct Request {
    filename: String,
    blksize: usize,
    wants_tsize: bool,
}

fn parse_rrq(buf: &[u8]) -> Option<Request> {
    if buf.len() < 4 || u16::from_be_bytes([buf[0], buf[1]]) != OP_RRQ {
        return None;
    }
    let mut parts = buf[2..].split(|b| *b == 0).map(|s| String::from_utf8_lossy(s).into_owned());
    let filename = parts.next()?;
    let _mode = parts.next()?;
    let mut blksize = DEFAULT_BLKSIZE;
    let mut wants_tsize = false;
    let opts: Vec<String> = parts.collect();
    let mut i = 0;
    while i + 1 < opts.len() {
        match opts[i].to_ascii_lowercase().as_str() {
            "blksize" => {
                if let Ok(v) = opts[i + 1].parse::<usize>() {
                    blksize = v.clamp(8, 1468);
                }
            }
            "tsize" => wants_tsize = true,
            _ => {}
        }
        i += 2;
    }
    Some(Request {
        filename,
        blksize,
        wants_tsize,
    })
}

/// Resolve a request path inside the root, rejecting traversal.
fn resolve(root: &Path, filename: &str) -> Option<PathBuf> {
    let clean = filename.replace('\\', "/");
    if clean.contains("..") || clean.starts_with('/') {
        return None;
    }
    let path = root.join(clean);
    path.is_file().then_some(path)
}

async fn send_error(sock: &UdpSocket, dest: SocketAddr, code: u16, msg: &str) {
    let mut b = Vec::with_capacity(5 + msg.len());
    b.extend_from_slice(&OP_ERROR.to_be_bytes());
    b.extend_from_slice(&code.to_be_bytes());
    b.extend_from_slice(msg.as_bytes());
    b.push(0);
    let _ = sock.send_to(&b, dest).await;
}

async fn wait_ack(sock: &UdpSocket, expected_block: u16) -> bool {
    let mut buf = [0u8; 64];
    for _ in 0..MAX_RETRIES {
        match tokio::time::timeout(ACK_TIMEOUT, sock.recv(&mut buf)).await {
            Ok(Ok(n)) if n >= 4 => {
                let op = u16::from_be_bytes([buf[0], buf[1]]);
                let block = u16::from_be_bytes([buf[2], buf[3]]);
                if op == OP_ACK && block == expected_block {
                    return true;
                }
                if op == OP_ERROR {
                    return false;
                }
            }
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

async fn handle_rrq(root: PathBuf, req: Request, client: SocketAddr) -> anyhow::Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).await?;
    sock.connect(client).await?;

    let Some(path) = resolve(&root, &req.filename) else {
        send_error(&sock, client, 1, "file not found").await;
        log::warn!("tftp: {client} requested missing '{}'", req.filename);
        return Ok(());
    };
    let data = tokio::fs::read(&path).await?;
    log::info!(
        "tftp: {client} <- {} ({} bytes, blksize {})",
        req.filename,
        data.len(),
        req.blksize
    );

    // Negotiated options are confirmed with an OACK the client must ACK (block 0).
    if req.blksize != DEFAULT_BLKSIZE || req.wants_tsize {
        let mut oack = Vec::new();
        oack.extend_from_slice(&OP_OACK.to_be_bytes());
        if req.blksize != DEFAULT_BLKSIZE {
            oack.extend_from_slice(b"blksize\0");
            oack.extend_from_slice(format!("{}\0", req.blksize).as_bytes());
        }
        if req.wants_tsize {
            oack.extend_from_slice(b"tsize\0");
            oack.extend_from_slice(format!("{}\0", data.len()).as_bytes());
        }
        sock.send(&oack).await?;
        if !wait_ack(&sock, 0).await {
            return Ok(());
        }
    }

    let mut block: u16 = 1;
    let mut offset = 0usize;
    loop {
        let end = (offset + req.blksize).min(data.len());
        let mut packet = Vec::with_capacity(4 + end - offset);
        packet.extend_from_slice(&OP_DATA.to_be_bytes());
        packet.extend_from_slice(&block.to_be_bytes());
        packet.extend_from_slice(&data[offset..end]);

        let mut acked = false;
        for _ in 0..MAX_RETRIES {
            sock.send(&packet).await?;
            if wait_ack(&sock, block).await {
                acked = true;
                break;
            }
        }
        if !acked {
            log::warn!("tftp: {client} stalled at block {block}");
            return Ok(());
        }
        if end - offset < req.blksize {
            break;
        }
        offset = end;
        block = block.wrapping_add(1);
    }
    log::info!("tftp: {client} finished {}", req.filename);
    Ok(())
}

/// TFTP listener on :69.
pub async fn serve_tftp(root: PathBuf) -> anyhow::Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", 69)).await?;
    log::info!("TFTP listening on :69 (root {})", root.display());
    let mut buf = vec![0u8; 1500];
    loop {
        let (n, from) = sock.recv_from(&mut buf).await?;
        if n >= 2 && u16::from_be_bytes([buf[0], buf[1]]) == OP_WRQ {
            send_error(&sock, from, 2, "read-only server").await;
            continue;
        }
        let Some(req) = parse_rrq(&buf[..n]) else { continue };
        let root = root.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_rrq(root, req, from).await {
                log::warn!("tftp: transfer to {from} failed: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rrq_with_options() {
        let mut b = Vec::new();
        b.extend_from_slice(&OP_RRQ.to_be_bytes());
        b.extend_from_slice(b"ipxe.efi\0octet\0blksize\01428\0tsize\00\0");
        let req = parse_rrq(&b).unwrap();
        assert_eq!(req.filename, "ipxe.efi");
        assert_eq!(req.blksize, 1428);
        assert!(req.wants_tsize);
    }

    #[test]
    fn rejects_traversal() {
        let root = std::env::temp_dir();
        assert!(resolve(&root, "../secrets").is_none());
        assert!(resolve(&root, "/etc/passwd").is_none());
        assert!(resolve(&root, "..\\..\\windows\\system32\\config\\sam").is_none());
    }
}
