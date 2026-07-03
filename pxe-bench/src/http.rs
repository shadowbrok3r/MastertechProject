//! Minimal static HTTP server for the boot payload (wimboot, boot.ipxe, WinPE
//! media). GET/HEAD only, path-traversal-safe, supports the byte-Range form
//! wimboot uses for large .wim files.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn resolve(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let clean = uri_path.trim_start_matches('/').replace('\\', "/");
    if clean.contains("..") {
        return None;
    }
    let path = root.join(clean);
    path.is_file().then_some(path)
}

fn parse_range(headers: &str, len: u64) -> Option<(u64, u64)> {
    let line = headers
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))?;
    let spec = line.split(':').nth(1)?.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = match end.trim() {
        "" => len.saturating_sub(1),
        e => e.parse().ok()?,
    };
    (start <= end && end < len).then_some((start, end))
}

async fn handle(mut stream: TcpStream, root: PathBuf) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
    let mut lines = request.lines();
    let first = lines.next().unwrap_or_default().to_string();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let uri = parts.next().unwrap_or_default().to_string();

    if method != "GET" && method != "HEAD" {
        stream
            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }
    let Some(path) = resolve(&root, uri.split('?').next().unwrap_or(&uri)) else {
        stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")
            .await?;
        log::warn!("http: 404 {uri}");
        return Ok(());
    };

    let mut file = tokio::fs::File::open(&path).await?;
    let len = file.metadata().await?.len();
    let range = parse_range(&request, len);

    let (status, start, body_len) = match range {
        Some((s, e)) => ("206 Partial Content", s, e - s + 1),
        None => ("200 OK", 0, len),
    };
    let mut header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {body_len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n"
    );
    if let Some((s, e)) = range {
        header.push_str(&format!("Content-Range: bytes {s}-{e}/{len}\r\n"));
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes()).await?;

    if method == "HEAD" {
        return Ok(());
    }
    log::info!("http: {status} {uri} ({body_len} bytes)");
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let mut remaining = body_len;
    let mut chunk = vec![0u8; 256 * 1024];
    while remaining > 0 {
        let want = chunk.len().min(remaining as usize);
        let n = file.read(&mut chunk[..want]).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&chunk[..n]).await?;
        remaining -= n as u64;
    }
    Ok(())
}

/// HTTP listener for boot payloads.
pub async fn serve_http(root: PathBuf, port: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    log::info!("HTTP listening on :{port} (root {})", root.display());
    loop {
        let (stream, _from) = listener.accept().await?;
        let root = root.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, root).await {
                log::debug!("http: connection error: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range("Range: bytes=0-99\r\n", 1000), Some((0, 99)));
        assert_eq!(parse_range("range: bytes=500-\r\n", 1000), Some((500, 999)));
        assert_eq!(parse_range("Range: bytes=999-500\r\n", 1000), None);
        assert_eq!(parse_range("no range header", 1000), None);
    }

    #[test]
    fn traversal_rejected() {
        let root = std::env::temp_dir();
        assert!(resolve(&root, "/../secret").is_none());
    }
}
